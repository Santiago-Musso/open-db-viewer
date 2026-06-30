<script lang="ts">
  import { appState } from "../state.svelte";
  import { onMount } from "svelte";
  import { Terminal, Search } from "lucide-svelte";
  
  let { isOpen = $bindable(false), onAction } = $props<{
    isOpen: boolean;
    onAction?: (action: string) => void;
  }>();

  let searchQuery = $state("");
  let selectedIndex = $state(0);

  let options = $derived.by(() => {
    const list = [
      { id: "new-tab", name: "Workspace: New SQL Editor Tab", category: "Workspace" },
      { id: "run-query", name: "Query: Run Current SQL Statement", category: "Execution" },
      { id: "toggle-theme", name: "Appearance: Toggle Light / Dark Theme", category: "Appearance" },
      { id: "toggle-er", name: "View: Switch to ER Diagram Mode", category: "View" },
      { id: "toggle-query", name: "View: Switch to SQL Editor Mode", category: "View" },
    ];

    appState.connections.forEach(c => {
      list.push({
        id: `connect-${c.id}`,
        name: `Database: Connect to ${c.name}`,
        category: "Connection"
      });
    });

    if (appState.activeConnectionId) {
      list.push({
        id: "disconnect",
        name: "Database: Disconnect Current Connection",
        category: "Connection"
      });
    }

    return list.filter(item => 
      item.name.toLowerCase().includes(searchQuery.toLowerCase()) || 
      item.category.toLowerCase().includes(searchQuery.toLowerCase())
    );
  });

  $effect(() => {
    searchQuery;
    selectedIndex = 0;
  });

  function handleKeyDown(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      selectedIndex = options.length > 0 ? (selectedIndex + 1) % options.length : 0;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      selectedIndex = options.length > 0 ? (selectedIndex - 1 + options.length) % options.length : 0;
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (options[selectedIndex]) {
        triggerAction(options[selectedIndex]);
      }
    } else if (e.key === "Escape") {
      isOpen = false;
    }
  }

  function triggerAction(option: { id: string, name: string }) {
    if (onAction) {
      onAction(option.id);
    }
    isOpen = false;
    searchQuery = "";
  }

  function handleGlobalKeyDown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === "k") {
      e.preventDefault();
      isOpen = !isOpen;
      searchQuery = "";
    }
  }

  onMount(() => {
    window.addEventListener("keydown", handleGlobalKeyDown);
    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown);
    };
  });
</script>

{#if isOpen}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="palette-backdrop" onclick={() => isOpen = false}>
    <div class="palette-modal" onclick={e => e.stopPropagation()}>
      <div class="palette-search">
        <Search size={18} class="search-icon" />
        <input 
          type="text" 
          placeholder="Search commands (e.g. theme, connect, run)..." 
          bind:value={searchQuery}
          onkeydown={handleKeyDown}
          autofocus
        />
      </div>

      <div class="palette-results">
        {#if options.length === 0}
          <div class="no-results">No commands found matching search query.</div>
        {:else}
          {#each options as opt, i}
            <div 
              class="result-item" 
              class:selected={selectedIndex === i}
              onclick={() => triggerAction(opt)}
            >
              <Terminal size={14} class="item-icon" />
              <div class="item-details">
                <span class="item-name">{opt.name}</span>
                <span class="item-category">{opt.category}</span>
              </div>
            </div>
          {/each}
        {/if}
      </div>
      
      <div class="palette-footer">
        <span>↑↓ to navigate</span>
        <span>enter to select</span>
        <span>esc to close</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .palette-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background-color: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 15vh;
    z-index: 10000;
  }

  .palette-modal {
    width: 600px;
    background-color: var(--bg-content);
    border: 1px solid var(--border-color);
    border-radius: 12px;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.5);
    overflow: hidden;
    display: flex;
    flex-direction: column;
    max-height: 400px;
  }

  .palette-search {
    display: flex;
    align-items: center;
    padding: 16px;
    border-bottom: 1px solid var(--border-color);
    gap: 12px;
  }

  .search-icon {
    color: var(--text-muted);
  }

  .palette-search input {
    flex: 1;
    background: transparent;
    border: none;
    color: var(--text-normal);
    font-size: 15px;
    outline: none;
    font-family: Inter, system-ui, sans-serif;
  }

  .palette-results {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .no-results {
    padding: 16px;
    color: var(--text-muted);
    font-size: 13px;
    text-align: center;
  }

  .result-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 14px;
    border-radius: 8px;
    cursor: pointer;
    transition: background-color 0.15s;
  }

  .result-item:hover, .result-item.selected {
    background-color: rgba(137, 180, 250, 0.15);
  }

  .item-icon {
    color: var(--text-muted);
  }

  .result-item:hover .item-icon, .result-item.selected .item-icon {
    color: var(--color-primary);
  }

  .item-details {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex: 1;
  }

  .item-name {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-normal);
  }

  .item-category {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    color: var(--text-muted);
    background-color: rgba(255, 255, 255, 0.05);
    padding: 2px 6px;
    border-radius: 4px;
    letter-spacing: 0.5px;
  }

  .palette-footer {
    display: flex;
    justify-content: flex-end;
    gap: 16px;
    padding: 10px 16px;
    border-top: 1px solid var(--border-color);
    background-color: var(--bg-sidebar);
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
