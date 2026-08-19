# Releasing Ketikin

A maintainer runbook. This covers cutting a release, verifying it, publishing it, and recovering
from a bad one.

The short version: bump three version numbers, write a changelog entry, push a `vX.Y.Z` tag, wait
for the workflow, check the draft release, publish it by hand.

## One-time setup: signing keys

Ketikin's updater only installs artifacts signed with a minisign key whose public half is compiled
into the application. That keypair is generated once and then reused for every release.

Generate it with the Tauri CLI:

```bash
npx @tauri-apps/cli signer generate -w ~/.tauri/ketikin.key
```

This produces two files:

- `~/.tauri/ketikin.key` — the **private** key. Never commit it. Back it up somewhere you will not
  lose it; losing it means no existing installation can be updated automatically again.
- `~/.tauri/ketikin.key.pub` — the **public** key. This value goes into the updater configuration
  in `src-tauri/tauri.conf.json` and is compiled into every build.

Use an **empty password** when prompted. The release workflow runs unattended and has no way to
answer an interactive prompt.

### Required repository secrets

Set these under **Settings > Secrets and variables > Actions** on `rendyuwu/ketikin`:

| Secret | Value |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | The full contents of the private key file (`~/.tauri/ketikin.key`) — the entire file, including any header lines, not the file path. |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | An empty string. The secret must still exist; its value is empty. |

If `TAURI_SIGNING_PRIVATE_KEY` is missing or malformed, the build will either fail or produce
artifacts without `.sig` files. Unsigned artifacts are not usable by the updater, so a release like
that must not be published.

## Cutting a release

### 1. Bump the version in three places

These must all agree, or the updater will compare the wrong numbers:

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

Run `npm install` after bumping `package.json` so the lockfile picks up the new version, and build
once so `Cargo.lock` updates too.

### 2. Write the changelog entry

Add a section to `CHANGELOG.md` above the previous release. The heading format is load-bearing —
the release workflow extracts the release notes by slicing the file for the `## [X.Y.Z]` heading
that matches the tag:

```
## [0.2.0] - 2026-09-01
```

Exactly that: `## `, the version in square brackets, a space-hyphen-space, and an ISO date. If the
heading does not match the tag, the release comes out with empty notes.

### 3. Commit and push

```bash
git add -A
git commit -m "chore(release): v0.2.0"
git push origin main
```

### 4. Tag

```bash
git tag v0.2.0
git push origin v0.2.0
```

Pushing the tag is what triggers the release workflow. It builds all four targets, signs each
artifact with the private key from the repository secrets, generates `latest.json`, and creates a
**draft** GitHub release with the notes sliced out of `CHANGELOG.md`.

### 5. Wait for the build

All four platform builds must succeed:

- Windows x64
- Linux x64
- macOS x64 (Intel)
- macOS arm64 (Apple Silicon)

