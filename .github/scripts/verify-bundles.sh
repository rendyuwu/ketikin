#!/usr/bin/env bash
#
# Post-build smoke check: assert that `tauri build` actually emitted the
# installers we promise on the release page. A Tauri build can succeed while
# silently skipping a bundle target (missing tool, unsupported target triple),
# and a release that ships three of five artifacts is worse than a red build.
#
# Usage: verify-bundles.sh <bundle-root> <matrix-id>
#   bundle-root  e.g. src-tauri/target/release/bundle
#   matrix-id    linux-x64 | windows-x64 | macos-x64 | macos-arm64

set -euo pipefail

root="${1:?verify-bundles: bundle root argument is required}"
id="${2:?verify-bundles: matrix id argument is required}"

die() {
  printf '::error::%s\n' "$*" >&2
  exit 1
}

[ -d "$root" ] || die "verify-bundles: bundle root '$root' does not exist — tauri build produced nothing."

# "<human label>|<find -name pattern>"
case "$id" in
  linux-*)
    expected=("AppImage|*.AppImage" "Debian package|*.deb")
    ;;
  windows-*)
    expected=("MSI installer|*.msi" "NSIS installer|*-setup.exe")
    ;;
  macos-*)
    # .app is a directory, not a file, so these checks deliberately do not
    # constrain -type.
    expected=("disk image|*.dmg" "app bundle|*.app")
    ;;
  *)
    die "verify-bundles: unknown matrix id '$id'."
    ;;
esac

missing=0
for entry in "${expected[@]}"; do
  label="${entry%%|*}"
  pattern="${entry#*|}"
  found="$(find "$root" -name "$pattern" 2>/dev/null || true)"
  if [ -z "$found" ]; then
    printf '::error::Missing %s for %s — no %s under %s\n' "$label" "$id" "$pattern" "$root" >&2
    missing=1
  else
    while IFS= read -r path; do
      printf 'ok  %-16s %s\n' "$label" "$path"
    done <<<"$found"
  fi
done

if [ "$missing" -ne 0 ]; then
  printf '\nContents of %s:\n' "$root" >&2
  find "$root" -maxdepth 2 >&2 || true
  die "verify-bundles: one or more expected bundles were not produced for $id."
fi

printf 'All expected bundles present for %s.\n' "$id"
