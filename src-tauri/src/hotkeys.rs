//! Global shortcut registration.
//!
//! Two slots exist, start and stop, and each is managed independently so one
//! accelerator being claimed by another application cannot disturb the other.
//!
//! Failure policy: a shortcut that cannot be registered never fails a settings
//! save, and never destroys a working binding. [`apply`] runs *before* the
//! settings are persisted and returns the accelerators that are actually in
//! force; the caller persists those. If the new accelerator is refused, the
//! previous one is put back and kept in the stored settings, so a restart
//! recovers rather than repeating the failure. The rejected accelerator is
//! reported on the `hotkey://error` event as `{ which, accelerator, message }`.
//!
//! `unregister_all` is deliberately never used here. The plugin empties its own
//! bookkeeping map *before* asking the OS to release the grabs and does not
//! restore it if that call fails, which would leave the plugin believing
//! nothing is registered while the OS still holds every grab — after which
//! every later registration fails as already-registered until restart. Targeted
//! per-accelerator `unregister` calls the OS first and only updates the map on
//! success, so that is what this module uses.

use std::str::FromStr;
use std::sync::Mutex;

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
/// actual stop happens in Rust first — see [`register_accelerator`].
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

/// One of the two shortcut slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Start,
    Stop,
}

impl Slot {
    const ALL: [Slot; 2] = [Slot::Start, Slot::Stop];

    fn as_str(self) -> &'static str {
        match self {
            Slot::Start => "start",
            Slot::Stop => "stop",
        }
    }

    fn accelerator(self, settings: &Settings) -> &str {
        match self {
            Slot::Start => &settings.start_hotkey,
            Slot::Stop => &settings.stop_hotkey,
        }
    }

    fn set_accelerator(self, settings: &mut Settings, value: String) {
        match self {
            Slot::Start => settings.start_hotkey = value,
            Slot::Stop => settings.stop_hotkey = value,
        }
    }
}

/// What is actually bound right now.
#[derive(Debug, Default)]
struct Bound {
    start: Option<String>,
    stop: Option<String>,
    /// While true the OS grabs are released so the user can capture a
    /// replacement without the current hotkey firing at them.
    suspended: bool,
}

impl Bound {
    fn get(&self, slot: Slot) -> Option<&str> {
        match slot {
            Slot::Start => self.start.as_deref(),
            Slot::Stop => self.stop.as_deref(),
        }
    }

    fn set(&mut self, slot: Slot, value: Option<String>) {
        match slot {
            Slot::Start => self.start = value,
            Slot::Stop => self.stop = value,
        }
    }
}

/// Managed state tracking which accelerators are live.
#[derive(Default)]
pub struct HotkeyHandle {
    bound: Mutex<Bound>,
}

/// Result of moving one slot to a new accelerator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotOutcome {
    /// Accelerator actually bound now; `None` means the slot ended up unbound.
    pub bound: Option<String>,
    /// User-facing failure, if the desired accelerator did not take.
    pub error: Option<String>,
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

/// Move one slot from `current` to `desired`, rolling back on refusal.
///
/// Pure apart from the two injected closures, so the rollback policy is
/// testable without an OS keyboard grab. `fallback` is what to restore if
/// `desired` is refused — normally whatever was bound before, or the previously
/// persisted value when nothing was bound.
fn move_slot(
    current: Option<&str>,
    desired: &str,
    fallback: Option<&str>,
    register: &mut dyn FnMut(&str) -> Result<(), String>,
    unregister: &mut dyn FnMut(&str),
) -> SlotOutcome {
    if current == Some(desired) {
        // Already bound to exactly this; touching the OS would only create a
        // window where the shortcut is dead.
        return SlotOutcome {
            bound: Some(desired.to_string()),
            error: None,
        };
    }

    if let Some(old) = current {
        unregister(old);
    }

    let Err(error) = register(desired) else {
        return SlotOutcome {
            bound: Some(desired.to_string()),
            error: None,
        };
    };

    // Refused. Put back whatever was working, if that is a different key.
    match fallback {
        Some(old) if old != desired => match register(old) {
            Ok(()) => SlotOutcome {
                bound: Some(old.to_string()),
                error: Some(format!(
                    "{error} Your previous shortcut {old} is still active."
                )),
            },
            Err(restore_error) => SlotOutcome {
                bound: None,
                error: Some(format!(
                    "{error} Restoring the previous shortcut {old} also failed \
                     ({restore_error}), so this shortcut is currently unavailable."
                )),
            },
        },
        // Nothing better to fall back to: either this is startup, or the value
        // did not change and simply cannot be bound in this environment.
        _ => SlotOutcome {
            bound: None,
            error: Some(error),
        },
    }
}

