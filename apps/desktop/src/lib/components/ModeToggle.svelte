<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  ModeToggle — four-button segmented control for the four QueryModes.
  Active mode rendered with --accent-soft + bold; literal-language hint
  appears below so users always know what the current mode means.
-->
<script lang="ts">
	import type { QueryMode } from '$lib/ipc';

	interface Option {
		value: QueryMode;
		label: string;
		hint: string;
	}

	const OPTIONS: Option[] = [
		{
			value: 'strict_intersection',
			label: '∩ Strict',
			hint: 'Notes tagged with all the selected tags exactly.'
		},
		{
			value: 'recursive_intersection',
			label: '∩ Recursive',
			hint: 'Notes tagged with all the selected branches (a tag and any of its children counts).'
		},
		{
			value: 'strict_union',
			label: '∪ Strict',
			hint: 'Notes tagged with any of the selected tags exactly.'
		},
		{
			value: 'recursive_union',
			label: '∪ Recursive',
			hint: 'Notes tagged with any of the selected branches.'
		}
	];

	interface Props {
		mode: QueryMode;
		onModeChange: (next: QueryMode) => void;
	}

	let { mode, onModeChange }: Props = $props();

	let hint = $derived(OPTIONS.find((o) => o.value === mode)?.hint ?? '');
</script>

<div role="group" aria-label="Tag query mode">
	<div class="pn-modes">
		{#each OPTIONS as opt (opt.value)}
			<button
				class="pn-mode"
				type="button"
				aria-pressed={opt.value === mode}
				data-testid="mode-btn"
				data-mode={opt.value}
				onclick={() => onModeChange(opt.value)}
				title={opt.hint}>{opt.label}</button
			>
		{/each}
	</div>
	<p class="pn-modes__hint" data-testid="mode-hint">{hint}</p>
</div>
