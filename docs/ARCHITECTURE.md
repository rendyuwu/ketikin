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
| Settings | The settings model, its defaults, and range validation. Missing or unrecognised keys fall back to defaults, so older and hand-edited files still load. Saving clamps out-of-range values and returns the normalized settings. It does not apply them: theme is applied by the frontend, and always-on-top and hotkey re-registration are driven from `lib.rs`. |
| Templates | CRUD over the saved snippets, persisted through storage. |
| Typing engine | Turns a block of text into keystrokes. Owns the countdown, the per-keystroke delay, newline handling, and the cancellable worker. |
| Hotkeys | Registers and unregisters the global start and stop accelerators and maps them onto the typing engine's start and stop. Registration can fail when another process already owns a combination; the failure is surfaced to the UI and the previous binding is kept. |
| Tray | Builds the tray icon and its menu (show/hide the window, quit), and intercepts window close and minimize when the tray settings call for it. Also swaps the icon for a run-state variant while typing, twice per run, driven from the typing engine's `RunGuard` — the tray is the only part of Ketikin still on screen once the window is hidden. If the icon cannot be created — no StatusNotifier host on the desktop — both tray settings are ignored for the session so the window can never become unreachable, and the UI shows a banner. Persisted settings are not rewritten. |
| Icons | Windows only. Tauri's build-time codegen turns `icon.ico` into a single raster, but Windows draws the titlebar, the notification area, Alt+Tab and the taskbar at sizes that follow the display scale. This module reads the discarded `.ico` entries back at runtime and picks one per surface, at startup and on every `ScaleFactorChanged`. Conservative by design: it only replaces the window icon with an entry that is at least as good for *both* the small and the large size, so nothing that reads the icon can come out worse. |
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
4. A `data` folder beside the executable — portable and per-user installs
5. The system temp directory — last resort, may not survive a reboot

The chain is shallower than five entries suggests, because on Windows several candidates share a
failure mode:

- Candidate 1 already resolves to a subdirectory of `%APPDATA%`, so candidate 2 adds nothing there:
  if `%APPDATA%` is unwritable or unset, both fail together.
- Candidate 5 is not independent of candidate 3 on Windows. `std::env::temp_dir()` goes through
  `GetTempPath()`, which normally yields `%LOCALAPPDATA%\Temp` — inside the candidate 3 root. An
  ACL that rejects candidate 3 will usually reject temp for the same reason. On Linux and macOS
  `/tmp` is a genuinely separate location, so there candidate 5 is a real backstop. This is also
  why candidates 2 and 3 are not `#[cfg]`-gated to Windows: leaving them in the list on every
  platform keeps the fallback path exercised on developer machines.
- Candidate 4 is install-dependent. `tauri.conf.json` sets `nsis.installMode: "both"`, so a
  per-machine install resolves it to `C:\Program Files\Ketikin\data`, which a standard user cannot
  create. It is only a real candidate for portable and per-user installs.

So the recovery that genuinely carries Windows Server, RDP session hosts, and roaming-profile
environments is candidate 3: a different root on local disk that survives when the roaming profile
is redirected, unavailable, or read-only. When the profile is denied outright, the tail of the
chain goes with it and storage lands in in-memory mode — degraded, but announced rather than
silent, which is the property that matters. On an ordinary desktop the first candidate wins and
the rest never runs.

Writes are **atomic**: serialize to a temporary file in the target directory, flush, then rename
over the destination. The guarantee this buys is that a crash, power loss, or full disk *during a
write* leaves the previous file intact instead of truncating it — the rename either happens or it
does not. It is not a guarantee against every form of corruption: a file damaged out from under the
app, or a filesystem that reorders the rename against the data, is outside what this protects.

If every candidate fails, storage degrades to an **in-memory mode** instead of returning a fatal
error. The app runs normally for the session and nothing is persisted. Refusing to start would be
the wrong behaviour for a tool whose main job — typing into another window — does not need the disk.

Storage reports its result as a path, the chain entry that produced it, an optional error, and a
list of notices. The UI splits that into two channels deliberately, and `StorageInfo::degraded` is
the single owner of the rule — derived in `Storage::info()` rather than stored, so it cannot go
stale behind a later notice, and consumed by the frontend directly rather than reconstructed from
`source` / `writable` / `notices`.

The **banner** fires for three things: the temp directory, in-memory mode, and any notice flagged
alarming. Today the only alarming notices are the temp location warning and a JSON file that had to
be reset — the latter fires on *any* source, including a healthy `appData`, because it means data
the user already had is gone. Deliberately it is *not* "notices is non-empty". Running from the
folder beside the executable always carries notices but raises no banner: it is a supported
portable deployment, and warning on every launch of a working install would train users to dismiss
warnings. Those notices — that the location may be shared with other users, and that the resolved
directory can depend on elevation — surface in **Settings > Storage**, which is the complete view.
Failure to create the `logs/` subdirectory is likewise Settings-only: it costs diagnosability, not
data.

Logging is anchored to the same resolved directory in a `logs/` subdirectory, so it follows the
chain wherever it lands. Two consequences: in-memory mode has no log at all, since logging falls
back to stdout and a Windows release build discards it; and the `logs/` subdirectory can fail to
be created even when the data directory is writable, because Windows grants file creation and
subdirectory creation separately. In both cases Settings > Storage is the only diagnostic channel,
which is why it reports when file logging is unavailable.

## Frontend (`src/`)

The UI is three panels:

- **Type** — the compose area, the cadence control, the countdown takeover, and Start / Stop.
  Subscribes to the typing engine's progress events. The cadence slider and the numeric delay field
  are two views of the same setting: both write through `settings.update`, and the slider's discrete
  stops mean a whole drag lands inside one 400 ms debounce window.
- **Templates** — list, create, edit, and delete templates; clicking one loads it into the Type
  panel's text box.
- **Settings** — every setting from the settings module, plus the read-only data location.

The run's progress indicator is deliberately **not** in the Type panel: it is a 3px rail fixed to
the window's top edge, rendered by `App` because that is the only component outside the panel's
content box. Ketikin is behind another window for the whole time it is typing — the user starts a
run and clicks into a KVM console — so an indicator inside the content area is one that never gets
read. It is positioned rather than laid out so that appearing and disappearing with a run cannot
shift the interface, and only its width animates, because `typing://state` arrives at ~20 events a
second.

The Rust core is the authority for persisted state, but the Settings panel does not wait on it.
`useSettings` keeps optimistic local state: an edit updates the UI immediately and schedules a save
behind a 400 ms debounce, so a user dragging a slider does not generate a write per frame. During
that window the frontend deliberately holds a value the backend has not seen yet. When the save
lands, the backend returns the normalized settings — clamped to the documented ranges — and the UI
re-renders from that response, which is what makes an out-of-range entry snap back to the limit.
The backend's answer wins; the local copy is a latency hiding measure, not a second source of
truth.

The Type panel's text box is different again: it is frontend-only and never sent to storage unless
the user explicitly saves it as a template.
