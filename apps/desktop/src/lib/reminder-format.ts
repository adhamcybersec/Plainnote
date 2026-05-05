// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Reminder dialog support — date math + plain-language summary.
 *
 * Pure functions, deterministic given a `now` reference. UI calls them
 * with `new Date()` in production; tests pass a fixed reference.
 *
 * Design constraint: literal-language labels only. No emoji, no
 * abbreviations ("min", "hr"). The design
 * brief asks for exact, non-cute copy.
 */

export type LeadTimePreset = 'now' | 'in_5m' | 'in_1h' | 'tomorrow_9am';

export interface LeadTimeOption {
	id: LeadTimePreset;
	label: string;
}

/** Canonical preset list. Order is the order the UI renders. */
export const leadTimePresets: readonly LeadTimeOption[] = [
	{ id: 'now', label: 'Right now' },
	{ id: 'in_5m', label: 'In 5 minutes' },
	{ id: 'in_1h', label: 'In 1 hour' },
	{ id: 'tomorrow_9am', label: 'Tomorrow at 9:00 AM' }
] as const;

/**
 * Turn a preset into an absolute Date relative to `now`. The result is a
 * fresh Date object — callers can `.toISOString()` to ship it over IPC.
 */
export function addLeadTime(preset: LeadTimePreset, now: Date): Date {
	switch (preset) {
		case 'now':
			return new Date(now);
		case 'in_5m':
			return new Date(now.getTime() + 5 * 60_000);
		case 'in_1h':
			return new Date(now.getTime() + 60 * 60_000);
		case 'tomorrow_9am': {
			const out = new Date(now);
			out.setDate(out.getDate() + 1);
			out.setHours(9, 0, 0, 0);
			return out;
		}
	}
}

/**
 * Plain-language description of when a reminder fires. Examples:
 *   - "now"
 *   - "in 5 minutes"
 *   - "in 3 hours"
 *   - "in 2 days"
 *   - "tomorrow at 9:00 AM"  (when delta ≈ 1 day at 9am local)
 *
 * Coarse on purpose — exact wallclock formatting belongs to the dialog
 * itself; this helper is for the inline summary line.
 */
export function plainSummary(fireAt: Date, now: Date): string {
	const deltaMs = fireAt.getTime() - now.getTime();
	if (deltaMs <= 0) return 'now';
	const minutes = Math.round(deltaMs / 60_000);
	if (minutes < 60) {
		return `in ${minutes} ${minutes === 1 ? 'minute' : 'minutes'}`;
	}
	const hours = Math.round(deltaMs / (60 * 60_000));
	if (hours < 24) {
		return `in ${hours} ${hours === 1 ? 'hour' : 'hours'}`;
	}
	const days = Math.round(deltaMs / (24 * 60 * 60_000));
	if (days < 7) {
		return `in ${days} ${days === 1 ? 'day' : 'days'}`;
	}
	// Beyond a week, fall back to absolute date.
	return fireAt.toLocaleDateString(undefined, {
		weekday: 'long',
		year: 'numeric',
		month: 'long',
		day: 'numeric'
	});
}
