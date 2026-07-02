<script lang="ts">
  import { ChevronDown, ChevronRight, Cpu } from "lucide-svelte";
  import PlanNodeView from "./PlanNodeView.svelte";

  interface Props {
    node: any;
    depth?: number;
  }
  let { node, depth = 0 }: Props = $props();
  let expanded = $state(true);
</script>

<div class="plan-node-box">
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="plan-node-header" onclick={() => expanded = !expanded}>
    {#if node["Plans"] && node["Plans"].length > 0}
      <span class="toggle-wrapper">
        {#if expanded}
          <ChevronDown size={14} class="toggle-icon" />
        {:else}
          <ChevronRight size={14} class="toggle-icon" />
        {/if}
      </span>
    {:else}
      <span class="empty-toggle"></span>
    {/if}
    <Cpu size={14} class="node-icon" />
    <span class="node-type">{node["Node Type"]}</span>
    {#if node["Relation Name"]}
      <span class="relation-info">on <strong>{node["Relation Name"]}</strong>{#if node["Alias"]} (as {node["Alias"]}){/if}</span>
    {/if}
    <span class="node-cost">(cost={node["Startup Cost"]}..{node["Total Cost"]} rows={node["Plan Rows"]} width={node["Plan Width"]})</span>
  </div>

  {#if expanded && node["Plans"]}
    <div class="plan-children" style="border-left: 1px dashed var(--border-color); margin-left: 16px; padding-left: 12px;">
      {#each node["Plans"] as child}
        <PlanNodeView node={child} depth={depth + 1} />
      {/each}
    </div>
  {/if}
</div>

<style>
  .plan-node-box {
    margin-bottom: 6px;
    font-family: var(--font-mono, monospace);
    font-size: 13px;
  }
  .plan-node-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px;
    border-radius: 4px;
    background-color: var(--bg-card);
    border: 1px solid var(--border-color);
    cursor: pointer;
    user-select: none;
    transition: background-color 0.15s ease;
  }
  .plan-node-header:hover {
    background-color: var(--bg-hover);
  }
  .toggle-wrapper {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }
  .empty-toggle {
    width: 14px;
  }
  .node-icon {
    color: var(--color-primary);
  }
  .node-type {
    font-weight: 600;
    color: var(--text-normal);
  }
  .relation-info {
    color: var(--text-muted);
  }
  .node-cost {
    font-size: 11px;
    color: var(--text-muted);
    margin-left: auto;
  }
  .plan-children {
    margin-top: 4px;
  }
</style>
