// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Settings page — covers the reminders section + meta-key persistence.
 */
import { render, screen, waitFor, fireEvent } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { setIpcTransport, type Reminder } from '$lib/ipc';
import SettingsPage from '../../src/routes/settings/+page.svelte';

interface InvokeCall {
	cmd: string;
	args?: Record<string, unknown>;
}

function makeTransport(handlers: Record<string, (args?: Record<string, unknown>) => unknown>) {
	const calls: InvokeCall[] = [];
	// Settings page calls vault_info, list_reminders × 2 filters, and the
	// appearance loader's get_meta on mount. Tests provide explicit handlers
	// for what they care about; we add stub defaults for the rest so a
	// single test doesn't have to re-declare the universe.
	const merged: typeof handlers = {
		vault_info: () => ({ path: '/tmp/vault', note_count: 0 }),
		list_reminders: () => [],
		get_meta: () => null,
		set_meta: () => null,
		...handlers
	};
	const t = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
		calls.push({ cmd, args });
		const h = merged[cmd];
		if (!h) throw new Error(`unexpected cmd: ${cmd}`);
		return h(args) as T;
	};
	return { t, calls };
}

const ACTIVE_FIXTURE: Reminder[] = [
	{
		id: '01HABC0000000000000000000A',
		note_id: null,
		fire_at: '2026-06-01T10:00:00Z',
		fired_at: null,
		cancelled_at: null,
		body: 'drink water'
	}
];

const FIRED_FIXTURE: Reminder[] = [
	{
		id: '01HABC0000000000000000000B',
		note_id: null,
		fire_at: '2026-05-04T10:00:00Z',
		fired_at: '2026-05-04T10:00:01Z',
		cancelled_at: null,
		body: 'old reminder'
	}
];

