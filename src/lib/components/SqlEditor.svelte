<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { sql } from "@codemirror/lang-sql";

  interface Props {
    value: string;
    onChange: (val: string) => void;
    onExecute: () => void;
  }

  let { value, onChange, onExecute }: Props = $props();

  let container = $state<HTMLDivElement | null>(null);
  let view = $state<EditorView | null>(null);

  onMount(() => {
    if (!container) return;

    const startState = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        history(),
        sql(),
        keymap.of([
          ...defaultKeymap,
          ...historyKeymap,
          {
            key: "Mod-Enter",
            run: () => {
              onExecute();
              return true;
            },
          },
        ]),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChange(update.state.doc.toString());
          }
        }),
        EditorView.theme({
          "&": { height: "100%", fontSize: "13px", fontFamily: "'Fira Code', 'JetBrains Mono', Monaco, monospace" },
          ".cm-scroller": { overflow: "auto" },
          "&.cm-focused": { outline: "none" },
          ".cm-gutters": { backgroundColor: "var(--bg-editor-gutter)", borderRight: "1px solid var(--border-color)" }
        })
      ],
    });

    view = new EditorView({
      state: startState,
      parent: container,
    });
  });

  onDestroy(() => {
    if (view) {
      view.destroy();
    }
  });

  $effect(() => {
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value },
      });
    }
  });
</script>

<div class="editor-container" bind:this={container}></div>

<style>
  .editor-container {
    width: 100%;
    height: 100%;
    border-bottom: 1px solid var(--border-color);
    background-color: var(--bg-editor);
  }
  :global(.cm-editor) {
    height: 100%;
  }
</style>
