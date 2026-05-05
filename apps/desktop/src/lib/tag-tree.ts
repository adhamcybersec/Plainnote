// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure helper for building a tag tree out of the flat `TagRow[]` returned
//! by `listTags`. Lives outside the Svelte component so it's unit-testable
//! without mounting the DOM.

import type { TagRow } from '$lib/ipc';

export interface TagNode {
	path: string;
	parent: string | null;
	note_count: number;
	leaf_label: string;
	children: TagNode[];
}

/** Last segment of a slash-separated path, e.g. 'learning/math' → 'math'. */
export function leafLabel(path: string): string {
	const i = path.lastIndexOf('/');
	return i === -1 ? path : path.slice(i + 1);
}

/**
 * Build a forest of `TagNode` from a flat list. Children are sorted
 * alphabetically by path. Roots are returned in alphabetical order.
 *
 * Robust against malformed input: if a row references a parent path that
 * isn't itself in the input, it's promoted to a root.
 */
export function buildTagForest(rows: TagRow[]): TagNode[] {
	const byPath = new Map<string, TagNode>();
	for (const r of rows) {
		byPath.set(r.path, {
			path: r.path,
			parent: r.parent,
			note_count: r.note_count,
			leaf_label: leafLabel(r.path),
			children: []
		});
	}
	const roots: TagNode[] = [];
	for (const node of byPath.values()) {
		if (node.parent && byPath.has(node.parent)) {
			byPath.get(node.parent)!.children.push(node);
		} else {
			roots.push(node);
		}
	}
	const sortByPath = (a: TagNode, b: TagNode) => a.path.localeCompare(b.path);
	roots.sort(sortByPath);
	for (const node of byPath.values()) {
		node.children.sort(sortByPath);
	}
	return roots;
}
