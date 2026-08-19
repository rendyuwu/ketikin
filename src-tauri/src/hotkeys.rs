//! Global shortcut registration.
//!
//! Two slots exist, start and stop, and each is managed independently so one
//! accelerator being claimed by another application cannot disturb the other.
//!
//! Failure policy: a shortcut that cannot be registered never fails a settings
//! save, and never destroys a working binding. [`apply`] runs *before* the
//! settings are persisted and returns the accelerators that are actually in
//! force; the caller persists those. If the new accelerator is refused, the
//! whole pass rolls back and the previous accelerators are kept in the stored
//! settings, so a restart recovers rather than repeating the failure.
//!
//! Every failure is reported twice, because neither channel alone is enough.
//! `hotkey://error` carries `{ which, accelerator, message }` and describes
//! something happening now, but `emit` does not buffer and the startup pass runs
//! before any listener exists; [`HotkeyHandle::status`] is the pull-based
//! counterpart the `hotkey_status` command serves, holding at most one failure
//! per slot and clearing it the moment that slot binds successfully.
//!
//! `unregister_all` is deliberately never used here. The plugin empties its own
//! bookkeeping map *before* asking the OS to release the grabs and does not
//! restore it if that call fails, which would leave the plugin believing
//! nothing is registered while the OS still holds every grab — after which
//! every later registration fails as already-registered until restart. Targeted
//! per-accelerator `unregister` calls the OS first and only updates the map on
//! success, so that is what this module uses. The flip side is that a *failed*
//! targeted unregister leaves a genuinely live grab, which is why
//! [`Bound::leaked`] remembers those and they reach the user rather than a
//! `log::warn` nobody reads.
//!
//! Threading: the plugin marshals every register and unregister onto the main
//! thread and blocks until it has run. So `bound` may be held only by code that
//! is *not* the main thread — see `AppState` — and events are emitted after the
//! guard is dropped, never under it.

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

/// Payload of [`EVENT_ERROR`], and the element type of [`HotkeyStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyError {
    /// `"start"` or `"stop"`.
    pub which: String,
    /// The accelerator the message is about.
    ///
    /// For a registration failure this is the one that was refused. For a
    /// *release* failure it is the **old** accelerator — the one that may still
    /// be grabbed — which is not what the field the frontend renders it under
    /// now contains. That is why only `message` is displayed.
    pub accelerator: String,
    pub message: String,
}

/// Reply from the `hotkey_status` command.
///
/// An object with a single `failures` key rather than a bare array, so the
/// reply can grow another field without breaking the frontend's parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    /// At most one entry per slot, empty when everything is bound.
    pub failures: Vec<HotkeyError>,
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

    /// Position in the per-slot arrays. `Slot::ALL` is in this order.
    fn index(self) -> usize {
        match self {
            Slot::Start => 0,
            Slot::Stop => 1,
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

/// What each slot owns right now.
///
/// While [`Bound::suspended`] is set the OS holds nothing; the accelerators
/// recorded here are then an *intent* that [`resume`] will claim.
#[derive(Debug, Default)]
struct Bound {
    start: Option<String>,
    stop: Option<String>,
    /// While true the OS grabs are released so the user can capture a
    /// replacement without the current hotkey firing at them.
    suspended: bool,
    /// Accelerators the OS refused to release.
    ///
    /// It still holds them, so they keep firing until the process exits — and a
    /// later registration of one of them comes back "already registered"
    /// because of Ketikin, not because of another application. Without this the
    /// user gets told to go hunting for a program that does not exist.
    leaked: Vec<String>,
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
    /// At most one failure per slot, indexed by [`Slot::index`].
    ///
    /// A separate mutex from `bound` on purpose. `hotkey_status` is served on
    /// the main thread, and `bound` is held by workers across calls that need
    /// the main thread; this one is never held across anything at all.
    failures: Mutex<[Option<HotkeyError>; 2]>,
}

impl HotkeyHandle {
    /// Snapshot for the `hotkey_status` command.
    pub fn status(&self) -> HotkeyStatus {
        HotkeyStatus {
            failures: crate::lock(&self.failures)
                .iter()
                .flatten()
                .cloned()
                .collect(),
        }
    }

    /// Replace a slot's failure. `None` clears it.
    fn set_failure(&self, slot: Slot, failure: Option<HotkeyError>) {
        crate::lock(&self.failures)[slot.index()] = failure;
    }
}

/// A failure to record against a slot, before the slot label is attached.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Failure {
    accelerator: String,
    message: String,
}

impl Failure {
    fn labelled(self, slot: Slot) -> HotkeyError {
        HotkeyError {
            which: slot.as_str().to_string(),
            accelerator: self.accelerator,
            message: self.message,
        }
    }
}

/// Why a registration did not take.
///
/// Kept apart from its wording because the caller is the only thing that knows
/// whether the conflicting grab is Ketikin's own.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Refusal {
    /// The string is not something the OS could even be asked for.
    Unparseable(String),
    /// The OS was asked, and said no.
    Rejected(String),
}