/// Apply `desired`, returning the settings that should actually be persisted.
///
/// Call this *before* writing settings to disk. Any slot whose new accelerator
/// was refused comes back holding the previous value, so stored settings never
/// contain a shortcut that is not really bound.
pub fn apply(app: &AppHandle, previous: &Settings, desired: &Settings) -> Settings {
    let mut effective = desired.clone();

    let Some(state) = handle(app) else {
        return effective;
    };
    let mut bound = crate::lock(&state.hotkeys.bound);

    // Actually registering an accelerator is the only way to learn whether the
    // OS will accept it, and that is what drives the rollback below — so a
    // capture suspend has to be lifted to do this work. It must not be
    // *consumed*, though: the frontend holds the suspend as a lease until the
    // capture field closes, and a debounced save fires while that field is
    // still focused. Dropping the lease here would re-arm the old hotkey under
    // the user's cursor, which is precisely the bug the suspend exists to
    // prevent. Restore it on the way out.
    let was_suspended = bound.suspended;
    bound.suspended = false;

    if !desired.hotkeys_enabled {
        for slot in Slot::ALL {
            if let Some(accelerator) = bound.get(slot) {
                unregister_accelerator(app, accelerator);
            }
            bound.set(slot, None);
        }
        bound.suspended = was_suspended;
        log::info!("hotkeys: disabled by settings");
        // Nothing is bound, so nothing can be wrong: keep what the user typed.
        return effective;
    }

    for slot in Slot::ALL {
        let current = bound.get(slot).map(str::to_string);
        let fallback = current
            .clone()
            .unwrap_or_else(|| slot.accelerator(previous).to_string());
        let wanted = slot.accelerator(desired).to_string();

        let outcome = move_slot(
            current.as_deref(),
            &wanted,
            Some(fallback.as_str()),
            &mut |accelerator| register_accelerator(app, slot, accelerator),
            &mut |accelerator| unregister_accelerator(app, accelerator),
        );

        bound.set(slot, outcome.bound.clone());
        match &outcome.bound {
            Some(accelerator) => {
                slot.set_accelerator(&mut effective, accelerator.clone());
                log::info!("hotkeys: {} shortcut bound to {accelerator}", slot.as_str());
            }
            // Unbound: keep the previously stored value rather than recording
            // an accelerator that does not work.
            None => slot.set_accelerator(&mut effective, slot.accelerator(previous).to_string()),
        }

        if let Some(message) = outcome.error {
            report(app, slot, &wanted, message);
        }
    }

    if was_suspended {
        suspend_locked(app, &mut bound);
    }

    effective
}

/// Release the OS grabs so the user can press their current hotkey into a
/// capture field without triggering it.
///
/// Registered shortcuts are grabbed globally, so without this, pressing the
/// existing start hotkey while rebinding it fires a real typing run into the
/// settings panel.
pub fn suspend(app: &AppHandle) {
    let Some(state) = handle(app) else {
        return;
    };
    let mut bound = crate::lock(&state.hotkeys.bound);
    suspend_locked(app, &mut bound);
}

/// Body of [`suspend`], for callers that already hold the lock.
fn suspend_locked(app: &AppHandle, bound: &mut Bound) {
    if bound.suspended {
        return;
    }
    for slot in Slot::ALL {
        if let Some(accelerator) = bound.get(slot) {
            unregister_accelerator(app, accelerator);
        }
    }
    bound.suspended = true;
    log::debug!("hotkeys: suspended for capture");
}

