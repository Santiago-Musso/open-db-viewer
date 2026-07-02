use async_trait::async_trait;
use driver_api::{
    ColumnInfo, DbResultSet, DbSession, DbStatement, ExecutionContext, RowBatch, SqlDialect,
    DatabaseError, ErrorCategory,
};
use futures_util::Stream;
use futures_util::StreamExt;
use std::pin::Pin;
use std::sync::Arc;
use tokio_postgres::Client;
use crate::dialect::PostgreDialect;
use crate::types::CustomTypeRegistry;

pub fn map_db_error(e: tokio_postgres::Error) -> DatabaseError {
    if let Some(db_err) = e.as_db_error() {
        let sql_state = db_err.code().code().to_string();
        let position = match db_err.position() {
            Some(tokio_postgres::error::ErrorPosition::Original(pos)) => Some(*pos as usize),
            Some(tokio_postgres::error::ErrorPosition::Internal { position, .. }) => Some(*position as usize),
            None => None,
        };

        let category = match sql_state.as_str() {
            s if s.starts_with("42") => ErrorCategory::SyntaxError,
            s if s.starts_with("28") => ErrorCategory::PermissionDenied,
            s if s.starts_with("23") => ErrorCategory::IntegrityConstraintViolation,
            s if s.starts_with("08") => ErrorCategory::ConnectionFailure,
            _ => ErrorCategory::Unknown,
        };

        DatabaseError {
            message: db_err.message().to_string(),
            sql_state: Some(sql_state),
            position,
            severity: Some(db_err.severity().to_string()),
            detail: db_err.detail().map(|s| s.to_string()),
            category,
        }
    } else {
        DatabaseError {
            message: e.to_string(),
            sql_state: None,
            position: None,
            severity: None,
            detail: None,
            category: ErrorCategory::Unknown,
        }
    }
}

pub struct PostgresExecutionContext {
    client: Arc<Client>,
    active_schema: tokio::sync::Mutex<String>,
    pub type_registry: Arc<CustomTypeRegistry>,
}

impl PostgresExecutionContext {
    pub async fn new(client: Arc<Client>, type_registry: Arc<CustomTypeRegistry>) -> Result<Self, DatabaseError> {
        let rows = client
            .query("SELECT current_schema()", &[])
            .await
            .map_err(map_db_error)?;
        let active_schema = if let Some(row) = rows.first() {
            row.get::<_, Option<String>>(0)
                .unwrap_or_else(|| "public".to_string())
        } else {
            "public".to_string()
        };
        Ok(Self {
            client,
            active_schema: tokio::sync::Mutex::new(active_schema),
            type_registry,
        })
    }

    pub fn cancel_token(&self) -> tokio_postgres::CancelToken {
        self.client.cancel_token()
    }
}

#[async_trait]
impl ExecutionContext for PostgresExecutionContext {
    async fn get_active_schema(&self) -> Result<String, DatabaseError> {
        let schema = self.active_schema.lock().await;
        Ok(schema.clone())
    }

    async fn set_active_schema(&self, schema: &str) -> Result<(), DatabaseError> {
        let quoted = PostgreDialect.quote_identifier(schema);
        self.client
            .execute(&format!("SET search_path TO {}", quoted), &[])
            .await
            .map_err(map_db_error)?;
        let mut active = self.active_schema.lock().await;
        *active = schema.to_string();
        Ok(())
    }

    async fn get_search_path(&self) -> Result<Vec<String>, DatabaseError> {
        let rows = self
            .client
            .query("SHOW search_path", &[])
            .await
            .map_err(map_db_error)?;
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

    async fn open_session(&self, _purpose: &str) -> Result<Box<dyn DbSession>, DatabaseError> {
        Ok(Box::new(PostgresSession {
            client: self.client.clone(),
            type_registry: self.type_registry.clone(),
        }))
    }
}

pub struct PostgresSession {
    client: Arc<Client>,
    type_registry: Arc<CustomTypeRegistry>,
}

#[async_trait]
impl DbSession for PostgresSession {
    async fn prepare_statement(&self, sql: &str) -> Result<Box<dyn DbStatement>, DatabaseError> {
        let stmt = self.client.prepare(sql).await.map_err(map_db_error)?;
        Ok(Box::new(PostgresStatement {
            client: self.client.clone(),
            stmt,
            type_registry: self.type_registry.clone(),
            _fetch_size: 100,
            _timeout_seconds: None,
        }))
    }
}

pub struct PostgresStatement {
    client: Arc<Client>,
    stmt: tokio_postgres::Statement,
    type_registry: Arc<CustomTypeRegistry>,
    _fetch_size: usize,
    _timeout_seconds: Option<u32>,
}

#[async_trait]
impl DbStatement for PostgresStatement {
    async fn execute_query(&self) -> Result<Box<dyn DbResultSet>, DatabaseError> {
        let row_stream = self
            .client
            .query_raw(&self.stmt, std::iter::empty::<Option<i32>>())
            .await
            .map_err(map_db_error)?;

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
            type_registry: self.type_registry.clone(),
        }))
    }

    async fn execute_update(&self) -> Result<u64, DatabaseError> {
        let rows_affected = self
            .client
            .execute(&self.stmt, &[])
            .await
            .map_err(map_db_error)?;
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
    type_registry: Arc<CustomTypeRegistry>,
}

#[async_trait]
impl DbResultSet for PostgresResultSet {
    fn get_metadata(&self) -> Result<Vec<ColumnInfo>, DatabaseError> {
        Ok(self.columns.clone())
    }

    async fn next_row_batch(&mut self, batch_size: usize) -> Result<Option<RowBatch>, DatabaseError> {
        let mut stream = self.stream.lock().await;
        let mut rows = Vec::new();

        for _ in 0..batch_size {
            match stream.next().await {
                Some(Ok(row)) => {
                    let mut row_values = Vec::new();
                    for i in 0..row.len() {
                        row_values.push(crate::types::pg_value_to_json(&row, i, Some(&self.type_registry)));
                    }
                    rows.push(row_values);
                }
                Some(Err(e)) => return Err(map_db_error(e)),
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

#[cfg(test)]
mod connection_tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_error_mapping() {
        let conn_res = tokio_postgres::connect(
            "host=invalid_host_123456789 port=5432 user=postgres",
            tokio_postgres::NoTls,
        )
        .await;
        assert!(conn_res.is_err());
        let pg_err = match conn_res {
            Ok(_) => panic!("Expected error"),
            Err(e) => e,
        };
        let db_err = map_db_error(pg_err);
        assert_eq!(db_err.category, ErrorCategory::Unknown);
        assert!(!db_err.message.is_empty());
    }
}


