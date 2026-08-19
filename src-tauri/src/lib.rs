//! Ketikin backend.
//!
//! Ketikin types text into whatever window currently has focus, one keystroke
//! at a time, for consoles that refuse clipboard paste (hypervisor web
//! consoles, KVM-over-IP, and friends).
//!
//! This file owns the wiring: managed state, the command surface, the tray, the
//! global shortcuts, window behaviour, and the background update poller. The
//! actual work lives in the sibling modules.

pub mod error;
pub mod hotkeys;
pub mod settings;
pub mod storage;
pub mod templates;
pub mod tray;
pub mod typing;
pub mod updater;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};

use settings::Settings;
use storage::{Storage, StorageInfo};
use templates::Template;
use typing::{TypingHandle, TypingState};
use updater::UpdateInfo;

/// Wait before announcing degraded storage, so the WebView has attached its
/// `storage://warning` listener. The frontend can also just call
/// `storage_info` on mount; this event exists so it does not have to poll.
const STORAGE_WARNING_DELAY: Duration = Duration::from_millis(1_500);

/// Delay before the first background update check.
const FIRST_UPDATE_CHECK_DELAY: Duration = Duration::from_secs(5);

/// Gap between background update checks.
const UPDATE_POLL_INTERVAL: Duration = Duration::from_secs(60 * 60);

/// Everything the commands share.
///
/// All locks are plain [`std::sync::Mutex`] and are held for as short a span as
/// possible; none is ever held across an `.await` or across a sleep.
pub struct AppState {
    pub storage: Storage,
    pub settings: Mutex<Settings>,
    pub templates: Mutex<Vec<Template>>,
    pub typing: TypingHandle,
    /// The update resolved by the last successful check, so `install_update`
    /// does not have to ask the endpoint a second time.
    pub pending_update: Mutex<Option<tauri_plugin_updater::Update>>,
    /// Set when the user picks Quit, so the close handler stops intercepting.
    quitting: AtomicBool,
    /// Whether [`tray::create`] actually succeeded.
    ///
    /// Starts `false` and is only raised on success, so the safe direction is
    /// the default: if this is somehow never set, close exits (recoverable)
    /// rather than hiding a window with no tray to restore it from
    /// (unrecoverable without killing the process).
    tray_available: AtomicBool,
}

impl AppState {
    pub fn begin_quit(&self) {
        self.quitting.store(true, Ordering::Relaxed);
    }

    fn is_quitting(&self) -> bool {
        self.quitting.load(Ordering::Relaxed)
    }

    fn set_tray_available(&self, available: bool) {
        self.tray_available.store(available, Ordering::Relaxed);
    }

    fn is_tray_available(&self) -> bool {
        self.tray_available.load(Ordering::Relaxed)
    }
}

/// Payload of `tray://unavailable`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayUnavailable {
    pub message: String,
}

/// Should a hide-to-tray setting actually be honoured right now?
///
/// `minimizeToTray` and `closeToTray` both default to on, which makes the tray
/// menu the only way to quit. If the tray failed to build there is no icon to
/// restore or quit from, so both settings are ignored at the point of use —
/// close exits and minimize minimizes normally.
///
/// Deliberately does *not* rewrite `settings.json`: the user's preference is
/// still their preference, and it should come back the next time they run on a
/// machine where the tray works.
fn hide_to_tray_allowed(setting: bool, tray_available: bool, quitting: bool) -> bool {
    setting && tray_available && !quitting
}

/// Lock a mutex, recovering from poisoning instead of propagating a panic.
///
/// Every value behind these locks is plain state with no cross-field invariant
/// that a half-finished update could violate, so continuing with the inner
/// value is safe — and far better than a single panicking command bricking the
/// whole app for the rest of the session.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// Commands
//
// Every fallible command returns `Result<T, String>`; `AppError` converts into
// a human-readable sentence at this boundary and nowhere else.
// ---------------------------------------------------------------------------

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    lock(&state.settings).clone()
}

