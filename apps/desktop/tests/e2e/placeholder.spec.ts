// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Placeholder E2E test.
 *
 * This file exists to register Playwright with the project; it does not
 * exercise the app. Real Tauri-driver-backed E2E tests land in M1a once
 * we have a capture-save-list flow worth asserting.
 *
 * Skipped because:
 *   - Browser binaries are not installed in this milestone (heavy download).
 *   - There is no app behavior beyond the M0 ping/pong landing page.
 *
 * Remove the .skip and add real assertions in M1a.
 */
import { test, expect } from '@playwright/test';

test.skip('M0 placeholder — real tests land in M1a', async ({ page }) => {
	await page.goto('/');
	await expect(page.locator('h1')).toHaveText('Plainnote');
});
