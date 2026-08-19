//! Global shortcut registration.
//!
//! Two shortcuts are supported: start and stop. They are registered
//! individually rather than as a batch so that one accelerator being claimed by
//! another application does not also take out the other.
//!
//! Failure policy: a shortcut that cannot be registered never fails a settings
//! save. `save_settings` still persists and still returns the normalized
//! `Settings`; the failure is reported out-of-band on the `hotkey://error`
//! event as `{ which, accelerator, message }` so the UI can point at the exact
//! field that did not take.

use std::str::FromStr;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::error::AppError;
use crate::settings::Settings;
use crate::typing;

/// Fired when the start shortcut is pressed. The backend has no idea what is in
/// the text box, so the frontend answers this by invoking `start_typing` with
/// its own current contents.
pub const EVENT_START: &str = "hotkey://start";

/// Fired when the stop shortcut is pressed, purely so the UI can update. The
/// actual stop happens in Rust first — see [`register`].
pub const EVENT_STOP: &str = "hotkey://stop";

/// Fired when an accelerator could not be registered.
pub const EVENT_ERROR: &str = "hotkey://error";

/// Payload of [`EVENT_ERROR`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyError {
    /// `"start"` or `"stop"`.
    pub which: String,
    pub accelerator: String,
    pub message: String,
}

/// Parse an accelerator without registering it, so the Settings panel can
/// validate as the user types.
pub fn validate(accelerator: &str) -> Result<(), AppError> {
    let trimmed = accelerator.trim();
    if trimmed.is_empty() {
        return Err(AppError::Hotkey("enter a shortcut".to_string()));
    }

    Shortcut::from_str(trimmed)
        .map(|_| ())
        .map_err(|err| AppError::Hotkey(format!("{trimmed} is not a valid shortcut: {err}")))
}

/// Drop every previously registered shortcut and re-register from `settings`.
///
/// Always unregisters first, so this is the single entry point for both the
/// startup registration and every later settings change.
pub fn apply(app: &AppHandle, settings: &Settings) {
    let manager = app.global_shortcut();

    if let Err(err) = manager.unregister_all() {
        log::warn!("hotkeys: could not clear existing shortcuts: {err}");
    }

    if !settings.hotkeys_enabled {
        log::info!("hotkeys: disabled by settings");
        return;
    }

    register(app, "start", &settings.start_hotkey);
    register(app, "stop", &settings.stop_hotkey);
}

fn register(app: &AppHandle, which: &str, accelerator: &str) {
    let shortcut = match Shortcut::from_str(accelerator.trim()) {
        Ok(shortcut) => shortcut,
        Err(err) => {
            report(
                app,
                which,
                accelerator,
                format!("{accelerator} is not a valid shortcut: {err}"),
            );
            return;
        }
    };

    let is_start = which == "start";
    let result = app
        .global_shortcut()
        .on_shortcut(shortcut, move |app, _, event| {
            // Fires for press *and* release; acting on both would double-trigger.
            if event.state != ShortcutState::Pressed {
                return;
            }

            if is_start {
                let _ = app.emit(EVENT_START, ());
            } else {
                // Stop runs in Rust before the event goes out, so it still works
                // when the WebView is busy or wedged.
                if let Err(err) = typing::stop(app) {
                    log::warn!("hotkeys: stop shortcut failed: {err}");
                }
                let _ = app.emit(EVENT_STOP, ());
            }
        });

    match result {
        Ok(()) => log::info!("hotkeys: registered {which} shortcut {accelerator}"),
        Err(err) => report(
            app,
            which,
            accelerator,
            format!("{accelerator} could not be registered — another application may already be using it ({err})"),
        ),
    }
}

fn report(app: &AppHandle, which: &str, accelerator: &str, message: String) {
    log::warn!("hotkeys: {which} shortcut failed: {message}");

    let _ = app.emit(
        EVENT_ERROR,
        HotkeyError {
            which: which.to_string(),
            accelerator: accelerator.to_string(),
            message,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_default_accelerators() {
        validate("CommandOrControl+Alt+T").expect("start default should parse");
        validate("CommandOrControl+Alt+X").expect("stop default should parse");
        validate("  Alt+Shift+K  ").expect("surrounding whitespace should be ignored");
    }

    #[test]
    fn rejects_nonsense() {
        assert!(validate("").is_err());
        assert!(validate("   ").is_err());
        assert!(validate("NotAKey").is_err());
        assert!(validate("Ctrl+").is_err());
    }

    #[test]
    fn rejection_message_names_the_accelerator() {
        let err = validate("Meta+Nope").expect_err("must fail");
        assert!(err.to_string().contains("Meta+Nope"));
    }
}
