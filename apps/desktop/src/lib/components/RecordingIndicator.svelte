<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	RecordingIndicator — visible UI state during voice capture.

	Pure presentational component. Parent (the capture page in M4-T8) owns
	polling get_transcription_state via IPC and passes the resulting state
	as a prop. Parent also passes the stop and dismiss callbacks; this
	component does not call IPC directly.

	States:
	  idle         → renders nothing
	  recording    → red pulsing dot, mm:ss elapsed timer, Stop button
	  transcribing → spinner + "Transcribing…"
	  error        → red-tinted bar with the message + Dismiss button

	Animation: animate-pulse / animate-spin classes are used. The global
	prefers-reduced-motion handling in tokens.css (and the
	[data-reduce-motion='true'] override) collapses transition / animation
	durations and iteration counts globally, so no component-local guard
	is needed.
-->
<script lang="ts">
	import type { RecordingState } from '$lib/ipc';

	let {
		state: recState,
		onStop = () => {},
		onDismiss = () => {}
	} = $props<{
		state: RecordingState;
		onStop?: () => void;
		onDismiss?: () => void;
	}>();

	// Local "now" tick for elapsed-time rendering. Updates every second while
	// recording is active. Cleared on component teardown via $effect cleanup.
	let now = $state(Date.now());

	$effect(() => {
		if (recState.kind !== 'recording') return;
		const handle = setInterval(() => {
			now = Date.now();
		}, 1000);
		return () => clearInterval(handle);
	});

	function elapsedMs(): number {
		if (recState.kind !== 'recording') return 0;
		return Math.max(0, now - recState.started_at_ms);
	}

	function formatMmSs(ms: number): string {
		const totalSec = Math.floor(ms / 1000);
		const m = Math.floor(totalSec / 60);
		const s = totalSec % 60;
		return `${m.toString().padStart(2, '0')}:${s.toString().padStart(2, '0')}`;
	}
</script>

{#if recState.kind === 'recording'}
	<div
		data-testid="recording-indicator"
		data-state="recording"
		class="flex items-center gap-3 px-3 py-2 rounded border"
		style="background: var(--surface); border-color: var(--rule); color: var(--ink);"
		role="status"
		aria-live="polite"
	>
		<span
			data-testid="recording-dot"
			class="inline-block w-3 h-3 rounded-full animate-pulse"
			style="background: var(--danger);"
			aria-hidden="true"
		></span>
		<span
			data-testid="recording-timer"
			class="font-mono text-sm tabular-nums"
			style="color: var(--ink);"
		>{formatMmSs(elapsedMs())}</span>
		<button
			data-testid="recording-stop"
			type="button"
			class="ml-auto px-3 py-1 rounded text-sm font-medium border"
			style="background: var(--accent); color: var(--bg); border-color: var(--accent);"
			aria-label="Stop recording and transcribe"
			onclick={onStop}
		>
			Stop
		</button>
	</div>
{:else if recState.kind === 'transcribing'}
	<div
		data-testid="recording-indicator"
		data-state="transcribing"
		class="flex items-center gap-3 px-3 py-2 rounded border"
		style="background: var(--surface); border-color: var(--rule); color: var(--ink);"
		role="status"
		aria-live="polite"
	>
		<span
			data-testid="transcribing-spinner"
			class="inline-block w-4 h-4 rounded-full border-2 border-t-transparent animate-spin"
			style="border-color: var(--accent); border-top-color: transparent;"
			aria-hidden="true"
		></span>
		<span class="text-sm" style="color: var(--ink);">Transcribing…</span>
	</div>
{:else if recState.kind === 'error'}
	<div
		data-testid="recording-indicator"
		data-state="error"
		class="flex items-center gap-3 px-3 py-2 rounded border"
		style="background: var(--accent-soft); border-color: var(--danger); color: var(--ink);"
		role="alert"
	>
		<span
			data-testid="recording-error-message"
			class="text-sm flex-1"
			style="color: var(--danger);"
		>{recState.message}</span>
		<button
			data-testid="recording-dismiss"
			type="button"
			class="px-3 py-1 rounded text-sm font-medium border"
			style="background: transparent; color: var(--ink); border-color: var(--rule);"
			aria-label="Dismiss recording error"
			onclick={onDismiss}
		>
			Dismiss
		</button>
	</div>
{/if}
