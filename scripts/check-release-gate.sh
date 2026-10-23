#!/usr/bin/env bash
# Reject a release tag unless it names a tested main commit with matching notes.
# Annotated and lightweight tags are both accepted; the tag date is the tagger
# date of an annotated tag or the commit date of a lightweight tag.
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 || ! $1 =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo 'usage: scripts/check-release-gate.sh <vX.Y.Z> [changelog]' >&2
  exit 1
fi

tag=$1
changelog=${2:-CHANGELOG.md}
version=${tag#v}

fail() {
  echo "release gate: $*" >&2
  exit 1
}

git fetch --force --no-tags origin "refs/tags/$tag:refs/tags/$tag" || fail "cannot fetch $tag"
commit=$(git rev-parse --verify "refs/tags/$tag^{commit}") || fail "cannot resolve $tag"
checkout_commit=$(git rev-parse --verify HEAD) || fail 'cannot resolve checkout HEAD'
[[ $commit == "$checkout_commit" ]] || fail "$tag does not point to checkout HEAD"
git merge-base --is-ancestor "$commit" refs/remotes/origin/main || fail "$tag commit is not on origin/main"

tag_date=$(git for-each-ref --format='%(creatordate:short)' "refs/tags/$tag")
[[ $tag_date =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || fail "invalid tag date"
notes=$(scripts/extract-release-notes.sh "$version" "$changelog") || fail "missing release notes"
[[ ${notes%%$'\n'*} == "## $version ($tag_date)" ]] || fail "CHANGELOG date differs from tag date $tag_date"

[[ ${GITHUB_REPOSITORY:-} =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'GITHUB_REPOSITORY is missing or invalid'
gh_error_file=$(mktemp) || fail 'cannot prepare CI request'
trap 'rm -f -- "$gh_error_file"' EXIT
if ! runs=$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$commit&event=push&per_page=100" 2> "$gh_error_file"); then
  gh_error=$(< "$gh_error_file")
  if LC_ALL=C grep -Eiq 'HTTP 429|rate limit' <<< "$gh_error"; then
    fail 'CI workflow runs request was rate limited'
  elif LC_ALL=C grep -Eiq 'HTTP (401|403)|bad credentials|authentication required|resource not accessible by integration' <<< "$gh_error"; then
    fail 'CI workflow runs request was denied by authentication or permissions'
  elif LC_ALL=C grep -Eiq 'HTTP 404' <<< "$gh_error"; then
    fail 'CI workflow runs resource was not found'
  elif LC_ALL=C grep -Eiq 'HTTP 5[0-9][0-9]' <<< "$gh_error"; then
    fail 'CI workflow runs server error'
  elif LC_ALL=C grep -Eiq 'could not resolve host|no such host|failed to connect|could not connect|connection (refused|reset)|network is unreachable|temporary failure in name resolution|timed out|deadline exceeded|dial tcp|tls handshake' <<< "$gh_error"; then
    fail 'CI workflow runs connection failed'
  fi
  fail 'cannot read CI workflow runs'
fi
jq -e --arg sha "$commit" '
  [.workflow_runs[]? | select(.head_sha == $sha and .head_branch == "main" and .event == "push")]
  | if length == 0 then false else (max_by(.run_number) | .status == "completed" and .conclusion == "success") end
' <<< "$runs" > /dev/null || fail "CI is not green for $commit on main"

echo "release gate passed for $tag ($commit)"
