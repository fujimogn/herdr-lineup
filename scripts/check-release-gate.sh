#!/usr/bin/env bash
# Reject a release tag unless it names a tested main commit with matching notes.
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

[[ $(git cat-file -t "refs/tags/$tag" 2>/dev/null) == tag ]] || fail "$tag must be an annotated tag"
commit=$(git rev-parse --verify "refs/tags/$tag^{commit}") || fail "cannot resolve $tag"
git merge-base --is-ancestor "$commit" refs/remotes/origin/main || fail "$tag commit is not on origin/main"

tag_date=$(git for-each-ref --format='%(taggerdate:short)' "refs/tags/$tag")
[[ $tag_date =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] || fail "invalid tagger date"
notes=$(scripts/extract-release-notes.sh "$version" "$changelog") || fail "missing release notes"
[[ ${notes%%$'\n'*} == "## $version ($tag_date)" ]] || fail "CHANGELOG date differs from tagger date $tag_date"

[[ ${GITHUB_REPOSITORY:-} =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'GITHUB_REPOSITORY is missing or invalid'
runs=$(gh api "repos/$GITHUB_REPOSITORY/actions/workflows/ci.yml/runs?head_sha=$commit&event=push&per_page=100") || fail 'cannot read CI workflow runs'
jq -e --arg sha "$commit" '
  [.workflow_runs[]? | select(.head_sha == $sha and .head_branch == "main" and .event == "push")]
  | if length == 0 then false else (max_by(.run_number) | .status == "completed" and .conclusion == "success") end
' <<< "$runs" > /dev/null || fail "CI is not green for $commit on main"

echo "release gate passed for $tag ($commit)"
