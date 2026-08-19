//! Data-directory resolution and atomic JSON persistence.
//!
//! This module is the reason the backend was rewritten. The predecessor wrote
//! its JSON only under the user's home directory and silently lost every save
//! on locked-down Windows Server profiles where that path is not writable.
//!
//! Here the directory is resolved exactly once, *before* `tauri::Builder`
//! exists, by probing a list of candidates and keeping the first one that
//! survives a real create → write → fsync → delete round trip. Resolving this
//! early matters: the log plugin's file target is pointed at the result, and
//! anything that runs inside `tauri::Builder` can abort startup before our own
//! setup hook ever executes.
//!
//! If none of the candidates work the app keeps running with purely in-memory
//! state instead of failing to launch.
//!
//! Resolving this early has one cost: `log::set_logger` has not run yet, so
//! every `log::` macro in here is a silent no-op. The diagnostics are therefore
//! buffered on the [`Storage`] value and replayed by
//! [`Storage::replay_diagnostics`] from `setup`, once the logger is live.
//! Without that, "which candidate was rejected and why" — the single most
//! useful thing in a locked-down-Windows bug report — never reaches the file.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Bundle identifier, used as the directory name for every candidate.
pub const APP_DIR_NAME: &str = "com.rendyuwu.ketikin";

/// Emitted once at startup when storage is degraded or carries notices.
pub const EVENT_WARNING: &str = "storage://warning";

/// Cap per log file, before rotation.
///
/// Set explicitly rather than inherited: the plugin defaults to 40 KB with
/// `KeepOne`, which a single typing session rolls straight past — discarding
/// the startup storage-resolution and hotkey-registration diagnostics, which
/// are log-only and are exactly what a bug report needs. 1 MB across three
/// files stays small enough to attach to an issue.
///
/// It lives here rather than next to the plugin builder because the log probe
/// has to make the same rotation decision the plugin would, and two copies of
/// this number would eventually disagree.
pub const LOG_MAX_FILE_SIZE: u64 = 1024 * 1024;

/// How many *rotated* log files to keep, excluding the active one.
///
/// Must match the `RotationStrategy::KeepSome` argument in `lib.rs`, for the
/// same reason as [`LOG_MAX_FILE_SIZE`].
pub const LOG_KEEP_FILES: usize = 2;

/// Stem of the log file, before the per-user suffix and the `.log` extension.
///
/// Matches `productName` in `tauri.conf.json`, which is what the plugin falls
/// back to when no file name is given. So the no-user case produces exactly the
/// file every existing install already has, and `README.md`'s worked example
/// only gains a suffix rather than changing shape.
const LOG_FILE_BASE: &str = "Ketikin";

/// Longest user-name fragment appended to [`LOG_FILE_BASE`].
const MAX_LOG_USER_CHARS: usize = 32;

/// Per-candidate ceiling on the writability probe.
///
/// A candidate pointing at a down SMB share blocks in the kernel for the SMB
/// client timeout — roughly a minute, and the main window already exists by
/// then, so the user would sit in front of a hung blank window.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// Ceiling on *all* probing combined.
///
/// Several candidates can point at the same dead share, so a per-candidate
/// timeout alone still multiplies. Once this is spent the remaining candidates
/// get whatever is left, floored at [`PROBE_MIN_TIMEOUT`]. The tail candidates
/// are local disk and answer in milliseconds, so this costs nothing in the
/// normal case and caps the pathological one.
const PROBE_BUDGET: Duration = Duration::from_secs(8);

/// Never give a candidate less than this, even with the budget exhausted.
const PROBE_MIN_TIMEOUT: Duration = Duration::from_millis(500);

/// Retry schedule for the final rename. See [`rename_with_retry`].
const RENAME_BACKOFF: [Duration; 3] = [
    Duration::from_millis(50),
    Duration::from_millis(150),
    Duration::from_millis(400),
];

/// Distinguishes concurrent temp files. Paired with the pid, this is unique
/// across both threads and separate Ketikin processes.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Where Ketikin ended up storing its data, and whether that actually worked.
///
/// `source` is one of `appData`, `appDataEnv`, `localAppDataEnv`, `nextToExe`,
/// `temp`, or `memory`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub path: String,
    pub source: String,
    pub writable: bool,
    pub error: Option<String>,
    /// Plain-English things the user should be told: data that had to be reset,
    /// a location shared with other users, or file logging being unavailable.
    ///
    /// Not every notice raises the startup banner — see `degraded`. All of them
    /// belong in Settings > Storage.
    pub notices: Vec<String>,
    /// Whether the startup banner is warranted.
    ///
    /// The backend's verdict, exposed so the frontend gates on it directly
    /// rather than reconstructing the rule from the other fields. Notably it is
    /// *not* "notices is non-empty": a portable `nextToExe` install always
    /// carries notices but is a working configuration, while a corrupt-file
    /// reset on a perfectly healthy `appData` path is real data loss and must
    /// raise it.
    pub degraded: bool,
}

/// A resolved, writability-checked data directory plus atomic JSON helpers.
#[derive(Debug)]
pub struct Storage {
    /// `None` when we are running in memory-only mode.
    dir: Option<PathBuf>,
    /// `<dir>/logs`, but only once proven writable in its own right. `None`
    /// means file logging is off for this run.
    log_dir: Option<PathBuf>,
    /// File-name stem handed to the log plugin, and the one the probe opened.
    /// Derived once so the two can never disagree.
    log_file: String,
    /// Diagnostics recorded before a logger existed, in order.
    ///
    /// Drained exactly once by [`Storage::take_diagnostics`], which also stops
    /// further buffering — everything after that point has a live logger and
    /// goes straight to it.
    diagnostics: Mutex<Vec<(log::Level, String)>>,
    /// Whether [`Storage::record`] still buffers instead of logging.
    buffering: AtomicBool,
    info: Mutex<StorageInfo>,
    /// Whether anything recorded so far justifies the startup banner.
    ///
    /// Deliberately narrower than "has notices". A `nextToExe` install is a
    /// working portable deployment, and a banner on every launch of a working
    /// configuration is how users learn to dismiss warnings unread. Its notices
    /// still surface in Settings, where someone investigating will find them.
    alarming: AtomicBool,
}

