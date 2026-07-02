use async_trait::async_trait;
use driver_api::{
    ColumnInfo, ConnectionConfig, DataSource, DbResultSet, DbSession, DbStatement,
    ExecutionContext, RelationalDriver, RowBatch, SchemaEdge, SchemaGraph, SchemaInfo, SchemaNode,
    TableInfo, TableSchema,
};
use futures_util::Stream;
use futures_util::StreamExt;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};

pub struct PostgresDriver {
    client: Arc<Client>,
    context: Arc<PostgresExecutionContext>,
    _connection_task: tokio::task::JoinHandle<()>,
    cancel_tokens: Arc<Mutex<HashMap<String, tokio_postgres::CancelToken>>>,
}

impl PostgresDriver {
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

        let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
            .await
            .map_err(|e| e.to_string())?;

        let connection_task = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Postgres connection error: {}", e);
            }
        });

        let client_arc = Arc::new(client);
        let context = Arc::new(PostgresExecutionContext::new(client_arc.clone()).await?);

        Ok(Self {
            client: client_arc,
            context,
            _connection_task: connection_task,
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

pub struct PostgresExecutionContext {
    client: Arc<Client>,
    active_schema: tokio::sync::Mutex<String>,
}

impl PostgresExecutionContext {
    pub async fn new(client: Arc<Client>) -> Result<Self, String> {
        let rows = client
            .query("SELECT current_schema()", &[])
            .await
            .map_err(|e| e.to_string())?;
        let active_schema = if let Some(row) = rows.first() {
            row.get::<_, Option<String>>(0)
                .unwrap_or_else(|| "public".to_string())
        } else {
            "public".to_string()
        };
        Ok(Self {
            client,
            active_schema: tokio::sync::Mutex::new(active_schema),
        })
    }
}

#[async_trait]
impl ExecutionContext for PostgresExecutionContext {
    async fn get_active_schema(&self) -> Result<String, String> {
        let schema = self.active_schema.lock().await;
        Ok(schema.clone())
    }

    async fn set_active_schema(&self, schema: &str) -> Result<(), String> {
        let escaped = schema.replace("\"", "\"\"");
        self.client
            .execute(&format!("SET search_path TO \"{}\"", escaped), &[])
            .await
            .map_err(|e| e.to_string())?;
        let mut active = self.active_schema.lock().await;
        *active = schema.to_string();
        Ok(())
    }

    async fn get_search_path(&self) -> Result<Vec<String>, String> {
        let rows = self
            .client
            .query("SHOW search_path", &[])
            .await
            .map_err(|e| e.to_string())?;
        if let Some(row) = rows.first() {
            let path_str: String = row.get(0);
            let paths = path_str
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .collect();
            Ok(paths)
        } else {
            Ok(Vec::new())
        }
    }

    async fn open_session(&self, _purpose: &str) -> Result<Box<dyn DbSession>, String> {
        Ok(Box::new(PostgresSession {
            client: self.client.clone(),
        }))
    }
}

pub struct PostgresSession {
    client: Arc<Client>,
}

#[async_trait]
impl DbSession for PostgresSession {
    async fn prepare_statement(&self, sql: &str) -> Result<Box<dyn DbStatement>, String> {
        let stmt = self.client.prepare(sql).await.map_err(|e| e.to_string())?;
        Ok(Box::new(PostgresStatement {
            client: self.client.clone(),
            stmt,
            _fetch_size: 100,
            _timeout_seconds: None,
        }))
    }
}

pub struct PostgresStatement {
    client: Arc<Client>,
    stmt: tokio_postgres::Statement,
    _fetch_size: usize,
    _timeout_seconds: Option<u32>,
}

#[async_trait]
impl DbStatement for PostgresStatement {
    async fn execute_query(&self) -> Result<Box<dyn DbResultSet>, String> {
        let row_stream = self
            .client
            .query_raw(&self.stmt, std::iter::empty::<Option<i32>>())
            .await
            .map_err(|e| e.to_string())?;

        let columns = self
            .stmt
            .columns()
            .iter()
            .map(|c| ColumnInfo {
                name: c.name().to_string(),
                data_type: c.type_().name().to_string(),
            })
            .collect();

        Ok(Box::new(PostgresResultSet {
            columns,
            stream: tokio::sync::Mutex::new(Box::pin(row_stream)),
        }))
    }

    async fn execute_update(&self) -> Result<u64, String> {
        let rows_affected = self
            .client
            .execute(&self.stmt, &[])
            .await
            .map_err(|e| e.to_string())?;
        Ok(rows_affected)
    }

    fn set_fetch_size(&mut self, size: usize) {
        self._fetch_size = size;
    }

    fn set_query_timeout(&mut self, seconds: u32) {
        self._timeout_seconds = Some(seconds);
    }
}

pub struct PostgresResultSet {
    columns: Vec<ColumnInfo>,
    stream: tokio::sync::Mutex<
        Pin<
            Box<
                dyn Stream<Item = Result<tokio_postgres::Row, tokio_postgres::Error>>
                    + Send,
            >,
        >,
    >,
}

#[async_trait]
impl DbResultSet for PostgresResultSet {
    fn get_metadata(&self) -> Result<Vec<ColumnInfo>, String> {
        Ok(self.columns.clone())
    }

    async fn next_row_batch(&mut self, batch_size: usize) -> Result<Option<RowBatch>, String> {
        let mut stream = self.stream.lock().await;
        let mut rows = Vec::new();

        for _ in 0..batch_size {
            match stream.next().await {
                Some(Ok(row)) => {
                    let mut row_values = Vec::new();
                    for i in 0..row.len() {
                        row_values.push(pg_value_to_json(&row, i));
                    }
                    rows.push(row_values);
                }
                Some(Err(e)) => return Err(e.to_string()),
                None => break,
            }
        }

        if rows.is_empty() {
            Ok(None)
        } else {
            Ok(Some(RowBatch {
                columns: self.columns.clone(),
                rows,
            }))
        }
    }
}

struct RawValue<'a>(&'a [u8]);

impl<'a> tokio_postgres::types::FromSql<'a> for RawValue<'a> {
    fn from_sql(
        _ty: &tokio_postgres::types::Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(RawValue(raw))
    }

    fn accepts(_ty: &tokio_postgres::types::Type) -> bool {
        true
    }
}