If any one of them fails, do not publish a partial release — see
[When a release is bad](#when-a-release-is-bad).

## How `latest.json` works

`latest.json` is the update manifest. The updater in a running copy of Ketikin fetches it, compares
the version it declares against its own, and if the manifest is newer, downloads the URL listed for
its platform and verifies it against the signature in the manifest.

The release workflow generates it and uploads it as a release asset. For each platform it records
the download URL of the artifact and the contents of that artifact's `.sig` file. You do not write
this file by hand.

The Linux entry in the manifest refers to the AppImage. It is the only Linux artifact the updater
can install in place, so a release that builds a `.deb` and `.rpm` but no AppImage would leave
Linux users with no upgrade path — worth a glance when you check the manifest.

The updater endpoint configured in `src-tauri/tauri.conf.json` points at
`releases/latest/download/latest.json`. That `latest` path resolves to the most recent *published*
release, which is exactly why a draft release is invisible to the updater: GitHub does not count a
draft as the latest release, so the endpoint keeps serving the previous version's manifest until
you publish.

The important consequence: `latest.json` must contain an entry for every platform you intend to
serve. A platform missing from the manifest simply never receives the update — installations on it
stay on the old version indefinitely, with no error shown to the user.

## Verifying a release before publishing

Go through this list on the draft release before touching the Publish button.

1. **All four platform entries are present in `latest.json`.** Download the asset and read it.
   Confirm there is an entry for Windows x64, Linux x64, macOS x64, and macOS arm64, that each URL
   points at an asset that actually exists on this release, and that the `version` field matches
   the tag.
2. **Every artifact the updater serves has its `.sig` uploaded.** That means the `.msi`, the NSIS
   `-setup.exe`, the `.AppImage`, and the macOS `.app.tar.gz`. An artifact without a signature
   cannot be installed by the updater.

   Do **not** read a missing `.sig` beside the `.dmg` or the `.rpm` as a fault. Neither is an
   updater target — the `.dmg` is a manual-download format and the `.rpm` cannot self-install, as
   described under [Linux: only the AppImage self-updates](../README.md#linux-only-the-appimage-self-updates)
   — so the bundler never signs them. The authoritative check is the one below: if every platform
   entry in `latest.json` carries a non-empty signature, the updater has everything it needs.
3. **The version numbers agree.** The tag, the version in `latest.json`, and the version shown by
   the built app should all be the same.
4. **The release notes are right.** They should be the `### Added` / `### Fixed` content from the
   matching `CHANGELOG.md` section. Empty or wrong notes almost always mean the changelog heading
   did not match the tag.
5. **Smoke test at least one build.** Download it, install it, launch it, and type something.
   Ideally test an actual update from the previous version.

### Publishing

The workflow creates the release as a **draft** on purpose. A human must publish it.

Nothing reaches users until you do. GitHub does not serve draft release assets to the updater, so
the manifest is unreachable and no installation will see the new version while the release sits in
draft. This is the safety gate: everything is built, signed, and inspectable, and it only goes live
when someone has looked at it.

When the checks above pass, click **Publish release**. Installations with automatic update checks
enabled will start picking it up on their next check.

## When a release is bad

If a build failed, an artifact is missing a signature, or you spot a problem after publishing, the
recovery is to remove the release and tag, fix the cause, and re-tag.

1. **Delete the GitHub release.** On the release page, choose Delete. If it was already published,
   delete it promptly — anything already downloaded by an updater is out of your hands, which is
   the reason for the pre-publish checks.

2. **Delete the tag**, locally and on the remote:

   ```bash
   git tag -d v0.2.0
   git push origin :refs/tags/v0.2.0
   ```

3. **Fix the problem** and commit the fix to `main`.

4. **Re-tag and push**, using the same version number if nothing was ever published, or bumping to
   the next patch version if a broken release did go public. Do not reuse a version number that
   users may already have installed — the updater compares versions, and a machine that already has
   `0.2.0` will not install a different `0.2.0`.

If a published release turns out to be broken in a way that affects users, ship a patch release
rather than trying to un-publish it. Once the updater has served a version, the only reliable fix
is a newer version.

## Triaging a user report

**Ask for the `logs/` directory first.** Storage resolution and hotkey registration failures are
logged and nothing else surfaces them, so without the log most reports cannot be diagnosed at all.

The log lives in a `logs/` subdirectory of whatever storage directory the app resolved — the path
Settings displays, plus `logs/`. Ask for the whole directory rather than `Ketikin.log` alone: it
rotates at 1 MB keeping two dated files, so the evidence for a startup problem may already have
rolled into `Ketikin_<timestamp>.log`. The cap means the whole directory is around 3 MB at worst,
which is safe to ask someone to attach.

Note that the rotation settings are deliberate. The logging plugin's defaults — roughly 40 KB,
keeping one file — are small enough that a single long typing session rolls straight past them and
discards the startup diagnostics that make the log worth having. If you ever touch the logging
configuration, keep the size well above one session's worth of output.

**When there is no log, ask for Settings > Storage instead.** Two situations produce no log file.
In in-memory mode every storage candidate failed, so logging fell back to standard output, which a
Windows release build discards. Separately — and this one is easy to misread as "the log is
missing" — the data directory can be writable while the `logs/` subdirectory cannot be created,
which is a real Windows ACL case because adding files and adding subdirectories are separately
granted permissions. Data saves fine; no log is ever written. Settings > Storage states outright
when file logging is unavailable, and carries the resolved path, the fallback source, the error,
and any notices. A screenshot of that panel is the substitute for the log.

**Do not use the banner as your signal, in either direction.** It fires for three things: the temp
directory, in-memory mode, and a `settings.json` or `templates.json` that could not be read and had
to be reset to defaults.

That third one is worth recognising on sight, because it is the only one that says nothing about
*where* the data went — it fires on a perfectly healthy `appData` path and means the user has
already lost something. The notice names the backup the unreadable file was renamed to, so a
"my templates all disappeared" report is often recoverable by hand from that file. Do not file it
under storage problems.

In the other direction, "no banner" does not mean "nothing to see". Running from the folder beside
the executable is a supported portable deployment and shows no banner, but it does carry notices in
Settings > Storage — that the location may be shared with other users, and that the resolved
directory can depend on elevation. A `logs/` subdirectory that could not be created is also
Settings-only. Always ask for the panel, not the banner.

**`ketikin-startup-error.log`** is written into the resolved directory when startup fails before a
window can exist — the case where there is no UI to show an error and, on Windows, no console
either. Ask for it when the report is "nothing happens when I launch it". Treat its absence as
uninformative rather than exculpatory: it can only be written if storage resolved.

**Ask every time:** the exact version, the platform, and how they installed it. The install method
matters more than it looks — on Linux it determines whether they can self-update at all, and on
Windows it determines whether an elevation mismatch explains "nothing gets typed".

**Ask whether they ran it elevated**, if the report involves missing settings or templates on
Windows. An elevated and a non-elevated launch can resolve to different data directories on
locked-down machines, which presents exactly as data loss and is not.
