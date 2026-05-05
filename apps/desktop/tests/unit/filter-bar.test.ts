// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import FilterBar from '$lib/components/FilterBar.svelte';

describe('FilterBar component', () => {
	it('is hidden when no tags are selected', () => {
		render(FilterBar, {
			props: {
				selected: [],
				mode: 'recursive_intersection',
				onSelectionChange: () => {}
			}
		});
		expect(screen.queryByTestId('filterbar')).toBeNull();
	});

	it('renders one chip per selected tag plus the mode chip', () => {
		render(FilterBar, {
			props: {
				selected: ['learning/math', 'work'],
				mode: 'recursive_intersection',
				onSelectionChange: () => {}
			}
		});
		const chips = screen.getAllByTestId('filter-chip');
		expect(chips).toHaveLength(2);
		expect(chips.map((c) => (c as HTMLElement).dataset.path)).toEqual([
			'learning/math',
			'work'
		]);
		expect(screen.getByTestId('filter-mode').textContent).toContain('Recursive ∩');
	});

	it('clicking a close button removes that tag from the selection', async () => {
		const user = userEvent.setup();
		const calls: string[][] = [];
		render(FilterBar, {
			props: {
				selected: ['a', 'b', 'c'],
				mode: 'strict_union',
				onSelectionChange: (next) => calls.push(next)
			}
		});
		const closes = screen.getAllByTestId('filter-chip-close');
		// Remove the middle one ('b').
		await user.click(closes[1]);
		expect(calls).toEqual([['a', 'c']]);
	});

	it('mode chip shows the literal label for each mode', () => {
		const { rerender } = render(FilterBar, {
			props: {
				selected: ['x'],
				mode: 'strict_intersection',
				onSelectionChange: () => {}
			}
		});
		expect(screen.getByTestId('filter-mode').textContent).toContain('Strict ∩');

		rerender({
			selected: ['x'],
			mode: 'recursive_union',
			onSelectionChange: () => {}
		});
		expect(screen.getByTestId('filter-mode').textContent).toContain('Recursive ∪');
	});
});
