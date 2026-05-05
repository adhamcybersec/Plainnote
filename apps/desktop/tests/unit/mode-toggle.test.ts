// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import ModeToggle from '$lib/components/ModeToggle.svelte';
import type { QueryMode } from '$lib/ipc';

describe('ModeToggle component', () => {
	it('renders all four mode buttons', () => {
		render(ModeToggle, {
			props: { mode: 'recursive_intersection', onModeChange: () => {} }
		});
		const buttons = screen.getAllByTestId('mode-btn');
		expect(buttons).toHaveLength(4);
		expect(buttons.map((b) => (b as HTMLElement).dataset.mode)).toEqual([
			'strict_intersection',
			'recursive_intersection',
			'strict_union',
			'recursive_union'
		]);
	});

	it('marks the active mode with aria-pressed=true', () => {
		render(ModeToggle, {
			props: { mode: 'strict_union', onModeChange: () => {} }
		});
		const buttons = screen.getAllByTestId('mode-btn');
		for (const b of buttons) {
			const expected = (b as HTMLElement).dataset.mode === 'strict_union';
			expect(b.getAttribute('aria-pressed')).toBe(String(expected));
		}
	});

	it('shows the literal-language hint matching the current mode', () => {
		const { rerender } = render(ModeToggle, {
			props: { mode: 'recursive_intersection', onModeChange: () => {} }
		});
		expect(screen.getByTestId('mode-hint').textContent).toContain('all the selected branches');

		rerender({ mode: 'strict_intersection', onModeChange: () => {} });
		expect(screen.getByTestId('mode-hint').textContent).toContain('all the selected tags exactly');

		rerender({ mode: 'strict_union', onModeChange: () => {} });
		expect(screen.getByTestId('mode-hint').textContent).toContain('any of the selected tags exactly');

		rerender({ mode: 'recursive_union', onModeChange: () => {} });
		expect(screen.getByTestId('mode-hint').textContent).toContain('any of the selected branches');
	});

	it('clicking a button calls onModeChange with that mode', async () => {
		const user = userEvent.setup();
		const calls: QueryMode[] = [];
		render(ModeToggle, {
			props: {
				mode: 'recursive_intersection',
				onModeChange: (next) => calls.push(next)
			}
		});
		await user.click(
			screen
				.getAllByTestId('mode-btn')
				.find((b) => (b as HTMLElement).dataset.mode === 'strict_union') as HTMLElement
		);
		expect(calls).toEqual(['strict_union']);
	});
});
