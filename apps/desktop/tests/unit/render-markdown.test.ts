// SPDX-License-Identifier: AGPL-3.0-or-later
/**
 * Markdown render pipeline — security-critical.
 *
 * Pipeline: markdown-it (html: false, linkify: true) → DOMPurify → string.
 * Both layers must hold: markdown-it should not parse raw HTML in the
 * first place, and DOMPurify is the second-line defense against bypasses
 * (e.g. autolinked `javascript:` URLs, on-event handlers in attributes).
 *
 * Per plan §3.7 + §6 M3, ALL of the following hostile inputs must produce
 * output where the dangerous payload does NOT reach the rendered HTML.
 */
import { renderMarkdown } from '$lib/render-markdown';

describe('renderMarkdown — security', () => {
	it('does not pass raw <script> tags through (escaped is fine)', () => {
		// Escaped text is safe — `&lt;script&gt;...` is inert. The kill
		// criterion is no live <script> open tag in the output.
		const out = renderMarkdown('hello <script>alert(1)</script> world');
		expect(out).not.toMatch(/<script[\s>]/i);
		expect(out).not.toMatch(/<\/script>/i);
	});

	it('strips javascript: URLs from links', () => {
		// markdown-it leaves a malformed link as paragraph text — no href is
		// emitted. We assert the kill criterion: no `href="javascript:..."`.
		const out = renderMarkdown('[click](javascript:alert(1))');
		expect(out).not.toMatch(/href=["']?\s*javascript:/i);
	});

	it('strips javascript: URLs from autolinks', () => {
		// linkify can promote bare URLs; the sanitizer must catch the unsafe scheme.
		const out = renderMarkdown('see javascript:alert(1) here');
		// If linkify wrapped it as <a href="javascript:..."> DOMPurify drops href.
		expect(out).not.toMatch(/href=["']javascript:/i);
	});

	it('strips data: URLs that could carry HTML/JS payloads', () => {
		const out = renderMarkdown('[x](data:text/html,<script>alert(1)</script>)');
		expect(out).not.toMatch(/href=["']data:/i);
		expect(out).not.toMatch(/<script/i);
	});

	it('drops on-event handlers if any slip into output', () => {
		// Parse the output as DOM and check no element has on* attributes —
		// a regex against the string can't tell `onerror=` inside a `title`
		// attribute value (inert) apart from a real attribute (live).
		const a = renderMarkdown('<img src=x onerror="alert(1)">');
		const aDoc = new DOMParser().parseFromString(`<div>${a}</div>`, 'text/html');
		for (const el of aDoc.querySelectorAll('*')) {
			for (const attr of el.attributes) {
				expect(attr.name.toLowerCase()).not.toMatch(/^on/);
			}
		}
		const b = renderMarkdown('![x](https://example.com/x.png "alt onerror=alert(1)")');
		const bDoc = new DOMParser().parseFromString(`<div>${b}</div>`, 'text/html');
		for (const el of bDoc.querySelectorAll('*')) {
			for (const attr of el.attributes) {
				expect(attr.name.toLowerCase()).not.toMatch(/^on/);
			}
		}
	});

	it('renders plain text and standard markdown safely', () => {
		const out = renderMarkdown('# Hello\n\nThis is **bold**.');
		expect(out).toMatch(/<h1/);
		expect(out).toMatch(/<strong>bold<\/strong>/);
	});

	it('renders task lists', () => {
		const out = renderMarkdown('- [ ] todo\n- [x] done');
		// Default markdown-it does not render task-list checkboxes; we accept
		// either a checkbox or a literal `[ ]` prefix as long as the list renders.
		expect(out).toMatch(/<ul[\s>]/);
		expect(out).toMatch(/<li/);
	});

	it('renders code blocks with content escaped', () => {
		const out = renderMarkdown('```\n<script>alert(1)</script>\n```');
		// The code block should appear escaped, not as a live tag.
		expect(out).not.toMatch(/<script>alert/);
		expect(out).toMatch(/&lt;script&gt;/);
	});

	it('renders tables', () => {
		const out = renderMarkdown('| a | b |\n|---|---|\n| 1 | 2 |');
		expect(out).toMatch(/<table/);
	});

	it('does not render iframes or embeds', () => {
		const out = renderMarkdown('<iframe src="https://evil"></iframe>');
		expect(out).not.toMatch(/<iframe/i);
	});

	it('does not let HTML comments smuggle script content', () => {
		const out = renderMarkdown('<!-- <script>alert(1)</script> -->');
		expect(out).not.toMatch(/<script/i);
	});
});