impl Storage {
    /// Build the candidate list, in priority order.
    ///
    /// Deliberately takes no `AppHandle`: storage has to be resolved before
    /// `tauri::Builder` so the log plugin can be pointed at the result.
    /// Candidate 1 is `dirs::data_dir().join(APP_DIR_NAME)`, which is
    /// byte-identical to what Tauri's `app_data_dir()` returns — Tauri computes
    /// it the same way, from the same crate.
    ///
    /// Candidates 2 and 3 are deliberately *not* `#[cfg]`-gated to Windows: the
    /// environment variables simply do not exist elsewhere, and gating them
    /// would mean the fallback chain is never exercised on developer machines.
    pub fn candidates() -> Vec<(&'static str, PathBuf)> {
        let mut candidates: Vec<(&'static str, PathBuf)> = Vec::with_capacity(5);

        if let Some(dir) = dirs::data_dir() {
            candidates.push(("appData", dir.join(APP_DIR_NAME)));
        }
        if let Some(dir) = std::env::var_os("APPDATA") {
            candidates.push(("appDataEnv", PathBuf::from(dir).join(APP_DIR_NAME)));
        }
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(("localAppDataEnv", PathBuf::from(dir).join(APP_DIR_NAME)));
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                candidates.push(("nextToExe", parent.join("data")));
            }
        }
        candidates.push(("temp", std::env::temp_dir().join(APP_DIR_NAME)));

