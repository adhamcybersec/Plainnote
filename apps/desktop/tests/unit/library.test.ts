// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, waitFor } from '@testing-library/svelte';
import { setIpcTransport, type NoteSummary, type TagRow } from '$lib/ipc';
import Library from '../../src/routes/library/+page.svelte';

interface InvokeCall {
	cmd: string;
	args?: Record<string, unknown>;
}

function makeTransport(handlers: Record<string, (args?: Record<string, unknown>) => unknown>) {
	const calls: InvokeCall[] = [];
	const t = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
		calls.push({ cmd, args });
		const handler = handlers[cmd];
		if (!handler) throw new Error(`unexpected cmd: ${cmd}`);
		return handler(args) as T;
	};
	return { t, calls };
}

function transportReturning(notes: NoteSummary[], tags: TagRow[] = []) {
	return makeTransport({
		list_notes: () => notes,
		list_tags: () => tags,
		query_notes: () => notes
	}).t;
}

function transportRejecting(message: string) {
	return async <_T>(): Promise<never> => {
		throw { code: 'io', message };
	};
}

describe('Library list view', () => {
	it('renders an empty state when there are no notes', async () => {
		setIpcTransport(transportReturning([]));
		render(Library);
		await waitFor(() => expect(screen.getByTestId('empty')).toBeInTheDocument());
		expect(screen.getByTestId('count').textContent?.replace(/\s+/g, ' ')).toMatch(/0\s+notes/);
	});

	it('renders one card per summary, newest first', async () => {
		const fixture: NoteSummary[] = [
			{
				id: '01HZZZ0000000000000000000A',
				title: 'newer',
				created: '2026-05-04T12:00:00Z',
				updated: '2026-05-04T12:00:00Z',
				preview: 'newer body'
			},
			{
				id: '01HXYZ0000000000000000000A',
				title: 'older',
				created: '2026-05-03T12:00:00Z',
				updated: '2026-05-03T12:00:00Z',
				preview: 'older body'
			}
		];
		setIpcTransport(transportReturning(fixture));
		render(Library);

		await waitFor(() => {
			const cards = screen.getAllByTestId('card');
			expect(cards).toHaveLength(2);
		});
		const cards = screen.getAllByTestId('card');
		expect(cards[0].textContent).toContain('newer');
		expect(cards[1].textContent).toContain('older');
		expect(screen.getByTestId('count').textContent?.replace(/\s+/g, ' ')).toMatch(/2\s+notes/);
	});

	it('uses preview as the title fallback when title is null', async () => {
		const fixture: NoteSummary[] = [
			{
				id: '01HXYZ0000000000000000000A',
				title: null,
				created: '2026-05-04T12:00:00Z',
				updated: '2026-05-04T12:00:00Z',
				preview: 'first line of body'
			}
		];
		setIpcTransport(transportReturning(fixture));
		render(Library);
		await waitFor(() => {
			const card = screen.getByTestId('card');
			expect(card.textContent).toContain('first line of body');
		});
	});

	it('shows an error message when listNotes rejects', async () => {
		setIpcTransport(transportRejecting('disk full'));
		render(Library);
		await waitFor(() => {
			expect(screen.getByTestId('error').textContent).toContain('disk full');
		});
	});

	it('cards link to /note/<id>', async () => {
		const fixture: NoteSummary[] = [
			{
				id: '01HXYZ0000000000000000000A',
				title: 't',
				created: '2026-05-04T12:00:00Z',
				updated: '2026-05-04T12:00:00Z',
				preview: 'p'
			}
		];
		setIpcTransport(transportReturning(fixture));
		render(Library);
		await waitFor(() => {
			const card = screen.getByTestId('card') as HTMLAnchorElement;
			expect(card.getAttribute('href')).toBe('/note/01HXYZ0000000000000000000A');
		});
	});

	// ─── M2 integration ────────────────────────────────────────────────

	it('initial load uses list_notes (no tags selected)', async () => {
		const { t, calls } = makeTransport({
			list_notes: () => [],
			list_tags: () => []
		});
		setIpcTransport(t);
		render(Library);
		await waitFor(() => expect(screen.getByTestId('empty')).toBeInTheDocument());
		const cmds = calls.map((c) => c.cmd);
		expect(cmds).toContain('list_notes');
		expect(cmds).toContain('list_tags');
		expect(cmds).not.toContain('query_notes');
	});

	it('selecting a tag switches the feed to query_notes', async () => {
		const tags: TagRow[] = [
			{ path: 'work', parent: null, note_count: 1 }
		];
		const { t, calls } = makeTransport({
			list_notes: () => [],
			list_tags: () => tags,
			query_notes: () => []
		});
		setIpcTransport(t);
		const user = (await import('@testing-library/user-event')).default.setup();
		render(Library);

		// Wait for the tag tree to render.
		await waitFor(() => expect(screen.getByText('work')).toBeInTheDocument());

		// Click 'work' to select it.
		const workBtn = screen
			.getAllByRole('button')
			.find((b) => (b as HTMLElement).dataset?.path === 'work') as HTMLElement;
		await user.click(workBtn);

		// query_notes must now have been invoked.
		await waitFor(() => {
			const qn = calls.find((c) => c.cmd === 'query_notes');
			expect(qn).toBeDefined();
			expect((qn!.args as { tags: string[]; mode: string }).tags).toEqual(['work']);
			expect((qn!.args as { mode: string }).mode).toBe('recursive_intersection');
		});
	});

	it('switching mode while tags are selected re-queries', async () => {
		const tags: TagRow[] = [
			{ path: 'work', parent: null, note_count: 1 }
		];
		const { t, calls } = makeTransport({
			list_notes: () => [],
			list_tags: () => tags,
			query_notes: () => []
		});
		setIpcTransport(t);
		const user = (await import('@testing-library/user-event')).default.setup();
		render(Library);
		await waitFor(() => expect(screen.getByText('work')).toBeInTheDocument());

		// Select work, then change mode.
		await user.click(
			screen
				.getAllByRole('button')
				.find((b) => (b as HTMLElement).dataset?.path === 'work') as HTMLElement
		);

		await waitFor(() =>
			expect(calls.filter((c) => c.cmd === 'query_notes').length).toBeGreaterThan(0)
		);

		await user.click(
			screen
				.getAllByTestId('mode-btn')
				.find((b) => (b as HTMLElement).dataset.mode === 'strict_union') as HTMLElement
		);

		// A second query_notes invocation should now have happened with the new mode.
		await waitFor(() => {
			const qn = calls.filter((c) => c.cmd === 'query_notes');
			const last = qn[qn.length - 1];
			expect((last.args as { mode: string }).mode).toBe('strict_union');
		});
	});

	it('removing a tag chip falls back to list_notes when none remain', async () => {
		const tags: TagRow[] = [{ path: 'work', parent: null, note_count: 1 }];
		const { t, calls } = makeTransport({
			list_notes: () => [],
			list_tags: () => tags,
			query_notes: () => []
		});
		setIpcTransport(t);
		const user = (await import('@testing-library/user-event')).default.setup();
		render(Library);
		await waitFor(() => expect(screen.getByText('work')).toBeInTheDocument());

		// Select then deselect via the FilterBar close button.
		await user.click(
			screen
				.getAllByRole('button')
				.find((b) => (b as HTMLElement).dataset?.path === 'work') as HTMLElement
		);
		await waitFor(() => expect(screen.getByTestId('filter-chip')).toBeInTheDocument());

		await user.click(screen.getByTestId('filter-chip-close'));
		await waitFor(() => expect(screen.queryByTestId('filterbar')).toBeNull());

		// The most recent feed call after deselection must be list_notes again.
		const after = calls
			.map((c) => c.cmd)
			.filter((c) => c === 'list_notes' || c === 'query_notes');
		expect(after[after.length - 1]).toBe('list_notes');
	});
});
