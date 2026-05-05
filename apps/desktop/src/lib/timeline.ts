// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Timeline grouping for the Library feed.
 *
 * Pure date math, deterministic given a `now` reference (production calls
 * `new Date()`; tests pass a fixed reference for reproducibility).
 *
 * Labels are literal-language strings — no emoji, no abbreviations, per
 * the design prompt's restraint constraints.
 */
import type { NoteSummary } from '$lib/ipc';

export interface TimelineGroup {
	label: string;
	items: NoteSummary[];
}

const MONTHS = [
	'January',
	'February',
	'March',
	'April',
	'May',
	'June',
	'July',
	'August',
	'September',
	'October',
	'November',
	'December'
];

/** True if both Dates fall on the same local calendar day. */
function sameLocalDay(a: Date, b: Date): boolean {
	return (
		a.getFullYear() === b.getFullYear() &&
		a.getMonth() === b.getMonth() &&
		a.getDate() === b.getDate()
	);
}

/** Label for a single note's created date relative to `now`. */
function labelFor(created: Date, now: Date): string {
	if (sameLocalDay(created, now)) return 'Today';
	const yesterday = new Date(now);
	yesterday.setDate(now.getDate() - 1);
	if (sameLocalDay(created, yesterday)) return 'Yesterday';
	const month = MONTHS[created.getMonth()];
	if (created.getFullYear() === now.getFullYear()) return month;
	return `${month} ${created.getFullYear()}`;
}

/**
 * Group an already-sorted list of summaries by recency. Input order is
 * preserved within and across groups — callers send newest-first and we
 * emit groups in the same order.
 *
 * Notes whose `created` is unparseable are dropped silently so the UI
 * never renders `Invalid Date` headers.
 */
export function groupByRecency(
	items: NoteSummary[],
	now: Date = new Date()
): TimelineGroup[] {
	const groups: TimelineGroup[] = [];
	let current: TimelineGroup | null = null;
	for (const item of items) {
		const t = new Date(item.created);
		if (Number.isNaN(t.getTime())) continue;
		const label = labelFor(t, now);
		if (!current || current.label !== label) {
			current = { label, items: [] };
			groups.push(current);
		}
		current.items.push(item);
	}
	return groups;
}
