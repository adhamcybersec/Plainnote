// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * groupByRecency: arrange a timeline of NoteSummary into "Today",
 * "Yesterday", earlier same year by month name, older years by "Month YYYY".
 *
 * Pure date math, deterministic given a `now` reference passed in for
 * testability. Library calls it with `new Date()` in production.
 */
import { groupByRecency } from '$lib/timeline';
import type { NoteSummary } from '$lib/ipc';

function note(id: string, createdISO: string): NoteSummary {
	return {
		id,
		title: id,
		created: createdISO,
		updated: createdISO,
		preview: ''
	};
}

describe('groupByRecency', () => {
	const NOW = new Date('2026-05-05T12:00:00Z');

	it('returns no groups for an empty input', () => {
		expect(groupByRecency([], NOW)).toEqual([]);
	});

	it('puts notes from today under "Today"', () => {
		const items = [
			note('a', '2026-05-05T08:00:00Z'),
			note('b', '2026-05-05T11:30:00Z')
		];
		const groups = groupByRecency(items, NOW);
		expect(groups).toHaveLength(1);
		expect(groups[0].label).toBe('Today');
		expect(groups[0].items.map((i) => i.id)).toEqual(['a', 'b']);
	});

	it('puts notes from the calendar day before under "Yesterday"', () => {
		// Construct a date that is unambiguously one local-day before NOW
		// in the test runner's timezone — this avoids the trap that
		// "23:59Z May 4" is "May 5 local" in UTC+ timezones.
		const yesterdayNoon = new Date(NOW);
		yesterdayNoon.setDate(NOW.getDate() - 1);
		yesterdayNoon.setHours(12, 0, 0, 0);
		const items = [note('y', yesterdayNoon.toISOString())];
		const groups = groupByRecency(items, NOW);
		expect(groups[0].label).toBe('Yesterday');
	});

	it('uses month name for earlier dates in the same year', () => {
		const items = [note('m', '2026-03-12T10:00:00Z')];
		const groups = groupByRecency(items, NOW);
		expect(groups[0].label).toBe('March');
	});

	it('uses "Month YYYY" for older years', () => {
		const items = [note('o', '2025-11-02T10:00:00Z')];
		const groups = groupByRecency(items, NOW);
		expect(groups[0].label).toBe('November 2025');
	});

	it('preserves input order within each group', () => {
		// Library sends newest-first; grouping should not re-sort.
		const items = [
			note('a', '2026-05-05T11:00:00Z'),
			note('b', '2026-05-05T08:00:00Z'),
			note('c', '2026-05-04T15:00:00Z')
		];
		const groups = groupByRecency(items, NOW);
		expect(groups[0].items.map((i) => i.id)).toEqual(['a', 'b']);
		expect(groups[1].items.map((i) => i.id)).toEqual(['c']);
	});

	it('emits groups in input order (no re-sorting across groups)', () => {
		// Caller is responsible for newest-first ordering. Use NOW-relative
		// dates so the test is timezone-independent.
		const today = new Date(NOW);
		const yesterday = new Date(NOW);
		yesterday.setDate(NOW.getDate() - 1);
		const march = new Date(NOW.getFullYear(), 2, 15, 12, 0, 0);
		const lastyear = new Date(NOW.getFullYear() - 1, 10, 2, 12, 0, 0);
		const items = [
			note('today', today.toISOString()),
			note('yesterday', yesterday.toISOString()),
			note('march', march.toISOString()),
			note('lastyear', lastyear.toISOString())
		];
		const groups = groupByRecency(items, NOW);
		expect(groups.map((g) => g.label)).toEqual([
			'Today',
			'Yesterday',
			'March',
			`November ${NOW.getFullYear() - 1}`
		]);
	});

	it('groups are independent of UTC vs local: uses local calendar day', () => {
		// `now` is at UTC 12:00 on May 5; local "today" depends on the test
		// runner's timezone but the function compares within one timezone
		// consistently. Asserting that two notes whose local-day matches
		// `now`'s local-day land in "Today" together.
		const dayStart = new Date(NOW);
		dayStart.setHours(0, 1, 0, 0);
		const dayEnd = new Date(NOW);
		dayEnd.setHours(23, 59, 0, 0);
		const items = [
			note('a', dayStart.toISOString()),
			note('b', dayEnd.toISOString())
		];
		const groups = groupByRecency(items, NOW);
		expect(groups).toHaveLength(1);
		expect(groups[0].label).toBe('Today');
	});

	it('handles invalid timestamps by dropping them rather than crashing', () => {
		const items = [
			note('good', '2026-05-05T10:00:00Z'),
			note('bad', 'not-a-date')
		];
		const groups = groupByRecency(items, NOW);
		// The good note still groups; the bad one is dropped silently
		// (UI should never render NaN dates).
		expect(groups).toHaveLength(1);
		expect(groups[0].items.map((i) => i.id)).toEqual(['good']);
	});
});