impl Refusal {
    /// The underlying error text, for messages that already carry their own
    /// framing and only need the reason.
    fn reason(&self) -> &str {
        match self {
            Self::Unparseable(err) | Self::Rejected(err) => err,
        }
    }

    fn describe(&self, accelerator: &str, ours: bool) -> String {
        match self {
            Self::Unparseable(err) => format!("{accelerator} is not a valid shortcut ({err})."),
            Self::Rejected(err) if ours => format!(
                "{accelerator} could not be registered because Ketikin is still holding it \
                 ({err}). Restart Ketikin to release it."
            ),
            Self::Rejected(err) => format!(
                "{accelerator} could not be registered — another application may already be \
                 using it ({err})."
            ),
        }
    }
}

/// Wording for a release the OS refused.
///
/// The plugin calls the OS first and only forgets the shortcut on success, so
/// this means the grab is genuinely still live: the old chord keeps starting
/// typing runs even though nothing in Ketikin believes it is bound.
fn release_failed(accelerator: &str, err: &str) -> String {
    format!(
        "Ketikin could not release the old shortcut {accelerator} ({err}), so pressing it may \
         still start typing. Restarting Ketikin clears it."
    )
}

/// Remember an accelerator the OS would not give back, without duplicating it.
fn note_leak(leaked: &mut Vec<String>, accelerator: &str) {
    if !leaked.iter().any(|held| held == accelerator) {
        leaked.push(accelerator.to_string());
    }
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

/// What one two-phase pass decided. Per-slot arrays are indexed by
/// [`Slot::index`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct PassOutcome {
    /// Accelerator each slot holds afterwards; `None` means unbound.
    bound: [Option<String>; 2],
    /// The slot's failure after this pass. `None` clears whatever was there,
    /// so a successful rebind cannot leave a stale error behind.
    failures: [Option<Failure>; 2],
    /// Accelerators the OS still holds despite being asked to release them.
    leaked: Vec<String>,
}

