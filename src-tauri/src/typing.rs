//! The cancellable typing engine.
//!
//! A run lives entirely on a dedicated OS thread so neither the async runtime
//! nor the UI thread is ever blocked by a multi-minute sleep loop. The thread
//! owns the `Enigo` connection; the rest of the app talks to it through a
//! shared [`AtomicBool`] cancellation flag and a small mutex-protected state
//! snapshot.
//!
//! Invariants this module is careful about:
//! - Exactly one terminal `typing://done` is emitted per accepted run, on every
//!   exit path including panics (enforced by [`RunGuard`]).
//! - No modifier key is ever left held down. A stuck Shift would corrupt
//!   whatever the user types next in a remote console.
//! - `typing://state` is coalesced to at most ~20 events/second, so a 1 ms
//!   delay does not flood the IPC bridge with a million messages.
//! - The tray's run mark is set and cleared by the same [`RunGuard`], so it
//!   cannot be left showing a run that has ended — including after a panic.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use enigo::{Direction, Enigo, InputError, Key, Keyboard, NewConError, Settings as EnigoSettings};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::error::AppError;
use crate::settings::Settings;
use crate::AppState;

pub const EVENT_STATE: &str = "typing://state";
pub const EVENT_DONE: &str = "typing://done";

/// Refuse anything larger; at the default 25 ms this is already ~7 hours.
pub const MAX_CHARS: usize = 1_000_000;

/// Upper bound on how often `typing://state` is emitted while typing.
const EMIT_INTERVAL: Duration = Duration::from_millis(50);

/// Longest a sleep runs before the cancellation flag is re-checked.
const CANCEL_SLICE: Duration = Duration::from_millis(25);

/// How long `start_typing` waits for the worker to report whether the keyboard
/// backend came up. Enigo initialisation is milliseconds in practice.
const INIT_TIMEOUT: Duration = Duration::from_secs(5);

const ALREADY_TYPING: &str =
    "Ketikin is already typing. Stop the current run before starting another one.";
const STATE_MISSING: &str = "Ketikin is still starting up. Try again in a moment.";

/// Progress snapshot mirrored to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingState {
    /// `"idle" | "countdown" | "typing"`.
    pub phase: String,
    pub typed: u32,
    pub total: u32,
    pub countdown: u32,
}

impl TypingState {
    fn idle() -> Self {
        Self {
            phase: "idle".to_string(),
            typed: 0,
            total: 0,
            countdown: 0,
        }
    }
}

impl Default for TypingState {
    fn default() -> Self {
        Self::idle()
    }
}

/// Terminal `typing://done` payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    /// `"completed" | "stopped" | "error"`.
    pub reason: String,
    pub message: Option<String>,
}

impl Done {
    fn completed() -> Self {
        Self {
            reason: "completed".to_string(),
            message: None,
        }
    }

    fn stopped() -> Self {
        Self {
            reason: "stopped".to_string(),
            message: None,
        }
    }

    fn error(message: String) -> Self {
        Self {
            reason: "error".to_string(),
            message: Some(message),
        }
    }
}

#[derive(Default)]
struct Shared {
    state: TypingState,
    /// `Some` exactly while a run is accepted and in flight. Doubles as the
    /// "is a run active" flag, so starting twice is impossible.
    cancel: Option<Arc<AtomicBool>>,
}

/// Managed state for the typing engine.
#[derive(Default)]
pub struct TypingHandle {
    shared: Mutex<Shared>,
}

