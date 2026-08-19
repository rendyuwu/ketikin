# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Only one copy of Ketikin runs at a time.** Launching it again while it was hidden in the tray
  started a whole second app, complete with its own window and its own tray icon, while the
  original kept running invisibly. That was worse than a duplicate: global shortcuts can only
  belong to one process, so the new window reported a hotkey error while the shortcuts that
  actually worked belonged to the copy you could not see, and both copies wrote the same settings
  and templates files, so a template saved in one window vanished when the other saved. A second
  launch now brings the existing window back and exits.
  ([#13](https://github.com/rendyuwu/ketikin/issues/13))
- **Typing into a template no longer throws you back into the name field.** Every character typed
  into the content box moved the cursor back up to **Template name**, so the first character
  landed in the content and the rest of what you typed was appended to the name. The name field is
  focused when the form opens, and only then.
  ([#2](https://github.com/rendyuwu/ketikin/issues/2))

### Changed

- `docs/RELEASING.md` now describes the `target_commitish` release failure — what the error means,
  that it discards four platform builds that had already succeeded, and that CI cannot catch it —
  and no longer assumes a failed release left a GitHub release behind to delete.
  ([#1](https://github.com/rendyuwu/ketikin/issues/1))
- CI and release builds no longer hang when the Ubuntu mirror the runners default to stops
  answering. `apt-get update` had no timeout of its own for that case, so a job that hit it sat
  silent for as long as an hour instead of failing; on the release path that meant the release
  could not be published at all until someone cancelled the run by hand. The apt step now pins the
  mirror and bounds apt's own waits, every job in both workflows has a `timeout-minutes` sized off
  its observed runtime rather than GitHub's 360-minute default, and `docs/RELEASING.md` describes
  the symptom alongside the other "the build looks fine and produces nothing" failure.
  ([#18](https://github.com/rendyuwu/ketikin/issues/18))

## [0.1.0] - 2026-08-19

Initial public release of Ketikin, a simple cross-platform auto-typer for consoles that do not
accept clipboard paste.

### Added

- **Type panel** with a multi-line text box, a configurable per-keystroke delay, a countdown
  before typing begins so the target window can be focused, a live progress readout, and Start /
  Stop controls. A character count and an estimated run time are shown as you type. The estimate
  covers the countdown and every character in every newline mode, including the mode that skips
  line breaks — a skipped line break still costs its delay, so skipping does not make a run
  shorter.
- **Templates** for reusable snippets. Templates have a name and content, can be loaded into the
  Type panel with a click, and can be edited and deleted. Stored locally as JSON.
- **Settings** covering typing delay, countdown length, theme (system, dark, or light),
  minimize-to-tray, close-to-tray, always-on-top, global hotkeys (enable, disable, and rebind),
  newline handling mode, automatic update checks, and the resolved data storage path, with a
  button that opens that folder in the system file manager rather than leaving the path to be
  copied out by hand.
- **System tray icon** with show/hide window and quit. On desktops that provide no system-tray
  host, the minimize-to-tray and close-to-tray settings are ignored for that session and a banner
  explains why, so the window can never be hidden with no way to restore or quit it. Saved
  preferences are left intact.
- **Global hotkeys** to start and stop typing without focusing the Ketikin window. Defaults are
  `Ctrl+Alt+T` to start and `Ctrl+Alt+X` to stop, shown as `⌘+Alt+T` and `⌘+Alt+X` on macOS.
  A rebind the operating system refuses does not leave you with no shortcut at all: the previous
  combination is put back, it is the one that stays in the saved settings, and the failure is
  reported under that field in Settings. A shortcut that fails to register at startup — because
  another application already owns the combination, say — is reported the same way, rather than
  appearing bound and silently doing nothing. Where the conflict is Ketikin's own other shortcut
  the message says so, instead of sending you hunting for another application. While a shortcut
  field is capturing, both global shortcuts are released, so pressing your current shortcut into
  the field records it rather than starting a typing run. A settings file that somehow carries
  the same combination in both slots is repaired when it loads, rather than loading cleanly and
  then rejecting every later save.
- **Auto-update** against GitHub Releases. Updates are downloaded, verified against a
  minisign signature, installed, and applied on restart. Can be disabled in Settings. On Linux,
  in-place self-installation works only for the AppImage; `.deb` and `.rpm` installations are
  notified of a new version and given a download link instead.
- **Resilient storage** with a fallback chain: the OS application-data directory, then `%APPDATA%`
  and `%LOCALAPPDATA%` on Windows, then a `data` folder next to the executable, then the system
  temp directory. Each candidate is qualified by actually writing to it, under a time limit — at
  most two seconds per location and eight seconds for the whole chain — so a profile redirected to
  an unreachable network share falls through to the next candidate instead of leaving you in front
  of a window that never finishes opening. Writes are atomic, so an interrupted save leaves the
  previous file intact, and a save blocked by an antivirus scanner or a search indexer holding the
  file open is retried for about half a second before it is reported as a failure. That retry
  happens off the main thread, so a locked file cannot freeze the window mid-edit. If no location
  is writable the app continues with in-memory data and shows a warning banner. On Windows the
  candidates are less independent than the list suggests — the first already sits inside
  `%APPDATA%`, temp sits inside `%LOCALAPPDATA%`, and the folder beside the executable is only
  writable for portable and per-user installs — so the recovery that matters on Windows Server,
  RDP, and roaming-profile setups is `%LOCALAPPDATA%`, which survives a roaming profile that is
  unavailable or read-only. Where even that is denied, Ketikin runs in memory and says so instead
  of silently discarding saves.
- **Storage problems are shown, not just logged.** A settings or templates file that cannot be
  read is reset to defaults and says so, and the unreadable file is kept beside it as
  `settings.json.bak` or `templates.json.bak` so it can still be recovered by hand. A location
  that may be shared with other users of the machine says so, which is worth knowing before a
  password or a licence key goes into a template. A deliberate portable install next to the
  executable is a working configuration and does not raise a banner on every launch, but its
  notices stay in Settings > Storage — where all of them remain readable, whether or not the
  startup banner was shown or dismissed.
- **A log file** at `logs/Ketikin-<your user name>.log` inside the data folder, capped at 1 MB
  with two rotated copies kept, so it stays small enough to attach to a bug report. The name
  carries the user name because a file created by one account on a shared machine can otherwise
  lock another account out of it. Where the folder cannot be created at all — some hardened
  Windows profiles grant permission to add files but not sub-folders — Ketikin runs without a log
  file and says so in Settings rather than refusing to start. If startup fails so early that no
  window and no console exist to report it, Ketikin writes `ketikin-startup-error.log` into the
  data folder, and deletes it again on the next launch that gets as far as a window, so it never
  goes on describing a problem that has already been fixed.
- **Builds for Windows x64** (NSIS `.exe` supporting per-machine and per-user installs, plus
  `.msi`), **Linux x64** (`.AppImage`, `.deb`, and `.rpm`), and **macOS** on both Intel x64 and
  Apple Silicon arm64 (`.dmg`).

[Unreleased]: https://github.com/rendyuwu/ketikin/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/rendyuwu/ketikin/releases/tag/v0.1.0
