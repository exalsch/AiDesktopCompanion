# Changelog

User-facing changes only. Anything a user would not notice - refactors, CI,
dependency bumps, docs - belongs in the commit history, not here.

Every user-facing change adds one `###` entry under `## Unreleased` in the same
pull request. Cutting a version files those entries under it; see the Changelog
section of `CLAUDE.md`.

## Unreleased

### The app tells you what changed after an update

kind: feat

After installing a new version, a What's New window lists what changed since the
version you were running. Each item shows a one-line headline; click it to read
the detail. You can reopen it any time from the version number at the bottom of
the sidebar.

## 0.1.23 - 2026-09-07

### The app re-checks for updates when you come back to it

kind: fix

The update check used to run once at startup, so a build left running in the
tray for days never noticed a new release. It now runs again whenever the main
window comes back to the front. The check still honours its six-hour cache, so
switching windows repeatedly costs nothing.

## 0.1.22 - 2026-09-05

### Assistant Mode keeps a history of your calls

kind: feat

Every Assistant Mode call is now recorded with its duration, model, voice,
token counts, estimated cost and transcript. Open it from the Assistant
section. Calls you pin are kept indefinitely; the rest are swept once they pass
the retention window.
