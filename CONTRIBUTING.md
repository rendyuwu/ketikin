# Contributing to Ketikin

Thanks for your interest in improving Ketikin. This document covers how to get a development
environment running, how the project is laid out, what checks to run before you push, and how
releases are made.

Bug reports and feature discussion belong in
[GitHub Issues](https://github.com/rendyuwu/ketikin/issues). If you are planning a substantial
change, opening an issue first is usually faster than opening a pull request and discovering the
change was heading in a different direction than intended.

## Development environment

You need two toolchains regardless of platform:

- **Rust**, stable channel. Install via [rustup](https://rustup.rs/).
- **Node.js 22.** The repository contains an `.nvmrc`, so `nvm use` will pick the right version.

On top of that, each platform needs its own set of native build dependencies, because Tauri
compiles against the system WebView and GUI libraries.

### Linux (Ubuntu / Debian)

```bash
sudo apt update
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libssl-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

Note that keystroke injection requires X11 or XWayland; under a native Wayland session it may not
work, which is worth knowing when you are testing typing behaviour locally. There is no separate
input library to install — the typing engine speaks the X11 protocol directly.

Other distributions ship the same libraries under different names; consult the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) page for your package manager.

### Windows

- **Microsoft Visual Studio C++ Build Tools**, with the "Desktop development with C++" workload.
- **WebView2 runtime.** Already present on Windows 11 and on up-to-date Windows 10; install it
  manually otherwise.

### macOS

- **Xcode Command Line Tools**: `xcode-select --install`.

Remember that a locally built app still needs Accessibility permission granted in
**System Settings > Privacy & Security > Accessibility** before it can type anything. A freshly
rebuilt binary is sometimes treated as a new app, so you may need to re-grant it after a rebuild.

### Getting it running

```bash
git clone https://github.com/rendyuwu/ketikin.git
cd ketikin
npm install
npm run tauri dev
```

`npm run tauri dev` starts the Vite dev server and the Rust backend together, with hot reload on
the frontend. Rust changes trigger a recompile and restart.

To produce a release build for your current platform:

```bash
npm run tauri build
```

Bundles are written to `src-tauri/target/release/bundle/`.

## Project layout

```
ketikin/
├── src/                  React + TypeScript frontend (Vite)
├── src-tauri/            Rust backend — the Tauri application core
├── .github/workflows/    CI and release automation
├── docs/                 Architecture and maintainer documentation
├── package.json          Frontend dependencies and npm scripts
└── CHANGELOG.md          Release notes, consumed by the release workflow
```

`src/` holds the interface: the Type, Templates, and Settings panels. `src-tauri/` holds
everything that touches the operating system — storage, the typing engine, hotkeys, the tray icon,
and the updater. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for how the pieces fit together
before making a change that spans both sides.

## Running checks

These are exactly what CI enforces. Run them before pushing and CI should not surprise you.

**Frontend**

```bash
npm run typecheck
npm run build
```

`npm run build` matters because it type-checks with the full project references and catches
failures that `typecheck` alone does not.

**Backend** — run from inside `src-tauri/`:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo check
cargo test
```

Two of these are stricter than the bare commands you might reach for by habit, and the difference
is where contributors get caught out:

- **`cargo fmt --check`** reports misformatting and fails; plain `cargo fmt` silently rewrites your
  files instead. CI uses the `--check` form, so run `cargo fmt` to fix and `cargo fmt --check` to
  confirm.
- **`cargo clippy --all-targets -- -D warnings`** turns every lint into an error and covers tests
  and examples, not just the library. A plain `cargo clippy` can look clean while CI fails.

Please fix clippy findings rather than adding `#[allow(...)]`, unless the lint is genuinely wrong
for the situation — in which case a one-line comment explaining why is appreciated.

## Commit conventions

Commits follow [Conventional Commits](https://www.conventionalcommits.org/). The message starts
with a type, an optional scope, and a short imperative summary:

```
feat(templates): allow renaming a template in place
fix(storage): fall back to LOCALAPPDATA when APPDATA is read-only
docs: document the Wayland limitation in the README
```

Types in use:

| Type | Use it for |
| --- | --- |
| `feat` | A new user-visible capability. |
| `fix` | A bug fix. |
| `docs` | Documentation only. |
| `chore` | Maintenance that is not a feature or fix — dependency bumps, tidying. |
| `refactor` | Restructuring that does not change behaviour. |
| `test` | Adding or correcting tests. |
| `ci` | Changes to workflows and build automation. |

Keep the summary line under about 72 characters, and put the reasoning in the body if the change
is not self-explanatory. Small, focused commits are much easier to review than one large one.

## Pull requests

- Branch off `main` and open the pull request against `main`.
- Make sure all the checks above pass.
- Add a `CHANGELOG.md` entry under `## [Unreleased]` if the change is user-visible. Purely
  internal changes do not need one.
- Describe what you changed and, more importantly, why. If the change affects typing behaviour on
  a particular platform, say which platform you tested on — maintainers cannot always reproduce
  every environment.

## Release process

Releases are cut by tagging. The steps, in order:

1. **Bump the version in all three places, together.** They must agree:
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`

2. **Add a `CHANGELOG.md` entry** for the new version. The heading format matters — the release
   workflow extracts release notes by slicing the changelog for the heading that matches the tag,
   so it must be exactly:

   ```
   ## [0.2.0] - 2026-09-01
   ```

3. **Commit and push** the version bump and changelog entry to `main`.

4. **Push the tag:**

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

   Pushing a `vX.Y.Z` tag triggers the release workflow, which builds every platform, signs the
   artifacts, and creates the GitHub release.

The release is created as a **draft**. A maintainer reviews and publishes it manually; the updater
does not see it until it is published. Maintainers should follow
[docs/RELEASING.md](docs/RELEASING.md) for the full runbook, including the required repository
secrets and how to verify a release before publishing.

## License

By contributing, you agree that your contributions are licensed under the MIT License, the same as
the rest of the project. See [LICENSE](LICENSE).
