<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte';
  
  let { id, data } = $props<{
    id: string;
    data: {
      label: string;
      columns: { name: string; data_type: string }[];
    };
  }>();
</script>

<div class="table-node-card">
  <div class="table-node-header">
    <span class="table-icon">📋</span>
    <h3>{data.label}</h3>
  </div>
  <div class="table-node-columns">
    {#each data.columns as col}
      <div class="column-row">
        <!-- Target handle on the left for incoming foreign key constraints -->
        <Handle type="target" position={Position.Left} id={col.name} class="flow-handle left-handle" />
        
        <span class="col-name">{col.name}</span>
        <span class="col-type">{col.data_type.toLowerCase()}</span>
        
        <!-- Source handle on the right for outgoing foreign key constraints -->
        <Handle type="source" position={Position.Right} id={col.name} class="flow-handle right-handle" />
      </div>
    {/each}
  </div>
</div>

<style>
  .table-node-card {
    min-width: 220px;
    background-color: var(--bg-app);
    border: 2px solid var(--border-color);
    border-radius: 8px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.4);
    overflow: hidden;
    color: var(--text-normal);
    font-family: Inter, system-ui, sans-serif;
  }

  .table-node-card:hover {
    border-color: var(--color-primary);
  }

  .table-node-header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background-color: rgba(137, 180, 250, 0.15);
    border-bottom: 1px solid var(--border-color);
  }

  .table-node-header h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
    color: var(--color-primary);
  }

  .table-node-columns {
    padding: 6px 0;
    display: flex;
    flex-direction: column;
  }

  .column-row {
    position: relative;
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 12px;
    font-size: 11px;
    gap: 16px;
  }

  .column-row:hover {
    background-color: rgba(255, 255, 255, 0.05);
  }

  .col-name {
    font-weight: 500;
  }

  .col-type {
    color: var(--text-muted);
    font-family: monospace;
  }

  /* Handle styling */
  :global(.flow-handle) {
    width: 6px !important;
    height: 6px !important;
    background-color: var(--color-primary) !important;
    border: 1px solid var(--bg-app) !important;
    border-radius: 50% !important;
    min-width: 0 !important;
    min-height: 0 !important;
    transition: background-color 0.2s;
  }

  :global(.flow-handle:hover) {
    background-color: var(--color-primary-hover) !important;
  }

  :global(.left-handle) {
    left: -4px !important;
  }

  :global(.right-handle) {
    right: -4px !important;
  }
</style>
