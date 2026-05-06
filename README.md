# Plainnote

Local-first, files-on-disk note-taking with hierarchical tags and a four-mode tag query algebra. No telemetry. No cloud. No network calls.

**Status:** v0.1 — Linux desktop. Voice capture (M4) and Android port (M7/M8) are deferred to v0.2.

## What it does

- **Plain Markdown on disk.** Every note is a `.md` file. The vault works with `cat`, `vim`, `grep`, `git`, Obsidian, anything. The app is a layer over the filesystem, not the source of truth.
- **Hierarchical tags.** `work::projects::plainnote` inherits everything its parents inherit. Tags are folders, not flat strings.
- **Four-mode tag query algebra.** AND / OR / NOT / EXCLUDE — combinable. Find every note tagged `work` AND `urgent` but NOT `done`. Compose four-mode queries from the tag picker; no syntax to learn.
- **Wikilinks with rename-stable references.** `[[Some Note]]` resolves by title; the autocomplete inserts ULID-anchored links that survive title renames. Backlinks panel is reactive.
- **Force-directed graph view** of note connections, capped at 5,000 nodes.
- **Reminders** with native desktop notifications.
- **Theme + density + accent.** Settings are persisted; `prefers-color-scheme` and `prefers-reduced-motion` are honored.
- **Accessibility baseline.** Single global aria-live region, visible focus rings, keyboard nav contract.

## Install

Download from the [v0.1.0 release page](https://github.com/adhamcybersec/Plainnote/releases/tag/v0.1.0). Three formats are published; pick whichever fits your distro.

### Flatpak (recommended — sandboxed, distro-agnostic)

```bash
flatpak install --user ./Plainnote-0.1.0-x86_64.flatpak
flatpak run dev.plainnote.app
```

The Flatpak runs with a strict no-network sandbox: the app cannot make outbound connections — that's enforced by the Flatpak runtime, not by code review. Audit the granted permissions yourself with `flatpak info --show-permissions dev.plainnote.app`.

### .deb (Debian, Ubuntu, derivatives)

```bash
sudo dpkg -i plainnote_0.1.0_amd64.deb
sudo apt-get install -f   # if any deps are missing
plainnote
```

### AppImage (any glibc-based distro, no install)

```bash
chmod +x Plainnote-0.1.0-x86_64.AppImage
./Plainnote-0.1.0-x86_64.AppImage
```

Requires FUSE 2 (`libfuse2t64` on Ubuntu 24.04+, `libfuse2` on older).

### Verify checksums (every release)

```bash
# Download SHA256SUMS from the release page alongside the artifacts, then:
sha256sum -c SHA256SUMS
```

## Privacy posture

- **Zero telemetry.** No analytics, crash reporting, version-check pings, or any other outbound network call. The Flatpak release enforces this at the sandbox level (`--share=network` is not granted).
- **No cloud.** Notes never leave the device. Sync, when it lands in v0.2, is P2P (Linux↔Android), not via a vendor server.
- **Vault is yours.** Plain `.md` files in any directory you choose. Move them, sync them with Syncthing or `rsync`, version them with `git`. Plainnote reads them; it does not own them.

## Documentation

- [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) — building from source.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — how to file issues and submit changes.

## License

[AGPL-3.0-or-later](LICENSE). Fork-friendly: anyone can take this code, modify it, and self-host. The AGPL ensures network deployments stay open.
