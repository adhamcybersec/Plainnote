<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
  M0 landing page. Real Capture screen (R4 zero-friction) lands in M1a.
  This file exists to prove tokens load, the window opens, and IPC is wired.
-->
<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';

	let pong = $state<string | null>(null);
	let error = $state<string | null>(null);

	async function checkBackend() {
		try {
			pong = await invoke<string>('ping');
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		}
	}
</script>

<main class="page">
	<header>
		<h1>Plainnote</h1>
		<p class="lede">Local-first notes. Files on disk. No cloud.</p>
	</header>

	<section class="status" aria-live="polite">
		<button type="button" onclick={checkBackend}>Check backend</button>
		{#if pong}
			<span class="ok"
				><span class="dot dot--ok" aria-hidden="true"></span> backend says <code>{pong}</code></span
			>
		{:else if error}
			<span class="err">backend error: {error}</span>
		{:else}
			<span class="muted">click to call <code>ping</code> via Tauri IPC</span>
		{/if}
	</section>

	<footer>
		<small>v0.1.0-dev · M0 scaffold · AGPL-3.0</small>
	</footer>
</main>

<style>
	.page {
		max-width: 720px;
		margin: 0 auto;
		padding: var(--s-12) var(--s-6);
		display: flex;
		flex-direction: column;
		gap: var(--s-6);
		min-height: 100%;
	}

	h1 {
		margin: 0;
		font-size: 32px;
		font-weight: 600;
		letter-spacing: -0.01em;
		color: var(--ink);
	}

	.lede {
		margin: var(--s-2) 0 0;
		color: var(--ink-2);
		font-size: 15px;
	}

	.status {
		display: flex;
		align-items: center;
		gap: var(--s-3);
		padding: var(--row-pad);
		background: var(--surface);
		border: 1px solid var(--rule);
		border-radius: var(--r-3);
		box-shadow: var(--shadow-1);
		font-size: var(--text-ui);
	}

	button {
		font: inherit;
		padding: var(--s-2) var(--s-4);
		background: var(--accent);
		color: var(--surface);
		border: 1px solid var(--accent);
		border-radius: var(--r-2);
		cursor: pointer;
		transition: filter var(--t-fast) var(--ease-out);
	}
	button:hover {
		filter: brightness(0.95);
	}
	button:active {
		filter: brightness(0.9);
	}

	.dot {
		display: inline-block;
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--ink-3);
	}
	.dot--ok {
		background: var(--dot-ok);
	}

	code {
		font-family: var(--font-mono);
		font-size: 13px;
		padding: 2px 6px;
		background: var(--surface-2);
		border-radius: var(--r-1);
	}

	.muted {
		color: var(--ink-3);
	}
	.ok {
		color: var(--ink);
	}
	.err {
		color: var(--danger);
	}

	footer {
		margin-top: auto;
		padding-top: var(--s-4);
		border-top: 1px solid var(--rule);
		color: var(--ink-3);
	}
</style>
