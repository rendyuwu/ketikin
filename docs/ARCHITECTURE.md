# Architecture

A short map of how Ketikin is put together, for anyone about to change it.

## Process model

Ketikin is a Tauri v2 application, which means one process with two halves:

- **The Rust core** owns everything that touches the operating system: keystroke injection, file
  storage, global hotkey registration, the tray icon, and the updater.
- **The WebView UI** is a React + TypeScript frontend, built with Vite, rendered in the platform's
  native WebView (WebView2 on Windows, WebKitGTK on Linux, WKWebView on macOS).

The two halves talk over Tauri's IPC bridge. The frontend invokes commands exposed by the Rust
side; the Rust side pushes events back to the frontend for anything asynchronous — typing progress
most of all. The frontend holds no OS capability of its own, which keeps the security-relevant
surface in one place.

## Backend modules (`src-tauri/src/`)

| Module | Responsibility |
| --- | --- |
| Storage | Resolves where data lives, reads and writes `settings.json` and `templates.json` atomically, and exposes both the resolved path and which entry of the fallback chain produced it so the UI can display them. |
| Settings | The settings model, its defaults, and range validation. Missing or unrecognised keys fall back to defaults, so older and hand-edited files still load. Applies settings with runtime effects — theme, always-on-top, tray behaviour — and re-registers hotkeys when they change. |
| Templates | CRUD over the saved snippets, persisted through storage. |
| Typing engine | Turns a block of text into keystrokes. Owns the countdown, the per-keystroke delay, newline handling, and the cancellable worker. |
| Hotkeys | Registers and unregisters the global start and stop accelerators and maps them onto the typing engine's start and stop. Registration can fail when another process already owns a combination; the failure is surfaced to the UI and the previous binding is kept. |
| Tray | Builds the tray icon and its menu (show/hide the window, quit), and intercepts window close and minimize when the tray settings call for it. If the icon cannot be created — no StatusNotifier host on the desktop — both tray settings are ignored for the session so the window can never become unreachable, and the UI shows a banner. Persisted settings are not rewritten. |
| Updater | Checks GitHub Releases, verifies the minisign signature against the compiled-in public key, and installs and restarts. Skipped entirely when auto-check is off. |

## Typing engine

Keystroke injection goes through the [`enigo`](https://crates.io/crates/enigo) crate, which
abstracts the three platform input APIs behind one interface — SendInput on Windows, the X11
protocol via `x11rb` and `xkbcommon` on Linux, and the Core Graphics event APIs on macOS. This is
why the platform requirements in the README look the way they do: an X11 or XWayland session on
Linux, Accessibility permission on macOS. Because the Linux path speaks X11 directly rather than
going through a helper library, there is no additional runtime input dependency to install.

Typing runs on a **dedicated worker thread**, never on the UI thread or an async task that could
block the runtime. A slow console with a 200 ms delay and ten thousand characters means a worker
that runs for half an hour, and the app has to stay responsive throughout.

Cancellation uses an **atomic stop flag** shared between the worker and the rest of the app. The
worker checks it between keystrokes; Stop — whether from the button or the global hotkey — sets it
and returns immediately rather than waiting for the worker to finish. Because the check happens
between keystrokes rather than during one, stopping is bounded by a single delay interval, which
is what makes the Stop hotkey feel instant even at slow speeds.

The worker **streams progress events** to the frontend as it goes: the countdown ticking down, then
characters typed against the total, then a terminal event when the run finishes or is cancelled.
The frontend renders the progress readout purely from these events and holds no timer of its own,
so the display cannot drift out of sync with what is actually being typed.

## Storage fallback chain

Storage does not assume any particular directory exists or is writable. It walks a chain of
candidates, uses the first one it can successfully write to, and remembers the choice:

1. The OS application-data directory (`%APPDATA%\com.rendyuwu.ketikin`,
   `~/.local/share/com.rendyuwu.ketikin`, `~/Library/Application Support/com.rendyuwu.ketikin`)
2. `%APPDATA%` — Windows, read directly from the environment
3. `%LOCALAPPDATA%` — Windows, read directly from the environment
4. A `data` folder beside the executable — portable and locked-down installs
5. The system temp directory — last resort, may not survive a reboot

This exists for Windows Server, RDP session hosts, and roaming-profile environments, where the
profile directory is regularly redirected, unavailable, or read-only. On an ordinary desktop the
first candidate always wins and the rest never runs.

Writes are **atomic**: serialize to a temporary file in the target directory, flush, then rename
over the destination. A crash or a full disk mid-write leaves the previous good file intact rather
than a truncated one.

If every candidate fails, storage degrades to an **in-memory mode** instead of returning a fatal
error. The app runs normally for the session, nothing is persisted, and the frontend shows a
warning banner. Refusing to start would be the wrong behaviour for a tool whose main job — typing
text into another window — does not need the disk at all.

## Frontend (`src/`)

The UI is three panels:

- **Type** — the text box, delay control, countdown display, progress readout, and Start / Stop.
  Subscribes to the typing engine's progress events.
- **Templates** — list, create, edit, and delete templates; clicking one loads it into the Type
  panel's text box.
- **Settings** — every setting from the settings module, plus the read-only data location.

State lives in the Rust core, not the frontend. The panels read it through commands on mount and
after mutations, rather than keeping a parallel copy that could disagree with what is on disk. The
one exception is the Type panel's text box, which is deliberately frontend-only and in-memory —
its contents are never persisted unless the user explicitly saves them as a template.
