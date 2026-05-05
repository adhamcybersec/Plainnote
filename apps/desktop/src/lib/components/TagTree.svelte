<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  TagTree — recursive multi-select tree.
  Click toggles a single tag. Ctrl/Cmd+click adds to the selection.
  Disclosure triangles expand/collapse subtrees. Active rows get
  --accent-soft background.
-->
<script lang="ts">
	import type { TagRow } from '$lib/ipc';
	import { buildTagForest, type TagNode } from '$lib/tag-tree';

	interface Props {
		rows: TagRow[];
		selected: string[];
		onSelectionChange: (next: string[]) => void;
	}

	let { rows, selected, onSelectionChange }: Props = $props();

	let forest = $derived(buildTagForest(rows));

	// Persistent expand/collapse state, keyed by tag path. Default = expanded.
	let collapsed = $state(new Set<string>());

	function toggleCollapse(path: string) {
		if (collapsed.has(path)) collapsed.delete(path);
		else collapsed.add(path);
		collapsed = new Set(collapsed); // trigger reactivity
	}

	function onRowClick(event: MouseEvent, path: string) {
		const isMulti = event.metaKey || event.ctrlKey;
		const inSelection = selected.includes(path);
		let next: string[];
		if (isMulti) {
			next = inSelection ? selected.filter((p) => p !== path) : [...selected, path];
		} else if (inSelection && selected.length === 1) {
			next = [];
		} else {
			next = [path];
		}
		onSelectionChange(next);
	}
</script>

{#snippet treeNode(node: TagNode, depth: number)}
	{@const hasChildren = node.children.length > 0}
	{@const isExpanded = !collapsed.has(node.path)}
	{@const isSelected = selected.includes(node.path)}
	<li class="pn-tree__row" data-testid="tree-row" data-depth={depth}>
		<button
			class="pn-tree__btn"
			class:pn-tree__btn--selected={isSelected}
			type="button"
			onclick={(e) => onRowClick(e, node.path)}
			aria-pressed={isSelected}
			data-path={node.path}
			style="padding-left: {12 + depth * 14}px"
		>
			{#if hasChildren}
				<span
					class="pn-tree__caret"
					role="button"
					tabindex="0"
					aria-label={isExpanded ? 'Collapse' : 'Expand'}
					data-testid="caret"
					onclick={(e) => {
						e.stopPropagation();
						toggleCollapse(node.path);
					}}
					onkeydown={(e) => {
						if (e.key === 'Enter' || e.key === ' ') {
							e.preventDefault();
							toggleCollapse(node.path);
						}
					}}>{isExpanded ? '▾' : '▸'}</span
				>
			{:else}
				<span class="pn-tree__caret pn-tree__caret--leaf"></span>
			{/if}
			<span class="pn-tree__label">{node.leaf_label}</span>
			<span class="pn-tree__count">{node.note_count}</span>
		</button>
		{#if hasChildren && isExpanded}
			<ul class="pn-tree__children">
				{#each node.children as child (child.path)}
					{@render treeNode(child, depth + 1)}
				{/each}
			</ul>
		{/if}
	</li>
{/snippet}

<nav class="pn-tree" aria-label="Tag tree">
	{#if forest.length === 0}
		<p class="pn-tree__empty" data-testid="empty">No tags yet.</p>
	{:else}
		<ul>
			{#each forest as root (root.path)}
				{@render treeNode(root, 0)}
			{/each}
		</ul>
	{/if}
</nav>