describe('Settings page', () => {
	it('renders the reminders section with active and fired lists', async () => {
		const { t } = makeTransport({
			list_reminders: (args) => {
				if ((args as { filter: string }).filter === 'active') return ACTIVE_FIXTURE;
				if ((args as { filter: string }).filter === 'fired') return FIRED_FIXTURE;
				return [];
			},
			get_meta: () => null
		});
		setIpcTransport(t);
		render(SettingsPage);
		await waitFor(() => expect(screen.queryByTestId('loading')).toBeNull());
		expect(screen.getByTestId('active-item').textContent).toContain('drink water');
		expect(screen.getByTestId('fired-item').textContent).toContain('old reminder');
	});

	it('shows empty-state copy when no reminders exist', async () => {
		const { t } = makeTransport({
			list_reminders: () => [],
			get_meta: () => null
		});
		setIpcTransport(t);
		render(SettingsPage);
		await waitFor(() =>
			expect(screen.getByTestId('active-empty')).toBeInTheDocument()
		);
		expect(screen.getByTestId('fired-empty')).toBeInTheDocument();
	});

	it('cancels an active reminder via cancel_reminder + refreshes', async () => {
		// First list call returns the fixture; after cancel, an empty list.
		let listCallCount = 0;
		const { t, calls } = makeTransport({
			list_reminders: (args) => {
				const filter = (args as { filter: string }).filter;
				listCallCount += 1;
				if (filter === 'active') {
					return listCallCount <= 2 ? ACTIVE_FIXTURE : [];
				}
				return [];
			},
			get_meta: () => null,
			cancel_reminder: () => null
		});
		setIpcTransport(t);
		render(SettingsPage);
		const cancelBtn = await screen.findByTestId('cancel-reminder');
		await fireEvent.click(cancelBtn);
		await waitFor(() =>
			expect(screen.getByTestId('active-empty')).toBeInTheDocument()
		);
		const cancelCall = calls.find((c) => c.cmd === 'cancel_reminder');
		expect(cancelCall?.args).toEqual({ id: '01HABC0000000000000000000A' });
	});

	it('persists default lead time via setMeta when changed', async () => {
		const { t, calls } = makeTransport({
			list_reminders: () => [],
			get_meta: () => null,
			set_meta: () => null
		});
		setIpcTransport(t);
		render(SettingsPage);
		const select = await screen.findByTestId('default-lead-select');
		await fireEvent.change(select, { target: { value: 'tomorrow_9am' } });
		await waitFor(() => {
			const setCall = calls.find(
				(c) =>
					c.cmd === 'set_meta' &&
					(c.args as { key: string }).key === 'reminders.default_lead_time'
			);
			expect(setCall?.args).toEqual({
				key: 'reminders.default_lead_time',
				value: 'tomorrow_9am'
			});
		});
	});

	it('persists sound-enabled toggle via setMeta', async () => {
		const { t, calls } = makeTransport({
			list_reminders: () => [],
			get_meta: () => null,
			set_meta: () => null
		});
		setIpcTransport(t);
		render(SettingsPage);
		const toggle = await screen.findByTestId('sound-toggle');
		await fireEvent.click(toggle);
		await waitFor(() => {
			const setCall = calls.find(
				(c) =>
					c.cmd === 'set_meta' &&
					(c.args as { key: string }).key === 'reminders.sound_enabled'
			);
			expect(setCall?.args).toEqual({
				key: 'reminders.sound_enabled',
				value: 'true'
			});
		});
	});

	it('hydrates the default-lead select from a stored meta value', async () => {
		const { t } = makeTransport({
			list_reminders: () => [],
			get_meta: (args) => {
				const key = (args as { key: string }).key;
				if (key === 'reminders.default_lead_time') return 'tomorrow_9am';
				return null;
			}
		});
		setIpcTransport(t);
		render(SettingsPage);
		await waitFor(() => {
			const select = screen.getByTestId('default-lead-select') as HTMLSelectElement;
			expect(select.value).toBe('tomorrow_9am');
		});
	});

	// ─── Appearance section (M9-T1) ────────────────────────────────────────

	it('persists theme change via setMeta and applies to <html>', async () => {
		const user = userEvent.setup();
		const { t, calls } = makeTransport({});
		setIpcTransport(t);
		render(SettingsPage);
		const themeSelect = await screen.findByTestId('theme-select');
		// userEvent.selectOptions drives the <select> the way a user does:
		// it dispatches input + change events and works correctly with
		// Svelte's reactive value={...} binding (fireEvent.change alone
		// can race Svelte's re-assertion of the bound value).
		await user.selectOptions(themeSelect, 'dark');
		await waitFor(() => {
			const setCall = calls.find(
				(c) =>
					c.cmd === 'set_meta' &&
					(c.args as { key: string }).key === 'appearance.theme'
			);
			expect(setCall?.args).toEqual({ key: 'appearance.theme', value: 'dark' });
			expect(document.documentElement.dataset.theme).toBe('dark');
		});
	});

	it('renders 6 accent swatches and persists the selected one', async () => {
		const { t, calls } = makeTransport({});
		setIpcTransport(t);
		render(SettingsPage);
		await waitFor(() => expect(screen.getByTestId('accent-grid')).toBeInTheDocument());
		// All six swatches are present.
		for (const id of ['sage', 'teal', 'ochre', 'clay', 'ink', 'mauve']) {
			expect(screen.getByTestId(`accent-${id}`)).toBeInTheDocument();
		}
		await fireEvent.click(screen.getByTestId('accent-mauve'));
		await waitFor(() => {
			const setCall = calls.find(
				(c) =>
					c.cmd === 'set_meta' &&
					(c.args as { key: string }).key === 'appearance.accent'
			);
			expect(setCall?.args).toEqual({ key: 'appearance.accent', value: 'mauve' });
		});
	});

	it('persists reduce-motion toggle via setMeta', async () => {
		const { t, calls } = makeTransport({});
		setIpcTransport(t);
		render(SettingsPage);
		const toggle = await screen.findByTestId('reduce-motion-toggle');
		await fireEvent.click(toggle);
		await waitFor(() => {
			const setCall = calls.find(
				(c) =>
					c.cmd === 'set_meta' &&
					(c.args as { key: string }).key === 'appearance.reduce_motion'
			);
			expect(setCall?.args).toEqual({
				key: 'appearance.reduce_motion',
				value: 'true'
			});
		});
	});

	// ─── Vault section (M9-T1) ─────────────────────────────────────────────

	it('renders the vault path and note count from vault_info', async () => {
		const { t } = makeTransport({
			vault_info: () => ({ path: '/home/x/Plainnote/vault', note_count: 42 })
		});
		setIpcTransport(t);
		render(SettingsPage);
		await waitFor(() => {
			expect(screen.getByTestId('vault-path').textContent).toBe(
				'/home/x/Plainnote/vault'
			);
			expect(screen.getByTestId('vault-count').textContent).toBe('42');
		});
	});

	it('reindex button calls reindex_vault and shows the summary', async () => {
		const { t } = makeTransport({
			reindex_vault: () => ({ inserted: 3, updated: 1, deleted: 0 })
		});
		setIpcTransport(t);
		render(SettingsPage);
		const btn = await screen.findByTestId('reindex-btn');
		await fireEvent.click(btn);
		await waitFor(() => {
			const summary = screen.getByTestId('reindex-summary');
			expect(summary.textContent).toMatch(/3 added/);
			expect(summary.textContent).toMatch(/1 updated/);
		});
	});
});