impl TypingHandle {
    /// Claim the engine for a new run, or `None` if one is already in flight.
    fn begin(&self, total: u32, countdown: u32) -> Option<Arc<AtomicBool>> {
        let mut shared = crate::lock(&self.shared);
        if shared.cancel.is_some() {
            return None;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        shared.cancel = Some(Arc::clone(&cancel));
        shared.state = TypingState {
            phase: "countdown".to_string(),
            typed: 0,
            total,
            countdown,
        };
        Some(cancel)
    }

    /// Release the claim and go back to idle.
    fn release(&self) {
        let mut shared = crate::lock(&self.shared);
        shared.cancel = None;
        shared.state = TypingState::idle();
    }

    fn store(&self, state: &TypingState) {
        crate::lock(&self.shared).state = state.clone();
    }

    /// Ask the worker to stop. Returns whether a run was actually in flight.
    fn request_stop(&self) -> bool {
        let shared = crate::lock(&self.shared);
        match &shared.cancel {
            Some(cancel) => {
                cancel.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    pub fn status(&self) -> TypingState {
        crate::lock(&self.shared).state.clone()
    }
}

/// How a `\n` is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NewlineMode {
    Enter,
    ShiftEnter,
    Skip,
}

impl NewlineMode {
    fn parse(value: &str) -> Self {
        match value {
            "shiftEnter" => Self::ShiftEnter,
            "skip" => Self::Skip,
            // Settings::normalize already rejects anything else; "enter" is the
            // documented default for unknown values.
            _ => Self::Enter,
        }
    }
}

/// Collapse CRLF and lone CR into LF so newline handling has one shape.
pub fn normalize_text(text: &str) -> String {
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

/// Accept a new run, or explain why it was refused.
///
/// Returns as soon as the worker thread has proven it can talk to the system
/// keyboard, so an unusable environment (Wayland, missing macOS Accessibility
/// grant) surfaces as a command error rather than a silent no-op.
pub fn start(app: &AppHandle, text: &str, settings: Settings) -> Result<(), AppError> {
    let normalized = normalize_text(text);
    let char_count = normalized.chars().count();
    if char_count > MAX_CHARS {
        return Err(AppError::Invalid(format!(
            "that text is too long to type: {char_count} characters (limit is {MAX_CHARS})"
        )));
    }
    let total = u32::try_from(char_count).unwrap_or(u32::MAX);
    let countdown = settings.start_delay_secs;

    let cancel = {
        let state = app
            .try_state::<AppState>()
            .ok_or_else(|| AppError::Typing(STATE_MISSING.to_string()))?;
        state
            .typing
            .begin(total, countdown)
            .ok_or_else(|| AppError::Typing(ALREADY_TYPING.to_string()))?
    };

    let (init_tx, init_rx) = mpsc::channel::<Result<(), String>>();
    let (go_tx, go_rx) = mpsc::channel::<()>();

    let worker_app = app.clone();
    let worker_cancel = Arc::clone(&cancel);
    let spawned = thread::Builder::new()
        .name("ketikin-typing".to_string())
        .spawn(move || {
            let mut enigo = match Enigo::new(&EnigoSettings::default()) {
                Ok(enigo) => {
                    if init_tx.send(Ok(())).is_err() {
                        return;
                    }
                    enigo
                }
                Err(err) => {
                    let _ = init_tx.send(Err(describe_connection_error(&err)));
                    return;
                }
            };

            // Only proceed once the command thread has confirmed it observed
            // the successful init. If it gave up first, abandon the run
            // silently: the frontend already got an `Err` and must not also
            // receive a `typing://done` for a start that never happened.
            if go_rx.recv().is_err() {
                return;
            }

            let mut guard = RunGuard::new(worker_app.clone());
            guard.outcome = Some(run(
                &worker_app,
                &mut enigo,
                &normalized,
                &settings,
                &worker_cancel,
                total,
            ));
        });

    if let Err(err) = spawned {
        release(app);
        return Err(AppError::Typing(format!(
            "could not start the typing thread: {err}"
        )));
    }

    match init_rx.recv_timeout(INIT_TIMEOUT) {
        Ok(Ok(())) => {
            let _ = go_tx.send(());
            Ok(())
        }
        Ok(Err(message)) => {
            release(app);
            Err(AppError::Typing(message))
        }
        Err(_) => {
            // Worker is wedged or died before reporting. Drop `go_tx` (by
            // returning) so it aborts instead of typing into a run the
            // frontend believes never started.
            cancel.store(true, Ordering::Relaxed);
            release(app);
            Err(AppError::Typing(
                "timed out while connecting to the system keyboard".to_string(),
            ))
        }
    }
}

/// Ask any in-flight run to stop. Safe and cheap to call when idle.
///
/// This is deliberately callable straight from Rust (the stop hotkey uses it)
/// so it keeps working even if the WebView is busy or unresponsive.
pub fn stop(app: &AppHandle) -> Result<(), AppError> {
    if let Some(state) = app.try_state::<AppState>() {
        state.typing.request_stop();
    }
    Ok(())
}

fn release(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.typing.release();
    }
}

/// Guarantees the engine returns to idle, emits exactly one `typing://done`, and
/// leaves the tray unmarked, even if the worker panics partway through a run.
struct RunGuard {
    app: AppHandle,
    outcome: Option<Done>,
}

impl RunGuard {
    /// Marks the tray as running as the guard is created.
    ///
    /// Here rather than anywhere inside [`run`] so the mark cannot outlive the
    /// `drop` that clears it: there is no path that sets it without a guard
    /// already standing. It lands before the first `typing://state`, so the tray
    /// is marked for the countdown as well, which is the part of a run the user is
    /// most likely to be watching something else during.
    fn new(app: AppHandle) -> Self {
        crate::tray::set_running(&app, true);
        Self { app, outcome: None }
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        let done = self
            .outcome
            .take()
            .unwrap_or_else(|| Done::error("the typing engine stopped unexpectedly".to_string()));

        release(&self.app);
        let _ = self.app.emit(EVENT_STATE, TypingState::idle());
        let _ = self.app.emit(EVENT_DONE, done);
        // Last, and deliberately: `set_running` blocks until the main thread runs
        // it, and nothing above it should wait on that. Releasing the engine is
        // what lets the next run start, and the frontend needs its terminal event;
        // an unmarked tray is only cosmetic and can arrive a beat later.
        crate::tray::set_running(&self.app, false);
    }
}

fn run(
    app: &AppHandle,
    enigo: &mut Enigo,
    text: &str,
    settings: &Settings,
    cancel: &AtomicBool,
    total: u32,
) -> Done {
    for remaining in (1..=settings.start_delay_secs).rev() {
        if cancel.load(Ordering::Relaxed) {
            return Done::stopped();
        }
        publish(
            app,
            &TypingState {
                phase: "countdown".to_string(),
                typed: 0,
                total,
                countdown: remaining,
            },
        );
        if !sleep_cancellable(cancel, Duration::from_secs(1)) {
            return Done::stopped();
        }
    }

    if cancel.load(Ordering::Relaxed) {
        return Done::stopped();
    }

    let mut current = TypingState {
        phase: "typing".to_string(),
        typed: 0,
        total,
        countdown: 0,
    };
    publish(app, &current);

    let delay = Duration::from_millis(u64::from(settings.typing_delay_ms));
    let newline = NewlineMode::parse(&settings.newline_mode);
    let mut last_emit = Instant::now();
    let mut shift_held = false;

    for ch in text.chars() {
        if cancel.load(Ordering::Relaxed) {
            release_shift(enigo, &mut shift_held);
            return Done::stopped();
        }

        if let Err(err) = send_char(enigo, ch, newline, &mut shift_held) {
            release_shift(enigo, &mut shift_held);
            return Done::error(format!(
                "could not send a keystroke to the focused window: {err}"
            ));
        }

        current.typed = current.typed.saturating_add(1);
        store(app, &current);

        // Coalesce progress events: the mutex snapshot above is always fresh
        // for `typing_status`, but the IPC bridge only sees ~20 updates/second.
        if last_emit.elapsed() >= EMIT_INTERVAL {
            emit_state(app, &current);
            last_emit = Instant::now();
        }

        if !sleep_cancellable(cancel, delay) {
            release_shift(enigo, &mut shift_held);
            return Done::stopped();
        }
    }

    release_shift(enigo, &mut shift_held);
    publish(app, &current);
    Done::completed()
}

fn send_char(
    enigo: &mut Enigo,
    ch: char,
    newline: NewlineMode,
    shift_held: &mut bool,
) -> Result<(), InputError> {
    let mut buf = [0u8; 4];

    match ch {
        '\n' => match newline {
            NewlineMode::Skip => Ok(()),
            NewlineMode::Enter => enigo.key(Key::Return, Direction::Click),
            NewlineMode::ShiftEnter => {
                enigo.key(Key::Shift, Direction::Press)?;
                *shift_held = true;

                // Release Shift unconditionally, then report the first failure.
                // Returning early between press and release would leave the
                // modifier stuck down on the user's machine.
                let typed = enigo.key(Key::Return, Direction::Click);
                let released = enigo.key(Key::Shift, Direction::Release);
                if released.is_ok() {
                    *shift_held = false;
                }
                typed.and(released)
            }
        },
        '\t' => enigo.key(Key::Tab, Direction::Click),
        // Unicode text entry rather than a keycode, so accented and non-Latin
        // characters arrive correctly regardless of the active layout.
        _ => enigo.text(ch.encode_utf8(&mut buf)),
    }
}

fn release_shift(enigo: &mut Enigo, shift_held: &mut bool) {
    if !*shift_held {
        return;
    }
    if let Err(err) = enigo.key(Key::Shift, Direction::Release) {
        log::warn!("typing: could not release Shift while stopping: {err}");
    }
    *shift_held = false;
}

/// Sleep `total`, waking every [`CANCEL_SLICE`] to check for cancellation.
/// Returns `false` if the run was cancelled.
fn sleep_cancellable(cancel: &AtomicBool, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(CANCEL_SLICE));
    }
}

fn store(app: &AppHandle, state: &TypingState) {
    if let Some(app_state) = app.try_state::<AppState>() {
        app_state.typing.store(state);
    }
}

fn emit_state(app: &AppHandle, state: &TypingState) {
    let _ = app.emit(EVENT_STATE, state);
}

fn publish(app: &AppHandle, state: &TypingState) {
    store(app, state);
    emit_state(app, state);
}

/// Turn an Enigo connection failure into something the user can act on.
fn describe_connection_error(err: &NewConError) -> String {
    let base = format!("Ketikin could not connect to the system keyboard ({err})");

    if cfg!(target_os = "macos") {
        format!(
            "{base}. Grant Ketikin permission under System Settings > Privacy & Security > \
             Accessibility, then restart the app."
        )
    } else if cfg!(target_os = "linux") {
        format!(
            "{base}. Ketikin needs an X11 display: run it under X11 or XWayland (a native \
             Wayland session blocks synthetic keystrokes), and make sure DISPLAY is set."
        )
    } else {
        format!("{base}.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_crlf_and_lone_cr() {
        assert_eq!(normalize_text("a\r\nb"), "a\nb");
        assert_eq!(normalize_text("a\rb"), "a\nb");
        assert_eq!(normalize_text("a\r\n\r\nb"), "a\n\nb");
        assert_eq!(normalize_text("a\n\rb"), "a\n\nb");
        assert_eq!(normalize_text("a\r\rb"), "a\n\nb");
    }

    #[test]
    fn normalize_leaves_clean_text_alone() {
        assert_eq!(normalize_text("a\nb\tc"), "a\nb\tc");
        assert_eq!(normalize_text(""), "");
        assert_eq!(normalize_text("halo dunia"), "halo dunia");
    }

    #[test]
    fn normalize_preserves_non_ascii() {
        assert_eq!(normalize_text("café ☕\r\nselesai"), "café ☕\nselesai");
    }

    #[test]
    fn newline_mode_parses_known_values_and_defaults_the_rest() {
        assert_eq!(NewlineMode::parse("enter"), NewlineMode::Enter);
        assert_eq!(NewlineMode::parse("shiftEnter"), NewlineMode::ShiftEnter);
        assert_eq!(NewlineMode::parse("skip"), NewlineMode::Skip);
        assert_eq!(NewlineMode::parse("shift_enter"), NewlineMode::Enter);
        assert_eq!(NewlineMode::parse(""), NewlineMode::Enter);
    }

    #[test]
    fn typing_state_defaults_to_idle() {
        let state = TypingState::default();

        assert_eq!(state.phase, "idle");
        assert_eq!(state.typed, 0);
        assert_eq!(state.total, 0);
        assert_eq!(state.countdown, 0);
    }

    #[test]
    fn typing_state_serializes_as_camel_case() {
        let json = serde_json::to_string(&TypingState::idle()).expect("serialize");
        assert_eq!(
            json,
            r#"{"phase":"idle","typed":0,"total":0,"countdown":0}"#
        );
    }

    #[test]
    fn handle_admits_one_run_at_a_time() {
        let handle = TypingHandle::default();

        let cancel = handle.begin(10, 3).expect("first run should be accepted");
        assert_eq!(handle.status().phase, "countdown");
        assert_eq!(handle.status().total, 10);
        assert_eq!(handle.status().countdown, 3);

        assert!(handle.begin(5, 0).is_none(), "second run must be refused");

        assert!(handle.request_stop());
        assert!(cancel.load(Ordering::Relaxed));

        handle.release();
        assert_eq!(handle.status(), TypingState::idle());
        assert!(!handle.request_stop(), "stop while idle reports no run");
        assert!(
            handle.begin(1, 0).is_some(),
            "engine is reusable after release"
        );
    }

    #[test]
    fn sleep_cancellable_returns_early_when_cancelled() {
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            flag.store(true, Ordering::Relaxed);
        });

        let started = Instant::now();
        assert!(!sleep_cancellable(&cancel, Duration::from_secs(10)));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn sleep_cancellable_runs_to_completion_when_not_cancelled() {
        let cancel = AtomicBool::new(false);
        let started = Instant::now();

        assert!(sleep_cancellable(&cancel, Duration::from_millis(40)));
        assert!(started.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn connection_error_message_is_actionable() {
        let message = describe_connection_error(&NewConError::NoPermission);

        assert!(message.starts_with("Ketikin could not connect to the system keyboard"));
        if cfg!(target_os = "macos") {
            assert!(message.contains("Accessibility"));
        } else if cfg!(target_os = "linux") {
            assert!(message.contains("X11"));
        }
    }
}