        candidates
    }

    /// Take the first candidate that is genuinely writable.
    ///
    /// Every rejection is buffered so a support request can be diagnosed from
    /// the log file alone once [`Storage::replay_diagnostics`] has run.
    pub fn resolve(candidates: Vec<(&'static str, PathBuf)>) -> Self {
        let mut diagnostics: Vec<(log::Level, String)> = Vec::new();
        let mut failures: Vec<String> = Vec::new();
        let deadline = Instant::now() + PROBE_BUDGET;
        let log_file = log_file_stem();

        for (source, dir) in candidates {
            let probe_dir = dir.clone();
            let probed = probe_bounded(remaining_timeout(deadline), move || {
                probe_writable(&probe_dir)
            });

            match probed {
                Ok(()) => {
                    diagnostics.push((
                        log::Level::Info,
                        format!("storage: using {} (source: {source})", dir.display()),
                    ));

                    // Probe `<dir>/logs` separately rather than inferring it.
                    // On Windows, creating a *file* and creating a
                    // *subdirectory* are separately grantable rights
                    // (FILE_ADD_FILE vs FILE_ADD_SUBDIRECTORY), and hardened
                    // session-host ACLs really do grant one without the other —
                    // so passing the data probe does not imply the log folder
                    // can be created. Failing this must not disqualify an
                    // otherwise good data directory.
                    let logs = dir.join("logs");
                    let probe_logs = logs.clone();
                    let probe_stem = log_file.clone();
                    let probed_logs = probe_bounded(remaining_timeout(deadline), move || {
                        probe_log_dir(&probe_logs, &probe_stem)
                    });
                    let log_dir = match probed_logs {
                        Ok(()) => Some(logs),
                        Err(err) => {
                            diagnostics.push((
                                log::Level::Warn,
                                format!(
                                    "storage: log directory {} unusable: {err}",
                                    logs.display()
                                ),
                            ));
                            None
                        }
                    };
                    let log_failure = log_dir.is_none();

                    let storage = Self {
                        info: Mutex::new(StorageInfo {
                            path: dir.display().to_string(),
                            source: source.to_string(),
                            writable: true,
                            error: None,
                            notices: Vec::new(),
                            // Recomputed by `info()`; never read from here.
                            degraded: false,
                        }),
                        dir: Some(dir),
                        log_dir,
                        log_file,
                        diagnostics: Mutex::new(diagnostics),
                        buffering: AtomicBool::new(true),
                        alarming: AtomicBool::new(false),
                    };
                    storage.add_location_notices(source);

                    if log_failure {
                        // Not alarming: data saves fine and the user is not
                        // harmed right now. It only costs us diagnosability, so
                        // it belongs in Settings rather than a startup banner.
                        storage.push_notice(
                            "Ketikin could not create its log folder, so no log file will be \
                             written this run. Your settings and templates are unaffected."
                                .to_string(),
                            false,
                        );
                    }

                    return storage;
                }
                Err(err) => {
                    diagnostics.push((
                        log::Level::Warn,
                        format!(
                            "storage: {source} candidate {} rejected: {err}",
                            dir.display()
                        ),
                    ));
                    failures.push(format!("{source} ({}): {err}", dir.display()));
                }
            }
        }

        let error = if failures.is_empty() {
            "no data directory candidates were available".to_string()
        } else {
            format!("no writable data directory found — {}", failures.join("; "))
        };
        diagnostics.push((
            log::Level::Error,
            format!("storage: {error}; falling back to in-memory state"),
        ));

        let storage = Self {
            dir: None,
            log_dir: None,
            log_file,
            diagnostics: Mutex::new(diagnostics),
            buffering: AtomicBool::new(true),
            alarming: AtomicBool::new(true),
            info: Mutex::new(StorageInfo {
                path: String::new(),
                source: "memory".to_string(),
                writable: false,
                error: Some(error),
                notices: Vec::new(),
                degraded: true,
            }),
        };
        storage.push_notice(
            "Ketikin could not find anywhere to save data, so settings and templates will be \
             lost when you close it."
                .to_string(),
            true,
        );

        storage
    }

    /// Replay everything recorded before `log::set_logger` had been called.
    ///
    /// Call this from `setup`, before any other logging, so the log file reads
    /// chronologically. Draining also switches [`Storage::record`] to logging
    /// directly, so nothing accumulates for the rest of the run.
    pub fn replay_diagnostics(&self) {
        for (level, message) in self.take_diagnostics() {
            log::log!(level, "{message}");
        }
    }

    /// Drain the buffer and stop buffering. See [`Storage::replay_diagnostics`].
    pub fn take_diagnostics(&self) -> Vec<(log::Level, String)> {
        self.buffering.store(false, Ordering::Relaxed);
        std::mem::take(&mut *crate::lock(&self.diagnostics))
    }

    /// Buffer a diagnostic while there is no logger, and log it once there is.
    ///
    /// Deliberately never does both: a message is emitted exactly once, either
    /// by [`Storage::replay_diagnostics`] or here.
    fn record(&self, level: log::Level, message: String) {
        if self.buffering.load(Ordering::Relaxed) {
            crate::lock(&self.diagnostics).push((level, message));
        } else {
            log::log!(level, "{message}");
        }
    }

    /// Warn about locations that are per-machine rather than per-user.
    ///
    /// On a session host these are shared by every user who runs Ketikin: a
    /// read exposes whatever the previous user stored, and a write lets anyone
    /// plant template content that the next operator types into a root console.
    ///
    /// Only `temp` raises the banner. `nextToExe` is a legitimate portable
    /// deployment — warning on every launch of a working configuration teaches
    /// users to dismiss warnings, so its notices stay in Settings only.
    fn add_location_notices(&self, source: &str) {
        match source {
            "nextToExe" => {
                self.push_notice(
                    "Ketikin is saving next to its own program files. That location may be \
                     shared with other users of this machine, so avoid storing passwords or \
                     licence keys in templates."
                        .to_string(),
                    false,
                );
                self.push_notice(
                    "This location also depends on how Ketikin was launched — running as \
                     administrator and running normally can resolve to different folders, so \
                     settings saved one way may not appear the other."
                        .to_string(),
                    false,
                );
            }
            // The temp warning is genuinely different per platform, so it is
            // written per platform rather than hedged into one sentence that is
            // wrong everywhere. On Windows `std::env::temp_dir()` goes through
            // `GetTempPath()`, which yields `%LOCALAPPDATA%\Temp` — per-user,
            // and *inside* the `localAppDataEnv` candidate, so it is not the
            // independent backstop the chain's shape suggests. On Linux and
            // macOS `/tmp` really is a separate, machine-wide location.
            "temp" if cfg!(windows) => {
                self.push_notice(
                    "Ketikin is saving to the temporary folder. On Windows that folder lives \
                     inside your own user profile, so it is not shared with other users — but \
                     it is not an independent fallback from that profile either, and Windows \
                     can clear it, so your settings and templates may not survive a reboot."
                        .to_string(),
                    true,
                );
            }
            "temp" => {
                self.push_notice(
                    "Ketikin is saving to the temporary folder. That location may be shared \
                     with other users of this machine and can be cleared automatically, so \
                     avoid storing passwords or licence keys in templates."
                        .to_string(),
                    true,
                );
            }
            _ => {}
        }
    }

    /// Snapshot for the `storage_info` command and the `storage://warning` event.
    ///
    /// `degraded` is derived here rather than stored, so it cannot go stale
    /// behind a later notice. This is the single place the banner policy lives:
    /// the frontend consumes the verdict instead of reconstructing it from
    /// `source`/`writable`/`notices`, which would put one rule under two owners
    /// with nothing to catch the drift.
    pub fn info(&self) -> StorageInfo {
        let alarming = self.alarming.load(Ordering::Relaxed);
        let mut info = crate::lock(&self.info).clone();

        info.degraded =
            !info.writable || matches!(info.source.as_str(), "memory" | "temp") || alarming;

        info
    }

    /// The resolved directory, if there is one.
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// A directory proven writable for log files, or `None`.
    ///
    /// Decided once during [`Storage::resolve`], not on demand. The log
    /// plugin's folder target does its own `create_dir_all` and file open and
    /// propagates any failure out of its setup closure — which runs early
    /// enough to abort the whole app — so it may only ever be handed a
    /// directory that has already passed a real write.
    pub fn log_dir(&self) -> Option<&Path> {
        self.log_dir.as_deref()
    }

    /// File-name stem for the log plugin's folder target, without `.log`.
    ///
    /// Per-user, because [`restrict_to_owner`] is a no-op on Windows: in a
    /// shared location — a portable install beside the executable on a session
    /// host — the directory stays writable by everyone while the *file* one
    /// user created inherits an ACE that gives only its creator write access.
    /// The plugin opens that file with `create(true).append(true)` from inside
    /// its setup closure and `?`-propagates the failure out of
    /// `tauri::Builder::build()`, so a second user launching would get no
    /// window at all. A name per user means the collision cannot arise.
    ///
    /// This is the same value [`probe_log_dir`] opened. Deriving it once is the
    /// point: a probe of a different file proves nothing about the one the
    /// plugin will use.
    pub fn log_file(&self) -> &str {
        &self.log_file
    }

    /// True when the startup banner is warranted.
    ///
    /// Delegates to [`Storage::info`] rather than recomputing, so the boolean
    /// the backend acts on and the `degraded` field the frontend renders are
    /// the same value by construction — not two derivations that agree today.
    pub fn is_degraded(&self) -> bool {
        self.info().degraded
    }

    /// Record something the user should see. `alarming` decides whether it also
    /// raises the startup banner, or only appears in Settings > Storage.
    fn push_notice(&self, notice: String, alarming: bool) {
        self.record(log::Level::Warn, format!("storage: {notice}"));
        crate::lock(&self.info).notices.push(notice);

        if alarming {
            self.alarming.store(true, Ordering::Relaxed);
        }
    }

    /// Load `<name>.json`, falling back to `T::default()` rather than failing.
    ///
    /// A missing file is normal (first run). A *corrupt* file is moved aside so
    /// the user can still recover it by hand, defaults are returned, and a
    /// notice is recorded — silently resetting someone's whole template library
    /// with nothing but a log line is not acceptable.
    ///
    /// `reset_to` describes the fallback in the notice, e.g. `"empty"`.
    pub fn read<T: DeserializeOwned + Default>(&self, name: &str, reset_to: &str) -> T {
        let Some(dir) = &self.dir else {
            return T::default();
        };
        let path = dir.join(format!("{name}.json"));

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return T::default(),
            Err(err) => {
                self.push_notice(
                    format!(
                        "{name}.json could not be opened ({err}), so Ketikin started from \
                         {reset_to}. Your existing file was left untouched."
                    ),
                    true,
                );
                return T::default();
            }
        };

        match serde_json::from_str::<T>(&raw) {
            Ok(value) => value,
            Err(err) => {
                let backup = unused_backup_path(dir, name);
                let backup_label = backup
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("{name}.json.bak"));

                match fs::rename(&path, &backup) {
                    Ok(()) => self.push_notice(
                        format!(
                            "{name}.json could not be read and was reset to {reset_to}. The \
                             unreadable file was kept as {backup_label}."
                        ),
                        true,
                    ),
                    Err(rename_err) => self.push_notice(
                        format!(
                            "{name}.json could not be read ({err}) and could not be moved \
                             aside ({rename_err}), so Ketikin started from {reset_to}."
                        ),
                        true,
                    ),
                }
                T::default()
            }
        }
    }

    /// Persist `value` to `<name>.json` atomically.
    ///
    /// In memory-only mode this is a logged no-op: the in-process state stays
    /// authoritative and the frontend already knows storage is degraded from
    /// the `storage://warning` event.
    pub fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<(), AppError> {
        let Some(dir) = &self.dir else {
            log::warn!("storage: in-memory mode, {name}.json was not persisted");
            return Ok(());
        };

        let bytes = serde_json::to_vec_pretty(value)?;
        write_atomic(dir, name, &bytes)
            .map_err(|err| AppError::storage(format!("could not save {name}.json"), err))
    }
}

