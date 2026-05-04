<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Plainnote — development guide

## Prerequisites (Ubuntu 24.04)

- Rust ≥ 1.78 via [rustup](https://rustup.rs)
- Node.js 22+ (LTS) and npm 10+
- Tauri 2 system libraries:

```bash
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev \
  build-essential curl wget file
```

## First run

```bash
cd apps/desktop
npm install
npm run tauri:dev
```

A window titled **Plainnote** should open within a few seconds, showing a heading and a "Check backend" button. Clicking it calls the `ping` Tauri command (round-trip through the Rust core) and renders `pong`.

## Layout

```
apps/desktop/
├── src-tauri/         Rust crate (Tauri shell + core/)
│   ├── src/
│   │   ├── core/      pure business logic (no tauri:: imports)
│   │   ├── commands.rs Tauri command wrappers
│   │   ├── lib.rs
│   │   └── main.rs
│   ├── capabilities/  Tauri 2 capability files (default-deny)
│   ├── icons/         placeholder PNG; replace before v0.1
│   └── tauri.conf.json
└── src/               SvelteKit frontend
    ├── lib/
    │   └── tokens.css canonical design tokens
    ├── routes/        pages
    ├── app.css        Tailwind 4 entrypoint + theme bridge
    └── app.html
```

## Running tests

```bash
# Rust unit + integration
cd apps/desktop/src-tauri && cargo test

# Frontend unit (Vitest)
cd apps/desktop && npm run test

# E2E (Playwright)
cd apps/desktop && npm run test:e2e
```

## Code style

- Rust: `cargo fmt` and `cargo clippy -- -D warnings`. Both run in CI.
- TypeScript/Svelte: `svelte-check`. Format on save (Prettier config TBD in M1a).
- Every committed file starts with `// SPDX-License-Identifier: AGPL-3.0-or-later` (or `<!-- ... -->` for HTML/Markdown that we ship). Pre-commit hook lands in M0 close-out.

## Pre-commit hook (one-time setup)

Every source file must carry an SPDX header. A pre-commit hook enforces this locally:

```bash
# Run once, from the repository root:
git config core.hooksPath tools/git-hooks
```

The hook scans staged files and rejects any source file that doesn't have `SPDX-License-Identifier:` in its first 5 lines. To bypass once (rare; use only when you know what you're doing):

```bash
git commit --no-verify
```

CI also runs `bash tools/git-hooks/check-spdx-tree.sh` over the whole tracked tree, so `--no-verify` commits get caught at PR time.

## License

AGPL-3.0-or-later. Every source file carries an SPDX header.
