<script lang="ts">
  import { createVirtualizer } from '@tanstack/svelte-virtual';
  import { untrack } from 'svelte';
  import { appState } from '$lib/state.svelte';
  import { X } from 'lucide-svelte';

  interface Props {
    columns: { name: string; data_type: string }[];
    rows: any[][];
  }

  let { columns, rows }: Props = $props();

  let containerEl = $state<HTMLDivElement | null>(null);

  // Calculate dynamic column widths based on content
  function calculateColumnWidths(cols: {name: string, data_type: string}[], rowData: any[][]): number[] {
    return cols.map((col, colIndex) => {
      const lowerType = col.data_type.toLowerCase();
      
      // Fixed default for JSON since it can be massive
      if (lowerType === 'jsonb' || lowerType === 'json') return 300;
      
      // Calculate max length from header and first 100 rows
      let maxLen = col.name.length;
      const rowsToCheck = Math.min(rowData.length, 100);
      for (let i = 0; i < rowsToCheck; i++) {
        const val = rowData[i]?.[colIndex];
        if (val !== null && val !== undefined) {
          const strVal = String(val);
          if (strVal.length > maxLen) {
            maxLen = strVal.length;
          }
        }
      }
      
      // Cap max length to prevent excessively wide columns for text/varchar
      if (maxLen > 60) {
        maxLen = 60;
      }
      
      // ~7.5px per character for monospace fonts + padding
      let estimatedWidth = Math.max(100, Math.ceil(maxLen * 7.5) + 40);
      return Math.min(estimatedWidth, 500);
    });
  }

  let columnWidths = $state<number[]>([]);
  let lastColsKey = $state<string>('');
  
  $effect(() => {
    // Track the columns prop
    const cols = columns;
    untrack(() => {
      const currentColsKey = cols.map(c => c.name + ':' + c.data_type).join(',');
      if (currentColsKey !== lastColsKey) {
        columnWidths = calculateColumnWidths(cols, rows);
        lastColsKey = currentColsKey;
      }
    });
  });

  const virtualizer = createVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: 0,
    getScrollElement: () => containerEl!,
    estimateSize: () => 32,
    overscan: 10,
  });

  $effect(() => {
    const count = rows.length;
    const el = containerEl;
    untrack(() => {
      $virtualizer.setOptions({
        count,
        getScrollElement: () => el!,
        estimateSize: () => 32,
        overscan: 10,
      });
    });
  });

  // Infinite scrolling via scroll event (not $effect, to avoid reactive loops)
  function handleScroll(e: Event) {
    const el = e.target as HTMLDivElement;
    if (!el) return;
    const tab = appState.activeTab;
    if (!tab || tab.loading || tab.isFullyLoaded) return;
    
    // Trigger fetch when scrolled within 200px of the bottom
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distanceFromBottom < 200) {
      appState.executeQuery(true);
    }
  }

  let columnTemplate = $derived(columnWidths.map(w => `${Math.max(50, w)}px`).join(' '));
  let totalWidth = $derived(columnWidths.reduce((a, b) => a + Math.max(50, b), 0));

  function formatValue(val: any): string {
    if (val === null || val === undefined) return "NULL";
    if (typeof val === "object") return JSON.stringify(val);
    return String(val);
  }

  // Column Resizing logic
  function startResize(e: MouseEvent, index: number) {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX;
    const startWidth = columnWidths[index];
    
    function onMouseMove(moveEvent: MouseEvent) {
      const delta = moveEvent.clientX - startX;
      // Copy array to trigger reactivity
      const newWidths = [...columnWidths];
      newWidths[index] = Math.max(50, startWidth + delta);
      columnWidths = newWidths;
    }
    
    function onMouseUp() {
      document.removeEventListener('mousemove', onMouseMove);
      document.removeEventListener('mouseup', onMouseUp);
    }
    
    document.addEventListener('mousemove', onMouseMove);
    document.addEventListener('mouseup', onMouseUp);
  }

  // JSON Inspector Logic
  let inspectedJson = $state<any | null>(null);
  let inspectorOpen = $state(false);

  function openInspector(val: any) {
    inspectedJson = val;
    inspectorOpen = true;
  }

  function closeInspector() {
    inspectorOpen = false;
    inspectedJson = null;
  }

  // FK Navigation Logic
  // Parse the main table from the current query to avoid false positives
  function getCurrentTable(): string | null {
    if (!appState.activeTab?.sql) return null;
    const match = appState.activeTab.sql.match(/from\s+["']?([a-zA-Z0-9_]+)["']?(?:\.["']?([a-zA-Z0-9_]+)["']?)?/i);
    if (!match) return null;
    return match[2] ? match[2] : match[1];
  }

  function getFkEdge(colName: string) {
    if (!appState.schemaEdges) return undefined;
    
    const currentTable = getCurrentTable();
    if (currentTable) {
      return appState.schemaEdges.find(e => 
        e.source === currentTable && e.source_handle === colName
      );
    }
    
    return appState.schemaEdges.find(e => e.source_handle === colName);
  }

  // Reverse FK Context Menu Logic
  let fkMenu = $state({
    isOpen: false,
    x: 0,
    y: 0,
    edges: [] as any[],
    cellValue: null as any
  });

  function closeFkMenu() {
    fkMenu.isOpen = false;
  }

  function getReverseFkEdges(colName: string) {
    if (!appState.schemaEdges) return [];
    
    const currentTable = getCurrentTable();
    if (!currentTable) return [];
    
    // Find all edges where the current table is the target (inbound edges)
    return appState.schemaEdges.filter(e => 
      e.target === currentTable && e.target_handle === colName
    );
  }

  function navigateFk(event: MouseEvent, colName: string, cellValue: any) {
    if (!cellValue) return;
    
    // Prioritize outbound direct link first
    const edge = getFkEdge(colName);
    if (edge) {
      const query = `SELECT * FROM ${edge.target} WHERE ${edge.target_handle} = '${cellValue}';`;
      appState.openNewTab(query, true);
      return;
    }
    
    // Check for reverse inbound links
    const reverseEdges = getReverseFkEdges(colName);
    if (reverseEdges.length > 0) {
      fkMenu.edges = reverseEdges;
      fkMenu.cellValue = cellValue;
      fkMenu.x = event.clientX;
      fkMenu.y = event.clientY;
      fkMenu.isOpen = true;
    }
  }

  function navigateReverseFk(edge: any, cellValue: any) {
    const query = `SELECT * FROM ${edge.source} WHERE ${edge.source_handle} = '${cellValue}';`;
    appState.openNewTab(query, true);
    closeFkMenu();
  }

  function isFk(colName: string): boolean {
    return !!getFkEdge(colName) || getReverseFkEdges(colName).length > 0;
  }
</script>

<div class="grid-wrapper">
  {#if columns.length === 0}
    <div class="empty-state">No results to display</div>
  {:else}
    <div 
      class="scroll-container" 
      bind:this={containerEl}
      onscroll={handleScroll}
    >
      <div 
        class="inner-container" 
        style="height: {$virtualizer.getTotalSize() + 40}px; width: {totalWidth}px; position: relative;"
      >
        <!-- Header row -->
        <div 
          class="header-row" 
          style="grid-template-columns: {columnTemplate}; width: {totalWidth}px;"
        >
          {#each columns as col, i}
            <div class="header-cell relative">
              <div class="col-name">{col.name}</div>
              <div class="col-type">{col.data_type}</div>
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="resizer" onmousedown={(e) => startResize(e, i)}></div>
            </div>
          {/each}
        </div>

        <!-- Virtualized rows -->
        {#each $virtualizer.getVirtualItems() as item (item.index)}
          <div 
            class="data-row" 
            style="transform: translateY({item.start}px); height: {item.size}px; grid-template-columns: {columnTemplate}; width: {totalWidth}px;"
            class:odd={item.index % 2 !== 0}
          >
            {#each rows[item.index] as cell, colIndex}
              <div class="data-cell" class:null-value={cell === null || cell === undefined}>
                {#if isFk(columns[colIndex].name) && cell !== null && cell !== undefined}
                  <button class="fk-link" onclick={(e) => navigateFk(e, columns[colIndex].name, cell)} title="Navigate to related row">
                    {formatValue(cell)}
                  </button>
                {:else if columns[colIndex].data_type.toLowerCase() === 'jsonb' || columns[colIndex].data_type.toLowerCase() === 'json'}
                  <!-- svelte-ignore a11y_click_events_have_key_events -->
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <div class="json-cell" onclick={() => openInspector(cell)} title="Click to format JSON">
                    {formatValue(cell)}
                  </div>
                {:else}
                  <span class="selectable-text">{formatValue(cell)}</span>
                {/if}
              </div>
            {/each}
          </div>
        {/each}
      </div>
    </div>
  {/if}

  {#if inspectorOpen}
    <div class="inspector-panel">
      <div class="inspector-header">
        <h3>JSON Inspector</h3>
        <button class="close-btn" onclick={closeInspector}><X size={16} /></button>
      </div>
      <div class="inspector-body">
        <pre>{JSON.stringify(inspectedJson, null, 2)}</pre>
      </div>
    </div>
  {/if}

  {#if fkMenu.isOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fk-backdrop" onclick={closeFkMenu}></div>
    <div 
      class="fk-menu" 
      style="left: {fkMenu.x + 10}px; top: {fkMenu.y + 10}px;"
    >
      <div class="fk-menu-header">Navigate to related...</div>
      <div class="fk-menu-list">
        {#each fkMenu.edges as edge}
          <button class="fk-menu-item" onclick={() => navigateReverseFk(edge, fkMenu.cellValue)}>
            <span class="highlight">{edge.source}</span>
            <span class="fk-hint">(via {edge.source_handle})</span>
          </button>
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
  .grid-wrapper {
    width: 100%;
    height: 100%;
    overflow: hidden;
    background-color: var(--bg-grid);
    position: relative;
    display: flex;
  }

  .empty-state {
    display: flex;
    justify-content: center;
    align-items: center;
    width: 100%;
    height: 100%;
    color: var(--text-muted);
    font-size: 14px;
  }

  .scroll-container {
    flex: 1;
    height: 100%;
    overflow: auto;
  }

  .inner-container {
    position: relative;
  }

  .header-row {
    position: sticky;
    top: 0;
    z-index: 10;
    display: grid;
    background-color: var(--bg-grid-header);
    border-bottom: 2px solid var(--border-color);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
    height: 40px;
  }

  .header-cell {
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 4px 12px;
    border-right: 1px solid var(--border-color);
    position: relative;
  }

  .resizer {
    position: absolute;
    top: 0;
    right: 0;
    width: 5px;
    height: 100%;
    cursor: col-resize;
    background-color: transparent;
    z-index: 20;
  }

  .resizer:hover, .resizer:active {
    background-color: var(--color-primary);
  }

  .col-name {
    font-weight: 600;
    font-size: 12px;
    color: var(--text-normal);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .col-type {
    font-size: 9px;
    color: var(--text-muted);
    text-transform: uppercase;
    margin-top: 2px;
    font-family: Menlo, Monaco, Consolas, monospace;
    opacity: 0.8;
  }

  .data-row {
    position: absolute;
    top: 40px;
    left: 0;
    display: grid;
    border-bottom: 1px solid var(--border-color-light);
  }

  .data-row:hover {
    background-color: var(--bg-row-hover) !important;
  }

  .data-row.odd {
    background-color: var(--bg-row-alt);
  }

  .data-cell {
    display: flex;
    align-items: center;
    padding: 0 12px;
    font-size: 12px;
    font-family: Menlo, Monaco, Consolas, monospace;
    color: var(--text-normal);
    border-right: 1px solid var(--border-color-light);
    overflow: hidden;
  }

  .selectable-text {
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: all;
  }
  
  .json-cell {
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-column);
    cursor: pointer;
    user-select: none;
  }
  
  .json-cell:hover {
    text-decoration: underline;
  }

  .fk-link {
    background: none;
    border: none;
    padding: 0;
    margin: 0;
    color: var(--color-primary);
    font-family: inherit;
    font-size: inherit;
    cursor: pointer;
    text-align: left;
    width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  
  .fk-link:hover {
    text-decoration: underline;
  }

  .null-value {
    color: var(--text-null);
    font-style: italic;
  }

  .inspector-panel {
    width: 350px;
    background-color: var(--bg-content);
    border-left: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    box-shadow: -4px 0 12px rgba(0,0,0,0.1);
    z-index: 30;
  }

  .inspector-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-color);
    background-color: var(--bg-grid-header);
  }

  .inspector-header h3 {
    margin: 0;
    font-size: 14px;
    color: var(--text-normal);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
  }

  .close-btn:hover {
    background-color: var(--bg-hover);
    color: var(--text-normal);
  }

  .inspector-body {
    flex: 1;
    overflow: auto;
    padding: 16px;
    background-color: var(--bg-editor);
  }

  .inspector-body pre {
    margin: 0;
    font-family: Menlo, Monaco, Consolas, monospace;
    font-size: 12px;
    color: var(--text-normal);
    white-space: pre-wrap;
    word-wrap: break-word;
  }

  .fk-backdrop {
    position: fixed;
    top: 0; left: 0; right: 0; bottom: 0;
    z-index: 40;
  }

  .fk-menu {
    position: fixed;
    background-color: var(--bg-content);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    box-shadow: 0 4px 16px rgba(0,0,0,0.4);
    z-index: 50;
    min-width: 220px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .fk-menu-header {
    padding: 8px 12px;
    font-size: 11px;
    text-transform: uppercase;
    font-weight: 600;
    color: var(--text-muted);
    background-color: var(--bg-app);
    border-bottom: 1px solid var(--border-color);
  }

  .fk-menu-list {
    display: flex;
    flex-direction: column;
    max-height: 300px;
    overflow-y: auto;
  }

  .fk-menu-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 10px 12px;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border-color-light);
    color: var(--text-normal);
    cursor: pointer;
    text-align: left;
    font-size: 13px;
    transition: background 0.15s;
  }

  .fk-menu-item:last-child {
    border-bottom: none;
  }

  .fk-menu-item:hover {
    background-color: var(--bg-app);
  }

  .highlight {
    color: var(--color-primary);
    font-weight: 500;
  }

  .fk-hint {
    color: var(--text-muted);
    font-size: 11px;
    margin-left: 12px;
  }
</style>
