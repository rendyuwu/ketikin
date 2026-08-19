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
    /// Every rejection is logged so a support request can be diagnosed from the
    /// log file alone.
    pub fn resolve(candidates: Vec<(&'static str, PathBuf)>) -> Self {
        let mut failures: Vec<String> = Vec::new();
        let deadline = Instant::now() + PROBE_BUDGET;

        for (source, dir) in candidates {
            match probe_writable_bounded(&dir, remaining_timeout(deadline)) {
                Ok(()) => {
                    log::info!("storage: using {} (source: {source})", dir.display());

                    // Probe `<dir>/logs` separately rather than inferring it.
                    // On Windows, creating a *file* and creating a
                    // *subdirectory* are separately grantable rights
                    // (FILE_ADD_FILE vs FILE_ADD_SUBDIRECTORY), and hardened
                    // session-host ACLs really do grant one without the other —
                    // so passing the data probe does not imply the log folder
                    // can be created. Failing this must not disqualify an
                    // otherwise good data directory.
                    let logs = dir.join("logs");
                    let log_dir = match probe_writable_bounded(&logs, remaining_timeout(deadline)) {
                        Ok(()) => Some(logs),
                        Err(err) => {
                            log::warn!("storage: log directory {} unusable: {err}", logs.display());
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
                    log::warn!(
                        "storage: {source} candidate {} rejected: {err}",
                        dir.display()
                    );
                    failures.push(format!("{source} ({}): {err}", dir.display()));
                }
            }
        }

        let error = if failures.is_empty() {
            "no data directory candidates were available".to_string()
        } else {
            format!("no writable data directory found — {}", failures.join("; "))
        };
        log::error!("storage: {error}; falling back to in-memory state");

        Self {
            dir: None,
            log_dir: None,
            alarming: AtomicBool::new(true),
            info: Mutex::new(StorageInfo {
                path: String::new(),
                source: "memory".to_string(),
                writable: false,
                error: Some(error),
                notices: vec![
                    "Ketikin could not find anywhere to save data, so settings and templates \
                     will be lost when you close it."
                        .to_string(),
                ],
                degraded: true,
            }),
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
        log::warn!("storage: {notice}");
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

/// Run [`probe_writable`] with a deadline.
///
/// The probe is uninterruptible once it is blocked in a filesystem call, so on
/// timeout the worker thread is simply abandoned. It will finish on its own
/// eventually; what matters is that startup does not wait for it.
fn probe_writable_bounded(dir: &Path, timeout: Duration) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let target = dir.to_path_buf();

    thread::Builder::new()
        .name("ketikin-storage-probe".to_string())
        .spawn(move || {
            let _ = tx.send(probe_writable(&target).map_err(|err| err.to_string()));
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
/// The temp name carries the pid and a counter because Ketikin has no
/// single-instance lock: two running copies saving the same file would
/// otherwise truncate each other's temp file and rename interleaved bytes into
/// place as authoritative JSON.
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
    fn both_shared_locations_carry_a_notice() {
        let tmp = tempfile::tempdir().expect("tempdir");

        for source in ["nextToExe", "temp"] {
            let dir = tmp.path().join(source);
            let storage = Storage::resolve(vec![(source, dir)]);
            let info = storage.info();

            assert!(info.writable, "{source} should still be usable");
            assert!(
                info.notices
                    .iter()
                    .any(|n| n.contains("shared with other users")),
                "{source} must warn about being shared, got {:?}",
                info.notices
            );
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
    fn storage_info_serializes_notices_as_camel_case_array() {
        let storage = Storage::resolve(Vec::new());
        let json = serde_json::to_string(&storage.info()).expect("serialize");

        assert!(json.contains("\"notices\":["));
        assert!(json.contains("\"writable\":false"));
    }
}
