// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Wikilink autocomplete logic — pure-function tests for the trigger
 * detector and the CM6 completion source. The CM6 extension wiring is
 * covered indirectly through the completion-source contract.
 */
import { EditorState } from '@codemirror/state';
import {
	detectWikilinkTrigger,
	wikilinkCompletionSource,
	type TitleHitLite
} from '$lib/components/wikilink-autocomplete';
import type { CompletionContext } from '@codemirror/autocomplete';

describe('detectWikilinkTrigger', () => {
	it('returns null when no [[ is present', () => {
		expect(detectWikilinkTrigger('just plain text')).toBeNull();
	});

	it('detects an empty trigger right after [[', () => {
		const r = detectWikilinkTrigger('see [[');
		expect(r).toEqual({ from: 6, prefix: '' });
	});

	it('returns the typed prefix after [[', () => {
		const r = detectWikilinkTrigger('look at [[Calc');
		expect(r?.prefix).toBe('Calc');
		// `from` points at the position right after `[[`.
		expect(r?.from).toBe(10);
	});

	it('ignores closed wikilinks', () => {
		// `[[Done]]` is already closed; no trigger.
		expect(detectWikilinkTrigger('a [[Done]] b')).toBeNull();
	});

	it('finds the LAST unclosed [[ when multiple exist', () => {
		// First link is closed, second is open.
		const r = detectWikilinkTrigger('[[A]] then [[Bee');
		expect(r?.prefix).toBe('Bee');
	});

	it('respects the escape rule from core/links.rs', () => {
		// `\[[X` is escaped: no trigger.
		expect(detectWikilinkTrigger('see \\[[X')).toBeNull();
		// `\\[[X` — backslash escapes backslash, so [[ is real.
		const r = detectWikilinkTrigger('\\\\[[X');
		expect(r?.prefix).toBe('X');
	});
});

// Helper: build a CompletionContext at the given doc offset.
function ctx(doc: string, pos: number): CompletionContext {
	const state = EditorState.create({ doc });
	return {
		state,
		pos,
		explicit: true,
		matchBefore() {
			return null;
		},
		aborted: false,
		tokenBefore() {
			return null;
		}
	} as unknown as CompletionContext;
}

describe('wikilinkCompletionSource', () => {
	it('returns null when not inside a wikilink', () => {
		const src = wikilinkCompletionSource(async () => []);
		const result = src(ctx('hello world', 5));
		return expect(result).resolves.toBeNull();
	});

	it('does not call the searcher when prefix is empty', async () => {
		let calls = 0;
		const src = wikilinkCompletionSource(async () => {
			calls += 1;
			return [];
		});
		const r = await src(ctx('see [[', 6));
		expect(calls).toBe(0);
		expect(r?.options).toEqual([]);
		expect(r?.from).toBe(6);
	});

	it('passes the prefix to the searcher and returns mapped completions', async () => {
		const hits: TitleHitLite[] = [
			{ id: '01HXYZ0000000000000000000A', title: 'Calculus' },
			{ id: '01HXYZ0000000000000000000B', title: 'calculus II' }
		];
		const src = wikilinkCompletionSource(async (prefix, limit) => {
			expect(prefix).toBe('Calc');
			expect(limit).toBe(8);
			return hits;
		});
		const r = await src(ctx('see [[Calc', 10));
		expect(r?.options.length).toBe(2);
		expect(r?.options[0].label).toBe('Calculus');
		// `apply` inserts the ULID form `<id>]]` — the user-typed prefix is
		// replaced from `from`, and the closer comes with the completion so
		// the link is well-formed in one keystroke.
		expect(r?.options[0].apply).toBe('01HXYZ0000000000000000000A]]');
	});

	it('from points at the position right after [[', async () => {
		const src = wikilinkCompletionSource(async () => [
			{ id: '01HXYZ0000000000000000000A', title: 'X' }
		]);
		// Doc is "abc [[Q"; position right after the second `[` is index 6.
		const r = await src(ctx('abc [[Q', 7));
		expect(r?.from).toBe(6);
	});
});
