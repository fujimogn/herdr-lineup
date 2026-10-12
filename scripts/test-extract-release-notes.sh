#!/usr/bin/env bash
# Verify release note selection, failures, boundaries, and blank-line trimming.
set -euo pipefail

script_dir=$(cd "$(dirname "$0")" && pwd)
fixture_dir=$(mktemp -d)
trap 'rm -f "$fixture_dir/changelog" "$fixture_dir/expected" "$fixture_dir/actual" "$fixture_dir/error" "$fixture_dir/CHANGELOG.md"; rmdir "$fixture_dir"' EXIT

check_output() {
  "$script_dir/extract-release-notes.sh" "$1" "$fixture_dir/changelog" > "$fixture_dir/actual"
  diff -u "$fixture_dir/expected" "$fixture_dir/actual"
}

check_failure() {
  if "$script_dir/extract-release-notes.sh" "$1" "$fixture_dir/changelog" > "$fixture_dir/actual" 2> "$fixture_dir/error"; then
    echo "error: extraction unexpectedly succeeded for $1" >&2
    exit 1
  fi
  test ! -s "$fixture_dir/actual"
}

cat > "$fixture_dir/changelog" <<'CHANGELOG'
# Changelog

## Unreleased

Future changes.

## 1.2.3 (2026-10-13)

Release summary.

- Fix: **Release notes** retain their heading.
CHANGELOG
cat > "$fixture_dir/expected" <<'NOTES'
## 1.2.3 (2026-10-13)

Release summary.

- Fix: **Release notes** retain their heading.
NOTES
check_output 1.2.3
# The omitted path defaults to CHANGELOG.md in the caller working directory.
(cd "$fixture_dir"; cp changelog CHANGELOG.md; "$script_dir/extract-release-notes.sh" 1.2.3 > actual; diff -u expected actual; rm CHANGELOG.md)
check_failure 9.9.9

cat > "$fixture_dir/changelog" <<'CHANGELOG'
# Changelog

## Unreleased

No released versions yet.
CHANGELOG
check_failure 1.2.3

cat > "$fixture_dir/changelog" <<'CHANGELOG'


## 11.2.3 (2026-10-13)
Wrong version.
## 1.2.3 (2026-10-13)

Release summary.

- Fix: **Release notes** retain their heading.

   

## 1.2.2 (2026-10-12)
Previous release.
CHANGELOG
check_output 1.2.3
# Also trim trailing blank lines when the selected section reaches EOF.
printf '\n\n   \n' >> "$fixture_dir/expected"
cp "$fixture_dir/expected" "$fixture_dir/changelog"
# Restore the expected output without its trailing blanks.
cat > "$fixture_dir/expected" <<'NOTES'
## 1.2.3 (2026-10-13)

Release summary.

- Fix: **Release notes** retain their heading.
NOTES
check_output 1.2.3

for heading in '## 1.2.3 (2026-1-13)' '## 1.2.3 (2026/10/13)' '## 1.2.3' '## [1.2.3] - 2026-10-13'; do
  printf '%s\n\nInvalid heading.\n' "$heading" > "$fixture_dir/changelog"
  check_failure 1.2.3
done
check_failure '1x2x3'
check_failure 'v1.2.3'

# Lower-level headings belong to the selected release.
cat > "$fixture_dir/changelog" <<'CHANGELOG'
## 1.2.3 (2026-10-13)

Release summary.

### Fixes

- Fix: **Release notes** retain subheadings.

## Unreleased
Future changes.
CHANGELOG
cat > "$fixture_dir/expected" <<'NOTES'
## 1.2.3 (2026-10-13)

Release summary.

### Fixes

- Fix: **Release notes** retain subheadings.
NOTES
check_output 1.2.3

# Empty bodies fail closed, including bodies containing only whitespace.
printf '## 1.2.3 (2026-10-13)\n' > "$fixture_dir/changelog"
check_failure 1.2.3
printf '\n   \n\n## 1.2.2 (2026-10-12)\nOlder release.\n' >> "$fixture_dir/changelog"
check_failure 1.2.3

rm "$fixture_dir/changelog"
check_failure 1.2.3
test -s "$fixture_dir/error"

echo 'release notes extraction tests passed'
