<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
<!--
	ModelMissingDialog — first-run prompt shown when start_recording rejects
	with IpcError { code: 'model_missing' } (M4-T11).

	Plainnote does not bundle a whisper.cpp model (ADR-010): the binary is
	~150 MB and we want zero network traffic by default. This dialog tells
	the user where to drop the file, gives the official URL and SHA-1, and
	provides a copy-paste-friendly mkdir + curl + sha1sum block.

	Visibility is controlled by the parent via {#if …}; this component does
	not own its own open state.

	Clipboard: uses navigator.clipboard.writeText with a try/catch swallow.
	Tauri's clipboard plugin is not wired in v0.1, and adding it for one
	button is out of scope.
-->
<script lang="ts">
	const DEFAULT_MODEL_URL =
		'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin';
	const DEFAULT_MODEL_SHA1 = '137c40403d78fd54d454da0f9bd998f78703390c';

	let {
		expectedPath,
		onRetry = () => {},
		onCancel = () => {}
	} = $props<{
		expectedPath: string;
		onRetry?: () => void;
		onCancel?: () => void;
	}>();

	let pathCopied = $state(false);
	let urlCopied = $state(false);

	function dirOf(p: string): string {
		const ix = p.lastIndexOf('/');
		return ix > 0 ? p.slice(0, ix) : '.';
	}

	let quickCommand = $derived(
		[
			`mkdir -p ${dirOf(expectedPath)}`,
			`curl -L ${DEFAULT_MODEL_URL} -o ${expectedPath}`,
			`sha1sum ${expectedPath}`
		].join('\n')
	);

	async function copy(text: string, marker: 'path' | 'url'): Promise<void> {
		try {
			await navigator.clipboard.writeText(text);
			if (marker === 'path') pathCopied = true;
			else urlCopied = true;
			setTimeout(() => {
				if (marker === 'path') pathCopied = false;
				else urlCopied = false;
			}, 1500);
		} catch {
			// Some sandboxed configs disable clipboard. Silently swallow —
			// the user can still hand-copy from the visible text.
		}
	}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role —
     this is a dialog container; focus management is delegated to inner
     interactive elements (per ADR's deferral of focus-trap libraries). -->
<section
	data-testid="model-missing-dialog"
	role="dialog"
	aria-modal="true"
	aria-labelledby="model-missing-title"
	class="pn-reminder-dialog"
	style="max-width: 36rem;"
>
	<header class="pn-reminder-dialog__head">
		<h2 id="model-missing-title" class="pn-reminder-dialog__title">
			Voice needs a model file
		</h2>
		<button type="button" class="pn-reminder-dialog__x" onclick={onCancel}>
			Close
		</button>
	</header>

	<p style="color: var(--ink); margin: 0.5rem 0;">
		Plainnote transcribes voice locally using <a
			href="https://github.com/ggerganov/whisper.cpp"
			target="_blank"
			rel="noopener noreferrer">whisper.cpp</a
		>. We don't bundle a model file because they're large (≥150 MB) and you
		may prefer a different size. Nothing leaves your machine — once the
		file is on disk, transcription runs offline.
	</p>

	<div style="margin: 0.75rem 0;">
		<div style="font-weight: 600; color: var(--ink);">Place the file here:</div>
		<div style="display: flex; gap: 0.5rem; align-items: center; margin-top: 0.25rem;">
			<code
				data-testid="model-missing-path"
				style="flex: 1; padding: 0.35rem 0.5rem; background: var(--surface); border: 1px solid var(--rule); border-radius: 0.25rem; color: var(--ink); word-break: break-all; font-size: 0.85rem;"
				>{expectedPath}</code
			>
			<button
				type="button"
				class="pn-btn pn-btn--ghost"
				data-testid="model-missing-copy-path"
				onclick={() => copy(expectedPath, 'path')}
			>
				{pathCopied ? 'Copied' : 'Copy path'}
			</button>
		</div>
	</div>

	<div style="margin: 0.75rem 0;">
		<div style="color: var(--ink);">
			Default model (good enough for most speech): <code>ggml-base.en.bin</code>
			(~150 MB, SHA-1 <code style="font-size: 0.8rem;">{DEFAULT_MODEL_SHA1}</code>).
		</div>
		<div style="display: flex; gap: 0.5rem; align-items: center; margin-top: 0.35rem;">
			<code
				style="flex: 1; padding: 0.35rem 0.5rem; background: var(--surface); border: 1px solid var(--rule); border-radius: 0.25rem; color: var(--ink); word-break: break-all; font-size: 0.8rem;"
				>{DEFAULT_MODEL_URL}</code
			>
			<button
				type="button"
				class="pn-btn pn-btn--ghost"
				data-testid="model-missing-copy-url"
				onclick={() => copy(DEFAULT_MODEL_URL, 'url')}
			>
				{urlCopied ? 'Copied' : 'Copy URL'}
			</button>
		</div>
	</div>

	<div style="margin: 0.75rem 0;">
		<div style="font-weight: 600; color: var(--ink);">Quick command:</div>
		<pre
			data-testid="model-missing-quick-command"
			style="margin: 0.25rem 0 0; padding: 0.5rem 0.65rem; background: var(--surface); border: 1px solid var(--rule); border-radius: 0.25rem; color: var(--ink); font-size: 0.8rem; white-space: pre-wrap; word-break: break-all;">{quickCommand}</pre>
	</div>

	<p style="color: var(--ink); margin: 0.5rem 0; font-size: 0.9rem;">
		For better accuracy on names and technical terms, use
		<code>ggml-small.en.bin</code> (~466 MB) — point
		<a href="/settings">Settings → Voice &amp; speech</a> at it once
		downloaded.
	</p>

	<footer class="pn-reminder-dialog__foot">
		<button
			type="button"
			class="pn-btn pn-btn--ghost"
			data-testid="model-missing-cancel"
			onclick={onCancel}
		>
			Cancel
		</button>
		<button
			type="button"
			class="pn-btn"
			data-testid="model-missing-retry"
			onclick={onRetry}
		>
			I've placed the model — try again
		</button>
	</footer>
</section>
