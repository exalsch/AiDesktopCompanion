# Changelog

User-facing changes only. Anything a user would not notice - refactors, CI,
dependency bumps, docs - belongs in the commit history, not here.

Every user-facing change adds one `###` entry under `## Unreleased` in the same
pull request. Cutting a version files those entries under it; see the Changelog
section of `CLAUDE.md`.

## Unreleased

### Slow tools no longer hold a voice call

kind: fix

A tool that takes longer than a few seconds (an MCP server that asks another
agent, searches, or runs a job) now carries on in the background. The
assistant says it is running and you can keep talking; when the result comes,
the assistant tells you. If you have already hung up, a desktop notification
shows the result and the next call picks it up. Before, the call waited for
the tool, and a slow answer (over three minutes for a "do I need to prepare"
check) could arrive after the call ended and was lost. Fast tools answer in
the same turn as before.

The result is spoken in the language you speak (German or English) rather
than in whatever language the voice model picks; Gemini answered in Spanish,
Italian and once in Chinese. While the tool works, its progress pill sits
above the call pill instead of behind it, and its result shows as a line in
the call transcript.

### The voice assistant looks before it answers and shows you what it did

kind: fix

In a voice call the assistant used to say it had checked your calendar when it
had not, guess where your mouse was, and ask Claude Code about the event on
your screen, which Claude Code cannot see. Now:

- It reads the screen first. "This event", "what am I hovering" and "where is
  my cursor" go to the pointer and focus tools, and the answer starts with the
  name it read. In the new Outlook, whose calendar the usual hit test cannot
  see into, the app finds the event under the mouse itself.
- It does not claim a check it did not make, names the tools it really used
  when you ask, looks again when you correct it, and says so when it has no
  tool for something.
- A call gets only the screen tools of windows-commander (context, pointer,
  focus, find, OCR, screenshot and the on-screen arrows). Mouse, keyboard,
  PowerShell and files stay with chat. Switch back under Assistant > Screen
  presence and Hermes > Voice tools.
- Every tool call shows as a line in the live transcript, briefly on the call
  pill, and in the saved call history and its copied text.

### The voice assistant knows what is on your screen and can point at it

kind: feat

With windows-commander connected and MCP tools on, each turn of a voice call
now starts with a few lines about your screen: the app and window in front,
its web address, the focused control, what is under the mouse and where you
were just before. "What does this button do?" or "reply to this mail" now
mean something to the assistant. The lines are text only, sent again only
when something changed, and windows on windows-commander's block list
(password managers, private browsing) show as "details withheld".

The assistant is also told it can show you things: it draws an arrow or a
frame on your screen and clears it when you move on. Switch the screen lines
off under Assistant > Screen presence and Hermes.

### The app keeps one shared windows-commander running for every client

kind: feat

At start-up the app now checks for the shared windows-commander instance and
starts it in the background when it is not running, without a console
window. Claude Code, Hermes and this app then share one activity timeline and
one set of on-screen arrows. Quitting the app leaves it running, because the
others still use it; Settings > MCP Servers > Screen layer shows whether it
runs, its version and address, and has a button to stop it.

Your existing windows-commander server entry moves to the shared instance
automatically. Its address and access token are read when it connects and are
never stored in the settings file. When the shared instance cannot be reached,
the entry falls back to starting windows-commander itself, as before.

### HTTP MCP servers can send their own headers

kind: feat

An HTTP server in Settings > MCP Servers now has a list of headers to send
with every request, for servers that need an API key or a bearer token. The
values are hidden until you choose to show them.

### Hand long jobs to Hermes from a voice call

kind: feat

Turn on "Delegate long jobs to Hermes" under Assistant > Screen presence and
Hermes, enter the address and API key of your Hermes API server, and press
Test connection. In a call, ask for something that takes a while - research,
work through your mail - and the assistant hands it to Hermes and keeps
talking. The pill in the corner shows that Hermes is working. When the job is
done, the assistant tells you the result; if no call is open, a notification
does, and the next call picks it up. When Hermes needs your approval, you get
a notification with what it wants to do; the app never approves anything for
you. Hermes receives what is on your screen as text, never a screenshot.

### Two more accurate Parakeet models for local speech-to-text

kind: feat

Settings > Speech-to-text > Parakeet now offers two fine-tunes of Parakeet V3
next to the original. Both understand the same 25 languages, German included,
and make fewer mistakes than V3:

- **Parakeet Ultra** (Moondream) has the largest gain, most of all in noisy
  rooms and in languages other than English.
- **Orukeet** (Oruk AI) is more accurate than V3 on most test sets.

Like V3, both are a download of about 670 MB and run at a similar speed.

Pick one and press Download; V3 stays the default. Local file transcription
uses whichever Parakeet model is selected. Model downloads are now checked
against a known checksum, so a broken download is reported instead of failing
later with an unclear error.

## 0.1.28 - 2026-10-18

### The assistant can look at screenshots from your MCP tools

kind: fix

When a tool such as windows-commander's `capture_screen` returns a screenshot,
the model now actually sees it. Ask "what is on my screen?" in a voice call or
in the Prompt section and it can read and describe it. Before, the picture was
passed on as a long block of encoded text that the model could not use, and a
large screenshot could break the request.

This works with OpenAI and Gemini models. The older
`gemini-2.5-flash-native-audio` voice model sometimes starts guessing before
the picture arrives and then corrects itself; the 3.x Live models wait for it.

## 0.1.27 - 2026-10-05

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
its speaking style, and the same Rate and Volume settings. Gemini has no Tone
setting, because its voices read any style instruction out loud. Model and
voice are now dropdowns for both cloud engines.

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
