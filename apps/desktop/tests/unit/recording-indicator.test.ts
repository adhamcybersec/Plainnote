// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import RecordingIndicator from '$lib/components/RecordingIndicator.svelte';
import type { RecordingState } from '$lib/ipc';

describe('RecordingIndicator', () => {
	it('renders nothing in idle state', () => {
		const { container } = render(RecordingIndicator, {
			props: { state: { kind: 'idle' } as RecordingState }
		});
		expect(container.querySelector('[data-testid="recording-indicator"]')).toBeNull();
	});

	it('shows red dot, timer, and stop button while recording', () => {
		const startedAtMs = Date.now() - 12_000;
		render(RecordingIndicator, {
			props: { state: { kind: 'recording', started_at_ms: startedAtMs } as RecordingState }
		});
		expect(screen.getByTestId('recording-indicator').dataset.state).toBe('recording');
		expect(screen.getByTestId('recording-dot')).toBeInTheDocument();
		const timer = screen.getByTestId('recording-timer');
		expect(timer.textContent).toMatch(/^\d{2}:\d{2}$/);
		expect(screen.getByTestId('recording-stop')).toBeInTheDocument();
	});

	it('formats timer as mm:ss based on elapsed since started_at_ms', () => {
		const startedAtMs = Date.now() - 65_000; // ~1:05 ago
		render(RecordingIndicator, {
			props: { state: { kind: 'recording', started_at_ms: startedAtMs } as RecordingState }
		});
		// Allow ±1s drift for test scheduling.
		expect(screen.getByTestId('recording-timer').textContent).toMatch(/^01:0[4-6]$/);
	});

	it('calls onStop when stop button is clicked', async () => {
		const user = userEvent.setup();
		let stopped = false;
		render(RecordingIndicator, {
			props: {
				state: { kind: 'recording', started_at_ms: Date.now() } as RecordingState,
				onStop: () => {
					stopped = true;
				}
			}
		});
		await user.click(screen.getByTestId('recording-stop'));
		expect(stopped).toBe(true);
	});

	it('shows spinner and message in transcribing state', () => {
		render(RecordingIndicator, {
			props: { state: { kind: 'transcribing' } as RecordingState }
		});
		expect(screen.getByTestId('recording-indicator').dataset.state).toBe('transcribing');
		expect(screen.getByTestId('transcribing-spinner')).toBeInTheDocument();
		expect(screen.getByText(/Transcribing/i)).toBeInTheDocument();
	});

	it('shows error message and dismiss button in error state', () => {
		render(RecordingIndicator, {
			props: {
				state: { kind: 'error', message: 'Model file not found' } as RecordingState
			}
		});
		expect(screen.getByTestId('recording-indicator').dataset.state).toBe('error');
		expect(screen.getByTestId('recording-error-message').textContent).toContain(
			'Model file not found'
		);
		expect(screen.getByTestId('recording-dismiss')).toBeInTheDocument();
	});

	it('calls onDismiss when dismiss button is clicked', async () => {
		const user = userEvent.setup();
		let dismissed = false;
		render(RecordingIndicator, {
			props: {
				state: { kind: 'error', message: 'oops' } as RecordingState,
				onDismiss: () => {
					dismissed = true;
				}
			}
		});
		await user.click(screen.getByTestId('recording-dismiss'));
		expect(dismissed).toBe(true);
	});

	it('uses role="status" and aria-live=polite while recording or transcribing', () => {
		const { rerender } = render(RecordingIndicator, {
			props: { state: { kind: 'recording', started_at_ms: Date.now() } as RecordingState }
		});
		let indicator = screen.getByTestId('recording-indicator');
		expect(indicator.getAttribute('role')).toBe('status');
		expect(indicator.getAttribute('aria-live')).toBe('polite');

		rerender({ state: { kind: 'transcribing' } as RecordingState });
		indicator = screen.getByTestId('recording-indicator');
		expect(indicator.getAttribute('role')).toBe('status');
		expect(indicator.getAttribute('aria-live')).toBe('polite');
	});

	it('uses role="alert" in error state', () => {
		render(RecordingIndicator, {
			props: { state: { kind: 'error', message: 'x' } as RecordingState }
		});
		expect(screen.getByTestId('recording-indicator').getAttribute('role')).toBe('alert');
	});
});
