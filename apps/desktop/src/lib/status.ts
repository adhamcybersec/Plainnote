// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Tiny helper to announce save-state messages via the global aria-live
 * region (rendered by +layout.svelte). Plain DOM mutation — no Svelte
 * store dependency, so the helper works from any component including
 * test harnesses that don't mount the layout.
 *
 * The region is `aria-live="polite"`: assistive tech queues the message
 * and reads it after the current utterance finishes. Critical state
 * (errors) sets the message exactly once; redundant duplicates are
 * dropped by the aria-live machinery itself.
 */

const REGION_ID = 'pn-status';

export function announce(message: string): void {
	if (typeof document === 'undefined') return;
	const region = document.getElementById(REGION_ID);
	if (!region) return;
	// Clear-then-set so a repeated identical message is still announced.
	// Setting the same textContent twice in a row triggers no DOM mutation
	// on some screen readers; the empty-string interlude breaks the tie.
	region.textContent = '';
	// One microtask later so the cleared value commits first.
	queueMicrotask(() => {
		const r = document.getElementById(REGION_ID);
		if (r) r.textContent = message;
	});
}
