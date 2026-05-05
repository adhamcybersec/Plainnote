// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Appearance controller — applies persisted prefs to the <html> element.
 * Pure DOM mutation; we drive it with a fresh detached div per test so
 * the assertions don't leak between cases.
 */
import {
	applyAppearance,
	loadAndApplyAppearance,
	setAppearancePref,
	APPEARANCE_KEYS,
	ACCENTS,
	THEMES,
	DENSITIES,
	type Appearance
} from '$lib/appearance';
import { setIpcTransport } from '$lib/ipc';

interface InvokeCall {
	cmd: string;
	args?: Record<string, unknown>;
}

function mockIpc(handlers: Record<string, (args?: Record<string, unknown>) => unknown>) {
	const calls: InvokeCall[] = [];
	const t = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
		calls.push({ cmd, args });
		const h = handlers[cmd];
		if (!h) throw new Error(`unexpected: ${cmd}`);
		return h(args) as T;
	};
	return { t, calls };
}

describe('applyAppearance', () => {
	it('sets all four data attributes on the root element', () => {
		const el = document.createElement('div');
		applyAppearance(
			{ theme: 'dark', density: 'compact', accent: 'mauve', reduceMotion: true },
			el
		);
		expect(el.dataset.theme).toBe('dark');
		expect(el.dataset.density).toBe('compact');
		expect(el.dataset.accent).toBe('mauve');
		expect(el.dataset.reduceMotion).toBe('true');
	});

	it('encodes reduce-motion as the literal string "false" when off', () => {
		// CSS attribute selectors check string equality; "false" is meaningful.
		const el = document.createElement('div');
		applyAppearance(
			{ theme: 'light', density: 'comfortable', accent: 'sage', reduceMotion: false },
			el
		);
		expect(el.dataset.reduceMotion).toBe('false');
	});
});

describe('loadAndApplyAppearance', () => {
	it('falls back to defaults when no prefs are stored', async () => {
		const { t } = mockIpc({ get_meta: () => null });
		setIpcTransport(t);
		const el = document.createElement('div');
		const result = await loadAndApplyAppearance(el);
		expect(result).toEqual({
			theme: 'auto',
			density: 'comfortable',
			accent: 'sage',
			reduceMotion: false
		});
		expect(el.dataset.theme).toBe('auto');
	});

	it('hydrates each pref independently from the meta table', async () => {
		const stored: Record<string, string> = {
			[APPEARANCE_KEYS.theme]: 'dark',
			[APPEARANCE_KEYS.density]: 'compact',
			[APPEARANCE_KEYS.accent]: 'ochre',
			[APPEARANCE_KEYS.reduceMotion]: 'true'
		};
		const { t } = mockIpc({
			get_meta: (args) => stored[(args as { key: string }).key] ?? null
		});
		setIpcTransport(t);
		const el = document.createElement('div');
		const result = await loadAndApplyAppearance(el);
		expect(result.theme).toBe('dark');
		expect(result.density).toBe('compact');
		expect(result.accent).toBe('ochre');
		expect(result.reduceMotion).toBe(true);
	});

	it('rejects malformed values and keeps defaults for those keys', async () => {
		// User mucks with the meta table directly: theme='neon' isn't valid.
		// The controller must not apply junk to <html>.
		const stored: Record<string, string> = {
			[APPEARANCE_KEYS.theme]: 'neon',
			[APPEARANCE_KEYS.accent]: 'rainbow',
			[APPEARANCE_KEYS.density]: 'spacious'
		};
		const { t } = mockIpc({
			get_meta: (args) => stored[(args as { key: string }).key] ?? null
		});
		setIpcTransport(t);
		const el = document.createElement('div');
		const result = await loadAndApplyAppearance(el);
		expect(result.theme).toBe('auto');
		expect(result.accent).toBe('sage');
		expect(result.density).toBe('comfortable');
	});

	it('survives an IPC failure by applying defaults', async () => {
		const t = async <_T>(): Promise<never> => {
			throw new Error('no transport');
		};
		setIpcTransport(t);
		const el = document.createElement('div');
		const result = await loadAndApplyAppearance(el);
		expect(result.theme).toBe('auto');
		expect(el.dataset.theme).toBe('auto');
	});
});

describe('setAppearancePref', () => {
	it('persists string-valued prefs as their literal string', async () => {
		const { t, calls } = mockIpc({ set_meta: () => null });
		setIpcTransport(t);
		await setAppearancePref('theme', 'dark');
		expect(calls[0]).toEqual({
			cmd: 'set_meta',
			args: { key: APPEARANCE_KEYS.theme, value: 'dark' }
		});
	});

	it('encodes the boolean reduceMotion as "true" / "false"', async () => {
		const { t, calls } = mockIpc({ set_meta: () => null });
		setIpcTransport(t);
		await setAppearancePref('reduceMotion', true);
		expect(calls[0].args).toEqual({
			key: APPEARANCE_KEYS.reduceMotion,
			value: 'true'
		});
		await setAppearancePref('reduceMotion', false);
		expect(calls[1].args).toEqual({
			key: APPEARANCE_KEYS.reduceMotion,
			value: 'false'
		});
	});
});

describe('preset lists', () => {
	it('exposes the documented options for each preset family', () => {
		expect(THEMES.map((t) => t.id)).toEqual(['light', 'dark', 'auto']);
		expect(DENSITIES.map((d) => d.id)).toEqual(['comfortable', 'compact']);
		expect(ACCENTS).toHaveLength(6);
	});
});
