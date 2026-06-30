<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { appState } from '../state.svelte';
  import { SvelteFlow, Background, Controls, MarkerType } from '@xyflow/svelte';
  import dagre from 'dagre';
  import TableNode from './TableNode.svelte';
  import { X, RefreshCw } from 'lucide-svelte';
  
  import '@xyflow/svelte/dist/style.css';

  const nodeTypes = {
    table: TableNode
  };

  // State variables
  let selectedSchema = $state("public");
  let nodes = $state<any[]>([]);
  let edges = $state<any[]>([]);
  let isLoading = $state(false);
  let errorMsg = $state<string | null>(null);

  // Selected table metadata
  let selectedTable = $state<string | null>(null);
  let tableDdl = $state<string>("");
  let isDdlLoading = $state(false);

  function performLayout(rawNodes: any[], rawEdges: any[]) {
    const g = new dagre.graphlib.Graph();
    g.setGraph({ rankdir: 'LR', nodesep: 60, ranksep: 120 });
    g.setDefaultEdgeLabel(() => ({}));

    rawNodes.forEach(node => {
      const colCount = node.data.columns?.length || 0;
      g.setNode(node.id, { width: 240, height: 50 + colCount * 25 });
    });

    rawEdges.forEach(edge => {
      g.setEdge(edge.source, edge.target);
    });

    dagre.layout(g);

    return rawNodes.map(node => {
      const pos = g.node(node.id);
      return {
        ...node,
        position: {
          x: pos.x - 120,
          y: pos.y - (50 + (node.data.columns?.length || 0) * 25) / 2
        }
      };
    });
  }

  async function loadGraph() {
    if (!appState.activeConnectionId) return;
    isLoading = true;
    errorMsg = null;
    selectedTable = null;
    try {
      const graph: any = await invoke("get_schema_graph", {
        connectionId: appState.activeConnectionId,
        schema: selectedSchema
      });

      const rawNodes = graph.nodes.map((n: any) => ({
        id: n.id,
        type: 'table',
        data: { label: n.label, columns: n.columns },
        position: { x: 0, y: 0 }
      }));

      const rawEdges = graph.edges.map((e: any) => ({
        id: e.id,
        source: e.source,
        target: e.target,
        sourceHandle: e.source_handle,
        targetHandle: e.target_handle,
        type: 'smoothstep',
        animated: true,
        style: 'stroke: var(--text-muted); stroke-width: 1.5;',
        markerEnd: {
          type: MarkerType.ArrowClosed,
          color: 'var(--text-muted)',
        }
      }));

      nodes = performLayout(rawNodes, rawEdges);
      edges = rawEdges;
    } catch (e: any) {
      console.error("Failed to build ER Diagram", e);
      errorMsg = e.toString() || "Failed to load ER Diagram";
    } finally {
      isLoading = false;
    }
  }

  async function loadDdl(tableName: string) {
    selectedTable = tableName;
    isDdlLoading = true;
    try {
      const ddl: string = await invoke("get_table_ddl", {
        connectionId: appState.activeConnectionId,
        schema: selectedSchema,
        table: tableName
      });
      tableDdl = ddl;
    } catch (e: any) {
      tableDdl = "Failed to load DDL: " + e.message;
    } finally {
      isDdlLoading = false;
    }
  }

  function handleNodeClick({ node }: { event: any, node: any }) {
    if (node) {
      loadDdl(node.id);
    }
  }

  onMount(() => {
    // If active schema is in state, use it
    if (appState.schemas.length > 0) {
      selectedSchema = appState.schemas[0].name;
    }
    loadGraph();
  });
</script>

