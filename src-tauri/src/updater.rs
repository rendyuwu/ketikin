//! Update checking, installing, and release notes.
//!
//! The resolved [`Update`] from a successful check is cached in application
//! state so `install_update` can download immediately instead of paying for a
//! second round trip (and risking a different answer).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

use crate::error::AppError;
use crate::AppState;

/// Single source of truth for the GitHub repository this app updates from.
pub const REPO_SLUG: &str = "rendyuwu/ketikin";

/// Emitted when a background check finds a newer version.
pub const EVENT_AVAILABLE: &str = "update://available";

/// Ceiling on a single check so an unreachable endpoint cannot wedge the UI.
/// Applied to the check only — the download deliberately has no deadline.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// What the frontend needs to describe an available update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
    /// Whether [`install`] can actually apply this update in place.
    ///
    /// `false` only on Linux outside an AppImage — see [`install_supported`].
    /// The update is still reported when this is `false`: the user should know
    /// a new version exists, they just have to fetch it themselves.
    pub can_install: bool,
}

/// Can this build replace itself in place?
///
/// Only Linux says no, and only outside an AppImage. `createUpdaterArtifacts:
/// true` signs the AppImage directly: a bundle run on this tree emits
/// `Ketikin_<version>_amd64.AppImage` and a matching `.sig`, with no tarball
/// anywhere. (The zipped `.AppImage.tar.gz` layout is Tauri v1's, still
/// reachable via `createUpdaterArtifacts: "v1Compatible"` — this app does not
/// use it. Don't go looking for that file.) Either way the AppImage is the only
/// Linux updater artifact we publish, so `latest.json` always points a Linux
/// client at AppImage bytes.
///
/// A `.deb` or `.rpm` install has its bundle type patched into the binary at
/// bundle time, so the plugin routes those bytes to `install_deb`/`install_rpm`,
/// which reject them as the wrong archive format — an opaque
/// `InvalidUpdaterFormat` *after* a full download. Detect it up front instead.
///
/// `APPIMAGE` is the right runtime signal here (it is what Tauri's own `Env`
/// reads): it is set by the AppImage runtime, and its absence also correctly
/// catches an extracted AppImage being run from its unpacked directory, where
/// self-update is equally broken.
fn install_supported() -> bool {
    let has_appimage = std::env::var_os("APPIMAGE").is_some_and(|value| !value.is_empty());

    install_supported_for(cfg!(target_os = "linux"), has_appimage)
}

/// Pure form of [`install_supported`], so the rule is testable off-Linux.
fn install_supported_for(is_linux: bool, has_appimage: bool) -> bool {
    !is_linux || has_appimage
}

/// Where to send someone who has to update by hand.
fn releases_url() -> String {
    format!("https://github.com/{REPO_SLUG}/releases")
}

/// Ask the endpoint whether a newer version exists.
///
/// `Ok(None)` means "already up to date". The resolved update is cached for
/// [`install`].
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, AppError> {
    let updater = app
        .updater()
        .map_err(|err| AppError::Updater(format!("the updater is not configured: {err}")))?;

    let found = tokio::time::timeout(CHECK_TIMEOUT, updater.check())
        .await
        .map_err(|_| {
            AppError::Updater(
                "the update check timed out — check your network connection and try again"
                    .to_string(),
            )
        })?
        .map_err(|err| AppError::Updater(format!("could not check for updates: {err}")))?;

    let Some(update) = found else {
        cache(app, None);
        log::info!("updater: already up to date");
        return Ok(None);
    };

    let info = UpdateInfo {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        // `OffsetDateTime`'s Display form; the frontend treats this as an
        // opaque human-readable string.
        date: update.date.map(|date| date.to_string()),
        can_install: install_supported(),
    };
    log::info!(
        "updater: {} is available (current {}, self-install {})",
        info.version,
        info.current_version,
        if info.can_install {
            "supported"
        } else {
            "unavailable — not running from an AppImage"
        }
    );

    cache(app, Some(update));
    Ok(Some(info))
}

/// Download and install the update found by the most recent [`check`], then
/// restart into it. Never returns on success.
pub async fn install(app: &AppHandle) -> Result<(), AppError> {
    // Guard first, before touching the cached update or spending a download.
    // This is the authoritative check — `UpdateInfo::can_install` only tells the
    // UI what to render, and must never be the only thing standing between a
    // .deb user and a doomed download.
    if !install_supported() {
        return Err(AppError::Updater(format!(
            "Ketikin was installed from a system package (.deb or .rpm) and cannot update itself. \
             Download the new version from {} and install it with your package manager.",
            releases_url()
        )));
    }

    // Clone the cached update out and drop the guard immediately: the lock must
    // not be held across the download await.
    let update = {
        let state = app
            .try_state::<AppState>()
            .ok_or_else(|| AppError::Updater("Ketikin is still starting up".to_string()))?;
        let pending = crate::lock(&state.pending_update);
        pending.clone()
    };

    let update = update.ok_or_else(|| {
        AppError::Updater("no update is ready to install — check for updates first".to_string())
    })?;

    log::info!("updater: installing {}", update.version);
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|err| AppError::Updater(format!("could not install the update: {err}")))?;

    // `AppHandle::restart` is the in-process equivalent of the
    // `tauri-plugin-process` restart command, and diverges.
    app.restart()
}

