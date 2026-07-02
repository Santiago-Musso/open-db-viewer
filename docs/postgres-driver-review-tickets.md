# PostgreSQL Driver Review — Tickets & Changes Required

Based on a thorough review of the last 9 commits (Tickets 1-9) against the DBeaver reference architecture (`dbeaver/dbeaver` repo, `devel` branch). Each ticket below identifies a gap between our implementation and DBeaver's design, with specific code references and required changes.

---

## TICKET R1: Switch Metadata Queries from `information_schema` to `pg_catalog`

### Severity: HIGH
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 178-180, 205-209, 244-249)

### Problem
All metadata queries (`list_schemas`, `list_tables`, `describe_table`) use `information_schema` views. DBeaver **never** uses `information_schema` — it queries `pg_catalog` directly. This matters because:

1. `information_schema` is slow (it performs heavy permission filtering)
2. `information_schema` does not expose PostgreSQL-specific objects: foreign tables, partitioned tables, materialized views, sequences
3. `information_schema` does not expose OIDs needed for type resolution, constraint matching, or index discovery
4. `information_schema.tables` does not distinguish partitioned tables (`relkind='p'`) from regular tables (`relkind='r'`)

### DBeaver Reference
DBeaver's `PostgreSchema.TableCache.prepareLookupStatement`:
```sql
SELECT c.oid, c.*, d.description
FROM pg_catalog.pg_class c
LEFT OUTER JOIN pg_catalog.pg_description d
  ON d.objoid=c.oid AND d.objsubid=0 AND d.classoid='pg_class'::regclass
WHERE c.relnamespace=? AND c.relkind NOT IN ('i','I','c')
```

DBeaver's column query uses `pg_attribute` + `pg_attrdef` + `pg_description`:
```sql
SELECT c.relname, a.*,
       pg_catalog.pg_get_expr(ad.adbin, ad.adrelid, true) as def_value,
       dsc.description
FROM pg_catalog.pg_attribute a
INNER JOIN pg_catalog.pg_class c ON (a.attrelid=c.oid)
LEFT OUTER JOIN pg_catalog.pg_attrdef ad ON (a.attrelid=ad.adrelid AND a.attnum = ad.adnum)
LEFT OUTER JOIN pg_catalog.pg_description dsc ON (c.oid=dsc.objoid AND a.attnum = dsc.objsubid)
WHERE NOT a.attisdropped AND c.relkind NOT IN ('i','I','c')
ORDER BY a.attnum
```

### Required Changes
1. Replace `list_schemas` query with:
```sql
SELECT n.oid, n.nspname, pg_catalog.obj_description(n.oid, 'pg_namespace') as description
FROM pg_catalog.pg_namespace n
WHERE nspname NOT LIKE 'pg_%'
  AND nspname <> 'information_schema'
ORDER BY n.nspname
```

2. Replace `list_tables` query with:
```sql
SELECT c.relname, c.relkind::text
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
WHERE n.nspname = $1
  AND c.relkind IN ('r', 'p', 'f', 'v', 'm')
ORDER BY c.relname
```

3. Replace `describe_table` column query with:
```sql
SELECT a.attname,
       pg_catalog.format_type(a.atttypid, a.atttypmod) as data_type,
       a.attnotnull as not_null,
       pg_catalog.pg_get_expr(ad.adbin, ad.adrelid, true) as default_value,
       d.description
FROM pg_catalog.pg_attribute a
JOIN pg_catalog.pg_class c ON a.attrelid = c.oid
JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
LEFT OUTER JOIN pg_catalog.pg_attrdef ad ON a.attrelid = ad.adrelid AND a.attnum = ad.adnum
LEFT OUTER JOIN pg_catalog.pg_description d ON c.oid = d.objoid AND a.attnum = d.objsubid
WHERE n.nspname = $1 AND c.relname = $2
  AND a.attnum > 0 AND NOT a.attisdropped
ORDER BY a.attnum
```

4. Update `ColumnInfo` struct in `driver-api` to include:
```rust
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub is_nullable: bool,
    pub default_value: Option<String>,
    pub description: Option<String>,
    pub type_oid: u32,
}
```

---

