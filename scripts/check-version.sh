#!/usr/bin/env bash
# Verify that the release tag (when given), Cargo.toml, herdr-plugin.toml,
# and Cargo.lock all declare the same version.
#
# Usage:
#   scripts/check-version.sh            # check manifest/lock consistency only
#   scripts/check-version.sh v1.2.3     # also require the tag to match
set -euo pipefail

cd "$(dirname "$0")/.."

cargo_version=$(awk '/^\[package\]/{p=1; next} /^\[/{p=0} p && /^version = "/{gsub(/"/, ""); split($0, a, "= "); print a[2]; exit}' Cargo.toml)
plugin_version=$(awk -F'"' '/^\[/{exit} /^version = "/{print $2; exit}' herdr-plugin.toml)
lock_version=$(awk '
  /^\[\[package\]\]/{name=""}
  /^name = /{name=$3}
  /^version = / && name=="\"herdr-lineup\""{gsub(/"/, ""); print $3; exit}
' Cargo.lock)

fail=0
check() {
  # $1: label, $2: actual, $3: expected
  if [ "$2" != "$3" ]; then
    echo "error: $1 is $2, expected $3" >&2
    fail=1
  fi
}

if [ -z "${cargo_version}" ] || [ -z "${plugin_version}" ] || [ -z "${lock_version}" ]; then
  echo "error: could not read every version (Cargo.toml=${cargo_version:-?}, herdr-plugin.toml=${plugin_version:-?}, Cargo.lock=${lock_version:-?})" >&2
  exit 1
fi

check "herdr-plugin.toml version" "$plugin_version" "$cargo_version"
check "Cargo.lock version" "$lock_version" "$cargo_version"

if [ $# -ge 1 ]; then
  tag="${1#refs/tags/}"
  tag_version="${tag#v}"
  check "tag $1" "$tag_version" "$cargo_version"
fi

if [ "$fail" -ne 0 ]; then
  exit 1
fi

echo "version check passed: $cargo_version"
