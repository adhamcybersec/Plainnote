<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	Set-reminder dialog. Lives inside the note view; triggered by a
	"Set reminder" button. The dialog is a non-modal floating panel —
	clicking outside cancels.

	Form contract:
	  - Lead-time presets (radio): now / 5 minutes / 1 hour / tomorrow 9am
	  - Custom datetime (optional): overrides the preset
	  - Body text (optional): defaults to the note's title
	  - Plain-language summary line that updates live
	  - Save / Cancel buttons

	Calls setReminder(fireAt, body, noteId) from the IPC layer on save.
-->
<script lang="ts">
	import {
		leadTimePresets,
		addLeadTime,
		plainSummary,
		type LeadTimePreset
	} from '$lib/reminder-format';
	import { setReminder, type NoteId } from '$lib/ipc';

	let {
		noteId,
		defaultBody = '',
		onSaved = (_id: string) => {},
		onCancel = () => {}
	} = $props<{
		noteId: NoteId;
		defaultBody?: string;
		onSaved?: (id: string) => void;
		onCancel?: () => void;
	}>();

	let preset = $state<LeadTimePreset>('in_1h');
	let customISO = $state(''); // empty = use preset
	// defaultBody is read inside the initializer function so Svelte's
	// state-from-prop heuristic (state_referenced_locally) sees a closure
	// rather than a direct reference.
	let body = $state((() => defaultBody)());
	let saving = $state(false);
	let errorMessage = $state<string | null>(null);

	let now = $state(new Date());
	// Refresh `now` once per second so the summary doesn't go stale while
	// the dialog is open. Cleared on dialog destroy via $effect cleanup.
	$effect(() => {
		const handle = setInterval(() => {
			now = new Date();
		}, 1000);
		return () => clearInterval(handle);
	});

	let fireAt = $derived.by(() => {
		if (customISO) {
			const d = new Date(customISO);
			return isNaN(d.getTime()) ? addLeadTime(preset, now) : d;
		}
		return addLeadTime(preset, now);
	});

	let summary = $derived(plainSummary(fireAt, now));

	async function save() {
		saving = true;
		errorMessage = null;
		try {
			const id = await setReminder(
				fireAt.toISOString(),
				body.trim() || defaultBody || 'Reminder',
				noteId
			);
			onSaved(id);
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
			saving = false;
		}
	}
</script>

<section class="pn-reminder-dialog" data-testid="reminder-dialog">
	<header class="pn-reminder-dialog__head">
		<h2 class="pn-reminder-dialog__title">Set a reminder</h2>
		<button type="button" class="pn-reminder-dialog__x" onclick={onCancel}>
			Cancel
		</button>
	</header>

	<fieldset class="pn-reminder-dialog__presets" data-testid="presets">
		<legend>When</legend>
		{#each leadTimePresets as p (p.id)}
			<label class="pn-reminder-dialog__preset">
				<input
					type="radio"
					name="lead-time"
					value={p.id}
					checked={preset === p.id}
					onchange={() => {
						preset = p.id;
						customISO = '';
					}}
					data-testid="preset-{p.id}"
				/>
				<span>{p.label}</span>
			</label>
		{/each}
		<label class="pn-reminder-dialog__custom">
			<span>Or pick a date and time:</span>
			<input
				type="datetime-local"
				bind:value={customISO}
				data-testid="custom-datetime"
			/>
		</label>
	</fieldset>

	<label class="pn-reminder-dialog__body">
		<span>Body</span>
		<input
			type="text"
			bind:value={body}
			placeholder="Will fire with this text"
			data-testid="body-input"
		/>
	</label>

	<p class="pn-reminder-dialog__summary" data-testid="summary">
		Will fire {summary}.
	</p>

	{#if errorMessage}
		<p class="pn-empty pn-empty--error" data-testid="error">
			{errorMessage}
		</p>
	{/if}

	<footer class="pn-reminder-dialog__foot">
		<button
			type="button"
			class="pn-btn pn-btn--ghost"
			onclick={onCancel}
			disabled={saving}
		>
			Cancel
		</button>
		<button
			type="button"
			class="pn-btn"
			onclick={save}
			disabled={saving}
			data-testid="save"
		>
			{saving ? 'Saving…' : 'Save reminder'}
		</button>
	</footer>
</section>
