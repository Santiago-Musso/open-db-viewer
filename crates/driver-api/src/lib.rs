use async_trait::async_trait;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::sync::Arc;

pub mod error;
pub use error::{DatabaseError, ErrorCategory};

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

pub trait SqlDialect: Send + Sync {
    fn quote_identifier(&self, ident: &str) -> String;
    fn escape_string_literal(&self, val: &str) -> String;
    fn get_type_cast_clause(&self, column_type: &str) -> Option<String>;
    fn transform_query_limit(&self, sql: &str, limit: usize, offset: Option<usize>) -> String;
}

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn get_default_context(&self) -> Result<Arc<dyn ExecutionContext>, DatabaseError>;
    async fn open_context(&self, purpose: &str) -> Result<Arc<dyn ExecutionContext>, DatabaseError>;
    async fn get_server_version(&self) -> Result<String, DatabaseError>;
    fn get_dialect(&self) -> Arc<dyn SqlDialect>;
}

#[async_trait]
pub trait ExecutionContext: Send + Sync {
    async fn get_active_schema(&self) -> Result<String, DatabaseError>;
    async fn set_active_schema(&self, schema: &str) -> Result<(), DatabaseError>;
    async fn get_search_path(&self) -> Result<Vec<String>, DatabaseError>;
    async fn open_session(&self, purpose: &str) -> Result<Box<dyn DbSession>, DatabaseError>;
}

#[async_trait]
pub trait DbSession: Send + Sync {
    async fn prepare_statement(&self, sql: &str) -> Result<Box<dyn DbStatement>, DatabaseError>;
}

#[async_trait]
pub trait DbStatement: Send + Sync {
    async fn execute_query(&self) -> Result<Box<dyn DbResultSet>, DatabaseError>;
    async fn execute_update(&self) -> Result<u64, DatabaseError>;
    fn set_fetch_size(&mut self, size: usize);
    fn set_query_timeout(&mut self, seconds: u32);
}

#[async_trait]
pub trait DbResultSet: Send + Sync {
    fn get_metadata(&self) -> Result<Vec<ColumnInfo>, DatabaseError>;
    async fn next_row_batch(&mut self, batch_size: usize) -> Result<Option<RowBatch>, DatabaseError>;
}

#[async_trait]
pub trait RelationalDriver: Send + Sync {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, DatabaseError>;
    async fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DatabaseError>;
    async fn describe_table(&self, schema: &str, table: &str) -> Result<TableSchema, DatabaseError>;
    async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, DatabaseError>;
    async fn get_schema_graph(&self, schema: &str) -> Result<SchemaGraph, DatabaseError>;
    async fn execute_query_stream(
        &self,
        query_id: &str,
        sql: &str,
        batch_size: usize,
        offset: Option<usize>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<RowBatch, DatabaseError>> + Send>>, DatabaseError>;
    async fn cancel_query(&self, query_id: &str) -> Result<(), DatabaseError>;
    async fn refresh_schema(&self, _schema: &str) -> Result<(), DatabaseError> {
        Ok(())
    }
    async fn refresh_table(&self, _schema: &str, _table: &str) -> Result<(), DatabaseError> {
        Ok(())
    }
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

