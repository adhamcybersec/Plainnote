// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { setIpcTransport } from '$lib/ipc';
import Capture from '../../src/routes/+page.svelte';

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

describe('Capture screen', () => {
	it('renders a textarea, save button, and disabled record/attach', () => {
		setIpcTransport(async <T>() => '' as unknown as T); // unused
		render(Capture);
		expect(screen.getByLabelText('Capture a note')).toBeInTheDocument();
		expect(screen.getByTestId('save-button')).toBeDisabled(); // empty textarea
		const record = screen.getByText(/Record voice/);
		const attach = screen.getByText(/Attach file/);
		expect(record.closest('button')).toBeDisabled();
		expect(attach.closest('button')).toBeDisabled();
	});

	it('typing enables Save and clicking it calls save_note', async () => {
		const { t, calls } = makeTransport({
			save_note: () => '01HXYZ0000000000000000000A'
		});
		setIpcTransport(t);
		const user = userEvent.setup();
		render(Capture);

		const textarea = screen.getByLabelText('Capture a note');
		await user.type(textarea, 'first thought');

		const save = screen.getByTestId('save-button');
		expect(save).not.toBeDisabled();
		await user.click(save);

		expect(calls).toEqual([
			{ cmd: 'save_note', args: { body: 'first thought', title: null } }
		]);
	});

	it('clears the textarea after a successful save', async () => {
		const { t } = makeTransport({ save_note: () => '01HABC0000000000000000000A' });
		setIpcTransport(t);
		const user = userEvent.setup();
		render(Capture);

		const textarea = screen.getByLabelText('Capture a note') as HTMLTextAreaElement;
		await user.type(textarea, 'a thought');
		await user.click(screen.getByTestId('save-button'));

		await waitFor(() => expect(textarea.value).toBe(''));
	});

	it('shows the Saved chip after save', async () => {
		const { t } = makeTransport({ save_note: () => '01HABC0000000000000000000A' });
		setIpcTransport(t);
		const user = userEvent.setup();
		render(Capture);

		await user.type(screen.getByLabelText('Capture a note'), 'a thought');
		await user.click(screen.getByTestId('save-button'));

		await waitFor(() => expect(screen.getByTestId('saved-chip')).toBeInTheDocument());
	});

	it('Ctrl+Enter saves and clears', async () => {
		const { t, calls } = makeTransport({ save_note: () => '01HABC0000000000000000000A' });
		setIpcTransport(t);
		const user = userEvent.setup();
		render(Capture);

		const textarea = screen.getByLabelText('Capture a note') as HTMLTextAreaElement;
		await user.type(textarea, 'shortcut save');
		await user.keyboard('{Control>}{Enter}{/Control}');

		expect(calls[0]).toEqual({
			cmd: 'save_note',
			args: { body: 'shortcut save', title: null }
		});
		await waitFor(() => expect(textarea.value).toBe(''));
	});

	it('whitespace-only input does not call save', async () => {
		const { t, calls } = makeTransport({ save_note: () => 'unused' });
		setIpcTransport(t);
		const user = userEvent.setup();
		render(Capture);

		await user.type(screen.getByLabelText('Capture a note'), '   \n  ');
		// The button stays disabled because of trim()===''. Verify by invoking
		// the keyboard shortcut; it must short-circuit.
		await user.keyboard('{Control>}{Enter}{/Control}');
		expect(calls).toEqual([]);
	});

	it('shows an error chip when save_note rejects with an IpcError', async () => {
		const { t } = makeTransport({
			save_note: () => {
				throw { code: 'io', message: 'disk full' };
			}
		});
		setIpcTransport(t);
		const user = userEvent.setup();
		render(Capture);

		await user.type(screen.getByLabelText('Capture a note'), 'a thought');
		await user.click(screen.getByTestId('save-button'));

		await waitFor(() => expect(screen.getByTestId('error-chip')).toBeInTheDocument());
		expect(screen.getByTestId('error-chip').textContent).toContain('disk full');
	});
});
