// SPDX-License-Identifier: AGPL-3.0-or-later
import { defineConfig } from 'vitest/config';
import { sveltekit } from '@sveltejs/kit/vite';

const host = process.env.TAURI_DEV_HOST;

// Tailwind 4 is wired via PostCSS (`postcss.config.js`), not via
// `@tailwindcss/vite`. The Vite plugin intercepts SvelteKit's virtual
// `?svelte&type=style&lang.css` URLs and reads the raw .svelte file from
// disk, leading to "Invalid declaration: <js-symbol>" errors. PostCSS runs
// after Svelte's preprocessor extracts the style block, so it sees correct
// CSS. See docs/DECISIONS.md ADR-005.
export default defineConfig({
	plugins: [sveltekit()],

	// Tauri expects a fixed dev port and disables auto-open behavior
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: host || false,
		hmr: host
			? { protocol: 'ws', host, port: 1421 }
			: undefined,
		watch: {
			// don't watch the Rust crate — Tauri handles it
			ignored: ['**/src-tauri/**']
		}
	},
	// allow Tauri to access devUrl
	envPrefix: ['VITE_', 'TAURI_ENV_*'],

	test: {
		// Vitest configuration — see https://vitest.dev/config/
		include: ['tests/unit/**/*.{test,spec}.{ts,js}', 'src/**/*.{test,spec}.{ts,js}'],
		exclude: ['tests/e2e/**', 'node_modules/**', 'build/**', '.svelte-kit/**'],
		environment: 'node',
		// global test API; mirror Jest ergonomics so `expect`/`describe`/`it` are available without import
		globals: true,
		// CI-friendly defaults
		reporters: process.env.CI ? ['default', 'github-actions'] : ['default'],
		coverage: {
			provider: 'v8',
			reporter: ['text', 'html'],
			include: ['src/lib/**/*.{ts,svelte}'],
			exclude: ['**/*.d.ts', '**/*.test.ts']
		}
	}
});
