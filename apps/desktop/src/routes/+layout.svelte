<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<script lang="ts">
	import '../app.css';
	import { onMount } from 'svelte';
	import { loadAndApplyAppearance } from '$lib/appearance';

	let { children } = $props();

	onMount(() => {
		// Apply persisted appearance prefs as soon as the layout mounts so
		// the first painted route already shows the right theme. Errors
		// fall through silently — design defaults are applied either way.
		void loadAndApplyAppearance();
	});
</script>

<!-- aria-live region for save-state announcements (M9-T2). The Settings
     page and capture-flow components write into this region's text via
     a tiny store; assistive tech reads it without stealing focus. -->
<div id="pn-status" class="pn-sr-only" aria-live="polite" data-testid="status-region"></div>

{@render children()}
