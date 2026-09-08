#!/usr/bin/env bash
#
# Fail a user-facing pull request that did not add a changelog entry.
#
# Reads PR_TITLE, PR_LABELS (a JSON array of label names) and BASE_REF from the
# environment; the workflow supplies all three. Run it locally with, for example:
#
#   PR_TITLE='feat(app): thing' PR_LABELS='[]' BASE_REF=main bash scripts/changelog-check.sh

set -euo pipefail

title="${PR_TITLE:-}"
labels="${PR_LABELS:-[]}"
base="${BASE_REF:-main}"

# semantic-pr.yml has already forced the title into Conventional Commit shape,
# so this only has to read the type back out. Lowercase the type before the case
# statement as defense in depth: if the upstream type-case enforcement is not applied,
# we will still recognise feat, fix, perf in any case.
type="$(printf '%s' "${title}" | sed -nE 's/^([a-zA-Z]+)(\([^)]*\))?!?:.*/\1/p' | tr '[:upper:]' '[:lower:]')"

case "${type}" in
  feat|fix|perf) ;;
  *)
    echo "PR type '${type:-unrecognised}' is not user-facing. No changelog entry required."
    exit 0
    ;;
esac

if printf '%s' "${labels}" | grep -q '"skip-changelog"'; then
  echo "The skip-changelog label is set. Not requiring an entry."
  exit 0
fi

if ! git diff --name-only "origin/${base}...HEAD" | grep -qx 'CHANGELOG.md'; then
  cat >&2 <<EOF
This pull request is titled '${type}', so it changes something a user can see,
but it does not touch CHANGELOG.md.

Add one entry under '## Unreleased':

  ### A short sentence a user would recognise

  kind: ${type}

  What changed, and why it matters. Markdown, as long as it needs to be.

If the change really is invisible to users, apply the 'skip-changelog' label.
EOF
  exit 1
fi

# Touching the file is not enough: the entry has to be in the pending section.
# A fenced code block inside an entry's detail can legitimately contain a
# `### ` line (a worked markdown example, say), so fence state is tracked the
# same way parse.ts tracks it: a trimmed line starting with ``` or ~~~ toggles
# the fence, only a matching marker closes it, and no line inside a fence is
# read as a heading.
pending="$(awk '
  {
    trimmed = $0
    sub(/^[ \t]*/, "", trimmed)
    if (trimmed ~ /^(```|~~~)/) {
      marker = substr(trimmed, 1, 3)
      if (in_fence) {
        if (substr(trimmed, 1, length(fence_marker)) == fence_marker) { in_fence = 0; fence_marker = "" }
      } else {
        in_fence = 1; fence_marker = marker
      }
      next
    }
    if (in_fence) next
    if ($0 ~ /^## /) { if (seen) exit; if ($0 ~ /^## +Unreleased/) { seen = 1 }; next }
    if (seen && $0 ~ /^### /) print
  }
' CHANGELOG.md)"

if [ -z "${pending}" ]; then
  echo "CHANGELOG.md was changed but '## Unreleased' has no '### ' entry." >&2
  exit 1
fi

echo "Found $(printf '%s\n' "${pending}" | wc -l | tr -d ' ') pending changelog entr(y/ies)."
