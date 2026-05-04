// SPDX-License-Identifier: AGPL-3.0-or-later
//
// We use @tailwindcss/postcss instead of @tailwindcss/vite. The Vite plugin
// intercepts SvelteKit's `?svelte&type=style&lang.css` virtual URLs at
// `enforce: 'pre'`, reads the raw .svelte file from disk before Svelte's
// preprocessor extracts the <style> block, and parses script + markup as
// CSS — failing with misleading "Invalid declaration: <js-symbol>" errors.
// The PostCSS plugin operates after Svelte preprocessing and avoids that.
// See docs/DECISIONS.md ADR-005.
export default {
	plugins: {
		'@tailwindcss/postcss': {},
		autoprefixer: {}
	}
};
