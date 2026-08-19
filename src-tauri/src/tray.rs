//! System tray icon and its menu.
//!
//! Ketikin is a background-ish utility: the window is usually hidden while the
//! user works in a console, so the tray is the primary way back to it.
//!
//! The tray is built here in Rust rather than from the frontend on purpose: the
//! Rust path needs no capability at all, so nothing here depends on the ACL.
//!
//! Note that `capabilities/default.json` does *not* currently withhold the JS
//! tray API — it grants `core:default`, which transitively includes both
//! `core:tray:default` and `core:menu:default`. Narrowing that would mean
//! replacing `core:default` with an explicit list in the capability file, not
//! changing anything in this module.
//!
//! MANUAL TEST REQUIRED BEFORE RELEASE: nothing in this repo's automated checks
//! exercises close-to-tray or quit-from-tray — both need real windows and a
//! real StatusNotifier/shell. Verify by hand on each platform:
//!   1. close the window with `closeToTray` on — it hides, tray icon remains;
//!   2. Quit from the tray menu — the process actually exits;
//!   3. start with the tray broken (see [`create`]) — close must exit instead
//!      of hiding, and `tray://unavailable` must reach the UI;
//!   4. hide to the tray, start a run, and watch the icon: it must gain the run
//!      mark for the whole run (countdown included) and lose it again on finish
//!      and on the stop hotkey. On macOS check both a light and a dark menu bar,
//!      since a lost template flag only shows on one of them.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const MENU_SHOW: &str = "show";
const MENU_HIDE: &str = "hide";
const MENU_QUIT: &str = "quit";

/// Id the tray is built under, and how [`set_running`] finds it again afterwards.
const TRAY_ID: &str = "ketikin";

/// Idle artifact, when this platform needs one embedded rather than taking it
/// from the bundle.
///
/// macOS does, because the menu bar wants a template image and nothing in the
/// bundle is one. Everywhere else the bundled window icon is already the right
/// artifact, so there is nothing to embed — see [`artifact`].
#[cfg(target_os = "macos")]
const IDLE_BYTES: Option<&[u8]> = Some(include_bytes!("../icons/tray-macos-template.png"));
#[cfg(not(target_os = "macos"))]
const IDLE_BYTES: Option<&[u8]> = None;

/// Run-state artifact, embedded on every platform: unlike the idle icon this one
/// has no counterpart in the bundle to fall back on.
///
/// The two states differ by exactly one shape — a 2x2 dot knocked out of the
/// keycap above the caret — because the mark has to survive 16px and has to read
/// in a single tint for the macOS menu bar, where colour carries nothing. The
/// reasoning is in `icons/tray-run.svg`.
#[cfg(target_os = "macos")]
const RUN_BYTES: &[u8] = include_bytes!("../icons/tray-macos-template-run.png");
#[cfg(not(target_os = "macos"))]
const RUN_BYTES: &[u8] = include_bytes!("../icons/tray-run.png");

/// Emitted once at startup when the tray could not be created.
///
/// Fire-and-forget: `emit` does not buffer, so a WebView that is still starting
/// up misses it entirely. The `tray_status` command is the pull-based fallback
/// and is the one the frontend should rely on for correctness.
pub const EVENT_UNAVAILABLE: &str = "tray://unavailable";

/// Reply from the `tray_status` command.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayStatus {
    pub available: bool,
    /// Explanation when `available` is false, otherwise `null`.
    pub message: Option<String>,
}

/// Explain a tray construction failure in terms the user can act on.
///
/// The common real-world cause on Linux is a desktop with no StatusNotifier
/// host (or a missing `libayatana-appindicator3-1`), which is not something the
/// app can fix for them.
pub fn describe_failure(err: &tauri::Error) -> String {
    let base = format!("Ketikin could not create a system tray icon ({err})");

    if cfg!(target_os = "linux") {
        format!(
            "{base}. Your desktop may have no system tray (StatusNotifier host) running, or \
             libayatana-appindicator3-1 may not be installed. Close and minimize will not hide \
             Ketikin to the tray until this is resolved."
        )
    } else {
        format!(
            "{base}. Close and minimize will not hide Ketikin to the tray until this is resolved."
        )
    }
}

/// Build the tray icon. Called once from `setup`.
///
/// A failure here is not fatal, but it *is* load-bearing: with the default
/// `closeToTray`/`minimizeToTray` both on, the tray menu is the only way to
/// quit. The caller must record the failure so those two settings get ignored —
/// see `AppState::tray_available`.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, MENU_SHOW, "Show Ketikin", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, MENU_HIDE, "Hide", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &separator, &quit_item])?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Ketikin")
        // Left click toggles the window instead of opening the menu; the menu
        // stays on right click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_SHOW => show_window(app),
            MENU_HIDE => hide_window(app),
            MENU_QUIT => quit(app),
            other => log::warn!("tray: unhandled menu item {other}"),
        })
        .on_tray_icon_event(|tray, event| {
            // Not emitted on Linux (the tray backends there only surface menu
            // activation), so the menu items remain the portable path.
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_window(tray.app_handle());
            }
        });

    builder = with_icon(app, builder);

    builder.build(app)?;
    log::info!("tray: icon created");

    Ok(())
}

