<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  Capture screen (R4 zero-friction).
  One textarea, one primary action (Save), two disabled placeholders
  (Record / Attach — voice and attachments arrive in v0.2 / M3).
  Cmd+Enter / Ctrl+Enter saves. Esc does nothing — the user must
  never lose typed input by accident.
-->
<script lang="ts">
	import { saveNote } from '$lib/ipc';
	import type { IpcError } from '$lib/ipc';

	let body = $state('');
	let saving = $state(false);
	let saveStatus = $state<'idle' | 'saved' | 'error'>('idle');
	let errorMessage = $state<string | null>(null);
	let textarea: HTMLTextAreaElement | undefined = $state();
	let savedTimer: ReturnType<typeof setTimeout> | null = null;

	async function save() {
		const trimmed = body.trim();
		if (!trimmed || saving) return;
		saving = true;
		errorMessage = null;
		try {
			await saveNote(trimmed);
			body = '';
			saveStatus = 'saved';
			textarea?.focus();
			if (savedTimer) clearTimeout(savedTimer);
			savedTimer = setTimeout(() => {
				saveStatus = 'idle';
			}, 2000);
		} catch (e) {
			saveStatus = 'error';
			const ipc = e as Partial<IpcError>;
			errorMessage = ipc?.message ?? String(e);
		} finally {
			saving = false;
		}
	}

	function onKeydown(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
			event.preventDefault();
			void save();
		}
	}
</script>

<main class="pn-capture">
	<div class="pn-capture__panel">
		<!-- svelte-ignore a11y_autofocus — Capture is the user's reason to open
		     the app; cursor must land in the textarea instantly per R4. -->
		<textarea
			bind:this={textarea}
			bind:value={body}
			class="pn-textarea"
			placeholder="What do you want to remember?"
			aria-label="Capture a note"
			onkeydown={onKeydown}
			autofocus
		></textarea>

		<div class="pn-capture__actions">
			<button class="pn-btn pn-btn--ghost" type="button" disabled aria-disabled="true">
				<span aria-hidden="true">●</span> Record voice
			</button>
			<button class="pn-btn pn-btn--ghost" type="button" disabled aria-disabled="true">
				<span aria-hidden="true">▣</span> Attach file
			</button>
			<button
				class="pn-btn pn-btn--primary"
				type="button"
				onclick={save}
				disabled={saving || body.trim() === ''}
				data-testid="save-button">Save</button
			>
		</div>

		<div class="pn-capture__hint">
			Press <kbd>Ctrl</kbd>+<kbd>Enter</kbd> to save · Tags can be added later.
		</div>
	</div>

	<footer class="pn-footer" role="status" aria-live="polite">
		{#if saveStatus === 'saved'}
			<span class="pn-footer__chip pn-footer__chip--ok" data-testid="saved-chip">Saved</span>
		{:else if saveStatus === 'error'}
			<span class="pn-footer__chip pn-footer__chip--err" data-testid="error-chip">
				{errorMessage}
			</span>
		{:else}
			<span class="pn-footer__muted">Local-first · v0.1.0-dev</span>
		{/if}
		<span class="pn-footer__sep">·</span>
		<a class="pn-footer__link" href="/library">Library →</a>
	</footer>
</main>
