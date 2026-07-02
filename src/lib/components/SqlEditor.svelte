<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { EditorState, Compartment, StateField, StateEffect } from "@codemirror/state";
  import { EditorView, keymap, lineNumbers, Decoration, type DecorationSet } from "@codemirror/view";
  import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
  import { autocompletion, acceptCompletion } from "@codemirror/autocomplete";
  import { sql, PostgreSQL } from "@codemirror/lang-sql";
  import { appState } from "../state.svelte";

  interface Props {
    value: string;
    onChange: (val: string) => void;
    onExecute: () => void;
    error?: any;
  }

  let { value, onChange, onExecute, error }: Props = $props();

  let container = $state<HTMLDivElement | null>(null);
  let view = $state<EditorView | null>(null);
  
  const sqlCompartment = new Compartment();

  // State Effect and Field for syntax error squiggly underlines
  const setErrorUnderline = StateEffect.define<{from: number, to: number} | null>();

  const errorUnderlineField = StateField.define<DecorationSet>({
    create() { return Decoration.none; },
    update(underlines, tr) {
      underlines = underlines.map(tr.changes);
      for (let e of tr.effects) {
        if (e.is(setErrorUnderline)) {
          if (e.value) {
            underlines = Decoration.set([
              Decoration.mark({class: "cm-error-underline"}).range(e.value.from, e.value.to)
            ]);
          } else {
            underlines = Decoration.none;
          }
        }
      }
      return underlines;
    },
    provide: f => EditorView.decorations.from(f)
  });

  // Compute CodeMirror schema mapping from our global ER graph
  let cmSchema = $derived.by(() => {
    const s: Record<string, string[]> = {};
    if (appState.schemaNodes) {
      for (const node of appState.schemaNodes) {
        s[node.id] = (node.columns || []).map((c: any) => c.name);
      }
    }
    return s;
  });

  onMount(() => {
    if (!container) return;

    const startState = EditorState.create({
      doc: value,
      extensions: [
        lineNumbers(),
        history(),
        autocompletion(),
        errorUnderlineField,
        sqlCompartment.of(sql({ dialect: PostgreSQL, schema: cmSchema })),
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
          {
            key: "Tab",
            run: acceptCompletion,
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
          ".cm-gutters": { backgroundColor: "var(--bg-editor-gutter)", borderRight: "1px solid var(--border-color)" },
          ".cm-tooltip.cm-tooltip-autocomplete": {
            backgroundColor: "var(--bg-content)",
            border: "1px solid var(--border-color)",
            color: "var(--text-normal)",
            borderRadius: "4px",
            boxShadow: "0 4px 12px rgba(0,0,0,0.3)"
          },
          ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
            backgroundColor: "var(--color-primary)",
            color: "#ffffff"
          },
          ".cm-error-underline": {
            textDecoration: "underline wavy var(--color-error, #ef4444)",
            backgroundColor: "rgba(239, 68, 68, 0.1)"
          }
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

  // Reactively reconfigure the SQL language plugin whenever the DB schema changes
  $effect(() => {
    if (view) {
      view.dispatch({
        effects: sqlCompartment.reconfigure(sql({ dialect: PostgreSQL, schema: cmSchema }))
      });
    }
  });

  // Reactively set the error squiggly underline whenever error coordinates change
  $effect(() => {
    if (view) {
      if (error && typeof error === "object" && error.position !== null && error.position !== undefined) {
        const pos = Number(error.position);
        const zeroIndexedPos = pos > 0 ? pos - 1 : 0;
        const docLen = view.state.doc.length;
        const from = Math.min(zeroIndexedPos, docLen);
        let to = from;
        while (to < docLen && !/\s/.test(view.state.doc.slice(to, to + 1).toString())) {
          to++;
        }
        if (from === to && from > 0) {
          to = Math.min(from + 1, docLen);
        }
        view.dispatch({
          effects: setErrorUnderline.of({ from, to })
        });
      } else {
        view.dispatch({
          effects: setErrorUnderline.of(null)
        });
      }
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
