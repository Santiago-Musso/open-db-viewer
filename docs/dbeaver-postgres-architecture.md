# DBeaver PostgreSQL Query Pipeline — Architectural Reference

## Purpose
This document describes how DBeaver handles PostgreSQL queries end-to-end: from connection establishment through query execution, result streaming, metadata discovery, join/relationship handling, and error management. It is intended as a design reference for improving open-db-viewer's PostgreSQL query handling.

---

## 1. LAYERED ARCHITECTURE OVERVIEW

DBeaver uses a 4-layer architecture for database interaction:

```
┌─────────────────────────────────────────────────────────┐
│  Layer 4: DATABASE-SPECIFIC EXTENSION                    │
│  (org.jkiss.dbeaver.ext.postgresql)                      │
│  PostgreDataSource, PostgreDialect, PostgreSchema,       │
│  PostgreTable*, PostgreDataType, PostgreQueryBuilder     │
├─────────────────────────────────────────────────────────┤
│  Layer 3: SQL MODEL                                      │
│  (org.jkiss.dbeaver.model.sql)                           │
│  SQLDialect, SQLQuery, SQLScriptElement,                 │
│  SQLSyntaxManager, completion, parser, semantics         │
├─────────────────────────────────────────────────────────┤
│  Layer 2: JDBC BRIDGE                                    │
│  (org.jkiss.dbeaver.model.jdbc)                          │
│  JDBCDataSource, JDBCExecutionContext, JDBCSession,      │
│  JDBCStatementImpl, JDBCResultSetImpl, JDBCFactory       │
├─────────────────────────────────────────────────────────┤
│  Layer 1: CORE MODEL (Interfaces)                        │
│  (org.jkiss.dbeaver.model)                               │
│  DBPDataSource, DBCSession, DBCStatement, DBCResultSet,  │
│  DBSObject, DBSEntity, DBSDataContainer, DBPDataKind     │
└─────────────────────────────────────────────────────────┘
```

**Key design principle**: Layer 1 defines pure interfaces. Layer 2 provides JDBC implementations. Layer 3 adds SQL parsing/semantic analysis. Layer 4 specializes everything for PostgreSQL's catalog system.

---

## 2. CONNECTION LIFECYCLE

### 2.1 DataSource Initialization
```
PostgreDataSource(monitor, container)
  → super(monitor, container, new PostgreDialect())
  → initializeRemoteInstance(monitor)
      → Determine active database name from config or username
      → If showNonDefaultDB=true:
          → Open bootstrap connection
          → Read server version via bootstrap connection metadata
          → Query pg_catalog.pg_database for all databases
          → Create PostgreDatabase instance per row
      → If showNonDefaultDB=false:
          → Create single PostgreDatabase for configured database
      → databaseCache.setCache(dbList)
      → getDefaultInstance().checkInstanceConnection(monitor)
  → initialize(monitor)
      → super.initialize(monitor)  [reads JDBC metadata, initializes dialect]
      → Read server version via "SELECT version()"
      → Check pg_enum, pg_class.reltype existence
      → getDefaultInstance().cacheDataTypes(monitor, true)
```

### 2.2 Connection Opening
```
PostgreDataSource.openConnection(monitor, context, purpose)
  → Handle timezone replacement (legacy timezone mapping)
  → If multi-database mode:
      → Build new DBPConnectionConfiguration with target database
      → Generate new JDBC URL with correct database name
  → super.openConnection(monitor, context, connectionInfo, purpose)
      → getAllConnectionProperties() [driver props + internal props + user props]
      → getInternalConnectionProperties()
          → SSL config (sslmode, sslrootcert, sslcert, sslkey, sslfactory)
          → prepareThreshold=0 for PgBouncer compatibility
          → readOnly=true for read-only connections
      → substituteDriverIfNeeded() [driver substitution support]
      → createDriverInstance() [load JDBC driver class]
      → JDBCConnectionOpener.run() [actual DriverManager.getConnection()]
      → Set application_name client info
  → Handle SSL key read errors (DER→PEM conversion retry)
```

### 2.3 Execution Context
```
PostgreExecutionContext(database, purpose) extends JDBCExecutionContext
  → Maintains: searchPath[], activeSchemaId, activeUser
  → connect(monitor):
      → dataSource.openConnection() → raw JDBC Connection
      → Set transaction isolation level
      → Set auto-commit mode
      → initContextBootstrap() [run bootstrap SQL scripts]
      → initializeContextState()
          → refreshDefaults(monitor)
              → Query: SELECT current_schema(), session_user
              → Query: SHOW search_path
              → Parse search_path into schema list
              → Resolve activeSchemaId from schema name
          → setSessionRole() if configured: SET ROLE <role>
```

