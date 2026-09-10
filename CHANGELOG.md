# Changelog

User-facing changes only. Anything a user would not notice - refactors, CI,
dependency bumps, docs - belongs in the commit history, not here.

Every user-facing change adds one `###` entry under `## Unreleased` in the same
pull request. Cutting a version files those entries under it; see the Changelog
section of `CLAUDE.md`.

## Unreleased

## 0.1.24 - 2026-09-10

### You can now control how your microphone is cleaned up

kind: feat

Speech-to-text settings gained four microphone switches: echo cancellation,
noise suppression, automatic gain control and voice isolation. The first three
were always on, decided by the webview rather than by you, and now they are
visible and settable. Turning automatic gain control off is worth trying if
quiet speech comes back badly transcribed.

Voice isolation is the one that helps when something else on the machine is
making noise, because it is a Windows effect rather than an app one. It needs a
microphone that offers the effect, so on machines without one the setting says
so instead of pretending to work. It starts switched off.

The settings apply to Assistant Mode's microphone as well as to recording.

### The app recovers instead of going silent after a display change

kind: fix

Changing your monitor setup - unplugging displays, docking, or resuming from
hibernation - could kill the content of every window while the windows
themselves stayed open. The app looked like it was running, but the hotkeys did
nothing and neither the Assistant call pill nor the processing indicator ever
appeared again until you restarted it. The app now notices that failure and
reloads the affected windows, and the global hotkeys are re-registered when it
does, so everything works again on its own.

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