/// Re-register whatever was suspended. Idempotent, and safe to call when
/// nothing was ever suspended.
pub fn resume(app: &AppHandle) {
    let Some(state) = handle(app) else {
        return;
    };
    let mut bound = crate::lock(&state.hotkeys.bound);

    if !bound.suspended {
        return;
    }
    bound.suspended = false;

    for slot in Slot::ALL {
        let Some(accelerator) = bound.get(slot).map(ToString::to_string) else {
            continue;
        };
        if let Err(message) = register_accelerator(app, slot, &accelerator) {
            bound.set(slot, None);
            report(app, slot, &accelerator, message);
        }
    }
    log::debug!("hotkeys: resumed after capture");
}

fn handle(app: &AppHandle) -> Option<tauri::State<'_, crate::AppState>> {
    tauri::Manager::try_state::<crate::AppState>(app)
}

fn register_accelerator(app: &AppHandle, slot: Slot, accelerator: &str) -> Result<(), String> {
    let shortcut = Shortcut::from_str(accelerator.trim())
        .map_err(|err| format!("{accelerator} is not a valid shortcut ({err})."))?;

    let is_start = slot == Slot::Start;
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _, event| {
            // Fires for press *and* release; acting on both would double-trigger.
            if event.state != ShortcutState::Pressed {
                return;
            }

            if is_start {
                let _ = app.emit(EVENT_START, ());
            } else {
                // Stop runs in Rust before the event goes out, so it still
                // works when the WebView is busy or wedged.
                if let Err(err) = typing::stop(app) {
                    log::warn!("hotkeys: stop shortcut failed: {err}");
                }
                let _ = app.emit(EVENT_STOP, ());
            }
        })
        .map_err(|err| {
            format!(
                "{accelerator} could not be registered — another application may already be \
                 using it ({err})."
            )
        })
}

fn unregister_accelerator(app: &AppHandle, accelerator: &str) {
    match Shortcut::from_str(accelerator.trim()) {
        Ok(shortcut) => {
            if let Err(err) = app.global_shortcut().unregister(shortcut) {
                log::warn!("hotkeys: could not release {accelerator}: {err}");
            }
        }
        Err(err) => log::warn!("hotkeys: cannot parse {accelerator} to release it: {err}"),
    }
}

