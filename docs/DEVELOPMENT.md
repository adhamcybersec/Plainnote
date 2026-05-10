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

For voice capture (v0.2), also see the **Voice capture (M4)** section below — it adds `libasound2-dev`, `cmake`, and `libclang-dev`.

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

## Voice capture (M4 / v0.2)

Plainnote's voice-to-text runs entirely on-device via [whisper.cpp](https://github.com/ggerganov/whisper.cpp) (Rust binding: `whisper-rs`). No audio leaves the machine. See [DECISIONS.md ADR-010](DECISIONS.md) for the architecture rationale.

### Additional build prerequisites (Ubuntu 24.04)

```bash
sudo apt install -y libasound2-dev cmake libclang-dev
```

- `libasound2-dev` — cpal links against ALSA on Linux, even when PipeWire is the user-facing audio server.
- `cmake` and `libclang-dev` — `whisper-rs` compiles whisper.cpp from C/C++ via cmake on first build (~3–5 min). Subsequent builds are cached.

### Model file (user-supplied)

Plainnote does **not** bundle any whisper.cpp model. You download one once and drop it at the resolved path. Default location:

```
$XDG_DATA_HOME/plainnote/models/ggml-base.en.bin
# typically: ~/.local/share/plainnote/models/ggml-base.en.bin
```

#### Default model — `ggml-base.en.bin` (~150 MB)

```bash
mkdir -p ~/.local/share/plainnote/models
curl -L -o ~/.local/share/plainnote/models/ggml-base.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
sha1sum ~/.local/share/plainnote/models/ggml-base.en.bin
# expected: 137c40403d78fd54d454da0f9bd998f78703390c
```

`base.en` is fast and accurate enough for everyday English speech. It garbles novel proper nouns ("Plainnote" → "blend notes" in the spike test). For better recognition of names and technical terms, swap in `ggml-small.en.bin` (~466 MB) and point Settings → Voice & speech at it.

If the user clicks **Record** and no model file is present, the app shows a first-run dialog with the same instructions and a copy-pasteable command block. The app does not silently auto-download.

### Custom model path

Settings → Voice & speech accepts an absolute path to any whisper.cpp-format `.bin` file. The setting is persisted in the SQLite meta table under `whisper.model_path`; clearing it falls back to the XDG default.

### Running the gated whisper test

The whisper integration test loads the real model and runs inference on synthesized silence. It's gated behind a feature flag so plain `cargo test` doesn't pull a 150 MB file into CI:

```bash
cd apps/desktop/src-tauri
cargo test --lib --features whisper-integration
```

If the model is at a non-default path, point the test at it:

```bash
PLAINNOTE_TEST_MODEL_PATH=/path/to/ggml-base.en.bin \
  cargo test --lib --features whisper-integration
```

The test skips with a clear stderr message when the model is absent — useful for CI runners that don't carry the model.

### Flatpak audio (M4-T10)

The Flatpak manifest grants `--socket=pulseaudio` and `--filesystem=xdg-run/pipewire-0:ro`. Network is **not** granted (voice is fully local; `--share=network` is reserved for the M-Sync milestone). Before tagging v0.2.0, run a sandbox-side smoke: build the .flatpak, install via `flatpak install --user --reinstall`, click Record, speak, see transcript. If the minimum flags don't work, the fallback ladder lives in `docs/plans/M4-T0-spike-runbook.md`.

## License

AGPL-3.0-or-later. Every source file carries an SPDX header.
