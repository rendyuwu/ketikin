//! User settings: shape, defaults, normalization, and persistence.
//!
//! Every field carries a serde default so a settings file written by an older
//! build — or hand-edited into a partial state — still loads. Unknown keys are
//! ignored rather than rejected, which keeps downgrades survivable too.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::storage::Storage;

/// File name (without extension) inside the resolved data directory.
const FILE: &str = "settings";

pub const DEFAULT_TYPING_DELAY_MS: u32 = 25;
pub const DEFAULT_START_DELAY_SECS: u32 = 3;
pub const DEFAULT_START_HOTKEY: &str = "CommandOrControl+Alt+T";
pub const DEFAULT_STOP_HOTKEY: &str = "CommandOrControl+Alt+X";

const MIN_TYPING_DELAY_MS: u32 = 1;
const MAX_TYPING_DELAY_MS: u32 = 1000;
const MAX_START_DELAY_SECS: u32 = 10;

const THEMES: [&str; 3] = ["system", "dark", "light"];
const NEWLINE_MODES: [&str; 3] = ["enter", "shiftEnter", "skip"];

/// Everything the user can configure.
///
/// `#[serde(default)]` sits on the container so *any* missing key falls back to
/// the value from [`Settings::default`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub typing_delay_ms: u32,
    pub start_delay_secs: u32,
    pub theme: String,
    pub minimize_to_tray: bool,
    pub close_to_tray: bool,
    pub always_on_top: bool,
    pub hotkeys_enabled: bool,
    pub start_hotkey: String,
    pub stop_hotkey: String,
    pub newline_mode: String,
    pub auto_check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            typing_delay_ms: DEFAULT_TYPING_DELAY_MS,
            start_delay_secs: DEFAULT_START_DELAY_SECS,
            theme: "system".to_string(),
            minimize_to_tray: true,
            close_to_tray: true,
            always_on_top: false,
            hotkeys_enabled: true,
            start_hotkey: DEFAULT_START_HOTKEY.to_string(),
            stop_hotkey: DEFAULT_STOP_HOTKEY.to_string(),
            newline_mode: "enter".to_string(),
            auto_check_updates: true,
        }
    }
}

impl Settings {
    /// Clamp numbers into range and replace unrecognised enum-ish strings with
    /// their defaults.
    ///
    /// This runs on both load and save, so a hand-edited file cannot put the
    /// app into a state the UI has no way to represent. `save_settings` returns
    /// the normalized value, and the frontend renders what it gets back.
    pub fn normalize(&mut self) {
        self.typing_delay_ms = self
            .typing_delay_ms
            .clamp(MIN_TYPING_DELAY_MS, MAX_TYPING_DELAY_MS);
        self.start_delay_secs = self.start_delay_secs.min(MAX_START_DELAY_SECS);

        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = "system".to_string();
        }
        if !NEWLINE_MODES.contains(&self.newline_mode.as_str()) {
            self.newline_mode = "enter".to_string();
        }