/// How long the next probe may take, given the shared budget.
///
/// Clamped so a spent budget still allows a short attempt: the later candidates
/// are local disk and answer in milliseconds, so cutting them off entirely
/// would fail a perfectly good directory to save no time.
fn remaining_timeout(deadline: Instant) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .clamp(PROBE_MIN_TIMEOUT, PROBE_TIMEOUT)
}

/// Run a filesystem probe with a deadline.
///
/// The probe is uninterruptible once it is blocked in a filesystem call, so on
/// timeout the worker thread is simply abandoned. It will finish on its own
/// eventually; what matters is that startup does not wait for it.
fn probe_bounded(
    timeout: Duration,
    probe: impl FnOnce() -> std::io::Result<()> + Send + 'static,
) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();

    thread::Builder::new()
        .name("ketikin-storage-probe".to_string())
        .spawn(move || {
            let _ = tx.send(probe().map_err(|err| err.to_string()));
        })
        .map_err(|err| format!("could not start the probe thread: {err}"))?;

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "timed out after {}ms — the path may be an unavailable network location",
            timeout.as_millis()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("the writability probe stopped unexpectedly".to_string())
        }
    }
}

/// Prove a directory is writable by actually writing to it.
///
/// Metadata permission bits are not consulted on purpose: on Windows they lie
/// about the effective ACL, which is exactly the failure mode this app hit in
/// production. Only a completed create + write + fsync + delete counts.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    restrict_to_owner(dir);

    let probe = dir.join(format!(
        ".ketikin-write-probe-{}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut file = fs::File::create(&probe)?;
        file.write_all(b"ketikin")?;
        file.sync_all()?;
    }
    fs::remove_file(&probe)?;

    Ok(())
}

/// Qualify `<dir>/logs` by performing the operations the log plugin performs.
///
/// A uniquely-named create-then-delete proves nothing here. `TargetKind::Folder`
/// does three things that probe never touches, each `?`-propagated out of the
/// plugin's setup closure and therefore out of `tauri::Builder::build()` —
/// before our own `setup` hook exists to react, and under
/// `windows_subsystem = "windows"` with no stderr and no window, so the process
/// simply exits 1 in silence:
///
/// 1. `OpenOptions::new().create(true).append(true)` on a file that may already
///    exist and may deny us — the ACL case [`Storage::log_file`] describes;
/// 2. a rotation `fs::rename` once the file passes the size cap, with no retry,
///    which Windows fails transiently under antivirus and indexer locks;
/// 3. `fs::remove_file` of an old dated log.
///
/// So this does all three: it opens the real file and leaves it in place
/// (deleting is not something the plugin does), and it performs the rotation and
/// the pruning itself, using [`rename_with_retry`] where the plugin would use a
/// bare rename. The plugin then finds a small file and a pruned directory and
/// has no work left that can fail. Anything that still fails here disqualifies
/// the log directory, which costs a log file — infinitely cheaper than costing
/// the window.
fn probe_log_dir(dir: &Path, stem: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    restrict_to_owner(dir);

    let path = dir.join(format!("{stem}.log"));

    if append_open(&path)? >= LOG_MAX_FILE_SIZE {
        rotate_log(dir, stem, &path)?;
        // Rotating renamed the file away, so open it again — both to leave the
        // file the plugin expects sitting there and because that second open is
        // one the plugin will also perform.
        append_open(&path)?;
    }
    prune_rotated_logs(dir, stem, LOG_KEEP_FILES)
}

/// Open a log file exactly as the plugin does, and report its size.
///
/// The handle is dropped immediately: the point is to prove the call succeeds
/// and to leave the file in place, not to hold it.
fn append_open(path: &Path) -> std::io::Result<u64> {
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;

    file.metadata().map(|meta| meta.len())
}

