// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Pure task-list toggle. Given a single line of text, return the line
 * with its checkbox flipped — or null if the line is not a task.
 *
 * The CM6 extension wires this to a click handler so we don't need to
 * test the editor integration here.
 */
import { toggleTaskAtLine } from '$lib/components/task-list';

describe('toggleTaskAtLine', () => {
	it('returns null for a line with no checkbox', () => {
		expect(toggleTaskAtLine('just text')).toBeNull();
	});

	it('toggles unchecked → checked', () => {
		expect(toggleTaskAtLine('- [ ] todo')).toBe('- [x] todo');
	});

	it('toggles checked → unchecked', () => {
		expect(toggleTaskAtLine('- [x] done')).toBe('- [ ] done');
	});

	it('preserves the leading list marker variant', () => {
		// CommonMark allows `-`, `*`, `+`. All of them survive a toggle.
		expect(toggleTaskAtLine('* [ ] a')).toBe('* [x] a');
		expect(toggleTaskAtLine('+ [ ] a')).toBe('+ [x] a');
		expect(toggleTaskAtLine('- [x] a')).toBe('- [ ] a');
	});

	it('preserves indentation', () => {
		expect(toggleTaskAtLine('    - [ ] nested')).toBe('    - [x] nested');
		expect(toggleTaskAtLine('\t- [x] deeper')).toBe('\t- [ ] deeper');
	});

	it('handles uppercase X (CommonMark case-insensitive)', () => {
		// Some renderers emit `[X]`; we treat it as checked and toggle to `[ ]`.
		expect(toggleTaskAtLine('- [X] caps')).toBe('- [ ] caps');
	});

	it('preserves the rest of the line including punctuation', () => {
		expect(toggleTaskAtLine('- [ ] write the test, run it, commit'))
			.toBe('- [x] write the test, run it, commit');
	});

	it('returns null for an ordered-list task (rare, but not standard)', () => {
		// `1. [ ] x` is technically a task in some flavors; we don't toggle
		// it because CodeMirror's markdown extension renders it as a number
		// and the click target's column math gets murky. Document the gap.
		expect(toggleTaskAtLine('1. [ ] ordered')).toBeNull();
	});

	it('returns null for a malformed marker', () => {
		expect(toggleTaskAtLine('- [] missing space')).toBeNull();
		expect(toggleTaskAtLine('-[ ] missing space after marker')).toBeNull();
		expect(toggleTaskAtLine('- [ ]nospace')).toBeNull();
	});
});