fn report(app: &AppHandle, slot: Slot, accelerator: &str, message: String) {
    log::warn!("hotkeys: {} shortcut: {message}", slot.as_str());

    let _ = app.emit(
        EVENT_ERROR,
        HotkeyError {
            which: slot.as_str().to_string(),
            accelerator: accelerator.to_string(),
            message,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;

    /// Fake OS registry: refuses anything in `blocked`, tracks what is grabbed.
    struct FakeRegistry {
        grabbed: RefCell<HashSet<String>>,
        blocked: HashSet<String>,
    }

    impl FakeRegistry {
        fn new(blocked: &[&str]) -> Self {
            Self {
                grabbed: RefCell::new(HashSet::new()),
                blocked: blocked.iter().map(|s| s.to_string()).collect(),
            }
        }

        fn run(&self, current: Option<&str>, desired: &str, fallback: Option<&str>) -> SlotOutcome {
            move_slot(
                current,
                desired,
                fallback,
                &mut |accelerator| {
                    if self.blocked.contains(accelerator) {
                        return Err(format!("{accelerator} is taken."));
                    }
                    self.grabbed.borrow_mut().insert(accelerator.to_string());
                    Ok(())
                },
                &mut |accelerator| {
                    self.grabbed.borrow_mut().remove(accelerator);
                },
            )
        }

        fn grabbed(&self) -> Vec<String> {
            let mut all: Vec<String> = self.grabbed.borrow().iter().cloned().collect();
            all.sort();
            all
        }
    }

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

    #[test]
    fn a_successful_rebind_replaces_the_old_grab() {
        let os = FakeRegistry::new(&[]);
        let outcome = os.run(Some("Alt+A"), "Alt+B", Some("Alt+A"));

        assert_eq!(outcome.bound.as_deref(), Some("Alt+B"));
        assert_eq!(outcome.error, None);
        assert_eq!(os.grabbed(), vec!["Alt+B"]);
    }

    #[test]
    fn rebinding_to_the_same_accelerator_does_not_touch_the_os() {
        let os = FakeRegistry::new(&["Alt+A"]);
        // Blocked, but already bound: it must be left alone rather than
        // released and then refused.
        let outcome = os.run(Some("Alt+A"), "Alt+A", Some("Alt+A"));

        assert_eq!(outcome.bound.as_deref(), Some("Alt+A"));
        assert_eq!(outcome.error, None);
    }

    #[test]
    fn a_refused_rebind_restores_the_previous_binding() {
        let os = FakeRegistry::new(&["Alt+TAKEN"]);
        let outcome = os.run(Some("Alt+A"), "Alt+TAKEN", Some("Alt+A"));

        // This is the blocker: the working shortcut must survive.
        assert_eq!(outcome.bound.as_deref(), Some("Alt+A"));
        assert_eq!(os.grabbed(), vec!["Alt+A"]);

        let message = outcome.error.expect("must report the failure");
        assert!(message.contains("Alt+TAKEN"));
        assert!(message.contains("Alt+A is still active"));
    }

    #[test]
    fn a_refused_rebind_with_an_unrestorable_previous_is_reported_distinctly() {
        // Both the new key and the old one are refused — the genuinely broken
        // case, which must not read like an ordinary rollback.
        let os = FakeRegistry::new(&["Alt+TAKEN", "Alt+A"]);
        let outcome = os.run(Some("Alt+A"), "Alt+TAKEN", Some("Alt+A"));

        assert_eq!(outcome.bound, None);
        let message = outcome.error.expect("must report the failure");
        assert!(message.contains("Restoring the previous shortcut Alt+A also failed"));
        assert!(message.contains("currently unavailable"));
        assert!(os.grabbed().is_empty());
    }

    #[test]
    fn a_refused_first_registration_has_nothing_to_restore() {
        let os = FakeRegistry::new(&["Alt+TAKEN"]);
        let outcome = os.run(None, "Alt+TAKEN", Some("Alt+TAKEN"));

        assert_eq!(outcome.bound, None);
        let message = outcome.error.expect("must report the failure");
        assert!(message.contains("Alt+TAKEN is taken"));
        // No misleading claim that some previous shortcut survived.
        assert!(!message.contains("still active"));
    }

    #[test]
    fn binding_a_slot_for_the_first_time_succeeds_cleanly() {
        let os = FakeRegistry::new(&[]);
        let outcome = os.run(None, "Alt+A", Some("Alt+A"));

        assert_eq!(outcome.bound.as_deref(), Some("Alt+A"));
        assert_eq!(outcome.error, None);
        assert_eq!(os.grabbed(), vec!["Alt+A"]);
    }

    #[test]
    fn bound_tracks_slots_independently() {
        let mut bound = Bound::default();
        assert_eq!(bound.get(Slot::Start), None);
        assert_eq!(bound.get(Slot::Stop), None);

        bound.set(Slot::Start, Some("Alt+A".to_string()));
        assert_eq!(bound.get(Slot::Start), Some("Alt+A"));
        assert_eq!(bound.get(Slot::Stop), None, "slots must not alias");

        bound.set(Slot::Stop, Some("Alt+B".to_string()));
        bound.set(Slot::Start, None);
        assert_eq!(bound.get(Slot::Start), None);
        assert_eq!(bound.get(Slot::Stop), Some("Alt+B"));
    }

    #[test]
    fn slot_accessors_map_to_the_right_settings_field() {
        let mut settings = Settings::default();
        assert_eq!(Slot::Start.accelerator(&settings), "CommandOrControl+Alt+T");
        assert_eq!(Slot::Stop.accelerator(&settings), "CommandOrControl+Alt+X");

        Slot::Start.set_accelerator(&mut settings, "Alt+1".to_string());
        Slot::Stop.set_accelerator(&mut settings, "Alt+2".to_string());

        assert_eq!(settings.start_hotkey, "Alt+1");
        assert_eq!(settings.stop_hotkey, "Alt+2");
        assert_eq!(Slot::Start.as_str(), "start");
        assert_eq!(Slot::Stop.as_str(), "stop");
    }
}
