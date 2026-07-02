use async_trait::async_trait;
use driver_api::{
    ColumnInfo, DbResultSet, DbSession, DbStatement, ExecutionContext, RowBatch, SqlDialect,
};
use futures_util::Stream;
use futures_util::StreamExt;
use std::pin::Pin;
use std::sync::Arc;
use tokio_postgres::Client;
use crate::dialect::PostgreDialect;

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

    pub fn cancel_token(&self) -> tokio_postgres::CancelToken {
        self.client.cancel_token()
    }
}

#[async_trait]
impl ExecutionContext for PostgresExecutionContext {
    async fn get_active_schema(&self) -> Result<String, String> {
        let schema = self.active_schema.lock().await;
        Ok(schema.clone())
    }

    async fn set_active_schema(&self, schema: &str) -> Result<(), String> {
        let quoted = PostgreDialect.quote_identifier(schema);
        self.client
            .execute(&format!("SET search_path TO {}", quoted), &[])
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
                        row_values.push(crate::types::pg_value_to_json(&row, i));
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
