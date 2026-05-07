# Plainnote v0.1.0

First release. Linux desktop only — voice and Android come in v0.2.

## What's in this release

**Capture, tag, query**

- Files-on-disk vault. Every note is a `.md` file the app neither owns nor mutates without your action.
- Hierarchical tags with inheritance (`work::projects::plainnote`).
- Four-mode tag query algebra (AND / OR / NOT / EXCLUDE) — composable from the tag picker, no syntax to memorize.

**Edit + connect**

- CodeMirror 6 editor with markdown support and live preview.
- Wikilinks (`[[Title]]`) with autocomplete that inserts ULID-anchored references — links survive title renames.
- Backlinks panel reactive to renames (no link-rotting on edit).
- Force-directed graph view, capped at 5,000 nodes.

**Reminders**

- Schedule reminders on any note. Tokio-backed scheduler delivers via `notify-rust` → D-Bus.
- Notifications fire even after the app is restarted (rows persist in the index).

**UX baseline**

- Theme (light / dark / system), density (compact / comfortable), accent color (4 swatches).
- `prefers-color-scheme` and `prefers-reduced-motion` honored.
- Accessibility: single global aria-live region, visible focus rings, keyboard nav contract.

**Privacy**

- Zero telemetry. No analytics. No version-check pings. No cloud.
- The Flatpak release runs without `--share=network` — outbound connections are blocked at the sandbox level, not just absent in the source.

## Verification

```bash
sha256sum -c SHA256SUMS
```

Compare against the SHA256SUMS file uploaded with this release.

## Known limitations

- **Title-collision wikilink resolution** picks an arbitrary winner when two notes share a title. Use ULID-anchored links (the autocomplete inserts these) for stability. Addressed in v0.2.
- **Accent contrast on warm swatches** (ochre, clay) may not hit WCAG AA on the lightest text/surface combinations. The default swatches (sage, ink) are AA.
- **Graph view** softens above ~3,000 nodes (force simulation throttles). Hard cap is 5,000. Larger vaults render but may be sluggish during initial layout.
- **No Orca screen-reader pass.** Smoke tests exist for the aria contract but the app hasn't been driven by a live Orca user. Reports welcome.
- **No high-contrast or reduced-transparency mode awareness.** OS-level reduce-motion is honored.

## Deferred to v0.2

- **Voice capture (M4)** — local Whisper.cpp transcription. Postponed to scope phone hardware first.
- **Android port (M7/M8)** — Tauri 2 Android target needs more soak time before we depend on it.
- **P2P sync** — Linux ↔ Android over Syncthing, not via a vendor server. Will require Flatpak manifest update to add `--share=network` (surfaced as a permission change at upgrade time, not snuck in).
- **Vault encryption at rest.**
- **Color-contrast audit in CI** (pa11y / axe-core).

## Build artifacts

Three formats, all on x86_64 Linux:

| Format | File | Notes |
|---|---|---|
| Flatpak | `Plainnote-0.1.0-x86_64.flatpak` | Sandboxed, no-network, GNOME 49 runtime |
| AppImage | `Plainnote-0.1.0-x86_64.AppImage` | Portable, requires `libfuse2t64` (or `libfuse2` on older distros) |
| .deb | `plainnote_0.1.0_amd64.deb` | Debian 12+, Ubuntu 22.04+ |

See [README.md](../README.md) for install instructions per format.

## License

AGPL-3.0-or-later. Fork-friendly. Anyone can take this code, modify it, self-host it.