/// Move both slots to `desired` in two phases, rolling the whole pass back on
/// any refusal.
///
/// Pure apart from the two injected closures, so the collision and rollback
/// policy is testable without an OS keyboard grab.
///
/// The phases exist because walking the slots one at a time cannot express a
/// swap. With start on `Alt+A` and stop on `Alt+B`, a user trading the two in a
/// single Settings visit passes `Settings::validate` — which only sees that the
/// two *desired* values differ — and then the start slot releases `Alt+A`, asks
/// for `Alt+B`, and is refused because the stop slot still holds it. The stop
/// slot fails symmetrically, nothing changes, and two errors send the user
/// hunting for a conflicting application that is Ketikin. Releasing every
/// changing slot before claiming anything removes the collision entirely.
///
/// `fallback` is the last persisted accelerator per slot, used as the rollback
/// target when a slot was not bound to anything to begin with.
fn apply_pass(
    current: [Option<String>; 2],
    desired: [&str; 2],
    fallback: [&str; 2],
    leaked: Vec<String>,
    register: &mut dyn FnMut(Slot, &str) -> Result<(), Refusal>,
    unregister: &mut dyn FnMut(&str) -> Result<(), String>,
) -> PassOutcome {
    let mut bound = current.clone();
    let mut failures: [Option<Failure>; 2] = [None, None];
    let mut leaked = leaked;

    // A slot already holding exactly what it wants is left alone: touching the
    // OS would only open a window where the shortcut is dead.
    let changing = [
        current[0].as_deref() != Some(desired[0]),
        current[1].as_deref() != Some(desired[1]),
    ];

    // Phase 1 — release.
    for slot in Slot::ALL {
        let index = slot.index();
        if !changing[index] {
            continue;
        }
        let Some(old) = bound[index].take() else {
            continue;
        };

        match unregister(&old) {
            Ok(()) => leaked.retain(|held| held != &old),
            Err(err) => {
                note_leak(&mut leaked, &old);
                failures[index] = Some(Failure {
                    message: release_failed(&old, &err),
                    accelerator: old,
                });
            }
        }
    }

    // Phase 2 — claim.
    let mut claimed: Vec<(usize, String)> = Vec::new();
    let mut refused: Option<(usize, String, String)> = None;

    for slot in Slot::ALL {
        let index = slot.index();
        if !changing[index] {
            continue;
        }
        let wanted = desired[index];

        match register(slot, wanted) {
            Ok(()) => {
                bound[index] = Some(wanted.to_string());
                leaked.retain(|held| held != wanted);
                claimed.push((index, wanted.to_string()));
            }
            Err(refusal) => {
                // The only two conflicts we can actually attribute: a grab
                // Ketikin asked for and never got back, and one its other slot
                // is still holding.
                let ours = leaked.iter().any(|held| held == wanted)
                    || bound[1 - index].as_deref() == Some(wanted);
                refused = Some((index, wanted.to_string(), refusal.describe(wanted, ours)));
                break;
            }
        }
    }

    let Some((failed, wanted, refusal)) = refused else {
        return PassOutcome {
            bound,
            failures,
            leaked,
        };
    };

    // Rolled back in full rather than left half-applied: the caller persists
    // whatever comes back here, and a pass that moved one slot but not the
    // other would write a pairing the user never asked for.
    for (index, accelerator) in claimed {
        bound[index] = None;
        if let Err(err) = unregister(&accelerator) {
            note_leak(&mut leaked, &accelerator);
            failures[index] = Some(Failure {
                message: release_failed(&accelerator, &err),
                accelerator,
            });
        }
    }

    for slot in Slot::ALL {
        let index = slot.index();
        if !changing[index] {
            continue;
        }

        // Whatever the slot had, or the last value persisted for it if it was
        // not bound at all.
        let restore = current[index]
            .clone()
            .unwrap_or_else(|| fallback[index].to_string());
        if restore == desired[index] {
            // Nothing better to go back to: either this is startup, or the
            // value did not change and simply cannot be bound here.
            continue;
        }

        match register(slot, &restore) {
            Ok(()) => {
                leaked.retain(|held| held != &restore);
                bound[index] = Some(restore.clone());
                if index == failed {
                    failures[index] = Some(Failure {
                        accelerator: wanted.clone(),
                        message: format!(
                            "{refusal} Your previous shortcut {restore} is still active."
                        ),
                    });
                }
            }
            Err(err) => {
                let reason = err.reason();
                failures[index] = Some(if index == failed {
                    Failure {
                        accelerator: wanted.clone(),
                        message: format!(
                            "{refusal} Restoring the previous shortcut {restore} also failed \
                             ({reason}), so this shortcut is currently unavailable."
                        ),
                    }
                } else {
                    Failure {
                        message: format!(
                            "{restore} could not be registered again after the other shortcut \
                             was refused ({reason}), so it is currently unavailable."
                        ),
                        accelerator: restore,
                    }
                });
            }
        }
    }

    // The refused slot may have had nothing to fall back to at all.
    if failures[failed].is_none() {
        failures[failed] = Some(Failure {
            accelerator: wanted,
            message: refusal,
        });
    }

    PassOutcome {
        bound,
        failures,
        leaked,
    }
}

/// Apply `desired`, returning the settings that should actually be persisted.
///
/// Call this *before* writing settings to disk. Any slot whose new accelerator
/// was refused comes back holding the previous value, so stored settings never
/// contain a shortcut that is not really bound.
pub fn apply(app: &AppHandle, previous: &Settings, desired: &Settings) -> Settings {
    let (effective, errors) = apply_deferred(app, previous, desired);

    for error in errors {
        let _ = app.emit(EVENT_ERROR, error);
    }
    effective
}

