<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  FilterBar — sits above the timeline, shows the active query state.
  One chip per selected tag (removable via ✕) plus one read-only chip
  describing the active query mode in literal language. Hidden when
  nothing is selected.
-->
<script lang="ts">
	import type { QueryMode } from '$lib/ipc';

	const MODE_LABEL: Record<QueryMode, string> = {
		strict_intersection: 'Strict ∩',
		recursive_intersection: 'Recursive ∩',
		strict_union: 'Strict ∪',
		recursive_union: 'Recursive ∪'
	};

	interface Props {
		selected: string[];
		mode: QueryMode;
		onSelectionChange: (next: string[]) => void;
	}

	let { selected, mode, onSelectionChange }: Props = $props();

	function removeTag(path: string) {
		onSelectionChange(selected.filter((p) => p !== path));
	}
</script>

{#if selected.length > 0}
	<div class="pn-filterbar" data-testid="filterbar" role="toolbar" aria-label="Active filters">
		{#each selected as path (path)}
			<span class="pn-chip" data-testid="filter-chip" data-path={path}>
				{path}
				<button
					class="pn-chip__close"
					type="button"
					aria-label="Remove tag {path}"
					data-testid="filter-chip-close"
					onclick={() => removeTag(path)}>×</button
				>
			</span>
		{/each}
		<span class="pn-chip pn-chip--mode" data-testid="filter-mode">Mode: {MODE_LABEL[mode]}</span>
	</div>
{/if}