### 2.4 Connection Pooling Strategy
DBeaver does NOT use a traditional connection pool. Instead:
- Each `JDBCExecutionContext` holds exactly ONE raw JDBC `Connection`
- Multiple contexts can exist per database (Main, Metadata, utility)
- Contexts are created on-demand and cached per `JDBCRemoteInstance`
- Invalidation = close + reopen the single connection
- Thread safety: `StatementLock` (either `NoOpLock` for thread-safe drivers or `SingleThreadLock`)

---

## 3. QUERY EXECUTION PIPELINE

### 3.1 Session Creation
```
JDBCExecutionContext.openSession(monitor, purpose, taskTitle)
  → dataSource.createConnection(monitor, this, purpose, taskTitle)
      → new JDBCConnectionImpl(context, monitor, purpose, taskTitle)
          This wraps the raw JDBC Connection with:
          - Progress monitoring
          - Query logging (QM - Query Manager)
          - Statement factory
          - Transaction tracking
```

### 3.2 Statement Execution Flow
```
User SQL text
  → SQLSyntaxManager: parse into SQLScriptElement(s)
      - Split by statement delimiter (;)
      - Handle dollar-quotes ($$...$$)
      - Handle block comments (including nested)
      - Handle string literals with escape sequences
  → For each SQLScriptElement:
      → SQLQuery: parsed query object
          - Query type (SELECT, INSERT, UPDATE, DELETE, etc.)
          - Parameters extraction ($1, $2, ?)
          - Table/column references
      → JDBCSession.prepareStatement(sql) / createStatement()
          → JDBCStatementImpl / JDBCPreparedStatementImpl
              - Wraps java.sql.PreparedStatement
              - Sets fetch size for streaming
              - Sets query timeout
      → statement.execute() / executeQuery()
          → JDBCResultSetImpl wraps java.sql.ResultSet
```

### 3.3 Result Set Streaming
```
JDBCResultSetImpl
  → Wraps java.sql.ResultSet
  → nextRow() → resultSet.next()
  → Fetch size controlled by:
      - DBeaver preference: result set max rows
      - QueryTransformerLimit: adds LIMIT clause
      - FetchAll transformer: removes limits
  → Column metadata via JDBCResultSetMetaDataImpl
      - PostgreJdbcFactory overrides to create PostgreResultSetMetaDataImpl
      - Handles PG-specific type OIDs
  → Value reading through value handlers:
      - DBDValueHandler per data type
      - PostgreDataType maps to specific handlers
      - Special handling for: JSON/JSONB, arrays, bytea, intervals, enums
```

### 3.4 Query Transformations
```
PostgreDataSource.createQueryTransformer(type):
  - RESULT_SET_LIMIT → QueryTransformerLimit(false, true)
      Adds/modifies LIMIT clause in SQL
  - FETCH_ALL_TABLE → QueryTransformerFetchAll()
      Removes LIMIT for full table fetch
```

---

## 4. METADATA DISCOVERY (STRUCTURE READING)

### 4.1 Object Hierarchy
```
PostgreDataSource
  └── PostgreDatabase (per pg_database row)
        ├── roleCache (pg_roles)
        ├── schemaCache (pg_namespace)
        │     └── PostgreSchema
        │           ├── tableCache (pg_class WHERE relkind IN 'r','p','f')
        │           │     └── PostgreTable / PostgreTableRegular / PostgreTableForeign / PostgreTablePartition
        │           │           ├── columns (pg_attribute)
        │           │           ├── constraintCache (pg_constraint)
        │           │           │     ├── PostgreTableConstraint (PK, UNIQUE)
        │           │           │     └── PostgreTableForeignKey (FK)
        │           │           │           └── PostgreTableForeignKeyColumn
        │           │           └── indexCache (pg_index + pg_class)
        │           │                 └── PostgreIndex
        │           │                       └── PostgreIndexColumn
        │           ├── proceduresCache (pg_proc)
        │           ├── dataTypeCache (pg_type)
        │           ├── extensionCache (pg_extension)
        │           └── aggregateCache (pg_aggregate)
        ├── dataTypeCache (global, LongKeyMap by OID)
        ├── languageCache (pg_language)
        ├── collationCache (pg_collation)
        ├── tablespaceCache (pg_tablespace)
        ├── foreignDataWrapperCache (pg_foreign_data_wrapper)
        ├── foreignServerCache (pg_foreign_server)
        ├── eventTriggersCache (pg_event_trigger)
        ├── enumValueCache (pg_enum)
        └── jobCache (pgAgent jobs)
```