/// Move an oversized log aside, under the name the plugin would have chosen.
///
/// Matching the plugin's `<stem>_<UTC timestamp>.log` shape is load-bearing:
/// its own pruning only recognises files of that form, so a name of our own
/// invention would accumulate forever.
fn rotate_log(dir: &Path, stem: &str, path: &Path) -> std::io::Result<()> {
    let stamp = chrono::Utc::now().format("%Y-%m-%d_%H-%M-%S");
    let mut target = dir.join(format!("{stem}_{stamp}.log"));

    // Two launches inside one second would otherwise silently overwrite the
    // older archive. The suffix keeps the prefix and the `.log` tail intact, so
    // the plugin still sees it as one of ours.
    if target.exists() {
        target = dir.join(format!(
            "{stem}_{stamp}-{}-{}.log",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
    }

    rename_with_retry(path, &target)
}

/// Delete rotated logs beyond `keep`, oldest first.
///
/// Same selection and ordering the plugin uses — the timestamp format sorts
/// lexicographically, so a plain name sort is a chronological one.
fn prune_rotated_logs(dir: &Path, stem: &str, keep: usize) -> std::io::Result<()> {
    let prefix = format!("{stem}_");

    let mut rotated: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            (name.starts_with(&prefix) && name.ends_with(".log")).then(|| entry.path())
        })
        .collect();

    if rotated.len() <= keep {
        return Ok(());
    }
    rotated.sort();

    for path in rotated.iter().take(rotated.len() - keep) {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Per-user log file stem for this process. See [`Storage::log_file`].
fn log_file_stem() -> String {
    // `cfg!` rather than `#[cfg]` so both branches keep compiling everywhere.
    let variable = if cfg!(windows) { "USERNAME" } else { "USER" };

    log_file_stem_from(std::env::var(variable).ok().as_deref())
}

/// Body of [`log_file_stem`], with the environment injected so it is testable.
fn log_file_stem_from(user: Option<&str>) -> String {
    match user.and_then(sanitize_user) {
        Some(user) => format!("{LOG_FILE_BASE}-{user}"),
        // Unset or entirely unusable. Any fixed name is equally shared, so
        // there is nothing better to do than fall back to the bare base — which
        // is also what every pre-existing install already has on disk.
        None => LOG_FILE_BASE.to_string(),
    }
}

/// Reduce a user name to something safe in a file name.
///
/// Anything outside `[A-Za-z0-9_-]` becomes `-` rather than being dropped, so
/// two distinct names cannot collapse onto each other, and the result is
/// trimmed of leading and trailing separators. `None` when nothing usable is
/// left.
fn sanitize_user(raw: &str) -> Option<String> {
    let mapped: String = raw
        .chars()
        .take(MAX_LOG_USER_CHARS)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let trimmed = mapped.trim_matches('-');
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Keep the data directory out of other users' reach where the OS makes that a
/// one-liner.
///
/// This is a genuine `#[cfg]` rather than a `cfg!()` branch because
/// `PermissionsExt` only exists on unix. Note it does nothing on Windows, which
/// is where the shared-location risk actually bites — the `nextToExe` and
/// `temp` notices carry that warning instead.
#[cfg(unix)]
fn restrict_to_owner(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;

    if let Err(err) = fs::set_permissions(dir, fs::Permissions::from_mode(0o700)) {
        log::warn!(
            "storage: could not restrict {} to the current user: {err}",
            dir.display()
        );
    }
}

#[cfg(not(unix))]
fn restrict_to_owner(_dir: &Path) {}

/// Pick a backup name that is not already taken.
///
/// Overwriting an existing `.bak` would mean a second corruption destroys the
/// only surviving copy of the user's data.
fn unused_backup_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(format!("{name}.json.bak"));
    if !first.exists() {
        return first;
    }

    for n in 2..=20 {
        let candidate = dir.join(format!("{name}.json.bak{n}"));
        if !candidate.exists() {
            return candidate;
        }
    }

    dir.join(format!(
        "{name}.json.bak-{}-{}",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Write via a sibling temp file and rename over the destination.
///
/// The temp file must live in the *same* directory so the rename stays on one
/// filesystem and is therefore atomic. A crash mid-write leaves the previous
/// good file untouched.
///
/// The temp name carries the pid and a counter as defence in depth. Ketikin now
/// holds a single-instance lock (`tauri-plugin-single-instance`, registered
/// first in `run()`), so a second copy should never reach this function at all —
/// but if one ever did, two running copies saving the same file would otherwise
/// truncate each other's temp file and rename interleaved bytes into place as
/// authoritative JSON. The counter is load-bearing regardless: one process can
/// have several saves in flight.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!(
        "{name}.json.{}-{}.tmp",
        std::process::id(),
        TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let dest = dir.join(format!("{name}.json"));

    let written = (|| -> std::io::Result<()> {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();

    let result = written.and_then(|()| rename_with_retry(&tmp, &dest));
    if result.is_err() {
        // Never leave a stale temp file behind to accumulate or confuse.
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Rename, retrying briefly on failure.
///
/// On Windows `fs::rename` is `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`,
/// which fails outright while the destination is held open — an AV scanner, the
/// Search indexer, and backup agents all do this for a few hundred
/// milliseconds at a time. Without a retry those transient locks surface to the
/// user as "could not save settings.json".
fn rename_with_retry(from: &Path, to: &Path) -> std::io::Result<()> {
    let mut last_error = match fs::rename(from, to) {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };

    for delay in RENAME_BACKOFF {
        thread::sleep(delay);
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(err) => last_error = err,
        }
    }

    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
    struct Sample {
        value: u32,
        label: String,
    }

    fn storage_in(dir: &Path) -> Storage {
        Storage::resolve(vec![("appData", dir.to_path_buf())])
    }

    #[test]
    fn resolves_first_writable_candidate() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let unwritable = tmp.path().join("nested").join("file-not-dir");
        fs::create_dir_all(unwritable.parent().expect("parent")).expect("mkdir");
        fs::write(&unwritable, b"x").expect("write");

        // The first candidate is a *file*, so `create_dir_all` on it must fail
        // and resolution has to fall through to the second candidate.
        let good = tmp.path().join("good");
        let storage = Storage::resolve(vec![
            ("appData", unwritable),
            ("localAppDataEnv", good.clone()),
        ]);

        assert_eq!(storage.info().source, "localAppDataEnv");
        assert!(storage.info().writable);
        assert!(!storage.is_degraded());
        assert_eq!(storage.dir(), Some(good.as_path()));
    }

    #[test]
    fn falls_back_to_memory_when_nothing_is_writable() {
        let storage = Storage::resolve(Vec::new());
        let info = storage.info();

        assert_eq!(info.source, "memory");
        assert!(!info.writable);
        assert!(info.error.is_some());
        assert!(!info.notices.is_empty(), "memory mode must explain itself");
        assert!(storage.is_degraded());

        // Memory mode must still behave like storage: reads give defaults and
        // writes succeed (as no-ops) so no command path can fail because of it.
        assert_eq!(
            storage.read::<Sample>("settings", "defaults"),
            Sample::default()
        );
        assert!(storage.write("settings", &Sample::default()).is_ok());
        assert_eq!(storage.log_dir(), None);
    }

    #[test]
    fn a_portable_install_warns_that_the_folder_is_shared() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("nextToExe", tmp.path().join("portable"))]);
        let info = storage.info();

        assert!(info.writable, "a portable install is still usable");
        assert!(
            info.notices
                .iter()
                .any(|n| n.contains("shared with other users")),
            "got {:?}",
            info.notices
        );
    }

    #[test]
    fn the_temp_notice_is_accurate_for_the_platform_it_runs_on() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("temp", tmp.path().join("temp"))]);
        let info = storage.info();

        assert!(info.writable);
        let notice = info
            .notices
            .iter()
            .find(|n| n.contains("temporary folder"))
            .expect("temp must explain itself")
            .clone();

        if cfg!(windows) {
            // `GetTempPath()` yields `%LOCALAPPDATA%\Temp`, which is per-user
            // and *inside* the localAppDataEnv candidate. Claiming it is shared
            // is wrong, and claiming it is a fallback from that candidate is
            // worse — an ACL that rejected the profile rejects this too.
            assert!(
                !notice.contains("shared with other users"),
                "temp is per-user on Windows: {notice}"
            );
            assert!(notice.contains("inside your own user profile"), "{notice}");
            assert!(notice.contains("not an independent fallback"), "{notice}");
            assert!(notice.contains("reboot"), "{notice}");
        } else {
            // `/tmp` genuinely is machine-wide, and genuinely is an independent
            // backstop.
            assert!(notice.contains("shared with other users"), "{notice}");
        }
    }

    #[test]
    fn temp_raises_the_banner_but_next_to_exe_does_not() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // A portable install next to the executable is a working configuration.
        // Warning on every launch of a working setup is how users learn to
        // dismiss warnings unread, so its notices stay in Settings only.
        let portable = Storage::resolve(vec![("nextToExe", tmp.path().join("portable"))]);
        assert!(!portable.is_degraded(), "portable installs must not nag");
        assert!(
            !portable.info().notices.is_empty(),
            "but they must still explain themselves in Settings"
        );

        // Temp is always a fallback failure and can be cleared underneath us.
        let temp = Storage::resolve(vec![("temp", tmp.path().join("temp"))]);
        assert!(temp.is_degraded(), "temp must raise the banner");
    }

    #[test]
    fn a_corrupt_file_raises_the_banner_even_on_a_healthy_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());
        assert!(!storage.is_degraded(), "a healthy appData dir starts quiet");

        fs::write(tmp.path().join("sample.json"), b"not json").expect("write");
        storage.read::<Sample>("sample", "empty");

        // Data was silently lost. That always warrants the banner, however
        // healthy the location itself is.
        assert!(storage.is_degraded());
    }

    #[test]
    fn a_usable_data_dir_survives_an_unusable_log_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("data");
        fs::create_dir_all(&dir).expect("mkdir");

        // Occupy `logs` with a file so `create_dir_all` on it must fail. This
        // stands in for the Windows case where FILE_ADD_FILE is granted but
        // FILE_ADD_SUBDIRECTORY is not.
        fs::write(dir.join("logs"), b"in the way").expect("write");

        let storage = Storage::resolve(vec![("appData", dir.clone())]);

        // The data directory must still qualify — losing file logging is not a
        // reason to reject an otherwise good place to save the user's work.
        assert_eq!(storage.dir(), Some(dir.as_path()));
        assert!(storage.info().writable);
        assert_eq!(storage.log_dir(), None, "no folder target may be attached");
        assert!(storage
            .info()
            .notices
            .iter()
            .any(|n| n.contains("log folder")));
        // Diagnosability suffers, but the user's data does not: no banner.
        assert!(!storage.is_degraded());
    }

    #[test]
    fn degraded_field_matches_is_degraded_in_every_state() {
        let tmp = tempfile::tempdir().expect("tempdir");

        // The frontend gates its banner on the field while the backend gates
        // the event on the method. They must never disagree, so assert the
        // equivalence across each state that sets them differently.
        let healthy = storage_in(&tmp.path().join("healthy"));
        assert_eq!(healthy.info().degraded, healthy.is_degraded());
        assert!(!healthy.info().degraded);

        let portable = Storage::resolve(vec![("nextToExe", tmp.path().join("portable"))]);
        assert_eq!(portable.info().degraded, portable.is_degraded());
        assert!(!portable.info().degraded, "notices alone must not degrade");

        let temp = Storage::resolve(vec![("temp", tmp.path().join("temp"))]);
        assert_eq!(temp.info().degraded, temp.is_degraded());
        assert!(temp.info().degraded);

        let memory = Storage::resolve(Vec::new());
        assert_eq!(memory.info().degraded, memory.is_degraded());
        assert!(memory.info().degraded);

        // And it must flip live when a notice arrives after resolution.
        fs::write(healthy.dir().expect("dir").join("sample.json"), b"bad").expect("write");
        healthy.read::<Sample>("sample", "empty");
        assert!(
            healthy.info().degraded,
            "a reset must degrade a healthy dir"
        );
        assert_eq!(healthy.info().degraded, healthy.is_degraded());
    }

    #[test]
    fn degraded_is_on_the_wire_as_camel_case() {
        let storage = Storage::resolve(Vec::new());
        let json = serde_json::to_string(&storage.info()).expect("serialize");

        assert!(json.contains("\"degraded\":true"));
        // Round-trips, so the frontend's type can treat it as required.
        let parsed: StorageInfo = serde_json::from_str(&json).expect("parse");
        assert!(parsed.degraded);
    }

    #[test]
    fn probe_budget_clamps_between_floor_and_ceiling() {
        let now = Instant::now();

        // Plenty of budget left: capped at the per-candidate ceiling.
        assert_eq!(
            remaining_timeout(now + Duration::from_secs(60)),
            PROBE_TIMEOUT
        );
        // Budget exhausted: still gets the floor, because the tail candidates
        // are local disk and answer instantly.
        assert_eq!(remaining_timeout(now), PROBE_MIN_TIMEOUT);
        assert_eq!(
            remaining_timeout(now - Duration::from_secs(60)),
            PROBE_MIN_TIMEOUT
        );
        // In between: whatever is actually left.
        let mid = remaining_timeout(now + Duration::from_millis(1_200));
        assert!(
            mid > PROBE_MIN_TIMEOUT && mid <= PROBE_TIMEOUT,
            "got {mid:?}"
        );
    }

    #[test]
    fn next_to_exe_warns_about_elevation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = Storage::resolve(vec![("nextToExe", tmp.path().join("data"))]);

        assert!(storage
            .info()
            .notices
            .iter()
            .any(|n| n.contains("administrator")));
    }

    #[test]
    fn ordinary_locations_carry_no_notices() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        assert!(storage.info().notices.is_empty());
        assert!(!storage.is_degraded());
    }

    #[test]
    fn round_trips_json_atomically() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        let sample = Sample {
            value: 7,
            label: "hei".to_string(),
        };
        storage.write("sample", &sample).expect("write");

        assert_eq!(storage.read::<Sample>("sample", "defaults"), sample);
    }

    #[test]
    fn write_leaves_no_temp_files_behind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        for value in 0..5 {
            storage
                .write(
                    "sample",
                    &Sample {
                        value,
                        label: "x".into(),
                    },
                )
                .expect("write");
        }

        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .expect("read_dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();

        assert!(leftovers.is_empty(), "stale temp files: {leftovers:?}");
    }

    #[test]
    fn concurrent_writers_do_not_corrupt_the_destination() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().to_path_buf();

        // Simulates two Ketikin instances saving at once. With a fixed
        // `<name>.json.tmp` this interleaves and renames garbage into place.
        let handles: Vec<_> = (0..8)
            .map(|value| {
                let dir = dir.clone();
                thread::spawn(move || {
                    let bytes = serde_json::to_vec_pretty(&Sample {
                        value,
                        label: format!("writer-{value}"),
                    })
                    .expect("serialize");
                    write_atomic(&dir, "sample", &bytes).expect("write");
                })
            })
            .collect();

        for handle in handles {
            handle.join().expect("writer thread");
        }

        // Whichever writer won, the file must be one writer's complete output.
        let raw = fs::read_to_string(dir.join("sample.json")).expect("read");
        let parsed: Sample = serde_json::from_str(&raw).expect("destination must be valid JSON");
        assert_eq!(parsed.label, format!("writer-{}", parsed.value));
    }

    #[test]
    fn rename_retry_succeeds_on_a_clean_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let from = tmp.path().join("a");
        let to = tmp.path().join("b");
        fs::write(&from, b"payload").expect("write");

        rename_with_retry(&from, &to).expect("rename");

        assert!(!from.exists());
        assert_eq!(fs::read(&to).expect("read"), b"payload");
    }

    #[test]
    fn rename_retry_gives_up_and_reports_the_last_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let missing = tmp.path().join("does-not-exist");
        let to = tmp.path().join("b");

        let started = std::time::Instant::now();
        let err = rename_with_retry(&missing, &to).expect_err("must fail");

        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        // It must actually have backed off rather than failing instantly.
        assert!(started.elapsed() >= Duration::from_millis(600));
    }

    #[test]
    fn missing_file_reads_as_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        assert_eq!(
            storage.read::<Sample>("nope", "defaults"),
            Sample::default()
        );
        assert!(
            storage.info().notices.is_empty(),
            "a first run is not a notice-worthy event"
        );
    }

    #[test]
    fn corrupt_file_is_moved_aside_reset_and_announced() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        fs::write(tmp.path().join("sample.json"), b"{ not json at all").expect("write");

        assert_eq!(storage.read::<Sample>("sample", "empty"), Sample::default());
        assert!(tmp.path().join("sample.json.bak").exists());
        assert!(!tmp.path().join("sample.json").exists());

        // The user must actually be told, not just the log file.
        let notices = storage.info().notices;
        assert_eq!(notices.len(), 1, "got {notices:?}");
        assert!(notices[0].contains("sample.json"));
        assert!(notices[0].contains("reset to empty"));
        assert!(notices[0].contains("sample.json.bak"));
        assert!(
            storage.is_degraded(),
            "a silent reset must raise the warning"
        );
    }

    #[test]
    fn a_second_corruption_does_not_destroy_the_first_backup() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        fs::write(tmp.path().join("sample.json"), b"first corruption").expect("write");
        storage.read::<Sample>("sample", "empty");

        fs::write(tmp.path().join("sample.json"), b"second corruption").expect("write");
        storage.read::<Sample>("sample", "empty");

        assert_eq!(
            fs::read(tmp.path().join("sample.json.bak")).expect("read"),
            b"first corruption",
            "the original backup must survive"
        );
        assert_eq!(
            fs::read(tmp.path().join("sample.json.bak2")).expect("read"),
            b"second corruption"
        );
    }

    #[test]
    fn write_replaces_an_existing_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        storage
            .write(
                "sample",
                &Sample {
                    value: 1,
                    label: "first".into(),
                },
            )
            .expect("first write");
        storage
            .write(
                "sample",
                &Sample {
                    value: 2,
                    label: "second".into(),
                },
            )
            .expect("second write");

        assert_eq!(storage.read::<Sample>("sample", "defaults").value, 2);
    }

    #[test]
    fn log_dir_is_a_subdirectory_of_the_resolved_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        let logs = storage.log_dir().expect("log dir");
        assert_eq!(logs, tmp.path().join("logs"));
        assert!(logs.is_dir(), "probing must have created it");
    }

    #[test]
    fn the_log_probe_opens_the_file_the_plugin_will_open_and_leaves_it_there() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        // The point of the whole exercise: the plugin's folder target opens
        // `<stem>.log` with create+append, so that is what has to be proven —
        // not a uniquely-named file nobody will ever open again.
        let active = tmp
            .path()
            .join("logs")
            .join(format!("{}.log", storage.log_file()));

        assert!(active.is_file(), "the probe must create the real log file");
        assert_eq!(
            fs::read(&active).expect("read").len(),
            0,
            "and must not write to it"
        );
    }

    #[test]
    fn an_existing_log_file_survives_the_probe_untouched() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().to_path_buf();
        let logs = dir.join("logs");
        let stem = log_file_stem();
        fs::create_dir_all(&logs).expect("mkdir");
        fs::write(logs.join(format!("{stem}.log")), b"previous session\n").expect("write");

        let storage = Storage::resolve(vec![("appData", dir)]);

        assert!(storage.log_dir().is_some());
        assert_eq!(
            fs::read(logs.join(format!("{stem}.log"))).expect("read"),
            b"previous session\n",
            "appending is not truncating, and the probe must not delete it"
        );
    }

    #[test]
    fn a_log_file_that_cannot_be_opened_disqualifies_the_log_dir_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().to_path_buf();
        let logs = dir.join("logs");
        fs::create_dir_all(&logs).expect("mkdir");

        // Occupy the log file's own path with a directory, so the append-open
        // fails while everything around it succeeds. This stands in for the
        // Windows case the per-user file name exists to prevent: the folder is
        // writable by everyone, but the file inside it was created by another
        // user and its inherited ACE denies us. The plugin `?`-propagates that
        // out of `tauri::Builder::build()`, which is a silent exit 1 with no
        // window — so the probe has to catch it here.
        fs::create_dir_all(logs.join(format!("{}.log", log_file_stem()))).expect("mkdir");

        let storage = Storage::resolve(vec![("appData", dir.clone())]);

        assert_eq!(storage.log_dir(), None, "no folder target may be attached");
        // The data directory is fine, and losing a log file is not a reason to
        // throw away a working place to keep the user's work.
        assert_eq!(storage.dir(), Some(dir.as_path()));
        assert!(storage.info().writable);
        assert!(!storage.is_degraded());
        assert!(storage
            .info()
            .notices
            .iter()
            .any(|n| n.contains("log folder")));
    }

    #[test]
    fn an_oversized_log_is_rotated_before_the_plugin_has_to() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let logs = tmp.path().join("logs");
        fs::create_dir_all(&logs).expect("mkdir");
        let stem = "ketikin-test";
        let active = logs.join(format!("{stem}.log"));
        fs::write(&active, vec![b'x'; LOG_MAX_FILE_SIZE as usize + 1]).expect("write");

        probe_log_dir(&logs, stem).expect("probe");

        // The plugin's rotation is a bare `fs::rename` inside its setup closure
        // with no retry, and a failure there aborts startup. Doing it here means
        // it goes through the backoff and, if it still fails, costs only the log
        // directory.
        assert_eq!(fs::read(&active).expect("read").len(), 0);
        let archives = rotated(&logs, stem);
        assert_eq!(archives.len(), 1, "got {archives:?}");
        assert_eq!(
            fs::read(logs.join(&archives[0])).expect("read").len(),
            LOG_MAX_FILE_SIZE as usize + 1,
            "the previous session's log must be kept, not discarded"
        );
    }

    #[test]
    fn a_log_below_the_size_cap_is_left_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let logs = tmp.path().join("logs");
        fs::create_dir_all(&logs).expect("mkdir");
        let stem = "ketikin-test";
        fs::write(logs.join(format!("{stem}.log")), b"small").expect("write");

        probe_log_dir(&logs, stem).expect("probe");

        assert_eq!(
            fs::read(logs.join(format!("{stem}.log"))).expect("read"),
            b"small"
        );
        assert!(rotated(&logs, stem).is_empty());
    }

    #[test]
    fn old_rotated_logs_are_pruned_oldest_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let logs = tmp.path().join("logs");
        fs::create_dir_all(&logs).expect("mkdir");
        let stem = "ketikin-test";

        for day in 1..=4 {
            fs::write(
                logs.join(format!("{stem}_2026-01-0{day}_00-00-00.log")),
                b"x",
            )
            .expect("write");
        }
        // Another user's archives, and a file that only looks like one. Both
        // must survive: the prefix match is what keeps two users on a shared
        // portable install from deleting each other's diagnostics.
        fs::write(
            logs.join("ketikin-someone-else_2026-01-01_00-00-00.log"),
            b"x",
        )
        .expect("write");
        fs::write(logs.join(format!("{stem}.log.bak")), b"x").expect("write");

        probe_log_dir(&logs, stem).expect("probe");

        assert_eq!(
            rotated(&logs, stem),
            vec![
                format!("{stem}_2026-01-03_00-00-00.log"),
                format!("{stem}_2026-01-04_00-00-00.log"),
            ],
            "the newest {LOG_KEEP_FILES} survive"
        );
        assert!(logs
            .join("ketikin-someone-else_2026-01-01_00-00-00.log")
            .is_file());
        assert!(logs.join(format!("{stem}.log.bak")).is_file());
    }

    /// Rotated archives belonging to `stem`, sorted by name (so, by date).
    fn rotated(logs: &Path, stem: &str) -> Vec<String> {
        let prefix = format!("{stem}_");
        let mut names: Vec<String> = fs::read_dir(logs)
            .expect("read_dir")
            .filter_map(|entry| {
                let name = entry.ok()?.file_name().to_string_lossy().into_owned();
                (name.starts_with(&prefix) && name.ends_with(".log")).then_some(name)
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_log_file_name_carries_the_user() {
        assert_eq!(log_file_stem_from(Some("alice")), "Ketikin-alice");
        assert_eq!(log_file_stem_from(Some("Alice_2")), "Ketikin-Alice_2");
    }

    #[test]
    fn the_log_file_name_sanitises_anything_a_path_would_choke_on() {
        // A domain-qualified name, a space, and a traversal attempt.
        assert_eq!(
            log_file_stem_from(Some(r"CORP\alice")),
            "Ketikin-CORP-alice"
        );
        assert_eq!(
            log_file_stem_from(Some("ada lovelace")),
            "Ketikin-ada-lovelace"
        );
        assert_eq!(log_file_stem_from(Some("../../etc")), "Ketikin-etc");
        assert_eq!(log_file_stem_from(Some("a/b")), "Ketikin-a-b");
        // Distinct names must stay distinct: disallowed characters become a
        // separator rather than vanishing.
        assert_ne!(
            log_file_stem_from(Some("a b")),
            log_file_stem_from(Some("ab"))
        );
    }

    #[test]
    fn the_log_file_name_is_bounded_and_falls_back_when_there_is_no_user() {
        let long = "u".repeat(MAX_LOG_USER_CHARS * 3);
        assert_eq!(
            log_file_stem_from(Some(&long)),
            format!("Ketikin-{}", "u".repeat(MAX_LOG_USER_CHARS))
        );

        // Unset, empty, or nothing usable left after sanitising: any fixed name
        // is equally shared, so there is nothing better than the bare base —
        // which is also byte-identical to what every existing install already
        // writes, since the plugin's own default is the product name.
        for user in [None, Some(""), Some("   "), Some("///")] {
            assert_eq!(log_file_stem_from(user), "Ketikin", "for {user:?}");
        }
    }

    #[test]
    fn the_probed_file_name_is_the_one_the_plugin_is_given() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        // Two derivations would agree right up until they didn't, and the whole
        // value of the probe is that it opened *this* file.
        assert_eq!(storage.log_file(), log_file_stem());
        assert!(tmp
            .path()
            .join("logs")
            .join(format!("{}.log", storage.log_file()))
            .is_file());
    }

    #[test]
    fn resolution_diagnostics_are_buffered_in_order_for_replay() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let rejected = tmp.path().join("in-the-way");
        fs::write(&rejected, b"x").expect("write");
        let good = tmp.path().join("good");

        let storage = Storage::resolve(vec![
            ("appData", rejected.clone()),
            ("nextToExe", good.clone()),
        ]);

        // `Storage::resolve` runs before `tauri::Builder`, so `log::max_level()`
        // is still `Off` and every one of these would otherwise be dropped on
        // the floor — including *why* the chain fell through, which is the one
        // line a locked-down-Windows bug report needs.
        let diagnostics = storage.take_diagnostics();
        let messages: Vec<&str> = diagnostics.iter().map(|(_, m)| m.as_str()).collect();

        assert!(
            messages[0].contains("appData candidate") && messages[0].contains("rejected"),
            "the rejection and its error must come first: {messages:?}"
        );
        assert!(messages[0].contains(&rejected.display().to_string()));
        assert!(messages[1].contains("using") && messages[1].contains("nextToExe"));
        // Then every notice pushed during resolution, in the order it arrived.
        assert!(messages[2].contains("shared with other users"));
        assert!(messages[3].contains("administrator"));
        assert_eq!(messages.len(), 4, "got {messages:?}");

        assert_eq!(diagnostics[0].0, log::Level::Warn);
        assert_eq!(diagnostics[1].0, log::Level::Info);
    }

    #[test]
    fn the_in_memory_fallback_explains_itself_to_the_log_too() {
        let unusable = tempfile::tempdir().expect("tempdir");
        let blocked = unusable.path().join("file-not-dir");
        fs::write(&blocked, b"x").expect("write");

        let storage = Storage::resolve(vec![("appData", blocked)]);
        let messages: Vec<String> = storage
            .take_diagnostics()
            .into_iter()
            .map(|(_, message)| message)
            .collect();

        assert!(messages
            .iter()
            .any(|m| m.contains("falling back to in-memory state")));
        assert!(
            messages
                .iter()
                .any(|m| m.contains("lost when you close it")),
            "the user-facing notice belongs in the log as well: {messages:?}"
        );
    }

    #[test]
    fn draining_the_buffer_hands_later_diagnostics_straight_to_the_logger() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());
        assert_eq!(
            storage.take_diagnostics().len(),
            1,
            "a clean start records only which candidate won"
        );

        // Everything after `setup` has replayed has a live logger, so nothing
        // may keep accumulating behind it for the rest of the run.
        fs::write(tmp.path().join("sample.json"), b"not json").expect("write");
        storage.read::<Sample>("sample", "empty");

        assert!(!storage.info().notices.is_empty(), "the notice still lands");
        assert!(storage.take_diagnostics().is_empty(), "but is not buffered");
    }

    #[test]
    fn storage_info_serializes_notices_as_camel_case_array() {
        let storage = Storage::resolve(Vec::new());
        let json = serde_json::to_string(&storage.info()).expect("serialize");

        assert!(json.contains("\"notices\":["));
        assert!(json.contains("\"writable\":false"));
    }
}
