<script lang="ts">
  import { appState } from "../state.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import { Bookmark, Clock, Trash2, Plus, Copy, Play, RefreshCw } from "lucide-svelte";

  // Tabs for Manager: "snippets" or "history"
  let managerTab = $state<"snippets" | "history">("snippets");
  
  // Lists
  let snippets = $state<{ name: string; sql: string }[]>([]);
  let history = $state<{ timestamp: string; sql: string; duration_ms: number; status: string }[]>([]);

  // Input states
  let newSnippetName = $state("");
  let showSaveDialog = $state(false);
  let isSubmitting = $state(false);

  async function loadSnippets() {
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        snippets = await invoke("load_snippets");
      } catch (e) {
        console.error("Failed to load snippets", e);
      }
    }
  }

  async function loadHistory() {
    if (!appState.activeConnectionId) return;
    if (typeof window !== "undefined" && (window as any).__TAURI_INTERNALS__) {
      try {
        history = await invoke("get_query_history", {
          connectionId: appState.activeConnectionId
        });
      } catch (e) {
        console.error("Failed to load query history", e);
      }
    }
  }

  async function handleSaveSnippet(e: Event) {
    e.preventDefault();
    if (!newSnippetName || !appState.activeTab) return;
    isSubmitting = true;
    try {
      await invoke("save_snippet", {
        name: newSnippetName,
        sql: appState.activeTab.sql
      });
      newSnippetName = "";
      showSaveDialog = false;
      await loadSnippets();
    } catch (e: any) {
      alert("Failed to save snippet: " + e.message);
    } finally {
      isSubmitting = false;
    }
  }

  async function handleDeleteSnippet(name: string) {
    if (!confirm(`Are you sure you want to delete snippet "${name}"?`)) return;
    try {
      await invoke("delete_snippet", { name });
      await loadSnippets();
    } catch (e: any) {
      alert("Failed to delete snippet: " + e.message);
    }
  }

  function applySqlToActiveTab(sql: string) {
    const active: any = appState.activeTab;
    if (active) {
      active.sql = sql;
      appState.saveSessionState();
    } else {
      appState.openNewTab();
      const newActive: any = appState.activeTab;
      if (newActive) {
        newActive.sql = sql;
        appState.saveSessionState();
      }
    }
  }

  function formatTime(timestampStr: string) {
    const epoch = parseInt(timestampStr);
    if (isNaN(epoch)) return timestampStr;
    const date = new Date(epoch * 1000);
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
  }

  $effect(() => {
    // Reload history when connection ID changes or tab switches to history
    if (appState.activeConnectionId) {
      if (managerTab === "history") {
        loadHistory();
      }
    }
  });

  onMount(() => {
    loadSnippets();
  });
</script>

