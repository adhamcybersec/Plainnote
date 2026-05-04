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

<!--
  Page-scoped styles live in src/app.css under the .pn-landing-* prefix
  (this is a Tailwind 4 + SvelteKit interaction: component <style> blocks
  conflict with @tailwindcss/vite's transform of `?svelte&type=style&lang.css`
  virtual URLs). Real screens in M1a+ will use Tailwind utility classes,
  not custom component CSS.
-->
<main class="pn-landing">
	<header>
		<h1>Plainnote</h1>
		<p class="pn-landing__lede">Local-first notes. Files on disk. No cloud.</p>
	</header>

	<section class="pn-landing__status" aria-live="polite">
		<button class="pn-landing__btn" type="button" onclick={checkBackend}>Check backend</button>
		{#if pong}
			<span class="pn-landing__ok"
				><span class="pn-landing__dot pn-landing__dot--ok" aria-hidden="true"></span> backend says
				<code>{pong}</code></span
			>
		{:else if error}
			<span class="pn-landing__err">backend error: {error}</span>
		{:else}
			<span class="pn-landing__muted">click to call <code>ping</code> via Tauri IPC</span>
		{/if}
	</section>

	<footer class="pn-landing__footer">
		<small>v0.1.0-dev · M0 scaffold · AGPL-3.0</small>
	</footer>
</main>