        self.start_hotkey = self.start_hotkey.trim().to_string();
        self.stop_hotkey = self.stop_hotkey.trim().to_string();
        if self.start_hotkey.is_empty() {
            self.start_hotkey = DEFAULT_START_HOTKEY.to_string();
        }
        if self.stop_hotkey.is_empty() {
            self.stop_hotkey = DEFAULT_STOP_HOTKEY.to_string();
        }
    }

    /// Reject combinations that are individually valid but nonsense together.
    ///
    /// Only one rule so far: the two hotkeys cannot be the same accelerator.
    /// Without this the second registration fails with `AlreadyRegistered` and
    /// the generic hotkey error blames "another application", sending the user
    /// hunting for a conflict that is Ketikin itself.
    pub fn validate(&self) -> Result<(), AppError> {
        if self.hotkeys_enabled && self.start_hotkey.eq_ignore_ascii_case(&self.stop_hotkey) {
            return Err(AppError::Invalid(format!(
                "the start and stop shortcuts are both set to {} — give them different keys",
                self.start_hotkey
            )));
        }
        Ok(())
    }

    /// Read settings from disk, normalized. Never fails: a missing or corrupt
    /// file yields defaults (see [`Storage::read`]).
    pub fn load(storage: &Storage) -> Self {
        let mut settings: Settings = storage.read(FILE, "the built-in defaults");
        settings.normalize();
        settings
    }

    /// Persist settings atomically.
    pub fn save(&self, storage: &Storage) -> Result<(), AppError> {
        storage.write(FILE, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;

    #[test]
    fn defaults_match_the_documented_contract() {
        let settings = Settings::default();

        assert_eq!(settings.typing_delay_ms, 25);
        assert_eq!(settings.start_delay_secs, 3);
        assert_eq!(settings.theme, "system");
        assert!(settings.minimize_to_tray);
        assert!(settings.close_to_tray);
        assert!(!settings.always_on_top);
        assert!(settings.hotkeys_enabled);
        assert_eq!(settings.start_hotkey, "CommandOrControl+Alt+T");
        assert_eq!(settings.stop_hotkey, "CommandOrControl+Alt+X");
        assert_eq!(settings.newline_mode, "enter");
        assert!(settings.auto_check_updates);
    }

    #[test]
    fn normalize_clamps_numbers_and_rejects_unknown_enums() {
        let mut settings = Settings {
            typing_delay_ms: 0,
            start_delay_secs: 99,
            theme: "neon".to_string(),
            newline_mode: "carriage-pigeon".to_string(),
            start_hotkey: "   ".to_string(),
            stop_hotkey: "  Alt+Q  ".to_string(),
            ..Settings::default()
        };
        settings.normalize();

        assert_eq!(settings.typing_delay_ms, 1);
        assert_eq!(settings.start_delay_secs, 10);
        assert_eq!(settings.theme, "system");
        assert_eq!(settings.newline_mode, "enter");
        assert_eq!(settings.start_hotkey, DEFAULT_START_HOTKEY);
        assert_eq!(settings.stop_hotkey, "Alt+Q");

        let mut high = Settings {
            typing_delay_ms: 5_000,
            ..Settings::default()
        };
        high.normalize();
        assert_eq!(high.typing_delay_ms, 1000);
    }

    #[test]
    fn normalize_keeps_valid_values_untouched() {
        let original = Settings {
            typing_delay_ms: 250,
            start_delay_secs: 0,
            theme: "dark".to_string(),
            newline_mode: "shiftEnter".to_string(),
            ..Settings::default()
        };
        let mut normalized = original.clone();
        normalized.normalize();

        assert_eq!(original, normalized);
    }

    #[test]
    fn partial_json_fills_in_defaults_and_ignores_unknown_keys() {
        let json = r#"{ "typingDelayMs": 5, "theme": "dark", "somethingWeInvented": 42 }"#;
        let mut settings: Settings = serde_json::from_str(json).expect("parse");
        settings.normalize();

        assert_eq!(settings.typing_delay_ms, 5);
        assert_eq!(settings.theme, "dark");
        // Everything absent falls back to the documented default.
        assert_eq!(settings.start_delay_secs, 3);
        assert_eq!(settings.newline_mode, "enter");
        assert!(settings.close_to_tray);
    }

    #[test]
    fn serializes_as_camel_case() {
        let json = serde_json::to_string(&Settings::default()).expect("serialize");

        assert!(json.contains("\"typingDelayMs\""));
        assert!(json.contains("\"startDelaySecs\""));
        assert!(json.contains("\"minimizeToTray\""));
        assert!(json.contains("\"autoCheckUpdates\""));
        assert!(!json.contains("typing_delay_ms"));
    }

    #[test]
    fn validate_rejects_two_slots_sharing_one_accelerator() {
        let settings = Settings {
            start_hotkey: "Alt+K".to_string(),
            stop_hotkey: "Alt+K".to_string(),
            ..Settings::default()
        };

        let err = settings.validate().expect_err("must reject");
        let message = err.to_string();

        // The message has to name Ketikin's own conflict; the generic
        // registration error blames "another application" and sends the user
        // hunting for a program that isn't there.
        assert!(message.contains("Alt+K"));
        assert!(message.contains("start and stop"));
        assert!(!message.contains("another application"));
    }

    #[test]
    fn validate_ignores_case_when_comparing_accelerators() {
        let settings = Settings {
            start_hotkey: "Alt+K".to_string(),
            stop_hotkey: "alt+k".to_string(),
            ..Settings::default()
        };

        assert!(settings.validate().is_err());
    }

    #[test]
    fn validate_allows_a_clash_while_hotkeys_are_disabled() {
        // Nothing is bound, so nothing can conflict.
        let settings = Settings {
            hotkeys_enabled: false,
            start_hotkey: "Alt+K".to_string(),
            stop_hotkey: "Alt+K".to_string(),
            ..Settings::default()
        };

        settings
            .validate()
            .expect("disabled hotkeys cannot conflict");
    }

    #[test]
    fn validate_accepts_the_defaults() {
        Settings::default()
            .validate()
            .expect("defaults must be valid");
    }

    #[test]
    fn round_trips_through_storage() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("appData", tmp.path().to_path_buf())]);

        // First load with nothing on disk yields defaults.
        assert_eq!(Settings::load(&storage), Settings::default());

        let settings = Settings {
            typing_delay_ms: 40,
            theme: "light".to_string(),
            hotkeys_enabled: false,
            ..Settings::default()
        };
        settings.save(&storage).expect("save");

        assert_eq!(Settings::load(&storage), settings);
    }

    #[test]
    fn load_normalizes_an_out_of_range_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("appData", tmp.path().to_path_buf())]);

        std::fs::write(
            tmp.path().join("settings.json"),
            br#"{ "typingDelayMs": 99999, "startDelaySecs": 60, "theme": "puce" }"#,
        )
        .expect("write");

        let settings = Settings::load(&storage);
        assert_eq!(settings.typing_delay_ms, 1000);
        assert_eq!(settings.start_delay_secs, 10);
        assert_eq!(settings.theme, "system");
    }

    #[test]
    fn corrupt_settings_file_recovers_to_defaults() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("appData", tmp.path().to_path_buf())]);

        std::fs::write(tmp.path().join("settings.json"), b"\0\0not json").expect("write");

        assert_eq!(Settings::load(&storage), Settings::default());
        assert!(tmp.path().join("settings.json.bak").exists());
    }
}