/// [`apply`] without emitting, for a caller that needs to time the events.
///
/// Failures are recorded for `hotkey_status` immediately; only the events are
/// handed back. `setup` uses this so the startup emission can be held until the
/// WebView has mounted, exactly like `storage://warning` and
/// `tray://unavailable` — an unheard `hotkey://error` is how a chord another
/// application owns ends up rendered in Settings as though it were bound.
pub fn apply_deferred(
    app: &AppHandle,
    previous: &Settings,
    desired: &Settings,
) -> (Settings, Vec<HotkeyError>) {
    let mut effective = desired.clone();

    let Some(state) = handle(app) else {
        return (effective, Vec::new());
    };

    // Decided under the lock, published after it is dropped. See the module
    // header: the guard must not be held across anything that needs the main
    // thread, and under some build configurations `emit` does.
    let mut decided: Vec<(Slot, Option<Failure>)> = Vec::new();
    {
        let mut bound = crate::lock(&state.hotkeys.bound);

        if !desired.hotkeys_enabled {
            let suspended = bound.suspended;
            for slot in Slot::ALL {
                // While a capture is suspended the OS holds nothing — `bound`
                // is recording intent, not grabs — so there is nothing to
                // release.
                match bound.get(slot).map(str::to_string) {
                    Some(accelerator) if !suspended => {
                        match unregister_accelerator(app, &accelerator) {
                            Ok(()) => {
                                bound.leaked.retain(|held| held != &accelerator);
                                decided.push((slot, None));
                            }
                            Err(err) => {
                                note_leak(&mut bound.leaked, &accelerator);
                                decided.push((
                                    slot,
                                    Some(Failure {
                                        message: release_failed(&accelerator, &err),
                                        accelerator,
                                    }),
                                ));
                            }
                        }
                    }
                    _ => decided.push((slot, None)),
                }
                bound.set(slot, None);
            }
            log::info!("hotkeys: disabled by settings");
            // Nothing is bound, so nothing can be wrong: keep what the user
            // typed.
        } else if bound.suspended {
            // A capture is in progress. Registering now would re-arm the OS
            // grabs for exactly as long as this function runs, and a debounced
            // save lands while the capture field still has focus — so the chord
            // the user is pressing into that field would fire a real typing run
            // into the settings panel, which is the entire reason the suspend
            // exists. Record the intent instead and let `resume` claim it, and
            // report whatever it finds then.
            for slot in Slot::ALL {
                bound.set(slot, Some(slot.accelerator(desired).to_string()));
                decided.push((slot, None));
            }
            log::info!("hotkeys: capture in progress, registration deferred to resume");
        } else {
            let outcome = apply_pass(
                [
                    bound.get(Slot::Start).map(str::to_string),
                    bound.get(Slot::Stop).map(str::to_string),
                ],
                [
                    Slot::Start.accelerator(desired),
                    Slot::Stop.accelerator(desired),
                ],
                [
                    Slot::Start.accelerator(previous),
                    Slot::Stop.accelerator(previous),
                ],
                std::mem::take(&mut bound.leaked),
                &mut |slot, accelerator| register_accelerator(app, slot, accelerator),
                &mut |accelerator| unregister_accelerator(app, accelerator),
            );

            bound.leaked = outcome.leaked;
            for slot in Slot::ALL {
                let index = slot.index();
                bound.set(slot, outcome.bound[index].clone());

                match &outcome.bound[index] {
                    Some(accelerator) => {
                        slot.set_accelerator(&mut effective, accelerator.clone());
                        log::info!("hotkeys: {} shortcut bound to {accelerator}", slot.as_str());
                    }
                    // Unbound: keep the previously stored value rather than
                    // recording an accelerator that does not work.
                    None => {
                        slot.set_accelerator(&mut effective, slot.accelerator(previous).to_string())
                    }
                }
                decided.push((slot, outcome.failures[index].clone()));
            }
        }
    }

    let errors = decided
        .into_iter()
        .filter_map(|(slot, failure)| publish(&state.hotkeys, slot, failure))
        .collect();

    (effective, errors)
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

    let mut decided: Vec<(Slot, Option<Failure>)> = Vec::new();
    {
        let mut bound = crate::lock(&state.hotkeys.bound);
        if bound.suspended {
            return;
        }

        for slot in Slot::ALL {
            let Some(accelerator) = bound.get(slot).map(str::to_string) else {
                continue;
            };
            // Only failures are recorded here. A suspend is not a rebind, so a
            // clean release must not clear a failure the slot already carries.
            match unregister_accelerator(app, &accelerator) {
                Ok(()) => bound.leaked.retain(|held| held != &accelerator),
                Err(err) => {
                    note_leak(&mut bound.leaked, &accelerator);
                    decided.push((
                        slot,
                        Some(Failure {
                            message: release_failed(&accelerator, &err),
                            accelerator,
                        }),
                    ));
                }
            }
        }
        bound.suspended = true;
        log::debug!("hotkeys: suspended for capture");
    }

    announce(app, &state.hotkeys, decided);
}

