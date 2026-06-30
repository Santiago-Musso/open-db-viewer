<script lang="ts">
  import { appState } from "$lib/state.svelte";
  import { invoke } from "@tauri-apps/api/core";
  import SqlEditor from "../lib/components/SqlEditor.svelte";
  import ResultGrid from "../lib/components/ResultGrid.svelte";
  import SchemaTree from "../lib/components/SchemaTree.svelte";
  import RedisBrowser from "../lib/components/RedisBrowser.svelte";
  import ErDiagram from "../lib/components/ErDiagram.svelte";
  import SnippetsManager from "../lib/components/SnippetsManager.svelte";
  import CommandPalette from "../lib/components/CommandPalette.svelte";
  import { 
    Play, 
    Square, 
    Plus, 
    X, 
    Database, 
    Sun, 
    Moon, 
    Trash2, 
    Server, 
    LogOut 
  } from "lucide-svelte";

  // Connection form state
  let showForm = $state(false);
  let formName = $state("Local Postgres");
  let formDriver = $state<"postgres" | "redis">("postgres");
  let formHost = $state("localhost");
  let formPort = $state(5432);
  let formUser = $state("postgres");
  let formDb = $state("postgres");
  let formPass = $state("");
  
  let connectionError = $state<string | null>(null);
  let isConnecting = $state(false);
  let isTesting = $state(false);
  let testResult = $state<{ success: boolean; message: string } | null>(null);
  let theme = $state("dark");
  let activeConn = $derived(appState.connections.find(c => c.id === appState.activeConnectionId));
  let workspaceMode = $state<"query" | "er">("query");
  let isPaletteOpen = $state(false);

  async function handlePaletteAction(actionId: string) {
    if (actionId === "new-tab") {
      appState.openNewTab();
      workspaceMode = "query";
    } else if (actionId === "run-query") {
      appState.executeQuery();
    } else if (actionId === "toggle-theme") {
      toggleTheme();
    } else if (actionId === "toggle-er") {
      workspaceMode = "er";
    } else if (actionId === "toggle-query") {
      workspaceMode = "query";
    } else if (actionId === "disconnect") {
      appState.disconnect();
    } else if (actionId.startsWith("connect-")) {
      const connId = actionId.replace("connect-", "");
      await handleConnect(connId);
    }
  }

  let showExportModal = $state(false);
  let exportPath = $state("/Users/santiagomusso/Downloads/export.csv");
  let exportFormat = $state<"csv" | "json">("csv");
  let isExporting = $state(false);
  let exportError = $state<string | null>(null);
  let exportSuccess = $state(false);

  async function handleExport() {
    if (!appState.activeTab || !appState.activeConnectionId) return;
    isExporting = true;
    exportError = null;
    exportSuccess = false;
    try {
      await invoke("export_query_results", {
        connectionId: appState.activeConnectionId,
        sql: appState.activeTab.sql,
        filePath: exportPath,
        format: exportFormat
      });
      exportSuccess = true;
      setTimeout(() => {
        showExportModal = false;
        exportSuccess = false;
      }, 2000);
    } catch (e: any) {
      exportError = e.toString();
    } finally {
      isExporting = false;
    }
  }

  function handleDriverChange(event: Event) {
    const driver = (event.target as HTMLSelectElement).value as "postgres" | "redis";
    formDriver = driver;
    formPort = driver === "postgres" ? 5432 : 6379;
    if (driver === "redis") {
      formUser = "";
      formDb = "";
    } else {
      formUser = "postgres";
      formDb = "postgres";
    }
  }

  async function handleConnect(connId: string) {
    isConnecting = true;
    connectionError = null;
    workspaceMode = "query";
    try {
      await appState.connect(connId);
    } catch (e: any) {
      connectionError = e.message || "Failed to connect";
    } finally {
      isConnecting = false;
    }
  }

  async function handleTestConnection() {
    isTesting = true;
    testResult = null;
    try {
      await appState.testConnection({
        name: formName,
        driver: formDriver,
        host: formHost,
        port: formPort,
        user: formUser || undefined,
        db_name: formDb || undefined,
        password: formPass || undefined
      });
      testResult = { success: true, message: "Connection successful!" };
    } catch (e: any) {
      testResult = { success: false, message: e.message || "Connection failed." };
    } finally {
      isTesting = false;
    }
  }

  async function handleFormSubmit(e: Event) {
    e.preventDefault();
    const connId = await appState.addConnection({
      name: formName,
      driver: formDriver,
      host: formHost,
      port: formPort,
      user: formUser || undefined,
      db_name: formDb || undefined,
      password: formPass || undefined
    });
    showForm = false;
    testResult = null;
    await handleConnect(connId);
  }

  function toggleTheme() {
    theme = theme === "dark" ? "light" : "dark";
    if (typeof document !== "undefined") {
      document.documentElement.setAttribute("data-theme", theme);
    }
  }

  // Sidebar resizer state
  let sidebarWidth = $state(250);
  let isResizing = false;

  function startResizing(e: MouseEvent) {
    e.preventDefault();
    isResizing = true;
    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", stopResizing);
  }

  function handleMouseMove(e: MouseEvent) {
    if (!isResizing) return;
    const newWidth = Math.max(150, Math.min(600, e.clientX));
    sidebarWidth = newWidth;
  }

  function stopResizing() {
    isResizing = false;
    window.removeEventListener("mousemove", handleMouseMove);
    window.removeEventListener("mouseup", stopResizing);
  }

  let globalErrors = $state<string[]>([]);
  if (typeof window !== "undefined") {
    window.addEventListener("error", (event) => {
      globalErrors.push(`${event.message} at ${event.filename}:${event.lineno}`);
    });
    window.addEventListener("unhandledrejection", (event) => {
      globalErrors.push(`Unhandled Rejection: ${event.reason}`);
    });
  }
