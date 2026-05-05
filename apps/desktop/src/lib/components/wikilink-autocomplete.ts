// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * CodeMirror 6 autocomplete extension for `[[wikilinks]]`.
 *
 * Trigger: the cursor sits inside an open `[[…` (no closing `]]` between
 * the trigger and the cursor on the same line). Completion text is the
 * remaining title; we insert the ULID form `[[<id>]]` so links are
 * rename-stable (per ADR forthcoming in T8).
 *
 * The IPC fetcher is injected so this module is unit-testable without
 * Tauri; production code passes `searchNotesByTitle` from `$lib/ipc`.
 */
import {
	autocompletion,
	type CompletionContext,
	type CompletionResult,
	type Completion
} from '@codemirror/autocomplete';
import type { Extension } from '@codemirror/state';

export interface TitleHitLite {
	id: string;
	title: string;
}

export type TitleSearcher = (prefix: string, limit: number) => Promise<TitleHitLite[]>;

/**
 * Find an open `[[` immediately preceding the cursor on the same line.
 * Returns `{ from, prefix }` where `from` is the byte position right
 * after the `[[` and `prefix` is the user-typed text. Null means we are
 * not inside a wikilink trigger.
 *
 * Rules:
 *   - The match window is the current line up to the cursor.
 *   - Search backwards for the LAST unclosed `[[`. If a `]]` appears
 *     between it and the cursor, the link is already closed.
 *   - A `\` immediately before `[[` cancels the trigger (matches the
 *     parser's escape rule in core/links.rs).
 */
export function detectWikilinkTrigger(
	lineUpToCursor: string
): { from: number; prefix: string } | null {
	// Walk backwards looking for the most recent `[[` not preceded by `\`
	// (per the wikilink parser's escape rule).
	let i = lineUpToCursor.length - 2;
	while (i >= 0) {
		if (lineUpToCursor[i] === '[' && lineUpToCursor[i + 1] === '[') {
			// Escaped if a single backslash directly precedes, but not double.
			const prevIsBackslash = i > 0 && lineUpToCursor[i - 1] === '\\';
			const escaped = prevIsBackslash && (i < 2 || lineUpToCursor[i - 2] !== '\\');
			if (!escaped) {
				const inner = lineUpToCursor.slice(i + 2);
				// If there's a `]]` between the trigger and cursor, the link
				// is already closed — no trigger.
				if (inner.includes(']]')) return null;
				// Newline never appears (we sliced on a single line) but keep
				// a guard for malformed input.
				if (inner.includes('\n')) return null;
				return { from: i + 2, prefix: inner };
			}
		}
		i -= 1;
	}
	return null;
}

/**
 * Build a CodeMirror completion source bound to the given title searcher.
 * Exposed for unit testing — production calls `wikilinkAutocomplete()`.
 */
export function wikilinkCompletionSource(searcher: TitleSearcher) {
	return async (context: CompletionContext): Promise<CompletionResult | null> => {
		const line = context.state.doc.lineAt(context.pos);
		const lineText = line.text.slice(0, context.pos - line.from);
		const trigger = detectWikilinkTrigger(lineText);
		if (!trigger) return null;
		// `from` is line-relative; map to absolute doc offset.
		const fromAbs = line.from + trigger.from;
		// Empty prefix → return an empty result rather than null so the
		// dropdown opens immediately on `[[`, ready to filter as user types.
		const hits = trigger.prefix === '' ? [] : await searcher(trigger.prefix, 8);
		const options: Completion[] = hits.map((h) => ({
			label: h.title,
			detail: 'note',
			// Insert ULID form: `[[<id>]]`. CM6 replaces from `fromAbs` to
			// the current cursor — the user-typed prefix is wiped, the link
			// is closed in one motion.
			apply: `${h.id}]]`
		}));
		return {
			from: fromAbs,
			options,
			validFor: /^[\p{L}\p{N} _.-]*$/u
		};
	};
}

/**
 * Production extension factory. Pass the IPC searcher in.
 */
export function wikilinkAutocomplete(searcher: TitleSearcher): Extension {
	return autocompletion({
		override: [wikilinkCompletionSource(searcher)],
		// Accept on tab too — small ergonomics for keyboard-only flows.
		defaultKeymap: true
	});
}
