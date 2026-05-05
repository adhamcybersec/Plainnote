// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Date math + plain-language summaries for the reminder dialog.
 * Pure functions, deterministic given a `now` reference.
 */
import {
	addLeadTime,
	plainSummary,
	leadTimePresets,
	type LeadTimePreset
} from '$lib/reminder-format';

describe('addLeadTime', () => {
	const NOW = new Date('2026-05-05T12:00:00Z');

	it('returns the same time for the "now" preset', () => {
		expect(addLeadTime('now', NOW).toISOString()).toBe(NOW.toISOString());
	});

	it('adds 5 minutes for "in_5m"', () => {
		const out = addLeadTime('in_5m', NOW);
		expect(out.getTime() - NOW.getTime()).toBe(5 * 60_000);
	});

	it('adds 1 hour for "in_1h"', () => {
		const out = addLeadTime('in_1h', NOW);
		expect(out.getTime() - NOW.getTime()).toBe(60 * 60_000);
	});

	it('adds 1 day for "tomorrow_9am"', () => {
		// Tomorrow at 09:00 local. From 2026-05-05T12:00Z this lands on May 6.
		const out = addLeadTime('tomorrow_9am', NOW);
		expect(out.getDate()).toBe(NOW.getDate() + 1);
		expect(out.getHours()).toBe(9);
		expect(out.getMinutes()).toBe(0);
	});

	it('rolls into next month at month boundary for tomorrow_9am', () => {
		const lastOfMonth = new Date('2026-05-31T15:00:00Z');
		const out = addLeadTime('tomorrow_9am', lastOfMonth);
		// Should be 2026-06-01 at 09:00 local.
		expect(out.getMonth()).toBe(5); // June (zero-indexed)
		expect(out.getDate()).toBe(1);
		expect(out.getHours()).toBe(9);
	});
});

describe('leadTimePresets', () => {
	it('exposes the four canonical presets in the documented order', () => {
		const labels = leadTimePresets.map((p) => p.id as LeadTimePreset);
		expect(labels).toEqual(['now', 'in_5m', 'in_1h', 'tomorrow_9am']);
	});

	it('every preset has a literal-language label', () => {
		for (const p of leadTimePresets) {
			expect(p.label.length).toBeGreaterThan(0);
			// No emoji / no abbreviations per design.
			expect(p.label).not.toMatch(/[\u{1F300}-\u{1FAFF}]/u);
		}
	});
});

describe('plainSummary', () => {
	const NOW = new Date('2026-05-05T12:00:00Z');

	it('says "now" when fire_at is in the past or current', () => {
		expect(plainSummary(NOW, NOW)).toMatch(/now/i);
		const past = new Date(NOW.getTime() - 60_000);
		expect(plainSummary(past, NOW)).toMatch(/now/i);
	});

	it('uses minutes for sub-hour deltas', () => {
		const at = new Date(NOW.getTime() + 7 * 60_000);
		expect(plainSummary(at, NOW)).toMatch(/in 7 minutes/i);
	});

	it('singular minute', () => {
		const at = new Date(NOW.getTime() + 60_000);
		expect(plainSummary(at, NOW)).toMatch(/in 1 minute/i);
	});

	it('uses hours for sub-day deltas', () => {
		const at = new Date(NOW.getTime() + 3 * 60 * 60_000);
		expect(plainSummary(at, NOW)).toMatch(/in 3 hours/i);
	});

	it('uses days for multi-day deltas', () => {
		const at = new Date(NOW.getTime() + 2 * 24 * 60 * 60_000);
		expect(plainSummary(at, NOW)).toMatch(/in 2 days/i);
	});

	it('formats wallclock time for tomorrow_9am-style', () => {
		// 21h ahead — "in 21 hours" or "tomorrow at 9:00 AM" — we accept either
		// shape; the contract is "human-readable".
		const at = new Date(NOW.getTime() + 21 * 60 * 60_000);
		const summary = plainSummary(at, NOW);
		expect(summary.length).toBeGreaterThan(0);
		expect(summary).toMatch(/\d/); // contains a numeric part
	});
});
