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
use tauri_plugin_log::{Target, TargetKind};
use tauri_plugin_opener::OpenerExt;

use hotkeys::HotkeyHandle;
use settings::Settings;
use storage::{Storage, StorageInfo};
use templates::Template;
use tray::TrayStatus;
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

/// Cap per log file, before rotation.
///
/// Set explicitly rather than inherited: the plugin defaults to 40 KB with
/// `KeepOne`, which a single typing session rolls straight past — discarding
/// the startup storage-resolution and hotkey-registration diagnostics, which
/// are log-only and are exactly what a bug report needs. 1 MB across three
/// files stays small enough to attach to an issue.
const LOG_MAX_FILE_SIZE: u128 = 1024 * 1024;

/// Everything the commands share.
///
/// All locks are plain [`std::sync::Mutex`] and are held for as short a span as
/// possible; none is ever held across an `.await` or across a sleep.
pub struct AppState {
    pub storage: Storage,
    pub settings: Mutex<Settings>,
    pub templates: Mutex<Vec<Template>>,
    pub typing: TypingHandle,
    /// Which accelerators are actually bound right now.
    pub hotkeys: HotkeyHandle,
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
    /// Why the tray is unavailable, for `tray_status`.
    tray_message: Mutex<Option<String>>,
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

    fn tray_status(&self) -> TrayStatus {
        let available = self.is_tray_available();

        TrayStatus {
            // Structurally guaranteed: the frontend renders this verbatim, so
            // `available: false` must never arrive without an explanation. The
            // fallback covers only the unreachable pre-setup window.
            message: (!available).then(|| {
                lock(&self.tray_message).clone().unwrap_or_else(|| {
                    "The system tray is unavailable, so Ketikin cannot hide to it. Closing the \
                     window will exit the app."
                        .to_string()
                })
            }),
            available,
        }
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

/// Apply settings, then persist whatever actually took effect.
///
/// Order matters. Hotkeys are bound **before** anything is written to disk, and
/// [`hotkeys::apply`] returns the accelerators that are genuinely in force: if
/// the user's new shortcut is refused by the OS, the previous one is restored
/// and it is the previous one that gets persisted. Stored settings therefore
/// never contain a shortcut that is not really bound, and a restart recovers
/// instead of repeating the failure.
///
/// Registration failures do not fail this command — the rest of the settings
/// still save. The specific accelerator that could not be claimed is reported
/// on the `hotkey://error` event (see [`hotkeys`]).
#[tauri::command]
fn save_settings(
    settings: Settings,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Settings, String> {
    let mut settings = settings;
    settings.normalize();
    settings.validate()?;

    let previous = lock(&state.settings).clone();
    let effective = hotkeys::apply(&app, &previous, &settings);

    effective.save(&state.storage)?;
    *lock(&state.settings) = effective.clone();

    apply_window_settings(&app, &effective);

    Ok(effective)
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

/// Pull-based counterpart to `tray://unavailable`, for a frontend that mounted
/// after the event fired. Mirrors the `storage_info` pattern.
#[tauri::command]
fn tray_status(state: State<'_, AppState>) -> TrayStatus {
    state.tray_status()
}

/// Reveal the resolved data directory in the system file manager.
///
/// Triage for any Ketikin bug report ends at `<data dir>/logs/`, and the data
/// directory is exactly the thing that moves on the locked-down machines where
/// problems happen — so asking someone to transcribe a path out of a settings
/// pane is where those reports die.
///
/// The path is read from the same [`Storage`] every other command uses rather
/// than recomputed. A second derivation would be correct right up until the
/// fallback chain picked something unexpected, which is precisely when it
/// matters.
#[tauri::command]
fn open_data_folder(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let Some(dir) = state.storage.dir() else {
        return Err(
            "Ketikin could not find anywhere writable to store data, so it is keeping \
                    settings in memory only and there is no folder to open."
                .to_string(),
        );
    };
    let path = dir.display().to_string();

    // A minimal or headless Linux desktop may have no file manager at all.
    // Report that plainly — the path is still visible in Settings, so a clear
    // failure leaves the user no worse off, but a silent one would.
    app.opener()
        .open_path(&path, None::<&str>)
        .map_err(|err| format!("could not open {path}: {err}"))
}

/// Release the global grabs while the user captures a replacement shortcut.
///
/// Without this, pressing the current start hotkey into a capture field fires
/// a real typing run into the settings panel instead of being read as input.
/// The frontend calls this on capture focus.
#[tauri::command]
fn suspend_hotkeys(app: AppHandle) -> Result<(), String> {
    hotkeys::suspend(&app);
    Ok(())
}

/// Re-arm the global grabs. Idempotent, and safe when nothing was suspended.
///
/// The frontend calls this on capture blur, but the backend also calls it on
/// window blur and on close, and a `save_settings` expires any suspend — a
/// leaked suspend would silently disable both hotkeys until restart.
#[tauri::command]
fn resume_hotkeys(app: AppHandle) -> Result<(), String> {
    hotkeys::resume(&app);
    Ok(())
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
            // A suspend must not outlive the panel that asked for it, or both
            // hotkeys stay dead until restart.
            hotkeys::resume(app);

            if close_to_tray(app) {
                api.prevent_close();
                if let Err(err) = window.hide() {
                    log::warn!("window: could not hide on close: {err}");
                }
            }
        }
        // Same safety net: if the user clicks away mid-capture, the frontend's
        // blur handler may never run.
        WindowEvent::Focused(false) => hotkeys::resume(app),
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

/// Load persisted state, then snapshot storage — in that order.
///
/// The ordering is load-bearing and easy to break by tidying. A corrupt file
/// only records its notice at the moment it is *read*, so snapshotting storage
/// before the loads would report a clean state, `degraded` would be false, and
/// a silent template reset would never reach the banner. Extracted from
/// [`setup`] so a test can hold the ordering in place without an `AppHandle`.
fn load_state(storage: &Storage) -> (Settings, Vec<Template>, StorageInfo) {
    let settings = Settings::load(storage);
    let templates = templates::load(storage);

    (settings, templates, storage.info())
}

fn setup(app: &AppHandle, storage: Storage) {
    let (settings, saved_templates, info) = load_state(&storage);
    let degraded = info.degraded;

    log::info!(
        "ketikin {} starting — data directory: {} (source: {}, writable: {}), {} template(s), {} notice(s)",
        app.package_info().version,
        if info.path.is_empty() {
            "<none>"
        } else {
            &info.path
        },
        info.source,
        info.writable,
        saved_templates.len(),
        info.notices.len()
    );

    app.manage(AppState {
        storage,
        settings: Mutex::new(settings.clone()),
        templates: Mutex::new(saved_templates),
        typing: TypingHandle::default(),
        hotkeys: HotkeyHandle::default(),
        pending_update: Mutex::new(None),
        quitting: AtomicBool::new(false),
        tray_available: AtomicBool::new(false),
        tray_message: Mutex::new(None),
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
            if let Some(state) = app.try_state::<AppState>() {
                *lock(&state.tray_message) = Some(message.clone());
            }
            Some(message)
        }
    };

    // At startup there is nothing bound yet and no previous value to fall back
    // to, so `previous` and `desired` are the same: a refused accelerator is
    // reported but nothing can be destroyed.
    let effective = hotkeys::apply(app, &settings, &settings);
    if let Some(state) = app.try_state::<AppState>() {
        *lock(&state.settings) = effective.clone();
    }
    apply_window_settings(app, &effective);

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

/// Decide where the log plugin may write.
///
/// `TargetKind::LogDir` is deliberately never used. It hardcodes
/// `%LOCALAPPDATA%\<identifier>\logs` on Windows — ignoring the storage
/// fallback chain entirely — and propagates its `app_log_dir()`,
/// `create_dir_all`, and file-open failures straight out of the plugin's setup
/// closure. That closure runs during `tauri::Builder::build()`, *before* our
/// own setup, so on a session host where `%LOCALAPPDATA%` is ACL-denied or
/// redirected to an unavailable path it aborts the whole app before storage
/// resolution executes a single instruction. With `windows_subsystem =
/// "windows"` in release the resulting message goes to a stderr that does not
/// exist: no window, no error, silent exit 1.
///
/// `TargetKind::Folder` is fallible for the same reasons, so it is only
/// attached to a directory that has just passed a real write probe. That turns
/// a deterministic property of the environment into a TOCTOU window of
/// microseconds. Stdout is infallible and always present.
fn log_targets(storage: &Storage) -> Vec<Target> {
    let mut targets = vec![Target::new(TargetKind::Stdout)];

    match storage.log_dir() {
        Some(path) => targets.push(Target::new(TargetKind::Folder {
            path: path.to_path_buf(),
            file_name: None,
        })),
        None => log::warn!("logging to stdout only; no writable log directory"),
    }

    targets
}

/// Last-resort record of a startup abort.
///
/// Under `windows_subsystem = "windows"` there is no stderr and no window yet,
/// so a fatal error would otherwise be completely invisible. The storage
/// directory has already passed a write probe by this point, which makes it the
/// one place we know we can leave a breadcrumb.
fn record_fatal_error(path: Option<&std::path::Path>, message: &str) {
    let Some(path) = path else {
        return;
    };
    let stamp = chrono::Utc::now().to_rfc3339();
    let _ = std::fs::write(path, format!("[{stamp}] {message}\n"));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Storage is resolved before `tauri::Builder` exists, for two reasons: the
    // log plugin's file target has to point at the result, and anything that
    // fails inside `Builder` aborts startup before our setup hook could react.
    let storage = Storage::resolve(Storage::candidates());
    let targets = log_targets(&storage);
    let fatal_path = storage
        .dir()
        .map(|dir| dir.join("ketikin-startup-error.log"));

    let result = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets(targets)
                .max_file_size(LOG_MAX_FILE_SIZE)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(2))
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
            tray_status,
            suspend_hotkeys,
            resume_hotkeys,
            open_data_folder,
        ])
        .setup(move |app| {
            setup(app.handle(), storage);
            Ok(())
        })
        .on_window_event(handle_window_event)
        .run(tauri::generate_context!());

    if let Err(err) = result {
        let message = format!("ketikin could not start: {err}");
        log::error!("{message}");
        eprintln!("{message}");
        record_fatal_error(fatal_path.as_deref(), &message);
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
    /// The emission gate, not just the verdict.
    ///
    /// `setup` decides whether to fire `storage://warning` from the snapshot
    /// `load_state` returns. If the snapshot were taken before the loads, a
    /// corrupt-file reset would be invisible to it — the notice would exist on
    /// `Storage` but the already-captured `StorageInfo` would say `degraded:
    /// false` and no banner would ever fire.
    #[test]
    fn a_corrupt_file_reaches_the_startup_warning_gate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("data");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("templates.json"), b"{ not json").expect("write");

        let storage = Storage::resolve(vec![("appData", dir)]);
        // Healthy appData: nothing about the *location* is wrong.
        assert!(storage.info().writable);

        let (_settings, templates, info) = load_state(&storage);

        assert!(
            templates.is_empty(),
            "the corrupt file should reset to empty"
        );
        assert!(
            info.degraded,
            "the snapshot setup emits from must already see the reset"
        );
        assert!(info.notices.iter().any(|n| n.contains("templates.json")));
    }

    #[test]
    fn a_clean_start_does_not_trip_the_startup_warning_gate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("appData", tmp.path().to_path_buf())]);

        let (_settings, templates, info) = load_state(&storage);

        assert!(templates.is_empty());
        assert!(!info.degraded, "a first run must not raise the banner");
        assert!(info.notices.is_empty());
    }

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