### 4.2 Cache Pattern (JDBCObjectLookupCache)
Every metadata collection follows this pattern:
```java
class SomeCache extends JDBCObjectLookupCache<Owner, ObjectType> {
    prepareLookupStatement(session, owner, object, objectName)
        → Builds SQL query against pg_catalog
        → If object!=null: WHERE clause for specific object
        → If object==null: load all objects
    
    fetchObject(session, owner, dbResult)
        → Constructs model object from ResultSet row
    
    getAllObjects(monitor, owner)
        → Lazy: first call triggers SQL load
        → Subsequent calls return cached objects
        → Thread-safe with monitor synchronization
}
```

### 4.3 Data Type Resolution
```
PostgreDatabase.cacheDataTypes(monitor, forceRefresh):
  → Complex query joining pg_type + pg_class + pg_description
  → Filters by relkind and typcategory
  → Creates PostgreDataType per row
  → Populates both schema-level and database-level caches
  → PostgreDataType stores: typeId, typeType, typeCategory, dataKind,
    elementTypeId (for arrays), baseTypeId (for domains), collationId, etc.

Type resolution chain:
  1. Check database-level dataTypeCache (by OID)
  2. Check catalog schema (pg_catalog) cache
  3. Check schemas in search_path order
  4. Check remaining schemas
  5. If not found: resolve via SQL query to pg_type
```

---

## 5. SQL DIALECT (PostgreDialect)

### 5.1 Keyword & Function Registration
```
PostgreDialect.initDriverSettings(session, dataSource, metaData):
  → super.initDriverSettings() [JDBC metadata keywords]
  → addExtraKeywords(): SHOW, TYPE, LATERAL, MATERIALIZED, ILIKE, etc.
  → addExtraFunctions(): 20+ categories of PG functions:
      - Aggregate, Window, Math, String, DateTime
      - Geometry, Network, LO, Admin, Range
      - Text Search, XML, JSON, Array, Info
      - Conditional, Formatting, Enum, Sequence, etc.
  → removeSQLKeyword(): LENGTH, JSON, TEXT, FORMAT, WORK
  → setUnquotedIdentCase(LOWER)
  → serverExtension.configureDialect() [server-specific tweaks]
```

### 5.2 Type Casting for Conditions
```
PostgreDialect.getTypeCastClause(attribute, expression, isInCondition):
  → For JSON/XML columns in WHERE: append "::text"
  → For enum columns in WHERE: append "::text"
  → For OID types: append "::" + fully qualified type name
  → This enables filtering on tables without explicit keys
```

### 5.3 String Handling
```
- String quotes: single quotes ('...')
- Escape character: backslash (if standard_conforming_strings=off)
- Dollar quoting: $$...$$ for function bodies (SQLDollarQuoteRule)
- PostgreEscapeStringRule: handles E'...' escape strings
- escapeScriptValue(): special handling for bit, interval, OTHER, ARRAY, STRUCT types
```

### 5.4 Identifier Quoting
```
- Default case: LOWER (unquoted identifiers are lowercase)
- Quote character: double quote (")
- Supports: schema.table.column, catalog.schema.table
- Catalog usage: DML only (can't cross-database in standard PG)
- Schema usage: ALL (fully qualified in all contexts)
```

---

## 6. FOREIGN KEY / JOIN HANDLING

### 6.1 FK Discovery
```
PostgreSchema.ConstraintCache loads FK constraints:
  → Query: pg_constraint WHERE contype = 'f'
  → Joins: pg_class (for table names), pg_namespace (for schema)
  → For each FK:
      PostgreTableForeignKey constructor:
        → Read confmatchtype (f=FULL, p=PARTIAL, s=SIMPLE)
        → Read confupdtype/confdeltype (a=NO_ACTION, r=RESTRICT, c=CASCADE, n=SET_NULL, d=SET_DEFAULT)
        → Read refnamespace (schema OID of referenced table)
        → Read confrelid (OID of referenced table)
        → Resolve refTable via database.findTable(schemaId, tableId)
      
      PostgreTableForeignKeyColumn:
        → Maps local column → referenced column
        → cacheAttributes() resolves refConstraint on referenced table
```

### 6.2 Relationship Navigation
```
DBSEntityAssociation hierarchy:
  - PostgreTableForeignKey extends PostgreTableConstraintBase
    - getAssociatedEntity() → refTable (PostgreTableBase)
    - getReferencedConstraint() → refConstraint (unique/PK on target)
    - getAttributeReferences() → List<PostgreTableForeignKeyColumn>
    - Each FK column knows its referenced column

Reverse references:
  PostgreSchema reads incoming FKs via pg_constraint.confrelid
  → Used for "References" tab in UI
```