## TICKET R2: Enrich `TableInfo` with Table Kind and OID

### Severity: HIGH
### Affected Files
- `crates/driver-api/src/lib.rs` (lines 39-43)
- `crates/driver-postgres/src/lib.rs` (lines 198-234)

### Problem
`TableInfo` only has `schema` and `name`. DBeaver's `PostgreTableBase` tracks:
- OID (required for constraint/index/FK lookups)
- Table kind: regular (`r`), partitioned (`p`), foreign (`f`), view (`v`), materialized view (`m`)
- Description (from `pg_description`)
- Owner, persistence (permanent/temporary/unlogged), partition status

### Required Changes
```rust
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub oid: u64,
    pub table_kind: TableKind,
    pub description: Option<String>,
}

pub enum TableKind {
    Regular,
    Partitioned,
    Foreign,
    View,
    MaterializedView,
}
```

The `list_tables` query must return `c.oid`, `c.relkind`, and `pg_catalog.obj_description(c.oid, 'pg_class')`.

---

## TICKET R3: Implement Proper DDL Generation Using `pg_get_*def` Functions

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 275-286)

### Problem
Current `get_table_ddl` generates a naive `CREATE TABLE` with only column names and types. It misses:
- NOT NULL constraints
- DEFAULT values
- Primary key / unique constraints
- Foreign key constraints
- Index definitions
- Check constraints
- Table-level options (tablespace, fillfactor, etc.)

### DBeaver Reference
DBeaver uses PostgreSQL's built-in DDL functions:
- `pg_catalog.pg_get_constraintdef(oid)` for constraint DDL
- `pg_catalog.pg_get_indexdef(oid)` for index DDL
- `pg_catalog.pg_get_expr(adbin, adrelid)` for default values

### Required Changes
```rust
async fn get_table_ddl(&self, schema: &str, table: &str) -> Result<String, DatabaseError> {
    // 1. Get column definitions with format_type, not_null, defaults
    // 2. Get constraints via pg_get_constraintdef
    // 3. Get indexes via pg_get_indexdef
    // 4. Assemble complete DDL
}
```

Query for constraints:
```sql
SELECT conname, pg_catalog.pg_get_constraintdef(oid) as definition
FROM pg_catalog.pg_constraint
WHERE conrelid = (SELECT c.oid FROM pg_catalog.pg_class c
                  JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
                  WHERE n.nspname = $1 AND c.relname = $2)
ORDER BY contype, conname
```

Query for indexes:
```sql
SELECT pg_catalog.pg_get_indexdef(indexrelid) as indexdef
FROM pg_catalog.pg_index
WHERE indrelid = (SELECT c.oid FROM pg_catalog.pg_class c
                  JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
                  WHERE n.nspname = $1 AND c.relname = $2)
```

---

## TICKET R4: Enrich Foreign Key Handling with Match Types and Modify Rules

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 343-388, the `get_schema_graph` FK query)
- `crates/driver-api/src/lib.rs` (lines 70-77, `SchemaEdge` struct)

### Problem
The current FK query in `get_schema_graph` has two issues:

1. **Missing FK metadata**: No match type (FULL/PARTIAL/SIMPLE), no update/delete rules (CASCADE/RESTRICT/SET_NULL/etc.)
2. **Multi-column FK bug**: Using `ANY(c.conkey)` with `ANY(c.confkey)` produces a cross-product of columns, not the correct column pairing. For a FK on `(a,b) → (x,y)`, this returns `(a,x), (a,y), (b,x), (b,y)` instead of `(a,x), (b,y)`.

### DBeaver Reference
`PostgreTableForeignKey` reads from `pg_constraint`:
- `confmatchtype`: `f`=FULL, `p`=PARTIAL, `s`=SIMPLE
- `confupdtype`/`confdeltype`: `a`=NO_ACTION, `r`=RESTRICT, `c`=CASCADE, `n`=SET_NULL, `d`=SET_DEFAULT
- `confrelid`: referenced table OID
- `conkey`/`confkey`: arrays of column numbers (matched by position, not cross-joined)

