// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Appearance controller — applies persisted theme / density / accent /
 * reduce-motion prefs to the <html> element. The Settings page calls
 * `applyAppearance` after writing a meta key; the +layout calls
 * `loadAndApplyAppearance` once on app boot.
 *
 * Storage convention: every preference is a string in the meta table.
 * Empty / missing values fall back to defaults so a fresh vault renders
 * consistently with the design tokens' light defaults.
 */
import { getMeta, setMeta } from '$lib/ipc';

export type Theme = 'light' | 'dark' | 'auto';
export type Density = 'comfortable' | 'compact';
export type Accent = 'sage' | 'teal' | 'ochre' | 'clay' | 'ink' | 'mauve';

export const THEMES: readonly { id: Theme; label: string }[] = [
	{ id: 'light', label: 'Light' },
	{ id: 'dark', label: 'Dark' },
	{ id: 'auto', label: 'Match system' }
] as const;

export const DENSITIES: readonly { id: Density; label: string }[] = [
	{ id: 'comfortable', label: 'Comfortable' },
	{ id: 'compact', label: 'Compact' }
] as const;

export const ACCENTS: readonly { id: Accent; label: string; swatch: string }[] = [
	{ id: 'sage', label: 'Sage', swatch: '#5e7c82' },
	{ id: 'teal', label: 'Teal', swatch: '#4f7d80' },
	{ id: 'ochre', label: 'Ochre', swatch: '#b07a3a' },
	{ id: 'clay', label: 'Clay', swatch: '#b06b50' },
	{ id: 'ink', label: 'Ink', swatch: '#404654' },
	{ id: 'mauve', label: 'Mauve', swatch: '#836783' }
] as const;

export const APPEARANCE_KEYS = {
	theme: 'appearance.theme',
	density: 'appearance.density',
	accent: 'appearance.accent',
	reduceMotion: 'appearance.reduce_motion'
} as const;

export interface Appearance {
	theme: Theme;
	density: Density;
	accent: Accent;
	reduceMotion: boolean;
}

const DEFAULTS: Appearance = {
	theme: 'auto',
	density: 'comfortable',
	accent: 'sage',
	reduceMotion: false
};

function isTheme(s: string | null): s is Theme {
	return s === 'light' || s === 'dark' || s === 'auto';
}
function isDensity(s: string | null): s is Density {
	return s === 'comfortable' || s === 'compact';
}
function isAccent(s: string | null): s is Accent {
	return ACCENTS.some((a) => a.id === s);
}

/**
 * Apply the given Appearance to the <html> element. Pure DOM mutation;
 * does not touch storage. Tests can call with a custom `root` for jsdom.
 */
export function applyAppearance(a: Appearance, root: HTMLElement = document.documentElement): void {
	root.dataset.theme = a.theme;
	root.dataset.density = a.density;
	root.dataset.accent = a.accent;
	root.dataset.reduceMotion = a.reduceMotion ? 'true' : 'false';
}

/**
 * Read all four prefs from the meta table, fall back to defaults, and
 * apply to the document. Called from +layout.svelte at boot so the
 * theme is in place before any route renders.
 *
 * Errors are swallowed: a vault that can't reach IPC (e.g. a unit test
 * environment) gets the design defaults rather than a broken page.
 */
export async function loadAndApplyAppearance(
	root: HTMLElement = document.documentElement
): Promise<Appearance> {
	let appearance = { ...DEFAULTS };
	try {
		const [theme, density, accent, reduce] = await Promise.all([
			getMeta(APPEARANCE_KEYS.theme),
			getMeta(APPEARANCE_KEYS.density),
			getMeta(APPEARANCE_KEYS.accent),
			getMeta(APPEARANCE_KEYS.reduceMotion)
		]);
		if (isTheme(theme)) appearance.theme = theme;
		if (isDensity(density)) appearance.density = density;
		if (isAccent(accent)) appearance.accent = accent;
		if (reduce === 'true') appearance.reduceMotion = true;
	} catch {
		// IPC unavailable — keep defaults.
	}
	applyAppearance(appearance, root);
	return appearance;
}

/** Persist a single appearance preference. Returns the new value. */
export async function setAppearancePref<K extends keyof Appearance>(
	key: K,
	value: Appearance[K]
): Promise<void> {
	const metaKey =
		key === 'reduceMotion'
			? APPEARANCE_KEYS.reduceMotion
			: APPEARANCE_KEYS[key as Exclude<keyof Appearance, 'reduceMotion'>];
	const stored = typeof value === 'boolean' ? (value ? 'true' : 'false') : String(value);
	await setMeta(metaKey, stored);
}
