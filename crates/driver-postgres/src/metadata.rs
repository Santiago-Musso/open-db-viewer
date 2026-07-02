use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use tokio::sync::RwLock;

use driver_api::DatabaseError;

pub struct ObjectLookupCache<K, V> {
    pub cache: RwLock<HashMap<K, Arc<V>>>,
}

impl<K, V> ObjectLookupCache<K, V>
where
    K: Hash + Eq + Clone,
{
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_or_load<F, Fut>(&self, key: K, loader: F) -> Result<Arc<V>, DatabaseError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<V, DatabaseError>>,
    {
        // 1. Try to read with a read lock
        {
            let read = self.cache.read().await;
            if let Some(val) = read.get(&key) {
                return Ok(val.clone());
            }
        }

        // 2. Not found, get write lock
        let mut write = self.cache.write().await;
        // Double-check in case another thread populated it while we waited
        if let Some(val) = write.get(&key) {
            return Ok(val.clone());
        }

        // 3. Load the value and store it
        let val = loader().await?;
        let arc_val = Arc::new(val);
        write.insert(key, arc_val.clone());
        Ok(arc_val)
    }

    pub async fn invalidate(&self, key: &K) {
        let mut write = self.cache.write().await;
        write.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_cache_loading_and_invalidation() {
        let cache = ObjectLookupCache::new();
        let count = Arc::new(AtomicUsize::new(0));

        let count_clone = count.clone();
        let val = cache
            .get_or_load("key1".to_string(), || async move {
                count_clone.fetch_add(1, Ordering::SeqCst);
                Ok("value1".to_string())
            })
            .await
            .unwrap();

        assert_eq!(*val, "value1".to_string());
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Fetch again, should not run loader
        let count_clone = count.clone();
        let val2 = cache
            .get_or_load("key1".to_string(), || async move {
                count_clone.fetch_add(1, Ordering::SeqCst);
                Ok("value_other".to_string())
            })
            .await
            .unwrap();

        assert_eq!(*val2, "value1".to_string());
        assert_eq!(count.load(Ordering::SeqCst), 1);

        // Invalidate and fetch again, should run loader
        cache.invalidate(&"key1".to_string()).await;
        let count_clone = count.clone();
        let val3 = cache
            .get_or_load("key1".to_string(), || async move {
                count_clone.fetch_add(1, Ordering::SeqCst);
                Ok("value3".to_string())
            })
            .await
            .unwrap();

        assert_eq!(*val3, "value3".to_string());
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
