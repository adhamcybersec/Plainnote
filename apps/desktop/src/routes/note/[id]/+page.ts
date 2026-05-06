// SPDX-License-Identifier: AGPL-3.0-or-later
// /note/[id] is dynamic — the [id] is a ULID assigned at runtime, never
// known at build time. The parent +layout.ts sets prerender = true so
// every static page is baked at build, but this route must opt out:
// prerendering would require enumerating every possible ULID, which is
// neither possible nor useful. At runtime adapter-static's index.html
// fallback serves this route as a SPA.
export const prerender = false;