/// Give the tray its icon.
fn with_icon<R: tauri::Runtime>(
    app: &AppHandle<R>,
    builder: TrayIconBuilder<R>,
) -> TrayIconBuilder<R> {
    match artifact(app, false) {
        Some((icon, template)) => builder.icon(icon).icon_as_template(template),
        None => {
            log::warn!("tray: no bundled window icon available; using the system default");
            builder
        }
    }
}

/// Show or clear the mark that says a run is in progress.
///
/// The window already shows run state on its top edge, but the case this exists
/// for is the one where none of the window is on screen: minimized, or hidden by
/// `minimizeToTray` / `closeToTray`, which both default to on. The user is looking
/// at the console being typed into, and the tray is all of Ketikin they can see.
///
/// Called exactly twice per run — once as the run is accepted, once when it
/// finishes, is stopped, errors or panics — from the two ends of `typing`'s
/// `RunGuard`, which is what makes the second call as certain as the terminal
/// `typing://done` it sits beside. Deliberately *not* driven by `typing://state`:
/// that fires ~20 times a second and every swap here is a blocking round trip
/// through the main thread, for a mark that only ever changes twice. Anything
/// added to this function inherits that cost — do not move it into the keystroke
/// loop.
///
/// Callable from a typing worker, and that is where it is called from. `set_icon`
/// marshals onto the main thread and blocks until it runs, so `AppState`'s locking
/// rule applies: never call this while holding a lock the main thread can want.
/// It takes no lock of its own — `tray_by_id` only reads Tauri's own table.
///
/// Every failure is logged and swallowed. A tray icon left in the wrong state is
/// cosmetic; a run that refuses to start or to stop because of one is not.
pub fn set_running(app: &AppHandle, running: bool) {
    // `None` means the tray never built (see `create`), so there is nothing to
    // mark and nothing to warn about — the failure was already reported at
    // startup through `tray://unavailable`.
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let Some((icon, template)) = artifact(app, running) else {
        log::warn!("tray: no icon available, leaving the current one in place");
        return;
    };

    // `set_icon_with_as_template` rather than `set_icon`, and not for the flicker
    // its docs mention: on macOS `set_icon` passes `false` for the template flag
    // unconditionally, so swapping icons that way would leave the menu bar
    // drawing raw black artwork that no longer inverts with the bar or dims with
    // the app. On Windows and Linux this call *is* `set_icon` and the flag is
    // ignored, which is why there is no `cfg` here.
    if let Err(err) = tray.set_icon_with_as_template(Some(icon), template) {
        log::warn!(
            "tray: could not switch to the {} icon: {err}",
            label(running)
        );
    }
}

/// Names a run state for a log line.
fn label(running: bool) -> &'static str {
    if running {
        "run-state"
    } else {
        "idle"
    }
}

/// Resolve the icon for a run state, and whether macOS should tint it as a
/// template image.
///
/// Split by platform because macOS wants a different *artifact*, not a different
/// size of the same one: the menu bar expects a template image — black plus alpha,
/// which the system tints itself, so it inverts against a light or dark menu bar
/// and dims with the rest of the bar when the app is inactive. A full-colour tile
/// there reads as a foreign object beside every other item, and no choice of
/// colour fixes that.
///
/// For the idle state everywhere else the bundled window icon is right and is
/// already the correct artifact: `default_window_icon` resolves to
/// `icons/icon.ico` on Windows (whose 16px entry is drawn for 16px) and to the
/// first PNG in `tauri.conf.json`'s icon list, `icons/32x32.png`, elsewhere. The
/// run state has no bundle entry, so it ships as a 32px render of the same
/// 16-unit grid; every coordinate in it is a whole unit, so the halving Windows
/// does to reach 16px lands each edge back on a pixel boundary.
///
/// `None` only when there is no artifact at all for this state, which takes a
/// bundle with no window icon. The caller leaves whatever is showing alone.
fn artifact<R: tauri::Runtime>(app: &AppHandle<R>, running: bool) -> Option<(Image<'_>, bool)> {
    if let Some(bytes) = if running { Some(RUN_BYTES) } else { IDLE_BYTES } {
        // Decoded here rather than at build time because `Image` owns pixels, not
        // a PNG; the bytes themselves are embedded, so this cannot fail on a
        // missing file, only on a corrupt one.
        match Image::from_bytes(bytes) {
            Ok(icon) => return Some((icon, cfg!(target_os = "macos"))),
            Err(err) => {
                // Fall through to the window icon rather than giving up: an icon
                // that looks wrong in the menu bar still beats no tray at all,
                // because with `closeToTray` on the tray menu is the only way to
                // quit. The run state degrades to "no mark" the same way, which is
                // the same thing every platform did before it existed.
                log::warn!(
                    "tray: embedded {} icon unusable ({err}); falling back to the window icon",
                    label(running)
                );
            }
        }
    }

    // Never as a template: the window icon is a full-colour tile, and macOS would
    // flatten it to a black square.
    Some((app.default_window_icon()?.clone(), false))
}

