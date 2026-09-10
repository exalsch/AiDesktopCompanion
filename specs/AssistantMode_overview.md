# Assistant Mode - high level overview

Assistant Mode is the app's live voice conversation: a full-duplex audio call
between the user and an OpenAI Realtime model, held over WebRTC, with the
connected MCP servers available to the model as tools.

It is meant to be used while working in another application, so almost
everything about it is built around the main window being hidden: a global
push-to-talk hotkey, a floating call pill, audible call tones, and a session
that closes itself when nobody is talking.

**Scope of this document.** The supervisor arrangement (routing turns through
the Prompt-section chat model instead of letting the realtime model answer) is
deliberately left out. Everything below describes the mode with the supervisor
switched off.

## Where the code lives

| Concern | File |
| --- | --- |
| Session logic, WebRTC, turn handling, tools | `app/src/composables/useAssistantRealtime.ts` |
| Panel UI, defaults, persistence, call recording | `app/src/components/assistant/AssistantMode.vue` |
| Token and cost accounting | `app/src/composables/useRealtimeUsage.ts` |
| Floating call pill (window + state) | `app/src-tauri/src/assistant_pill.rs`, `app/src/components/AssistantPill.vue` |
| Ephemeral token, tool definitions, tool execution | `app/src-tauri/src/lib.rs` (`realtime_*` commands) |
| Call history (SQLite) | `app/src-tauri/src/call_history.rs`, `app/src/composables/useCallHistory.ts` |
| Global hotkey registration | `app/src/hotkeys.ts` |

## Starting a call

Three ways in, all landing on the same `activate()`:

1. **The panel.** The Assistant section of the main window has a Start button.
2. **The push-to-talk hotkey.** Pressing it with no call running does not start
   one - it *arms*. The pill appears and invites a second press within
   `ARM_WINDOW_MS` (`6000`), after which the arming lapses. Starting a session
   opens a microphone and starts spending money, so it should not happen because
   a key was brushed.
3. **Quick Actions.** `assistant_action` in `quick_actions.rs` brings the main
   window forward and emits `assistant:open` with `autostart: true`. Unlike the
   other quick actions it captures no selection, since a voice session has
   nothing to do with whatever text is highlighted.

## Connection

1. The frontend asks Rust for an ephemeral client secret
   (`realtime_create_ephemeral_token` -> `POST /v1/realtime/client_secrets`).
   The real API key never reaches the WebView. Model and voice are fixed at mint
   time; everything else is configured after connecting.
2. An `RTCPeerConnection` is created, the microphone is captured, an `oai-events`
   data channel is opened, and the SDP offer is posted to
   `https://api.openai.com/v1/realtime/calls` with the ephemeral token as the
   bearer.
3. Once the data channel opens, a single `session.update` pushes the full
   session configuration: instructions, tools, turn detection, transcription,
   noise reduction and reasoning effort.

Session settings are applied live. Changing a control mid-call re-sends
`session.update` through `syncSession()`. Two exceptions: the **model** and the
**voice** are baked into the token and cannot change without restarting the
session.

## Defaults

| Setting | Default | Notes |
| --- | --- | --- |
| Model | `gpt-realtime-2` | The list is refreshed from `/v1/models` filtered to realtime ids; the hardcoded list is only a fallback. |
| Voice | `alloy` | Ten realtime voices; fixed for the life of a session. |
| Microphone mode | `open` (always listening) | Push-to-talk is opt-in because it needs a hotkey or a held button to be usable. |
| Silence before a turn ends | `2000` ms | Server VAD `silence_duration_ms`; open-mic only. |
| Idle timeout | off | Optional; clamped to the API ceiling of `30000` ms, because one oversized value fails the whole `session.update` and silently discards every other setting in it. |
| Noise reduction | on (`near_field`) | Suits a desktop or headset microphone. |
| Reasoning effort | model default | Only sent for `gpt-realtime-2*`; the other models reject the field outright. |
| Auto-close after inactivity | `2` minutes | An open microphone keeps feeding billable audio, so a forgotten window hangs up on itself. |
| Call tones | on | Ringback while connecting, beep when the call is up. The connect usually happens by hotkey with the window hidden, so sound is the only feedback that reaches the user. |
| Transcription model | `gpt-4o-transcribe` | Always on. The transcript feeds the history, the panel and the debug log. |
| Push-to-talk hotkey | unset | Configured in Settings -> General. |
| Call history | on, 30-day retention | See below. |

## Microphone modes

### Open

Server VAD decides where a turn ends: `server_vad` with a `0.5` threshold,
`300` ms prefix padding and the configured silence duration.
`interrupt_response` is on, so speaking over the assistant cuts it off. The
panel offers a Mute button.

### Push-to-talk

Server VAD is switched **off** (`turn_detection: null`) and the turn boundary
becomes the key release. This matters: a disabled WebRTC track keeps
transmitting silence, so leaving VAD on made every release wait out the full
silence timeout before the model would answer.

Holding the hotkey (or the panel's hold-to-talk button, or the pill's) does
four things: it interrupts whatever the assistant is currently saying, opens the
microphone, pauses the user's media playback (`media_hold`), and starts a hold
timer. Releasing commits the captured audio (`input_audio_buffer.commit`) and
asks for a response.

