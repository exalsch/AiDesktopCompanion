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
# so this only has to read the type back out.
type="$(printf '%s' "${title}" | sed -nE 's/^([a-z]+)(\([^)]*\))?!?:.*/\1/p')"

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
pending="$(awk '
  /^## / { if (seen) exit; if ($0 ~ /^## +Unreleased/) { seen = 1 }; next }
  seen && /^### / { print }
' CHANGELOG.md)"

if [ -z "${pending}" ]; then
  echo "CHANGELOG.md was changed but '## Unreleased' has no '### ' entry." >&2
  exit 1
fi

echo "Found $(printf '%s\n' "${pending}" | wc -l | tr -d ' ') pending changelog entr(y/ies)."
