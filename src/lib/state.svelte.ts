import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface Connection {
  id: string;
  name: string;
  driver: "postgres" | "redis";
  host: string;
  port: number;
  user?: string;
  db_name?: string;
  password?: string;
  has_password_saved?: boolean;
}

export interface Tab {
  id: string;
  name: string;
  sql: string;
  loading: boolean;
  queryId: string | null;
  columns: { name: string; data_type: string }[];
  rows: any[][];
  error: string | null;
  executionTime: number | null; // in ms
  rowCount: number;
  offset: number;
  isFullyLoaded: boolean;
}

class AppState {
  connections = $state<Connection[]>([]);
  activeConnectionId = $state<string | null>(null);
  schemas = $state<{ name: string; tables?: { name: string; columns?: { name: string; data_type: string }[] }[] }[]>([]);
  tabs = $state<Tab[]>([]);
  activeTabId = $state<string | null>(null);
  
  // ER Diagram & Autocomplete data
  schemaEdges = $state<any[]>([]);
  schemaNodes = $state<any[]>([]);

  // Redis specific state
  redisKeys = $state<string[]>([]);
  redisCursor = $state<number>(0);
  redisPattern = $state<string>("*");
  redisActiveKey = $state<{ key: string; value: string; value_type: string; ttl: number | null } | null>(null);
  redisActiveKeyLoading = $state<boolean>(false);
  redisKeysLoading = $state<boolean>(false);
  redisServerInfo = $state<Record<string, string>>({});

