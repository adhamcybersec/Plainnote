// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Typed wrappers around Tauri IPC.
 *
 * Hard rule: this is the ONLY module in the frontend that imports from
 * `@tauri-apps/api`. Everything else uses the typed functions exported
 * here, which makes IPC mockable in Vitest without monkey-patching globals.
 *
 * Wire types mirror the Rust `*V1` structs in `src-tauri/src/commands.rs`.
 * Bumping a struct version means a new function (e.g. `saveNoteV2`) and a
 * deprecation note here, not silently changing the existing shape.
 */

import { invoke } from '@tauri-apps/api/core';

// ─── Wire types ────────────────────────────────────────────────────────────

export type NoteId = string;

export interface NoteSummary {
	id: NoteId;
	title: string | null;
	created: string;
	updated: string;
	preview: string;
}

export interface Note {
	id: NoteId;
	title: string | null;
	created: string;
	updated: string;
	tags: string[];
	body: string;
}

export type IpcErrorCode =
	| 'io'
	| 'invalid'
	| 'not_found'
	| 'locked'
	| 'model_missing'
	| 'no_input_device'
	| 'audio'
	| 'stt';

export interface IpcError {
	code: IpcErrorCode;
	message: string;
}

// ─── Pluggable transport ───────────────────────────────────────────────────
// Vitest tests swap this for a mock. Outside of tests the production import
// of `invoke` from `@tauri-apps/api/core` is used.

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

let transport: InvokeFn = invoke as InvokeFn;

/** Test-only: swap the transport. Restore by passing the original `invoke`. */
export function setIpcTransport(t: InvokeFn): void {
	transport = t;
}

// ─── Commands ──────────────────────────────────────────────────────────────

export function ping(): Promise<string> {
	return transport<string>('ping');
}

export function saveNote(body: string, title: string | null = null): Promise<NoteId> {
	return transport<NoteId>('save_note', { body, title });
}

export function listNotes(limit: number): Promise<NoteSummary[]> {
	return transport<NoteSummary[]>('list_notes', { limit });
}

export function readNote(id: NoteId): Promise<Note> {
	return transport<Note>('read_note', { id });
}

// ─── Tag system (M2) ───────────────────────────────────────────────────────

export type QueryMode =
	| 'strict_intersection'
	| 'recursive_intersection'
	| 'strict_union'
	| 'recursive_union';

/** SPEC §5 default. */
export const DEFAULT_QUERY_MODE: QueryMode = 'recursive_intersection';

export interface TagRow {
	path: string;
	parent: string | null;
	note_count: number;
}

/** Flat list of every tag in the index. Frontend builds the tree by joining on `parent`. */
export function listTags(): Promise<TagRow[]> {
	return transport<TagRow[]>('list_tags');
}

/** Run a four-mode tag query. `mode` defaults to RecursiveIntersection on the Rust side. */
export function queryNotes(
	tags: string[],
	mode: QueryMode = DEFAULT_QUERY_MODE
): Promise<NoteSummary[]> {
	return transport<NoteSummary[]>('query_notes', { tags, mode });
}

/** Replace the tag set on a note. */
export function setTags(id: NoteId, tags: string[]): Promise<void> {
	return transport<void>('set_tags', { id, tags });
}

// ─── Vault management (M9) ─────────────────────────────────────────────────

export interface VaultInfo {
	path: string;
	note_count: number;
}

export interface ReindexSummary {
	inserted: number;
	updated: number;
	deleted: number;
}

/** Path + note count for the Settings page. */
export function vaultInfo(): Promise<VaultInfo> {
	return transport<VaultInfo>('vault_info');
}

/** Force a full vault re-walk. Useful when external changes look out of sync. */
export function reindexVault(): Promise<ReindexSummary> {
	return transport<ReindexSummary>('reindex_vault');
}

// ─── App-wide preferences (M3) ─────────────────────────────────────────────

/** Read a string preference from the persistent meta table. Null = unset. */
export function getMeta(key: string): Promise<string | null> {
	return transport<string | null>('get_meta', { key });
}

/** Persist a string preference. `schema_version` is reserved (rejected). */
export function setMeta(key: string, value: string): Promise<void> {
	return transport<void>('set_meta', { key, value });
}

// ─── Reminders (M6) ────────────────────────────────────────────────────────

export type ReminderId = string;

export interface Reminder {
	id: ReminderId;
	note_id: NoteId | null;
	fire_at: string;
	fired_at: string | null;
	cancelled_at: string | null;
	body: string;
}

