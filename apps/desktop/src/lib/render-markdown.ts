// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Markdown rendering — security-critical entry point.
 *
 * Threat model: a hostile note (e.g. one synced in from an untrusted device,
 * or simply pasted from a malicious source) must not be able to execute
 * scripts, exfiltrate the vault via on-event handlers, or load remote
 * resources from data:/javascript: URLs.
 *
 * Defense in depth (per plan §3.7):
 *   1. markdown-it is configured `html: false` so raw HTML in the source
 *      is rendered as text, never parsed.
 *   2. DOMPurify is run over the final HTML string with a strict allowlist;
 *      any tag/attribute that slips through markdown-it is dropped.
 *   3. Schemes other than http(s) and relative are forbidden in href/src.
 *
 * The renderer never injects HTML directly into the DOM — callers must
 * receive the cleaned string and pass it through Svelte's `{@html …}` on a
 * controlled surface only. Never assign the output to innerHTML on a global
 * element.
 */
import MarkdownIt from 'markdown-it';
import DOMPurify from 'dompurify';

const md = new MarkdownIt({
	html: false, // raw HTML in source is treated as text
	linkify: true, // bare URLs become links — DOMPurify enforces the scheme allowlist
	breaks: false,
	typographer: false
});

// Conservative allowlist — only structural and inline elements we actually
// emit from markdown. No <iframe>, <object>, <embed>, <form>, no SVG.
const ALLOWED_TAGS = [
	'a',
	'p',
	'br',
	'hr',
	'em',
	'strong',
	'code',
	'pre',
	'blockquote',
	'ul',
	'ol',
	'li',
	'h1',
	'h2',
	'h3',
	'h4',
	'h5',
	'h6',
	'table',
	'thead',
	'tbody',
	'tr',
	'th',
	'td',
	'img',
	'span',
	'del',
	'sub',
	'sup'
];

const ALLOWED_ATTRS = ['href', 'title', 'src', 'alt', 'class', 'colspan', 'rowspan'];

// Forbid any URL scheme except http(s), mailto, and relative paths.
// `javascript:` and `data:` are the obvious vectors; we deny by allowlist
// rather than denylist so a future scheme cannot sneak in.
const SAFE_URL = /^(?:https?:|mailto:|\/|\.\/|\.\.\/|#|$)/i;

/**
 * Render Markdown source to a sanitized HTML string.
 *
 * The output is safe for `{@html …}` on a SvelteKit-controlled element.
 * Never bypass this function.
 */
export function renderMarkdown(source: string): string {
	const rendered = md.render(source);
	return DOMPurify.sanitize(rendered, {
		ALLOWED_TAGS,
		ALLOWED_ATTR: ALLOWED_ATTRS,
		ALLOW_DATA_ATTR: false,
		ALLOWED_URI_REGEXP: SAFE_URL,
		// Drop the entire element if it contains a script/iframe — don't
		// merely strip the tag and keep the children, which could contain
		// further payloads.
		FORBID_TAGS: ['style', 'script', 'iframe', 'object', 'embed', 'form'],
		FORBID_ATTR: ['style']
	}) as string;
}
