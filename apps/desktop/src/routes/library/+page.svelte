<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  Library list view (M1a slice).
  Reverse-chronological list of saved notes. Click → focus route (M3).
  Real timeline grouping headers and the four-mode tag filter ship in M2.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { listNotes, type NoteSummary } from '$lib/ipc';

	let summaries = $state<NoteSummary[]>([]);
	let loading = $state(true);
	let errorMessage = $state<string | null>(null);

	function relativeTime(iso: string): string {
		const then = new Date(iso).getTime();
		const now = Date.now();
		const seconds = Math.max(0, Math.floor((now - then) / 1000));
		if (seconds < 60) return 'just now';
		if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
		const days = Math.floor(seconds / 86400);
		if (days < 7) return `${days}d`;
		return iso.slice(0, 10);
	}

	function displayTitle(s: NoteSummary): string {
		if (s.title && s.title.trim() !== '') return s.title;
		return s.preview || '(no title)';
	}

	onMount(async () => {
		try {
			summaries = await listNotes(50);
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		} finally {
			loading = false;
		}
	});
</script>

<main class="pn-library">
	<header class="pn-library__head">
		<div>
			<h1 class="pn-library__title">Library</h1>
			<span class="pn-library__count" data-testid="count">
				{#if loading}Loading…{:else}{summaries.length} {summaries.length === 1 ? 'note' : 'notes'}{/if}
			</span>
		</div>
		<a class="pn-library__back" href="/">← Capture</a>
	</header>

	{#if errorMessage}
		<p class="pn-empty" data-testid="error">Error: {errorMessage}</p>
	{:else if !loading && summaries.length === 0}
		<p class="pn-empty" data-testid="empty">No notes yet. Open Capture and write something.</p>
	{:else}
		<ul class="pn-library__list" data-testid="list">
			{#each summaries as note (note.id)}
				<li>
					<a class="pn-card" href="/note/{note.id}" data-testid="card">
						<div class="pn-card__head">
							<h2 class="pn-card__title">{displayTitle(note)}</h2>
							<span class="pn-card__time">{relativeTime(note.created)}</span>
						</div>
						{#if note.preview && note.title && note.preview !== note.title}
							<p class="pn-card__preview">{note.preview}</p>
						{/if}
					</a>
				</li>
			{/each}
		</ul>
	{/if}
</main>
