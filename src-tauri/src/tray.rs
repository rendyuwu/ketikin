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
//!      of hiding, and `tray://unavailable` must reach the UI.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const MENU_SHOW: &str = "show";
const MENU_HIDE: &str = "hide";
const MENU_QUIT: &str = "quit";

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

    let mut builder = TrayIconBuilder::with_id("ketikin")
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
///
/// Split by platform because macOS wants a different *artifact*, not a different
/// size of the same one: the menu bar expects a template image — black plus alpha,
/// which the system tints itself, so it inverts against a light or dark menu bar
/// and dims with the rest of the bar when the app is inactive. A full-colour tile
/// there reads as a foreign object beside every other item, and no choice of
/// colour fixes that.
///
/// Everywhere else the bundled window icon is right and is already the correct
/// artifact: `default_window_icon` resolves to `icons/icon.ico` on Windows (whose
/// 16px entry is drawn for 16px) and to the first PNG in `tauri.conf.json`'s icon
/// list, `icons/32x32.png`, elsewhere.
fn with_icon<R: tauri::Runtime>(
    app: &AppHandle<R>,
    builder: TrayIconBuilder<R>,
) -> TrayIconBuilder<R> {
    #[cfg(target_os = "macos")]
    {
        // Decoded at startup rather than at build time because `Image` owns
        // pixels, not a PNG; the bytes themselves are embedded, so this cannot
        // fail on a missing file, only on a corrupt one.
        match tauri::image::Image::from_bytes(include_bytes!("../icons/tray-macos-template.png")) {
            Ok(icon) => return builder.icon(icon).icon_as_template(true),
            Err(err) => {
                // Fall through to the window icon rather than giving up: an icon
                // that looks wrong in the menu bar still beats no tray at all,
                // because with `closeToTray` on the tray menu is the only way to
                // quit.
                log::warn!("tray: template icon unusable ({err}); falling back to the window icon");
            }
        }
    }

    if let Some(icon) = app.default_window_icon() {
        builder.icon(icon.clone())
    } else {
        log::warn!("tray: no bundled window icon available; using the system default");
        builder
    }
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
