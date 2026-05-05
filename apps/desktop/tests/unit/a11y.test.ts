// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Accessibility audit (M9-T2).
 *
 * These are smoke tests for the contract; they don't replace a real
 * Orca / NVDA pass on a packaged build. The goal is to catch regressions
 * — e.g. a future PR that strips the aria-live region or adds a button
 * with no accessible name.
 */
import { render, screen, fireEvent } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { announce } from '$lib/status';
import { applyAppearance } from '$lib/appearance';

vi.mock('$app/state', () => ({
	page: { params: { id: '01HXYZ0000000000000000000A' } }
}));
vi.mock('$app/navigation', () => ({
	goto: vi.fn(async () => undefined)
}));

describe('announce()', () => {
	beforeEach(() => {
		// Mount the global aria-live region the way +layout.svelte does.
		const region = document.createElement('div');
		region.id = 'pn-status';
		document.body.appendChild(region);
	});
	afterEach(() => {
		document.getElementById('pn-status')?.remove();
	});

	it('writes the message to the aria-live region', async () => {
		announce('Note saved.');
		// announce uses queueMicrotask to clear-then-set — wait one turn.
		await new Promise((r) => queueMicrotask(() => r(undefined)));
		expect(document.getElementById('pn-status')!.textContent).toBe('Note saved.');
	});

	it('is a no-op when the region is missing (does not throw)', async () => {
		document.getElementById('pn-status')?.remove();
		expect(() => announce('orphan')).not.toThrow();
	});

	it('is a no-op outside a DOM context (does not throw)', () => {
		// Simulate SSR by stashing document. We use a flag on globalThis
		// rather than deleting `document` to avoid jsdom quirks.
		const orig = globalThis.document;
		// @ts-expect-error — intentionally unset for the test
		globalThis.document = undefined;
		try {
			expect(() => announce('ssr')).not.toThrow();
		} finally {
			globalThis.document = orig;
		}
	});
});

describe('applyAppearance — reduce motion', () => {
	it('sets data-reduce-motion="true" when toggled on', () => {
		const el = document.createElement('div');
		applyAppearance(
			{ theme: 'auto', density: 'comfortable', accent: 'sage', reduceMotion: true },
			el
		);
		expect(el.dataset.reduceMotion).toBe('true');
	});

	it('sets data-reduce-motion="false" when toggled off (CSS attribute selector contract)', () => {
		// The CSS selector [data-reduce-motion='true'] only fires on the
		// literal string "true"; "false" or absence is a non-match. Verify
		// we encode the off-state explicitly so a future move to a different
		// selector convention is loud.
		const el = document.createElement('div');
		applyAppearance(
			{ theme: 'auto', density: 'comfortable', accent: 'sage', reduceMotion: false },
			el
		);
		expect(el.dataset.reduceMotion).toBe('false');
	});
});

describe('Capture page — accessible names + announcements', () => {
	it('save button has a discernible name (M9-T2)', async () => {
		// Mount the global region so capture's announce() lands somewhere.
		const region = document.createElement('div');
		region.id = 'pn-status';
		document.body.appendChild(region);

		// Mock the IPC so saveNote completes without a real Tauri.
		const { setIpcTransport } = await import('$lib/ipc');
		const t = async <T>(cmd: string, _args?: Record<string, unknown>): Promise<T> => {
			if (cmd === 'save_note') return '01HABC0000000000000000000A' as T;
			throw new Error(`unexpected: ${cmd}`);
		};
		setIpcTransport(t);

		const { default: CapturePage } = await import('../../src/routes/+page.svelte');
		render(CapturePage);

		// Every button on the page either has visible text content or
		// aria-label — the screen reader will read SOMETHING for each.
		for (const btn of screen.getAllByRole('button')) {
			const visible = (btn.textContent ?? '').trim();
			const labelled = btn.getAttribute('aria-label');
			expect(visible || labelled).toBeTruthy();
		}

		// Saving via Ctrl+Enter announces 'Note saved.' on the global region.
		const user = userEvent.setup();
		const ta = screen.getByLabelText(/capture a note/i);
		await user.click(ta);
		await user.keyboard('hello');
		await user.keyboard('{Control>}{Enter}{/Control}');
		await new Promise((r) => setTimeout(r, 50));
		expect(document.getElementById('pn-status')!.textContent).toMatch(
			/note saved/i
		);

		document.getElementById('pn-status')?.remove();
	});
});

describe('Settings appearance section — keyboard navigation', () => {
	it('every accent swatch is reachable as a radio with aria-checked', async () => {
		const { setIpcTransport } = await import('$lib/ipc');
		const t = async <T>(cmd: string, _args?: Record<string, unknown>): Promise<T> => {
			switch (cmd) {
				case 'vault_info':
					return { path: '/x', note_count: 0 } as T;
				case 'list_reminders':
					return [] as T;
				case 'get_meta':
					return null as T;
				case 'set_meta':
					return null as T;
			}
			throw new Error(`unexpected: ${cmd}`);
		};
		setIpcTransport(t);
		const { default: SettingsPage } = await import('../../src/routes/settings/+page.svelte');
		render(SettingsPage);

		const grid = await screen.findByTestId('accent-grid');
		const swatches = grid.querySelectorAll('[role="radio"]');
		expect(swatches.length).toBe(6);
		// Exactly one is aria-checked at a time (the active accent).
		const checked = Array.from(swatches).filter(
			(s) => s.getAttribute('aria-checked') === 'true'
		);
		expect(checked.length).toBe(1);
		// Every swatch carries an aria-label.
		for (const s of swatches) {
			expect(s.getAttribute('aria-label')).toBeTruthy();
		}
	});
});