Guard rails:

- A hold shorter than `MIN_TALK_MS` (`200`) is discarded rather than committed.
  The API rejects a buffer holding under 100 ms of audio, so a stray tap would
  otherwise surface as an API error.
- A hold longer than `MAX_TALK_MS` (`60000`) closes the microphone and commits
  what it has. A dropped key-up would otherwise leave the microphone open
  indefinitely, which is the exact thing push-to-talk exists to prevent.

Muting does two things, and both are needed: `track.enabled = false` takes
effect immediately, and `replaceTrack(null)` actually detaches the track from
the sender so nothing leaves the machine. Realtime input is billed per audio
token, so a muted-but-attached microphone pays for its own silence. The track
itself is left running rather than stopped, because re-acquiring it on the next
key press would clip the start of what the user was already saying.

## Turns, responses and barge-in

- The API allows exactly one response at a time. `requestResponse()` defers a
  second one until `response.done` arrives, and keeps only the most recent
  deferred request.
- Barge-in cancels the response, clears the output audio buffer (the audio
  already pushed to the client keeps playing otherwise), and drops any deferred
  response. All of it has to happen together, or the interrupt is one the user
  can still hear.
- A response cut off mid-sentence is recorded in the transcript with an
  `(interrupted)` marker.
- The running transcript survives disconnect, so the last conversation is still
  readable after the call ends. Connecting clears it.

## Tool use

With tools enabled, the connected MCP servers are exposed to the realtime model
directly.

- `realtime_build_tools` (Rust) builds the definitions from the live MCP
  clients, applying the same server and tool filtering as the Prompt section. It
  reuses the chat-completions builder and flattens the result, because realtime
  takes `{type, name, description, parameters}` while chat-completions nests
  those under `function`. If the command fails, the frontend falls back to
  client-side discovery over `mcp_list_tools`.
- Tool names are mangled to `mcp__<serverId>__<tool>` so a call can be resolved
  back to a server.
- A `function_call` in a finished response is executed through
  `realtime_call_tool`, and the result is fed back as a `function_call_output`
  followed by one `response.create` for the whole batch. A failed tool still
  returns JSON with an `error` field: a turn with no output leaves the model
  waiting forever.
- `MAX_TOOL_ROUNDS` (`6`) caps how many rounds of call-and-answer a single
  conversation may run, mirroring the cap in `chat.rs` so a confused model cannot
  spin forever on a live microphone.
- The session instructions always end with a generated note describing the
  actual arrangement (tools or no tools). It is appended to any custom
  instructions rather than replacing them, because the switches are per-session
  and whoever wrote the prompt cannot know their state.

The tools badge in the panel is corrected from the server's own `session.updated`
echo, so it reports what the session accepted rather than what the app sent.

## Cost, usage and history

Realtime audio is the most expensive thing the app does, so a call reports what
it cost while it runs and after it ends.

- Every `response.done` carries a usage block split by modality and by cached
  versus fresh input. `useRealtimeUsage` accumulates it and estimates a dollar
  figure from a rate table keyed on model prefix. Tokens are exact; money is an
  estimate, and `RATES_CHECKED` says when the rates were last verified.
- A model with no known rates yields `null`, not zero: "we do not know" and "it
  was free" must not read the same.
- Finished calls are written to `calls.db` (SQLite, beside `settings.json`) with
  duration, model, voice, token counts, estimated cost, a generated title and the
  transcript. A session that connected but produced neither speech nor a billable
  response is not recorded. Calls can be pinned, and pinned calls survive the
  retention sweep (`keep_all`, `days_7`, `days_30`, `months_3`, `last_50`).
- Transcripts are stored as plain text, the same bargain `settings.json` already
  makes with the API key.

## The floating pill

A small always-on-top, non-activating window (`assistant-pill`, declared
statically in `tauri.conf.json`) that shows call state while the main window is
hidden: `armed`, `calling`, `live`, or hidden. It carries the elapsed counter, a
talking indicator, a hold-to-talk button in push-to-talk mode, and a hang-up
button. It runs in its own window and cannot reach the session, so it emits
`assistant:hangup`, `assistant:ptt-down` and `assistant:ptt-up`, which the panel
routes into the same handlers the global hotkey uses.

## Settings and persistence

Everything in the panel is written to the `assistant_realtime` blob inside
`%APPDATA%\AiDesktopCompanion\settings.json`, immediately on change. That blob is
passed through wholesale by `save_settings`, which is an allowlist: a new
top-level key that is not listed there is silently dropped on save. This is why
Assistant Mode settings, including the two read back by Rust
(`history_enabled` and `history_retention`), are nested inside it rather than
added at the top level.

## Known limits

- Windows-first, like the rest of the app. The pill, the media hold and the
  global hotkey all lean on Win32.
- The OS microphone indicator stays lit for the whole session in push-to-talk,
  because the track is kept alive for responsiveness. The UI says so rather than
  pretending otherwise.
- Model and voice cannot change mid-call.
- `connect-src` in the CSP only allows `self`, IPC, `https://api.openai.com` and
  `http://127.0.0.1`, so an OpenAI-compatible realtime endpoint on another host
  needs a CSP change before it can be reached.
