// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * CodeMirrorWrapper exposes an *imperative* API. The wrapper owns the
 * EditorView lifecycle inside a `$effect`; the rest of the app talks to
 * it through `getValue()` and `applyOp()` rather than reactive bindings.
 *
 * Why this shape:
 *   - CodeMirror's EditorView is its own state container. Mirroring the
 *     doc into a Svelte rune would race on every keystroke.
 *   - Tests and parents acquire the imperative ref via `bind:this` and
 *     call methods directly. No two-way binding into editor internals.
 */
import { render } from '@testing-library/svelte';
import CodeMirrorWrapper from '$lib/components/CodeMirrorWrapper.svelte';
import Harness from './harness/CodeMirrorHarness.svelte';

describe('CodeMirrorWrapper', () => {
	it('mounts an editor with the initial value', () => {
		const { container } = render(CodeMirrorWrapper, { value: 'hello world' });
		// CM6 renders a `.cm-editor` root with `.cm-content` carrying the doc.
		const root = container.querySelector('.cm-editor');
		expect(root).toBeTruthy();
		expect(container.querySelector('.cm-content')?.textContent).toContain('hello world');
	});

	it('exposes getValue() returning the current doc', () => {
		const harness = render(Harness, { initial: 'first line' });
		const ref = harness.component.getRef();
		expect(ref.getValue()).toBe('first line');
	});

	it('applyOp inserts text at the given position', () => {
		const harness = render(Harness, { initial: 'hello world' });
		const ref = harness.component.getRef();
		ref.applyOp({ kind: 'insert', pos: 5, text: ',' });
		expect(ref.getValue()).toBe('hello, world');
	});

	it('applyOp deletes a span', () => {
		const harness = render(Harness, { initial: 'hello, world' });
		const ref = harness.component.getRef();
		ref.applyOp({ kind: 'delete', pos: 5, len: 2 });
		expect(ref.getValue()).toBe('helloworld');
	});

	it('applyOp replace combines delete and insert', () => {
		const harness = render(Harness, { initial: 'hello world' });
		const ref = harness.component.getRef();
		ref.applyOp({ kind: 'replace', pos: 6, len: 5, text: 'there' });
		expect(ref.getValue()).toBe('hello there');
	});

	it('user typing fires onChange with the new doc', async () => {
		// Sanity that the wrapper reports user edits — the UI relies on this
		// to know when to debounce-save. We synthesize an op via the imperative
		// API rather than DOM keypress because jsdom does not implement the
		// contenteditable IME pathway CM6 uses.
		const calls: string[] = [];
		const harness = render(Harness, {
			initial: 'a',
			onChange: (v: string) => calls.push(v)
		});
		const ref = harness.component.getRef();
		ref.applyOp({ kind: 'insert', pos: 1, text: 'b' });
		expect(calls.at(-1)).toBe('ab');
	});
});
