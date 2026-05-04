// SPDX-License-Identifier: AGPL-3.0-or-later
import { defineConfig } from '@playwright/test';

export default defineConfig({
	testDir: './tests/e2e',
	timeout: 30_000,
	fullyParallel: false, // Tauri app is single-instance
	reporter: process.env.CI ? 'github' : 'list',
	use: {
		// Tests against the Tauri dev server until M0+1 lands tauri-driver
		baseURL: 'http://localhost:1420',
		trace: 'on-first-retry'
	}
});
