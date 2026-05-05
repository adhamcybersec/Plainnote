<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  Focus / Note view.
    - Loads the note by ULID from the URL.
    - Editor uses CodeMirrorWrapper. Wikilink autocomplete is wired in
      via the IPC searcher prop; the wrapper itself is unaware of Tauri.
    - Render-mode toggle ("source" | "rendered") persists via meta table
      so the user's preference survives restarts.
    - Backlinks panel sits below; it reacts to the loaded id (not to the
      doc body) so editing doesn't churn the panel.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/state';
	import {
		readNote,
		searchNotesByTitle,
		backlinksFor,
		getMeta,
		setMeta,
		type Note,
		type Backlink,
		type TitleHit
	} from '$lib/ipc';
	import CodeMirrorWrapper from '$lib/components/CodeMirrorWrapper.svelte';
	import { renderMarkdown } from '$lib/render-markdown';

	type RenderMode = 'source' | 'rendered';
	const RENDER_MODE_KEY = 'editor.render_mode';
	const DEFAULT_RENDER_MODE: RenderMode = 'rendered';

	let noteId = $derived(page.params.id ?? '');
	let note = $state<Note | null>(null);
	let body = $state('');
	let backlinks = $state<Backlink[]>([]);
	let renderMode = $state<RenderMode>(DEFAULT_RENDER_MODE);
	let loading = $state(true);
	let errorMessage = $state<string | null>(null);

	async function titleSearcher(prefix: string, limit: number): Promise<TitleHit[]> {
		// Errors here would dump the dropdown — swallow to keep the editor
		// responsive; the user can always type the title manually.
		try {
			return await searchNotesByTitle(prefix, limit);
		} catch {
			return [];
		}
	}

	async function load() {
		loading = true;
		errorMessage = null;
		try {
			note = await readNote(noteId);
			body = note.body;
			backlinks = await backlinksFor(noteId);
			const stored = await getMeta(RENDER_MODE_KEY);
			renderMode =
				stored === 'source' || stored === 'rendered'
					? (stored as RenderMode)
					: DEFAULT_RENDER_MODE;
		} catch (e) {
			const ipc = e as { code?: string; message?: string };
			errorMessage = ipc?.message ?? String(e);
		} finally {
			loading = false;
		}
	}

	onMount(load);

	async function toggleRenderMode() {
		renderMode = renderMode === 'source' ? 'rendered' : 'source';
		try {
			await setMeta(RENDER_MODE_KEY, renderMode);
		} catch {
			// Persistence failure is non-fatal — preference reverts on next load.
		}
	}

	function displayTitle(n: Note | null): string {
		if (!n) return '';
		if (n.title && n.title.trim() !== '') return n.title;
		return '(no title)';
	}

	let renderedHtml = $derived(renderMode === 'rendered' ? renderMarkdown(body) : '');
</script>

<svelte:head>
	<title>{displayTitle(note) || 'Note'} — Plainnote</title>
</svelte:head>

<main class="pn-focus">
	<header class="pn-focus__head">
		<a href="/library" class="pn-focus__back" data-testid="back">← Library</a>
		<h1 class="pn-focus__title" data-testid="title">{displayTitle(note)}</h1>
		<button
			type="button"
			class="pn-btn pn-btn--ghost"
			data-testid="toggle-render-mode"
			aria-pressed={renderMode === 'rendered'}
			onclick={toggleRenderMode}
		>
			{renderMode === 'rendered' ? 'Source' : 'Rendered'}
		</button>
	</header>

	{#if loading}
		<p class="pn-empty" data-testid="loading">Loading…</p>
	{:else if errorMessage}
		<p class="pn-empty pn-empty--error" data-testid="error">{errorMessage}</p>
	{:else if note}
		<section
			class="pn-focus__body"
			data-testid="body"
			data-render-mode={renderMode}
		>
			{#if renderMode === 'source'}
				<CodeMirrorWrapper
					value={body}
					onChange={(v: string) => (body = v)}
					{titleSearcher}
				/>
			{:else}
				<!-- DOMPurify-cleaned in render-markdown.ts; safe for {@html}. -->
				<article class="pn-prose" data-testid="rendered">{@html renderedHtml}</article>
			{/if}
		</section>

		<section class="pn-backlinks" data-testid="backlinks">
			<h2 class="pn-backlinks__title">Notes that link here</h2>
			{#if backlinks.length === 0}
				<p class="pn-empty" data-testid="backlinks-empty">No notes link here yet.</p>
			{:else}
				<ul class="pn-backlinks__list">
					{#each backlinks as bl (bl.source_id + bl.raw)}
						<li>
							<a
								class="pn-backlink"
								href="/note/{bl.source_id}"
								data-testid="backlink"
							>
								<span class="pn-backlink__title">
									{bl.source_title ?? '(no title)'}
								</span>
								<span class="pn-backlink__preview">{bl.source_preview}</span>
							</a>
						</li>
					{/each}
				</ul>
			{/if}
		</section>
	{/if}
</main>
