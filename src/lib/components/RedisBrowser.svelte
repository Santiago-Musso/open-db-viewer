<script lang="ts">
  import { appState } from "../state.svelte";
  import { 
    Search, 
    Plus, 
    Trash2, 
    Save, 
    Key, 
    Clock, 
    Cpu, 
    Info, 
    RefreshCw, 
    ChevronRight,
    Play,
    LogOut
  } from "lucide-svelte";

  // Search & Filter
  let filterPattern = $state("*");
  let newKeyName = $state("");
  let newKeyValue = $state("");
  let newKeyTtl = $state<number>(0);
  let showCreateModal = $state(false);

  // Edit fields for active key
  let editValue = $state("");
  let editTtl = $state<number>(0);
  let isSaving = $state(false);
  let isDeleting = $state(false);

  // View state: "keys" or "info"
  let activeTab = $state<"keys" | "info">("keys");

  // Load key detail when active key changes
  $effect(() => {
    if (appState.redisActiveKey) {
      editValue = appState.redisActiveKey.value;
      editTtl = appState.redisActiveKey.ttl || 0;
    } else {
      editValue = "";
      editTtl = 0;
    }
  });

  async function handleSearch(e?: Event) {
    if (e) e.preventDefault();
    await appState.loadRedisKeys(0, filterPattern);
  }

  async function handleLoadMore() {
    await appState.loadRedisKeys(appState.redisCursor, filterPattern);
  }

  async function handleSelectKey(key: string) {
    try {
      await appState.loadRedisKey(key);
    } catch (e: any) {
      alert("Failed to load key: " + e.message);
    }
  }

  async function handleSaveKey() {
    if (!appState.redisActiveKey) return;
    isSaving = true;
    try {
      await appState.setRedisKey(
        appState.redisActiveKey.key, 
        editValue, 
        editTtl > 0 ? editTtl : undefined
      );
      alert("Key saved successfully!");
    } catch (e: any) {
      alert("Failed to save key: " + e.message);
    } finally {
      isSaving = false;
    }
  }

  async function handleDeleteKey(key: string) {
    if (!confirm(`Are you sure you want to delete "${key}"?`)) return;
    isDeleting = true;
    try {
      await appState.deleteRedisKey(key);
      alert("Key deleted successfully!");
    } catch (e: any) {
      alert("Failed to delete key: " + e.message);
    } finally {
      isDeleting = false;
    }
  }

  async function handleCreateKey(e: Event) {
    e.preventDefault();
    if (!newKeyName) return;
    try {
      await appState.setRedisKey(newKeyName, newKeyValue, newKeyTtl > 0 ? newKeyTtl : undefined);
      showCreateModal = false;
      newKeyName = "";
      newKeyValue = "";
      newKeyTtl = 0;
      alert("Key created successfully!");
    } catch (e: any) {
      alert("Failed to create key: " + e.message);
    }
  }

  function formatBytes(bytesStr: string) {
    const bytes = parseInt(bytesStr);
    if (isNaN(bytes)) return bytesStr;
    if (bytes === 0) return '0 Bytes';
    const k = 1024;
    const sizes = ['Bytes', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  }
</script>

<div class="redis-browser">
  <!-- Top Navigation tabs -->
  <div class="redis-nav">
    <button 
      class="nav-tab" 
      class:active={activeTab === 'keys'} 
      onclick={() => activeTab = 'keys'}
    >
      <Key size={16} />
      <span>Key Explorer</span>
    </button>
    <button 
      class="nav-tab" 
      class:active={activeTab === 'info'} 
      onclick={() => { activeTab = 'info'; appState.loadRedisServerInfo(); }}
    >
      <Cpu size={16} />
      <span>Server Statistics</span>
    </button>
    <div style="flex: 1;"></div>
    <button class="btn-disconnect-redis" onclick={() => appState.disconnect()} title="Disconnect">
      <LogOut size={16} />
      <span>Disconnect</span>
    </button>
  </div>

  {#if activeTab === 'keys'}
    <div class="redis-explorer-layout">
      <!-- Left sidebar: Keys list -->
      <div class="keys-sidebar">
        <form onsubmit={handleSearch} class="search-form">
          <div class="search-input-wrapper">
            <Search size={14} class="search-icon" />
            <input 
              type="text" 
              placeholder="Filter keys (e.g. user:*)" 
              bind:value={filterPattern} 
            />
          </div>
          <button type="submit" class="btn-search-icon" title="Search">
            <Play size={14} />
          </button>
          <button 
            type="button" 
            class="btn-add-key" 
            onclick={() => showCreateModal = true}
            title="Add New Key"
          >
            <Plus size={16} />
          </button>
        </form>

        <div class="keys-list-container">
          {#if appState.redisKeysLoading && appState.redisKeys.length === 0}
            <div class="loading-keys">
              <div class="spinner"></div>
              <span>Scanning keys...</span>
            </div>
          {:else if appState.redisKeys.length === 0}
            <div class="empty-keys">
              <span>No keys found.</span>
            </div>
          {:else}
            <div class="keys-list">
              {#each appState.redisKeys as key}
                <button 
                  class="key-item" 
                  class:active={appState.redisActiveKey?.key === key}
                  onclick={() => handleSelectKey(key)}
                >
                  <span class="key-name">{key}</span>
                  <ChevronRight size={14} class="chevron" />
                </button>
              {/each}
            </div>

            {#if appState.redisCursor !== 0}
              <button class="btn-load-more" onclick={handleLoadMore} disabled={appState.redisKeysLoading}>
                {#if appState.redisKeysLoading}
                  Loading...
                {:else}
                  Load More Keys
                {/if}
              </button>
            {/if}
          {/if}
        </div>
      </div>

      <!-- Right area: Selected key details / value editor -->
      <div class="key-details-panel">
        {#if appState.redisActiveKeyLoading}
          <div class="panel-loading">
            <div class="spinner"></div>
            <span>Fetching key details...</span>
          </div>
        {:else if appState.redisActiveKey}
          <div class="detail-container">
            <div class="detail-header">
              <div class="key-identity">
                <span class="type-badge" data-type={appState.redisActiveKey.value_type}>
                  {appState.redisActiveKey.value_type.toUpperCase()}
                </span>
                <h2>{appState.redisActiveKey.key}</h2>
              </div>
              <button 
                class="btn-delete-key" 
                onclick={() => handleDeleteKey(appState.redisActiveKey!.key)} 
                disabled={isDeleting}
                title="Delete Key"
              >
                <Trash2 size={16} />
                <span>Delete</span>
              </button>
            </div>

            <div class="metadata-row">
              <div class="meta-item">
                <Clock size={14} />
                <span class="meta-label">TTL:</span>
                <input 
                  type="number" 
                  class="ttl-input" 
                  bind:value={editTtl} 
                  placeholder="TTL in seconds (0 or less for persistent)"
                />
                <span class="ttl-unit">sec ({editTtl <= 0 ? 'No Expiry' : 'Expires in ' + editTtl + 's'})</span>
              </div>
            </div>

            <div class="value-editor">
              <div class="editor-header">
                <h3>Value Editor</h3>
                {#if appState.redisActiveKey.value_type !== 'string'}
                  <span class="complex-type-warning">
                    ⚠️ JSON Representation of complex types (non-strings cannot be converted back to their structures via simple save yet).
                  </span>
                {/if}
              </div>
              <textarea 
                bind:value={editValue} 
                class="value-textarea" 
                placeholder="Key value..."
                disabled={appState.redisActiveKey.value_type !== 'string'}
              ></textarea>
            </div>

            <div class="detail-actions">
              <button 
                class="btn-save-key" 
                onclick={handleSaveKey} 
                disabled={isSaving || appState.redisActiveKey.value_type !== 'string'}
              >
                <Save size={16} />
                <span>{isSaving ? 'Saving...' : 'Save Changes'}</span>
              </button>
            </div>
          </div>
        {:else}
          <div class="no-key-selected">
            <Key size={48} class="empty-icon" />
            <h3>No Key Selected</h3>
            <p>Select a key from the sidebar to view and edit its content, or create a new key.</p>
          </div>
        {/if}
      </div>
    </div>
  {:else if activeTab === 'info'}
    <!-- Server statistics dashboard -->
    <div class="stats-panel">
      <div class="stats-header">
        <h2>Server Statistics & Details</h2>
        <button class="btn-refresh" onclick={() => appState.loadRedisServerInfo()} title="Refresh stats">
          <RefreshCw size={14} /> Refresh
        </button>
      </div>

      <div class="stats-grid">
        <div class="stat-card">
          <div class="icon-box"><Info size={20} /></div>
          <div class="stat-info">
            <span class="label">Redis Version</span>
            <span class="value">{appState.redisServerInfo['redis_version'] || 'N/A'}</span>
          </div>
        </div>
        <div class="stat-card">
          <div class="icon-box"><Clock size={20} /></div>
          <div class="stat-info">
            <span class="label">Uptime</span>
            <span class="value">{appState.redisServerInfo['uptime_in_days'] ? appState.redisServerInfo['uptime_in_days'] + ' days' : 'N/A'}</span>
          </div>
        </div>
        <div class="stat-card">
          <div class="icon-box"><Cpu size={20} /></div>
          <div class="stat-info">
            <span class="label">Used Memory</span>
            <span class="value">{formatBytes(appState.redisServerInfo['used_memory'] || 'N/A')}</span>
          </div>
        </div>
        <div class="stat-card">
          <div class="icon-box"><RefreshCw size={20} /></div>
          <div class="stat-info">
            <span class="label">Connected Clients</span>
            <span class="value">{appState.redisServerInfo['connected_clients'] || 'N/A'}</span>
          </div>
        </div>
      </div>

      <div class="all-stats-table-wrapper">
        <h3>All Configuration & Health Metrics</h3>
        <table class="all-stats-table">
          <thead>
            <tr>
              <th>Metric Key</th>
              <th>Value</th>
            </tr>
          </thead>
          <tbody>
            {#each Object.entries(appState.redisServerInfo).sort((a,b) => a[0].localeCompare(b[0])) as [k, v]}
              <tr>
                <td><code>{k}</code></td>
                <td>{v}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}

  <!-- Create Key Dialog Modal -->
  {#if showCreateModal}
    <div class="modal-backdrop">
      <div class="modal-content">
        <h3>Create New Key</h3>
        <form onsubmit={handleCreateKey}>
          <div class="form-group">
            <label for="new-key-name">Key Name</label>
            <input 
              id="new-key-name"
              type="text" 
              placeholder="e.g. user:session:100" 
              bind:value={newKeyName} 
              required 
            />
          </div>
          <div class="form-group">
            <label for="new-key-value">String Value</label>
            <textarea 
              id="new-key-value"
              placeholder="Enter value text..." 
              bind:value={newKeyValue}
            ></textarea>
          </div>
          <div class="form-group">
            <label for="new-key-ttl">TTL (Seconds, optional)</label>
            <input 
              id="new-key-ttl"
              type="number" 
              placeholder="0 for no expiration" 
              bind:value={newKeyTtl} 
            />
          </div>
          <div class="modal-actions">
            <button type="button" class="btn-cancel" onclick={() => showCreateModal = false}>Cancel</button>
            <button type="submit" class="btn-submit">Create Key</button>
          </div>
        </form>
      </div>
    </div>
  {/if}
</div>

<style>
  .redis-browser {
    display: flex;
    flex-direction: column;
    height: 100%;
    background-color: var(--bg-normal, var(--bg-content));
    color: var(--text-normal, var(--text-normal));
    font-family: Inter, system-ui, sans-serif;
  }

  .redis-nav {
    display: flex;
    gap: 8px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border-color, var(--border-color));
    background-color: var(--bg-header, var(--bg-sidebar));
    align-items: center;
  }

  .btn-disconnect-redis {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border: 1px solid var(--border-color, var(--border-color));
    background: transparent;
    border-radius: 6px;
    color: var(--color-schema);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  
  .btn-disconnect-redis:hover {
    background-color: rgba(243, 139, 168, 0.1);
    border-color: rgba(243, 139, 168, 0.2);
  }

  .nav-tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border: none;
    background: transparent;
    border-radius: 6px;
    color: var(--text-muted, var(--text-muted));
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .nav-tab:hover {
    background-color: rgba(255, 255, 255, 0.05);
    color: var(--text-normal);
  }

  .nav-tab.active {
    background-color: var(--color-primary, var(--color-primary));
    color: var(--bg-app);
  }

  .redis-explorer-layout {
    display: grid;
    grid-template-columns: 320px 1fr;
    flex: 1;
    overflow: hidden;
  }

  /* Sidebar keys list */
  .keys-sidebar {
    display: flex;
    flex-direction: column;
    border-right: 1px solid var(--border-color, var(--border-color));
    overflow: hidden;
    background-color: var(--bg-sidebar, var(--bg-app));
  }

  .search-form {
    display: flex;
    gap: 8px;
    padding: 12px;
    border-bottom: 1px solid var(--border-color, var(--border-color));
  }

  .search-input-wrapper {
    position: relative;
    flex: 1;
  }

  .search-icon {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--text-muted, var(--text-muted));
  }

  .search-input-wrapper input {
    width: 100%;
    padding: 8px 10px 8px 32px;
    border-radius: 6px;
    border: 1px solid var(--border-color, var(--border-color));
    background-color: var(--bg-normal, var(--bg-content));
    color: var(--text-normal);
    font-size: 13px;
    outline: none;
  }

  .search-input-wrapper input:focus {
    border-color: var(--color-primary, var(--color-primary));
  }

  .btn-search-icon, .btn-add-key {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 6px;
    border: none;
    cursor: pointer;
    transition: background-color 0.2s;
  }

  .btn-search-icon {
    background-color: rgba(255, 255, 255, 0.05);
    color: var(--text-normal);
  }

  .btn-search-icon:hover {
    background-color: rgba(255, 255, 255, 0.1);
  }

  .btn-add-key {
    background-color: var(--color-primary, var(--color-primary));
    color: var(--bg-app);
  }

  .btn-add-key:hover {
    background-color: var(--color-primary-hover, var(--color-primary-hover));
  }

  .keys-list-container {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
  }

  .keys-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .key-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    background: transparent;
    border: none;
    border-radius: 6px;
    color: var(--text-normal);
    font-size: 13px;
    cursor: pointer;
    text-align: left;
    transition: background-color 0.15s;
  }

  .key-item:hover {
    background-color: rgba(255, 255, 255, 0.05);
  }

  .key-item.active {
    background-color: rgba(137, 180, 250, 0.15);
    color: var(--color-primary, var(--color-primary));
  }

  .key-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    padding-right: 8px;
  }

  .key-item .chevron {
    opacity: 0;
    transition: opacity 0.15s;
  }

  .key-item:hover .chevron, .key-item.active .chevron {
    opacity: 1;
  }

  .btn-load-more {
    margin-top: 12px;
    padding: 8px;
    background-color: rgba(255, 255, 255, 0.05);
    border: 1px dashed var(--border-color);
    border-radius: 6px;
    color: var(--text-muted);
    font-size: 12px;
    cursor: pointer;
    transition: all 0.2s;
  }

  .btn-load-more:hover:not(:disabled) {
    background-color: rgba(255, 255, 255, 0.1);
    color: var(--text-normal);
  }

  /* Right Side Details panel */
  .key-details-panel {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    background-color: var(--bg-normal, var(--bg-content));
    padding: 24px;
  }

  .no-key-selected {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    color: var(--text-muted);
    text-align: center;
    max-width: 400px;
    margin: auto;
  }

  .empty-icon {
    margin-bottom: 16px;
    opacity: 0.3;
  }

  .no-key-selected h3 {
    font-size: 18px;
    font-weight: 600;
    margin-bottom: 8px;
    color: var(--text-normal);
  }

  .no-key-selected p {
    font-size: 13px;
    line-height: 1.5;
  }

  .detail-container {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .detail-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 16px;
  }

  .key-identity {
    display: flex;
    flex-direction: column;
    gap: 8px;
    flex: 1;
    overflow: hidden;
  }

  .key-identity h2 {
    font-size: 20px;
    font-weight: 700;
    color: var(--text-normal);
    overflow-wrap: break-word;
    word-break: break-all;
  }

  .type-badge {
    align-self: flex-start;
    padding: 3px 8px;
    font-size: 11px;
    font-weight: 700;
    border-radius: 4px;
    color: var(--bg-app);
  }

  .type-badge[data-type="string"] { background-color: var(--color-column); }
  .type-badge[data-type="hash"] { background-color: var(--color-table); }
  .type-badge[data-type="list"] { background-color: var(--color-primary); }
  .type-badge[data-type="set"] { background-color: var(--color-table); }
  .type-badge[data-type="zset"] { background-color: var(--color-table); }

  .btn-delete-key {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 12px;
    background-color: rgba(243, 139, 168, 0.1);
    border: 1px solid rgba(243, 139, 168, 0.2);
    border-radius: 6px;
    color: var(--color-schema);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 0.2s;
  }

  .btn-delete-key:hover {
    background-color: rgba(243, 139, 168, 0.2);
  }

  .metadata-row {
    background-color: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    padding: 12px;
  }

  .meta-item {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }

  .meta-label {
    color: var(--text-muted);
    font-weight: 500;
  }

  .ttl-input {
    width: 100px;
    padding: 4px 8px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
    background-color: var(--bg-sidebar);
    color: var(--text-normal);
    font-size: 13px;
  }

  .ttl-unit {
    color: var(--text-muted);
    font-size: 12px;
  }

  .value-editor {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .editor-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .editor-header h3 {
    font-size: 14px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-muted);
    letter-spacing: 0.5px;
  }

  .complex-type-warning {
    font-size: 11px;
    color: var(--color-table);
  }

  .value-textarea {
    width: 100%;
    height: 300px;
    padding: 12px;
    border-radius: 8px;
    border: 1px solid var(--border-color);
    background-color: var(--bg-sidebar, var(--bg-app));
    color: var(--text-normal);
    font-family: monospace;
    font-size: 13px;
    line-height: 1.5;
    resize: vertical;
    outline: none;
  }

  .value-textarea:focus {
    border-color: var(--color-primary);
  }

  .value-textarea:disabled {
    opacity: 0.7;
    background-color: rgba(255,255,255,0.02);
  }

  .detail-actions {
    display: flex;
    justify-content: flex-end;
  }

  .btn-save-key {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 20px;
    background-color: var(--color-primary, var(--color-primary));
    border: none;
    border-radius: 6px;
    color: var(--bg-app);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color 0.2s;
  }

  .btn-save-key:hover:not(:disabled) {
    background-color: var(--color-primary-hover, var(--color-primary-hover));
  }

  .btn-save-key:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  /* Stats Dashboard style */
  .stats-panel {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 24px;
  }

  .stats-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-bottom: 1px solid var(--border-color);
    padding-bottom: 16px;
  }

  .stats-header h2 {
    font-size: 20px;
    font-weight: 700;
  }

  .btn-refresh {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background-color: rgba(255,255,255,0.05);
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-normal);
    font-size: 12px;
    cursor: pointer;
  }

  .btn-refresh:hover {
    background-color: rgba(255,255,255,0.1);
  }

  .stats-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 16px;
  }

  .stat-card {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 16px;
    background-color: var(--bg-sidebar, var(--bg-app));
    border: 1px solid var(--border-color);
    border-radius: 8px;
  }

  .icon-box {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    background-color: rgba(137, 180, 250, 0.1);
    color: var(--color-primary, var(--color-primary));
    border-radius: 8px;
  }

  .stat-info {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .stat-info .label {
    font-size: 11px;
    text-transform: uppercase;
    color: var(--text-muted);
    font-weight: 600;
  }

  .stat-info .value {
    font-size: 18px;
    font-weight: 700;
    color: var(--text-normal);
  }

  .all-stats-table-wrapper {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .all-stats-table-wrapper h3 {
    font-size: 16px;
    font-weight: 600;
  }

  .all-stats-table {
    width: 100%;
    border-collapse: collapse;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    overflow: hidden;
    font-size: 13px;
  }

  .all-stats-table th, .all-stats-table td {
    padding: 10px 14px;
    text-align: left;
    border-bottom: 1px solid var(--border-color);
  }

  .all-stats-table th {
    background-color: var(--bg-header);
    font-weight: 600;
    color: var(--text-muted);
  }

  .all-stats-table td code {
    background-color: rgba(255,255,255,0.05);
    padding: 2px 4px;
    border-radius: 4px;
    font-family: monospace;
  }

  .all-stats-table tr:hover {
    background-color: rgba(255,255,255,0.01);
  }

  /* Modal stylings */
  .modal-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background-color: rgba(0,0,0,0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal-content {
    background-color: var(--bg-normal, var(--bg-content));
    border: 1px solid var(--border-color, var(--border-color));
    border-radius: 12px;
    padding: 24px;
    width: 480px;
    box-shadow: 0 10px 30px rgba(0,0,0,0.3);
  }

  .modal-content h3 {
    font-size: 18px;
    font-weight: 700;
    margin-bottom: 16px;
  }

  .modal-content .form-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-bottom: 16px;
  }

  .modal-content label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }

  .modal-content input, .modal-content textarea {
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--border-color);
    background-color: var(--bg-sidebar);
    color: var(--text-normal);
    border-radius: 6px;
    font-size: 13px;
    outline: none;
  }

  .modal-content textarea {
    height: 120px;
    resize: vertical;
    font-family: monospace;
  }

  .modal-content input:focus, .modal-content textarea:focus {
    border-color: var(--color-primary);
  }

  .modal-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    margin-top: 24px;
  }

  .modal-actions .btn-cancel {
    padding: 8px 16px;
    background: transparent;
    border: 1px solid var(--border-color);
    color: var(--text-normal);
    border-radius: 6px;
    font-size: 13px;
    cursor: pointer;
  }

  .modal-actions .btn-cancel:hover {
    background-color: rgba(255,255,255,0.05);
  }

  .modal-actions .btn-submit {
    padding: 8px 16px;
    background-color: var(--color-primary);
    border: none;
    color: var(--bg-app);
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .modal-actions .btn-submit:hover {
    background-color: var(--color-primary-hover);
  }

  .loading-keys, .empty-keys, .panel-loading {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 40px 20px;
    color: var(--text-muted);
    font-size: 13px;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid rgba(255,255,255,0.1);
    border-radius: 50%;
    border-top-color: var(--color-primary);
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }
</style>
