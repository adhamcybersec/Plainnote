<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	Settings — full v0.1 surface (M9-T1 + M6-T4).

	Sections, in order:
	  1. Appearance — theme / density / accent / reduce-motion. Applied
	     immediately to <html> via the appearance controller.
	  2. Reminders — default lead time + sound-off preference + active /
	     fired lists with cancel button per active row.
	  3. Vault — absolute vault path + Reindex action + note count.
	  4. About — version + license link + source link.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import {
		listReminders,
		cancelReminder,
		vaultInfo,
		reindexVault,
		type Reminder,
		type VaultInfo
	} from '$lib/ipc';
	import {
		leadTimePresets,
		type LeadTimePreset
	} from '$lib/reminder-format';
	import {
		applyAppearance,
		loadAndApplyAppearance,
		setAppearancePref,
		THEMES,
		DENSITIES,
		ACCENTS,
		type Appearance
	} from '$lib/appearance';
	import { announce } from '$lib/status';
	import {
		getMeta as ipcGetMeta,
		setMeta as ipcSetMeta
	} from '$lib/ipc';

	const REMINDER_DEFAULT_KEY = 'reminders.default_lead_time';
	const REMINDER_SOUND_KEY = 'reminders.sound_enabled';

	let appearance = $state<Appearance>({
		theme: 'auto',
		density: 'comfortable',
		accent: 'sage',
		reduceMotion: false
	});

	let active = $state<Reminder[]>([]);
	let fired = $state<Reminder[]>([]);
	let defaultLead = $state<LeadTimePreset>('in_1h');
	let soundEnabled = $state(false);
	let vault = $state<VaultInfo | null>(null);
	let reindexing = $state(false);
	let lastReindexSummary = $state<string | null>(null);
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

	async function refreshVault() {
		try {
			vault = await vaultInfo();
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		}
	}

	async function loadPrefs() {
		const [defaultRaw, soundRaw] = await Promise.all([
			ipcGetMeta(REMINDER_DEFAULT_KEY),
			ipcGetMeta(REMINDER_SOUND_KEY)
		]);
		const valid: LeadTimePreset[] = ['now', 'in_5m', 'in_1h', 'tomorrow_9am'];
		if (defaultRaw && (valid as string[]).includes(defaultRaw)) {
			defaultLead = defaultRaw as LeadTimePreset;
		}
		soundEnabled = soundRaw === 'true';
		appearance = await loadAndApplyAppearance();
	}

	onMount(async () => {
		try {
			await Promise.all([refreshReminders(), refreshVault(), loadPrefs()]);
		} finally {
			loading = false;
		}
	});

	async function persistAppearance<K extends keyof Appearance>(key: K, value: Appearance[K]) {
		appearance = { ...appearance, [key]: value };
		applyAppearance(appearance);
		try {
			await setAppearancePref(key, value);
			announce('Appearance saved.');
		} catch {
			// Non-fatal; reverts on next load.
		}
	}

	async function setDefaultLead(preset: LeadTimePreset) {
		defaultLead = preset;
		try {
			await ipcSetMeta(REMINDER_DEFAULT_KEY, preset);
			announce('Default lead time saved.');
		} catch {
			// Non-fatal
		}
	}

	async function toggleSound() {
		soundEnabled = !soundEnabled;
		try {
			await ipcSetMeta(REMINDER_SOUND_KEY, soundEnabled ? 'true' : 'false');
			announce(soundEnabled ? 'Sound enabled.' : 'Sound disabled.');
		} catch {
			// Non-fatal
		}
	}

	async function cancel(id: string) {
		try {
			await cancelReminder(id);
			announce('Reminder cancelled.');
			await refreshReminders();
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		}
	}

	async function reindex() {
		reindexing = true;
		lastReindexSummary = null;
		try {
			const summary = await reindexVault();
			lastReindexSummary = `Reindex complete: ${summary.inserted} added, ${summary.updated} updated, ${summary.deleted} removed.`;
			announce(lastReindexSummary);
			await refreshVault();
		} catch (e) {
			const ipc = e as { message?: string };
			errorMessage = ipc?.message ?? String(e);
		} finally {
			reindexing = false;
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

	<!-- ─── Appearance ──────────────────────────────────────────────────── -->
	<section class="pn-settings__section" data-testid="appearance-section">
		<h2>Appearance</h2>

		<div class="pn-settings__row">
			<label for="theme-select">Theme</label>
			<select
				id="theme-select"
				value={appearance.theme}
				onchange={(e) =>
					persistAppearance('theme', (e.currentTarget as HTMLSelectElement).value as Appearance['theme'])}
				data-testid="theme-select"
			>
				{#each THEMES as t (t.id)}
					<option value={t.id}>{t.label}</option>
				{/each}
			</select>
		</div>

		<div class="pn-settings__row">
			<label for="density-select">Density</label>
			<select
				id="density-select"
				value={appearance.density}
				onchange={(e) =>
					persistAppearance('density', (e.currentTarget as HTMLSelectElement).value as Appearance['density'])}
				data-testid="density-select"
			>
				{#each DENSITIES as d (d.id)}
					<option value={d.id}>{d.label}</option>
				{/each}
			</select>
		</div>

		<div class="pn-settings__row">
			<span>Accent</span>
			<div class="pn-accent-grid" data-testid="accent-grid" role="radiogroup" aria-label="Accent color">
				{#each ACCENTS as a (a.id)}
					<button
						type="button"
						class="pn-accent-swatch"
						class:pn-accent-swatch--active={appearance.accent === a.id}
						style="--swatch: {a.swatch}"
						aria-label={a.label}
						aria-checked={appearance.accent === a.id}
						role="radio"
						data-testid="accent-{a.id}"
						onclick={() => persistAppearance('accent', a.id)}
					></button>
				{/each}
			</div>
		</div>

		<div class="pn-settings__row">
			<label class="pn-settings__check">
				<input
					type="checkbox"
					checked={appearance.reduceMotion}
					onchange={() => persistAppearance('reduceMotion', !appearance.reduceMotion)}
					data-testid="reduce-motion-toggle"
				/>
				Reduce motion
			</label>
			<p class="pn-settings__hint">
				Disables transitions and animations. Useful for vestibular comfort.
			</p>
		</div>
	</section>

	<!-- ─── Reminders ───────────────────────────────────────────────────── -->
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

	<!-- ─── Vault ───────────────────────────────────────────────────────── -->
	<section class="pn-settings__section" data-testid="vault-section">
		<h2>Vault</h2>
		{#if vault}
			<div class="pn-settings__row">
				<span>Location</span>
				<code class="pn-settings__path" data-testid="vault-path">{vault.path}</code>
			</div>
			<div class="pn-settings__row">
				<span>Notes indexed</span>
				<span data-testid="vault-count">{vault.note_count}</span>
			</div>
		{/if}
		<div class="pn-settings__row">
			<button
				type="button"
				class="pn-btn"
				onclick={reindex}
				disabled={reindexing}
				data-testid="reindex-btn"
			>
				{reindexing ? 'Reindexing…' : 'Reindex vault'}
			</button>
			<p class="pn-settings__hint">
				Walks every .md file and rebuilds the index. Use only if external
				edits look out of sync.
			</p>
		</div>
		{#if lastReindexSummary}
			<p class="pn-settings__notice" data-testid="reindex-summary">
				{lastReindexSummary}
			</p>
		{/if}
	</section>

	<!-- ─── About ───────────────────────────────────────────────────────── -->
	<section class="pn-settings__section" data-testid="about-section">
		<h2>About</h2>
		<div class="pn-settings__row">
			<span>Version</span>
			<span data-testid="version">v0.1.0-dev</span>
		</div>
		<div class="pn-settings__row">
			<span>License</span>
			<a href="https://www.gnu.org/licenses/agpl-3.0.html">AGPL-3.0-or-later</a>
		</div>
	</section>
</main>
