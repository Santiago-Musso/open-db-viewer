use async_trait::async_trait;
use driver_api::{KeyValue, KeyValueDriver, ScanResult, ServerInfo};
use std::collections::HashMap;

pub struct RedisDriver {
    client: redis::Client,
}

impl RedisDriver {
    pub async fn connect(config: &driver_api::ConnectionConfig) -> Result<Self, String> {
        let conn_str = if let Some(password) = &config.password {
            if password.is_empty() {
                format!("redis://{}:{}/", config.host, config.port)
            } else {
                format!("redis://:{}@{}:{}/", password, config.host, config.port)
            }
        } else {
            format!("redis://{}:{}/", config.host, config.port)
        };

        let client = redis::Client::open(conn_str).map_err(|e| e.to_string())?;
        
        // Test connection immediately
        let mut conn = client.get_async_connection().await.map_err(|e| e.to_string())?;
        let _: String = redis::cmd("PING").query_async(&mut conn).await.map_err(|e| e.to_string())?;

        Ok(Self { client })
    }
}

#[async_trait]
impl KeyValueDriver for RedisDriver {
    async fn scan_keys(&self, pattern: &str, cursor: u64, count: usize) -> Result<ScanResult, String> {
        let mut conn = self.client.get_async_connection().await.map_err(|e| e.to_string())?;
        let mut cmd = redis::cmd("SCAN");
        cmd.arg(cursor);
        if !pattern.is_empty() {
            cmd.arg("MATCH").arg(pattern);
        }
        if count > 0 {
            cmd.arg("COUNT").arg(count);
        }

        let (next_cursor, keys): (u64, Vec<String>) = cmd.query_async(&mut conn).await.map_err(|e| e.to_string())?;

        Ok(ScanResult {
            cursor: next_cursor,
            keys,
        })
    }

    async fn get_key(&self, key: &str) -> Result<KeyValue, String> {
        let mut conn = self.client.get_async_connection().await.map_err(|e| e.to_string())?;
        let val_type: String = redis::cmd("TYPE").arg(key).query_async(&mut conn).await.map_err(|e| e.to_string())?;
        
        let ttl: i64 = redis::cmd("TTL").arg(key).query_async(&mut conn).await.map_err(|e| e.to_string())?;
        let ttl_opt = if ttl >= 0 { Some(ttl) } else { None };

        let value = match val_type.as_str() {
            "none" => return Err("Key does not exist".to_string()),
            "string" => {
                let val: String = redis::cmd("GET").arg(key).query_async(&mut conn).await.map_err(|e| e.to_string())?;
                val
            }
            "hash" => {
                let val: HashMap<String, String> = redis::cmd("HGETALL").arg(key).query_async(&mut conn).await.map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&val).unwrap_or_else(|_| "{}".to_string())
            }
            "list" => {
                let val: Vec<String> = redis::cmd("LRANGE").arg(key).arg(0).arg(-1).query_async(&mut conn).await.map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&val).unwrap_or_else(|_| "[]".to_string())
            }
            "set" => {
                let val: Vec<String> = redis::cmd("SMEMBERS").arg(key).query_async(&mut conn).await.map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&val).unwrap_or_else(|_| "[]".to_string())
            }
            "zset" => {
                let val: Vec<(String, f64)> = redis::cmd("ZRANGE").arg(key).arg(0).arg(-1).arg("WITHSCORES").query_async(&mut conn).await.map_err(|e| e.to_string())?;
                serde_json::to_string_pretty(&val).unwrap_or_else(|_| "[]".to_string())
            }
            _ => {
                format!("(Unsupported key type: {})", val_type)
            }
        };

        Ok(KeyValue {
            key: key.to_string(),
            value,
            value_type: val_type,
            ttl: ttl_opt,
        })
    }

    async fn set_key(&self, key: &str, value: &str, ttl: Option<i64>) -> Result<(), String> {
        let mut conn = self.client.get_async_connection().await.map_err(|e| e.to_string())?;
        
        let mut cmd = redis::cmd("SET");
        cmd.arg(key).arg(value);
        if let Some(t) = ttl {
            if t > 0 {
                cmd.arg("EX").arg(t);
            }
        }
        cmd.query_async::<_, ()>(&mut conn).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn delete_key(&self, key: &str) -> Result<(), String> {
        let mut conn = self.client.get_async_connection().await.map_err(|e| e.to_string())?;
        let _: i64 = redis::cmd("DEL").arg(key).query_async(&mut conn).await.map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn server_info(&self) -> Result<ServerInfo, String> {
        let mut conn = self.client.get_async_connection().await.map_err(|e| e.to_string())?;
        let info_str: String = redis::cmd("INFO").query_async(&mut conn).await.map_err(|e| e.to_string())?;
        
        let mut stats = HashMap::new();
        for line in info_str.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if let Some(pos) = line.find(':') {
                let (key, val) = line.split_at(pos);
                let val = &val[1..];
                stats.insert(key.trim().to_string(), val.trim().to_string());
            }
        }

        Ok(ServerInfo { stats })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_invalid_connection() {
        let config = ConnectionConfig {
            id: "test".to_string(),
            host: "invalid_host_123456789".to_string(),
            port: 6379,
            user: None,
            db_name: None,
            password: None,
        };
        let result = RedisDriver::connect(&config).await;
        assert!(result.is_err());
    }
}