### Required Changes
1. Fix FK column pairing using `unnest` with ordinality:
```sql
SELECT con.conname,
       cl1.relname as src_table,
       a1.attname as src_column,
       cl2.relname as tgt_table,
       a2.attname as tgt_column,
       con.confmatchtype,
       con.confupdtype,
       con.confdeltype
FROM pg_catalog.pg_constraint con
JOIN pg_catalog.pg_class cl1 ON con.conrelid = cl1.oid
JOIN pg_catalog.pg_class cl2 ON con.confrelid = cl2.oid
JOIN pg_catalog.pg_namespace n1 ON cl1.relnamespace = n1.oid
CROSS JOIN LATERAL unnest(con.conkey) WITH ORDINALITY AS src(attnum, ord)
CROSS JOIN LATERAL unnest(con.confkey) WITH ORDINALITY AS tgt(attnum, ord)
JOIN pg_catalog.pg_attribute a1 ON a1.attrelid = cl1.oid AND a1.attnum = src.attnum
JOIN pg_catalog.pg_attribute a2 ON a2.attrelid = cl2.oid AND a2.attnum = tgt.attnum
WHERE con.contype = 'f' AND n1.nspname = $1
  AND src.ord = tgt.ord
```

2. Add FK metadata to `SchemaEdge`:
```rust
pub struct SchemaEdge {
    pub id: String,
    pub source: String,
    pub source_handle: String,
    pub target: String,
    pub target_handle: String,
    pub match_type: Option<String>,     // FULL, PARTIAL, SIMPLE
    pub update_rule: Option<String>,    // CASCADE, RESTRICT, SET_NULL, SET_DEFAULT, NO_ACTION
    pub delete_rule: Option<String>,
}
```

---

## TICKET R5: Add Index Discovery and Listing

### Severity: MEDIUM
### Affected Files
- `crates/driver-api/src/lib.rs` (missing `IndexInfo` struct)
- `crates/driver-postgres/src/lib.rs` (missing `list_indexes` method)
- `crates/driver-api/src/lib.rs` (missing `list_indexes` in `RelationalDriver` trait)

### Problem
DBeaver has a full `IndexCache` per schema that discovers indexes via `pg_index` + `pg_class`. Our driver has no index discovery at all. Indexes are critical for:
- Understanding query performance
- DDL generation
- Schema documentation

### DBeaver Reference
```sql
SELECT i.*, i.indkey as keys, c.relname, c.relnamespace, c.relam, c.reltablespace,
       tc.relname as tabrelname, dsc.description,
       pg_catalog.pg_get_expr(i.indpred, i.indrelid) as pred_expr,
       pg_catalog.pg_get_expr(i.indexprs, i.indrelid, true) as expr
FROM pg_catalog.pg_index i
INNER JOIN pg_catalog.pg_class c ON c.oid=i.indexrelid
INNER JOIN pg_catalog.pg_class tc ON tc.oid=i.indrelid
LEFT OUTER JOIN pg_catalog.pg_description dsc ON i.indexrelid=dsc.objoid
WHERE i.indrelid=?
ORDER BY c.relname
```

### Required Changes
1. Add to `driver-api`:
```rust
pub struct IndexInfo {
    pub name: String,
    pub table_name: String,
    pub is_unique: bool,
    pub is_primary: bool,
    pub columns: Vec<String>,
    pub index_type: Option<String>,  // btree, hash, gist, gin, etc.
    pub predicate: Option<String>,   // partial index expression
    pub description: Option<String>,
}
```

2. Add `list_indexes` to `RelationalDriver` trait and implement it.

---

## TICKET R6: Add Constraint Discovery (PK, Unique, Check)

### Severity: MEDIUM
### Affected Files
- `crates/driver-api/src/lib.rs` (missing `ConstraintInfo` struct)
- `crates/driver-postgres/src/lib.rs` (missing `list_constraints` method)

### Problem
DBeaver's `ConstraintCache` reads all constraints from `pg_constraint`. Our driver only discovers FKs indirectly through `get_schema_graph`. We have no way to list primary keys, unique constraints, or check constraints for a table.

