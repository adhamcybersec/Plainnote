// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Smoke test — confirms Vitest is wired and basic test ergonomics work.
 * Real tests land starting M1a (frontmatter parser, ULID validation, IPC layer).
 */
describe('vitest smoke', () => {
	it('runs', () => {
		expect(2 + 2).toBe(4);
	});

	it('has globals available without imports', () => {
		// describe/it/expect work without a top-of-file `import { ... } from 'vitest'`.
		// This proves vite.config.ts test.globals = true is in effect.
		expect(typeof describe).toBe('function');
	});
});
