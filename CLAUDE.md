# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

Ketikin is a Tauri v2 desktop auto-typer: it replays a block of text as real keystrokes into
whatever window has focus, for consoles that accept no clipboard paste. Rust core (`src-tauri/`)
owns every OS capability; React + TypeScript frontend (`src/`) renders the UI and holds none.

## Commands

Frontend (repository root):

```bash
npm install
npm run tauri dev      # Vite dev server + Rust backend together
npm run tauri build    # bundles into src-tauri/target/release/bundle/
npm run typecheck      # tsc --noEmit
npm run build          # tsc -b && vite build — catches project-reference errors typecheck misses
```

Backend (run from inside `src-tauri/`) — these four are exactly what CI enforces:

```bash
cargo fmt --all -- --check      # --check fails; plain `cargo fmt` silently rewrites
cargo clippy --all-targets -- -D warnings
cargo check
cargo test                      # plain, not --all-targets: the latter skips doctests
```

Single test: `cargo test typing::tests::sleep_cancellable_returns_early_when_cancelled`, or
`cargo test <substring>` for a group. Tests are in-module `#[cfg(test)] mod tests` blocks; `tempfile` is the only dev-dependency.

Version consistency check (also a CI job): `bash .github/scripts/check-version-sync.sh`.

Node 22 (`.nvmrc`). Rust stable, MSRV 1.77.2. On Linux, keystroke injection needs X11 or XWayland —
typing may silently do nothing under a native Wayland session.

## Architecture

Read `docs/ARCHITECTURE.md` before any change spanning both halves; it is current and detailed. The
points most likely to bite:

**IPC has exactly two chokepoints.** Every command is declared in `src-tauri/src/lib.rs` and every
frontend call and event subscription lives in `src/lib/api.ts`. Adding a backend command means
touching both plus `src/lib/types.ts`. Events are named `domain://name` (`typing://state`,
`storage://warning`, `hotkey://error`, `tray://unavailable`, `update://available`).

**Errors.** Backend code works with `AppError` (`error.rs`) and converts to `String` only at the
command boundary; commands return `Result<T, String>` and the frontend shows that string verbatim.
Never `Debug`-format into a user-facing message.

**Locking discipline in `AppState`.** The global-shortcut plugin marshals register/unregister onto
the main thread and blocks until done, so any lock a worker can hold must never be taken on the main
thread — that deadlocks the event loop permanently. Commands that write to disk are
`#[tauri::command(async)]` on purpose, so storage's rename backoff sleeps on a worker. `lock()`
recovers from poisoning rather than panicking.

**Typing engine.** One dedicated OS thread per run owns the `Enigo` connection; cancellation is an
`AtomicBool` checked between keystrokes. Three invariants the module defends: exactly one terminal
`typing://done` per accepted run (via `RunGuard`, including on panic), no modifier ever left held,
and `typing://state` coalesced to ~20 events/second.

**Startup events are delayed 1.5 s** (`STARTUP_EVENT_DELAY`) because `emit` does not buffer and
these are decided before the WebView mounts. Each has a pull-based counterpart (`storage_info`,
`tray_status`, `hotkey_status`) which is the channel that actually guarantees delivery — keep both
sides when adding a startup notice.

**Storage** walks a fallback chain of candidate directories, uses the first writable one, and
degrades to in-memory mode rather than failing startup. Writes are temp-file-plus-rename. It reports
a path, the chain entry that produced it, errors, and notices; `StorageInfo::degraded` is derived in
`Storage::info()` and is the single owner of the "show a banner" rule — do not reconstruct it in the
frontend. Banner fires for temp directory, in-memory mode, and a reset JSON file only; other notices
belong in Settings > Storage. Logs live in a `logs/` subdirectory of the same resolved directory.

**Settings** `normalize()` runs on both load and save; `validate()` runs on save only (so a bad value
is reported, not silently rewritten); `repair_hotkey_clash()` runs on load only (so a hand-edited
file can never reach a state save would refuse). Container-level `#[serde(default)]` keeps old and
partial files loading. `save_settings` returns the normalized value and the frontend re-renders from
that response — `useSettings` holds optimistic state behind a 400 ms debounce, and the backend's
answer always wins.

**Accelerators are stored verbatim** — trimmed, otherwise unmodified, compared exactly to decide
whether to rebind (so `Alt+K` → `alt+k` triggers a harmless rebind) and case-insensitively for the
start-vs-stop duplicate rule.

**`icons.rs` is Windows-only at runtime but compiled everywhere under `cfg(test)`** (`#[cfg(any(windows,
test))]`), because its whole content is size arithmetic over embedded files and CI runs tests on Linux.
Nothing outside `cfg(windows)` calls it. That also means `cargo check` on Linux does not compile the
callers in `lib.rs` and `tray.rs`: to type-check those without a Windows box, temporarily swap
`#[cfg(windows)]` for `#[cfg(all())]` in the three files and run `cargo clippy --all-targets`.

`src-tauri/capabilities/default.json` keeps `core:default` deliberately and nothing else: the
window, updater, process, opener and global-shortcut calls all happen in Rust, which bypasses the
ACL entirely.

## CI and release constraints

`.github/workflows/ci.yml` carries load-bearing steps that look removable:

- `npm run build` runs **before** clippy, because `tauri-build` resolves `frontendDist` at compile
  time and fails if `dist/` is absent.
- The ephemeral updater-signing-key step is required, not decorative. With `createUpdaterArtifacts:
  true` and a `pubkey` set, a bundle build with no signing key is a hard failure, and CI must never
  hold the production secret (fork PRs get none).
- CI is secret-free and read-only so it runs unchanged on fork PRs.

Releases are cut by pushing a `vX.Y.Z` tag. The version must be identical in `package.json`,
`src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`, and `CHANGELOG.md` must carry a matching
`## [X.Y.Z] - YYYY-MM-DD` heading — the release workflow slices release notes from that heading, and
`check-version-sync.sh` fails the build otherwise. Full runbook in `docs/RELEASING.md`.

## Conventions

Conventional Commits (`feat`, `fix`, `docs`, `chore`, `refactor`, `test`, `ci`), summary under ~72
characters. Branch off and PR against `main`. User-visible changes get a `CHANGELOG.md` entry under
`## [Unreleased]`. Fix clippy findings rather than adding `#[allow(...)]`; if an allow is genuinely
right, leave a one-line reason.

Comments in this codebase explain *why* a non-obvious choice was made, often at length, and several
document invariants that a plausible-looking simplification would break. Read them before deleting
or "tidying" them.