### DBeaver Reference
```sql
SELECT c.oid, c.*, t.relname as tabrelname, rt.relnamespace as refnamespace, d.description
FROM pg_catalog.pg_constraint c
INNER JOIN pg_catalog.pg_class t ON t.oid=c.conrelid
LEFT OUTER JOIN pg_catalog.pg_class rt ON rt.oid=c.confrelid
LEFT OUTER JOIN pg_catalog.pg_description d ON d.objoid=c.oid
WHERE t.relnamespace=?
ORDER BY c.oid
```

### Required Changes
```rust
pub struct ConstraintInfo {
    pub name: String,
    pub table_name: String,
    pub constraint_type: ConstraintType,  // PrimaryKey, Unique, Check, Exclusion
    pub columns: Vec<String>,
    pub definition: String,               // from pg_get_constraintdef
    pub description: Option<String>,
}
```

---

## TICKET R7: Complete Error Category Mapping

### Severity: HIGH
### Affected Files
- `crates/driver-api/src/error.rs` (lines 3-10)
- `crates/driver-postgres/src/connection.rs` (lines 23-29)

### Problem
Current `ErrorCategory` enum is missing critical categories that DBeaver maps:

| SQLState | DBeaver ErrorType | Our Mapping |
|----------|-------------------|-------------|
| `57014` | `EXECUTION_CANCELED` | Missing |
| `57P01` | `CONNECTION_LOST` (admin shutdown) | Missing |
| `25P02` | `TRANSACTION_ABORTED` | Missing |
| `42501` | `PERMISSION_DENIED` | Mapped via `starts_with("42")` (wrong — 42 is syntax, not permission) |
| `08xxx` | `CONNECTION_LOST` | Mapped correctly |
| `23xxx` | `UNIQUE_KEY_VIOLATION` | Mapped as `IntegrityConstraintViolation` (acceptable but imprecise) |

**Critical bug**: `28xxx` is mapped to `PermissionDenied` but it should be `AuthenticationFailed`. SQL state `28000` = "invalid authorization specification", `28P01` = "invalid password".

### DBeaver Reference
`PostgreDataSource.discoverErrorType`:
```java
if (PostgreConstants.EC_QUERY_CANCELED.equals(sqlState)) return ErrorType.EXECUTION_CANCELED;
if (PostgreConstants.ERROR_ADMIN_SHUTDOWN.equals(sqlState)) return ErrorType.CONNECTION_LOST;
if (PostgreConstants.ERROR_TRANSACTION_ABORTED.equals(sqlState)) return ErrorType.TRANSACTION_ABORTED;
```

`JDBCDataSource.discoverErrorType`:
```java
if (sqlState == "HY008") return EXECUTION_CANCELED;
if (sqlState starts with "08") return CONNECTION_LOST;
if (sqlState == "23000" || "23505") return UNIQUE_KEY_VIOLATION;
if (sqlState starts with "28") return AUTHENTICATION_FAILED;
```

### Required Changes
```rust
pub enum ErrorCategory {
    SyntaxError,           // 42xxx
    PermissionDenied,      // 42501
    AuthenticationFailed,  // 28xxx
    IntegrityConstraintViolation, // 23xxx
    UniqueKeyViolation,    // 23505
    ConnectionFailure,     // 08xxx
    ConnectionLost,        // 57P01
    ExecutionCanceled,     // 57014
    TransactionAborted,    // 25P02
    Unknown,
}
```

Fix the mapping in `connection.rs`:
```rust
let category = match sql_state.as_str() {
    "57014" => ErrorCategory::ExecutionCanceled,
    "57P01" => ErrorCategory::ConnectionLost,
    "25P02" => ErrorCategory::TransactionAborted,
    "42501" => ErrorCategory::PermissionDenied,
    s if s.starts_with("42") => ErrorCategory::SyntaxError,
    "23505" => ErrorCategory::UniqueKeyViolation,
    s if s.starts_with("23") => ErrorCategory::IntegrityConstraintViolation,
    s if s.starts_with("28") => ErrorCategory::AuthenticationFailed,
    s if s.starts_with("08") => ErrorCategory::ConnectionFailure,
    _ => ErrorCategory::Unknown,
};
```

---

## TICKET R8: Implement `standard_conforming_strings` Detection

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/dialect.rs` (lines 18-20)
- `crates/driver-postgres/src/connection.rs`

### Problem
DBeaver's `PostgreDialect.getStringEscapeCharacter()` checks if the server has `standard_conforming_strings` enabled. When OFF, backslash is an escape character (`\'` escapes a quote). When ON, only `''` escapes a quote.