  constructor() {
    // Load saved connections from Tauri config backend on init
    this.loadProfiles();
    this.loadSessionState();

    // Set up Tauri event listeners for query streaming
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      listen("query:batch", (event: any) => {
        const { query_id, batch } = event.payload;
        const tab = this.tabs.find((t) => t.queryId === query_id);
        if (tab) {
          if (tab.columns.length === 0) {
            tab.columns = batch.columns;
          }
          tab.rows.push(...batch.rows);
          tab.rowCount += (batch.rows.len || batch.rows.length);
          
          if ((batch.rows.len || batch.rows.length) < 100) {
            tab.isFullyLoaded = true;
          }
        }
      });

      listen("query:error", (event: any) => {
        const { query_id, error } = event.payload;
        const tab = this.tabs.find((t) => t.queryId === query_id);
        if (tab) {
          tab.error = error;
          tab.loading = false;
        }
      });

      listen("query:done", (event: any) => {
        const { query_id, row_count } = event.payload;
        const tab = this.tabs.find((t) => t.queryId === query_id);
        if (tab) {
          tab.loading = false;
          tab.offset += row_count;
        }
      });
    }
  }

  async loadProfiles() {
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        const profiles: any[] = await invoke("load_connection_profiles");
        this.connections = profiles.map((p) => ({
          id: p.config.id,
          name: p.name || p.config.host,
          driver: p.driver_id,
          host: p.config.host,
          port: p.config.port,
          user: p.config.user || "",
          db_name: p.config.db_name || "",
          has_password_saved: p.has_password_saved,
        }));
      } catch (e) {
        console.error("Failed to load connection profiles", e);
      }
    } else {
      const saved = localStorage.getItem("db_client_connections");
      if (saved) {
        try {
          this.connections = JSON.parse(saved);
        } catch (_) {}
      }
    }
  }

  async saveConnection(conn: Connection) {
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        await invoke("save_connection_profile", {
          driverId: conn.driver,
          name: conn.name,
          config: {
            id: conn.id,
            host: conn.host,
            port: Number(conn.port),
            user: conn.user || null,
            db_name: conn.db_name || null,
            password: conn.password || null,
          },
        });
        await this.loadProfiles();
      } catch (e) {
        console.error("Failed to save connection profile", e);
        throw e;
      }
    } else {
      const index = this.connections.findIndex((c) => c.id === conn.id);
      if (index !== -1) {
        this.connections[index] = conn;
      } else {
        this.connections.push(conn);
      }
      localStorage.setItem("db_client_connections", JSON.stringify(this.connections));
    }
  }

  async addConnection(conn: Omit<Connection, "id">) {
    const id = crypto.randomUUID();
    const newConn = { ...conn, id };
    await this.saveConnection(newConn);
    return id;
  }

  async removeConnection(id: string) {
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        await invoke("delete_connection_profile", { id });
        await this.loadProfiles();
      } catch (e) {
        console.error("Failed to delete connection profile", e);
      }
    } else {
      this.connections = this.connections.filter((c) => c.id !== id);
      localStorage.setItem("db_client_connections", JSON.stringify(this.connections));
    }
    if (this.activeConnectionId === id) {
      this.activeConnectionId = null;
      this.schemas = [];
    }
  }

  async testConnection(conn: Omit<Connection, "id"> & { id?: string }) {
    try {
      await invoke("test_connection", {
        driverId: conn.driver,
        config: {
          id: conn.id || "test-temp-id",
          host: conn.host,
          port: Number(conn.port),
          user: conn.user || null,
          db_name: conn.db_name || null,
          password: conn.password || null,
        },
      });
    } catch (e: any) {
      throw new Error(e.toString());
    }
  }

  async connect(connectionId: string) {
    const conn = this.connections.find((c) => c.id === connectionId);
    if (!conn) return;

    try {
      await invoke("connect_db", {
        driverId: conn.driver,
        config: {
          id: conn.id,
          host: conn.host,
          port: conn.port,
          user: conn.user || null,
          db_name: conn.db_name || null,
          password: conn.password || null,
        },
      });

      this.activeConnectionId = connectionId;

      if (conn.driver === "redis") {
        this.redisKeys = [];
        this.redisCursor = 0;
        this.redisPattern = "*";
        this.redisActiveKey = null;
        await this.loadRedisKeys(0, "*");
        await this.loadRedisServerInfo();
      } else {
        this.schemas = [];
        if (this.tabs.length === 0) {
          this.openNewTab();
        } else {
          this.saveSessionState();
        }
        await this.loadSchemas();
      }
    } catch (e: any) {
      throw new Error(e.toString());
    }
  }

  async disconnect() {
    if (!this.activeConnectionId) return;
    try {
      await invoke("disconnect_db", { connectionId: this.activeConnectionId });
    } catch (_) {}
    this.activeConnectionId = null;
    this.schemas = [];
    this.tabs = [];
    this.activeTabId = null;
    this.redisKeys = [];
    this.redisCursor = 0;
    this.redisActiveKey = null;
    this.redisServerInfo = {};
    this.schemaEdges = [];
    this.schemaNodes = [];
    console.log("Global state reset completed.");
    this.saveSessionState();
  }

  async loadRedisKeys(cursor = 0, pattern = "*") {
    if (!this.activeConnectionId) return;
    this.redisKeysLoading = true;
    try {
      const res: any = await invoke("redis_scan_keys", {
        connectionId: this.activeConnectionId,
        pattern,
        cursor,
        count: 100,
      });
      if (cursor === 0) {
        this.redisKeys = res.keys;
      } else {
        // Filter duplicates
        const uniqueKeys = new Set([...this.redisKeys, ...res.keys]);
        this.redisKeys = Array.from(uniqueKeys);
      }
      this.redisCursor = res.cursor;
      this.redisPattern = pattern;
    } catch (e) {
      console.error("Failed to scan redis keys", e);
    } finally {
      this.redisKeysLoading = false;
    }
  }

  async loadRedisKey(key: string) {
    if (!this.activeConnectionId) return;
    this.redisActiveKeyLoading = true;
    try {
      const res: any = await invoke("redis_get_key", {
        connectionId: this.activeConnectionId,
        key,
      });
      this.redisActiveKey = res;
    } catch (e: any) {
      console.error("Failed to get redis key", e);
      throw e;
    } finally {
      this.redisActiveKeyLoading = false;
    }
  }

  async setRedisKey(key: string, value: string, ttl?: number) {
    if (!this.activeConnectionId) return;
    try {
      await invoke("redis_set_key", {
        connectionId: this.activeConnectionId,
        key,
        value,
        ttl: ttl && ttl > 0 ? ttl : null,
      });
      
      if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
        invoke("log_query", {
          connectionId: this.activeConnectionId,
          sql: `SET "${key}" "${value.substring(0, 100)}${value.length > 100 ? '...' : ''}"${ttl ? ' EX ' + ttl : ''}`,
          durationMs: 0,
          status: "success",
        }).catch(console.error);
      }

      await this.loadRedisKeys(0, this.redisPattern);
      if (this.redisActiveKey?.key === key) {
        await this.loadRedisKey(key);
      }
    } catch (e: any) {
      console.error("Failed to set redis key", e);
      if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
        invoke("log_query", {
          connectionId: this.activeConnectionId,
          sql: `SET "${key}" "${value.substring(0, 100)}${value.length > 100 ? '...' : ''}"${ttl ? ' EX ' + ttl : ''}`,
          durationMs: 0,
          status: "error: " + e.toString(),
        }).catch(console.error);
      }
      throw e;
    }
  }

  async deleteRedisKey(key: string) {
    if (!this.activeConnectionId) return;
    try {
      await invoke("redis_delete_key", {
        connectionId: this.activeConnectionId,
        key,
      });

      if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
        invoke("log_query", {
          connectionId: this.activeConnectionId,
          sql: `DEL "${key}"`,
          durationMs: 0,
          status: "success",
        }).catch(console.error);
      }

      if (this.redisActiveKey?.key === key) {
        this.redisActiveKey = null;
      }
      await this.loadRedisKeys(0, this.redisPattern);
    } catch (e: any) {
      console.error("Failed to delete redis key", e);
      if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
        invoke("log_query", {
          connectionId: this.activeConnectionId,
          sql: `DEL "${key}"`,
          durationMs: 0,
          status: "error: " + e.toString(),
        }).catch(console.error);
      }
      throw e;
    }
  }

  async loadRedisServerInfo() {
    if (!this.activeConnectionId) return;
    try {
      const res: any = await invoke("redis_server_info", {
        connectionId: this.activeConnectionId,
      });
      this.redisServerInfo = res.stats;
    } catch (e) {
      console.error("Failed to load redis server info", e);
    }
  }

  schemaError = $state<string | null>(null);

  async loadSchemas() {
    if (!this.activeConnectionId) return;
    this.schemaError = null;
    console.log("loadSchemas: activeConnectionId =", this.activeConnectionId);
    try {
      console.log("loadSchemas: invoking list_schemas...");
      const list: any[] = await invoke("list_schemas", { connectionId: this.activeConnectionId });
      console.log("loadSchemas: list_schemas returned", list.length, "items:", list);
      this.schemas = list.map((s) => ({ name: s.name }));
      console.log("loadSchemas: set schemas to", this.schemas);
    } catch (e: any) {
      console.error("loadSchemas error:", e);
      this.schemaError = e.toString() || "Failed to load schemas";
      return;
    }
      
    // Fetch schema graph in background (fire-and-forget) so it doesn't block schema display
    this.loadSchemaGraph();
  }

  async loadSchemaGraph() {
    if (!this.activeConnectionId) return;
    try {
      const graph: any = await invoke("get_schema_graph", { 
        connectionId: this.activeConnectionId, 
        schema: "public" 
      });
      if (graph) {
        if (graph.edges) this.schemaEdges = graph.edges;
        if (graph.nodes) this.schemaNodes = graph.nodes;
      }
    } catch (e) {
      console.warn("Failed to load schema graph for FKs", e);
    }
  }

  async loadTables(schemaName: string) {
    if (!this.activeConnectionId) return;
    try {
      const list: any[] = await invoke("list_tables", {
        connectionId: this.activeConnectionId,
        schema: schemaName,
      });
      const schemaIdx = this.schemas.findIndex((s) => s.name === schemaName);
      if (schemaIdx !== -1) {
        this.schemas[schemaIdx].tables = list.map((t) => ({ name: t.name }));
      }
    } catch (e) {
      console.error(e);
    }
  }

  async loadColumns(schemaName: string, tableName: string) {
    if (!this.activeConnectionId) return;
    try {
      const info: any = await invoke("describe_table", {
        connectionId: this.activeConnectionId,
        schema: schemaName,
        table: tableName,
      });
      const schema = this.schemas.find((s) => s.name === schemaName);
      if (schema?.tables) {
        const table = schema.tables.find((t) => t.name === tableName);
        if (table) {
          table.columns = info.columns;
        }
      }
    } catch (e) {
      console.error(e);
    }
  }

  openNewTab(sql: string = "SELECT * FROM information_schema.tables LIMIT 100;", execute: boolean = false) {
    const id = crypto.randomUUID();
    const newTab: Tab = {
      id,
      name: `Query ${this.tabs.length + 1}`,
      sql,
      loading: false,
      queryId: null,
      columns: [],
      rows: [],
      error: null,
      executionTime: null,
      rowCount: 0,
      offset: 0,
      isFullyLoaded: false,
    };
    this.tabs.push(newTab);
    this.activeTabId = id;
    this.saveSessionState();
    
    if (execute) {
      setTimeout(() => this.executeQuery(), 50);
    }
  }

  closeTab(tabId: string) {
    this.tabs = this.tabs.filter((t) => t.id !== tabId);
    if (this.activeTabId === tabId) {
      this.activeTabId = this.tabs[this.tabs.length - 1]?.id || null;
    }
    this.saveSessionState();
  }

  get activeTab() {
    return this.tabs.find((t) => t.id === this.activeTabId) || null;
  }

  async executeQuery(isNextPage: boolean = false) {
    const tab = this.activeTab;
    if (!tab || !this.activeConnectionId || tab.loading) return;

    tab.loading = true;
    tab.error = null;
    
    if (!isNextPage) {
      tab.columns = [];
      tab.rows = [];
      tab.rowCount = 0;
      tab.offset = 0;
      tab.isFullyLoaded = false;
      tab.executionTime = null;
    }
    
    const queryId = crypto.randomUUID();
    tab.queryId = queryId;

    const start = performance.now();
    try {
      await invoke("execute_query", {
        connectionId: this.activeConnectionId,
        queryId,
        sql: tab.sql,
        batchSize: 100,
        offset: tab.offset
      });
      tab.executionTime = Math.round(performance.now() - start);

      if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
        invoke("log_query", {
          connectionId: this.activeConnectionId,
          sql: tab.sql,
          durationMs: tab.executionTime,
          status: "success",
        }).catch(console.error);
      }
    } catch (e: any) {
      tab.error = e.toString();
      tab.loading = false;
      tab.executionTime = Math.round(performance.now() - start);

      if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
        invoke("log_query", {
          connectionId: this.activeConnectionId,
          sql: tab.sql,
          durationMs: tab.executionTime,
          status: "error: " + e.toString(),
        }).catch(console.error);
      }
    }
    this.saveSessionState();
  }

  async cancelQuery() {
    const tab = this.activeTab;
    if (!tab || !this.activeConnectionId || !tab.queryId || !tab.loading) return;

    try {
      await invoke("cancel_query", {
        connectionId: this.activeConnectionId,
        queryId: tab.queryId,
      });
      tab.loading = false;
      tab.error = "Query cancelled by user.";
    } catch (e) {
      console.error(e);
    }
  }

  async saveSessionState() {
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        await invoke("save_session", {
          state: {
            active_tab_id: this.activeTabId,
            tabs: this.tabs.map((t) => ({
              id: t.id,
              name: t.name,
              sql: t.sql,
            })),
          },
        });
      } catch (e) {
        console.error("Failed to save session state", e);
      }
    }
  }

  async loadSessionState() {
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        const state: any = await invoke("load_session");
        if (state) {
          this.tabs = state.tabs.map((t: any) => ({
            id: t.id,
            name: t.name,
            sql: t.sql,
            loading: false,
            queryId: null,
            columns: [],
            rows: [],
            error: null,
            executionTime: null,
            rowCount: 0,
            offset: 0,
            isFullyLoaded: false,
          }));
          this.activeTabId = state.active_tab_id;
        }
      } catch (e) {
        console.error("Failed to load session state", e);
      }
    }
  }
}

export const appState = new AppState();
