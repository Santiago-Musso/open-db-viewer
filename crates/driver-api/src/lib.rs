use async_trait::async_trait;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum DriverKind {
    Relational,
    KeyValue,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DriverManifest {
    pub id: String, // "postgres" | "redis"
    pub kind: DriverKind,
    pub display_name: String,
    pub default_port: u16,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectionConfig {
    pub id: String,
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub db_name: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchemaInfo {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableSchema {
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RowBatch {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<serde_json::Value>>,
}

// For ER diagrams
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchemaNode {
    pub id: String,
    pub label: String,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchemaEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub source_handle: String,
    pub target_handle: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchemaGraph {
    pub nodes: Vec<SchemaNode>,
    pub edges: Vec<SchemaEdge>,
}

#[async_trait]
pub trait RelationalDriver: Send + Sync {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, String>;
    async fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>, String>;
    async fn describe_table(&self, schema: &str, table: &str) -> Result<TableSchema, String>;
    async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, String>;
    async fn get_schema_graph(&self, schema: &str) -> Result<SchemaGraph, String>;
    async fn execute_query_stream(
        &self,
        query_id: &str,
        sql: &str,
        batch_size: usize,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<RowBatch, String>> + Send>>, String>;
    async fn cancel_query(&self, query_id: &str) -> Result<(), String>;
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScanResult {
    pub cursor: u64,
    pub keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct KeyValue {
    pub key: String,
    pub value: String,
    pub value_type: String,
    pub ttl: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerInfo {
    pub stats: std::collections::HashMap<String, String>,
}

#[async_trait]
pub trait KeyValueDriver: Send + Sync {
    async fn scan_keys(
        &self,
        pattern: &str,
        cursor: u64,
        count: usize,
    ) -> Result<ScanResult, String>;
    async fn get_key(&self, key: &str) -> Result<KeyValue, String>;
    async fn set_key(&self, key: &str, value: &str, ttl: Option<i64>) -> Result<(), String>;
    async fn delete_key(&self, key: &str) -> Result<(), String>;
    async fn server_info(&self) -> Result<ServerInfo, String>;
}
