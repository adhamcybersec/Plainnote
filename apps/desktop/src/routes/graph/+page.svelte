<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	Graph view — visualizes the wikilink graph using Sigma.js. Loads
	the graph payload from the Rust backend (which computes the
	force-directed layout and node sizes), passes it straight to the
	SigmaWrapper component. Click a node to navigate to that note.

	v0.1 scope per plan §6 M5: static layout, no search, no zoom-to-
	neighbor, no color-by-tag. Those land in v0.2.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { graphData, type GraphData, type NoteId } from '$lib/ipc';
	import SigmaWrapper from '$lib/components/SigmaWrapper.svelte';

	let data = $state<GraphData | null>(null);
	let loading = $state(true);
	let errorMessage = $state<string | null>(null);

	onMount(async () => {
		try {
			data = await graphData();
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		} finally {
			loading = false;
		}
	});

	function onNodeClick(id: NoteId) {
		void goto(`/note/${id}`);
	}
</script>

<svelte:head>
	<title>Graph — Plainnote</title>
</svelte:head>

<main class="pn-graph">
	<header class="pn-graph__head">
		<a href="/library" class="pn-focus__back" data-testid="back">← Library</a>
		<h1 class="pn-graph__title" data-testid="title">Graph</h1>
		{#if data}
			<span class="pn-graph__count" data-testid="count">
				{data.nodes.length} {data.nodes.length === 1 ? 'note' : 'notes'}
			</span>
		{/if}
	</header>

	{#if data?.truncated}
		<p class="pn-graph__notice" data-testid="truncated-notice">
			Graph truncated to the {data.nodes.length} most-connected notes. Larger
			graphs ship in v0.2.
		</p>
	{/if}

	{#if loading}
		<p class="pn-empty" data-testid="loading">Loading graph…</p>
	{:else if errorMessage}
		<p class="pn-empty pn-empty--error" data-testid="error">{errorMessage}</p>
	{:else if data && data.nodes.length === 0}
		<p class="pn-empty" data-testid="empty">
			No notes yet. Open Capture and write something to start the graph.
		</p>
	{:else if data}
		<SigmaWrapper {data} {onNodeClick} />
	{/if}
</main>
