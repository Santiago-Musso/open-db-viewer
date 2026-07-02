# DBeaver PostgreSQL Refactor Roadmap & Implementation Tickets

This document defines the implementation tickets required to refactor the `open-db-viewer` PostgreSQL query pipeline to match the architecture of **DBeaver** (`org.jkiss.dbeaver.ext.postgresql`), based on the architectural reference in [dbeaver-postgres-architecture.md](file:///Users/santiagomusso/Proyectos/open-db-viewer/docs/dbeaver-postgres-architecture.md).

---

## ARCHITECTURAL MAPPING: DBEAVER TO RUST

To implement DBeaver's design principles in a Rust-native framework, we map Java-specific abstractions to idiomatic Rust abstractions:

| DBeaver Concept | Java Core Class / Interface | Proposed Rust Module / Abstraction |
| :--- | :--- | :--- |
| **Layer 1: Core Interfaces** | `DBPDataSource`, `DBCExecutionContext`, `DBCSession`, `DBCStatement` | Generic traits in `driver-api`: `DataSource`, `ExecutionContext`, `DbSession`, `DbStatement`, `DbResultSet` |
| **Layer 2: JDBC Bridge** | `JDBCDataSource`, `JDBCExecutionContext`, `JDBCSession`, `JDBCResultSet` | Standard implementation details wrapped by postgres-specific layers |
| **Layer 3: SQL Dialect Engine** | `SQLDialect`, `SQLQuery`, `SQLSyntaxManager` | `SqlDialect` trait (`driver-api`) & `PostgreDialect` (`driver-postgres`) |
| **Layer 4: Postgres Extension** | `PostgreDataSource`, `PostgreExecutionContext`, `PostgreTable` | Implementations of `DataSource` & `ExecutionContext` in `driver-postgres` |
| **Multi-context Connections** | Separate `JDBCExecutionContext` instances (Main, Metadata, Utility) | Pool of distinct `tokio_postgres::Client` contexts within `PostgresDriver` |
| **Lazy Metadata Cache** | `JDBCObjectLookupCache<Owner, ObjectType>` | Thread-safe `ObjectLookupCache<K, V>` in `driver-postgres` using lazy loading |
| **Custom Value Decoding** | `DBDValueHandler`, `PostgreValueHandler` | Extensible `ValueHandler` registry mapping Pg OIDs to `serde_json::Value` |

---

# IMPLEMENTATION TICKETS

---

## TICKET 1: Core Database Trait Abstractions
### **Goal**
Decouple the monolithic `RelationalDriver` trait into modular JDBC-like traits inside `driver-api` representing connection sources, runtime execution contexts, statements, and streaming result sets. This establishes Layer 1 of the architecture.

### **Current State**
In [crates/driver-api/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-api/src/lib.rs#L81-L96), the `RelationalDriver` trait acts as a monolithic interface for metadata retrieval (`list_schemas`, `list_tables`, etc.) and execution (`execute_query_stream`, `cancel_query`). It lacks abstractions for active sessions, transactions, search paths, or statements.

### **Proposed Changes**
Modify [crates/driver-api/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-api/src/lib.rs):
1. Define the following new traits:
   - `DataSource`: Responsible for creating execution contexts and managing server information/feature checks.
   - `ExecutionContext`: Tracks current active schema/database state, search paths, transaction status, and instantiates sessions.
   - `DbSession`: Represents an active execution block, logging, and monitoring state.
   - `DbStatement`: Prepares SQL commands, manages execution parameters, timeouts, and query limits.
   - `DbResultSet`: Wraps raw rows, exposes metadata, and streams decoded values.
2. Refactor `RelationalDriver` to be thin or implemented in terms of these traits.

### **Implementation Steps**
1. Define the new traits with async signatures mirroring JDBC-like behavior.
2. Add a `DbColumnMetadata` struct in `driver-api` containing data type OIDs, precision, and names.
3. Update `driver_api::RelationalDriver` to delegate to these traits so that existing drivers do not break immediately.

### **DBeaver Source Code Reference**
When in doubt about interfaces, browse:
- `org.jkiss.dbeaver.model.exec.DBCSession`
- `org.jkiss.dbeaver.model.exec.DBCStatement`
- `org.jkiss.dbeaver.model.exec.DBCResultSet`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.model/src/org/jkiss/dbeaver/model/exec/`

### **Verification Plan**
- **Automated Tests**: Add unit tests in `driver-api` verifying mock implementations of `DataSource` and `ExecutionContext`.
- **Compile Check**: Run `cargo check --workspace` to ensure that `driver-redis` and existing `ConnectionManager` modules compile cleanly.

---

## TICKET 2: Multi-Context Connection Management
### **Goal**
Refactor the PostgreSQL driver connection management to support multiple active connections (contexts) per active database profile (Main User context, Background Metadata context, and Utility/Cancellation context). This prevents user queries from blocking sidebar metadata retrieval or query cancellation.

### **Current State**
In [crates/driver-postgres/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-postgres/src/lib.rs#L14-L54), the `PostgresDriver` holds a single `tokio_postgres::Client` connection. Any long-running query prevents metadata fetching (like loading table columns) and makes cancellation difficult since the channel is occupied.

### **Proposed Changes**
Modify [crates/driver-postgres/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-postgres/src/lib.rs):
1. Split `PostgresDriver` into separate files: create `connection.rs` to handle connections and context management.
2. Create a `PostgresExecutionContext` struct that wraps a `tokio_postgres::Client`.
3. In `PostgresDriver`, maintain a pool or map of execution contexts:
   - `main_context`: Dedicated for running user-submitted queries.
   - `metadata_context`: Dedicated for background metadata inspection (loading tables, DDL, schemas).
   - `utility_context`: Dedicated for execution plans (`EXPLAIN`), cancellations, or transactional control.
4. Track context state: active schema name, active user, and parsed `search_path` array. On context activation, run bootstrap scripts (`SET search_path = ...`).

### **Implementation Steps**
1. Implement a connection manager within `PostgresDriver` to open three separate socket connections (with identical credentials) to the Postgres database.
2. Direct all metadata queries (`list_schemas`, `list_tables`, etc.) to the `metadata_context`.
3. Direct user SQL queries to the `main_context`.
4. Ensure `cancel_query` uses the `utility_context` (or out-of-band tokio-postgres cancellation tokens).

### **DBeaver Source Code Reference**
When in doubt about context routing, browse:
- `org.jkiss.dbeaver.model.impl.jdbc.struct.JDBCExecutionContext`
- `org.jkiss.dbeaver.ext.postgresql.model.PostgreExecutionContext`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.ext.postgresql/src/org/jkiss/dbeaver/ext/postgresql/model/`

### **Verification Plan**
- **Manual Verification**: Run a long-running query (e.g., `SELECT pg_sleep(30);`) in the user editor, and simultaneously click to reload the schema list in the sidebar. The sidebar should reload immediately without blocking.

---

## TICKET 3: SQL Dialect Abstraction & Query Transformer
### **Goal**
Create a dialect abstraction layer to manage database-specific escaping, identifier quoting rules, and SQL query transformations (such as SQL-level LIMIT/OFFSET injection), rather than hardcoding string manipulation in the executor.

### **Current State**
In [crates/driver-postgres/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-postgres/src/lib.rs#L354-L360), query transformation is hardcoded: it checks if the query starts with `select` or `with` and formats a subquery wrapper: `SELECT * FROM ({}) AS _odv_wrapper LIMIT {} OFFSET {}`. It does not support advanced quoting or escaping.

### **Proposed Changes**
1. Create `dialect.rs` in `crates/driver-postgres/src/`.
2. Define a `SqlDialect` trait in `driver-api` with functions:
   - `quote_identifier(ident: &str) -> String` (quotes identifiers in double-quotes, respecting schema-qualification).
   - `escape_string_literal(val: &str) -> String` (escapes string quotes, respects `standard_conforming_strings`).
   - `get_type_cast_clause(column_type: &str) -> Option<&str>` (maps JSON/XML/enums to `::text` when used in dynamic conditions).
3. Implement `PostgreDialect` implementing `SqlDialect`.
4. Implement a `QueryTransformer` struct that parses the input SQL (detecting dollar-quotes and block comments) and inserts appropriate paging parameters.

### **Implementation Steps**
1. Implement identifier quoting logic: if the identifier contains a dot (e.g. `public.users`), split and quote both parts (e.g. `"public"."users"`).
2. Implement standard and PostgreSQL-specific escaping (including dollar quote token scanning: `$$` or `$tag$`).
3. Refactor query wrapping to use the `PostgreDialect` instead of inline string manipulation.

### **DBeaver Source Code Reference**
When in doubt, browse:
- `org.jkiss.dbeaver.model.sql.SQLDialect`
- `org.jkiss.dbeaver.ext.postgresql.model.PostgreDialect`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.model.sql/src/org/jkiss/dbeaver/model/sql/`

### **Verification Plan**
- **Automated Tests**: Write unit tests in `driver-postgres` checking combinations of identifiers (e.g., lowercase, uppercase, schema-qualified, containing spaces) and verifying that the dialect quotes them correctly. Check dollar-quoted strings.

---

## TICKET 4: Lazy Metadata Caching Hierarchy
### **Goal**
Implement a lazy-loading metadata caching structure modeled after DBeaver's `JDBCObjectLookupCache`. Metadata (schemas, tables, columns, constraints, indexes) should only be fetched from the database on first access and cached in memory. Provide explicit API methods to invalidate/refresh specific cache branches.

### **Current State**
In [crates/driver-postgres/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-postgres/src/lib.rs#L187-L247), the functions `list_schemas`, `list_tables`, and `describe_table` make raw SQL calls to information_schema every time they are called. There is no cache layer.

### **Proposed Changes**
1. Create `metadata.rs` in `crates/driver-postgres/src/` to hold caching structures.
2. Define a thread-safe `ObjectLookupCache<K, V>` using locks or concurrent maps (e.g., `dashmap` or `tokio::sync::RwLock`).
3. Set up the metadata cache hierarchy:
   - Database-level Cache: stores schemas and global data types.
   - Schema-level Cache: stores tables, indexes, and custom types inside each schema.
   - Table-level Cache: stores columns, constraints (primary/foreign keys).
   - Expose invalidation APIs: `refresh_schema(schema_name)` and `refresh_table(schema_name, table_name)`.

### **Implementation Steps**
1. Implement the generic cache lookup pattern:
   - `get_or_load(key, loader_fn)`: if the key is in the map, return the cached value; otherwise, run the loader function (which queries the DB), insert it, and return.
2. Refactor `list_schemas`, `list_tables`, and `describe_table` to route requests through this hierarchy.
3. Expose a refresh command to the Tauri command layer so users can reload specific nodes in the UI.

### **DBeaver Source Code Reference**
When in doubt, browse:
- `org.jkiss.dbeaver.model.impl.jdbc.cache.JDBCObjectLookupCache`
- `org.jkiss.dbeaver.model.impl.jdbc.cache.JDBCStructCache`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.model/src/org/jkiss/dbeaver/model/impl/jdbc/cache/`

### **Verification Plan**
- **Automated Tests**: Mock the execution contexts to count the number of database queries executed. Verify that calling `list_tables` three times in succession results in only one actual database query, and that calling `refresh` forces a second database query.

---

## TICKET 5: Search-Path Aware Data Type Resolution & Value Handlers
### **Goal**
Implement PostgreSQL-specific type discovery and specialized value handlers for reading and writing columns. Enable type resolution that dynamically respects PostgreSQL's active `search_path`.

### **Current State**
In [crates/driver-postgres/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-postgres/src/lib.rs#L72-L183), the value mapping `pg_value_to_json` matches static type OIDs. It does not load custom enum definitions or query catalog structures to resolve custom/user-defined types, domains, or geometric structures.

### **Proposed Changes**
1. Create `types.rs` in `crates/driver-postgres/src/`.
2. Load all system and custom types from `pg_type` during connection initialization and cache them globally by OID.
3. Implement a type resolution algorithm:
   - Look up type OID in the cache.
   - If not found or if resolved by name, check the catalog `pg_catalog`.
   - Iterate schemas in the active context's `search_path` to resolve unqualified custom type names.
4. Implement specialized `ValueDecoder` modules:
   - Array Decoder: reads array type metadata, parses delimiters, and recursively decodes child elements using their base type decoder.
   - Enum Decoder: resolves enum strings from the `pg_enum` catalog.
   - Composite / HStore / JSON decoders.

### **Implementation Steps**
1. Add a startup query to read type definitions from `pg_catalog.pg_type` (joining `pg_namespace` and `pg_description`).
2. Replace `pg_value_to_json` with an extensible mapping engine that calls the registered `ValueDecoder` for the column OID.
3. Support decoding PostgreSQL numeric arrays and text arrays safely.

### **DBeaver Source Code Reference**
When in doubt, browse:
- `org.jkiss.dbeaver.model.data.DBDValueHandler`
- `org.jkiss.dbeaver.ext.postgresql.model.data.PostgreValueHandler`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.ext.postgresql/src/org/jkiss/dbeaver/ext/postgresql/model/data/`

### **Verification Plan**
- **Manual Verification**: Create a test database containing a custom enum (e.g. `CREATE TYPE mood AS ENUM ('sad', 'ok', 'happy');`), a custom array of enums, and a JSONB column. Insert values and query the table to verify they render as structured JSON strings rather than raw bytes or default placeholders.

---

## TICKET 6: Advanced SQL Error Processing & Positioning
### **Goal**
Improve connection error diagnostics by parsing Postgres-specific SQLState codes into structured error categories, and extract the exact character offset of syntax errors to highlight them directly in the frontend SQL editor.

### **Current State**
Errors from query execution are captured as simple string outputs via `.map_err(|e| e.to_string())` (e.g. [crates/driver-postgres/src/lib.rs#L362](file:///Users/santiagomusso/Proyectos/open-db-viewer/crates/driver-postgres/src/lib.rs#L362)). The UI receives a plain string and cannot highlight where the syntax error is.

### **Proposed Changes**
1. Create `error.rs` in `crates/driver-postgres/src/`.
2. Define a `DatabaseError` struct containing:
   - `message`: User-friendly message.
   - `sql_state`: The 5-character Postgres SQLState code.
   - `error_type`: An enum representing classification (`ExecutionCanceled`, `ConnectionLost`, `TransactionAborted`, `SyntaxError`, `ConstraintViolation`).
   - `position`: Option containing character offset `usize`.
3. Extract error details from `tokio_postgres::error::DbError` (utilizing the `position()` and `code()` APIs).

### **Implementation Steps**
1. Write a helper function `map_postgres_error(err: tokio_postgres::Error) -> DatabaseError`.
2. Map standard error states:
   - `57014` -> `ErrorType::ExecutionCanceled`
   - `57P01` -> `ErrorType::ConnectionLost`
   - `42601` -> `ErrorType::SyntaxError`
   - `23505` -> `ErrorType::UniqueViolation`
3. Map the error position (if any) and translate it to line/column based on the query string.

### **DBeaver Source Code Reference**
When in doubt, browse:
- `org.jkiss.dbeaver.ext.postgresql.model.PostgreDataSource.discoverErrorType`
- `org.jkiss.dbeaver.ext.postgresql.model.PostgreDataSource.getErrorPosition`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.ext.postgresql/src/org/jkiss/dbeaver/ext/postgresql/model/PostgreDataSource.java`

### **Verification Plan**
- **Automated Tests**: Write tests passing invalid SQL queries (e.g., `SELECT * FROMM users;`) and assert that the returned error contains `ErrorType::SyntaxError` and a position pointing to the double-M in `FROMM`.

---

## TICKET 7: Query Execution Plan Visualizer (EXPLAIN Plan)
### **Goal**
Add support for running `EXPLAIN` on user queries and parsing the JSON execution plan into a node tree that can be rendered graphically in the frontend.

### **Current State**
There is no feature in the backend or frontend to request, parse, or visualize query execution plans.

### **Proposed Changes**
1. Create `plan.rs` in `crates/driver-postgres/src/`.
2. Implement a method `get_execution_plan(sql: &str) -> Result<ExecutionPlan, String>` in the relational driver.
3. Prepend the query with `EXPLAIN (FORMAT JSON, ANALYZE, VERBOSE)` and execute it.
4. Define Rust serialization structs representing Postgres execution plan nodes (e.g., Node Type, Relation Name, Startup Cost, Total Cost, Plan Rows, Plan Width, Actual Startup Time, Actual Loops).
5. Parse the returned JSON array into a tree structure.

### **Implementation Steps**
1. Read the JSON string returned by the EXPLAIN query.
2. Deserialize the complex plan tree using `serde`.
3. Return the plan root node to the frontend.

### **DBeaver Source Code Reference**
When in doubt, browse:
- `org.jkiss.dbeaver.ext.postgresql.model.plan.PostgreQueryPlanner`
- `org.jkiss.dbeaver.ext.postgresql.model.plan.PostgrePlanNodeBase`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.ext.postgresql/src/org/jkiss/dbeaver/ext/postgresql/model/plan/`

### **Verification Plan**
- **Automated Tests**: Run an EXPLAIN query on a basic select. Verify that the response contains a root node of type `Seq Scan` or `Index Scan` with costs greater than zero.

---

## TICKET 8: Active Session Monitoring & Control
### **Goal**
Add a session management view that allows database administrators to inspect active backend sessions, query status, and cancel or terminate backend sessions directly from the UI.

### **Current State**
There are no administrator tools or session monitoring commands in the backend.

### **Proposed Changes**
1. Create `session.rs` in `crates/driver-postgres/src/`.
2. Define a `DbSessionInfo` struct:
   - `pid`: Process identifier.
   - `username`: Active user.
   - `query`: Current active query.
   - `state`: e.g. "active", "idle", "idle in transaction".
   - `query_start`: Timestamp query started.
   - `client_addr`: Client IP address.
3. Implement commands:
   - `list_active_sessions() -> Result<Vec<DbSessionInfo>, String>`: queries `pg_catalog.pg_stat_activity`.
   - `cancel_session(pid: i32) -> Result<(), String>`: executes `SELECT pg_cancel_backend($1)`.
   - `terminate_session(pid: i32) -> Result<(), String>`: executes `SELECT pg_terminate_backend($1)`.

### **Implementation Steps**
1. Implement the SQL query against `pg_stat_activity`, filtering out background worker processes.
2. Expose the cancel and terminate commands via Tauri command handlers.

### **DBeaver Source Code Reference**
When in doubt, browse:
- `org.jkiss.dbeaver.ext.postgresql.model.session.PostgreSessionManager`
- `org.jkiss.dbeaver.ext.postgresql.model.session.PostgreSession`
- Repository search path: `dbeaver/dbeaver` inside `plugins/org.jkiss.dbeaver.ext.postgresql/src/org/jkiss/dbeaver/ext/postgresql/model/session/`

### **Verification Plan**
- **Manual Verification**: Open two separate connection instances. From the session view in the first, confirm you can see the process PID of the second connection. Trigger a cancellation and verify that the active operation terminates.

---

## TICKET 9: Tauri Command Bridge & Svelte UI Integration
### **Goal**
Update the Tauri command handlers to expose the new abstractions, and modify the Svelte frontend to support error position highlighting, cached metadata node refreshes, query plans, and session monitoring.

### **Current State**
- [src-tauri/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/src-tauri/src/lib.rs) commands directly reference the old monolithic driver signatures.
- [src/lib/state.svelte.ts](file:///Users/santiagomusso/Proyectos/open-db-viewer/src/lib/state.svelte.ts) and [src/routes/+page.svelte](file:///Users/santiagomusso/Proyectos/open-db-viewer/src/routes/+page.svelte) have no UI views or actions for query planning, active sessions, metadata clearing, or error position markings.

### **Proposed Changes**
1. Update commands in [src-tauri/src/lib.rs](file:///Users/santiagomusso/Proyectos/open-db-viewer/src-tauri/src/lib.rs):
   - Expose `get_execution_plan`, `list_active_sessions`, `cancel_session`, `terminate_session`, and `refresh_metadata_cache`.
   - Ensure errors returned are structured json representations of `DatabaseError`.
2. Modify [src/lib/state.svelte.ts](file:///Users/santiagomusso/Proyectos/open-db-viewer/src/lib/state.svelte.ts):
   - Add state nodes for active sessions and the current query plan.
   - Track syntax error details and editor offsets.
3. Modify [src/routes/+page.svelte](file:///Users/santiagomusso/Proyectos/open-db-viewer/src/routes/+page.svelte):
   - Add an **Explain Plan** tab alongside the query result table. Render the plan tree visually or as an indented list.
   - Add a **Session Manager** view showing active connections and action buttons (Cancel/Kill).
   - In the SQL editor, use error offsets to draw squiggly underlines or mark the line of failure.
   - Add a "Refresh" context menu option on metadata tree nodes (schemas/tables) to call cache invalidation.

### **Implementation Steps**
1. Add Tauri commands for plan and session endpoints.
2. In the Svelte frontend, parse structured error packages. Use Monaco / CodeMirror editor markers (or native textbox cursor positioning) to select or highlight text at the error offset.
3. Design a responsive tree component or card component to display EXPLAIN plans visually.

### **Verification Plan**
- **End-to-End Verification**: Run the entire app. Connect to a local PostgreSQL instance, execute invalid SQL statements to verify red underline highlights, check execution plans, inspect sessions, and refresh metadata dynamically.
