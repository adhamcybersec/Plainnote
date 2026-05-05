// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Task-list toggle for markdown editor lines.
 *
 * Pure transform: given a line, return the same line with its checkbox
 * flipped (or null if the line isn't a task). The CM6 extension wires
 * this into a click handler; everything testable lives here.
 *
 * Recognized syntax (CommonMark + GFM):
 *   - [ ] item   → unchecked task
 *   - [x] item   → checked task (case-insensitive: `X` accepted)
 *   * [ ] item   → asterisk variant
 *   + [ ] item   → plus variant
 *
 * NOT recognized (returns null — documented gap, see test):
 *   1. [ ] item  → ordered-list tasks
 *   - [] item    → missing inner space
 */
import {
	type Extension,
	StateField,
	EditorState,
	type Transaction
} from '@codemirror/state';
import { EditorView } from '@codemirror/view';

const TASK_RE = /^(?<indent>\s*)(?<marker>[-*+])\s\[(?<state>[ xX])\]\s/;

export function toggleTaskAtLine(line: string): string | null {
	const m = line.match(TASK_RE);
	if (!m) return null;
	const state = m.groups!.state;
	const checked = state === 'x' || state === 'X';
	const replaced = line.replace(
		/^(\s*[-*+]\s\[)([ xX])(\]\s)/,
		(_full, prefix, _ch, suffix) => `${prefix}${checked ? ' ' : 'x'}${suffix}`
	);
	return replaced;
}

/**
 * CM6 extension: click on a `[ ]` or `[x]` checkbox toggles the task.
 * The hit area is the 3 characters of the checkbox (`[`, the inner state
 * char, `]`) so the user can click anywhere inside without selecting text.
 *
 * We don't bind the markdown extension's task-rendering (which would
 * replace the literal `[ ]` with a real <input> in the DOM) because that
 * would diverge the editor from "what's on disk is what you see". A
 * literal click on the brackets is the discoverable affordance.
 */
export function taskListToggle(): Extension {
	return EditorView.domEventHandlers({
		mousedown(event, view) {
			const target = event.target as HTMLElement | null;
			if (!target) return false;
			const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
			if (pos == null) return false;
			const line = view.state.doc.lineAt(pos);
			const text = line.text;
			const m = text.match(TASK_RE);
			if (!m) return false;
			// Column of the inner state char ([x] → index of x).
			const checkboxCol = m[0].length - 3 + 1; // `[`, state, `]`
			const clickCol = pos - line.from;
			// Hit area: the three checkbox chars.
			if (clickCol < checkboxCol - 1 || clickCol > checkboxCol + 1) {
				return false;
			}
			const replaced = toggleTaskAtLine(text);
			if (replaced == null) return false;
			view.dispatch({
				changes: { from: line.from, to: line.to, insert: replaced }
			});
			event.preventDefault();
			return true;
		}
	});
}

/**
 * Convenience for callers/tests: apply a toggle imperatively at a given
 * line (1-indexed). Returns true if the line was toggled, false otherwise.
 */
export function toggleTaskAtCursor(state: EditorState, lineNumber: number): Transaction | null {
	if (lineNumber < 1 || lineNumber > state.doc.lines) return null;
	const line = state.doc.line(lineNumber);
	const replaced = toggleTaskAtLine(line.text);
	if (replaced == null) return null;
	return state.update({
		changes: { from: line.from, to: line.to, insert: replaced }
	});
}
