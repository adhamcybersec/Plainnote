// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * /graph route — state-machine tests. The Sigma renderer requires WebGL
 * (not in jsdom), so we mock the wrapper component and assert on the
 * route's branching: loading → loaded, empty state, truncated notice,
 * error path. The Sigma rendering itself is covered by E2E in M9 polish.
 */
import { render, screen, waitFor } from '@testing-library/svelte';
import { setIpcTransport } from '$lib/ipc';

vi.mock('$lib/components/SigmaWrapper.svelte', async () => {
	// Tiny stub: a no-op component that just renders a marker so tests
	// can assert "the renderer mounted with data".
	const SigmaStub = (await import('./harness/SigmaStub.svelte')).default;
	return { default: SigmaStub };
});

vi.mock('$app/navigation', () => ({
	goto: vi.fn(async () => undefined)
}));

import GraphPage from '../../src/routes/graph/+page.svelte';

interface InvokeCall {
	cmd: string;
	args?: Record<string, unknown>;
}

function transportReturning(payload: unknown) {
	const calls: InvokeCall[] = [];
	const t = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
		calls.push({ cmd, args });
		if (cmd !== 'graph_data') throw new Error(`unexpected cmd: ${cmd}`);
		return payload as T;
	};
	return { t, calls };
}

describe('Graph route', () => {
	it('renders the empty state when the index has no notes', async () => {
		const { t } = transportReturning({ nodes: [], edges: [], truncated: false });
		setIpcTransport(t);
		render(GraphPage);
		await waitFor(() => expect(screen.getByTestId('empty')).toBeInTheDocument());
		expect(screen.getByTestId('empty').textContent).toMatch(/no notes yet/i);
	});

	it('renders the SigmaWrapper stub when data is loaded', async () => {
		const fixture = {
			nodes: [
				{ id: '01HABC0000000000000000000A', title: 'A', size: 6, x: 0, y: 0 },
				{ id: '01HABC0000000000000000000B', title: 'B', size: 6, x: 1, y: 0 }
			],
			edges: [
				{
					source: '01HABC0000000000000000000A',
					target: '01HABC0000000000000000000B'
				}
			],
			truncated: false
		};
		const { t } = transportReturning(fixture);
		setIpcTransport(t);
		render(GraphPage);
		await waitFor(() => expect(screen.getByTestId('sigma-stub')).toBeInTheDocument());
		// The header count reflects the loaded data.
		expect(screen.getByTestId('count').textContent).toMatch(/2 notes/);
	});

	it('shows the truncated notice when the graph exceeded the cap', async () => {
		const fixture = {
			nodes: [{ id: '01HABC0000000000000000000A', title: 'A', size: 6, x: 0, y: 0 }],
			edges: [],
			truncated: true
		};
		const { t } = transportReturning(fixture);
		setIpcTransport(t);
		render(GraphPage);
		await waitFor(() =>
			expect(screen.getByTestId('truncated-notice')).toBeInTheDocument()
		);
		expect(screen.getByTestId('truncated-notice').textContent).toMatch(
			/graph truncated/i
		);
	});

	it('shows the error message when graph_data rejects', async () => {
		const t = async <_T>(): Promise<never> => {
			throw { code: 'io', message: 'database locked' };
		};
		setIpcTransport(t);
		render(GraphPage);
		await waitFor(() => expect(screen.getByTestId('error')).toBeInTheDocument());
		expect(screen.getByTestId('error').textContent).toContain('database locked');
	});
});
