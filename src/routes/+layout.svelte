<script lang="ts">
  import "../app.css";
  interface Props {
    children: any;
  }
  let { children }: Props = $props();

  let zoomLevel = $state(1.0);

  function handleKeydown(e: KeyboardEvent) {
    if (e.metaKey || e.ctrlKey) {
      if (e.key === '=' || e.key === '+') {
        e.preventDefault();
        zoomLevel = Math.min(zoomLevel + 0.1, 3.0);
      } else if (e.key === '-') {
        e.preventDefault();
        zoomLevel = Math.max(zoomLevel - 0.1, 0.5);
      } else if (e.key === '0') {
        e.preventDefault();
        zoomLevel = 1.0;
      }
    }
  }

  $effect(() => {
    document.body.style.zoom = zoomLevel.toString();
  });
</script>

<svelte:window onkeydown={handleKeydown} />

{@render children()}
