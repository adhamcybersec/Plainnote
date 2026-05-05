// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Focus route — loads the note, toggles render mode, persists preference,
 * shows backlinks. Uses the IPC transport mock so this test never touches
 * Tauri or a real SQLite database.
 */
import { render, screen, fireEvent } from '@testing-library/svelte';
import { setIpcTransport } from '$lib/ipc';
import FocusPage from '../../src/routes/note/[id]/+page.svelte';

const NOTE_ID = '01HXYZ0000000000000000000A';
const SOURCE_ID = '01HABC0000000000000000000A';

// SvelteKit's `$app/state` page store is not present in jsdom; stub it.
// Factory must be self-contained because vi.mock is hoisted above the file.
vi.mock('$app/state', () => ({
	page: { params: { id: '01HXYZ0000000000000000000A' } }
}));

interface InvokeCall {
	cmd: string;
	args?: Record<string, unknown>;
}

function makeTransport(handlers: Record<string, (args: unknown) => unknown>) {
	const calls: InvokeCall[] = [];
	const t = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
		calls.push({ cmd, args });
		const h = handlers[cmd];
		if (!h) throw new Error(`unexpected cmd: ${cmd}`);
		return h(args) as T;
	};
	return { t, calls };
}

const baseNote = {
	id: NOTE_ID,
	title: 'Calculus',
	created: '2026-05-04T10:23:11Z',
	updated: '2026-05-04T10:23:11Z',
	tags: [],
	body: '# Hello\n\nThis is **bold**.'
};

describe('Focus route', () => {
	it('loads the note and shows the title', async () => {
		const { t } = makeTransport({
			read_note: () => baseNote,
			backlinks_for: () => [],
			get_meta: () => null
		});
		setIpcTransport(t);
		render(FocusPage);
		await screen.findByTestId('title');
		expect(screen.getByTestId('title').textContent).toContain('Calculus');
	});

	it('renders markdown by default (rendered mode)', async () => {
		const { t } = makeTransport({
			read_note: () => baseNote,
			backlinks_for: () => [],
			get_meta: () => null
		});
		setIpcTransport(t);
		render(FocusPage);
		const rendered = await screen.findByTestId('rendered');
		// `# Hello` should become an h1 element through the sanitized pipeline.
		expect(rendered.querySelector('h1')?.textContent).toBe('Hello');
		expect(rendered.querySelector('strong')?.textContent).toBe('bold');
	});

	it('respects the persisted source preference', async () => {
		const { t } = makeTransport({
			read_note: () => baseNote,
			backlinks_for: () => [],
			get_meta: () => 'source'
		});
		setIpcTransport(t);
		render(FocusPage);
		const body = await screen.findByTestId('body');
		// data-render-mode reflects the active mode and is asserted instead of
		// querying CodeMirror internals (which jsdom struggles with).
		expect(body.getAttribute('data-render-mode')).toBe('source');
	});

	it('toggling the render-mode button persists via setMeta', async () => {
		const recorded: InvokeCall[] = [];
		const { t, calls } = makeTransport({
			read_note: () => baseNote,
			backlinks_for: () => [],
			get_meta: () => null,
			set_meta: (args) => {
				recorded.push({ cmd: 'set_meta', args: args as Record<string, unknown> });
				return null;
			}
		});
		setIpcTransport(t);
		render(FocusPage);
		const btn = await screen.findByTestId('toggle-render-mode');
		await fireEvent.click(btn);
		// The first set_meta call carries the new mode (now 'source').
		const setCalls = calls.filter((c) => c.cmd === 'set_meta');
		expect(setCalls.length).toBeGreaterThan(0);
		expect(setCalls[0].args).toEqual({
			key: 'editor.render_mode',
			value: 'source'
		});
	});

	it('shows the empty-state copy when there are no backlinks', async () => {
		const { t } = makeTransport({
			read_note: () => baseNote,
			backlinks_for: () => [],
			get_meta: () => null
		});
		setIpcTransport(t);
		render(FocusPage);
		const empty = await screen.findByTestId('backlinks-empty');
		expect(empty.textContent).toMatch(/no notes link here yet/i);
	});

	it('renders backlinks with source title + preview', async () => {
		const { t } = makeTransport({
			read_note: () => baseNote,
			backlinks_for: () => [
				{
					source_id: SOURCE_ID,
					source_title: 'Source Note',
					source_preview: 'mentions calculus',
					raw: '[[Calculus]]'
				}
			],
			get_meta: () => null
		});
		setIpcTransport(t);
		render(FocusPage);
		const link = await screen.findByTestId('backlink');
		expect(link.getAttribute('href')).toBe(`/note/${SOURCE_ID}`);
		expect(link.textContent).toContain('Source Note');
		expect(link.textContent).toContain('mentions calculus');
	});
});