</script>

{#if globalErrors.length > 0}
  <div style="background: #ef4444; color: white; padding: 16px; font-family: monospace; z-index: 99999; position: fixed; top: 0; left: 0; right: 0; max-height: 200px; overflow-y: auto; box-shadow: 0 4px 6px rgba(0,0,0,0.2);">
    <h3 style="margin: 0 0 8px 0; font-size: 14px;">Unhandled Application Errors:</h3>
    <ul style="margin: 0 0 12px 0; padding-left: 20px; font-size: 12px;">
      {#each globalErrors as err}
        <li>{err}</li>
      {/each}
    </ul>
    <button style="background: white; color: #ef4444; border: none; padding: 4px 8px; border-radius: 4px; cursor: pointer; font-size: 11px; font-weight: bold;" onclick={() => globalErrors = []}>Clear</button>
  </div>
{/if}

<div class="app-layout">
  {#if !appState.activeConnectionId}
    <!-- Connection Launcher Panel -->
    <div class="connection-launcher">
      <div class="connection-card">
        <div class="header">
          <Database size={24} class="db-logo" />
          <h1>Database Client Launcher</h1>
        </div>

        {#if connectionError}
          <div class="error-banner">{connectionError}</div>
        {/if}

        {#if isConnecting}
          <div class="loading-state">
            <div class="spinner"></div>
            <p>Establishing connection to database...</p>
          </div>
        {:else if showForm}
          <!-- Connection Creator Form -->
          <form onsubmit={handleFormSubmit} class="conn-form">
            <h2>New Connection Profile</h2>
            
            <div class="form-group">
              <label for="profile-name">Profile Name</label>
              <input id="profile-name" type="text" bind:value={formName} required />
            </div>

            <div class="form-row">
              <div class="form-group">
                <label for="driver-select">Database Engine</label>
                <select id="driver-select" value={formDriver} onchange={handleDriverChange}>
                  <option value="postgres">PostgreSQL</option>
                  <option value="redis">Redis (Phase 3)</option>
                </select>
              </div>

              <div class="form-group">
                <label for="host-input">Host</label>
                <input id="host-input" type="text" bind:value={formHost} required />
              </div>
            </div>

            <div class="form-row">
              <div class="form-group">
                <label for="port-input">Port</label>
                <input id="port-input" type="number" bind:value={formPort} required />
              </div>

              {#if formDriver === 'postgres'}
                <div class="form-group">
                  <label for="user-input">Username</label>
                  <input id="user-input" type="text" bind:value={formUser} />
                </div>
              {/if}
            </div>

            {#if formDriver === 'postgres'}
              <div class="form-group">
                <label for="db-input">Database Name</label>
                <input id="db-input" type="text" bind:value={formDb} />
              </div>
            {/if}

            <div class="form-group">
              <label for="pass-input">Password</label>
              <input id="pass-input" type="password" bind:value={formPass} placeholder="••••••••" />
            </div>

            {#if testResult}
              <div class="test-result-banner" class:success={testResult.success}>
                {testResult.message}
              </div>
            {/if}

            <div class="form-actions">
              <button type="button" class="btn-cancel" onclick={() => { showForm = false; testResult = null; }}>Cancel</button>
              <button type="button" class="btn-test" disabled={isTesting} onclick={handleTestConnection}>
                {#if isTesting}Testing...{:else}Test Connection{/if}
              </button>
              <button type="submit" class="btn-submit">Connect & Save</button>
            </div>
          </form>
        {:else}
          <!-- Saved Connections List -->
          <div class="connections-list-section">
            <div class="list-header">
              <h2>Saved Profiles</h2>
              <button class="btn-new" onclick={() => { showForm = true; testResult = null; }}>
                <Plus size={16} /> New Profile
              </button>
            </div>

            {#if appState.connections.length === 0}
              <div class="no-connections">
                <p>No saved connections yet.</p>
                <button class="btn-start" onclick={() => { showForm = true; testResult = null; }}>Create one now</button>
              </div>
            {:else}
              <div class="connections-grid">
                {#each appState.connections as conn}
                  <div class="connection-item">
                    <div class="conn-info" onclick={() => handleConnect(conn.id)} role="button" tabindex="0" onkeydown={(e) => e.key === 'Enter' && handleConnect(conn.id)}>
                      <Server size={18} class="conn-icon" />
                      <div class="details">
                        <span class="name">{conn.name}</span>
                        <span class="meta">{conn.driver.toUpperCase()} • {conn.host}:{conn.port}</span>
                      </div>
                    </div>
                    <button class="btn-delete" aria-label="Delete connection" onclick={() => appState.removeConnection(conn.id)}>
                      <Trash2 size={16} />
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  {:else}
    <!-- Main Connected Workspace -->
    {#if activeConn?.driver === 'redis'}
      <div class="main-workspace" style="display: block;">
        <RedisBrowser />
      </div>
    {:else}
      <div class="main-workspace">
        <!-- Left Sidebar: Schema Explorer -->
        <aside class="sidebar" style="width: {sidebarWidth}px;">
          <div class="sidebar-header">
            <div class="connection-summary">
              <Server size={16} class="active-icon" />
              <div class="active-details">
                <h3>{activeConn?.name}</h3>
                <p>PostgreSQL</p>
              </div>
            </div>
          <button class="btn-disconnect" onclick={() => appState.disconnect()} title="Disconnect">
            <LogOut size={16} />
          </button>
        </div>

        <div class="sidebar-content" style="display: flex; flex-direction: column; height: calc(100% - 48px); padding: 0;">
          <div style="flex: 3; overflow-y: auto; display: flex; flex-direction: column; min-height: 180px; padding: 12px 0 0 0;">
            <div class="schema-explorer-label" style="padding: 0 12px 4px 12px;">Schema Explorer</div>
            <div style="flex: 1; overflow-y: auto; padding: 0 12px;">
              <SchemaTree />
            </div>
          </div>
          <div style="flex: 2; display: flex; flex-direction: column; min-height: 150px; border-top: 1px solid var(--border-color);">
            <SnippetsManager />
          </div>
        </div>
      </aside>

      <!-- Sidebar Resizer -->
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div 
        class="sidebar-resizer" 
        onmousedown={startResizing}
        role="separator"
        aria-valuenow={sidebarWidth}
        aria-valuemin={150}
        aria-valuemax={600}
      ></div>

      <!-- Center Pane: Editor and Result Grid -->
      <main class="content-pane">
        <!-- Editor Tabs -->
        <header class="tabs-header">
          <div class="tabs-list">
            {#each appState.tabs as tab}
              <div 
                class="tab-item" 
                class:active={appState.activeTabId === tab.id}
                onclick={() => appState.activeTabId = tab.id}
                role="button"
                tabindex="0"
                onkeydown={(e) => e.key === 'Enter' && (appState.activeTabId = tab.id)}
              >
                <span>{tab.name}</span>
                <button 
                  class="btn-close-tab" 
                  aria-label="Close tab"
                  onclick={(e) => { e.stopPropagation(); appState.closeTab(tab.id); }}
                >
                  <X size={12} />
                </button>
              </div>
            {/each}
          </div>
          <button class="btn-new-tab" onclick={() => { appState.openNewTab(); workspaceMode = 'query'; }} title="Open New Query Tab">
            <Plus size={16} />
          </button>

          <div style="flex: 1;"></div>

          <div class="workspace-modes">
            <button class="btn-mode" class:active={workspaceMode === 'query'} onclick={() => workspaceMode = 'query'}>
              SQL Editor
            </button>
            <button class="btn-mode" class:active={workspaceMode === 'er'} onclick={() => workspaceMode = 'er'}>
              ER Diagram
            </button>
          </div>
        </header>

        {#if workspaceMode === 'er'}
          <div class="workspace-area">
            <ErDiagram />
          </div>
        {:else if appState.activeTab}
          <div class="workspace-area">
            <!-- SQL Editor Container -->
            <div class="editor-pane">
              <SqlEditor 
                value={appState.activeTab.sql}
                onChange={(val) => { if (appState.activeTab) appState.activeTab.sql = val; }}
                onExecute={() => appState.executeQuery()}
              />
            </div>

            <!-- Run Control Bar -->
            <div class="control-bar">
              {#if appState.activeTab.loading}
                <button class="btn-control cancel" onclick={() => appState.cancelQuery()}>
                  <Square size={14} fill="currentColor" /> Cancel
                </button>
              {:else}
                <button class="btn-control run" onclick={() => appState.executeQuery()}>
                  <Play size={14} fill="currentColor" /> Run Query
                </button>
              {/if}

              <div class="query-meta-info">
                {#if appState.activeTab.loading}
                  <span class="status-msg loading">Running query...</span>
                {:else if appState.activeTab.error}
                  <span class="status-msg error">Query failed</span>
                {:else if appState.activeTab.rowCount > 0}
                  <span class="status-msg success">
                    Fetched {appState.activeTab.rowCount} rows
                    {#if appState.activeTab.executionTime !== null}
                      in {appState.activeTab.executionTime}ms
                    {/if}
                  </span>
                {/if}
              </div>

              <div style="flex: 1;"></div>

              {#if !appState.activeTab.loading && appState.activeTab.rowCount > 0}
                <button class="btn-control export" onclick={() => { showExportModal = true; exportSuccess = false; exportError = null; }}>
                  Export Data
                </button>
              {/if}
            </div>

            <!-- Results Panel -->
            <div class="results-pane">
              {#if appState.activeTab.loading && appState.activeTab.rows.length === 0}
                <div class="results-loading">
                  <div class="spinner"></div>
                  <p>Streaming query results...</p>
                </div>
              {:else if appState.activeTab.error}
                <div class="results-error">
                  <h3>Query Error</h3>
                  <pre>{appState.activeTab.error}</pre>
                </div>
              {:else}
                <ResultGrid 
                  columns={appState.activeTab.columns}
                  rows={appState.activeTab.rows}
                />
              {/if}
            </div>
          </div>
        {:else}
          <div class="no-tabs-state">
            <Plus size={32} class="plus-icon" />
            <p>No query editors open</p>
            <button class="btn-new-tab-center" onclick={() => appState.openNewTab()}>Open New Query Tab</button>
          </div>
        {/if}
      </main>
      </div>
    {/if}
  {/if}

  <!-- Footer Status Bar -->
  <footer class="status-bar">
    <div class="status-left">
      {#if appState.activeConnectionId}
        <span class="indicator connected"></span>
        <span class="status-text">Connected to {activeConn?.driver === 'redis' ? 'Redis' : 'PostgreSQL'}</span>
      {:else}
        <span class="indicator disconnected"></span>
        <span class="status-text">Idle</span>
      {/if}
    </div>
    <div class="status-right" style="display: flex; gap: 8px; align-items: center;">
      <button class="btn-palette" onclick={() => isPaletteOpen = true} title="Open Command Palette (⌘K)">
        <span>⌘K</span>
      </button>
      <button class="btn-theme" onclick={toggleTheme} title="Toggle Dark/Light Mode">
        {#if theme === 'dark'}
          <Sun size={14} />
        {:else}
          <Moon size={14} />
        {/if}
      </button>
    </div>
  </footer>
</div>

<CommandPalette bind:isOpen={isPaletteOpen} onAction={handlePaletteAction} />

{#if showExportModal}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="palette-backdrop" onclick={() => showExportModal = false}>
    <div class="palette-modal" onclick={e => e.stopPropagation()} style="width: 480px; padding: 20px;">
      <h4 style="margin: 0 0 16px 0; font-size: 16px; font-weight: 700; color: #89b4fa;">Export Query Results</h4>
      
      {#if exportSuccess}
        <div class="export-success-message" style="background-color: rgba(166, 227, 161, 0.1); border: 1px solid #a6e3a1; color: #a6e3a1; padding: 12px; border-radius: 6px; font-size: 13px; margin-bottom: 16px;">
          ✓ Results successfully exported to target file!
        </div>
      {:else}
        <form onsubmit={handleExport}>
          <div style="display: flex; flex-direction: column; gap: 12px; margin-bottom: 20px;">
            <div style="display: flex; flex-direction: column; gap: 6px;">
              <label for="export-format" style="font-size: 12px; color: var(--text-muted); font-weight: 600;">Export Format</label>
              <select id="export-format" bind:value={exportFormat} style="padding: 8px 12px; background-color: #11111b; border: 1px solid var(--border-color); border-radius: 6px; color: var(--text-normal); outline: none;">
                <option value="csv">CSV (Comma-Separated Values)</option>
                <option value="json">JSON Array of Objects</option>
              </select>
            </div>
            
            <div style="display: flex; flex-direction: column; gap: 6px;">
              <label for="export-path" style="font-size: 12px; color: var(--text-muted); font-weight: 600;">Destination Absolute Path</label>
              <input 
                type="text" 
                id="export-path" 
                bind:value={exportPath} 
                placeholder="e.g. /Users/santiagomusso/Downloads/export.csv" 
                required 
                style="padding: 8px 12px; background-color: #11111b; border: 1px solid var(--border-color); border-radius: 6px; color: var(--text-normal); outline: none;"
              />
            </div>
            
            {#if exportError}
              <div style="color: #f38ba8; font-size: 12px; word-break: break-all;">
                Failed to export: {exportError}
              </div>
            {/if}
          </div>
          
          <div style="display: flex; justify-content: flex-end; gap: 8px;">
            <button type="button" class="btn-secondary" onclick={() => showExportModal = false} style="padding: 6px 12px; background: transparent; border: 1px solid var(--border-color); color: var(--text-normal); border-radius: 4px; font-size: 12px; cursor: pointer;">Cancel</button>
            <button type="submit" class="btn-primary" disabled={isExporting} style="padding: 6px 12px; background-color: #89b4fa; border: none; color: #11111b; border-radius: 4px; font-size: 12px; font-weight: 600; cursor: pointer;">
              {isExporting ? 'Exporting...' : 'Export'}
            </button>
          </div>
        </form>
      {/if}
    </div>
  </div>
{/if}

<style>
  .app-layout {
    width: 100vw;
    height: 100vh;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-app);
    overflow: hidden;
  }

  /* Connection Launcher styling */
  .connection-launcher {
    flex: 1;
    display: flex;
    justify-content: center;
    align-items: center;
    padding: 20px;
    background: radial-gradient(circle at 50% 50%, var(--bg-content) 0%, var(--bg-app) 100%);
  }

  .connection-card {
    width: 100%;
    max-width: 500px;
    background-color: var(--bg-sidebar);
    border: 1px solid var(--border-color);
    border-radius: 8px;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3);
    padding: 24px;
    max-height: 90vh;
    overflow-y: auto;
  }

  .connection-card .header {
    display: flex;
    align-items: center;
    margin-bottom: 24px;
    padding-bottom: 16px;
    border-bottom: 1px solid var(--border-color);
  }

  :global(.db-logo) {
    color: var(--color-primary);
    margin-right: 12px;
  }

  .connection-card h1 {
    font-size: 18px;
    font-weight: 600;
  }

  .error-banner {
    background-color: rgba(239, 68, 68, 0.1);
    border: 1px solid rgb(239, 68, 68);
    color: rgb(239, 68, 68);
    padding: 10px 12px;
    border-radius: 4px;
    font-size: 13px;
    margin-bottom: 16px;
  }

  .loading-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 40px 0;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--border-color);
    border-top: 2px solid var(--color-primary);
    border-radius: 50%;
    animation: spin 1s linear infinite;
    margin-bottom: 16px;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  .conn-form h2 {
    font-size: 14px;
    color: var(--text-muted);
    margin-bottom: 16px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .form-group {
    display: flex;
    flex-direction: column;
    margin-bottom: 16px;
  }

  .form-group label {
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 6px;
  }

  .form-group input, .form-group select {
    background-color: var(--bg-app);
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-normal);
    padding: 8px 12px;
    font-size: 13px;
    outline: none;
  }

  .form-group input:focus, .form-group select:focus {
    border-color: var(--color-primary);
  }

  .form-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 16px;
  }

  .form-actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    margin-top: 20px;
  }

  .btn-cancel {
    background: none;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-normal);
    padding: 8px 16px;
    font-size: 13px;
    cursor: pointer;
  }

  .btn-submit {
    background-color: var(--color-primary);
    border: none;
    border-radius: 4px;
    color: white;
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-submit:hover {
    background-color: var(--color-primary-hover);
  }

  .btn-test {
    background-color: transparent;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-normal);
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    transition: background-color 0.2s, border-color 0.2s;
  }
  
  .btn-test:hover {
    background-color: var(--bg-hover, rgba(255, 255, 255, 0.05));
    border-color: var(--text-muted);
  }
  
  .btn-test:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .test-result-banner {
    padding: 10px 12px;
    border-radius: 4px;
    font-size: 13px;
    margin-bottom: 16px;
    background-color: rgba(239, 68, 68, 0.1);
    border: 1px solid rgba(239, 68, 68, 0.2);
    color: #ef4444;
  }
  
  .test-result-banner.success {
    background-color: rgba(34, 197, 94, 0.1);
    border: 1px solid rgba(34, 197, 94, 0.2);
    color: #22c55e;
  }

  .connections-list-section h2 {
    font-size: 14px;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .list-header {
    display: flex;
    justify-content: flex-between;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 16px;
  }

  .btn-new {
    display: flex;
    align-items: center;
    gap: 6px;
    background-color: var(--color-primary);
    border: none;
    border-radius: 4px;
    color: white;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-new:hover {
    background-color: var(--color-primary-hover);
  }

  .no-connections {
    text-align: center;
    padding: 40px 0;
  }

  .no-connections p {
    color: var(--text-muted);
    font-size: 13px;
    margin-bottom: 16px;
  }

  .btn-start {
    background-color: var(--color-primary);
    border: none;
    color: white;
    padding: 8px 16px;
    border-radius: 4px;
    font-size: 13px;
    cursor: pointer;
  }

  .connections-grid {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .connection-item {
    display: flex;
    align-items: center;
    background-color: var(--bg-content);
    border: 1px solid var(--border-color);
    border-radius: 4px;
    overflow: hidden;
  }

  .conn-info {
    flex: 1;
    display: flex;
    align-items: center;
    padding: 10px 12px;
    cursor: pointer;
    outline: none;
  }

  .conn-info:hover {
    background-color: var(--bg-hover);
  }

  :global(.conn-icon) {
    color: var(--text-muted);
    margin-right: 12px;
  }

  .connection-item .details {
    display: flex;
    flex-direction: column;
  }

  .connection-item .name {
    font-size: 13px;
    font-weight: 500;
  }

  .connection-item .meta {
    font-size: 11px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .btn-delete {
    background: none;
    border: none;
    padding: 10px;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
    border-left: 1px solid var(--border-color);
  }

  .btn-delete:hover {
    color: rgb(239, 68, 68);
    background-color: rgba(239, 68, 68, 0.05);
  }

  /* Main Workspace Workspace Layout */
  .main-workspace {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .sidebar {
    min-width: 150px;
    max-width: 600px;
    background-color: var(--bg-sidebar);
    border-right: 1px solid var(--border-color);
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .sidebar-resizer {
    width: 4px;
    cursor: col-resize;
    background-color: transparent;
    transition: background-color 0.2s;
    z-index: 100;
    align-self: stretch;
    flex-shrink: 0;
  }

  .sidebar-resizer:hover, .sidebar-resizer:active {
    background-color: var(--color-primary);
  }

  .sidebar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px;
    border-bottom: 1px solid var(--border-color);
    background-color: var(--bg-app);
  }

  .connection-summary {
    display: flex;
    align-items: center;
  }

  :global(.active-icon) {
    color: var(--color-column);
    margin-right: 8px;
  }

  .active-details h3 {
    font-size: 12px;
    font-weight: 600;
  }

  .active-details p {
    font-size: 10px;
    color: var(--text-muted);
  }

  .btn-disconnect {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    padding: 6px;
    border-radius: 4px;
  }

  .btn-disconnect:hover {
    background-color: var(--bg-hover);
    color: var(--text-normal);
  }

  .sidebar-content {
    flex: 1;
    overflow-y: auto;
  }

  .schema-explorer-label {
    padding: 12px 12px 4px 12px;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }

  .content-pane {
    flex: 1;
    display: flex;
    flex-direction: column;
    background-color: var(--bg-content);
    overflow: hidden;
  }

  .tabs-header {
    height: 36px;
    display: flex;
    background-color: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-color);
    align-items: center;
  }

  .workspace-modes {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 12px;
  }

  .btn-mode {
    background: transparent;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-muted);
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    transition: all 0.2s ease;
  }

  .btn-mode:hover {
    color: var(--text-normal);
    background-color: var(--bg-hover);
  }

  .btn-mode.active {
    background-color: var(--color-primary);
    border-color: var(--color-primary);
    color: #11111b;
  }

  .tabs-list {
    flex: 1;
    display: flex;
    overflow-x: auto;
    overflow-y: hidden;
  }

  .tab-item {
    display: flex;
    align-items: center;
    padding: 0 16px;
    border-right: 1px solid var(--border-color);
    font-size: 12px;
    color: var(--text-muted);
    cursor: pointer;
    background-color: var(--bg-sidebar);
    max-width: 150px;
    white-space: nowrap;
    outline: none;
  }

  .tab-item.active {
    color: var(--text-normal);
    background-color: var(--bg-content);
    border-bottom: 2px solid var(--color-primary);
  }

  .btn-close-tab {
    background: none;
    border: none;
    color: var(--text-muted);
    margin-left: 8px;
    cursor: pointer;
    display: flex;
    align-items: center;
    border-radius: 50%;
    padding: 2px;
  }

  .btn-close-tab:hover {
    background-color: var(--bg-hover);
    color: var(--text-normal);
  }

  .btn-new-tab {
    background: none;
    border: none;
    border-left: 1px solid var(--border-color);
    color: var(--text-muted);
    padding: 0 12px;
    cursor: pointer;
    display: flex;
    align-items: center;
  }

  .btn-new-tab:hover {
    color: var(--text-normal);
    background-color: var(--bg-hover);
  }

  .workspace-area {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .editor-pane {
    height: 40%;
    min-height: 100px;
  }

  .control-bar {
    height: 36px;
    background-color: var(--bg-sidebar);
    border-bottom: 1px solid var(--border-color);
    display: flex;
    align-items: center;
    padding: 0 12px;
    gap: 16px;
  }

  .btn-control {
    display: flex;
    align-items: center;
    gap: 6px;
    border: none;
    border-radius: 4px;
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }

  .btn-control.run {
    background-color: var(--color-primary);
    color: white;
  }

  .btn-control.run:hover {
    background-color: var(--color-primary-hover);
  }

  .btn-control.cancel {
    background-color: rgb(239, 68, 68);
    color: white;
  }

  .btn-control.cancel:hover {
    background-color: rgb(220, 38, 38);
  }

  .query-meta-info {
    font-size: 11px;
  }

  .status-msg.loading {
    color: var(--color-table);
  }

  .status-msg.error {
    color: rgb(239, 68, 68);
  }

  .status-msg.success {
    color: var(--color-column);
  }

  .results-pane {
    flex: 1;
    overflow: hidden;
  }

  .results-loading {
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    height: 100%;
    color: var(--text-muted);
    font-size: 13px;
  }

  .results-error {
    padding: 20px;
    height: 100%;
    overflow: auto;
    color: rgb(239, 68, 68);
  }

  .results-error h3 {
    font-size: 14px;
    font-weight: 600;
    margin-bottom: 10px;
  }

  .results-error pre {
    font-family: Menlo, Monaco, monospace;
    font-size: 12px;
    background-color: rgba(239, 68, 68, 0.05);
    border: 1px solid rgba(239, 68, 68, 0.2);
    padding: 12px;
    border-radius: 4px;
    white-space: pre-wrap;
  }

  .no-tabs-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    color: var(--text-muted);
  }

  :global(.plus-icon) {
    color: var(--border-color);
    margin-bottom: 16px;
  }

  .btn-new-tab-center {
    background-color: var(--color-primary);
    border: none;
    border-radius: 4px;
    color: white;
    padding: 8px 16px;
    font-size: 13px;
    margin-top: 16px;
    cursor: pointer;
  }

  /* Status Bar footer */
  .status-bar {
    height: 24px;
    background-color: var(--status-bar-bg);
    border-top: 1px solid var(--border-color);
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0 12px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .status-left {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .indicator {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .indicator.connected {
    background-color: var(--color-column);
  }

  .indicator.disconnected {
    background-color: var(--text-muted);
  }

  .btn-theme {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    display: flex;
    align-items: center;
  }

  .btn-theme:hover {
    color: var(--text-normal);
  }

  .btn-palette {
    background-color: rgba(255, 255, 255, 0.05);
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 10px;
    font-weight: 700;
    padding: 2px 6px;
    display: flex;
    align-items: center;
  }

  .btn-palette:hover {
    color: var(--text-normal);
    background-color: var(--bg-hover);
  }
</style>
