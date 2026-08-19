//! Data-directory resolution and atomic JSON persistence.
//!
//! This module is the reason the backend was rewritten. The predecessor wrote
//! its JSON only under the user's home directory and silently lost every save
//! on locked-down Windows Server profiles where that path is not writable.
//!
//! Here the directory is resolved exactly once at startup by *probing* a list
//! of candidates and keeping the first one that survives a real create → write
//! → fsync → delete round trip. If none of them work the app keeps running with
//! purely in-memory state instead of failing to launch.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// Bundle identifier, reused as the directory name for the env-var fallbacks.
pub const APP_DIR_NAME: &str = "com.rendyuwu.ketikin";

/// Emitted once at startup when saves may not survive a restart.
pub const EVENT_WARNING: &str = "storage://warning";

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
}

/// A resolved, writability-checked data directory plus atomic JSON helpers.
#[derive(Debug)]
pub struct Storage {
    /// `None` when we are running in memory-only mode.
    dir: Option<PathBuf>,
    info: StorageInfo,
}

impl Storage {
    /// Build the candidate list for the real application.
    ///
    /// Candidates 2 and 3 are deliberately *not* `#[cfg]`-gated to Windows: the
    /// environment variables simply do not exist elsewhere, and gating them
    /// would mean the fallback chain is never exercised on developer machines.
    pub fn candidates(app: &AppHandle) -> Vec<(&'static str, PathBuf)> {
        let mut candidates: Vec<(&'static str, PathBuf)> = Vec::with_capacity(5);

        if let Ok(dir) = app.path().app_data_dir() {
            candidates.push(("appData", dir));
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

        for (source, dir) in candidates {
            match probe_writable(&dir) {
                Ok(()) => {
                    log::info!("storage: using {} (source: {source})", dir.display());
                    return Self {
                        info: StorageInfo {
                            path: dir.display().to_string(),
                            source: source.to_string(),
                            writable: true,
                            error: None,
                        },
                        dir: Some(dir),
                    };
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
            info: StorageInfo {
                path: String::new(),
                source: "memory".to_string(),
                writable: false,
                error: Some(error),
            },
        }
    }

    /// Snapshot for the `storage_info` command and the `storage://warning` event.
    pub fn info(&self) -> StorageInfo {
        self.info.clone()
    }

    /// True when the user should be told that saves may not survive a restart.
    pub fn is_degraded(&self) -> bool {
        !self.info.writable || self.info.source == "temp" || self.info.source == "memory"
    }

    /// Load `<name>.json`, falling back to `T::default()` rather than failing.
    ///
    /// A missing file is normal (first run). A *corrupt* file is moved aside to
    /// `<name>.json.bak` so the user can still recover it by hand, and defaults
    /// are returned — a single bad byte must not brick the whole app.
    pub fn read<T: DeserializeOwned + Default>(&self, name: &str) -> T {
        let Some(dir) = &self.dir else {
            return T::default();
        };
        let path = dir.join(format!("{name}.json"));

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return T::default(),
            Err(err) => {
                log::warn!("storage: could not read {}: {err}", path.display());
                return T::default();
            }
        };

        match serde_json::from_str::<T>(&raw) {
            Ok(value) => value,
            Err(err) => {
                let backup = dir.join(format!("{name}.json.bak"));
                match fs::rename(&path, &backup) {
                    Ok(()) => log::error!(
                        "storage: {} was corrupt ({err}); moved to {} and reset to defaults",
                        path.display(),
                        backup.display()
                    ),
                    Err(rename_err) => log::error!(
                        "storage: {} was corrupt ({err}) and could not be moved aside ({rename_err}); using defaults",
                        path.display()
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

/// Prove a directory is writable by actually writing to it.
///
/// Metadata permission bits are not consulted on purpose: on Windows they lie
/// about the effective ACL, which is exactly the failure mode this app hit in
/// production. Only a completed create + write + fsync + delete counts.
fn probe_writable(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;

    let probe = dir.join(format!(".ketikin-write-probe-{}", std::process::id()));
    {
        let mut file = fs::File::create(&probe)?;
        file.write_all(b"ketikin")?;
        file.sync_all()?;
    }
    fs::remove_file(&probe)?;

    Ok(())
}

/// Write via a sibling temp file and rename over the destination.
///
/// The temp file must live in the *same* directory so the rename stays on one
/// filesystem and is therefore atomic. Rust's `fs::rename` passes
/// `MOVEFILE_REPLACE_EXISTING` on Windows, so clobbering an existing file works
/// there too. A crash mid-write leaves the previous good file untouched.
fn write_atomic(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!("{name}.json.tmp"));
    let dest = dir.join(format!("{name}.json"));

    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    match fs::rename(&tmp, &dest) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Do not leave a stale .tmp behind to confuse the next write.
            let _ = fs::remove_file(&tmp);
            Err(err)
        }
    }
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
        assert_eq!(storage.dir.as_deref(), Some(good.as_path()));
    }

    #[test]
    fn falls_back_to_memory_when_nothing_is_writable() {
        let storage = Storage::resolve(Vec::new());
        let info = storage.info();

        assert_eq!(info.source, "memory");
        assert!(!info.writable);
        assert!(info.error.is_some());
        assert!(storage.is_degraded());

        // Memory mode must still behave like storage: reads give defaults and
        // writes succeed (as no-ops) so no command path can fail because of it.
        assert_eq!(storage.read::<Sample>("settings"), Sample::default());
        assert!(storage.write("settings", &Sample::default()).is_ok());
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

        assert_eq!(storage.read::<Sample>("sample"), sample);
        // The temp file must not survive a successful write.
        assert!(!tmp.path().join("sample.json.tmp").exists());
    }

    #[test]
    fn missing_file_reads_as_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        assert_eq!(storage.read::<Sample>("nope"), Sample::default());
    }

    #[test]
    fn corrupt_file_is_moved_aside_and_reset() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let storage = storage_in(tmp.path());

        fs::write(tmp.path().join("sample.json"), b"{ not json at all").expect("write");

        assert_eq!(storage.read::<Sample>("sample"), Sample::default());
        assert!(tmp.path().join("sample.json.bak").exists());
        assert!(!tmp.path().join("sample.json").exists());

        // And the next write must repair the file rather than stay broken.
        let sample = Sample {
            value: 1,
            label: "ok".to_string(),
        };
        storage.write("sample", &sample).expect("write");
        assert_eq!(storage.read::<Sample>("sample"), sample);
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

        assert_eq!(storage.read::<Sample>("sample").value, 2);
    }
}
