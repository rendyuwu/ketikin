#!/usr/bin/env bash
#
# Assert that Ketikin's version is declared identically in all three places
# that ship it, and that CHANGELOG.md documents that version.
#
#   package.json              .version
#   src-tauri/tauri.conf.json .version
#   src-tauri/Cargo.toml      [package] version
#
# Usage:
#   check-version-sync.sh                  # package.json is the source of truth
#   check-version-sync.sh 0.1.0            # the three files must also equal 0.1.0
#
# On success the agreed version is printed to stdout (and nothing else — every
# diagnostic goes to stderr, so callers can capture it safely).

set -euo pipefail

expected="${1:-}"

die() {
  printf '::error::%s\n' "$*" >&2
  exit 1
}

for file in package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml CHANGELOG.md; do
  [ -f "$file" ] || die "check-version-sync: required file '$file' is missing."
done

package_version="$(jq -r '.version // empty' package.json)"
tauri_version="$(jq -r '.version // empty' src-tauri/tauri.conf.json)"

# Cargo.toml is read with awk rather than a TOML parser so this script has no
# dependencies beyond what every GitHub runner already ships. Tracking the
# current section header keeps us from picking up a `version = "..."` that
# belongs to a dependency table instead of [package].
cargo_version="$(
  awk '
    /^[[:space:]]*\[/ { section = $0; next }
    section ~ /^\[package\]/ && /^[[:space:]]*version[[:space:]]*=/ {
      line = $0
      sub(/^[^=]*=[[:space:]]*"/, "", line)
      sub(/".*$/, "", line)
      print line
      exit
    }
  ' src-tauri/Cargo.toml
)"

[ -n "$package_version" ] || die "check-version-sync: package.json has no .version field."
[ -n "$tauri_version" ] || die "check-version-sync: src-tauri/tauri.conf.json has no .version field."
[ -n "$cargo_version" ] || die "check-version-sync: src-tauri/Cargo.toml has no [package] version field."

if [ "$package_version" != "$tauri_version" ] || [ "$package_version" != "$cargo_version" ]; then
  die "$(printf 'Version mismatch — all three must agree. package.json=%s tauri.conf.json=%s Cargo.toml=%s' \
    "$package_version" "$tauri_version" "$cargo_version")"
fi

version="$package_version"

if [ -n "$expected" ] && [ "$expected" != "$version" ]; then
  die "$(printf 'Version mismatch — expected %s but the source tree declares %s in all three files.' \
    "$expected" "$version")"
fi

# Dots in the version would otherwise act as regex wildcards.
escaped="${version//./\\.}"
if ! grep -qE "^## \[${escaped}\]" CHANGELOG.md; then
  die "$(printf 'CHANGELOG.md has no "## [%s]" heading. Add the release section before shipping %s.' \
    "$version" "$version")"
fi

printf 'Version %s is consistent across package.json, tauri.conf.json and Cargo.toml.\n' "$version" >&2
printf '%s\n' "$version"