---

## 7. QUERY EXECUTION PLANNING

### 7.1 EXPLAIN Plan
```
PostgreQueryPlaner.planQueryExecution(session, query, configuration):
  → Creates PostgreExecutionPlan
  → Executes: EXPLAIN (FORMAT JSON, ANALYZE, VERBOSE, ...) <query>
  → Parses JSON response into plan tree
  → Supports parameters: ANALYZE, VERBOSE, COSTS, SETTINGS, BUFFERS, WAL, TIMING, SUMMARY
  → Plan nodes: PostgrePlanNodeBase with attributes map
  → Serialization: JSON format for save/load
```

---

## 8. ERROR HANDLING

### 8.1 Error Type Discovery
```
PostgreDataSource.discoverErrorType(error):
  → Extract SQL state from exception
  → Map to ErrorType:
      57014 → EXECUTION_CANCELED (query canceled)
      57P01 → CONNECTION_LOST (admin shutdown)
      25P02 → TRANSACTION_ABORTED
  → Delegate to serverExtension for server-specific errors
  → Fallback to JDBCDataSource:
      HY000 → EXECUTION_CANCELED
      08xxx → CONNECTION_LOST
      23xxx → UNIQUE_KEY_VIOLATION
      28xxx → AUTHENTICATION_FAILED
```

### 8.2 Error Position Extraction
```
PostgreDataSource.getErrorPosition(monitor, context, query, error):
  → Try PSQLException.serverErrorMessage.position (via reflection)
  → Fallback: regex match on error message for "at position N"
  → Returns ErrorPosition[] with character offset into query
```

### 8.3 Query Cancellation
```
PostgreDataSource.cancelCurrentExecution(connection, thread):
  → BeanUtils.invokeObjectMethod(connection, "cancelQuery")
  → This calls org.postgresql.jdbc.PgConnection.cancelQuery()
  → Sends a cancel signal to the backend via a new connection
```

---

## 9. SESSION MANAGEMENT

### 9.1 Active Session Monitoring
```
PostgreSessionManager:
  → Query: SELECT sa.* FROM pg_catalog.pg_stat_activity sa
  → Optional filter: exclude idle sessions
  → Can cancel query: pg_cancel_backend(pid)
  → Can terminate session: pg_terminate_backend(pid)
```

---

## 10. KEY DESIGN PATTERNS FOR open-db-viewer

### 10.1 Separation of Concerns
- **Connection management** is separate from **query execution**
- **Metadata caching** is separate from **data retrieval**
- **SQL dialect** knowledge is encapsulated in a single class
- **Server-specific behavior** is delegated to PostgreServerExtension

### 10.2 Lazy Metadata Loading
- Nothing is loaded until first accessed
- Caches are populated on first `getAllObjects()` call
- Data types are cached globally AND per-schema
- Search path determines type resolution order

### 10.3 JDBC Wrapper Pattern
- Every JDBC object is wrapped: Connection→JDBCSession, Statement→JDBCStatement, ResultSet→JDBCResultSet
- Wrappers add: progress monitoring, logging, error handling, type conversion
- Factory pattern (JDBCFactory) allows per-database overrides

### 10.4 Cache Invalidation
- Per-object refresh: `refreshObject()` clears specific caches
- Full refresh: clear all caches, re-read from pg_catalog
- Schema cache invalidation cascades to table/index/constraint caches

### 10.5 Multi-Database Support
- PostgreSQL's "database" = separate JDBC connection (not just a namespace)
- Switching databases = closing + reopening connection
- `isSharedDatabase()` flag for variants that allow cross-database queries

---

## 11. CRITICAL SQL QUERIES USED

### Database listing
```sql
SELECT db.oid, db.* FROM pg_catalog.pg_database db
WHERE datallowconn AND NOT datistemplate
ORDER BY db.datname
```

### Schema listing
```sql
SELECT n.oid, n.*, pg_catalog.obj_description(n.oid, 'pg_namespace') as description
FROM pg_catalog.pg_namespace n
WHERE nspname <> 'information_schema'
AND NOT nspname LIKE 'pg_temp_%'
AND NOT nspname LIKE 'pg_toast_temp_%'
ORDER BY nspname
```

### Table listing
```sql
SELECT c.oid, c.*, pg_catalog.obj_description(c.oid, 'pg_class') as description
FROM pg_catalog.pg_class c
WHERE c.relnamespace = ? AND c.relkind IN ('r','p','f')
ORDER BY c.relname
```