<div class="er-diagram-container">
  <!-- Toolbar header -->
  <div class="er-toolbar">
    <div class="toolbar-left">
      <label for="schema-select">Active Schema:</label>
      <select id="schema-select" bind:value={selectedSchema} onchange={loadGraph}>
        {#each appState.schemas as schema}
          <option value={schema.name}>{schema.name}</option>
        {/each}
      </select>
      <button class="btn-refresh" onclick={loadGraph} disabled={isLoading}>
        <RefreshCw size={14} class={isLoading ? 'spin' : ''} />
        <span>Refresh</span>
      </button>
    </div>
    <div class="toolbar-right">
      <span class="view-title">Schema Entity-Relationship View</span>
    </div>
  </div>

  <div class="er-body">
    {#if isLoading && nodes.length === 0}
      <div class="er-loading">
        <div class="spinner"></div>
        <span>Constructing relationship graph from database metadata...</span>
      </div>
    {:else if errorMsg}
      <div class="er-error">
        <p>{errorMsg}</p>
        <button onclick={loadGraph} class="btn-retry">Retry Connection Introspection</button>
      </div>
    {:else}
      <div class="flow-wrapper">
        <SvelteFlow 
          {nodes} 
          {edges} 
          {nodeTypes} 
          onnodeclick={handleNodeClick}
          fitView
        >
          <Background gap={16} />
          <Controls />
        </SvelteFlow>
      </div>
    {/if}

    <!-- Slide-out sidebar details panel -->
    {#if selectedTable}
      <div class="ddl-sidebar">
        <div class="sidebar-header">
          <div class="title-wrapper">
            <span class="icon">📄</span>
            <h3>{selectedTable} DDL</h3>
          </div>
          <button onclick={() => selectedTable = null} class="btn-close" aria-label="Close sidebar">
            <X size={16} />
          </button>
        </div>
        <div class="sidebar-content">
          {#if isDdlLoading}
            <div class="sidebar-loading">
              <div class="spinner"></div>
              <span>Fetching schema DDL...</span>
            </div>
          {:else}
            <pre class="ddl-pre"><code>{tableDdl}</code></pre>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .er-diagram-container {
    display: flex;
    flex-direction: column;
    height: 100%;
    background-color: var(--bg-content);
    color: var(--text-normal);
  }

  .er-toolbar {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    background-color: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-color);
  }

  .toolbar-left {
    display: flex;
    align-items: center;
    gap: 12px;
    font-size: 13px;
  }

  .toolbar-left label {
    font-weight: 500;
    color: var(--text-muted);
  }

  .toolbar-left select {
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid var(--border-color);
    background-color: var(--bg-app);
    color: var(--text-normal);
    font-size: 13px;
    outline: none;
    cursor: pointer;
  }

  .toolbar-left select:focus {
    border-color: var(--color-primary);
  }

  .btn-refresh {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background-color: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-normal);
    font-size: 12px;
    cursor: pointer;
    transition: background-color 0.2s;
  }

  .btn-refresh:hover:not(:disabled) {
    background-color: rgba(255, 255, 255, 0.1);
  }

  .btn-refresh:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .view-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--color-primary);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .er-body {
    flex: 1;
    position: relative;
    overflow: hidden;
    display: flex;
  }

  .flow-wrapper {
    flex: 1;
    height: 100%;
  }

  .er-loading, .er-error {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    gap: 16px;
    color: var(--text-muted);
    font-size: 14px;
  }

  .btn-retry {
    padding: 8px 16px;
    background-color: var(--color-primary);
    color: var(--bg-app);
    border: none;
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-retry:hover {
    background-color: var(--color-primary-hover);
  }

  /* Slide-out Sidebar styling */
  .ddl-sidebar {
    width: 400px;
    border-left: 1px solid var(--border-color);
    background-color: var(--bg-app);
    display: flex;
    flex-direction: column;
    z-index: 10;
    box-shadow: -4px 0 16px rgba(0, 0, 0, 0.3);
  }

  .sidebar-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px;
    border-bottom: 1px solid var(--border-color);
  }

  .title-wrapper {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .title-wrapper h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 700;
    color: var(--color-primary);
  }

  .btn-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .btn-close:hover {
    background-color: rgba(255, 255, 255, 0.05);
    color: var(--text-normal);
  }

  .sidebar-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }

  .sidebar-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 200px;
    gap: 12px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .ddl-pre {
    margin: 0;
    background-color: var(--bg-content);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    padding: 12px;
    overflow-x: auto;
    font-family: monospace;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-normal);
    white-space: pre-wrap;
    word-break: break-all;
  }

  /* Spinner */
  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid rgba(255, 255, 255, 0.1);
    border-radius: 50%;
    border-top-color: var(--color-primary);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }
</style>