Our `escape_string_literal` always uses `''` doubling, which is correct only when `standard_conforming_strings = on` (the default since PG 9.1, but configurable).

### DBeaver Reference
`PostgreDataSource.SettingCache` queries `pg_catalog.pg_settings` for `standard_conforming_strings` and caches it. `PostgreDialect.isEscapeBackslash()` delegates to `serverExtension.supportsBackslashStringEscape()`.

### Required Changes
1. On connection init, query: `SHOW standard_conforming_strings`
2. Store the result in `PostgresExecutionContext`
3. If OFF, `escape_string_literal` should also escape backslashes: `val.replace('\\', '\\\\').replace("'", "''")`
4. Alternatively, prefix with `E'...'` syntax when backslash escaping is needed

---

## TICKET R9: Fix Search Path Management

### Severity: HIGH
### Affected Files
- `crates/driver-postgres/src/connection.rs` (lines 88-96)

### Problem
`set_active_schema` replaces the entire search path with just the schema name:
```sql
SET search_path TO "schema_name"
```

DBeaver's `PostgreExecutionContext.setSearchPath()` **prepends** the new schema to the existing search path, preserving other entries. It also adds the active user to the path if not present.

This matters because:
- Users may rely on `pg_catalog` being in the search path
- Other schemas may be explicitly configured
- Replacing the path breaks access to objects in other schemas

### DBeaver Reference
```java
private void setSearchPath(DBRProgressMonitor monitor, String defSchemaName) {
    List<String> newSearchPath = new ArrayList<>(searchPath);
    int schemaIndex = newSearchPath.indexOf(defSchemaName);
    if (schemaIndex < 0) {
        newSearchPath.addFirst(defSchemaName);
    }
    // ... build comma-separated string and execute SET search_path
}
```

### Required Changes
```rust
async fn set_active_schema(&self, schema: &str) -> Result<(), DatabaseError> {
    let mut search_path = self.get_search_path().await?;
    let quoted = PostgreDialect.quote_identifier(schema);
    search_path.retain(|s| s != schema);
    search_path.insert(0, quoted);
    let path_str = search_path.join(", ");
    self.client
        .execute(&format!("SET search_path TO {}", path_str), &[])
        .await
        .map_err(map_db_error)?;
    let mut active = self.active_schema.lock().await;
    *active = schema.to_string();
    Ok(())
}
```

---

## TICKET R10: Add View and Materialized View Support

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 205-209, `list_tables` query)
- `crates/driver-api/src/lib.rs`

### Problem
The current `list_tables` query filters `table_type = 'BASE TABLE'`, which excludes views and materialized views entirely. DBeaver supports all relation kinds:
- `r` = regular table
- `v` = view
- `m` = materialized view
- `p` = partitioned table
- `f` = foreign table

### Required Changes
1. Update `list_tables` to include views and materialized views (using `pg_class` query from R1)
2. Add `get_view_ddl` method using `pg_get_viewdef`:
```sql
SELECT pg_catalog.pg_get_viewdef(c.oid, true) as definition
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('v', 'm')
```

---

## TICKET R11: Add Sequence Support

### Severity: LOW
### Affected Files
- `crates/driver-api/src/lib.rs` (missing `SequenceInfo` struct)
- `crates/driver-postgres/src/lib.rs` (missing `list_sequences` method)

### Problem
DBeaver discovers sequences via `pg_class WHERE relkind = 'S'`. Our driver has no sequence support.

### Required Changes
```rust
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
```

---

## TICKET R12: Add Function/Procedure Listing

### Severity: LOW
### Affected Files
- `crates/driver-api/src/lib.rs` (missing `ProcedureInfo` struct)
- `crates/driver-postgres/src/lib.rs` (missing `list_procedures` method)

### Problem
DBeaver's `ProceduresCache` reads from `pg_proc`. Our driver has no function/procedure discovery.

