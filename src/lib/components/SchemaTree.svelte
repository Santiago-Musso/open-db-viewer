<script lang="ts">
  import { appState } from "$lib/state.svelte";
  import { 
    Database, 
    Table, 
    Columns, 
    ChevronRight, 
    ChevronDown,
    RefreshCw
  } from "lucide-svelte";
  import { invoke } from "@tauri-apps/api/core";

  let expandedSchemas = $state<Record<string, boolean>>({});
  let expandedTables = $state<Record<string, boolean>>({});

  async function toggleSchema(schemaName: string) {
    expandedSchemas[schemaName] = !expandedSchemas[schemaName];
    if (expandedSchemas[schemaName]) {
      const schema = appState.schemas.find(s => s.name === schemaName);
      if (!schema?.tables) {
        await appState.loadTables(schemaName);
      }
    }
  }

  async function toggleTable(schemaName: string, tableName: string) {
    const key = `${schemaName}.${tableName}`;
    expandedTables[key] = !expandedTables[key];
    if (expandedTables[key]) {
      const schema = appState.schemas.find(s => s.name === schemaName);
      const table = schema?.tables?.find(t => t.name === tableName);
      if (!table?.columns) {
        await appState.loadColumns(schemaName, tableName);
      }
    }
  }

  async function refreshSchema(schemaName: string) {
    if (!appState.activeConnectionId) return;
    try {
      await invoke("refresh_metadata_cache", {
        connectionId: appState.activeConnectionId,
        schema: schemaName,
      });
      // Force reload the tables of this schema
      await appState.loadTables(schemaName);
      // Reload schema graph too
      await appState.loadSchemaGraph();
    } catch (e: any) {
      alert("Failed to refresh schema: " + e.toString());
    }
  }

  async function refreshTable(schemaName: string, tableName: string) {
    if (!appState.activeConnectionId) return;
    try {
      await invoke("refresh_metadata_cache", {
        connectionId: appState.activeConnectionId,
        schema: schemaName,
        table: tableName,
      });
      // Force reload columns
      await appState.loadColumns(schemaName, tableName);
      // Reload schema graph too
      await appState.loadSchemaGraph();
    } catch (e: any) {
      alert("Failed to refresh table: " + e.toString());
    }
  }

  function appendToEditor(text: string) {
    const tab = appState.activeTab;
    if (tab) {
      tab.sql = tab.sql ? `${tab.sql} ${text}` : text;
    }
  }

  function formatType(type: string): string {
    if (!type) return "";
    const lower = type.toLowerCase();
    if (lower.startsWith("character varying")) {
      return type.replace(/character varying/i, "varchar");
    }
    if (lower === "timestamp without time zone") return "timestamp";
    if (lower === "timestamp with time zone") return "timestamptz";
    if (lower === "double precision") return "float8";
    if (lower === "integer") return "int";
    return lower;
  }

  $effect(() => {
    console.log("EFFECT SchemaTree: schemas changed to", $state.snapshot(appState.schemas));
  });
</script>