/// Reveal, unminimize, and focus the main window.
pub fn show_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        log::warn!("tray: main window is gone");
        return;
    };

    if let Err(err) = window.show() {
        log::warn!("tray: could not show the window: {err}");
    }
    // Showing a minimized window leaves it minimized on Windows.
    if let Err(err) = window.unminimize() {
        log::debug!("tray: could not unminimize the window: {err}");
    }
    if let Err(err) = window.set_focus() {
        log::warn!("tray: could not focus the window: {err}");
    }
}

fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(err) = window.hide() {
            log::warn!("tray: could not hide the window: {err}");
        }
    }
}

fn toggle_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    match window.is_visible() {
        Ok(true) => hide_window(app),
        Ok(false) => show_window(app),
        Err(err) => {
            log::warn!("tray: could not read window visibility ({err}); showing it");
            show_window(app);
        }
    }
}

/// Exit the process for real, bypassing the close-to-tray interception.
fn quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        state.begin_quit();
    }
    log::info!("tray: quit requested");
    app.exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The artifact this platform shows when idle. On macOS that is the embedded
    /// template; elsewhere it is what `default_window_icon` resolves to, which
    /// cannot be reached without a running app, so the test reads the same file
    /// off disk instead.
    #[cfg(target_os = "macos")]
    const IDLE_FOR_COMPARISON: &[u8] = include_bytes!("../icons/tray-macos-template.png");
    #[cfg(not(target_os = "macos"))]
    const IDLE_FOR_COMPARISON: &[u8] = include_bytes!("../icons/32x32.png");

    /// A run has to be visible from the tray, which takes two artifacts that are
    /// actually different. Both are renders of the same grid — the run one with a
    /// single 2x2-unit mark added — so re-rendering either from the wrong source
    /// SVG would silently leave the two states identical and this feature doing
    /// nothing at all. Catching that is what this test is for; whether the mark
    /// *reads* at 16px is a judgement no assertion makes, and is item 4 of the
    /// manual list at the top of this module.
    #[test]
    fn the_run_icon_is_a_different_artifact_from_the_idle_one() {
        let run = Image::from_bytes(RUN_BYTES).expect("the run icon should decode");
        let idle = Image::from_bytes(IDLE_FOR_COMPARISON).expect("the idle icon should decode");

        assert_eq!(
            (run.width(), run.height()),
            (idle.width(), idle.height()),
            "the two states must be the same size, or the tray icon resizes mid-run"
        );

        let differing = run
            .rgba()
            .chunks(4)
            .zip(idle.rgba().chunks(4))
            .filter(|(marked, plain)| marked != plain)
            .count();

        // 2x2 grid units is 4 pixels at 16px and 16 at the sizes these actually
        // ship at, so the floor is the smallest a conforming mark can ever be.
        assert!(
            differing >= 4,
            "the run marker is missing or too small: {differing} pixels differ"
        );
    }

    #[test]
    fn the_run_marker_leaves_the_keycap_edge_alone() {
        let run = Image::from_bytes(RUN_BYTES).expect("the run icon should decode");
        let idle = Image::from_bytes(IDLE_FOR_COMPARISON).expect("the idle icon should decode");
        let width = run.width();

        // A mark that reaches the edge of the key reads as a nick taken out of it
        // rather than a mark on it, so every changed pixel has to sit inside the
        // keycap. One eighth in is where the keycap starts on the app icon's grid
        // (2 units of 16) and is well inside it on the menu bar variant's (1 of
        // 18), so it is the boundary for both.
        let margin = width / 8;
        for (index, (marked, plain)) in run.rgba().chunks(4).zip(idle.rgba().chunks(4)).enumerate()
        {
            if marked == plain {
                continue;
            }
            let (x, y) = (index as u32 % width, index as u32 / width);
            assert!(
                x >= margin && x < width - margin && y >= margin && y < run.height() - margin,
                "the run marker reaches the edge of the key at ({x}, {y})"
            );
        }
    }
}