/// Re-register whatever was suspended. Idempotent, and safe to call when
/// nothing was ever suspended.
///
/// This is also where accelerators a save recorded *during* the capture are
/// finally claimed — see [`apply_deferred`] — so it is the point at which those
/// registrations succeed or are reported.
pub fn resume(app: &AppHandle) {
    let Some(state) = handle(app) else {
        return;
    };

    let mut decided: Vec<(Slot, Option<Failure>)> = Vec::new();
    {
        let mut bound = crate::lock(&state.hotkeys.bound);
        if !bound.suspended {
            return;
        }
        bound.suspended = false;

        for slot in Slot::ALL {
            let Some(accelerator) = bound.get(slot).map(str::to_string) else {
                continue;
            };
            match register_accelerator(app, slot, &accelerator) {
                Ok(()) => {
                    bound.leaked.retain(|held| held != &accelerator);
                    decided.push((slot, None));
                }
                Err(refusal) => {
                    let ours = bound.leaked.iter().any(|held| held == &accelerator);
                    bound.set(slot, None);
                    decided.push((
                        slot,
                        Some(Failure {
                            message: refusal.describe(&accelerator, ours),
                            accelerator,
                        }),
                    ));
                }
            }
        }
        log::debug!("hotkeys: resumed after capture");
    }

    announce(app, &state.hotkeys, decided);
}

fn handle(app: &AppHandle) -> Option<tauri::State<'_, crate::AppState>> {
    tauri::Manager::try_state::<crate::AppState>(app)
}

/// Record a slot's outcome and return the event that should go out, if any.
///
/// `None` clears the slot, which is what keeps `hotkey_status` from
/// accumulating: a late poll can never hand back an error the user has since
/// fixed by rebinding.
fn publish(handle: &HotkeyHandle, slot: Slot, failure: Option<Failure>) -> Option<HotkeyError> {
    let error = failure.map(|failure| failure.labelled(slot));
    handle.set_failure(slot, error.clone());

    if let Some(error) = &error {
        log::warn!("hotkeys: {} shortcut: {}", slot.as_str(), error.message);
    }
    error
}

/// Record and emit a batch of outcomes. Call with the `bound` guard dropped.
fn announce(app: &AppHandle, handle: &HotkeyHandle, decided: Vec<(Slot, Option<Failure>)>) {
    for (slot, failure) in decided {
        if let Some(error) = publish(handle, slot, failure) {
            let _ = app.emit(EVENT_ERROR, error);
        }
    }
}

fn register_accelerator(app: &AppHandle, slot: Slot, accelerator: &str) -> Result<(), Refusal> {
    let shortcut = Shortcut::from_str(accelerator.trim())
        .map_err(|err| Refusal::Unparseable(err.to_string()))?;

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
        .map_err(|err| Refusal::Rejected(err.to_string()))
}

