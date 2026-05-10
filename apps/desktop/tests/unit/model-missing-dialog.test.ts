// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import ModelMissingDialog from '$lib/components/ModelMissingDialog.svelte';

describe('ModelMissingDialog', () => {
	it('renders the expected path in the body', () => {
		render(ModelMissingDialog, {
			props: { expectedPath: '/home/u/.local/share/plainnote/models/ggml-base.en.bin' }
		});
		expect(screen.getAllByText(/ggml-base\.en\.bin/).length).toBeGreaterThan(0);
		expect(
			screen.getByText('/home/u/.local/share/plainnote/models/ggml-base.en.bin')
		).toBeInTheDocument();
	});

	it('exposes Copy path and Copy URL buttons', () => {
		render(ModelMissingDialog, {
			props: { expectedPath: '/x/ggml-base.en.bin' }
		});
		expect(screen.getByTestId('model-missing-copy-path')).toBeInTheDocument();
		expect(screen.getByTestId('model-missing-copy-url')).toBeInTheDocument();
	});

	it('Cancel calls onCancel; Retry calls onRetry', async () => {
		let cancelled = false;
		let retried = false;
		render(ModelMissingDialog, {
			props: {
				expectedPath: '/x/ggml-base.en.bin',
				onCancel: () => {
					cancelled = true;
				},
				onRetry: () => {
					retried = true;
				}
			}
		});
		const user = userEvent.setup();
		await user.click(screen.getByTestId('model-missing-cancel'));
		expect(cancelled).toBe(true);
		expect(retried).toBe(false);
	});

	it('Retry button click invokes onRetry', async () => {
		let retried = false;
		render(ModelMissingDialog, {
			props: {
				expectedPath: '/x/ggml-base.en.bin',
				onRetry: () => {
					retried = true;
				}
			}
		});
		const user = userEvent.setup();
		await user.click(screen.getByTestId('model-missing-retry'));
		expect(retried).toBe(true);
	});

	it('uses role=dialog and aria-modal=true', () => {
		render(ModelMissingDialog, {
			props: { expectedPath: '/x/ggml-base.en.bin' }
		});
		const dlg = screen.getByTestId('model-missing-dialog');
		expect(dlg.getAttribute('role')).toBe('dialog');
		expect(dlg.getAttribute('aria-modal')).toBe('true');
	});

	it('quick-command block contains the resolved path', () => {
		render(ModelMissingDialog, {
			props: { expectedPath: '/home/u/foo/bar.bin' }
		});
		const code = screen.getByTestId('model-missing-quick-command');
		expect(code.textContent).toContain('/home/u/foo/bar.bin');
		expect(code.textContent).toMatch(/curl.+huggingface/);
		expect(code.textContent).toContain('sha1sum');
	});
});
