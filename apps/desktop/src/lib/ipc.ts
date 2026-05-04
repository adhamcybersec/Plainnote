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

export type IpcErrorCode = 'io' | 'invalid' | 'not_found' | 'locked';

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
