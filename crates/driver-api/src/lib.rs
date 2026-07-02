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

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Regular,
    Partitioned,
    Foreign,
    View,
    MaterializedView,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableStats {
    pub table_size: i64,
    pub total_size: i64,
    pub estimated_row_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub oid: u64,
    pub table_kind: TableKind,
    pub description: Option<String>,
    pub stats: Option<TableStats>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub type_oid: u32,
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
    pub match_type: Option<String>,  // FULL, PARTIAL, SIMPLE
    pub update_rule: Option<String>, // CASCADE, RESTRICT, SET_NULL, SET_DEFAULT, NO_ACTION
    pub delete_rule: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchemaGraph {
    pub nodes: Vec<SchemaNode>,
    pub edges: Vec<SchemaEdge>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IndexInfo {
    pub name: String,
    pub table_name: String,
    pub is_unique: bool,
    pub is_primary: bool,
    pub columns: Vec<String>,
    pub index_type: Option<String>, // btree, hash, gist, gin, etc.
    pub predicate: Option<String>,  // partial index expression
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintType {
    PrimaryKey,
    Unique,
    Check,
    Exclusion,
    ForeignKey,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConstraintInfo {
    pub name: String,
    pub table_name: String,
    pub constraint_type: ConstraintType,
    pub columns: Vec<String>,
    pub definition: String, // from pg_get_constraintdef
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SequenceInfo {
    pub name: String,
    pub schema: String,
    pub data_type: String,
    pub start_value: i64,
    pub min_value: i64,
    pub max_value: i64,
    pub increment: i64,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProcedureInfo {
    pub name: String,
    pub schema: String,
    pub argument_types: Vec<String>,
    pub return_type: String,
    pub definition: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtensionInfo {
    pub name: String,
    pub version: String,
    pub schema: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct ExplainOptions {
    pub analyze: bool,
    pub verbose: bool,
    pub costs: bool,
    pub buffers: bool,
    pub timing: bool,
    pub settings: bool,
}

#[async_trait]
pub trait TransactionManager: Send + Sync {
    async fn is_auto_commit(&self) -> Result<bool, DatabaseError>;
    async fn set_auto_commit(&self, enabled: bool) -> Result<(), DatabaseError>;
    async fn commit(&self) -> Result<(), DatabaseError>;
    async fn rollback(&self) -> Result<(), DatabaseError>;
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
    async fn open_context(&self, purpose: &str)
        -> Result<Arc<dyn ExecutionContext>, DatabaseError>;
    async fn get_server_version(&self) -> Result<String, DatabaseError>;
    fn get_dialect(&self) -> Arc<dyn SqlDialect>;
}

#[async_trait]
pub trait ExecutionContext: Send + Sync {
    async fn get_active_schema(&self) -> Result<String, DatabaseError>;
    async fn set_active_schema(&self, schema: &str) -> Result<(), DatabaseError>;
    async fn get_search_path(&self) -> Result<Vec<String>, DatabaseError>;
    async fn open_session(&self, purpose: &str) -> Result<Box<dyn DbSession>, DatabaseError>;
    fn transaction_manager(&self) -> Option<&dyn TransactionManager> {
        None
    }
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
    async fn next_row_batch(
        &mut self,
        batch_size: usize,
    ) -> Result<Option<RowBatch>, DatabaseError>;
}

#[async_trait]
pub trait RelationalDriver: Send + Sync {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, DatabaseError>;
    async fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DatabaseError>;
    async fn describe_table(&self, schema: &str, table: &str)
        -> Result<TableSchema, DatabaseError>;
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
    async fn get_execution_plan(
        &self,
        _sql: &str,
        _options: ExplainOptions,
    ) -> Result<PlanNode, DatabaseError> {
        Err(DatabaseError::new(
            "Explain plan is not supported by this driver".to_string(),
        ))
    }
    async fn list_active_sessions(
        &self,
        _show_idle: bool,
    ) -> Result<Vec<DbSessionInfo>, DatabaseError> {
        Err(DatabaseError::new(
            "Session management is not supported by this driver".to_string(),
        ))
    }
    async fn cancel_session(&self, _pid: i32) -> Result<(), DatabaseError> {
        Err(DatabaseError::new(
            "Session cancel is not supported by this driver".to_string(),
        ))
    }
    async fn terminate_session(&self, _pid: i32) -> Result<(), DatabaseError> {
        Err(DatabaseError::new(
            "Session termination is not supported by this driver".to_string(),
        ))
    }
    async fn list_indexes(
        &self,
        _schema: &str,
        _table: &str,
    ) -> Result<Vec<IndexInfo>, DatabaseError> {
        Ok(Vec::new())
    }
    async fn list_constraints(
        &self,
        _schema: &str,
        _table: &str,
    ) -> Result<Vec<ConstraintInfo>, DatabaseError> {
        Ok(Vec::new())
    }
    async fn list_sequences(&self, _schema: &str) -> Result<Vec<SequenceInfo>, DatabaseError> {
        Ok(Vec::new())
    }
    async fn list_procedures(&self, _schema: &str) -> Result<Vec<ProcedureInfo>, DatabaseError> {
        Ok(Vec::new())
    }
    async fn list_extensions(&self) -> Result<Vec<ExtensionInfo>, DatabaseError> {
        Ok(Vec::new())
    }
    async fn get_view_ddl(&self, _schema: &str, _view: &str) -> Result<String, DatabaseError> {
        Err(DatabaseError::new(
            "Views not supported by this driver".to_string(),
        ))
    }
    async fn refresh_all(&self) -> Result<(), DatabaseError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanNode {
    #[serde(rename = "Node Type")]
    pub node_type: String,
    #[serde(rename = "Relation Name")]
    pub relation_name: Option<String>,
    #[serde(rename = "Alias")]
    pub alias: Option<String>,
    #[serde(rename = "Startup Cost")]
    pub startup_cost: f64,
    #[serde(rename = "Total Cost")]
    pub total_cost: f64,
    #[serde(rename = "Plan Rows")]
    pub plan_rows: f64,
    #[serde(rename = "Plan Width")]
    pub plan_width: u64,
    #[serde(rename = "Actual Rows")]
    pub actual_rows: Option<f64>,
    #[serde(rename = "Actual Loops")]
    pub actual_loops: Option<u64>,
    #[serde(rename = "Plans")]
    pub plans: Option<Vec<PlanNode>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DbSessionInfo {
    pub pid: i32,
    pub username: Option<String>,
    pub query: Option<String>,
    pub state: Option<String>,
    pub query_start: Option<String>,
    pub client_addr: Option<String>,
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
