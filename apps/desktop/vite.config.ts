// SPDX-License-Identifier: AGPL-3.0-or-later
import { defineConfig } from 'vitest/config';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/  +  https://vitest.dev/config/
export default defineConfig({
	// Note on plugin ordering: keep sveltekit() before tailwindcss() as a
	// defensive default. Even with this order, Tailwind 4's vite plugin uses
	// `enforce: 'pre'` and intercepts `?svelte&type=style&lang.css` virtual
	// URLs, which causes "Invalid declaration" errors when components have
	// scoped <style> blocks. The robust workaround is to keep page styles in
	// src/app.css (use class-prefixed selectors) and use Tailwind utility
	// classes inside components, rather than .svelte component-scoped <style>.
	plugins: [sveltekit(), tailwindcss()],

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