export type ReminderFilter = 'active' | 'fired' | 'cancelled' | 'all';

/** Schedule a reminder. fire_at must be ISO-8601 (any offset; backend canonicalizes to Z). */
export function setReminder(
	fireAt: string,
	body: string,
	noteId: NoteId | null = null
): Promise<ReminderId> {
	// Tauri 2 auto-converts camelCase JS keys to snake_case Rust args.
	return transport<ReminderId>('set_reminder', { noteId, fireAt, body });
}

/** Mark a reminder cancelled. Errors with code='not_found' if already cancelled/fired. */
export function cancelReminder(id: ReminderId): Promise<void> {
	return transport<void>('cancel_reminder', { id });
}

/** List reminders. Default filter = 'active'. */
export function listReminders(filter: ReminderFilter = 'active'): Promise<Reminder[]> {
	return transport<Reminder[]>('list_reminders', { filter });
}

// ─── Graph view (M5) ───────────────────────────────────────────────────────

export interface GraphNode {
	id: NoteId;
	title: string;
	size: number;
	x: number;
	y: number;
}

export interface GraphEdge {
	source: NoteId;
	target: NoteId;
}

export interface GraphData {
	nodes: GraphNode[];
	edges: GraphEdge[];
	/** True when the vault exceeded the 5k-node cap. */
	truncated: boolean;
}

/**
 * Build the graph payload (nodes + edges + force-layout coordinates).
 * The layout is deterministic given the same vault contents.
 */
export function graphData(): Promise<GraphData> {
	return transport<GraphData>('graph_data');
}

// ─── Wikilink autocomplete (M3) ────────────────────────────────────────────

export interface TitleHit {
	id: NoteId;
	title: string;
}

/**
 * Case-insensitive title-prefix search for the wikilink autocomplete.
 * Empty `prefix` returns []; backend caps `limit` at 50.
 */
export function searchNotesByTitle(prefix: string, limit = 8): Promise<TitleHit[]> {
	return transport<TitleHit[]>('search_notes_by_title', { prefix, limit });
}

// ─── Wikilink graph (M3) ───────────────────────────────────────────────────

export interface LinkRef {
	raw: string;
	target_text: string;
	alias: string | null;
	/** Resolved NoteId if the target exists; null for dangling links. */
	target_id: NoteId | null;
}

export interface Backlink {
	source_id: NoteId;
	source_title: string | null;
	source_preview: string;
	raw: string;
}

/** Outbound wikilinks from a note. */
export function outboundLinksOf(id: NoteId): Promise<LinkRef[]> {
	return transport<LinkRef[]>('outbound_links_of', { id });
}

/** Notes that link *to* a given note (via resolved target_id). */
export function backlinksFor(id: NoteId): Promise<Backlink[]> {
	return transport<Backlink[]>('backlinks_for', { id });
}

// ─── Voice / STT (M4) ──────────────────────────────────────────────────────

/**
 * Coarse user-facing recording state machine. Mirrors the Rust
 * `RecordingState` enum (serde tagged on `kind`, snake_case variants).
 * The UI polls `getTranscriptionState()` every ~100 ms while recording
 * (M4-T7 RecordingIndicator).
 */
export type RecordingState =
	| { kind: 'idle' }
	| { kind: 'recording'; started_at_ms: number }
	| { kind: 'transcribing' }
	| { kind: 'error'; message: string };

/**
 * Begin a recording. Lazy-loads the whisper model from the XDG default path
 * on first call; subsequent calls reuse the loaded engine. Throws an
 * `IpcError` with `code: 'model_missing'` (message = path) when the user
 * has not placed a model file yet — M4-T11 catches this for the first-run
 * prompt. Other failure codes: `'no_input_device'`, `'audio'`, `'stt'`.
 */
export function startRecording(): Promise<void> {
	return transport<void>('start_recording');
}

/**
 * Stop the active session and run whisper inference on a blocking task pool.
 * Returns the transcript string. Errors leave `rec_state` as `error` until
 * the next `startRecording()` resets it.
 */
export function stopRecordingAndTranscribe(): Promise<string> {
	return transport<string>('stop_recording_and_transcribe');
}

/** Synchronous getter; UI polls this while recording / transcribing. */
export function getTranscriptionState(): Promise<RecordingState> {
	return transport<RecordingState>('get_transcription_state');
}
