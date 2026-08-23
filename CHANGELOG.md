# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **The Templates panel no longer names itself twice.** A `Templates` head sat about 56 pixels below
  a tab strip whose selected tab also read `Templates`, and after the section-head pass
  ([#22](https://github.com/rendyuwu/ketikin/issues/22)) the two were the same colour, one weight
  apart — neither told you anything the other did not. That is the argument the `.header` comment
  already makes about a wordmark under a titlebar that carries one, applied a level down in a window
  whose minimum height is 560px. The head row stays, because it was quietly doing a second job: the
  eyebrow's trailing hairline grew to fill the row, and that grow was the only thing pushing the New
  button to the right edge. The row now states its own alignment in one property, so New still lands
  in the same column as the Edit and Delete buttons on the rows below it. No hairline replaces the
  one that went with the label: the header's bottom border is 20px above the row and is the app's
  only chrome-to-content divider, so a second line that close to it would have had nothing to
  introduce. Measured in the built frontend at 460x560 and 560x700, in the list state and the empty
  state, every coordinate is unchanged — the row's height was always set by the button, not the
  label, so this buys back no vertical space and was never about the pixels. The five Settings heads
  and the template form's own `New template` / `Edit template` head are untouched and keep their
  hairlines; that last one names something that appears nowhere else on screen, which is what the
  class is for. ([#28](https://github.com/rendyuwu/ketikin/issues/28))

- **Settings is one list at one rhythm, and the row under the pointer lights up.** Giving fields the
  same row shape as the switches ([#33](https://github.com/rendyuwu/ketikin/issues/33)) left the old
  spacing rule behind it: 8px between two consecutive switches, 20px between everything else. That
  pair was written when a field was a stacked form control and a switch was a row, where 8px said
  "these three switches are one group" and 20px kept a field's label line clear of the one above it.
  Once every setting was a row it read as two rhythms inside one list — and the Window section runs
  one straight into the other, a select at 20px followed by three switches at 8px. Every row now sits
  at the tighter of the two. Grouping was never this distance's job and has not been for a while: the
  section eyebrow names the group, its hairline draws the top of it, and 32px separates one section
  from the next, so a fourth signal was only spending height. Measured in the built frontend at the
  460x560 minimum window, the scroll column goes from 983px to 907px — 2.20 viewports of content down
  to 2.03. The 8px between two rows is now inside them as padding rather than between them as margin,
  which changes no coordinate but means a row's box covers its whole share of the column, and that is
  what the highlight paints: hovering a switch fills the full width of the panel and the next band
  begins exactly where that one ends. Only the switches take it. A switch is a `<label>`, so its whole
  row flips it; a field row is not clickable outside its own control, and lighting it up would promise
  a target that is not there. ([#35](https://github.com/rendyuwu/ketikin/issues/35))

- **The Windows titlebar and tray icon are now picked for the size Windows is about to draw them
  at.** Leading `icon.ico` with its 64px entry ([#24](https://github.com/rendyuwu/ketikin/issues/24))
  made every size a downscale rather than an upscale, but only 100% and 200% scaling are reached
  *cleanly* from 64: the drawing is on a 16-unit grid, so 64 halves exactly to 32 and to 16 and lands
  every edge back on a pixel boundary. At 150% — an ordinary Windows laptop setting — the titlebar and
  the notification area want 24px, which was a 2.67:1 resample of that buffer, while the file had held
  a purpose-drawn 24px entry all along that nothing at runtime could reach. Ketikin now embeds the
  whole `.ico`, parses its entry table back out at startup and on every `ScaleFactorChanged` (which
  also fires when the window moves to a monitor with a different scale), and hands each surface an
  entry chosen for it. The two surfaces get different rules, because they are read differently: a tray
  icon is drawn in the notification area and nowhere else, so it simply takes the exact size or the
  next one up, but the window's icon is the only one `tao` sets — `ICON_BIG` is never assigned and the
  window class registers a null icon — so whether Alt+Tab and the taskbar button fall back to it
  cannot be observed from inside the process. The window rule therefore only accepts an entry that is
  at least the large size Windows asks for *and* an exact integer multiple of the small one, which
  makes every swap an improvement on both surfaces or no swap at all. Against this file that means
  150% moves to the 48px entry — exact 2:1 for the titlebar's 24 and an exact match for Alt+Tab's 48 —
  and 100%, 125%, 175% and 200% keep the 64px entry they were already served correctly by. 125% and
  175% cannot be fixed by any raster: 20 and 28 pixels are 1.25 and 1.75 grid units each, so no render
  of this drawing is crisp there. The run-state tray mark ships at 16, 24, 32 and 48 to match, so the
  icon does not soften when a run starts. All of this is Windows-only; macOS resolves its own sizes out
  of the `.icns` and the menu bar template, and on Linux the tray artifact belongs to the
  StatusNotifier host. ([#37](https://github.com/rendyuwu/ketikin/issues/37))

### Fixed

- **A screen reader now reads which shortcut a hotkey field is bound to.** The two hotkey buttons in
  Settings carried an `aria-label` of "Start typing hotkey" and "Stop typing hotkey", and an
  accessible name taken from an attribute replaces the element's content — so the accelerator drawn
  inside the button, the one thing a reader needs from that control, was the one thing it never said.
  Dropping the attribute is not enough on its own: the field's own `<label>` names a `<button>` too
  and a label outranks the button's content, which leaves "Start typing" and still no accelerator.
  The button now names itself from both, by pointing at the field label and at the span holding the
  value, so it reads as "Start typing Alt+K". That also holds through capture, when the value is
  hidden to keep the button from resizing: a hidden element referenced by name still contributes its
  text, so the name does not flicker to "Press a key combination…" and back on every focus. The
  prompt is a description instead, announced when focus arrives — which on this control is the moment
  capture begins. ([#35](https://github.com/rendyuwu/ketikin/issues/35))

- **The Windows titlebar and tray icon are no longer a small raster blown up.** `icon.ico` ships six
  purpose-drawn sizes, but only one of them ever reaches the running window: Tauri's build-time
  codegen decodes `entries()[0]` and discards the rest, so whichever entry happens to be physically
  first in the file becomes the icon at every display scaling
  ([tauri-apps/tauri#14596](https://github.com/tauri-apps/tauri/issues/14596)). That entry was the
  32px one, so above 100% scaling Windows was enlarging a 32px drawing — the exact smearing that
  redrawing the icon for small sizes ([#11](https://github.com/rendyuwu/ketikin/issues/11)) existed
  to remove. The file now leads with its 64px entry, so every size Windows asks for is reached by
  scaling down from a purpose-drawn raster rather than up from a small one. 64 rather than 48, which
  was the other candidate: the drawing is on a 16-unit grid with every coordinate a whole unit, so 64
  halves exactly to the 32 and 16 that 100% scaling asks for and each edge lands back on a pixel
  boundary, where 48 would have reached 32 at 1.5:1 and made the most common configuration softer in
  order to be exact at 150%. All six entries stay in the file byte for byte, so the Explorer, Start
  Menu and shortcut icons — which read the whole group and pick their own size — are untouched. The
  order is the kind of thing a regeneration undoes silently, so the reasoning is written down in
  `src-tauri/icons/README.md` and a test asserts it.
  ([#24](https://github.com/rendyuwu/ketikin/issues/24))

- **The scrollbar is no longer the loudest thing on the screen.** Its thumb was filled with the token
  that draws the boundary of a control, which is held at 3.4:1 so the edge of a button cannot be
  missed — a value the scrollbar inherited by accident of naming rather than by decision. In Settings
  that put a permanently visible bar at 3.4:1 down the full height of a panel whose content is
  deliberately quiet, because the section list always overflows a 700px window. The thumb now has a
  token of its own, translucent rather than solid, so it tints whatever surface it is over: 1.46:1 at
  rest and 2.11:1 while the pointer is over the list (1.59:1 and 2.69:1 on dark) — above the app's
  quietest divider, well below its faintest text. It is drawn as a 4px pill inside a 10px hit area,
  so it stays thin to look at and full width to drag; hiding it was considered and rejected, since
  that costs both the only sign that a list continues below the fold and pointer-dragging as a way to
  scroll. Settings also holds the bar's width open whether or not it is showing, so its five switches
  no longer step sideways when the window is resized past the point where the list overflows. Fixing
  this turned up two further bugs in the old rule, both now gone: `scrollbar-width: thin` was
  declared on `body` and does not inherit, so it reached no scroll container at all, and setting
  `scrollbar-color` makes current Chromium ignore `::-webkit-scrollbar` rules — so on Windows the app
  had been drawing a full-width native scrollbar rather than the narrow one the stylesheet described.
  ([#20](https://github.com/rendyuwu/ketikin/issues/20))

- **Clicking into the box no longer draws a black rectangle around it.** One rule set the focus
  indicator for the whole app — a 2px ring in near-black, offset 2px clear of whatever it surrounded —
  which is correct on a filled button and wrong on anything you type into. The compose canvas carries
  no border and fills the Type panel, so focusing it, the single most common action in the app, boxed
  the entire screen; and the delay, countdown, newline, theme, hotkey and template fields each got
  that ring a second boundary out from the 1px edge they already had, which is the two concentric
  squares. Those fields now gain weight rather than a ring: the control's own edge goes to
  full-strength text and an inset band doubles it, so the boundary reads 2px while focused, sits
  inside the control's own 6px corner, moves nothing by a pixel, and cannot be clipped by the scroll
  container it sits in — a halo drawn outward would have been cut flat on the left of every field in
  Settings and on both sides of the two hotkey fields. The compose surface is marked on the hairline
  under it instead, which goes from the app's quietest divider at 1.34:1 to 17.2:1, and from one pixel
  to two, alongside the brass caret it already had. Buttons, tabs, switches, template rows, the
  cadence slider, links and icon buttons keep the ring exactly as it was — on the brass Start button a
  ring drawn without that offset measures 1.79:1 and is not there at all. Focused edges measure
  17.2:1 on light and 15.1:1 on dark, the change from the resting edge is 5.0:1 and 4.4:1, high
  contrast mode gets the outline back rather than nothing, and no ratio already recorded in the
  stylesheet moved. ([#21](https://github.com/rendyuwu/ketikin/issues/21))

- **Focus rings are no longer cut off at the edges of Settings and Templates.** Both panels scroll, and
  a scroll container clips what is inside it: `overflow-y: auto` computes `overflow-x` to `auto` as
  well, and the ring is drawn 2px outside the control it marks. Five controls sit flush against that
  edge — all five switches and the Reset button beside a hotkey field on the right, Check for updates on
  the left, the Delete button on a template row on the right, and New template in the empty state on the
  left — so each of them showed three sides of a ring instead of four. Both containers now hold 4px
  open, exactly the distance the ring reaches, and hand that width straight back to the panel, so
  nothing inside moves: every control's edge lands on the pixel it landed on before and no horizontal
  scrollbar appears. The one thing that does move is the scrollbar, 4px nearer the window edge, which
  gains it 4px of clearance from the content it used to sit against.
  ([#29](https://github.com/rendyuwu/ketikin/issues/29))

- **Tab can no longer escape the delete confirmation while the delete is running.** Both of its buttons
  are disabled for as long as the request is in flight, and a browser blurs a control the moment it is
  disabled — so focus fell to the document body, outside the dialog, where the dialog's own key handling
  never saw a keystroke. Tab then walked into the panel behind the scrim, and Escape stopped cancelling.
  Focus is now held on the dialog itself for the duration, and Tab is swallowed while there is nothing
  inside it to move to. ([#29](https://github.com/rendyuwu/ketikin/issues/29))

### Changed

- **Every setting in Settings is now the same kind of row.** A field stacked its label above its
  control while a switch put the label beside it, and the Window section ran a stacked Theme select
  straight into three switch rows — so one group changed shape halfway through with nothing to explain
  the change. That mismatch, rather than the absence of boxes, is what still read as a form after the
  section heads and corners were fixed. A field now takes the switch's shape: name on the left,
  control against the right edge, hint or error on its own full-width line underneath. It returns
  space rather than spending it, because the separate label line every field paid 27px for is gone —
  a field row is 33px instead of 60px, six of them come to 161px, and the scrolling column at the
  460x560 minimum window holds 983px of content where it held 1144px, against a 446px viewport. The
  96px number inputs and the 140px select no longer leave 300 to 440px of dead gutter to their right;
  their right edges line up with the switch tracks instead. The hotkey field, which used to stretch
  the full width of the panel, is now sized to hold either of the two things it displays — its
  accelerator, or `Press a key combination…` — so clicking into it no longer shoves itself and the
  Reset button beside it 110px sideways. Field labels also go from muted grey to full-strength text,
  which is what a switch's label always was: a muted **Theme** directly above a full-strength
  **Minimize to tray** ranks two rows that are peers.
  ([#33](https://github.com/rendyuwu/ketikin/issues/33))

- **The panel reads less like a form.** Every control had a 4px corner and a 1px border, nothing on
  screen sat above anything else, and the Settings section heads were 11px uppercase, letterspaced,
  in the faintest grey in the palette. Each was defensible on its own; together they were a 2016 web
  form rather than the instrument the rest of the design is aiming at. Corners are now derived rather
  than picked: the app's icon is a keycap drawn as a 12-unit square at `rx="2.5"`, 20.8% of its side,
  and a control in this app is between 29 and 33 pixels tall, which at that ratio is 6.0 to 6.8px —
  so a button takes 6px, the whole pixel inside that range, and the app's corner and the icon's
  corner are one decision instead of two. Panels and dialogs take 14px, which stays inside the 16px
  padding they already carry. **Typing**, **Window**, **Hotkeys**, **Updates**, **Storage** and the
  two heads in Templates are sentence case in full-strength text instead of small caps in the
  lightest grey available — of everything here that is the single largest change in how old the panel
  looks, and it is the treatment that dated it rather than the words or their size, so the size did
  not move. The confirmation dialog now carries one step of elevation in light mode, which is the one
  place in the app where something genuinely floats above something else; on dark the scrim already
  does that job and a black shadow against a near-black background would not have been visible
  anyway. And the switch thumb now travels over 160ms on a front-loaded curve rather than the 90ms
  the app uses for colour, because a shape that moves as fast as a fill changes colour reads as
  teleporting. The boundary of every control is still its own 3:1 stroke — no shadow stands in for a
  border — and no contrast ratio recorded in the stylesheet moved.
  ([#22](https://github.com/rendyuwu/ketikin/issues/22))

- **The cadence slider is focused on its thumb rather than boxed.** It was the last control still
  wearing the app-wide ring the change above replaced everywhere else, and the worst fit for it: a 2px
  near-black rectangle around the full width of an 18px-tall control whose visible part is a 2px
  hairline. The mark now goes where the state is — the 12px thumb, which is the part the arrow keys move
  and the only part carrying a value — as a 2px ring held 2px clear of the dot, which is the same
  clearance and the same weight the ring spends everywhere else, bent around a circle instead of a box.
  Nothing moves: it is drawn as a shadow, so the row keeps its height.
  ([#29](https://github.com/rendyuwu/ketikin/issues/29))

## [0.2.0] - 2026-08-20

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

[Unreleased]: https://github.com/rendyuwu/ketikin/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/rendyuwu/ketikin/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/rendyuwu/ketikin/releases/tag/v0.1.0
