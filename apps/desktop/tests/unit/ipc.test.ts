// SPDX-License-Identifier: AGPL-3.0-or-later
import {
	ping,
	saveNote,
	listNotes,
	readNote,
	listTags,
	queryNotes,
	setTags,
	searchNotesByTitle,
	getMeta,
	setMeta,
	outboundLinksOf,
	backlinksFor,
	setIpcTransport,
	DEFAULT_QUERY_MODE
} from '$lib/ipc';
import type { NoteSummary, Note, TagRow, LinkRef, Backlink, TitleHit } from '$lib/ipc';

interface InvokeCall {
	cmd: string;
	args?: Record<string, unknown>;
}

function mockTransport(responses: Record<string, unknown>) {
	const calls: InvokeCall[] = [];
	const t = async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
		calls.push({ cmd, args });
		if (!(cmd in responses)) throw new Error(`unexpected cmd: ${cmd}`);
		return responses[cmd] as T;
	};
	return { t, calls };
}

describe('ipc layer', () => {
	it('ping forwards to the ping command', async () => {
		const { t, calls } = mockTransport({ ping: 'pong' });
		setIpcTransport(t);
		const result = await ping();
		expect(result).toBe('pong');
		expect(calls).toEqual([{ cmd: 'ping', args: undefined }]);
	});

	it('saveNote passes body and title; returns the id', async () => {
		const { t, calls } = mockTransport({
			save_note: '01HXYZ0000000000000000000A'
		});
		setIpcTransport(t);
		const id = await saveNote('hello body', 'a title');
		expect(id).toBe('01HXYZ0000000000000000000A');
		expect(calls[0]).toEqual({
			cmd: 'save_note',
			args: { body: 'hello body', title: 'a title' }
		});
	});

	it('saveNote sends null when title is omitted', async () => {
		// Critical: the Rust side accepts Option<String>; passing undefined
		// would serialize to nothing and be parsed as a missing key.
		const { t, calls } = mockTransport({
			save_note: '01HABC0000000000000000000A'
		});
		setIpcTransport(t);
		await saveNote('only body');
		expect(calls[0].args).toEqual({ body: 'only body', title: null });
	});

	it('listNotes returns a typed array of summaries', async () => {
		const fixture: NoteSummary[] = [
			{
				id: '01HXYZ0000000000000000000A',
				title: 'one',
				created: '2026-05-04T10:23:11Z',
				updated: '2026-05-04T10:23:11Z',
				preview: 'first line'
			}
		];
		const { t, calls } = mockTransport({ list_notes: fixture });
		setIpcTransport(t);
		const result = await listNotes(50);
		expect(result).toEqual(fixture);
		expect(calls[0]).toEqual({ cmd: 'list_notes', args: { limit: 50 } });
	});

	it('readNote returns a typed Note', async () => {
		const fixture: Note = {
			id: '01HXYZ0000000000000000000A',
			title: null,
			created: '2026-05-04T10:23:11Z',
			updated: '2026-05-04T10:23:11Z',
			tags: ['learning/math'],
			body: 'hello'
		};
		const { t, calls } = mockTransport({ read_note: fixture });
		setIpcTransport(t);
		const result = await readNote('01HXYZ0000000000000000000A');
		expect(result).toEqual(fixture);
		expect(calls[0]).toEqual({
			cmd: 'read_note',
			args: { id: '01HXYZ0000000000000000000A' }
		});
	});

	it('listTags forwards to list_tags and returns typed rows', async () => {
		const fixture: TagRow[] = [
			{ path: 'learning', parent: null, note_count: 0 },
			{ path: 'learning/math', parent: 'learning', note_count: 2 }
		];
		const { t, calls } = mockTransport({ list_tags: fixture });
		setIpcTransport(t);
		const result = await listTags();
		expect(result).toEqual(fixture);
		expect(calls[0]).toEqual({ cmd: 'list_tags', args: undefined });
	});

	it('queryNotes sends tags and the default mode when mode is omitted', async () => {
		const fixture: NoteSummary[] = [];
		const { t, calls } = mockTransport({ query_notes: fixture });
		setIpcTransport(t);
		await queryNotes(['learning/math', 'work']);
		expect(calls[0]).toEqual({
			cmd: 'query_notes',
			args: { tags: ['learning/math', 'work'], mode: DEFAULT_QUERY_MODE }
		});
		expect(DEFAULT_QUERY_MODE).toBe('recursive_intersection');
	});

	it('queryNotes accepts each of the four modes', async () => {
		const { t, calls } = mockTransport({ query_notes: [] });
		setIpcTransport(t);
		const modes = [
			'strict_intersection',
			'recursive_intersection',
			'strict_union',
			'recursive_union'
		] as const;
		for (const m of modes) {
			await queryNotes(['x'], m);
		}
		expect(calls.map((c) => (c.args as { mode: string }).mode)).toEqual([...modes]);
	});

	it('setTags forwards id and tags', async () => {
		const { t, calls } = mockTransport({ set_tags: null });
		setIpcTransport(t);
		await setTags('01HXYZ0000000000000000000A', ['a', 'b']);
		expect(calls[0]).toEqual({
			cmd: 'set_tags',
			args: { id: '01HXYZ0000000000000000000A', tags: ['a', 'b'] }
		});
	});

	it('searchNotesByTitle forwards prefix + default limit and returns typed TitleHit[]', async () => {
		const fixture: TitleHit[] = [
			{ id: '01HABC0000000000000000000A', title: 'Calculus' },
			{ id: '01HABC0000000000000000000B', title: 'calculus II' }
		];
		const { t, calls } = mockTransport({ search_notes_by_title: fixture });
		setIpcTransport(t);
		const result = await searchNotesByTitle('cal');
		expect(result).toEqual(fixture);
		expect(calls[0]).toEqual({
			cmd: 'search_notes_by_title',
			args: { prefix: 'cal', limit: 8 }
		});
	});

	it('searchNotesByTitle accepts a custom limit', async () => {
		const { t, calls } = mockTransport({ search_notes_by_title: [] });
		setIpcTransport(t);
		await searchNotesByTitle('q', 3);
		expect(calls[0].args).toEqual({ prefix: 'q', limit: 3 });
	});

	it('getMeta forwards the key and returns string|null', async () => {
		const { t, calls } = mockTransport({ get_meta: 'rendered' });
		setIpcTransport(t);
		const v = await getMeta('editor.render_mode');
		expect(v).toBe('rendered');
		expect(calls[0]).toEqual({
			cmd: 'get_meta',
			args: { key: 'editor.render_mode' }
		});
	});

	it('getMeta returns null for unknown keys', async () => {
		const { t } = mockTransport({ get_meta: null });
		setIpcTransport(t);
		expect(await getMeta('nope')).toBeNull();
	});

	it('setMeta forwards key and value', async () => {
		const { t, calls } = mockTransport({ set_meta: null });
		setIpcTransport(t);
		await setMeta('editor.render_mode', 'source');
		expect(calls[0]).toEqual({
			cmd: 'set_meta',
			args: { key: 'editor.render_mode', value: 'source' }
		});
	});

	it('outboundLinksOf forwards the id and returns typed LinkRef[]', async () => {
		const fixture: LinkRef[] = [
			{
				raw: '[[Target]]',
				target_text: 'Target',
				alias: null,
				target_id: '01HABC0000000000000000000A'
			},
			{
				raw: '[[Nope]]',
				target_text: 'Nope',
				alias: null,
				target_id: null
			}
		];
		const { t, calls } = mockTransport({ outbound_links_of: fixture });
		setIpcTransport(t);
		const result = await outboundLinksOf('01HXYZ0000000000000000000A');
		expect(result).toEqual(fixture);
		expect(calls[0]).toEqual({
			cmd: 'outbound_links_of',
			args: { id: '01HXYZ0000000000000000000A' }
		});
	});

	it('backlinksFor forwards the id and returns typed Backlink[]', async () => {
		const fixture: Backlink[] = [
			{
				source_id: '01HABC0000000000000000000A',
				source_title: 'Source',
				source_preview: 'preview',
				raw: '[[Target]]'
			}
		];
		const { t, calls } = mockTransport({ backlinks_for: fixture });
		setIpcTransport(t);
		const result = await backlinksFor('01HXYZ0000000000000000000A');
		expect(result).toEqual(fixture);
		expect(calls[0]).toEqual({
			cmd: 'backlinks_for',
			args: { id: '01HXYZ0000000000000000000A' }
		});
	});

	it('rejects when the backend errors with a structured IpcError', async () => {
		// Tauri rejects an invoke promise with the Rust-side serialized error.
		// We confirm the wrapper preserves it instead of swallowing.
		const t = async <_T>(): Promise<never> => {
			throw { code: 'not_found', message: 'no such note' };
		};
		setIpcTransport(t);
		await expect(readNote('01HXYZ0000000000000000000A')).rejects.toMatchObject({
			code: 'not_found',
			message: 'no such note'
		});
	});
});
