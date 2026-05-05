<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<script lang="ts">
	import CodeMirrorWrapper from '$lib/components/CodeMirrorWrapper.svelte';
	import type { EditorRef } from '$lib/components/CodeMirrorWrapper.svelte';

	let { initial = '', onChange = (_v: string) => {} } = $props<{
		initial?: string;
		onChange?: (v: string) => void;
	}>();

	let inner: EditorRef | undefined = $state();

	export function getRef(): EditorRef {
		if (!inner) throw new Error('editor not mounted');
		return inner;
	}
</script>

<CodeMirrorWrapper bind:this={inner} value={initial} {onChange} />
