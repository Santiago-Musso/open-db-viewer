use async_trait::async_trait;
use driver_api::{
    ColumnInfo, ConnectionConfig, ConstraintInfo, ConstraintType, DataSource, DatabaseError,
    DbSessionInfo, ExecutionContext, ExplainOptions, ExtensionInfo, IndexInfo, PlanNode,
    ProcedureInfo, RelationalDriver, RowBatch, SchemaEdge, SchemaGraph, SchemaInfo, SchemaNode,
    SequenceInfo, SqlDialect, TableInfo, TableKind, TableSchema, TableStats,
};
use futures_util::Stream;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_postgres::NoTls;

pub mod connection;
pub mod dialect;
pub mod metadata;
pub mod plan;
pub mod types;

use connection::{map_db_error, PostgresExecutionContext};

pub struct PostgresDriver {
    main_context: Arc<PostgresExecutionContext>,
    metadata_context: Arc<PostgresExecutionContext>,
    utility_context: Arc<PostgresExecutionContext>,
    dialect: Arc<dialect::PostgreDialect>,
    #[allow(dead_code)]
    type_registry: Arc<types::CustomTypeRegistry>,

    schema_cache: metadata::ObjectLookupCache<(), Vec<SchemaInfo>>,
    table_cache: metadata::ObjectLookupCache<String, Vec<TableInfo>>,
    column_cache: metadata::ObjectLookupCache<(String, String), TableSchema>,
    graph_cache: metadata::ObjectLookupCache<String, SchemaGraph>,

    _connection_tasks: Vec<tokio::task::JoinHandle<()>>,
    cancel_tokens: Arc<Mutex<HashMap<String, tokio_postgres::CancelToken>>>,
}