fn pg_value_to_json(row: &tokio_postgres::Row, index: usize) -> serde_json::Value {
    let col = &row.columns()[index];
    let ty = col.type_();

    match *ty {
        tokio_postgres::types::Type::BOOL => match row.try_get::<_, Option<bool>>(index) {
            Ok(Some(val)) => serde_json::Value::Bool(val),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::INT2 => match row.try_get::<_, Option<i16>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::INT4 => match row.try_get::<_, Option<i32>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::INT8 => match row.try_get::<_, Option<i64>>(index) {
            Ok(Some(val)) => serde_json::Value::Number(val.into()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::FLOAT4 => match row.try_get::<_, Option<f32>>(index) {
            Ok(Some(val)) => {
                if let Some(n) = serde_json::Number::from_f64(val as f64) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::FLOAT8 => match row.try_get::<_, Option<f64>>(index) {
            Ok(Some(val)) => {
                if let Some(n) = serde_json::Number::from_f64(val) {
                    serde_json::Value::Number(n)
                } else {
                    serde_json::Value::Null
                }
            }
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::VARCHAR
        | tokio_postgres::types::Type::TEXT
        | tokio_postgres::types::Type::BPCHAR
        | tokio_postgres::types::Type::NAME => match row.try_get::<_, Option<String>>(index) {
            Ok(Some(val)) => serde_json::Value::String(val),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::JSON | tokio_postgres::types::Type::JSONB => {
            match row.try_get::<_, Option<serde_json::Value>>(index) {
                Ok(Some(val)) => val,
                _ => serde_json::Value::Null,
            }
        }
        tokio_postgres::types::Type::TIMESTAMP | tokio_postgres::types::Type::TIMESTAMPTZ => {
            if let Ok(val_opt) = row.try_get::<_, Option<chrono::NaiveDateTime>>(index) {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else if let Ok(val_opt) =
                row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(index)
            {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else {
                serde_json::Value::String("<timestamp>".to_string())
            }
        }
        tokio_postgres::types::Type::DATE => {
            if let Ok(val_opt) = row.try_get::<_, Option<chrono::NaiveDate>>(index) {
                match val_opt {
                    Some(val) => serde_json::Value::String(val.to_string()),
                    None => serde_json::Value::Null,
                }
            } else {
                serde_json::Value::String("<date>".to_string())
            }
        }
        tokio_postgres::types::Type::UUID => match row.try_get::<_, Option<uuid::Uuid>>(index) {
            Ok(Some(val)) => serde_json::Value::String(val.to_string()),
            _ => serde_json::Value::Null,
        },
        tokio_postgres::types::Type::NUMERIC => {
            match row.try_get::<_, Option<rust_decimal::Decimal>>(index) {
                Ok(Some(val)) => serde_json::to_value(val).unwrap_or(serde_json::Value::Null),
                _ => serde_json::Value::Null,
            }
        }
        _ => {
            if let tokio_postgres::types::Kind::Enum(_) = ty.kind() {
                if let Ok(Some(raw_val)) = row.try_get::<_, Option<RawValue>>(index) {
                    if let Ok(s) = std::str::from_utf8(raw_val.0) {
                        return serde_json::Value::String(s.to_string());
                    }
                }
            }

            if let Ok(Some(val)) = row.try_get::<_, Option<String>>(index) {
                serde_json::Value::String(val)
            } else {
                if let Ok(None) = row.try_get::<_, Option<String>>(index) {
                    serde_json::Value::Null
                } else {
                    serde_json::Value::String(format!("<type: {}>", ty.name()))
                }
            }
        }
    }
}

#[async_trait]
impl DataSource for PostgresDriver {
    async fn get_default_context(&self) -> Result<Arc<dyn ExecutionContext>, String> {
        Ok(self.context.clone())
    }

    async fn open_context(&self, _purpose: &str) -> Result<Arc<dyn ExecutionContext>, String> {
        let ctx = PostgresExecutionContext::new(self.client.clone()).await?;
        Ok(Arc::new(ctx))
    }

    async fn get_server_version(&self) -> Result<String, String> {
        let rows = self
            .client
            .query("SELECT version()", &[])
            .await
            .map_err(|e| e.to_string())?;
        if let Some(row) = rows.first() {
            let version: String = row.get(0);
            Ok(version)
        } else {
            Err("Failed to query version".to_string())
        }
    }
}

#[async_trait]
impl RelationalDriver for PostgresDriver {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, String> {
        let ctx = self.get_default_context().await?;
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
        let ctx = self.get_default_context().await?;
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
        let ctx = self.get_default_context().await?;
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
        let ctx = self.get_default_context().await?;
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
        let cancel_token = self.client.cancel_token();
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

        let ctx = self.get_default_context().await?;
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

