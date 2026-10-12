#!/usr/bin/env bash
# Print one released CHANGELOG section, including its dated heading.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 || ! $1 =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo 'usage: scripts/extract-release-notes.sh <X.Y.Z> [changelog]' >&2
  exit 1
fi

awk -v version="$1" '
  /^## / {
    if (found) exit
    prefix = "## " version " ("
    if (index($0, prefix) == 1 &&
        substr($0, length(prefix) + 1) ~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\)$/) {
      found = 1
    }
  }
  found {
    lines[++count] = $0
    if ($0 !~ /^[[:space:]]*$/) last = count
  }
  END {
    if (!found || last <= 1) exit 1
    for (i = 1; i <= last; i++) print lines[i]
  }
' "${2:-CHANGELOG.md}" || {
  echo "error: could not extract dated CHANGELOG section for $1" >&2
  exit 1
}
