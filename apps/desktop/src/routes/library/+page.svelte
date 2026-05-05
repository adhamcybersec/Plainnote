<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  Library — three-pane equivalent for M2.
    Left:   TagTree (multi-select, ctrl+click to extend)
    Top:    ModeToggle (four-mode segmented control + literal hint)
    Top-2:  FilterBar (active filter chips, removable)
    Center: Timeline of matching notes (newest first)
  When no tags are selected, the timeline shows all notes (chrono).
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listNotes,
		listTags,
		queryNotes,
		DEFAULT_QUERY_MODE,
		type NoteSummary,
		type TagRow,
		type QueryMode
	} from '$lib/ipc';
	import TagTree from '$lib/components/TagTree.svelte';
	import ModeToggle from '$lib/components/ModeToggle.svelte';
	import FilterBar from '$lib/components/FilterBar.svelte';
	import { groupByRecency } from '$lib/timeline';

	let summaries = $state<NoteSummary[]>([]);
	let tagRows = $state<TagRow[]>([]);
	let selectedTags = $state<string[]>([]);
	let mode = $state<QueryMode>(DEFAULT_QUERY_MODE);
	let loading = $state(true);
	let errorMessage = $state<string | null>(null);
	let timelineGroups = $derived(groupByRecency(summaries));

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

	async function refreshFeed() {
		loading = true;
		errorMessage = null;
		try {
			summaries =
				selectedTags.length === 0
					? await listNotes(50)
					: await queryNotes(selectedTags, mode);
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		} finally {
			loading = false;
		}
	}

	async function refreshTags() {
		try {
			tagRows = await listTags();
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		}
	}

	function onSelectionChange(next: string[]) {
		selectedTags = next;
		void refreshFeed();
	}

	function onModeChange(next: QueryMode) {
		mode = next;
		if (selectedTags.length > 0) void refreshFeed();
	}

	onMount(async () => {
		await Promise.all([refreshFeed(), refreshTags()]);
	});
</script>

<main class="pn-library2">
	<aside class="pn-library2__tags">
		<h2 class="pn-library2__h">Tags</h2>
		<TagTree rows={tagRows} selected={selectedTags} {onSelectionChange} />
	</aside>

	<section class="pn-library2__feed">
		<header class="pn-library__head">
			<div>
				<h1 class="pn-library__title">Library</h1>
				<span class="pn-library__count" data-testid="count">
					{#if loading}Loading…{:else}{summaries.length}
						{summaries.length === 1 ? 'note' : 'notes'}{/if}
				</span>
			</div>
			<a class="pn-library__back" href="/">← Capture</a>
			<a class="pn-library__back" href="/graph" data-testid="graph-link">Graph →</a>
		</header>

		<div class="pn-library2__modebar">
			<ModeToggle {mode} {onModeChange} />
		</div>

		<FilterBar selected={selectedTags} {mode} {onSelectionChange} />

		{#if errorMessage}
			<p class="pn-empty" data-testid="error">Error: {errorMessage}</p>
		{:else if !loading && summaries.length === 0}
			<p class="pn-empty" data-testid="empty">
				{#if selectedTags.length > 0}
					No notes match this filter.
				{:else}
					No notes yet. Open Capture and write something.
				{/if}
			</p>
		{:else}
			<div class="pn-library__list" data-testid="list">
				{#each timelineGroups as group (group.label)}
					<section class="pn-timeline-group" data-testid="timeline-group">
						<h2 class="pn-timeline-group__heading" data-testid="timeline-heading">
							{group.label}
						</h2>
						<ul class="pn-timeline-group__items">
							{#each group.items as note (note.id)}
								<li>
									<a class="pn-card" href="/note/{note.id}" data-testid="card">
										<div class="pn-card__head">
											<h3 class="pn-card__title">{displayTitle(note)}</h3>
											<span class="pn-card__time">{relativeTime(note.created)}</span>
										</div>
										{#if note.preview && note.title && note.preview !== note.title}
											<p class="pn-card__preview">{note.preview}</p>
										{/if}
									</a>
								</li>
							{/each}
						</ul>
					</section>
				{/each}
			</div>
		{/if}
	</section>
</main>