<div class="snippets-manager">
  <!-- Tab selectors -->
  <div class="manager-nav">
    <button 
      class="nav-btn" 
      class:active={managerTab === 'snippets'} 
      onclick={() => { managerTab = 'snippets'; loadSnippets(); }}
    >
      <Bookmark size={14} />
      <span>Snippets</span>
    </button>
    <button 
      class="nav-btn" 
      class:active={managerTab === 'history'} 
      onclick={() => { managerTab = 'history'; loadHistory(); }}
      disabled={!appState.activeConnectionId}
    >
      <Clock size={14} />
      <span>History</span>
    </button>
  </div>

  <div class="manager-body">
    {#if managerTab === 'snippets'}
      <div class="snippets-section">
        <div class="section-header">
          <h3>Saved Snippets</h3>
          {#if appState.activeTab}
            <button class="btn-action-add" onclick={() => showSaveDialog = true} title="Save Current Query">
              <Plus size={14} /> Save Current
            </button>
          {/if}
        </div>

        {#if snippets.length === 0}
          <div class="empty-state">
            <Bookmark size={24} class="empty-icon" />
            <span>No saved SQL snippets.</span>
          </div>
        {:else}
          <div class="list-container">
            {#each snippets as snippet}
              <div class="list-item snippet-item">
                <div class="item-info">
                  <span class="item-title">{snippet.name}</span>
                  <pre class="item-preview"><code>{snippet.sql.substring(0, 100)}{snippet.sql.length > 100 ? '...' : ''}</code></pre>
                </div>
                <div class="item-actions">
                  <button class="btn-icon" onclick={() => applySqlToActiveTab(snippet.sql)} title="Load into Editor">
                    <Copy size={13} />
                  </button>
                  <button class="btn-icon delete" onclick={() => handleDeleteSnippet(snippet.name)} title="Delete Snippet">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="history-section">
        <div class="section-header">
          <h3>Query History</h3>
          <button class="btn-action-add" onclick={loadHistory} title="Refresh History">
            <RefreshCw size={12} style="margin-right: 4px;" /> Refresh
          </button>
        </div>

        {#if history.length === 0}
          <div class="empty-state">
            <Clock size={24} class="empty-icon" />
            <span>No query logs for this session.</span>
          </div>
        {:else}
          <div class="list-container">
            {#each history as entry}
              <div class="list-item history-item" class:error={entry.status.startsWith('error')}>
                <div class="item-info">
                  <div class="history-meta">
                    <span class="time">{formatTime(entry.timestamp)}</span>
                    <span class="duration">• {entry.duration_ms}ms</span>
                    {#if entry.status.startsWith('error')}
                      <span class="status-error">Error</span>
                    {:else}
                      <span class="status-ok">Success</span>
                    {/if}
                  </div>
                  <pre class="item-preview"><code>{entry.sql}</code></pre>
                </div>
                <div class="item-actions">
                  <button class="btn-icon" onclick={() => applySqlToActiveTab(entry.sql)} title="Load into Editor">
                    <Copy size={13} />
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Save Snippet dialog -->
  {#if showSaveDialog}
    <div class="snippet-modal">
      <div class="modal-card">
        <h4>Save Query Snippet</h4>
        <form onsubmit={handleSaveSnippet}>
          <input 
            type="text" 
            placeholder="Snippet name (e.g. get_all_users)" 
            bind:value={newSnippetName} 
            required 
            autofocus 
          />
          <div class="modal-buttons">
            <button type="button" class="btn-secondary" onclick={() => showSaveDialog = false}>Cancel</button>
            <button type="submit" class="btn-primary" disabled={isSubmitting}>
              {isSubmitting ? 'Saving...' : 'Save'}
            </button>
          </div>
        </form>
      </div>
    </div>
  {/if}
</div>

<style>
  .snippets-manager {
    display: flex;
    flex-direction: column;
    height: 100%;
    background-color: var(--bg-app);
    border-top: 1px solid var(--border-color);
  }

  .manager-nav {
    display: flex;
    border-bottom: 1px solid var(--border-color);
    background-color: var(--bg-sidebar);
  }

  .nav-btn {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 10px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    border-bottom: 2px solid transparent;
    transition: all 0.2s;
  }

  .nav-btn:hover:not(:disabled) {
    color: var(--text-normal);
    background-color: rgba(255, 255, 255, 0.02);
  }

  .nav-btn.active {
    color: var(--color-primary);
    border-bottom-color: var(--color-primary);
  }

  .nav-btn:disabled {
    opacity: 0.3;
    cursor: not-allowed;
  }

  .manager-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
  }

  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 12px;
  }

  .section-header h3 {
    margin: 0;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--text-muted);
    letter-spacing: 0.5px;
  }

  .btn-action-add {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: none;
    border-radius: 4px;
    background-color: rgba(137, 180, 250, 0.1);
    color: var(--color-primary);
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-action-add:hover {
    background-color: rgba(137, 180, 250, 0.2);
  }

  .list-container {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .list-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px;
    background-color: var(--bg-content);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    gap: 8px;
  }

  .item-info {
    flex: 1;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .item-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-normal);
  }

  .item-preview {
    margin: 0;
    font-size: 10px;
    font-family: monospace;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item-preview code {
    background: transparent;
    padding: 0;
  }

  .item-actions {
    display: flex;
    gap: 4px;
  }

  .btn-icon {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .btn-icon:hover {
    background-color: rgba(255, 255, 255, 0.05);
    color: var(--text-normal);
  }

  .btn-icon.delete:hover {
    background-color: rgba(243, 139, 168, 0.1);
    color: var(--color-schema);
    border-color: rgba(243, 139, 168, 0.2);
  }

  /* History styles */
  .history-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    color: var(--color-table);
  }

  .history-meta .time {
    font-weight: 600;
  }

  .history-meta .duration {
    color: var(--text-muted);
  }

  .status-ok {
    color: var(--color-column);
    font-weight: 600;
    margin-left: auto;
  }

  .status-error {
    color: var(--color-schema);
    font-weight: 600;
    margin-left: auto;
  }

  .list-item.error {
    border-color: rgba(243, 139, 168, 0.3);
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 30px 10px;
    color: var(--text-muted);
    font-size: 11px;
    text-align: center;
  }

  .empty-icon {
    opacity: 0.2;
  }

  /* Modal Dialog */
  .snippet-modal {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background-color: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .modal-card {
    background-color: var(--bg-content);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 16px;
    width: 320px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
  }

  .modal-card h4 {
    margin: 0 0 12px 0;
    font-size: 14px;
    font-weight: 700;
    color: var(--color-primary);
  }

  .modal-card input {
    width: 100%;
    padding: 8px 12px;
    background-color: var(--bg-app);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-normal);
    font-size: 13px;
    outline: none;
    margin-bottom: 16px;
  }

  .modal-card input:focus {
    border-color: var(--color-primary);
  }

  .modal-buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }

  .btn-secondary {
    padding: 6px 12px;
    background: transparent;
    border: 1px solid var(--border-color);
    color: var(--text-normal);
    border-radius: 4px;
    font-size: 12px;
    cursor: pointer;
  }

  .btn-secondary:hover {
    background-color: rgba(255, 255, 255, 0.05);
  }

  .btn-primary {
    padding: 6px 12px;
    background-color: var(--color-primary);
    border: none;
    color: var(--bg-app);
    border-radius: 4px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .btn-primary:hover {
    background-color: var(--color-primary-hover);
  }
</style>
