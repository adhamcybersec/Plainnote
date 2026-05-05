<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	Settings — minimal v0.1 surface. Sections that ship now:
	  - Reminders: list of active + fired reminders, cancel button
	    on active rows. Lead-time default + sound-off preference
	    persisted via meta table.

	The full Settings surface (vault location, reindex, density,
	accent, reduce-motion) lands in M9 polish.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listReminders,
		cancelReminder,
		getMeta,
		setMeta,
		type Reminder
	} from '$lib/ipc';
	import {
		leadTimePresets,
		type LeadTimePreset
	} from '$lib/reminder-format';

	const REMINDER_DEFAULT_KEY = 'reminders.default_lead_time';
	const REMINDER_SOUND_KEY = 'reminders.sound_enabled';

	let active = $state<Reminder[]>([]);
	let fired = $state<Reminder[]>([]);
	let defaultLead = $state<LeadTimePreset>('in_1h');
	let soundEnabled = $state(false);
	let loading = $state(true);
	let errorMessage = $state<string | null>(null);

	async function refreshReminders() {
		try {
			[active, fired] = await Promise.all([
				listReminders('active'),
				listReminders('fired')
			]);
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		}
	}

	async function loadPrefs() {
		const [defaultRaw, soundRaw] = await Promise.all([
			getMeta(REMINDER_DEFAULT_KEY),
			getMeta(REMINDER_SOUND_KEY)
		]);
		const valid: LeadTimePreset[] = ['now', 'in_5m', 'in_1h', 'tomorrow_9am'];
		if (defaultRaw && (valid as string[]).includes(defaultRaw)) {
			defaultLead = defaultRaw as LeadTimePreset;
		}
		// Sound is opt-in. Persisted as 'true' / 'false'.
		soundEnabled = soundRaw === 'true';
	}

	onMount(async () => {
		try {
			await Promise.all([refreshReminders(), loadPrefs()]);
		} finally {
			loading = false;
		}
	});

	async function setDefaultLead(preset: LeadTimePreset) {
		defaultLead = preset;
		try {
			await setMeta(REMINDER_DEFAULT_KEY, preset);
		} catch {
			// Non-fatal — reverts on next reload.
		}
	}

	async function toggleSound() {
		soundEnabled = !soundEnabled;
		try {
			await setMeta(REMINDER_SOUND_KEY, soundEnabled ? 'true' : 'false');
		} catch {
			// Non-fatal
		}
	}

	async function cancel(id: string) {
		try {
			await cancelReminder(id);
			await refreshReminders();
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		}
	}

	function formatTime(iso: string): string {
		const d = new Date(iso);
		if (isNaN(d.getTime())) return iso;
		return d.toLocaleString(undefined, {
			weekday: 'short',
			month: 'short',
			day: 'numeric',
			hour: 'numeric',
			minute: '2-digit'
		});
	}
</script>

<svelte:head>
	<title>Settings — Plainnote</title>
</svelte:head>

<main class="pn-settings" data-testid="settings-page">
	<header class="pn-settings__head">
		<a href="/library" class="pn-focus__back" data-testid="back">← Library</a>
		<h1 class="pn-settings__title">Settings</h1>
	</header>

	{#if errorMessage}
		<p class="pn-empty pn-empty--error" data-testid="error">{errorMessage}</p>
	{/if}

	<section class="pn-settings__section" data-testid="reminders-section">
		<h2>Reminders</h2>

		<div class="pn-settings__row">
			<label for="default-lead">Default lead time</label>
			<select
				id="default-lead"
				bind:value={defaultLead}
				onchange={() => setDefaultLead(defaultLead)}
				data-testid="default-lead-select"
			>
				{#each leadTimePresets as p (p.id)}
					<option value={p.id}>{p.label}</option>
				{/each}
			</select>
		</div>

		<div class="pn-settings__row">
			<label class="pn-settings__check">
				<input
					type="checkbox"
					checked={soundEnabled}
					onchange={toggleSound}
					data-testid="sound-toggle"
				/>
				Play a sound when a reminder fires
			</label>
			<p class="pn-settings__hint">
				Off by default. The notification still appears either way.
			</p>
		</div>

		<h3 class="pn-settings__sub">Active</h3>
		{#if loading}
			<p class="pn-empty" data-testid="loading">Loading…</p>
		{:else if active.length === 0}
			<p class="pn-empty" data-testid="active-empty">
				No reminders set. Open a note and use “Set reminder”.
			</p>
		{:else}
			<ul class="pn-settings__list" data-testid="active-list">
				{#each active as r (r.id)}
					<li class="pn-settings__item" data-testid="active-item">
						<div>
							<div class="pn-settings__item-time">{formatTime(r.fire_at)}</div>
							<div class="pn-settings__item-body">{r.body}</div>
						</div>
						<button
							type="button"
							class="pn-btn pn-btn--ghost"
							data-testid="cancel-reminder"
							onclick={() => cancel(r.id)}
						>
							Cancel
						</button>
					</li>
				{/each}
			</ul>
		{/if}

		<h3 class="pn-settings__sub">Recently fired</h3>
		{#if fired.length === 0}
			<p class="pn-empty" data-testid="fired-empty">No fired reminders yet.</p>
		{:else}
			<ul class="pn-settings__list" data-testid="fired-list">
				{#each fired.slice(0, 10) as r (r.id)}
					<li class="pn-settings__item" data-testid="fired-item">
						<div>
							<div class="pn-settings__item-time">
								{formatTime(r.fired_at ?? r.fire_at)}
							</div>
							<div class="pn-settings__item-body">{r.body}</div>
						</div>
					</li>
				{/each}
			</ul>
		{/if}
	</section>
</main>
