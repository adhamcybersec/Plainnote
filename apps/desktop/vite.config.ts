// SPDX-License-Identifier: AGPL-3.0-or-later
import { defineConfig } from 'vite';
import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';

const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
	plugins: [tailwindcss(), sveltekit()],

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
	envPrefix: ['VITE_', 'TAURI_ENV_*']
}));
