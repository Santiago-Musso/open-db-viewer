use async_trait::async_trait;
use driver_api::{
    ColumnInfo, ConnectionConfig, RelationalDriver, RowBatch, SchemaEdge, SchemaGraph, SchemaInfo,
    SchemaNode, TableInfo, TableSchema,
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

        Ok(Self {
            client: Arc::new(client),
            _connection_task: connection_task,
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        })
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
impl RelationalDriver for PostgresDriver {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, String> {
        let rows = self
            .client
            .query(
                "SELECT schema_name FROM information_schema.schemata \
                 WHERE schema_name NOT IN ('pg_catalog', 'information_schema') \
                 ORDER BY schema_name",
                &[],
            )
            .await
            .map_err(|e| e.to_string())?;

        let mut schemas = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            schemas.push(SchemaInfo { name });
        }
        Ok(schemas)
    }

    async fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>, String> {
        let rows = self
            .client
            .query(
                "SELECT table_schema, table_name FROM information_schema.tables \
                 WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
                 ORDER BY table_name",
                &[&schema],
            )
            .await
            .map_err(|e| e.to_string())?;

        let mut tables = Vec::new();
        for row in rows {
            let schema: String = row.get(0);
            let name: String = row.get(1);
            tables.push(TableInfo { schema, name });
        }
        Ok(tables)
    }

    async fn describe_table(&self, schema: &str, table: &str) -> Result<TableSchema, String> {
        let rows = self
            .client
            .query(
                "SELECT column_name, data_type FROM information_schema.columns \
                 WHERE table_schema = $1 AND table_name = $2 \
                 ORDER BY ordinal_position",
                &[&schema, &table],
            )
            .await
            .map_err(|e| e.to_string())?;

        let mut columns = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            let data_type: String = row.get(1);
            columns.push(ColumnInfo { name, data_type });
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
        // Fetch all tables
        let tables = self.list_tables(schema).await?;
        let mut nodes = Vec::new();
        for t in &tables {
            let schema_info = self.describe_table(schema, &t.name).await?;
            nodes.push(SchemaNode {
                id: t.name.clone(),
                label: t.name.clone(),
                columns: schema_info.columns,
            });
        }

        // Fetch all foreign key constraints using pg_catalog to avoid information_schema privilege bugs
        let rows = self
            .client
            .query(
                "SELECT 
                    c.conname AS constraint_name, 
                    cl1.relname AS source_table, 
                    a1.attname AS source_column, 
                    cl2.relname AS target_table, 
                    a2.attname AS target_column 
                 FROM pg_constraint c 
                 JOIN pg_class cl1 ON c.conrelid = cl1.oid 
                 JOIN pg_class cl2 ON c.confrelid = cl2.oid 
                 JOIN pg_namespace n1 ON cl1.relnamespace = n1.oid 
                 JOIN pg_attribute a1 ON a1.attnum = ANY(c.conkey) AND a1.attrelid = cl1.oid 
                 JOIN pg_attribute a2 ON a2.attnum = ANY(c.confkey) AND a2.attrelid = cl2.oid 
                 WHERE c.contype = 'f' AND n1.nspname = $1",
                &[&schema],
            )
            .await
            .map_err(|e| e.to_string())?;

        let mut edges = Vec::new();
        for row in rows {
            let constraint_name: String = row.get(0);
            let source_table: String = row.get(1);
            let source_column: String = row.get(2);
            let target_table: String = row.get(3);
            let target_column: String = row.get(4);

            edges.push(SchemaEdge {
                id: constraint_name,
                source: source_table.clone(),
                target: target_table.clone(),
                source_handle: source_column,
                target_handle: target_column,
            });
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
                final_sql = format!("SELECT * FROM ({}) AS _odv_wrapper LIMIT {} OFFSET {}", sql, batch_size, off);
            }
        }

        let stmt = self.client.prepare(&final_sql).await.map_err(|e| e.to_string())?;
        let columns: Vec<ColumnInfo> = stmt
            .columns()
            .iter()
            .map(|c| ColumnInfo {
                name: c.name().to_string(),
                data_type: c.type_().name().to_string(),
            })
            .collect();

        let row_stream = self
            .client
            .query_raw(&stmt, std::iter::empty::<Option<i32>>())
            .await
            .map_err(|e| e.to_string())?;

        let columns_clone = columns.clone();
        let mapped_stream = row_stream.map(move |row_res| match row_res {
            Ok(row) => {
                let mut row_values = Vec::new();
                for i in 0..row.len() {
                    let val = pg_value_to_json(&row, i);
                    row_values.push(val);
                }
                Ok(row_values)
            }
            Err(e) => Err(e.to_string()),
        });

        let cancel_tokens_clone = self.cancel_tokens.clone();
        let query_id_clone = query_id.to_string();

        let batched_stream = mapped_stream
            .chunks(batch_size)
            .map(move |chunk| {
                let mut rows = Vec::new();
                for row_res in chunk {
                    match row_res {
                        Ok(row) => rows.push(row),
                        Err(e) => return Err(e),
                    }
                }
                Ok(RowBatch {
                    columns: columns_clone.clone(),
                    rows,
                })
            })
            // Remove the cancel token when the stream completes/ends
            .then(move |batch_res| {
                let cancel_tokens = cancel_tokens_clone.clone();
                let query_id = query_id_clone.clone();
                async move {
                    // Check if it's the end of stream (handled automatically when dropped, but let's clear here as well)
                    // If we want to clean it up, we can do it inside this map block
                    cancel_tokens.lock().await.remove(&query_id);
                    batch_res
                }
            });

        Ok(Box::pin(batched_stream))
    }

    async fn cancel_query(&self, query_id: &str) -> Result<(), String> {
        let token_opt = self.cancel_tokens.lock().await.remove(query_id);
        if let Some(token) = token_opt {
            // Execute the cancellation out of band (non-blocking)
            // Wait, we need an active connection to cancel. tokio-postgres cancel_query() makes a new connection to request cancel
            // It is an async operation that requires NoTls or similar
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
