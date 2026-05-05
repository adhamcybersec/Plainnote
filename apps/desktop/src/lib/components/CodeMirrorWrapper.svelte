<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	CodeMirrorWrapper.svelte — owns the CodeMirror 6 EditorView lifecycle
	in a Svelte 5 `$effect`. The rest of the app interacts through the
	imperative API exported below, never via reactive bindings into the
	editor's internal state. This is the pattern from `@frontend-design`:
	wrap third-party stateful libraries once, expose imperative methods,
	and never let Svelte's reactivity fight the library's own state.

	Why no two-way bind on `value`:
	  - CM6's transactions are the source of truth for the doc.
	  - A Svelte rune mirroring the doc would race on every keystroke
	    and corrupt the editor on rapid input.

	Caller pattern:
	    let editor: EditorRef | undefined = $state();
	    <CodeMirrorWrapper bind:this={editor} value={initial} onChange={...} />
	    // later:
	    editor?.applyOp({ kind: 'insert', pos: 0, text: 'x' });
-->
<script lang="ts" module>
	export type EditorOp =
		| { kind: 'insert'; pos: number; text: string }
		| { kind: 'delete'; pos: number; len: number }
		| { kind: 'replace'; pos: number; len: number; text: string };

	export interface EditorRef {
		getValue: () => string;
		applyOp: (op: EditorOp) => void;
	}
</script>

<script lang="ts">
	import { EditorState } from '@codemirror/state';
	import { EditorView, keymap, lineNumbers } from '@codemirror/view';
	import { defaultKeymap, history, historyKeymap } from '@codemirror/commands';
	import { markdown } from '@codemirror/lang-markdown';
	import {
		wikilinkAutocomplete,
		type TitleSearcher
	} from '$lib/components/wikilink-autocomplete';

	let {
		value = '',
		onChange = (_v: string) => {},
		// Optional: pass a custom searcher for tests / non-Tauri contexts.
		// Undefined disables the wikilink autocomplete extension entirely.
		titleSearcher = undefined
	} = $props<{
		value?: string;
		onChange?: (v: string) => void;
		titleSearcher?: TitleSearcher | undefined;
	}>();

	let host: HTMLDivElement | undefined = $state();
	let view: EditorView | undefined;

	$effect(() => {
		if (!host) return;
		view = new EditorView({
			parent: host,
			state: EditorState.create({
				doc: value,
				extensions: [
					lineNumbers(),
					history(),
					markdown(),
					keymap.of([...defaultKeymap, ...historyKeymap]),
					...(titleSearcher ? [wikilinkAutocomplete(titleSearcher)] : []),
					EditorView.updateListener.of((u) => {
						if (u.docChanged) onChange(u.state.doc.toString());
					})
				]
			})
		});
		return () => {
			view?.destroy();
			view = undefined;
		};
	});

	export function getValue(): string {
		return view?.state.doc.toString() ?? '';
	}

	export function applyOp(op: EditorOp): void {
		if (!view) return;
		switch (op.kind) {
			case 'insert':
				view.dispatch({
					changes: { from: op.pos, to: op.pos, insert: op.text }
				});
				break;
			case 'delete':
				view.dispatch({
					changes: { from: op.pos, to: op.pos + op.len }
				});
				break;
			case 'replace':
				view.dispatch({
					changes: { from: op.pos, to: op.pos + op.len, insert: op.text }
				});
				break;
		}
	}
</script>

<!-- ADR-005: no component-scoped <style>; styles for .cm-editor live in src/lib/tokens.css -->
<div bind:this={host} class="pn-editor h-full"></div>
