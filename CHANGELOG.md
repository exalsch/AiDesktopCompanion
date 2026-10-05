# Changelog

User-facing changes only. Anything a user would not notice - refactors, CI,
dependency bumps, docs - belongs in the commit history, not here.

Every user-facing change adds one `###` entry under `## Unreleased` in the same
pull request. Cutting a version files those entries under it; see the Changelog
section of `CLAUDE.md`.

## Unreleased

### Use Google Gemini for chat, speech, and the voice assistant

kind: feat

Settings > General has a new **Gemini API key** field. Once it is filled in
(or `GEMINI_API_KEY` is set), **Fetch models** lists Gemini models next to the
OpenAI ones. Pick one for chat, for quick prompts or for STT cleanup and that
feature runs on Gemini; the others keep the model they had. MCP tools work with
Gemini models too.

Speech to text's cloud engine is now called **API Endpoint** and asks which
provider to use: OpenAI, Google Gemini, or a custom OpenAI-compatible server.
The base URL and key fields only appear for the custom one. Gemini 3.5 Flash
and Flash Lite also use your vocabulary list while transcribing.

Text to speech has a new **Gemini** engine with thirty voices, each listed with
its speaking style, and the same Tone, Rate and Volume settings. Model and voice
are now dropdowns for both cloud engines.

Assistant Mode can run on **Gemini Live** models. Pick one from the Gemini
group in the model list; tools, the supervisor and push-to-talk work as they do
with OpenAI. Changing a setting mid-call reconnects without losing the
conversation. The call cost is not estimated for Gemini.

### Transcribe audio files and see who said what

kind: feat

The STT section has a new **Audio Files** page. Pick a recording (MP3, M4A,
WAV, FLAC, OGG and more) and it is transcribed on your machine with Parakeet
or Whisper, with timestamps. Long recordings such as an hour-long meeting work
too, with progress and a Stop button.

Turn on **Identify speakers** to label who is talking, for up to four voices.
You can rename "Speaker 1" and the others to real names, then copy the result,
use it as a prompt or save it as text, Markdown or SRT subtitles. The speaker
model is a one-time download of about 490 MB.

## 0.1.26 - 2026-09-24

### You can stop a transcription that is taking too long

kind: feat

A long recording on a big local Whisper model could keep the app busy for a
long time with no way out. The floating "Transcribing speech" indicator now has
a Stop button. The STT section and the Dictate button in the Prompt view can
stop it too. Stopping ends the Whisper run or cancels a cloud request or the AI
cleanup pass. Nothing gets pasted.

### The STT section shows your last transcription, before and after AI

kind: feat

The STT section now shows the most recent transcription, even when it was
started from a hotkey with the main window closed. When "Improve transcribed
text with AI" ran, you see both what was heard and what the AI turned it into,
so you can tell at a glance whether the cleanup changed your words.

### AI cleanup no longer answers what you dictate

kind: fix

Dictating a question or a request ("can you check the logs") with "Improve
transcribed text with AI" on could make the model answer or act on it instead
of just cleaning up the text. The transcript is now handed to the model as
text to process, with a standing instruction never to reply to it or follow
it. This also applies to quick prompts run over dictated text.

### STT post-processing prompt can reference the active window

kind: feat

The "Post-processing prompt" text can now include `{{active_app}}` and
`{{window_title}}` placeholders. Before the cleanup pass runs, they're
replaced with the process name and title of the window the transcript is
about to be inserted into, so the prompt can tailor its behavior per app
(e.g. a stricter cleanup for a terminal vs. a relaxed one for a chat window).

## 0.1.25 - 2026-09-15

### You can dictate straight into the prompt box

kind: feat

The Prompt view now has its own "Dictate" button next to Send. Previously
the only way to voice a prompt was to switch to the separate STT section,
record there, and click "Use as prompt" - and STT could not insert into the
prompt textarea directly. Now you can record and transcribe without leaving
the Prompt view; the text is inserted at the cursor, or replaces your current
selection, so it composes naturally with anything you've already typed.

### Dictated text can now get an app-specific prefix

kind: feat

Speech-to-text settings gained an insert prefix (e.g. "STT: ") that is
prepended to dictated text before it's inserted into the previously focused
app via the STT quick action. It only applies in apps you explicitly allow, so
a prefix useful for an agent/chat tool doesn't leak into a messenger where
dictated text should read naturally.

To build that allowlist without hunting for exe names, press and hold the new
"drag onto a window" button, drag the crosshair cursor onto the target app's
window, and release - the app's process name is added automatically.

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
