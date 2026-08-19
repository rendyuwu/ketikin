## What changed

<!-- One or two sentences. What does this PR do? -->

## Why

<!-- The problem or request behind the change. Link an issue if there is one. -->

## How it was tested

<!-- Which platforms did you actually run this on? Which flows did you exercise? -->

- [ ] Linux
- [ ] Windows
- [ ] macOS

## Checklist

- [ ] `npm run typecheck` passes
- [ ] `cargo clippy --all-targets -- -D warnings` is clean (run from `src-tauri/`)
- [ ] `cargo fmt --all -- --check` is clean (run from `src-tauri/`)
- [ ] `CHANGELOG.md` updated if this is a user-facing change
- [ ] If releasing: version bumped together in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
