// SPDX-License-Identifier: AGPL-3.0-or-later
import { buildTagForest, leafLabel } from '$lib/tag-tree';
import type { TagRow } from '$lib/ipc';

describe('leafLabel', () => {
	it('returns the last segment of a slash-separated path', () => {
		expect(leafLabel('learning')).toBe('learning');
		expect(leafLabel('learning/math')).toBe('math');
		expect(leafLabel('learning/math/calculus')).toBe('calculus');
	});
});

describe('buildTagForest', () => {
	it('returns empty array for empty input', () => {
		expect(buildTagForest([])).toEqual([]);
	});

	it('builds a forest of single roots', () => {
		const rows: TagRow[] = [
			{ path: 'learning', parent: null, note_count: 2 },
			{ path: 'work', parent: null, note_count: 1 }
		];
		const forest = buildTagForest(rows);
		expect(forest.length).toBe(2);
		expect(forest.map((n) => n.path)).toEqual(['learning', 'work']);
		expect(forest[0].children).toEqual([]);
	});

	it('nests children under their parent', () => {
		const rows: TagRow[] = [
			{ path: 'learning', parent: null, note_count: 0 },
			{ path: 'learning/math', parent: 'learning', note_count: 0 },
			{ path: 'learning/math/calculus', parent: 'learning/math', note_count: 1 }
		];
		const forest = buildTagForest(rows);
		expect(forest.length).toBe(1);
		expect(forest[0].path).toBe('learning');
		expect(forest[0].children.length).toBe(1);
		expect(forest[0].children[0].path).toBe('learning/math');
		expect(forest[0].children[0].children[0].path).toBe('learning/math/calculus');
	});

	it('sorts siblings alphabetically by path', () => {
		const rows: TagRow[] = [
			{ path: 'learning', parent: null, note_count: 0 },
			{ path: 'learning/physics', parent: 'learning', note_count: 0 },
			{ path: 'learning/math', parent: 'learning', note_count: 0 }
		];
		const forest = buildTagForest(rows);
		expect(forest[0].children.map((c) => c.leaf_label)).toEqual(['math', 'physics']);
	});

	it('promotes orphaned children to root', () => {
		// If a row references a parent that is not present (corrupt data),
		// don't drop the row — promote it.
		const rows: TagRow[] = [
			{ path: 'orphan', parent: 'missing/parent', note_count: 0 }
		];
		const forest = buildTagForest(rows);
		expect(forest.length).toBe(1);
		expect(forest[0].path).toBe('orphan');
	});

	it('exposes leaf_label and note_count on every node', () => {
		const rows: TagRow[] = [
			{ path: 'learning', parent: null, note_count: 5 },
			{ path: 'learning/math', parent: 'learning', note_count: 2 }
		];
		const forest = buildTagForest(rows);
		expect(forest[0].leaf_label).toBe('learning');
		expect(forest[0].note_count).toBe(5);
		expect(forest[0].children[0].leaf_label).toBe('math');
		expect(forest[0].children[0].note_count).toBe(2);
	});
});