impl PostgresDriver {
    async fn connect_single(
        conn_str: &str,
    ) -> Result<(tokio_postgres::Client, tokio::task::JoinHandle<()>), String> {
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
        // R22: Add application_name client info
        conn_str.push_str(" application_name=open-db-viewer");

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

        // R8: standard_conforming_strings detection
        let dialect = Arc::new(dialect::PostgreDialect::default());
        if let Ok(row) = utility_arc
            .query_one("SHOW standard_conforming_strings", &[])
            .await
        {
            let scs_val: String = row.get(0);
            dialect
                .standard_conforming_strings
                .store(scs_val == "on", Ordering::SeqCst);
        }

        // Fetch all custom type definitions (R17)
        let mut type_registry = types::CustomTypeRegistry::new();
        if let Ok(rows) = utility_arc.query(
            "SELECT t.oid, t.typname, n.nspname, t.typtype::text, \
                    t.typcategory::text, t.typelem, t.typbasetype, \
                    bt.typname as base_type_name, \
                    pg_catalog.format_type(t.oid, t.typtypmod) as full_type_name, \
                    d.description \
             FROM pg_catalog.pg_type t \
             LEFT OUTER JOIN pg_catalog.pg_type et ON et.oid = t.typelem \
             LEFT OUTER JOIN pg_catalog.pg_class c ON c.oid = t.typrelid \
             LEFT OUTER JOIN pg_catalog.pg_type bt ON bt.oid = t.typbasetype \
             LEFT OUTER JOIN pg_catalog.pg_namespace n ON t.typnamespace = n.oid \
             LEFT OUTER JOIN pg_catalog.pg_description d ON t.oid = d.objoid AND d.classoid = 'pg_type'::regclass \
             WHERE t.typname IS NOT NULL \
               AND (c.relkind IS NULL OR c.relkind = 'c') \
               AND (et.typcategory IS NULL OR et.typcategory <> 'C')",
            &[]
        ).await {
            for row in rows {
                let oid: u32 = row.get(0);
                let name: String = row.get(1);
                let schema: String = row.get(2);
                let typtype_str: String = row.get(3);
                let typtype = typtype_str.chars().next().unwrap_or(' ');
                let typcategory_str: String = row.get(4);
                let typcategory = typcategory_str.chars().next().unwrap_or(' ');
                let typelem: u32 = row.get(5);
                let typbasetype: u32 = row.get(6);
                let base_type_name: Option<String> = row.get(7);
                let full_type_name: String = row.get(8);
                let description: Option<String> = row.get(9);

                type_registry.insert(types::CustomTypeInfo {
                    oid,
                    name,
                    schema,
                    typtype,
                    typcategory,
                    typelem,
                    typbasetype,
                    base_type_name,
                    full_type_name,
                    description,
                });
            }
        }
        let type_registry_arc = Arc::new(type_registry);

        let main_context = Arc::new(
            PostgresExecutionContext::new(main_arc, type_registry_arc.clone())
                .await
                .map_err(|e| e.to_string())?,
        );
        let metadata_context = Arc::new(
            PostgresExecutionContext::new(metadata_arc, type_registry_arc.clone())
                .await
                .map_err(|e| e.to_string())?,
        );
        let utility_context = Arc::new(
            PostgresExecutionContext::new(utility_arc, type_registry_arc.clone())
                .await
                .map_err(|e| e.to_string())?,
        );

        let schema_cache = metadata::ObjectLookupCache::new();
        let table_cache = metadata::ObjectLookupCache::new();
        let column_cache = metadata::ObjectLookupCache::new();
        let graph_cache = metadata::ObjectLookupCache::new();

        Ok(Self {
            main_context,
            metadata_context,
            utility_context,
            dialect,
            type_registry: type_registry_arc,
            schema_cache,
            table_cache,
            column_cache,
            graph_cache,
            _connection_tasks: vec![main_task, metadata_task, utility_task],
            cancel_tokens: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl DataSource for PostgresDriver {
    async fn get_default_context(&self) -> Result<Arc<dyn ExecutionContext>, DatabaseError> {
        Ok(self.main_context.clone())
    }

    async fn open_context(
        &self,
        purpose: &str,
    ) -> Result<Arc<dyn ExecutionContext>, DatabaseError> {
        match purpose {
            "metadata" => Ok(self.metadata_context.clone()),
            "plan" | "utility" | "cancel" => Ok(self.utility_context.clone()),
            _ => Ok(self.main_context.clone()),
        }
    }

    fn get_dialect(&self) -> Arc<dyn SqlDialect> {
        self.dialect.clone()
    }

    async fn get_server_version(&self) -> Result<String, DatabaseError> {
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
        Err(DatabaseError::new("Failed to query version".to_string()))
    }
}

#[async_trait]
impl RelationalDriver for PostgresDriver {
    async fn list_schemas(&self) -> Result<Vec<SchemaInfo>, DatabaseError> {
        let metadata_ctx = self.metadata_context.clone();
        let val = self
            .schema_cache
            .get_or_load((), || async move {
                let rows = metadata_ctx
                    .client
                    .query(
                        "SELECT n.nspname FROM pg_catalog.pg_namespace n \
                 WHERE nspname NOT LIKE 'pg_%' \
                   AND nspname <> 'information_schema' \
                 ORDER BY n.nspname",
                        &[],
                    )
                    .await
                    .map_err(map_db_error)?;

                let mut schemas = Vec::new();
                for row in rows {
                    let name: String = row.get(0);
                    schemas.push(SchemaInfo { name });
                }
                Ok(schemas)
            })
            .await?;
        Ok((*val).clone())
    }

    async fn list_tables(&self, schema: &str) -> Result<Vec<TableInfo>, DatabaseError> {
        let schema_owned = schema.to_string();
        let loader_schema = schema_owned.clone();
        let metadata_ctx = self.metadata_context.clone();
        let val = self.table_cache.get_or_load(schema_owned, || async move {
            let rows = metadata_ctx.client.query(
                "SELECT c.oid, c.relname, c.relkind::text, pg_catalog.obj_description(c.oid, 'pg_class') as description \
                 FROM pg_catalog.pg_class c \
                 JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
                 WHERE n.nspname = $1 \
                   AND c.relkind IN ('r', 'p', 'f', 'v', 'm') \
                 ORDER BY c.relname",
                &[&loader_schema],
            ).await.map_err(map_db_error)?;

            let oids: Vec<u32> = rows.iter().map(|row| row.get::<_, u32>(0)).collect();
            let mut stats_map = HashMap::new();
            if !oids.is_empty() {
                if let Ok(stats_rows) = metadata_ctx.client.query(
                    "SELECT oid, \
                            pg_catalog.pg_relation_size(oid) as table_size, \
                            pg_catalog.pg_total_relation_size(oid) as total_size, \
                            GREATEST(reltuples, 0)::bigint as estimated_row_count \
                     FROM pg_catalog.pg_class \
                     WHERE oid = ANY($1)",
                    &[&oids],
                ).await {
                    for r in stats_rows {
                        let oid: u32 = r.get(0);
                        let table_size: i64 = r.get(1);
                        let total_size: i64 = r.get(2);
                        let estimated_row_count: i64 = r.get(3);
                        stats_map.insert(oid, TableStats {
                            table_size,
                            total_size,
                            estimated_row_count,
                        });
                    }
                }
            }

            let mut tables = Vec::new();
            for row in rows {
                let oid_u32: u32 = row.get(0);
                let name: String = row.get(1);
                let relkind: String = row.get(2);
                let description: Option<String> = row.get(3);

                let table_kind = match relkind.as_str() {
                    "p" => TableKind::Partitioned,
                    "f" => TableKind::Foreign,
                    "v" => TableKind::View,
                    "m" => TableKind::MaterializedView,
                    _ => TableKind::Regular,
                };

                let stats = stats_map.get(&oid_u32).cloned();

                tables.push(TableInfo {
                    schema: loader_schema.clone(),
                    name,
                    oid: oid_u32 as u64,
                    table_kind,
                    description,
                    stats,
                });
            }
            Ok(tables)
        }).await?;
        Ok((*val).clone())
    }

    async fn describe_table(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<TableSchema, DatabaseError> {
        let key = (schema.to_string(), table.to_string());
        let loader_key = key.clone();
        let metadata_ctx = self.metadata_context.clone();
        let val = self.column_cache.get_or_load(key, || async move {
            let rows = metadata_ctx.client.query(
                "SELECT a.attname, \
                        pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type, \
                        a.attnotnull as not_null, \
                        pg_catalog.pg_get_expr(ad.adbin, ad.adrelid, true) as default_value, \
                        d.description, \
                        a.atttypid as type_oid \
                 FROM pg_catalog.pg_attribute a \
                 JOIN pg_catalog.pg_class c ON a.attrelid = c.oid \
                 JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
                 LEFT OUTER JOIN pg_catalog.pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum \
                 LEFT OUTER JOIN pg_catalog.pg_description d ON c.oid = d.objoid AND a.attnum = d.objsubid \
                 WHERE n.nspname = $1 AND c.relname = $2 \
                   AND a.attnum > 0 AND NOT a.attisdropped \
                 ORDER BY a.attnum",
                &[&loader_key.0, &loader_key.1],
            ).await.map_err(map_db_error)?;

            let mut columns = Vec::new();
            for row in rows {
                let name: String = row.get(0);
                let data_type: String = row.get(1);
                let not_null: bool = row.get(2);
                let default_value: Option<String> = row.get(3);
                let description: Option<String> = row.get(4);
                let type_oid: u32 = row.get(5);

                columns.push(ColumnInfo {
                    name,
                    data_type,
                    is_nullable: !not_null,
                    default_value,
                    description,
                    type_oid,
                });
            }
            Ok(TableSchema { columns })
        }).await?;
        Ok((*val).clone())
    }

    async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, DatabaseError> {
        let cols = self.describe_table(schema, table).await?;

        let mut ddl = format!("CREATE TABLE {}.{} (\n", schema, table);
        let mut parts = Vec::new();

        for c in &cols.columns {
            let mut col_def = format!("    {} {}", c.name, c.data_type.to_uppercase());
            if !c.is_nullable {
                col_def.push_str(" NOT NULL");
            }
            if let Some(def) = &c.default_value {
                col_def.push_str(&format!(" DEFAULT {}", def));
            }
            parts.push(col_def);
        }

        if let Ok(constraint_rows) = self
            .metadata_context
            .client
            .query(
                "SELECT conname, pg_catalog.pg_get_constraintdef(oid) as definition \
             FROM pg_catalog.pg_constraint \
             WHERE conrelid = (SELECT c.oid FROM pg_catalog.pg_class c \
                               JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
                               WHERE n.nspname = $1 AND c.relname = $2) \
             ORDER BY contype, conname",
                &[&schema, &table],
            )
            .await
        {
            for row in constraint_rows {
                let conname: String = row.get(0);
                let definition: String = row.get(1);
                parts.push(format!("    CONSTRAINT {} {}", conname, definition));
            }
        }

        ddl.push_str(&parts.join(",\n"));
        ddl.push_str("\n);");

        if let Ok(index_rows) = self
            .metadata_context
            .client
            .query(
                "SELECT pg_catalog.pg_get_indexdef(indexrelid) as indexdef \
             FROM pg_catalog.pg_index \
             WHERE indrelid = (SELECT c.oid FROM pg_catalog.pg_class c \
                               JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
                               WHERE n.nspname = $1 AND c.relname = $2) \
               AND NOT indisprimary",
                &[&schema, &table],
            )
            .await
        {
            for row in index_rows {
                let index_def: String = row.get(0);
                ddl.push_str(&format!("\n\n{};", index_def));
            }
        }

        Ok(ddl)
    }

    async fn get_schema_graph(&self, schema: &str) -> Result<SchemaGraph, DatabaseError> {
        let schema_owned = schema.to_string();
        let loader_schema = schema_owned.clone();
        let metadata_ctx = self.metadata_context.clone();
        let val = self.graph_cache.get_or_load(schema_owned, || async move {
            let col_rows = metadata_ctx.client.query(
                "SELECT c.relname AS table_name, a.attname AS column_name, t.typname AS data_type, \
                        a.attnotnull as not_null, pg_catalog.pg_get_expr(ad.adbin, ad.adrelid, true) as default_value, \
                        d.description, a.atttypid as type_oid \
                 FROM pg_class c \
                 JOIN pg_namespace n ON c.relnamespace = n.oid \
                 JOIN pg_attribute a ON a.attrelid = c.oid \
                 JOIN pg_type t ON a.atttypid = t.oid \
                 LEFT OUTER JOIN pg_catalog.pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum \
                 LEFT OUTER JOIN pg_catalog.pg_description d ON c.oid = d.objoid AND a.attnum = d.objsubid \
                 WHERE n.nspname = $1 \
                   AND c.relkind = 'r' \
                   AND a.attnum > 0 \
                   AND NOT a.attisdropped \
                 ORDER BY c.relname, a.attnum",
                &[&loader_schema],
            ).await.map_err(map_db_error)?;

            let mut table_map: HashMap<String, Vec<ColumnInfo>> = HashMap::new();
            for row in col_rows {
                let table_name: String = row.get(0);
                let col_name: String = row.get(1);
                let data_type: String = row.get(2);
                let not_null: bool = row.get(3);
                let default_value: Option<String> = row.get(4);
                let description: Option<String> = row.get(5);
                let type_oid: u32 = row.get(6);

                table_map
                    .entry(table_name.clone())
                    .or_default()
                    .push(ColumnInfo {
                        name: col_name,
                        data_type,
                        is_nullable: !not_null,
                        default_value,
                        description,
                        type_oid,
                    });
            }

            let nodes: Vec<SchemaNode> = table_map
                .into_iter()
                .map(|(name, columns)| SchemaNode {
                    id: name.clone(),
                    label: name,
                    columns,
                })
                .collect();

            let fk_rows = metadata_ctx.client.query(
                "SELECT con.conname, \
                        cl1.relname as src_table, \
                        a1.attname as src_column, \
                        cl2.relname as tgt_table, \
                        a2.attname as tgt_column, \
                        con.confmatchtype::text, \
                        con.confupdtype::text, \
                        con.confdeltype::text \
                 FROM pg_catalog.pg_constraint con \
                 JOIN pg_catalog.pg_class cl1 ON con.conrelid = cl1.oid \
                 JOIN pg_catalog.pg_class cl2 ON con.confrelid = cl2.oid \
                 JOIN pg_catalog.pg_namespace n1 ON cl1.relnamespace = n1.oid \
                 CROSS JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS src(attnum, ord) \
                 CROSS JOIN LATERAL unnest(con.confkey) WITH ORDINALITY AS tgt(attnum, ord) \
                 JOIN pg_catalog.pg_attribute a1 ON a1.attrelid = cl1.oid AND a1.attnum = src.attnum \
                 JOIN pg_catalog.pg_attribute a2 ON a2.attrelid = cl2.oid AND a2.attnum = tgt.attnum \
                 WHERE con.contype = 'f' AND n1.nspname = $1 \
                   AND src.ord = tgt.ord",
                &[&loader_schema],
            ).await.map_err(map_db_error)?;

            let mut edges = Vec::new();
            for row in fk_rows {
                let conname: String = row.get(0);
                let table1: String = row.get(1);
                let col1: String = row.get(2);
                let table2: String = row.get(3);
                let col2: String = row.get(4);
                let match_type_char: String = row.get(5);
                let update_rule_char: String = row.get(6);
                let delete_rule_char: String = row.get(7);

                let match_type = match match_type_char.as_str() {
                    "f" => Some("FULL".to_string()),
                    "p" => Some("PARTIAL".to_string()),
                    "s" => Some("SIMPLE".to_string()),
                    _ => None,
                };

                let rule_map = |c: &str| match c {
                    "c" => Some("CASCADE".to_string()),
                    "r" => Some("RESTRICT".to_string()),
                    "n" => Some("SET_NULL".to_string()),
                    "d" => Some("SET_DEFAULT".to_string()),
                    "a" => Some("NO_ACTION".to_string()),
                    _ => None,
                };

                edges.push(SchemaEdge {
                    id: conname,
                    source: table1,
                    source_handle: col1,
                    target: table2,
                    target_handle: col2,
                    match_type,
                    update_rule: rule_map(&update_rule_char),
                    delete_rule: rule_map(&delete_rule_char),
                });
            }

            Ok(SchemaGraph { nodes, edges })
        }).await?;
        Ok((*val).clone())
    }

    async fn execute_query_stream(
        &self,
        query_id: &str,
        sql: &str,
        batch_size: usize,
        offset: Option<usize>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<RowBatch, DatabaseError>> + Send>>, DatabaseError>
    {
        let cancel_token = self.main_context.cancel_token();
        self.cancel_tokens
            .lock()
            .await
            .insert(query_id.to_string(), cancel_token);

        let final_sql = self
            .get_dialect()
            .transform_query_limit(sql, batch_size, offset);

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

    async fn cancel_query(&self, query_id: &str) -> Result<(), DatabaseError> {
        let token_opt = self.cancel_tokens.lock().await.remove(query_id);
        if let Some(token) = token_opt {
            tokio::spawn(async move {
                let _ = token.cancel_query(NoTls).await;
            });
        }
        Ok(())
    }

    async fn refresh_schema(&self, schema: &str) -> Result<(), DatabaseError> {
        self.table_cache.invalidate(&schema.to_string()).await;
        self.graph_cache.invalidate(&schema.to_string()).await;
        let mut write = self.column_cache.cache.write().await;
        write.retain(|(s, _), _| s != schema);
        Ok(())
    }

    async fn refresh_table(&self, schema: &str, table: &str) -> Result<(), DatabaseError> {
        self.column_cache
            .invalidate(&(schema.to_string(), table.to_string()))
            .await;
        self.graph_cache.invalidate(&schema.to_string()).await;
        Ok(())
    }

    async fn get_execution_plan(
        &self,
        sql: &str,
        options: ExplainOptions,
    ) -> Result<PlanNode, DatabaseError> {
        let mut opts = vec!["FORMAT JSON"];
        if options.analyze {
            opts.push("ANALYZE");
        }
        if options.verbose {
            opts.push("VERBOSE");
        }
        if options.costs {
            opts.push("COSTS");
        }
        if options.buffers {
            opts.push("BUFFERS");
        }
        if options.timing {
            opts.push("TIMING");
        }
        if options.settings {
            opts.push("SETTINGS");
        }

        let explain_sql = format!("EXPLAIN ({}) {}", opts.join(", "), sql);
        let rows = self
            .utility_context
            .client
            .query(&explain_sql, &[])
            .await
            .map_err(map_db_error)?;

        if let Some(row) = rows.first() {
            let raw_str = if let Ok(val) = row.try_get::<_, String>(0) {
                val
            } else if let Ok(val) = row.try_get::<_, &str>(0) {
                val.to_string()
            } else if let Ok(val) = row.try_get::<_, serde_json::Value>(0) {
                serde_json::to_string(&val).unwrap_or_default()
            } else {
                match row.try_get::<_, types::RawValue>(0) {
                    Ok(raw_val) => {
                        if !raw_val.0.is_empty()
                            && raw_val.0[0] == 1
                            && row.columns()[0].type_().name() == "jsonb"
                        {
                            String::from_utf8_lossy(&raw_val.0[1..]).into_owned()
                        } else {
                            String::from_utf8_lossy(raw_val.0).into_owned()
                        }
                    }
                    Err(e) => {
                        return Err(DatabaseError::new(format!(
                            "Failed to retrieve EXPLAIN result: {}",
                            e
                        )))
                    }
                }
            };

            let arr: serde_json::Value = serde_json::from_str(&raw_str)
                .map_err(|e| DatabaseError::new(format!("Failed to parse EXPLAIN JSON: {}", e)))?;

            let plan_val = if arr.is_array() {
                &arr[0]["Plan"]
            } else {
                &arr["Plan"]
            };

            if plan_val.is_null() {
                return Err(DatabaseError::new("Explain plan key not found".to_string()));
            }

            let node: PlanNode = serde_json::from_value(plan_val.clone()).map_err(|e| {
                DatabaseError::new(format!("Failed to deserialize PlanNode: {}", e))
            })?;

            return Ok(node);
        }
        Err(DatabaseError::new(
            "Explain plan returned no results".to_string(),
        ))
    }

    async fn list_active_sessions(
        &self,
        show_idle: bool,
    ) -> Result<Vec<DbSessionInfo>, DatabaseError> {
        let mut sql = "SELECT \
                         pid, \
                         usename::text, \
                         query::text, \
                         state::text, \
                         query_start::text, \
                         client_addr::text \
                       FROM pg_catalog.pg_stat_activity \
                       WHERE backend_type = 'client backend'"
            .to_string();

        if !show_idle {
            sql.push_str(" AND (state IS NULL OR state NOT LIKE 'idle%')");
        }

        let rows = self
            .utility_context
            .client
            .query(&sql, &[])
            .await
            .map_err(map_db_error)?;
        let mut sessions = Vec::new();
        for row in rows {
            let pid: i32 = row.get(0);
            let username: Option<String> = row.get(1);
            let query: Option<String> = row.get(2);
            let state: Option<String> = row.get(3);
            let query_start: Option<String> = row.get(4);
            let client_addr: Option<String> = row.get(5);

            sessions.push(DbSessionInfo {
                pid,
                username,
                query,
                state,
                query_start,
                client_addr,
            });
        }
        Ok(sessions)
    }

    async fn cancel_session(&self, pid: i32) -> Result<(), DatabaseError> {
        let session = self.utility_context.open_session("utility").await?;
        let sql = format!("SELECT pg_catalog.pg_cancel_backend({})", pid);
        let stmt = session.prepare_statement(&sql).await?;
        let _ = stmt.execute_update().await?;
        Ok(())
    }

    async fn terminate_session(&self, pid: i32) -> Result<(), DatabaseError> {
        let session = self.utility_context.open_session("utility").await?;
        let sql = format!("SELECT pg_catalog.pg_terminate_backend({})", pid);
        let stmt = session.prepare_statement(&sql).await?;
        let _ = stmt.execute_update().await?;
        Ok(())
    }

    async fn list_indexes(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<IndexInfo>, DatabaseError> {
        let rows = self
            .metadata_context
            .client
            .query(
                "SELECT c.relname as index_name, \
                    tc.relname as table_name, \
                    i.indisunique as is_unique, \
                    i.indisprimary as is_primary, \
                    am.amname as index_type, \
                    pg_catalog.pg_get_expr(i.indpred, i.indrelid) as predicate, \
                    dsc.description, \
                    ARRAY( \
                      SELECT a.attname \
                      FROM pg_catalog.pg_attribute a \
                      WHERE a.attrelid = i.indrelid \
                        AND a.attnum = ANY(i.indkey) \
                      ORDER BY pg_catalog.array_position(i.indkey::int[], a.attnum) \
                    ) as column_names \
             FROM pg_catalog.pg_index i \
             JOIN pg_catalog.pg_class c ON c.oid = i.indexrelid \
             JOIN pg_catalog.pg_class tc ON tc.oid = i.indrelid \
             JOIN pg_catalog.pg_namespace n ON tc.relnamespace = n.oid \
             LEFT OUTER JOIN pg_catalog.pg_am am ON c.relam = am.oid \
             LEFT OUTER JOIN pg_catalog.pg_description dsc ON i.indexrelid = dsc.objoid \
             WHERE n.nspname = $1 AND tc.relname = $2 \
             ORDER BY c.relname",
                &[&schema, &table],
            )
            .await
            .map_err(map_db_error)?;

        let mut indexes = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            let table_name: String = row.get(1);
            let is_unique: bool = row.get(2);
            let is_primary: bool = row.get(3);
            let index_type: Option<String> = row.get(4);
            let predicate: Option<String> = row.get(5);
            let description: Option<String> = row.get(6);
            let columns: Vec<String> = row.get(7);

            indexes.push(IndexInfo {
                name,
                table_name,
                is_unique,
                is_primary,
                columns,
                index_type,
                predicate,
                description,
            });
        }
        Ok(indexes)
    }

    async fn list_constraints(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ConstraintInfo>, DatabaseError> {
        let rows = self
            .metadata_context
            .client
            .query(
                "SELECT con.conname, \
                    tc.relname as table_name, \
                    con.contype::text, \
                    pg_catalog.pg_get_constraintdef(con.oid) as definition, \
                    d.description, \
                    ARRAY( \
                      SELECT a.attname \
                      FROM pg_catalog.pg_attribute a \
                      WHERE a.attrelid = con.conrelid \
                        AND a.attnum = ANY(con.conkey) \
                      ORDER BY pg_catalog.array_position(con.conkey::int[], a.attnum) \
                    ) as column_names \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class tc ON tc.oid = con.conrelid \
             JOIN pg_catalog.pg_namespace n ON tc.relnamespace = n.oid \
             LEFT OUTER JOIN pg_catalog.pg_description d ON d.objoid = con.oid \
             WHERE n.nspname = $1 AND tc.relname = $2 \
             ORDER BY con.conname",
                &[&schema, &table],
            )
            .await
            .map_err(map_db_error)?;

        let mut constraints = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            let table_name: String = row.get(1);
            let contype_str: String = row.get(2);
            let definition: String = row.get(3);
            let description: Option<String> = row.get(4);
            let columns: Vec<String> = row.get(5);

            let constraint_type = match contype_str.as_str() {
                "p" => ConstraintType::PrimaryKey,
                "u" => ConstraintType::Unique,
                "c" => ConstraintType::Check,
                "x" => ConstraintType::Exclusion,
                _ => ConstraintType::ForeignKey,
            };

            constraints.push(ConstraintInfo {
                name,
                table_name,
                constraint_type,
                columns,
                definition,
                description,
            });
        }
        Ok(constraints)
    }

    async fn list_sequences(&self, schema: &str) -> Result<Vec<SequenceInfo>, DatabaseError> {
        let rows = self
            .metadata_context
            .client
            .query(
                "SELECT c.relname as name, \
                    n.nspname as schema, \
                    pg_catalog.format_type(s.seqtypid, null) as data_type, \
                    s.seqstart as start_value, \
                    s.seqmin as min_value, \
                    s.seqmax as max_value, \
                    s.seqincrement as increment, \
                    d.description \
             FROM pg_catalog.pg_sequence s \
             JOIN pg_catalog.pg_class c ON s.seqrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
             LEFT OUTER JOIN pg_catalog.pg_description d ON c.oid = d.objoid \
             WHERE n.nspname = $1 \
             ORDER BY c.relname",
                &[&schema],
            )
            .await
            .map_err(map_db_error)?;

        let mut sequences = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            let schema: String = row.get(1);
            let data_type: String = row.get(2);
            let start_value: i64 = row.get(3);
            let min_value: i64 = row.get(4);
            let max_value: i64 = row.get(5);
            let increment: i64 = row.get(6);
            let description: Option<String> = row.get(7);

            sequences.push(SequenceInfo {
                name,
                schema,
                data_type,
                start_value,
                min_value,
                max_value,
                increment,
                description,
            });
        }
        Ok(sequences)
    }

    async fn list_procedures(&self, schema: &str) -> Result<Vec<ProcedureInfo>, DatabaseError> {
        let rows = self
            .metadata_context
            .client
            .query(
                "SELECT p.proname as name, \
                    n.nspname as schema, \
                    pg_catalog.pg_get_function_arguments(p.oid) as argument_types_str, \
                    pg_catalog.format_type(p.prorettype, null) as return_type, \
                    p.prosrc as definition, \
                    d.description \
             FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON p.pronamespace = n.oid \
             LEFT OUTER JOIN pg_catalog.pg_description d ON p.oid = d.objoid \
             WHERE n.nspname = $1 \
             ORDER BY p.proname",
                &[&schema],
            )
            .await
            .map_err(map_db_error)?;

        let mut procedures = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            let schema: String = row.get(1);
            let argument_types_str: String = row.get(2);
            let return_type: String = row.get(3);
            let definition: String = row.get(4);
            let description: Option<String> = row.get(5);

            let argument_types = argument_types_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            procedures.push(ProcedureInfo {
                name,
                schema,
                argument_types,
                return_type,
                definition,
                description,
            });
        }
        Ok(procedures)
    }

    async fn list_extensions(&self) -> Result<Vec<ExtensionInfo>, DatabaseError> {
        let rows = self
            .metadata_context
            .client
            .query(
                "SELECT e.extname as name, \
                    e.extversion as version, \
                    n.nspname as schema, \
                    pg_catalog.obj_description(e.oid, 'pg_extension') as description \
             FROM pg_catalog.pg_extension e \
             JOIN pg_catalog.pg_namespace n ON e.extnamespace = n.oid \
             ORDER BY e.extname",
                &[],
            )
            .await
            .map_err(map_db_error)?;

        let mut extensions = Vec::new();
        for row in rows {
            let name: String = row.get(0);
            let version: String = row.get(1);
            let schema: String = row.get(2);
            let description: Option<String> = row.get(3);

            extensions.push(ExtensionInfo {
                name,
                version,
                schema,
                description,
            });
        }
        Ok(extensions)
    }

    async fn get_view_ddl(&self, schema: &str, view: &str) -> Result<String, DatabaseError> {
        let rows = self
            .metadata_context
            .client
            .query(
                "SELECT pg_catalog.pg_get_viewdef(c.oid, true) as definition \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
             WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v', 'm')",
                &[&schema, &view],
            )
            .await
            .map_err(map_db_error)?;

        if let Some(row) = rows.first() {
            let definition: String = row.get(0);
            Ok(format!(
                "CREATE OR REPLACE VIEW {}.{} AS\n{}",
                schema, view, definition
            ))
        } else {
            Err(DatabaseError::new("View not found".to_string()))
        }
    }

    async fn refresh_all(&self) -> Result<(), DatabaseError> {
        self.schema_cache.clear().await;
        self.table_cache.clear().await;
        self.column_cache.clear().await;
        self.graph_cache.clear().await;
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