/// Persist settings, then re-apply everything derived from them.
///
/// Hotkey registration failures do **not** fail this command: the settings are
/// already on disk, and the specific accelerator that could not be claimed is
/// reported on the `hotkey://error` event instead (see [`hotkeys`]).
#[tauri::command]
fn save_settings(
    settings: Settings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, String> {
    let mut settings = settings;
    settings.normalize();

    settings.save(&state.storage)?;
    *lock(&state.settings) = settings.clone();

    apply_window_settings(&app, &settings);
    hotkeys::apply(&app, &settings);

    Ok(settings)
}

#[tauri::command]
fn list_templates(state: State<'_, AppState>) -> Vec<Template> {
    lock(&state.templates).clone()
}

#[tauri::command]
fn create_template(
    name: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<Template, String> {
    let mut current = lock(&state.templates);

    // Mutate a copy and only commit once it is safely on disk, so a failed
    // write can never leave memory and disk disagreeing.
    let mut next = current.clone();
    let created = templates::create(&mut next, name, content)?;
    templates::save(&state.storage, &next)?;
    *current = next;

    Ok(created)
}

#[tauri::command]
fn update_template(
    id: String,
    name: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<Template, String> {
    let mut current = lock(&state.templates);

    let mut next = current.clone();
    let updated = templates::update(&mut next, &id, name, content)?;
    templates::save(&state.storage, &next)?;
    *current = next;

    Ok(updated)
}

#[tauri::command]
fn delete_template(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut current = lock(&state.templates);

    let mut next = current.clone();
    templates::delete(&mut next, &id)?;
    templates::save(&state.storage, &next)?;
    *current = next;

    Ok(())
}

/// `(async)` puts this on the blocking threadpool rather than the main thread:
/// it waits (briefly, and with a deadline) for the worker to report whether the
/// system keyboard came up, and a wedged display server must not freeze the UI.
#[tauri::command(async)]
fn start_typing(text: String, app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Snapshot the settings so a save mid-run cannot change the delay or the
    // newline mode under the worker's feet.
    let snapshot = lock(&state.settings).clone();
    typing::start(&app, &text, snapshot)?;
    Ok(())
}

#[tauri::command]
fn stop_typing(app: AppHandle) -> Result<(), String> {
    typing::stop(&app).map_err(Into::into)
}

#[tauri::command]
fn typing_status(state: State<'_, AppState>) -> TypingState {
    state.typing.status()
}

#[tauri::command]
fn storage_info(state: State<'_, AppState>) -> StorageInfo {
    state.storage.info()
}

#[tauri::command]
async fn check_for_updates(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    updater::check(&app).await.map_err(Into::into)
}

#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await.map_err(Into::into)
}

#[tauri::command]
async fn open_release_notes(version: String, app: AppHandle) -> Result<(), String> {
    updater::open_release_notes(&app, &version).map_err(Into::into)
}

#[tauri::command]
fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

/// Parse-only check for the Settings panel. Registers nothing.
#[tauri::command]
fn validate_hotkey(accelerator: String) -> Result<(), String> {
    hotkeys::validate(&accelerator).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// Push the window-affecting settings onto the live window.
fn apply_window_settings(app: &AppHandle, settings: &Settings) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    if let Err(err) = window.set_always_on_top(settings.always_on_top) {
        log::warn!("window: could not set always-on-top: {err}");
    }
}

fn close_to_tray(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let setting = lock(&state.settings).close_to_tray;

    hide_to_tray_allowed(setting, state.is_tray_available(), state.is_quitting())
}

fn minimize_to_tray(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let setting = lock(&state.settings).minimize_to_tray;

    hide_to_tray_allowed(setting, state.is_tray_available(), state.is_quitting())
}

fn auto_check_updates(app: &AppHandle) -> bool {
    app.try_state::<AppState>()
        .map(|state| lock(&state.settings).auto_check_updates)
        .unwrap_or(false)
}

fn handle_window_event(window: &tauri::Window, event: &WindowEvent) {
    let app = window.app_handle();

    match event {
        WindowEvent::CloseRequested { api, .. } => {
            if close_to_tray(app) {
                api.prevent_close();
                if let Err(err) = window.hide() {
                    log::warn!("window: could not hide on close: {err}");
                }
            }
        }
        // There is no portable "about to minimize" hook, so minimize-to-tray is
        // implemented after the fact: the window minimizes normally and is then
        // hidden. On Windows and macOS that means a brief minimize animation
        // before it disappears; on some Linux WMs `is_minimized` is unreliable
        // and the window may simply stay in the taskbar. The tray menu's Hide
        // item is the portable path.
        WindowEvent::Resized(_) => {
            if minimize_to_tray(app) && window.is_minimized().unwrap_or(false) {
                if let Err(err) = window.hide() {
                    log::warn!("window: could not hide on minimize: {err}");
                }
            }
        }
        _ => {}
    }
}

/// Poll for updates forever, re-reading `auto_check_updates` every iteration so
/// toggling the setting at runtime takes effect without a restart.
fn spawn_update_poller(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_UPDATE_CHECK_DELAY).await;

        loop {
            if auto_check_updates(&app) {
                match updater::check(&app).await {
                    Ok(Some(info)) => {
                        let _ = app.emit(updater::EVENT_AVAILABLE, info);
                    }
                    Ok(None) => {}
                    Err(err) => log::warn!("updater: background check failed: {err}"),
                }
            }
            tokio::time::sleep(UPDATE_POLL_INTERVAL).await;
        }
    });
}

