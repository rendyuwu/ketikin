# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-19

Initial public release of Ketikin, a simple cross-platform auto-typer for consoles that do not
accept clipboard paste.

### Added

- **Type panel** with a multi-line text box, a configurable per-keystroke delay, a countdown
  before typing begins so the target window can be focused, a live progress readout, and Start /
  Stop controls.
- **Templates** for reusable snippets. Templates have a name and content, can be loaded into the
  Type panel with a click, and can be edited and deleted. Stored locally as JSON.
- **Settings** covering typing delay, countdown length, theme (system, dark, or light),
  minimize-to-tray, close-to-tray, always-on-top, global hotkeys (enable, disable, and rebind),
  newline handling mode, automatic update checks, and a read-only display of the resolved data
  storage path.
- **System tray icon** with show/hide window and quit. On desktops that provide no system-tray
  host, the minimize-to-tray and close-to-tray settings are ignored for that session and a banner
  explains why, so the window can never be hidden with no way to restore or quit it. Saved
  preferences are left intact.
- **Global hotkeys** to start and stop typing without focusing the Ketikin window. Defaults are
  `Ctrl+Alt+T` to start and `Ctrl+Alt+X` to stop, shown as `⌘+Alt+T` and `⌘+Alt+X` on macOS.
- **Auto-update** against GitHub Releases. Updates are downloaded, verified against a
  minisign signature, installed, and applied on restart. Can be disabled in Settings. On Linux,
  in-place self-installation works only for the AppImage; `.deb` and `.rpm` installations are
  notified of a new version and given a download link instead.
- **Resilient storage** with a fallback chain: the OS application-data directory, then `%APPDATA%`
  and `%LOCALAPPDATA%` on Windows, then a `data` folder next to the executable, then the system
  temp directory. Writes are atomic, so an interrupted save leaves the previous file intact. If no
  location is writable the app continues with in-memory data and shows a warning banner. On
  Windows the candidates are less independent than the list suggests — the first already sits
  inside `%APPDATA%`, temp sits inside `%LOCALAPPDATA%`, and the folder beside the executable is
  only writable for portable and per-user installs — so the recovery that matters on Windows
  Server, RDP, and roaming-profile setups is `%LOCALAPPDATA%`, which survives a roaming profile
  that is unavailable or read-only. Where even that is denied, Ketikin runs in memory and says so
  instead of silently discarding saves.
- **Builds for Windows x64** (NSIS `.exe` supporting per-machine and per-user installs, plus
  `.msi`), **Linux x64** (`.AppImage`, `.deb`, and `.rpm`), and **macOS** on both Intel x64 and
  Apple Silicon arm64 (`.dmg`).

[Unreleased]: https://github.com/rendyuwu/ketikin/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rendyuwu/ketikin/releases/tag/v0.1.0
