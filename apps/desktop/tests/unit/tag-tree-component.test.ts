// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import TagTree from '$lib/components/TagTree.svelte';
import type { TagRow } from '$lib/ipc';

const sampleRows: TagRow[] = [
	{ path: 'learning', parent: null, note_count: 4 },
	{ path: 'learning/math', parent: 'learning', note_count: 2 },
	{ path: 'learning/math/calculus', parent: 'learning/math', note_count: 1 },
	{ path: 'learning/physics', parent: 'learning', note_count: 1 },
	{ path: 'work', parent: null, note_count: 0 }
];

describe('TagTree component', () => {
	it('renders empty state when there are no tags', () => {
		render(TagTree, {
			props: { rows: [], selected: [], onSelectionChange: () => {} }
		});
		expect(screen.getByTestId('empty')).toBeInTheDocument();
	});

	it('renders one row per tag with leaf labels and counts', () => {
		render(TagTree, {
			props: { rows: sampleRows, selected: [], onSelectionChange: () => {} }
		});
		// 5 tags = 5 rows.
		expect(screen.getAllByTestId('tree-row')).toHaveLength(5);
		// Leaf labels appear in document.
		expect(screen.getByText('learning')).toBeInTheDocument();
		expect(screen.getByText('math')).toBeInTheDocument();
		expect(screen.getByText('calculus')).toBeInTheDocument();
		expect(screen.getByText('physics')).toBeInTheDocument();
		expect(screen.getByText('work')).toBeInTheDocument();
	});

	it('clicking a row replaces the selection (single-select)', async () => {
		const user = userEvent.setup();
		const calls: string[][] = [];
		render(TagTree, {
			props: {
				rows: sampleRows,
				selected: [],
				onSelectionChange: (next) => calls.push(next)
			}
		});
		const learning = screen
			.getAllByRole('button', { pressed: false })
			.find((b) => b.dataset.path === 'learning');
		await user.click(learning!);
		expect(calls).toEqual([['learning']]);
	});

	it('Ctrl+click adds to the selection (multi-select)', async () => {
		const user = userEvent.setup();
		const calls: string[][] = [];
		render(TagTree, {
			props: {
				rows: sampleRows,
				selected: ['learning'],
				onSelectionChange: (next) => calls.push(next)
			}
		});
		const work = screen
			.getAllByRole('button')
			.find((b) => (b as HTMLElement).dataset.path === 'work')! as HTMLElement;
		await user.keyboard('{Control>}');
		await user.click(work);
		await user.keyboard('{/Control}');
		expect(calls[calls.length - 1]).toEqual(['learning', 'work']);
	});

	it('clicking the only selected row clears the selection', async () => {
		const user = userEvent.setup();
		const calls: string[][] = [];
		render(TagTree, {
			props: {
				rows: sampleRows,
				selected: ['learning'],
				onSelectionChange: (next) => calls.push(next)
			}
		});
		const learning = screen
			.getAllByRole('button')
			.find((b) => (b as HTMLElement).dataset.path === 'learning')! as HTMLElement;
		await user.click(learning);
		expect(calls).toEqual([[]]);
	});

	it('marks selected rows with aria-pressed=true', () => {
		render(TagTree, {
			props: {
				rows: sampleRows,
				selected: ['learning/math'],
				onSelectionChange: () => {}
			}
		});
		const buttons = screen.getAllByRole('button', { pressed: true });
		expect(buttons.some((b) => (b as HTMLElement).dataset.path === 'learning/math')).toBe(
			true
		);
	});

	it('clicking the caret toggles expand/collapse without changing selection', async () => {
		const user = userEvent.setup();
		const calls: string[][] = [];
		render(TagTree, {
			props: {
				rows: sampleRows,
				selected: [],
				onSelectionChange: (next) => calls.push(next)
			}
		});
		// All rows visible initially (5).
		expect(screen.getAllByTestId('tree-row')).toHaveLength(5);

		// Find the caret on the 'learning' row and click it.
		const learningCaret = screen
			.getAllByTestId('caret')
			.find((c) => {
				const btn = (c as HTMLElement).closest('button[data-path]') as HTMLElement | null;
				return btn?.dataset.path === 'learning';
			})! as HTMLElement;
		await user.click(learningCaret);

		// Subtree collapsed: math, calculus, physics gone (3 hidden).
		expect(screen.getAllByTestId('tree-row').length).toBeLessThan(5);
		// Selection callback must NOT have fired.
		expect(calls).toEqual([]);
	});
});
