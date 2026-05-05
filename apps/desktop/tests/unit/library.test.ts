// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, waitFor } from '@testing-library/svelte';
import { setIpcTransport, type NoteSummary } from '$lib/ipc';
import Library from '../../src/routes/library/+page.svelte';

function transportReturning(notes: NoteSummary[]) {
	return async <T>(_cmd: string): Promise<T> => notes as unknown as T;
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
		expect(screen.getByTestId('count').textContent).toContain('0 notes');
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
		expect(screen.getByTestId('count').textContent).toContain('2 notes');
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
});
