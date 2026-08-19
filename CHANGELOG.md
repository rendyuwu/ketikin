# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **The tray icon shows when a run is going.** Ketikin's whole premise is that you are not looking at
  it while it types — you are watching the console it is typing into — and both of the settings that
  hide the window to the tray are on by default, so the usual state of a run is that the tray icon is
  all of the app you can see. It said nothing: idle and mid-run looked identical. The icon now gains a
  mark for the length of a run, countdown included, and loses it again when the run finishes, is
  stopped or fails. It is one shape added to the same keycap rather than a second icon — a dot knocked
  out of the key above the caret — so it reads as the same app in a different state, survives 16
  pixels, and works in the macOS menu bar, where the icon is a single tint and colour could not have
  carried it. The mark is set and cleared by the same guard that guarantees the run's terminal event,
  so a run that panics cannot leave the tray claiming to be busy.
  ([#16](https://github.com/rendyuwu/ketikin/issues/16))

### Fixed

- **You can now see where every control ends.** The edge of every input, button, hotkey field,
  select, slider and switch was drawn in a grey that measured 1.87:1 against the surface behind it on
  light and 1.81:1 on dark, well under the 3:1 that WCAG 2.2 asks of the boundary of a user interface
  component. The switches were the worst of it: they carry no border, so the whole off-state track
  *was* that colour — a 30×18 shape with nothing else to say it was there. Rather than darken the one
  token and turn every divider in the app into a box with it, control boundaries now have their own
  token: `--control-edge`, at 3.42:1 on light and 3.41:1 on dark, measured against the sunken surface
  too because a button inside the template form or the storage panel sits on that one. The hairline
  under a section label and the line between two template rows are untouched, and the notice banner,
  the storage panel and the dialog keep the old stroke — they are surfaces, not controls. The
  template body field is included even though it was on the hairline: its sunken fill is 1.06:1
  against the surface around it, so that 1.30:1 stroke was the only thing marking out a field the
  user is asked to type into. ([#15](https://github.com/rendyuwu/ketikin/issues/15))
- **The app and tray icon are legible at the size they are actually seen at.** At 16 pixels — the
  titlebar and the system tray, which is where this icon spends nearly all of its life — the old one
  rendered as a blue smudge. It was four nested shapes with three concentric outlines, and at that
  size the keycap's outline, the inset behind it and the arms of the text cursor all measured under
  one pixel. It is now a single brass keycap with the caret *knocked out* of it rather than drawn on
  it, because a hole has no line weight to lose, and the whole thing is drawn on a 16-pixel grid and
  scaled up instead of drawn at 1024 and scaled down — every size ships as a render of that grid
  rather than a resample of the big one, the 16px one included. The brass is the same brass the
  interface uses; the icon used to be a third blue that matched neither theme's accent. macOS now
  gets a real menu bar template as well — black plus alpha, tinted by the system for a light or dark
  bar — instead of a colour tile sitting among the monochrome icons of every other app.
  ([#11](https://github.com/rendyuwu/ketikin/issues/11))
- **The titlebar now follows the theme you picked.** Choosing **Light** restyled the app but left
  the native titlebar dark, so a black bar sat on top of a white window on every tab. The theme
  setting only ever reached the WebView, and the frame around it is drawn by the OS, which was never
  told anything. It is now pushed to the window itself — at launch as well as on change, so a stored
  Light no longer waits for you to touch the control — and **System** hands the frame back to the OS
  rather than pinning it, so it keeps following along when you switch the desktop theme with Ketikin
  running. On Linux the frame belongs to the window manager, which may decline.
  ([#12](https://github.com/rendyuwu/ketikin/issues/12))
- **Pinning the theme to Dark no longer keeps the washed-out grey.** The contrast fix for the
  faintest text landed in the palette used when Theme is set to **System**, but not in the one used
  when it is pinned to **Dark**, so anyone who chose Dark explicitly still saw field hints,
  placeholders and the `TYPING` / `WINDOW` / `HOTKEYS` labels at 4.23:1, under the 4.5:1 minimum.
  Both dark palettes now carry identical values.
  ([#4](https://github.com/rendyuwu/ketikin/issues/4))
- **The faintest text in the app is now readable.** Field hints, input placeholders, the
  `TYPING` / `WINDOW` / `HOTKEYS` labels in Settings and the "Press a key combination…" prompt were
  drawn in a grey that missed the WCAG AA contrast minimum in both themes — 3.34:1 on the light
  background and 4.23:1 on the dark one, against the 4.5:1 required at the size they are rendered
  at. That is most of the explanatory copy in the app, and on white it was visibly washed out. Both
  values now clear the minimum. Switches also show a tick when they are on, so whether one is
  enabled no longer depends on being able to tell the accent colour apart from the track. The
  contrast half came from [@aditya226-sharma](https://github.com/aditya226-sharma) in
  [#14](https://github.com/rendyuwu/ketikin/pull/14).
  ([#4](https://github.com/rendyuwu/ketikin/issues/4))
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

- **A template is now used by clicking its row.** Every item in the Templates list carried a `Use`, an
  `Edit` and a `Delete` button on a line of their own — three controls for an item two lines tall —
  and the row itself did nothing when clicked, even though using the template is the intent almost
  every time. Clicking a row, or pressing Enter on it, now loads that template into the Type tab.
  **Edit** and **Delete** have become small icon buttons that appear when the row is hovered *or*
  when the keyboard reaches them, so they are still usable with no mouse at all. Delete stays grey
  until you point at it: it already asks for confirmation, and a list where every row carries a red
  button reads as a page full of hazards. The `1 template` counter is gone — the list is the count —
  and the head now says **Templates**. The empty state says what templates are for in one line and
  offers the one action. The three buttons that leave the app for somewhere else — **Release notes**
  in both places it appears, and **Open data folder** — now say so with an icon.
  ([#10](https://github.com/rendyuwu/ketikin/issues/10))
- **You can now see a run's progress with Ketikin's window almost entirely covered.** Progress used
  to be a 4px bar inside the panel and one line of small centred text, both of which disappeared the
  moment you clicked into the window you were typing into — which is every time, since that is what
  the app is for. It is now a brass rail along the very top edge of the window, so a sliver of
  Ketikin showing behind a KVM console or an iDRAC screen is enough to tell you how far along it is.
  The countdown has become a change of mode rather than a line of text: the text you are about to
  send dims, the number takes the centre of the window at four times its old size, and **Click into
  the target window.** sits under it. The header's counter uses fixed-width digits, so it no longer
  jitters as it climbs. **Stop** is still red, and now shows its shortcut too.
  ([#9](https://github.com/rendyuwu/ketikin/issues/9))
- **The Type panel now asks how careful you want to be, not how many milliseconds.** The bottom half
  of the panel was four stacked rows doing one job — a meta line, a delay field, the button, and a
  standing footnote — and the delay was stated twice in two of them. It is now one block. **Delay
  (ms)** has become **Cadence**: a slider with three named stops, **Careful**, **Normal** and
  **Fast**, because nobody knows what 25 ms feels like until they have ruined one paste into a
  production console. The millisecond field is still there beside it and still editable, and it is
  still the way to reach any value in the full range; the backend remains the authority on what is
  in range. The estimate of how long the run will take is now the largest thing in the footer rather
  than 12px grey text at the end of a sentence, since it is the only question you actually have
  before pressing Start. The text box has lost its border and its grey fill — what you paste is the
  content of this screen, not an entry in a form — and its empty state says **Paste what Ketikin
  should type.** on the line the cursor is on. The permanent **Click into the target window during
  the countdown.** footnote is gone; it appears during the countdown, which is the only time it is
  possible to act on. The keyboard shortcut for **Start typing** and **Stop** is now drawn inside
  each button, and is omitted when global hotkeys are switched off or the shortcut could not be
  registered, so it never advertises a key that does nothing.
  ([#8](https://github.com/rendyuwu/ketikin/issues/8))
- **Brass instead of blue, and the app now brings its own typeface.** Three different blues used to
  ship in one product, and blue is the most default accent in software. The accent is now a warm
  brass, and it is spent in exactly four places — the **Start typing** button, the active tab's
  underline, the countdown, and the progress bar — so that when Ketikin is working, the parts that
  say so are the only coloured things on screen. Everything else is graphite: switches, links and
  the focus ring no longer borrow the accent, which means an enabled switch is no longer the same
  colour as the button that starts a run. Both themes put near-black text on brass rather than white,
  which white cannot do at any brass light enough to still look like brass. Red is untouched, because
  red is right for **Stop**. The interface is also set in IBM Plex Sans and IBM Plex Mono, bundled
  with the app (about 88 KB, no network request, no change offline), so Ketikin reads the same on
  Windows, macOS and Linux instead of borrowing whatever the platform's UI font happens to be.
  ([#7](https://github.com/rendyuwu/ketikin/issues/7))
- **Notification banners have two tones instead of three.** The blue "info" and amber "warning" tints
  are gone, replaced by one neutral **notice** style; error banners stay red. The amber in particular
  had to go: it was brass's own family, so every warning read as accented and competed with the
  primary action. What a banner is about is carried by its wording and by the action it offers.
  ([#7](https://github.com/rendyuwu/ketikin/issues/7))
- **The controls no longer look a decade older than the app.** The boxed segmented tab strip is gone
  and so is the wordmark that repeated what the titlebar already said: the tabs are plain text with
  an underline under the active one, sharing a single row with the typing status, which gives about
  40px of vertical space back to a 700px-tall window. The **Newline handling** and **Theme**
  dropdowns are drawn like every other control instead of arriving with the platform's own chrome and
  font metrics, and they size to their options rather than stretching across the panel — the native
  popup they open is unchanged. Switches are smaller and flatter, scrollbars are thin and quiet in
  every panel, `TYPING` / `WINDOW` / `HOTKEYS` each head a hairline rule instead of floating above
  their group, and the delay and countdown fields carry their unit inside the field so their labels
  are simply **Delay** and **Countdown**. The standing "Changes save automatically." line has been
  dropped, since the **Saved** flash says the same thing at the moment it is true. Tab keyboard
  navigation is exactly as capable as before.
  ([#6](https://github.com/rendyuwu/ketikin/issues/6))
- **Spacing and text sizes now group the interface instead of flattening it.** Every distance on
  screen used to be the same 12px, so nothing read as belonging together, and six text sizes were
  crowded into a 4px range where two of them were indistinguishable. There are now three text sizes
  and three distances with distinct jobs: a label sits close to its control, controls that form a
  group sit closer to each other than to the next group, and each Settings section is separated by a
  clear band — so `TYPING`, `WINDOW` and `HOTKEYS` label the group beneath them instead of floating
  between two. Template names and dialog titles are larger, buttons and inputs have a slightly
  crisper corner while dialogs and panels have a softer one, and the delay and countdown fields use
  fixed-width digits so the value stops shifting sideways as you type.
  ([#5](https://github.com/rendyuwu/ketikin/issues/5))
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
