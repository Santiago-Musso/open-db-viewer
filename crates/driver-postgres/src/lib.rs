use async_trait::async_trait;
use driver_api::{
    ColumnInfo, ConnectionConfig, DataSource, ExecutionContext, RelationalDriver, RowBatch,
    SchemaEdge, SchemaGraph, SchemaInfo, SchemaNode, TableInfo, TableSchema,
};
use futures_util::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::NoTls;

pub mod connection;
pub mod types;

use connection::PostgresExecutionContext;

pub struct PostgresDriver {
    main_context: Arc<PostgresExecutionContext>,
    metadata_context: Arc<PostgresExecutionContext>,
    utility_context: Arc<PostgresExecutionContext>,
    _connection_tasks: Vec<tokio::task::JoinHandle<()>>,
    cancel_tokens: Arc<Mutex<HashMap<String, tokio_postgres::CancelToken>>>,
}

impl PostgresDriver {
    async fn connect_single(conn_str: &str) -> Result<(tokio_postgres::Client, tokio::task::JoinHandle<()>), String> {
        let (client, connection) = tokio_postgres::connect(conn_str, NoTls)
            .await
            .map_err(|e| e.to_string())?;

        let connection_task = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Postgres connection error: {}", e);
            }
        });
        Ok((client, connection_task))
    }

    pub async fn connect(config: &ConnectionConfig) -> Result<Self, String> {
        let mut conn_str = format!("host={} port={}", config.host, config.port);
        if let Some(user) = &config.user {
            if !user.is_empty() {
                conn_str.push_str(&format!(" user={}", user));
            }
        }
        if let Some(password) = &config.password {
            if !password.is_empty() {
                conn_str.push_str(&format!(" password={}", password));
            }
        }
        if let Some(db_name) = &config.db_name {
            if !db_name.is_empty() {
                conn_str.push_str(&format!(" dbname={}", db_name));
            }
        }

        // Connect three parallel contexts
        let (main_res, metadata_res, utility_res) = tokio::join!(
            Self::connect_single(&conn_str),
            Self::connect_single(&conn_str),
            Self::connect_single(&conn_str),
        );

        let (main_client, main_task) = main_res?;
        let (metadata_client, metadata_task) = metadata_res?;
        let (utility_client, utility_task) = utility_res?;

        let main_arc = Arc::new(main_client);
        let metadata_arc = Arc::new(metadata_client);
        let utility_arc = Arc::new(utility_client);

        let main_context = Arc::new(PostgresExecutionContext::new(main_arc).await?);
        let metadata_context = Arc::new(PostgresExecutionContext::new(metadata_arc).await?);
        let utility_context = Arc::new(PostgresExecutionContext::new(utility_arc).await?);

        Ok(Self {
            main_context,
            metadata_context,
            utility_context,
            _connection_tasks: vec![main_task, metadata_task, utility_task],
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl DataSource for PostgresDriver {
    async fn get_default_context(&self) -> Result<Arc<dyn ExecutionContext>, String> {
        Ok(self.main_context.clone())
    }

    async fn open_context(&self, purpose: &str) -> Result<Arc<dyn ExecutionContext>, String> {
        match purpose {
            "metadata" => Ok(self.metadata_context.clone()),
            "plan" | "utility" | "cancel" => Ok(self.utility_context.clone()),
            _ => Ok(self.main_context.clone()),
        }
    }

    async fn get_server_version(&self) -> Result<String, String> {
        let ctx = self.open_context("utility").await?;
        let session = ctx.open_session("utility").await?;
        let stmt = session.prepare_statement("SELECT version()").await?;
        let mut rs = stmt.execute_query().await?;
        if let Some(batch) = rs.next_row_batch(1).await? {
            if let Some(row) = batch.rows.first() {
                if let Some(serde_json::Value::String(version)) = row.first() {
                    return Ok(version.clone());
                }
            }
        }
        Err("Failed to query version".to_string())
    }
}

#[async_trait]
impl RelationalDriver for PostgresDriver {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, String> {
        let ctx = self.open_context("metadata").await?;
        let session = ctx.open_session("metadata").await?;
        let stmt = session
            .prepare_statement(
                "SELECT schema_name FROM information_schema.schemata \
                 WHERE schema_name NOT IN ('pg_catalog', 'information_schema') \
                 ORDER BY schema_name",
            )
            .await?;
        let mut rs = stmt.execute_query().await?;

        let mut schemas = Vec::new();
        while let Some(batch) = rs.next_row_batch(100).await? {
            for row in batch.rows {
                if let Some(serde_json::Value::String(name)) = row.first() {
                    schemas.push(SchemaInfo { name: name.clone() });
                }
            }
        }
        Ok(schemas)
    }

    async fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>, String> {
        let ctx = self.open_context("metadata").await?;
        let session = ctx.open_session("metadata").await?;
        let escaped_schema = schema.replace("'", "''");
        let sql = format!(
            "SELECT table_schema, table_name FROM information_schema.tables \
             WHERE table_schema = '{}' AND table_type = 'BASE TABLE' \
             ORDER BY table_name",
            escaped_schema
        );
        let stmt = session.prepare_statement(&sql).await?;
        let mut rs = stmt.execute_query().await?;

        let mut tables = Vec::new();
        while let Some(batch) = rs.next_row_batch(100).await? {
            for row in batch.rows {
                if row.len() >= 2 {
                    if let (
                        serde_json::Value::String(schema_val),
                        serde_json::Value::String(name_val),
                    ) = (&row[0], &row[1])
                    {
                        tables.push(TableInfo {
                            schema: schema_val.clone(),
                            name: name_val.clone(),
                        });
                    }
                }
            }
        }
        Ok(tables)
    }

    async fn describe_table(&self, schema: &str, table: &str) -> Result<TableSchema, String> {
        let ctx = self.open_context("metadata").await?;
        let session = ctx.open_session("metadata").await?;
        let escaped_schema = schema.replace("'", "''");
        let escaped_table = table.replace("'", "''");
        let sql = format!(
            "SELECT column_name, data_type FROM information_schema.columns \
             WHERE table_schema = '{}' AND table_name = '{}' \
             ORDER BY ordinal_position",
            escaped_schema, escaped_table
        );
        let stmt = session.prepare_statement(&sql).await?;
        let mut rs = stmt.execute_query().await?;

        let mut columns = Vec::new();
        while let Some(batch) = rs.next_row_batch(100).await? {
            for row in batch.rows {
                if row.len() >= 2 {
                    if let (
                        serde_json::Value::String(name_val),
                        serde_json::Value::String(data_type_val),
                    ) = (&row[0], &row[1])
                    {
                        columns.push(ColumnInfo {
                            name: name_val.clone(),
                            data_type: data_type_val.clone(),
                        });
                    }
                }
            }
        }
        Ok(TableSchema { columns })
    }

    async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, String> {
        let cols = self.describe_table(schema, table).await?;
        let mut ddl = format!("CREATE TABLE {}.{} (\n", schema, table);
        let col_definitions: Vec<String> = cols
            .columns
            .iter()
            .map(|c| format!("    {} {}", c.name, c.data_type.to_uppercase()))
            .collect();
        ddl.push_str(&col_definitions.join(",\n"));
        ddl.push_str("\n);");
        Ok(ddl)
    }

    async fn get_schema_graph(&self, schema: &str) -> Result<SchemaGraph, String> {
        let ctx = self.open_context("metadata").await?;
        let session = ctx.open_session("metadata").await?;
        let escaped_schema = schema.replace("'", "''");

        let col_sql = format!(
            "SELECT c.relname AS table_name, a.attname AS column_name, t.typname AS data_type \
             FROM pg_class c \
             JOIN pg_namespace n ON c.relnamespace = n.oid \
             JOIN pg_attribute a ON a.attrelid = c.oid \
             JOIN pg_type t ON a.atttypid = t.oid \
             WHERE n.nspname = '{}' \
               AND c.relkind = 'r' \
               AND a.attnum > 0 \
               AND NOT a.attisdropped \
             ORDER BY c.relname, a.attnum",
            escaped_schema
        );
        let col_stmt = session.prepare_statement(&col_sql).await?;
        let mut col_rs = col_stmt.execute_query().await?;

        let mut table_map: HashMap<String, Vec<ColumnInfo>> = HashMap::new();
        while let Some(batch) = col_rs.next_row_batch(100).await? {
            for row in batch.rows {
                if row.len() >= 3 {
                    if let (
                        serde_json::Value::String(table_name),
                        serde_json::Value::String(col_name),
                        serde_json::Value::String(data_type),
                    ) = (&row[0], &row[1], &row[2])
                    {
                        table_map
                            .entry(table_name.clone())
                            .or_default()
                            .push(ColumnInfo {
                                name: col_name.clone(),
                                data_type: data_type.clone(),
                            });
                    }
                }
            }
        }

        let nodes: Vec<SchemaNode> = table_map
            .into_iter()
            .map(|(name, columns)| SchemaNode {
                id: name.clone(),
                label: name,
                columns,
            })
            .collect();

        let fk_sql = format!(
            "SELECT \
                c.conname, \
                cl1.relname, \
                a1.attname, \
                cl2.relname, \
                a2.attname \
             FROM pg_constraint c \
             JOIN pg_class cl1 ON c.conrelid = cl1.oid \
             JOIN pg_class cl2 ON c.confrelid = cl2.oid \
             JOIN pg_namespace n1 ON cl1.relnamespace = n1.oid \
             JOIN pg_attribute a1 ON a1.attnum = ANY(c.conkey) AND a1.attrelid = cl1.oid \
             JOIN pg_attribute a2 ON a2.attnum = ANY(c.confkey) AND a2.attrelid = cl2.oid \
             WHERE c.contype = 'f' AND n1.nspname = '{}'",
            escaped_schema
        );
        let fk_stmt = session.prepare_statement(&fk_sql).await?;
        let mut fk_rs = fk_stmt.execute_query().await?;

        let mut edges = Vec::new();
        while let Some(batch) = fk_rs.next_row_batch(100).await? {
            for row in batch.rows {
                if row.len() >= 5 {
                    if let (
                        serde_json::Value::String(conname),
                        serde_json::Value::String(table1),
                        serde_json::Value::String(col1),
                        serde_json::Value::String(table2),
                        serde_json::Value::String(col2),
                    ) = (&row[0], &row[1], &row[2], &row[3], &row[4])
                    {
                        edges.push(SchemaEdge {
                            id: conname.clone(),
                            source: table1.clone(),
                            source_handle: col1.clone(),
                            target: table2.clone(),
                            target_handle: col2.clone(),
                        });
                    }
                }
            }
        }

        Ok(SchemaGraph { nodes, edges })
    }

    async fn execute_query_stream(
        &self,
        query_id: &str,
        sql: &str,
        batch_size: usize,
        offset: Option<usize>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<RowBatch, String>> + Send>>, String> {
        let cancel_token = self.main_context.cancel_token();
        self.cancel_tokens
            .lock()
            .await
            .insert(query_id.to_string(), cancel_token);

        let mut final_sql = sql.to_string();
        if let Some(off) = offset {
            let lower = sql.trim().to_lowercase();
            if lower.starts_with("select") || lower.starts_with("with") {
                final_sql = format!(
                    "SELECT * FROM ({}) AS _odv_wrapper LIMIT {} OFFSET {}",
                    sql, batch_size, off
                );
            }
        }

        let ctx = self.open_context("query").await?;
        let session = ctx.open_session("query").await?;
        let mut stmt = session.prepare_statement(&final_sql).await?;
        stmt.set_fetch_size(batch_size);

        let rs = stmt.execute_query().await?;

        let cancel_tokens_clone = self.cancel_tokens.clone();
        let query_id_clone = query_id.to_string();

        let stream = futures_util::stream::unfold(
            (rs, cancel_tokens_clone, query_id_clone, false),
            move |(mut rs, cancel_tokens, query_id, finished)| async move {
                if finished {
                    return None;
                }
                match rs.next_row_batch(batch_size).await {
                    Ok(Some(batch)) => Some((Ok(batch), (rs, cancel_tokens, query_id, false))),
                    Ok(None) => {
                        cancel_tokens.lock().await.remove(&query_id);
                        None
                    }
                    Err(e) => {
                        cancel_tokens.lock().await.remove(&query_id);
                        Some((Err(e), (rs, cancel_tokens, query_id, true)))
                    }
                }
            },
        );

        Ok(Box::pin(stream))
    }

    async fn cancel_query(&self, query_id: &str) -> Result<(), String> {
        let token_opt = self.cancel_tokens.lock().await.remove(query_id);
        if let Some(token) = token_opt {
            tokio::spawn(async move {
                let _ = token.cancel_query(NoTls).await;
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use driver_api::ConnectionConfig;

    #[tokio::test]
    async fn test_invalid_connection() {
        let config = ConnectionConfig {
            id: "test".to_string(),
            host: "invalid_host_123456789".to_string(),
            port: 5432,
            user: Some("postgres".to_string()),
            db_name: Some("test".to_string()),
            password: Some("postgres".to_string()),
        };
        let result = PostgresDriver::connect(&config).await;
        assert!(result.is_err());
    }
}