/// Open the GitHub release page for `version` in the default browser.
pub fn open_release_notes(app: &AppHandle, version: &str) -> Result<(), AppError> {
    let tag = sanitize_version(version)?;
    let url = format!("https://github.com/{REPO_SLUG}/releases/tag/v{tag}");

    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|err| AppError::Updater(format!("could not open {url}: {err}")))
}

fn cache(app: &AppHandle, update: Option<tauri_plugin_updater::Update>) {
    if let Some(state) = app.try_state::<AppState>() {
        *crate::lock(&state.pending_update) = update;
    }
}

/// Keep the version out of the URL's structure. Semver tags only ever contain
/// these characters, so anything else is a caller bug or an injection attempt.
fn sanitize_version(version: &str) -> Result<String, AppError> {
    let trimmed = version.trim().trim_start_matches('v');

    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
    {
        return Err(AppError::Invalid(format!(
            "{version} is not a valid version number"
        )));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_accepts_semver_with_or_without_the_v_prefix() {
        assert_eq!(sanitize_version("1.2.3").expect("valid"), "1.2.3");
        assert_eq!(sanitize_version("v1.2.3").expect("valid"), "1.2.3");
        assert_eq!(
            sanitize_version(" 1.2.3-rc.1 ").expect("valid"),
            "1.2.3-rc.1"
        );
        assert_eq!(
            sanitize_version("1.2.3+build7").expect("valid"),
            "1.2.3+build7"
        );
    }

    #[test]
    fn sanitize_rejects_anything_that_could_reshape_the_url() {
        assert!(sanitize_version("").is_err());
        assert!(sanitize_version("v").is_err());
        assert!(sanitize_version("1.2.3/../../evil").is_err());
        assert!(sanitize_version("1.2.3?x=1").is_err());
        assert!(sanitize_version("1.2.3 4").is_err());
    }

    #[test]
    fn release_url_is_built_from_the_single_repo_constant() {
        let tag = sanitize_version("v0.1.0").expect("valid");
        assert_eq!(
            format!("https://github.com/{REPO_SLUG}/releases/tag/v{tag}"),
            "https://github.com/rendyuwu/ketikin/releases/tag/v0.1.0"
        );
    }

    #[test]
    fn update_info_serializes_as_camel_case() {
        let json = serde_json::to_string(&UpdateInfo {
            version: "1.0.0".into(),
            current_version: "0.1.0".into(),
            notes: None,
            date: None,
            can_install: true,
        })
        .expect("serialize");

        // Pin the exact wire format. The frontend types this as a required,
        // non-nullable `canInstall: boolean` and takes the no-self-update
        // branch when it is falsy, so a rename or an accidental
        // `skip_serializing_if` would silently mis-render for every Windows
        // and macOS user.
        assert_eq!(
            json,
            r#"{"version":"1.0.0","currentVersion":"0.1.0","notes":null,"date":null,"canInstall":true}"#
        );
    }

    #[test]
    fn update_info_always_emits_can_install() {
        let json = serde_json::to_string(&UpdateInfo {
            version: "1.0.0".into(),
            current_version: "0.1.0".into(),
            notes: Some("fixed things".into()),
            date: Some("2026-01-15 10:30:00.0 +00:00:00".into()),
            can_install: false,
        })
        .expect("serialize");

        assert!(json.contains(r#""canInstall":false"#));
        assert!(!json.contains("can_install"));
    }

    #[test]
    fn self_install_is_unavailable_on_linux_outside_an_appimage() {
        // The one case that must be caught: a .deb or .rpm install.
        assert!(!install_supported_for(true, false));

        // Linux from an AppImage is fine.
        assert!(install_supported_for(true, true));

        // Windows and macOS always self-install; APPIMAGE is irrelevant there.
        assert!(install_supported_for(false, false));
        assert!(install_supported_for(false, true));
    }

    #[test]
    fn install_supported_matches_the_current_platform() {
        // Sanity-check that the runtime wrapper agrees with the pure rule for
        // whatever this test is running on, so the two cannot drift apart.
        let has_appimage = std::env::var_os("APPIMAGE").is_some_and(|value| !value.is_empty());

        assert_eq!(
            install_supported(),
            install_supported_for(cfg!(target_os = "linux"), has_appimage)
        );
    }

    #[test]
    fn manual_update_message_points_at_the_releases_index() {
        assert_eq!(
            releases_url(),
            "https://github.com/rendyuwu/ketikin/releases"
        );
    }
}
