#!/usr/bin/env bash
# Exercise the release gate without GitHub access or a real release tag.
set -euo pipefail

fixture_dir=$(mktemp -d)
mkdir "$fixture_dir/bin"
trap 'rm -f "$fixture_dir/bin/git" "$fixture_dir/bin/gh" "$fixture_dir/changelog" "$fixture_dir/output" "$fixture_dir/error"; rmdir "$fixture_dir/bin" "$fixture_dir"' EXIT

cat > "$fixture_dir/bin/git" <<'MOCK'
#!/usr/bin/env bash
case "$1" in
  cat-file) [[ ${TEST_TAG_TYPE:-tag} == tag ]] && echo tag || echo commit ;;
  rev-parse) echo 1111111111111111111111111111111111111111 ;;
  merge-base) [[ ${TEST_ON_MAIN:-yes} == yes ]] ;;
  for-each-ref) echo "${TEST_TAG_DATE:-2026-10-18}" ;;
  *) echo "unexpected git command: $*" >&2; exit 1 ;;
esac
MOCK
cat > "$fixture_dir/bin/gh" <<'MOCK'
#!/usr/bin/env bash
[[ $1 == api && $2 == *'/actions/workflows/ci.yml/runs?head_sha=1111111111111111111111111111111111111111&event=push&per_page=100' ]] || exit 1
printf '%s\n' "$TEST_CI_RUNS"
MOCK
chmod +x "$fixture_dir/bin/git" "$fixture_dir/bin/gh"

export PATH="$fixture_dir/bin:$PATH" GITHUB_REPOSITORY=example/herdr-lineup
export TEST_CI_RUNS='{"workflow_runs":[{"head_sha":"1111111111111111111111111111111111111111","head_branch":"main","event":"push","run_number":1,"status":"completed","conclusion":"success"}]}'
printf '## 1.2.3 (2026-10-18)\n\nRelease summary.\n' > "$fixture_dir/changelog"

gate() {
  scripts/check-release-gate.sh v1.2.3 "$fixture_dir/changelog" > "$fixture_dir/output" 2> "$fixture_dir/error"
}
failure() {
  if gate; then
    echo "error: gate unexpectedly passed: $1" >&2
    exit 1
  fi
  [[ $(< "$fixture_dir/error") == *"$1"* ]]
}

gate
[[ $(< "$fixture_dir/output") == *'release gate passed'* ]]

TEST_ON_MAIN=no failure 'not on origin/main'
unset TEST_ON_MAIN
TEST_TAG_DATE=2026-10-19 failure 'CHANGELOG date differs'
unset TEST_TAG_DATE
TEST_CI_RUNS='{"workflow_runs":[]}' failure 'CI is not green'
TEST_CI_RUNS='{"workflow_runs":[{"head_sha":"1111111111111111111111111111111111111111","head_branch":"main","event":"push","run_number":2,"status":"completed","conclusion":"failure"}]}' failure 'CI is not green'
TEST_CI_RUNS='{"workflow_runs":[{"head_sha":"1111111111111111111111111111111111111111","head_branch":"main","event":"push","run_number":1,"status":"completed","conclusion":"success"},{"head_sha":"1111111111111111111111111111111111111111","head_branch":"main","event":"push","run_number":2,"status":"completed","conclusion":"failure"}]}' failure 'CI is not green'
TEST_TAG_TYPE=commit failure 'must be an annotated tag'

echo 'release gate tests passed'
