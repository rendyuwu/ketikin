# Ketikin

> A simple cross-platform auto-typer.

[![License: MIT](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/rendyuwu/ketikin)](https://github.com/rendyuwu/ketikin/releases)

## What it does

Some consoles simply refuse to accept a paste. VM and hypervisor web consoles, KVM-over-IP
devices, RDP and VNC sessions, BIOS-style terminals, and a number of remote-support tools all
share the same limitation: there is no clipboard bridge between your machine and the screen you
are looking at. You end up retyping a long command, a config block, or a recovery key by hand,
one character at a time, hoping you do not make a typo halfway through.

Ketikin does that typing for you. You paste your text into Ketikin, press Start, click into the
target window while a countdown runs, and Ketikin types the text out as real keystrokes — exactly
as if it came from your keyboard. Because the input arrives as ordinary keystrokes, anything that
accepts a keyboard accepts Ketikin.

Ketikin is built with Tauri v2: a Rust core for the keystroke injection and storage, and a
React + TypeScript frontend for the interface. It runs on Windows, Linux, and macOS.

## Features

- **Type panel** — a large text box to paste into, a per-keystroke delay setting, a configurable
  countdown before typing begins so you have time to focus the target window, a live progress
  readout, and Start / Stop controls.
- **Templates** — save the snippets you use over and over as a name plus content. Click a template
  to load it into the text box; edit or delete templates at any time. Everything is stored locally
  as JSON.
- **Settings** — typing delay, countdown length, theme, tray behaviour, always-on-top, global
  hotkeys, newline handling, automatic update checks, and a read-only view of where your data
  lives on disk.
- **System tray** — show or hide the window and quit Ketikin straight from the tray icon.
- **Global hotkeys** — start and stop typing without switching back to the Ketikin window.
- **Auto-update** — Ketikin checks GitHub Releases, downloads a signed update, installs it, and
  restarts. Every update is signature-verified, and the whole thing can be switched off.

Ketikin deliberately does not do: usage statistics, cloud sync, accounts, macros or scripting,
or OCR. It types text into the focused window, and that is the whole job.

## Install

All builds are published on the
[GitHub Releases page](https://github.com/rendyuwu/ketikin/releases). Download the artifact that
matches your platform.

### Windows (x64)

| Artifact | What it is |
| --- | --- |
| `.msi` | Windows Installer package. Best for scripted or managed deployments. |
| `.exe` | NSIS installer. This is the one most people want. |

The NSIS installer supports both a per-machine and a per-user install. A per-user install writes
into your own profile and needs no administrator rights, which makes it the right choice on locked
down or shared machines. Ketikin itself does not require elevation to run — see
[Platform notes](#platform-notes) for the one case where running it elevated matters.

### Linux (x64)

| Artifact | What it is |
| --- | --- |
| `.AppImage` | Self-contained, runs anywhere. `chmod +x` it and run it. **The only Linux artifact that can update itself** — see [Auto-update](#auto-update). |
| `.deb` | Debian/Ubuntu package. Install with `sudo apt install ./ketikin_0.1.0_amd64.deb`. |
| `.rpm` | Fedora/RHEL/openSUSE package. Install with `sudo rpm -i` or your package manager. |

The `.deb` and `.rpm` pull in their dependencies automatically. For the AppImage you need these
packages present on the system:

```
libwebkit2gtk-4.1-0  libgtk-3-0  libayatana-appindicator3-1
```

If you want Ketikin to update itself on Linux, choose the AppImage. The `.deb` and `.rpm` will
tell you when a new version exists but cannot install it themselves.

### macOS

| Artifact | What it is |
| --- | --- |
| `.dmg` (x64) | Intel Macs. |
| `.dmg` (arm64) | Apple Silicon Macs. |

macOS 10.15 or later is required. Open the `.dmg` and drag Ketikin into Applications. Two things
to do before it will work — both are covered in [Platform notes](#platform-notes): grant
Accessibility permission, and get past the first-launch quarantine prompt.

## Quick start

1. **Paste your text** into the big text box on the Type panel.
2. **Set the delay** — how long Ketikin waits between keystrokes. Slower is more reliable on
   remote consoles that drop input; faster is fine for local windows.
3. **Press Start.** The countdown begins.
4. **Click into the target window** while the countdown runs. Whatever window has focus when the
   countdown ends is where the text goes.
5. **Watch the progress readout.** Press Stop — or your stop hotkey — if anything looks wrong.

The countdown is the safety mechanism here. Take the extra second to confirm the cursor is in the
right place before it expires.

## Templates

The Templates panel holds the snippets you find yourself typing repeatedly: a network config
block, a recovery command, a licence key, the same five-line bootstrap script you run on every new
VM.

- **Save** a template with a name and its content.
- **Click** a template to load its content into the Type panel's text box.
- **Edit** a template to change its name or content.
- **Delete** the ones you no longer need.

Templates live in `templates.json` alongside your settings, in the storage location described in
[Where your data is stored](#where-your-data-is-stored). Nothing is uploaded anywhere.

## Settings reference

| Setting | JSON key | Default | What it does |
| --- | --- | --- | --- |
| Typing delay | `typingDelayMs` | `25` | Milliseconds to wait between each keystroke, from 1 to 1000. Lower is faster, but some consoles begin dropping characters below about 15 ms. |
| Countdown | `startDelaySecs` | `3` | Seconds to wait after you press Start before typing begins, from 0 to 10, so you can click into the target window. `0` starts immediately. |
| Theme | `theme` | `"system"` | One of `"system"`, `"dark"`, or `"light"`. Follows the operating system's appearance by default. |
| Minimize to tray | `minimizeToTray` | `true` | Minimizing the window hides it to the system tray instead of the taskbar. |
| Close to tray | `closeToTray` | `true` | Closing the window hides it to the tray instead of quitting. Use Quit in the tray menu to actually exit. |
| Always on top | `alwaysOnTop` | `false` | Keeps the Ketikin window above other windows. |
| Global hotkeys | `hotkeysEnabled` | `true` | Master switch for the two hotkeys below. Turn it off to release both key combinations entirely. |
| Start hotkey | `startHotkey` | `"CommandOrControl+Alt+T"` | Starts typing the current contents of the text box without focusing Ketikin. Displayed as `Ctrl+Alt+T` on Windows and Linux, `Cmd+Alt+T` on macOS. |
| Stop hotkey | `stopHotkey` | `"CommandOrControl+Alt+X"` | Aborts a typing run in progress, and cancels the countdown if it is still running. Displayed as `Ctrl+Alt+X` / `Cmd+Alt+X`. |
| Newline handling | `newlineMode` | `"enter"` | How line breaks in your text are typed. `"enter"` presses Enter, which is what you want for a shell or console. `"shiftEnter"` presses Shift+Enter, useful in chat boxes and web consoles where Enter submits instead of inserting a line. `"skip"` drops line breaks entirely and types the text as one continuous line. |
| Auto-check for updates | `autoCheckUpdates` | `true` | Checks GitHub Releases on launch and roughly once an hour afterwards. See [Auto-update](#auto-update). |

Settings also shows a read-only display of the resolved data directory — that one is not a
setting, and it is described under
[Where your data is stored](#where-your-data-is-stored).

Both tray settings depend on the system actually providing a tray icon. If it is unavailable,
Ketikin ignores them rather than trapping you — see
[Troubleshooting](#troubleshooting).

`settings.json` is rewritten whenever a setting changes. Keys that are missing or unrecognised
fall back to the defaults above, so a file written by an older version — or one you edited by hand
— still loads. Values outside the ranges above are clamped when saved.

## Global hotkeys

Global hotkeys let you start and stop typing while the target window — not Ketikin — has focus.
That matters because Ketikin types into whatever is focused, so you generally do not want to have
to click back to Ketikin to stop it.

| Action | Windows / Linux | macOS |
| --- | --- | --- |
| Start typing | `Ctrl+Alt+T` | `Cmd+Alt+T` |
| Stop typing | `Ctrl+Alt+X` | `Cmd+Alt+X` |

The stop hotkey also cancels the countdown, so it works as an abort at any point between pressing
Start and the last character being typed.

Both combinations can be rebound in Settings, and the whole feature can be disabled there if you
would rather not have Ketikin hold on to a global key combination. Bindings are written as
accelerator strings: one or more modifiers joined to a key with `+`. `CommandOrControl` maps to
Cmd on macOS and Ctrl everywhere else, which is why one default covers all three platforms. The
other modifiers are `Alt`, `Shift`, and `Super`.

```
CommandOrControl+Alt+T
Alt+Shift+F9
Super+K
```

A rebind takes effect as soon as you save it. Global hotkeys are exclusive — if the combination is
already claimed by another application or by the operating system itself, registration fails,
Settings shows an inline error next to that field, and your previous binding stays active. Nothing
is silently lost; pick a different combination and save again.

## Auto-update

When automatic update checks are enabled, Ketikin asks GitHub Releases whether a newer version
exists — once on launch, and roughly once an hour while it is running. If one does, it downloads
the update, verifies it, installs it, and restarts into the new version.

Updates are signature-verified with [minisign](https://jedisct1.github.io/minisign/). Each release
artifact is published with a detached signature produced by a private key that only the maintainer
holds, and the matching public key is compiled into the Ketikin binary. An update that does not
carry a valid signature for that key is rejected and never installed. This means a compromised
download mirror or a tampered release file cannot push code into your installation.

To turn it off, switch off **Auto-check for updates** in Settings. The setting is re-read before
every check, so switching it off takes effect immediately — no restart needed — and Ketikin will
not contact GitHub on its own again. You can still check for an update on demand from Settings
whenever you want one, or simply download a new build from the Releases page.

### Linux: only the AppImage self-updates

Self-installation on Linux works only when Ketikin is running as an **AppImage**. The updater
replaces the running image in place, which it locates through the `APPIMAGE` environment variable.
A `.deb` or `.rpm` installation has no such image to replace — its files are owned by the system
package manager — so it cannot update itself.

Ketikin handles this honestly rather than failing at the last step. On a `.deb` or `.rpm` install
it still detects and announces a new version, but instead of offering to restart and install, it
gives you a link to the release along with a short explanation. You then update the same way you
installed: download the new `.deb` or `.rpm` and install it over the old one.

If you want hands-off updates on Linux, use the AppImage. Windows and macOS self-update normally
and are not affected by any of this.

## Where your data is stored

Ketikin writes two files: `settings.json` and `templates.json`.

Rather than assuming one fixed directory exists and is writable, Ketikin tries a chain of
locations in order and uses the first one it can actually write to. It then remembers which one it
picked.

1. **The OS application-data directory**
   - Windows: `%APPDATA%\com.rendyuwu.ketikin`
   - Linux: `~/.local/share/com.rendyuwu.ketikin`
   - macOS: `~/Library/Application Support/com.rendyuwu.ketikin`
2. **`%APPDATA%`** — Windows only, read directly from the environment variable.
3. **`%LOCALAPPDATA%`** — Windows only, read directly from the environment variable.
4. **A `data` folder next to the Ketikin executable** — for portable installs and locked-down
   machines where the user profile is off limits.
5. **The system temp directory** — a last resort. Data stored here may not survive a reboot.

Writes are atomic: Ketikin writes to a temporary file first and then renames it into place, so an
interrupted write cannot leave you with a truncated or corrupt `templates.json`.

If every location in the chain fails, Ketikin does not crash. It keeps running with your settings
and templates held in memory for the session and shows a warning banner so you know that nothing
will be persisted.

This fallback chain is the reason Ketikin works on **Windows Server, RDP session hosts, and
roaming-profile setups**, where the user profile directory is frequently unavailable, redirected,
or read-only. On a normal desktop you will never notice it; on a jump box you will.

Settings displays the path that was actually resolved and which entry in the chain it came from,
so you can confirm at a glance where your data went and whether Ketikin had to fall back.

## Platform notes

### Windows

The NSIS installer offers both a per-machine and a per-user install, and the per-user option needs
no administrator rights.

Ketikin does not need to run elevated. There is one important exception. Windows enforces User
Interface Privilege Isolation (UIPI): a process at a lower integrity level cannot send synthetic
input to a window owned by a process at a higher one. If your target window is running elevated —
an administrator command prompt, some management consoles, certain vendor tools — and Ketikin is
not, Windows silently discards the keystrokes. Nothing errors; nothing appears. The fix is to run
Ketikin as administrator as well, so both processes sit at the same integrity level.

### Linux

Keystroke injection requires X11, either a native X11 session or XWayland. Under a native Wayland
session, the compositor's security model blocks synthetic input from an unprivileged application,
and Ketikin may not be able to type at all. If you are on Wayland and nothing happens, log in to
an X11 session instead.

Runtime dependencies:

```
libwebkit2gtk-4.1-0  libgtk-3-0  libayatana-appindicator3-1
```

### macOS

Ketikin needs **Accessibility** permission before it can type. Open
**System Settings > Privacy & Security > Accessibility**, then add and enable Ketikin. macOS will
usually prompt for this the first time you press Start; if you dismissed the prompt, add it
manually. Typing will silently do nothing until this is granted.

macOS builds are ad-hoc signed and are **not notarized**. On first launch, Gatekeeper will refuse
to open the app from a double-click. Either right-click the app and choose **Open** (then confirm),
or clear the quarantine attribute from a terminal:

```bash
xattr -dr com.apple.quarantine /Applications/Ketikin.app
```

You only need to do this once.

## Troubleshooting

**Nothing is typed on Windows.**
The target window is almost certainly running elevated while Ketikin is not, and Windows UIPI is
dropping the keystrokes without any error. Restart Ketikin as administrator. If the target is not
elevated, check that the countdown finished with the correct window focused.

**Nothing is typed on macOS.**
Accessibility permission has not been granted. Go to
**System Settings > Privacy & Security > Accessibility** and enable Ketikin. If Ketikin is already
listed but still does nothing, remove it from the list, add it again, and restart the app.

**Nothing is typed on Linux.**
You are likely in a native Wayland session, where synthetic keystroke injection is blocked. Log in
to an X11 session (or one with XWayland available) and try again. Ketikin talks to the X server
directly, so there is no separate input package to install — if there is no X server or XWayland
to talk to, there is nothing for it to type into.

**Settings or templates are not being saved.**
Open Settings and look at the data location it reports. If it shows the system temp directory, the
earlier locations were not writable and your data will not survive a reboot — check permissions on
your application-data directory. If a warning banner is showing instead, no location was writable
at all and Ketikin is running purely in memory for this session.

**On Linux, an update is announced but there is no way to install it.**
This is expected on a `.deb` or `.rpm` install. Only the AppImage can replace itself in place, so
package-manager installs notify you about a new version and link to it rather than offering to
install it. Download the new package and install it the way you installed the old one, or switch
to the AppImage if you would rather updates be automatic. See
[Linux: only the AppImage self-updates](#linux-only-the-appimage-self-updates).

**No tray icon appears, or closing the window quits instead of hiding it.**
The tray icon needs the desktop to provide a system-tray host — on Linux, a StatusNotifier host.
Not every desktop ships one; bare GNOME without an AppIndicator extension is the usual case.
Installing an AppIndicator / StatusNotifier extension restores it.

When the tray icon cannot be created, Ketikin deliberately ignores both **minimize to tray** and
**close to tray** for that session, so closing the window exits normally, and it shows a banner
explaining why. This is on purpose: with close-to-tray enabled and no tray icon, closing would
hide the window with no way to bring it back or quit. The non-obvious part is that your saved
settings are left untouched — Ketikin overrides the behaviour at runtime without rewriting your
preferences, so both settings come back as you had them the next time you run on a system where
the tray works.

**A hotkey does nothing.**
Another application already owns that key combination. Global hotkeys are exclusive to whichever
process registers them first, and there is no way for Ketikin to take one that is already taken.
Rebind the hotkey in Settings to something else. If registration fails when you save, Settings
shows an inline error next to that field and keeps your previous binding — so if a rebind appears
to have no effect, check for that error before assuming the new combination is active. Also
confirm the global hotkeys master switch is on.

**Characters come out wrong or in the wrong order.**
Increase the typing delay. Remote consoles, KVM-over-IP devices, and high-latency RDP sessions
regularly drop or reorder input that arrives faster than they can process it.

## Building from source

**Prerequisites**

- Rust, stable toolchain
- Node.js 22
- Your platform's Tauri build dependencies — see [CONTRIBUTING.md](CONTRIBUTING.md) for the exact
  package list per platform, including the `apt` line for Ubuntu.

**Commands**

```bash
npm install          # install frontend dependencies
npm run tauri dev    # run in development with hot reload
npm run tauri build  # produce a release build for the current platform
```

Build output lands in `src-tauri/target/release/bundle/`.

## Security and responsible use

Ketikin simulates keystrokes into whichever window currently has focus. That is a genuinely
powerful capability, so a few plain words about it:

Do not use Ketikin to get input into a system you are not authorized to use. A console that
refuses paste is not an access control, but plenty of things around it are, and the fact that a
tool can type somewhere does not mean you are allowed to.

Be deliberate when the text you are about to type is a secret. Ketikin types into whatever is
focused, and if the wrong window is focused, your credential goes there instead. The countdown
exists precisely so you can look at the screen and confirm the target before anything is sent.

The contents of the Type panel's text box are never written to disk. They exist only in memory for
as long as the app is running, unless you explicitly save them as a template — in which case they
are stored in plaintext in `templates.json`, so think twice before saving a password as one.

To report a security vulnerability, see [SECURITY.md](SECURITY.md).

## Contributing

Bug reports, feature discussion, and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project layout, the checks to run before
you push, and commit conventions.

For a map of how the app is put together, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 rendyuwu.