### Column listing
```sql
SELECT a.attname, a.*, pg_catalog.format_type(a.atttypid, a.atttypmod) as format_type,
       pg_catalog.col_description(a.attrelid, a.attnum) as description
FROM pg_catalog.pg_attribute a
WHERE a.attrelid = ? AND a.attnum > 0 AND NOT a.attisdropped
ORDER BY a.attnum
```

### Foreign key listing
```sql
SELECT con.oid, con.*,
       connamespace AS refnamespace, confrelid
FROM pg_catalog.pg_constraint con
JOIN pg_class c ON c.oid = con.conrelid
WHERE con.contype = 'f' AND con.conrelid = ?
```

### Index listing
```sql
SELECT i.indexrelid, i.*, ix.indisunique, ix.indisprimary,
       pg_catalog.obj_description(i.indexrelid, 'pg_class') as description,
       pg_catalog.pg_get_expr(i.indexprs, i.indrelid) as pred_expr
FROM pg_catalog.pg_index ix
JOIN pg_class i ON i.oid = ix.indexrelid
WHERE ix.indrelid = ?
```

### Data type listing
```sql
SELECT t.oid, t.*, c.relkind, bt.typname as base_type_name, d.description
FROM pg_catalog.pg_type t
LEFT OUTER JOIN pg_catalog.pg_type et ON et.oid = t.typelem
LEFT OUTER JOIN pg_catalog.pg_class c ON c.oid = t.typrelid
LEFT OUTER JOIN pg_catalog.pg_description d ON t.oid = d.objoid
WHERE t.typname IS NOT NULL
AND (c.relkind IS NULL OR c.relkind = 'c')
AND (et.typcategory IS NULL OR et.typcategory <> 'C')
```

### Session listing
```sql
SELECT sa.* FROM pg_catalog.pg_stat_activity sa
WHERE sa.state IS NULL OR sa.state NOT LIKE 'idle%'
```

---

## 12. POSTGRESQL-SPECIFIC FEATURES HANDLED

1. **Dollar quoting** ($$...$$, $tag$...$tag$) for function bodies
2. **Search path** management (SET search_path)
3. **Schema-qualified** type resolution following search_path order
4. **OID pseudo-column** on tables (legacy)
5. **Array types** (_typename convention, elementTypeId)
6. **Domain types** (baseTypeId, typeMod)
7. **Enum types** (pg_enum, typcategory='E')
8. **Composite types** (typrelid → pg_class)
9. **Range types**, JSON/JSONB, XML, bytea, interval, hstore
10. **Table inheritance** (pg_inherits)
11. **Partitioned tables** (relispartition, relkind='p')
12. **Foreign tables** (relkind='f', FDW support)
13. **Materialized views**
14. **EXPLAIN (FORMAT JSON)** for query plans
15. **pg_stat_activity** for session monitoring
16. **ILIKE** operator for case-insensitive matching
17. **standard_conforming_strings** detection
18. **SET ROLE** for session-level role switching
19. **COPY FROM STDIN** for bulk data loading
20. **SSL** with client certificates, CA certs, various modes
21. **PgBouncer** compatibility (prepareThreshold=0)
22. **Multiple databases** as separate connections
23. **Error position** extraction from PSQLException
24. **Query cancellation** via PgConnection.cancelQuery()
25. **Tablespaces**, collations, extensions, languages, access methods

---

## 13. RECOMMENDATIONS FOR open-db-viewer

Based on studying DBeaver's architecture, here are the key design decisions to adopt:

1. **Interface-first design**: Define pure interfaces for DataSource, Session, Statement, ResultSet before implementing JDBC specifics
2. **Dialect abstraction**: Encapsulate all PostgreSQL-specific SQL generation in a single dialect class
3. **Lazy metadata caching**: Don't load all metadata upfront; cache on first access
4. **Cache hierarchy**: Database-level cache for types, Schema-level cache for tables/indexes/constraints
5. **JDBC wrapper layer**: Wrap raw JDBC objects to add monitoring, logging, error handling
6. **Search path awareness**: Type resolution must follow PostgreSQL's search_path
7. **Separate execution contexts**: Metadata queries should not interfere with user queries
8. **Query transformation**: Apply LIMIT/pagination at the SQL level, not just in memory
9. **Error position mapping**: Parse PostgreSQL error messages to highlight exact positions in SQL
10. **Server version awareness**: Feature detection via version checks, not assumptions
11. **Connection per database**: PostgreSQL databases are separate connections, not just namespaces
12. **Value handler per type**: Each PostgreSQL type needs specialized read/write handling
