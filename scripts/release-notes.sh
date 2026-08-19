#!/usr/bin/env bash
#
# Build the release notes for a tag from Conventional Commit subjects.
#
# Usage: scripts/release-notes.sh [tag]
#
# Prints markdown to stdout. When no tag is given the most recent one is used.
# The previous tag is resolved from the tagged commit's first parent, so the
# range is always "everything since the last release" even though the tag being
# released already points at HEAD - that is exactly what the previous
# conventional-changelog action got wrong, leaving every release body empty.

set -euo pipefail

cd "$(dirname "$0")/.."

current="${1:-}"
if [ -z "${current}" ] || ! git rev-parse -q --verify "refs/tags/${current}" >/dev/null; then
  # No tag given (or a branch name from a manual workflow_dispatch run): fall
  # back to the newest tag reachable from HEAD.
  current="$(git describe --tags --abbrev=0 2>/dev/null || true)"
fi

if [ -n "${current}" ]; then
  previous="$(git describe --tags --abbrev=0 "${current}^" 2>/dev/null || true)"
  range="${previous:+${previous}..}${current}"
else
  previous=""
  range="HEAD"
fi

# Conventional Commit types we group under each heading. Anything that matches
# none of them still gets listed under "Other changes" so nothing is dropped.
known_types='feat|fix|perf|docs|refactor|chore|build|ci|style|test|revert'

# Turn "feat(app): thing" into "- **app**: thing (abc1234)" and a scopeless
# "feat: thing" into "- thing (abc1234)".
prettify() {
  sed -E \
    -e "s/^- (${known_types})\(([^)]+)\)!?: /- **\2**: /" \
    -e "s/^- (${known_types})!?: /- /"
}

section() {
  local title="$1"
  shift
  local grep_args=()
  local type
  for type in "$@"; do
    grep_args+=(--grep="^${type}(\([^)]+\))?!?:")
  done

  local body
  body="$(git log --no-merges -E "${grep_args[@]}" --pretty=format:'- %s (%h)' "${range}" | prettify)"
  if [ -n "${body}" ]; then
    printf '### %s\n\n%s\n\n' "${title}" "${body}"
  fi
}

section 'Features' feat
section 'Bug Fixes' fix
section 'Performance' perf
section 'Documentation' docs
section 'Maintenance' chore refactor build ci style test revert

# Catch-all so a non-conventional subject is never silently swallowed.
other="$(git log --no-merges -E --invert-grep \
  --grep="^(${known_types})(\([^)]+\))?!?:" \
  --pretty=format:'- %s (%h)' "${range}")"
if [ -n "${other}" ]; then
  printf '### Other changes\n\n%s\n\n' "${other}"
fi

if [ -n "${previous}" ] && [ -n "${GITHUB_REPOSITORY:-}" ]; then
  printf '**Full changelog**: https://github.com/%s/compare/%s...%s\n' \
    "${GITHUB_REPOSITORY}" "${previous}" "${current}"
fi