<div class="schema-tree-container">
  {#if appState.schemaError}
    <div class="schema-error">
      <p>Failed to load schemas</p>
      <div class="error-msg">{appState.schemaError}</div>
      <button class="btn-retry" onclick={() => appState.loadSchemas()}>Retry</button>
    </div>
  {:else if appState.schemas.length === 0}
    <div class="loading-schemas">Loading schemas...</div>
  {:else}
    {#each appState.schemas as schema}
      <div class="tree-node schema-node">
        <button 
          class="node-header" 
          onclick={() => toggleSchema(schema.name)}
        >
          {#if expandedSchemas[schema.name]}
            <ChevronDown size={14} class="chevron" />
          {:else}
            <ChevronRight size={14} class="chevron" />
          {/if}
          <Database size={14} class="icon schema-icon" />
          <span class="node-label">{schema.name}</span>
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <span 
            class="btn-node-refresh" 
            onclick={(e) => { e.stopPropagation(); refreshSchema(schema.name); }} 
            title="Refresh Schema Cache"
          >
            <RefreshCw size={10} />
          </span>
        </button>

        {#if expandedSchemas[schema.name]}
          <div class="node-children">
            {#if !schema.tables}
              <div class="loading-text">Loading tables...</div>
            {:else if schema.tables.length === 0}
              <div class="empty-text">No tables found</div>
            {:else}
              {#each schema.tables as table}
                {@const tableKey = `${schema.name}.${table.name}`}
                <div class="tree-node table-node">
                  <button 
                    class="node-header" 
                    onclick={() => toggleTable(schema.name, table.name)}
                    ondblclick={() => appendToEditor(table.name)}
                  >
                    {#if expandedTables[tableKey]}
                      <ChevronDown size={14} class="chevron" />
                    {:else}
                      <ChevronRight size={14} class="chevron" />
                    {/if}
                    <Table size={14} class="icon table-icon" />
                    <span class="node-label">{table.name}</span>
                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <span 
                      class="btn-node-refresh" 
                      onclick={(e) => { e.stopPropagation(); refreshTable(schema.name, table.name); }} 
                      title="Refresh Table Cache"
                    >
                      <RefreshCw size={10} />
                    </span>
                  </button>

                  {#if expandedTables[tableKey]}
                    <div class="node-children">
                      {#if !table.columns}
                        <div class="loading-text">Loading columns...</div>
                      {:else}
                        {#each table.columns as col}
                          <div class="tree-node column-node">
                            <span class="node-header leaf-node">
                              <Columns size={12} class="icon column-icon" />
                              <span class="node-label column-name" title={col.name}>{col.name}</span>
                              <span class="column-type" title={col.data_type}>{formatType(col.data_type)}</span>
                            </span>
                          </div>
                        {/each}
                      {/if}
                    </div>
                  {/if}
                </div>
              {/each}
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</div>

<style>
  .schema-tree-container {
    width: 100%;
    padding: 8px 0;
    color: var(--text-normal);
  }

  .loading-schemas {
    padding: 16px;
    font-size: 13px;
    color: var(--text-muted);
    font-style: italic;
  }

  .tree-node {
    display: flex;
    flex-direction: column;
  }

  .node-header {
    display: flex;
    align-items: center;
    width: 100%;
    background: none;
    border: none;
    padding: 6px 12px;
    text-align: left;
    cursor: pointer;
    border-radius: 4px;
    transition: background-color 0.15s ease;
    color: inherit;
  }

  .node-header:hover {
    background-color: var(--bg-hover);
  }

  :global(.chevron) {
    color: var(--text-muted);
    margin-right: 4px;
  }

  :global(.icon) {
    margin-right: 6px;
    flex-shrink: 0;
  }

  :global(.schema-icon) {
    color: var(--color-schema);
  }

  :global(.table-icon) {
    color: var(--color-table);
  }

  :global(.column-icon) {
    color: var(--color-column);
  }

  .node-label {
    font-size: 13px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .btn-node-refresh {
    display: none;
    align-items: center;
    justify-content: center;
    margin-left: auto;
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 2px;
    border-radius: 4px;
    transition: color 0.15s, background-color 0.15s;
  }

  .btn-node-refresh:hover {
    color: var(--text-normal);
    background-color: var(--bg-hover-dark, rgba(255,255,255,0.1));
  }

  .node-header:hover .btn-node-refresh {
    display: inline-flex;
  }

  .leaf-node {
    cursor: default;
    padding-left: 30px;
  }

  .leaf-node:hover {
    background: none;
  }

  .node-children {
    padding-left: 16px;
  }

  .loading-text, .empty-text {
    padding: 4px 12px 4px 30px;
    font-size: 12px;
    color: var(--text-muted);
    font-style: italic;
  }

  .column-name {
    color: var(--text-normal);
    flex-grow: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-right: 8px;
  }

  .column-type {
    font-size: 10px;
    color: var(--text-muted);
    font-family: Menlo, Monaco, monospace;
    background-color: var(--bg-hover, rgba(255, 255, 255, 0.05));
    padding: 2px 6px;
    border-radius: 4px;
    flex-shrink: 0;
    white-space: nowrap;
    margin-left: auto;
  }

  .schema-error {
    padding: 16px;
    font-size: 12px;
  }

  .schema-error p {
    margin: 0 0 6px 0;
  }
</style>
