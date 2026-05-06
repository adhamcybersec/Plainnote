# Contributing to Plainnote

Thanks for considering a contribution. This document covers the practical bits — how to file issues, propose changes, and what gets accepted.

## Scope and project posture

Plainnote is a **local-first, files-on-disk note-taking app** with a hard set of constraints:

- Plain Markdown on disk; the app is a layer over the filesystem, never the source of truth.
- No telemetry, no analytics, no version-check pings, no cloud dependencies.
- No source-available licenses, no proprietary deps that gate features.
- Hierarchical tags with inheritance and the four-mode tag query algebra are core; changes that compromise these are out of scope.

If a contribution conflicts with any of these, it will be politely declined. Open a discussion before writing code if you're unsure.

## Filing issues

- **Bugs.** Include: OS + version, install format (Flatpak / .deb / AppImage), Plainnote version (`plainnote --version` or check Settings → About), steps to reproduce, observed vs. expected behavior. Logs help when relevant: `~/.local/share/dev.plainnote.app/logs/` (Flatpak) or `~/.local/share/Plainnote/logs/` (.deb / AppImage).
- **Feature requests.** Describe the user-facing problem you're trying to solve, not the implementation. "I want to find every note tagged X but not Y" is a request; "add a query syntax" is a solution. Solutions get pinned by the existing four-mode algebra; problems get heard.
- **Security issues.** Don't open a public issue. Email the maintainer directly via the address in the GitHub profile, or open a private security advisory through the repo's Security tab.

## Pull requests

1. **Branch from `main`.** No long-lived feature branches.
2. **Match the style.** Rust: `cargo fmt` + `cargo clippy --all-targets -- -D warnings`. TypeScript / Svelte: `npm run lint` + `npm run check`. CI runs both; PRs that fail style won't be merged.
3. **Write tests.** Rust: unit tests live next to the code (`#[cfg(test)] mod tests`). Frontend: vitest in `tests/unit/`. End-to-end Playwright tests live in `tests/e2e/`. New features need tests; bug fixes need a regression test.
4. **One concern per PR.** Refactors and behavior changes go in separate PRs — easier review, easier revert.
5. **Commits.** Conventional-ish prefix (`fix:`, `feat:`, `refactor:`, `docs:`, `test:`, `chore:`). Body explains *why*, not *what* — the diff already shows what.
6. **Sign off your commits** with `git commit -s` (DCO). No CLA — the AGPL-3.0 license is sufficient.

## Building from source

See [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for the full setup. Short version:

```bash
# System deps (Ubuntu 24.04+)
sudo apt install -y build-essential libwebkit2gtk-4.1-dev libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf libfuse2t64

# Rust + Node
rustup default stable
nvm install 22

# Build + run
cd apps/desktop
npm ci
npm run tauri dev
```

## Code of conduct

Be civil. Disagreements about technical direction are fine and expected; personal attacks are not. The maintainer reserves the right to remove comments and ban repeat offenders without lengthy debate.

## License

By contributing, you agree that your contributions will be licensed under the [AGPL-3.0-or-later](LICENSE), the same license as the rest of the project. The AGPL ensures that anyone who runs a modified network-deployed version of Plainnote must release their changes — that's the deal.

You retain copyright on your contributions; there's no Contributor License Agreement (CLA). If you don't want to license your work under AGPL, don't contribute. That's not unfriendly — it's the only way an AGPL project stays AGPL.