### DBeaver Reference
```sql
SELECT p.oid, p.*, pg_catalog.pg_get_expr(p.proargdefaults, 0) as arg_defaults, d.description
FROM pg_catalog.pg_proc p
LEFT OUTER JOIN pg_catalog.pg_description d ON d.objoid=p.oid
WHERE p.pronamespace=?
ORDER BY p.proname
```

---

## TICKET R13: Add Extension Listing

### Severity: LOW
### Affected Files
- `crates/driver-api/src/lib.rs` (missing `ExtensionInfo` struct)
- `crates/driver-postgres/src/lib.rs`

### Problem
DBeaver discovers installed extensions via `pg_extension`. This is important for understanding what capabilities are available (PostGIS, pg_trgm, etc.).

### Required Changes
```sql
SELECT e.oid, e.extname, e.extversion, n.nspname as schema,
       pg_catalog.obj_description(e.oid, 'pg_extension') as description
FROM pg_catalog.pg_extension e
JOIN pg_catalog.pg_namespace n ON e.extnamespace = n.oid
ORDER BY e.extname
```

---

## TICKET R14: Add Table Statistics (Size, Row Count)

### Severity: LOW
### Affected Files
- `crates/driver-api/src/lib.rs` (missing stats in `TableInfo`)
- `crates/driver-postgres/src/lib.rs`

### Problem
DBeaver's `PostgreDataSource.collectObjectStatistics` queries `pg_database_size` and `pg_relation_size`. Our driver provides no size information.

### Required Changes
```sql
SELECT pg_catalog.pg_relation_size(c.oid) as table_size,
       pg_catalog.pg_total_relation_size(c.oid) as total_size,
       c.reltuples::bigint as estimated_row_count
FROM pg_catalog.pg_class c
JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid
WHERE n.nspname = $1 AND c.relname = $2
```

---

## TICKET R15: Add EXPLAIN Options (ANALYZE, VERBOSE, BUFFERS)

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 463-495)
- `crates/driver-api/src/lib.rs` (lines 148-150)

### Problem
Current `get_execution_plan` only uses `EXPLAIN (FORMAT JSON)`. DBeaver's `PostgreQueryPlaner` supports configurable options:
- `ANALYZE` — actually execute the query and show actual rows/timing
- `VERBOSE` — show internal plan details
- `COSTS` — show cost estimates
- `BUFFERS` — show buffer usage
- `WAL` — show WAL record generation
- `TIMING` — show timing data
- `SETTINGS` — show non-default configuration

### Required Changes
1. Add configuration parameter to `get_execution_plan`:
```rust
async fn get_execution_plan(&self, sql: &str, options: ExplainOptions) -> Result<PlanNode, DatabaseError>;

pub struct ExplainOptions {
    pub analyze: bool,
    pub verbose: bool,
    pub costs: bool,
    pub buffers: bool,
    pub settings: bool,
    pub timing: bool,
}
```

2. Build EXPLAIN query dynamically:
```rust
let mut opts = vec!["FORMAT JSON"];
if options.analyze { opts.push("ANALYZE"); }
if options.verbose { opts.push("VERBOSE"); }
// ... etc
let explain_sql = format!("EXPLAIN ({}) {}", opts.join(", "), sql);
```

---

## TICKET R16: Add Transaction Management

### Severity: HIGH
### Affected Files
- `crates/driver-api/src/lib.rs` (missing transaction traits)
- `crates/driver-postgres/src/connection.rs`

### Problem
DBeaver's `JDBCExecutionContext` implements `DBCTransactionManager` with:
- `getTransactionIsolation()` / `setTransactionIsolation()`
- `isAutoCommit()` / `setAutoCommit()`
- `commit()` / `rollback()`
- Savepoint support

Our driver has **zero** transaction management. All queries run in auto-commit mode with no way to start/commit/rollback transactions.

### DBeaver Reference
`JDBCExecutionContext` manages:
```java
connection.setAutoCommit(autoCommit);
connection.setTransactionIsolation(txnLevel);
connection.commit();
connection.rollback();
connection.setSavepoint(name);
```