/// Release one grab, reporting whether the OS actually let go.
///
/// A failure is not cosmetic: the plugin only forgets the shortcut once the OS
/// has accepted, so the grab stays live and keeps firing while Ketikin's own
/// bookkeeping moves on without it.
fn unregister_accelerator(app: &AppHandle, accelerator: &str) -> Result<(), String> {
    let shortcut = Shortcut::from_str(accelerator.trim()).map_err(|err| err.to_string())?;

    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashSet;

    /// Fake OS registry.
    ///
    /// Refuses anything in `blocked`, refuses to *release* anything in
    /// `sticky`, and — the part that makes the swap test mean something —
    /// refuses to hand out a grab it is already holding, exactly as the real
    /// one answers `AlreadyRegistered`.
    struct FakeRegistry {
        grabbed: RefCell<HashSet<String>>,
        blocked: HashSet<String>,
        sticky: HashSet<String>,
    }

    impl FakeRegistry {
        fn new(blocked: &[&str]) -> Self {
            Self {
                grabbed: RefCell::new(HashSet::new()),
                blocked: blocked.iter().map(|s| s.to_string()).collect(),
                sticky: HashSet::new(),
            }
        }

        /// Grabs that the fake OS will never release.
        fn sticky(mut self, accelerators: &[&str]) -> Self {
            self.sticky = accelerators.iter().map(|s| s.to_string()).collect();
            self
        }

        /// Pre-seed the OS with grabs, as if an earlier pass had made them.
        fn holding(self, accelerators: &[&str]) -> Self {
            self.grabbed
                .borrow_mut()
                .extend(accelerators.iter().map(|s| s.to_string()));
            self
        }

        /// Drive one whole two-phase pass.
        fn apply(
            &self,
            current: [Option<&str>; 2],
            desired: [&str; 2],
            fallback: [&str; 2],
            leaked: &[&str],
        ) -> PassOutcome {
            apply_pass(
                [
                    current[0].map(str::to_string),
                    current[1].map(str::to_string),
                ],
                desired,
                fallback,
                leaked.iter().map(|s| s.to_string()).collect(),
                &mut |_slot, accelerator: &str| {
                    if self.blocked.contains(accelerator) {
                        return Err(Refusal::Rejected(format!("{accelerator} is taken")));
                    }
                    if !self.grabbed.borrow_mut().insert(accelerator.to_string()) {
                        return Err(Refusal::Rejected("already registered".to_string()));
                    }
                    Ok(())
                },
                &mut |accelerator: &str| {
                    if self.sticky.contains(accelerator) {
                        return Err("access denied".to_string());
                    }
                    self.grabbed.borrow_mut().remove(accelerator);
                    Ok(())
                },
            )
        }

        /// Convenience for the single-slot cases: only the start slot moves,
        /// and the stop slot is parked on something nothing else touches.
        fn run(&self, current: Option<&str>, desired: &str, fallback: &str) -> Failed {
            let outcome = self.apply(
                [current, Some(PARKED)],
                [desired, PARKED],
                [fallback, PARKED],
                &[],
            );

            Failed {
                bound: outcome.bound[0].clone(),
                failure: outcome.failures[0].clone(),
            }
        }

        fn grabbed(&self) -> Vec<String> {
            let mut all: Vec<String> = self.grabbed.borrow().iter().cloned().collect();
            all.sort();
            all
        }
    }

    /// The stop slot's parking accelerator for single-slot tests.
    const PARKED: &str = "Alt+PARKED";

    /// What a single-slot pass did to the start slot.
    struct Failed {
        bound: Option<String>,
        failure: Option<Failure>,
    }

    impl Failed {
        fn message(&self) -> String {
            self.failure
                .as_ref()
                .expect("must report the failure")
                .message
                .clone()
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
        let outcome = os.run(Some("Alt+A"), "Alt+B", "Alt+A");

        assert_eq!(outcome.bound.as_deref(), Some("Alt+B"));
        assert_eq!(outcome.failure, None);
        assert_eq!(os.grabbed(), vec!["Alt+B"]);
    }

    #[test]
    fn rebinding_to_the_same_accelerator_does_not_touch_the_os() {
        let os = FakeRegistry::new(&["Alt+A"]);
        // Blocked, but already bound: it must be left alone rather than
        // released and then refused.
        let outcome = os.run(Some("Alt+A"), "Alt+A", "Alt+A");

        assert_eq!(outcome.bound.as_deref(), Some("Alt+A"));
        assert_eq!(outcome.failure, None);
    }

    #[test]
    fn a_refused_rebind_restores_the_previous_binding() {
        let os = FakeRegistry::new(&["Alt+TAKEN"]);
        let outcome = os.run(Some("Alt+A"), "Alt+TAKEN", "Alt+A");

        // This is the blocker: the working shortcut must survive.
        assert_eq!(outcome.bound.as_deref(), Some("Alt+A"));
        assert_eq!(os.grabbed(), vec!["Alt+A"]);

        let message = outcome.message();
        assert!(message.contains("Alt+TAKEN"));
        assert!(message.contains("Alt+A is still active"));
    }

    #[test]
    fn a_refused_rebind_with_an_unrestorable_previous_is_reported_distinctly() {
        // Both the new key and the old one are refused — the genuinely broken
        // case, which must not read like an ordinary rollback.
        let os = FakeRegistry::new(&["Alt+TAKEN", "Alt+A"]);
        let outcome = os.run(Some("Alt+A"), "Alt+TAKEN", "Alt+A");

        assert_eq!(outcome.bound, None);
        let message = outcome.message();
        assert!(message.contains("Restoring the previous shortcut Alt+A also failed"));
        assert!(message.contains("currently unavailable"));
        assert!(os.grabbed().is_empty());
    }

    #[test]
    fn a_refused_first_registration_has_nothing_to_restore() {
        let os = FakeRegistry::new(&["Alt+TAKEN"]);
        let outcome = os.run(None, "Alt+TAKEN", "Alt+TAKEN");

        assert_eq!(outcome.bound, None);
        let message = outcome.message();
        assert!(message.contains("Alt+TAKEN is taken"));
        // No misleading claim that some previous shortcut survived.
        assert!(!message.contains("still active"));
    }

    #[test]
    fn binding_a_slot_for_the_first_time_succeeds_cleanly() {
        let os = FakeRegistry::new(&[]);
        let outcome = os.run(None, "Alt+A", "Alt+A");

        assert_eq!(outcome.bound.as_deref(), Some("Alt+A"));
        assert_eq!(outcome.failure, None);
        assert_eq!(os.grabbed(), vec!["Alt+A"]);
    }

    #[test]
    fn the_two_slots_can_trade_accelerators_in_one_pass() {
        // The regression this whole two-phase shape exists for. Sequentially,
        // start releases Alt+A and asks for Alt+B while the stop slot still
        // holds it — `AlreadyRegistered`, rollback, and both slots blame an
        // application that does not exist.
        let os = FakeRegistry::new(&[]).holding(&["Alt+A", "Alt+B"]);
        let outcome = os.apply(
            [Some("Alt+A"), Some("Alt+B")],
            ["Alt+B", "Alt+A"],
            ["Alt+A", "Alt+B"],
            &[],
        );

        assert_eq!(outcome.bound[0].as_deref(), Some("Alt+B"));
        assert_eq!(outcome.bound[1].as_deref(), Some("Alt+A"));
        assert_eq!(outcome.failures, [None, None], "a swap is not an error");
        assert_eq!(os.grabbed(), vec!["Alt+A", "Alt+B"]);
    }

    #[test]
    fn one_slots_new_value_may_be_the_others_old_value() {
        // The more reachable half of the same bug: only the start slot moves,
        // onto the accelerator the stop slot is giving up in the same save.
        let os = FakeRegistry::new(&[]).holding(&["Alt+A", "Alt+B"]);
        let outcome = os.apply(
            [Some("Alt+A"), Some("Alt+B")],
            ["Alt+B", "Alt+C"],
            ["Alt+A", "Alt+B"],
            &[],
        );

        assert_eq!(outcome.bound[0].as_deref(), Some("Alt+B"));
        assert_eq!(outcome.bound[1].as_deref(), Some("Alt+C"));
        assert_eq!(outcome.failures, [None, None]);
    }

    #[test]
    fn a_refusal_rolls_the_whole_pass_back_not_just_its_own_slot() {
        // Start moves fine, stop is refused. Leaving start on its new value
        // would persist a pairing the user never asked for.
        let os = FakeRegistry::new(&["Alt+TAKEN"]).holding(&["Alt+A", "Alt+B"]);
        let outcome = os.apply(
            [Some("Alt+A"), Some("Alt+B")],
            ["Alt+C", "Alt+TAKEN"],
            ["Alt+A", "Alt+B"],
            &[],
        );

        assert_eq!(outcome.bound[0].as_deref(), Some("Alt+A"));
        assert_eq!(outcome.bound[1].as_deref(), Some("Alt+B"));
        assert_eq!(
            os.grabbed(),
            vec!["Alt+A", "Alt+B"],
            "Alt+C must be given up"
        );

        assert_eq!(outcome.failures[0], None, "only the refused slot reports");
        let failure = outcome.failures[1].as_ref().expect("stop must report");
        assert_eq!(failure.accelerator, "Alt+TAKEN");
        assert!(failure.message.contains("Alt+B is still active"));
    }

    #[test]
    fn a_slot_released_but_never_reclaimed_is_put_back_too() {
        // The mirror of the case above: the *first* slot is refused, so the
        // second is released in phase 1 and then never even attempted. Leaving
        // it unbound would kill a working shortcut over an unrelated failure.
        let os = FakeRegistry::new(&["Alt+TAKEN"]).holding(&["Alt+A", "Alt+B"]);
        let outcome = os.apply(
            [Some("Alt+A"), Some("Alt+B")],
            ["Alt+TAKEN", "Alt+C"],
            ["Alt+A", "Alt+B"],
            &[],
        );

        assert_eq!(outcome.bound[0].as_deref(), Some("Alt+A"));
        assert_eq!(outcome.bound[1].as_deref(), Some("Alt+B"));
        assert_eq!(os.grabbed(), vec!["Alt+A", "Alt+B"]);
        assert_eq!(outcome.failures[1], None, "the stop slot is fine");
        assert!(outcome.failures[0]
            .as_ref()
            .expect("start must report")
            .message
            .contains("Alt+A is still active"));
    }

    #[test]
    fn a_release_the_os_refuses_is_reported_and_remembered() {
        // The rebind itself works, so the field shows Alt+B — but Alt+A is
        // still grabbed and still starts typing runs. Only `message` says so,
        // which is why the frontend renders that and not `accelerator`.
        let os = FakeRegistry::new(&[])
            .sticky(&["Alt+A"])
            .holding(&["Alt+A"]);
        let outcome = os.apply(
            [Some("Alt+A"), Some(PARKED)],
            ["Alt+B", PARKED],
            ["Alt+A", PARKED],
            &[],
        );

        assert_eq!(outcome.bound[0].as_deref(), Some("Alt+B"));
        let failure = outcome.failures[0].as_ref().expect("must report");
        assert_eq!(failure.accelerator, "Alt+A", "the *old* accelerator");
        assert!(failure.message.contains("may still start typing"));
        assert!(failure.message.contains("Restarting Ketikin"));
        assert_eq!(
            outcome.leaked,
            vec!["Alt+A"],
            "remembered for the next pass"
        );
    }

    #[test]
    fn a_conflict_with_our_own_leaked_grab_does_not_blame_another_application() {
        // Alt+X is one Ketikin asked for and never got back. Telling the user
        // to go find the application holding it sends them after themselves.
        let os = FakeRegistry::new(&["Alt+X"]);
        let outcome = os.apply(
            [None, Some(PARKED)],
            ["Alt+X", PARKED],
            ["Alt+X", PARKED],
            &["Alt+X"],
        );

        let message = outcome.failures[0]
            .as_ref()
            .expect("must report")
            .message
            .clone();
        assert!(message.contains("Ketikin is still holding it"), "{message}");
        assert!(message.contains("Restart Ketikin"));
        assert!(!message.contains("another application"));
    }

    #[test]
    fn a_leak_is_forgotten_once_the_accelerator_binds_again() {
        let os = FakeRegistry::new(&[]);
        let outcome = os.apply(
            [None, Some(PARKED)],
            ["Alt+X", PARKED],
            ["Alt+X", PARKED],
            &["Alt+X"],
        );

        assert_eq!(outcome.bound[0].as_deref(), Some("Alt+X"));
        assert!(
            outcome.leaked.is_empty(),
            "a successful grab proves it was released"
        );
    }

    #[test]
    fn a_successful_pass_clears_the_slots_previous_failure() {
        // `failures` is what `hotkey_status` serves, so a stale entry means a
        // late poll can resurrect an error the user has already fixed.
        let handle = HotkeyHandle::default();
        publish(
            &handle,
            Slot::Start,
            Some(Failure {
                accelerator: "Alt+A".to_string(),
                message: "nope".to_string(),
            }),
        );
        assert_eq!(handle.status().failures.len(), 1);

        publish(&handle, Slot::Start, None);
        assert!(handle.status().failures.is_empty());
    }

    #[test]
    fn a_slot_holds_at_most_one_failure_and_the_slots_are_independent() {
        let handle = HotkeyHandle::default();

        for message in ["first", "second"] {
            publish(
                &handle,
                Slot::Start,
                Some(Failure {
                    accelerator: "Alt+A".to_string(),
                    message: message.to_string(),
                }),
            );
        }
        publish(
            &handle,
            Slot::Stop,
            Some(Failure {
                accelerator: "Alt+B".to_string(),
                message: "stop failed".to_string(),
            }),
        );

        let failures = handle.status().failures;
        assert_eq!(failures.len(), 2, "one per slot, not one per event");
        assert_eq!(failures[0].which, "start");
        assert_eq!(failures[0].message, "second", "the latest wins");
        assert_eq!(failures[1].which, "stop");
    }

    #[test]
    fn hotkey_status_is_an_object_with_a_camel_case_failures_array() {
        // The frontend is committed against this exact shape.
        let handle = HotkeyHandle::default();
        assert_eq!(
            serde_json::to_string(&handle.status()).expect("serialize"),
            r#"{"failures":[]}"#
        );

        publish(
            &handle,
            Slot::Stop,
            Some(Failure {
                accelerator: "Alt+B".to_string(),
                message: "taken".to_string(),
            }),
        );

        let json = serde_json::to_string(&handle.status()).expect("serialize");
        assert_eq!(
            json,
            r#"{"failures":[{"which":"stop","accelerator":"Alt+B","message":"taken"}]}"#
        );
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
