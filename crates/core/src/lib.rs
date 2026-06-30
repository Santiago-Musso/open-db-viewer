use std::sync::Arc;
use dashmap::DashMap;
use driver_api::{ConnectionConfig, RelationalDriver, KeyValueDriver};
use driver_postgres::PostgresDriver;
use driver_redis::RedisDriver;

pub enum ActiveConnection {
    Relational(Arc<dyn RelationalDriver>),
    KeyValue(Arc<dyn KeyValueDriver>),
}

pub struct ConnectionManager {
    connections: DashMap<String, ActiveConnection>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: DashMap::new(),
        }
    }

    pub async fn connect(&self, driver_id: &str, config: &ConnectionConfig) -> Result<(), String> {
        match driver_id {
            "postgres" => {
                let driver = PostgresDriver::connect(config).await?;
                self.connections.insert(config.id.clone(), ActiveConnection::Relational(Arc::new(driver)));
                Ok(())
            }
            "redis" => {
                let driver = RedisDriver::connect(config).await?;
                self.connections.insert(config.id.clone(), ActiveConnection::KeyValue(Arc::new(driver)));
                Ok(())
            }
            _ => Err(format!("Unsupported driver: {}", driver_id)),
        }
    }

    pub async fn test_connection(&self, driver_id: &str, config: &ConnectionConfig) -> Result<(), String> {
        match driver_id {
            "postgres" => {
                let _driver = PostgresDriver::connect(config).await?;
                Ok(())
            }
            "redis" => {
                let _driver = RedisDriver::connect(config).await?;
                Ok(())
            }
            _ => Err(format!("Unsupported driver: {}", driver_id)),
        }
    }


    pub fn disconnect(&self, connection_id: &str) -> Result<(), String> {
        if self.connections.remove(connection_id).is_some() {
            Ok(())
        } else {
            Err("Connection not found".to_string())
        }
    }

    pub fn get_relational(&self, connection_id: &str) -> Result<Arc<dyn RelationalDriver>, String> {
        match self.connections.get(connection_id) {
            Some(conn) => match conn.value() {
                ActiveConnection::Relational(driver) => Ok(driver.clone()),
                _ => Err("Not a relational connection".to_string()),
            },
            None => Err("Connection not found".to_string()),
        }
    }

    pub fn get_key_value(&self, connection_id: &str) -> Result<Arc<dyn KeyValueDriver>, String> {
        match self.connections.get(connection_id) {
            Some(conn) => match conn.value() {
                ActiveConnection::KeyValue(driver) => Ok(driver.clone()),
                _ => Err("Not a key-value connection".to_string()),
            },
            None => Err("Connection not found".to_string()),
        }
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_manager_new() {
        let manager = ConnectionManager::new();
        assert_eq!(manager.connections.len(), 0);
    }

    #[test]
    fn test_disconnect_not_found() {
        let manager = ConnectionManager::new();
        let result = manager.disconnect("non_existent_id");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Connection not found");
    }

    #[test]
    fn test_get_relational_not_found() {
        let manager = ConnectionManager::new();
        let result = manager.get_relational("non_existent_id");
        assert!(result.is_err());
    }

    #[test]
    fn test_get_key_value_not_found() {
        let manager = ConnectionManager::new();
        let result = manager.get_key_value("non_existent_id");
        assert!(result.is_err());
    }
}