### Required Changes
1. Add to `driver-api`:
```rust
#[async_trait]
pub trait TransactionManager: Send + Sync {
    async fn is_auto_commit(&self) -> Result<bool, DatabaseError>;
    async fn set_auto_commit(&self, enabled: bool) -> Result<(), DatabaseError>;
    async fn commit(&self) -> Result<(), DatabaseError>;
    async fn rollback(&self) -> Result<(), DatabaseError>;
}
```

2. Implement in `PostgresExecutionContext` using tokio-postgres transaction API.

---

## TICKET R17: Fix Type Registry Query

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 88-107)

### Problem
The type registry query loads ALL types from `pg_type` without filtering:
```sql
SELECT t.oid, t.typname, n.nspname, t.typtype::text
FROM pg_catalog.pg_type t
JOIN pg_catalog.pg_namespace n ON t.typnamespace = n.oid
```

DBeaver's `PostgreDatabase.cacheDataTypes` has a much more sophisticated query that:
- Filters out array pseudo-types (`c.relkind IS NULL OR c.relkind = 'c'`)
- Filters out composite element types (`et.typcategory IS NULL OR et.typcategory <> 'C'`)
- Includes type category (`typcategory`)
- Includes base type name for domains
- Includes description from `pg_description`

### Required Changes
```sql
SELECT t.oid, t.typname, n.nspname, t.typtype::text,
       t.typcategory::text, t.typelem, t.typbasetype,
       bt.typname as base_type_name,
       pg_catalog.format_type(t.oid, t.typtypmod) as full_type_name,
       d.description
FROM pg_catalog.pg_type t
LEFT OUTER JOIN pg_catalog.pg_type et ON et.oid = t.typelem
LEFT OUTER JOIN pg_catalog.pg_class c ON c.oid = t.typrelid
LEFT OUTER JOIN pg_catalog.pg_type bt ON bt.oid = t.typbasetype
LEFT OUTER JOIN pg_catalog.pg_namespace n ON t.typnamespace = n.oid
LEFT OUTER JOIN pg_catalog.pg_description d ON t.oid = d.objoid
WHERE t.typname IS NOT NULL
  AND (c.relkind IS NULL OR c.relkind = 'c')
  AND (et.typcategory IS NULL OR et.typcategory <> 'C')
```

---

## TICKET R18: Add Missing Type Handlers

### Severity: MEDIUM
### Affected Files
- `crates/driver-postgres/src/types.rs`

### Problem
The following PostgreSQL types are not handled and fall through to the generic `<type: xxx>` placeholder:

| Type | DBeaver Handler | Notes |
|------|-----------------|-------|
| `bytea` | `JDBCContentBLOB` | Binary data, needs hex/base64 encoding |
| `interval` | `PostgreValueHandler` | PG interval (complex: years/months/days/time) |
| `inet`/`cidr` | `JDBCStringValueHandler` | Network addresses |
| `macaddr` | `JDBCStringValueHandler` | MAC addresses |
| `point`/`line`/`lseg`/`box`/`path`/`polygon`/`circle` | GIS handler | Geometric types |
| `hstore` | `PostgreValueHandler` | Key-value pairs |
| `xml` | `JDBCContentXML` | XML content |
| `money` | `JDBCNumberValueHandler` | Currency |
| `bit`/`varbit` | `JDBCStringValueHandler` | Bit strings |
| `oid` | `JDBCNumberValueHandler` | Object identifiers |
| `uuid` | Handled | Already implemented |
| `numeric` | Handled | Already implemented |
| `date`/`timestamp`/`timestamptz` | Handled | Already implemented |
| `ARRAY` (non-binary fallback) | `JDBCArrayValueHandler` | Text-format arrays `{a,b,c}` |
| Domain types | `PostgreDataType.resolveValueTypeFromBaseType` | Resolve to base type |

### Required Changes
Add handlers for at minimum: `bytea`, `interval`, `inet`/`cidr`, `xml`, `hstore`, `money`, `bit`/`varbit`, and text-format array fallback.

For `bytea`:
```rust
Type::BYTEA => {
    // Return hex-encoded string
    serde_json::Value::String(format!("\\x{}", hex::encode(raw)))
}
```

For `interval`, use `pg_interval` crate or parse the text representation.

---

## TICKET R19: Add `refresh_all` Cache Method

### Severity: LOW
### Affected Files
- `crates/driver-postgres/src/metadata.rs`
- `crates/driver-postgres/src/lib.rs`