fn setup(app: &AppHandle) {
    // Resolve the data directory exactly once, probing each candidate with a
    // real write. Everything downstream just asks `Storage`.
    let storage = Storage::resolve(Storage::candidates(app));
    let info = storage.info();
    let degraded = storage.is_degraded();

    let settings = Settings::load(&storage);
    let saved_templates = templates::load(&storage);

    log::info!(
        "ketikin {} starting — data directory: {} (source: {}, writable: {}), {} template(s)",
        app.package_info().version,
        if info.path.is_empty() {
            "<none>"
        } else {
            &info.path
        },
        info.source,
        info.writable,
        saved_templates.len()
    );

    app.manage(AppState {
        storage,
        settings: Mutex::new(settings.clone()),
        templates: Mutex::new(saved_templates),
        typing: TypingHandle::default(),
        pending_update: Mutex::new(None),
        quitting: AtomicBool::new(false),
        tray_available: AtomicBool::new(false),
    });

    // A tray failure disables close-to-tray and minimize-to-tray for this run;
    // without it there would be no way to restore or quit the app.
    let tray_failure = match tray::create(app) {
        Ok(()) => {
            if let Some(state) = app.try_state::<AppState>() {
                state.set_tray_available(true);
            }
            None
        }
        Err(err) => {
            let message = tray::describe_failure(&err);
            log::warn!("tray: {message}");
            Some(message)
        }
    };

    hotkeys::apply(app, &settings);
    apply_window_settings(app, &settings);

    // Both startup notices are delayed for the same reason: `emit` does not
    // buffer, so firing now would land before the WebView attaches a listener.
    if degraded {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STORAGE_WARNING_DELAY).await;
            let _ = handle.emit(storage::EVENT_WARNING, info);
        });
    }

    if let Some(message) = tray_failure {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(STORAGE_WARNING_DELAY).await;
            let _ = handle.emit(tray::EVENT_UNAVAILABLE, TrayUnavailable { message });
        });
    }

    spawn_update_poller(app.clone());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_templates,
            create_template,
            update_template,
            delete_template,
            start_typing,
            stop_typing,
            typing_status,
            storage_info,
            check_for_updates,
            install_update,
            open_release_notes,
            app_version,
            validate_hotkey,
        ])
        .setup(|app| {
            setup(app.handle());
            Ok(())
        })
        .on_window_event(handle_window_event)
        .run(tauri::generate_context!());

    if let Err(err) = result {
        log::error!("ketikin could not start: {err}");
        eprintln!("ketikin could not start: {err}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tauri does not enable tokio's `time` feature itself; this crate does,
    /// and Cargo feature unification is what makes the `enable_all()` inside
    /// Tauri's shared runtime actually start the timer driver. If that ever
    /// stops holding, `spawn_update_poller` would panic at runtime rather than
    /// fail to build — so pin the behaviour down here.
    #[test]
    fn hide_to_tray_needs_both_the_setting_and_a_working_tray() {
        assert!(hide_to_tray_allowed(true, true, false));

        // The failsafe: with no tray there is nothing to restore or quit from,
        // so the setting is ignored no matter what settings.json says.
        assert!(!hide_to_tray_allowed(true, false, false));

        // Turning the setting off still wins when the tray works.
        assert!(!hide_to_tray_allowed(false, true, false));

        // Quit must never be intercepted.
        assert!(!hide_to_tray_allowed(true, true, true));
        assert!(!hide_to_tray_allowed(true, false, true));
    }

    #[test]
    fn tray_availability_defaults_to_the_safe_direction() {
        let flag = AtomicBool::new(false);
        assert!(
            !hide_to_tray_allowed(true, flag.load(Ordering::Relaxed), false),
            "before tray::create succeeds, close must exit rather than hide"
        );

        flag.store(true, Ordering::Relaxed);
        assert!(hide_to_tray_allowed(
            true,
            flag.load(Ordering::Relaxed),
            false
        ));
    }

    #[test]
    fn tray_unavailable_payload_carries_a_message() {
        let json = serde_json::to_string(&TrayUnavailable {
            message: "no tray".to_string(),
        })
        .expect("serialize");

        assert_eq!(json, r#"{"message":"no tray"}"#);
    }

    #[test]
    fn the_shared_async_runtime_has_a_working_timer() {
        let started = std::time::Instant::now();

        tauri::async_runtime::block_on(async {
            tokio::time::sleep(Duration::from_millis(30)).await;
        });

        assert!(started.elapsed() >= Duration::from_millis(30));
    }
}