### Problem
DBeaver's `PostgreDatabase.refreshObject()` clears ALL caches and re-reads everything. Our `refresh_schema` only clears table and graph caches for one schema. There's no way to do a full refresh.

### Required Changes
```rust
async fn refresh_all(&self) -> Result<(), DatabaseError> {
    self.schema_cache.clear().await;
    self.table_cache.clear().await;
    self.column_cache.clear().await;
    self.graph_cache.clear().await;
    Ok(())
}
```

Also add `clear()` method to `ObjectLookupCache`.

---

## TICKET R20: Add SQL Injection Protection in Metadata Queries

### Severity: HIGH
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 204-211, 242-249, 294-308, 343-358)

### Problem
Multiple metadata queries use string interpolation with `escape_string_literal` instead of parameterized queries:
```rust
let sql = format!(
    "SELECT ... WHERE table_schema = '{}' ...",
    escaped_schema
);
```

While `escape_string_literal` doubles single quotes, this is still fragile. DBeaver uses `JDBCPreparedStatement` with `?` placeholders for all metadata queries.

### Required Changes
Convert all metadata queries to use parameterized statements:
```rust
let stmt = session.prepare_statement(
    "SELECT table_schema, table_name FROM information_schema.tables \
     WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
     ORDER BY table_name"
).await?;
// Pass schema as parameter
```

Note: `tokio_postgres` supports `$1`, `$2` parameter placeholders.

---

## TICKET R21: Session Listing Should Support Idle Filtering

### Severity: LOW
### Affected Files
- `crates/driver-postgres/src/lib.rs` (lines 497-554)

### Problem
DBeaver's `PostgreSessionManager` has an `OPTION_SHOW_IDLE` flag. By default, idle sessions are filtered out. Our implementation shows ALL client backends including idle ones, which clutters the view.

### DBeaver Reference
```java
if (!CommonUtils.getOption(options, OPTION_SHOW_IDLE)) {
    sql.append(" where sa.state is null or sa.state not like 'idle%'");
}
```

### Required Changes
Add `show_idle` parameter to `list_active_sessions` and filter accordingly.

---

## TICKET R22: Add `application_name` Client Info

### Severity: LOW
### Affected Files
- `crates/driver-postgres/src/lib.rs` (connection setup)

### Problem
DBeaver sets `application_name` on the connection for identification in `pg_stat_activity`:
```java
pgConnection.setClientInfo("ApplicationName", DBUtils.getClientApplicationName(...));
```

Our driver doesn't set any client identification, making it hard to distinguish our connections in session monitoring.

### Required Changes
Add to connection string:
```
application_name=open-db-viewer
```

---

## Summary: Priority Matrix

| Ticket | Severity | Effort | Impact |
|--------|----------|--------|--------|
| R1 | HIGH | Medium | Foundation for all metadata improvements |
| R2 | HIGH | Low | Required for R3, R4, R5, R6 |
| R7 | HIGH | Low | Fixes incorrect error mapping bug |
| R9 | HIGH | Low | Fixes broken search path behavior |
| R16 | HIGH | High | Missing core feature (transactions) |
| R20 | HIGH | Medium | Security fix (SQL injection) |
| R3 | MEDIUM | Medium | DDL quality improvement |
| R4 | MEDIUM | Medium | Fixes FK cross-product bug |
| R5 | MEDIUM | Medium | Missing index discovery |
| R6 | MEDIUM | Medium | Missing constraint discovery |
| R8 | MEDIUM | Low | Edge case for old PG configs |
| R10 | MEDIUM | Low | Missing view support |
| R15 | MEDIUM | Low | EXPLAIN feature completion |
| R17 | MEDIUM | Medium | Type system accuracy |
| R18 | MEDIUM | Medium | Missing type handlers |
| R11 | LOW | Low | Sequence support |
| R12 | LOW | Medium | Function/procedure listing |
| R13 | LOW | Low | Extension listing |
| R14 | LOW | Low | Table statistics |
| R19 | LOW | Low | Full cache refresh |
| R21 | LOW | Low | Session filtering |
| R22 | LOW | Low | Connection identification |
