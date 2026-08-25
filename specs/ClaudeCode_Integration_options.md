# Claude Code integration - options and recommendation

Status: **design note, nothing implemented.** Written in response to a request from the
Claude Code session running in `C:\Rob`, which asked for a read-only assessment of how
AiDesktopCompanion could become the voice front-end for a live Claude Code session.

Goal, in Alex's words: *"a real assistant call ongoing, so to speak, and I can communicate
with it"* - a persistent, voice-first channel where the companion is the microphone,
speaker and desktop UI, and Claude Code is the brain holding the real tools (Outlook/Graph,
ClickUp, Teams, the work-brain knowledge base, file index, browser automation, the M3 MCP
servers). Push in both directions: Alex talks to it, and it can speak up on its own when a
background job finishes.

---

## 1. Correcting the picture of this app

The externally-supplied summary was accurate as far as it went, but it missed the single
most important thing in the codebase for this purpose, and got two details wrong.

### 1.1 What was missed: Assistant Mode is already a full-duplex realtime voice call

`app/src/composables/useAssistantRealtime.ts` (773 lines) plus
`app/src/components/assistant/AssistantMode.vue` (793 lines) implement a **WebRTC session
against the OpenAI Realtime API**. This is not "the assistant chat panel". It is a live
phone call with the model. Concretely, all of the following already work:

| Capability | Where |
|---|---|
| WebRTC peer connection, bidirectional audio, ephemeral-token auth | `useAssistantRealtime.ts` `connect()`; token minted in Rust by `realtime_create_ephemeral_token` so the real API key never enters the WebView |
| Server-side VAD / endpointing, configurable `silence_duration_ms`, `idle_timeout_ms` (API cap 30 s), optional input noise reduction | `sendSessionUpdate()` |
| Barge-in | native to Realtime over WebRTC; nothing in this app suppresses it |
| Open-mic **and** push-to-talk, with a dedicated global hotkey slot (`push_to_talk_hotkey`, down/up events, 60 s max hold as a dropped-key-up guard) | `app/src/hotkeys.ts`, `startTalking()` / `stopTalking()` |
| Music pause while the mic is open, via WinRT media transport controls | `app/src-tauri/src/media.rs`, `media_hold` / `media_release` |
| Always-on-top, non-focus-stealing "call pill" with armed/live states, elapsed timer and a hang-up button, stacking above the busy pill | `app/src-tauri/src/assistant_pill.rs`, `AssistantPill.vue`, window `assistant-pill` declared statically in `tauri.conf.json` |
| Auto-close after N minutes of inactivity (realtime bills by the minute) | `resetAutoClose()` |
| Live rolling transcript of both sides, rendered in the panel | `history` ref |
| MCP tools exposed **to the realtime model itself** | `realtime_build_tools` flattens `build_openai_tools_from_mcp` output into realtime function-tool shape; `realtime_call_tool` executes them |
| Insert the assistant's answer into the previously focused app | `AssistantMode.vue` -> `refocus_previous_app` + `insert_text_into_focused_app` |

### 1.2 What was missed: there is already a "supervisor" escalation seam

This is the finding that should drive the whole design.

The realtime model is fast and cheap but weak. So the code gives it a tool called
`consult_supervisor`, described as *"Ask the supervisor model, which is slower but far more
capable and has access to tools, files and the internet."* When the realtime model calls it:

```
realtime model calls consult_supervisor(question)
  -> askSupervisor()  (useAssistantRealtime.ts:201)
     -> invoke('chat_complete', { messages })          // last 20 turns + the question
        -> chat::chat_complete_with_mcp()              // OpenAI chat completions + MCP tool loop
  -> answer text comes back
  -> response.create with "Say the following out loud, word for word"
  -> the realtime voice reads the supervisor's answer verbatim
```

Two modes, persisted as `assistant_realtime.supervisor_mode`:

- **`always`** - the realtime model is held silent (`create_response: false`) and the
  supervisor answers *every* user turn.
- **`needed`** - the realtime model handles what it can and escalates on its own.

**This is a text-in / text-out hole in the middle of a live, working voice call.** It is
already wired to the microphone, the speaker, the transcript, the turn-taking and the
conversation history. Anything that can answer a string can be the brain of Alex's ongoing
call. That is the integration point, and everything below is really about the cheapest way
to put Claude Code behind it.

#### 1.2.1 Which tools the realtime model actually receives

This gate is easy to get wrong and it decides whether an MCP server is visible to the voice
at all. From `useAssistantRealtime.ts:563-566`:

```js
const supervisorNeeded = params.useSupervisor === true && currentSupervisorMode === 'needed'
const includeTools     = params.enableTools === true && params.useSupervisor !== true
let toolsToSend        = includeTools ? tools : []
if (supervisorNeeded) toolsToSend = [SUPERVISOR_TOOL]
```

| "Enable MCP tools" | "Use supervisor agent" | `supervisor_mode` | Realtime model receives |
|---|---|---|---|
| off | off | - | **nothing** (this is the default state) |
| **on** | **off** | - | **all MCP tools**, named `mcp__<server_id>__<tool>` |
| any | on | `always` | nothing - it is only a voice; the supervisor answers every turn |
| any | on | `needed` | **only** `consult_supervisor` |

The consequence that matters: **MCP tools and the supervisor are mutually exclusive for the
realtime model.** Turning the supervisor on does not give the voice model MCP tools plus an
escalation path - it *replaces* the MCP tools with a single `consult_supervisor` tool. The
MCP tools are still reachable, but one layer down, because the supervisor itself runs through
`chat_complete_with_mcp`, which builds its tool list from every connected MCP client.

So there are two distinct ways to reach a Claude Code shim, and they are not the same thing:

- **Direct:** "Enable MCP tools" on, supervisor off. The realtime model calls
  `mcp__Rob__ask_claude` itself. Fewer hops, lower latency, but the realtime model is the one
  deciding when and with what arguments - and it is the weakest model in the chain.
- **Via the supervisor:** supervisor on, mode `needed`. The realtime model can only call
  `consult_supervisor`; GPT then decides whether to call `mcp__Rob__ask_claude`. One extra
  hop and one extra model's latency, but a much better tool-caller, and it sees the last 20
  turns of transcript.

Two further gotchas, both of which will present as "the assistant says it has no such tool":

- **Neither toggle is persisted.** `AssistantMode.vue:22-23` default both to `false`, and the
  `assistant_realtime` save block writes 11 fields but not these two. They reset on every app
  start. (Worth fixing; see §7.14.)
- **Tools are fixed at `session.update` time.** Only `enableTools`, `useSupervisor` and
  `supervisorMode` have watchers that call `syncSession`. Connecting an MCP server *during* a
  live call does not re-push the tool list; the call has to be restarted, or a toggle flipped.

The header badge **"Tools N"** in the Assistant Mode panel renders `statusRef.toolsCount` and
is the correct diagnostic for all of the above.

### 1.3 What was missed: an in-process loopback HTTP server already exists

`app/src-tauri/src/tts_streaming_server.rs` runs a **hyper 1.x HTTP/1 server bound to
`127.0.0.1`** inside the Tauri process, with its own accept loop, a session map and an idle
reaper. It exists to stream OpenAI TTS audio to the WebView (which is why `media-src` in the
CSP allows `http://127.0.0.1`).

So "run a loopback HTTP bridge in the Tauri process" is not new ground here - the
dependencies (`hyper`, `hyper-util`, `http-body-util`, `bytes`) are already in `Cargo.toml`
and there is a working, tested pattern to copy. Two caveats: it binds `127.0.0.1:0`
(**ephemeral port**, so a bridge needs a discovery file or a fixed port), and it is started
lazily by the OpenAI TTS path rather than at app startup.

### 1.4 What was missed: Alex already has a working "Rob, ..." pipeline, and it is bad

`%APPDATA%\AiDesktopCompanion\hooks\command.ps1` exists on this machine (5.6 KB, last
modified 2026-05-26). It:

1. matches `^Rob\b...` on the transcript,
2. finds a Windows Terminal tab whose title contains `ROB` via UI Automation, launching
   `wt -d C:\rob --title ROB claude` and sleeping 3 s if there is none,
3. selects the tab, `SetForegroundWindow`s it, walks the UIA tree to focus the `TermControl`
   pane,
4. `Set-Clipboard` + `SendKeys ^v` + `SendKeys {ENTER}`.

It works, and it is exactly the thing to replace. Its problems are the requirements list for
this project: it **steals focus** mid-sentence, it **clobbers the clipboard**, it depends on
UI Automation against Windows Terminal's control tree, there is **no answer channel at all**
(Alex has to look at the terminal), and nothing is ever spoken back.

### 1.5 Two details to correct

- **MCP transport is stdio or *streamable HTTP*, not SSE.** `rmcp` was moved to 3.1 and the
  standalone SSE client transport was dropped; the frontend normalises a server marked `sse`
  to `http` before it reaches Rust. See the comment in `Cargo.toml` above the `rmcp` line.
- **`rmcp` is compiled with the `client` feature only.** There is no MCP *server* capability
  in the binary today. Adding one is a real change, not a flag flip - see option B.

### 1.6 Everything else in the summary was right

Single `index.html` with `?window=` dispatch (now five windows: `main`, `quick-actions`,
`capture-overlay`, `busy-indicator`, `assistant-pill`); Quick Actions popup with aggressive
clipboard copy/restore; local Whisper/Parakeet plus cloud STT; Windows-native and OpenAI TTS;
`chat.rs` MCP tool loop; `mcp.rs` rmcp client with a global `MCP_CLIENTS` map; Command Mode
hook; locked-down CSP.

---

## 2. Facts that constrain every option below

These are the load-bearing details. Read this section before judging the options.

**Command Mode is fire-and-forget. There is no return channel.**
`command_hook.rs:436-438` sets `stdout` and `stderr` to the *log file*, not a pipe. The exit
code is logged and thrown away. The app never reads a byte back from the script. Contract:

- **In:** transcript on stdin, plus env `AIDC_TRANSCRIPT`, `AIDC_ACTIVE_APP` (foreground
  process name at trigger time), `AIDC_CLIPBOARD`, `AIDC_SELECTED_TEXT`. Empty values are
  passed as empty strings, never absent.
- **Out:** nothing. Whatever the hook wants to happen, the hook must make happen itself.
- **Concurrency:** `COMMAND_RUNNING: AtomicBool` single-flight. A second `C` while one runs
  is a silent no-op.
- **Timeout:** `command_hook_timeout_secs`, default **120 s** (settable, clamped to 5-3600 s
  in `config.rs`), then `child.kill()`. Children
  the script itself spawned are *not* reaped - detaching long work with `Start-Process` is
  explicitly the script author's job.
- **Events:** `command:state` (`running` / `idle`) and `command:error` are emitted to the
  frontend.

**`chat_complete` hardcodes `https://api.openai.com/v1/chat/completions`.**
Both `chat::chat_complete_with_mcp` (line 115) and `chat::chat_complete` (line 316) have the
URL inline. Unlike STT and TTS, the Prompt/supervisor path has **no configurable base URL**.
Pointing the supervisor at a local endpoint therefore needs a Rust change - small, but not
zero.

**CSP allows loopback HTTP but not loopback WebSocket.**
`connect-src` includes `http://127.0.0.1` with no port restriction, so `fetch` and
`EventSource` against any loopback port work from the WebView today. `ws://127.0.0.1` is
**not** listed, and CSP does not infer it from the `http://` entry. A WebSocket push channel
requires editing both `csp` and `devCsp` in `tauri.conf.json`. **Prefer SSE + POST.**

**The realtime call lives in the main window's WebView.**
Hiding the main window to tray keeps it alive. Killing the WebView2 process does not.
`webview_health.rs` exists precisely because WebView2 process failures are a live issue
(#14). An always-on call turns a rare annoyance into a visible one.

**Assistant Mode conversation state is in-memory and wiped on connect.**
`history.value = []` at the top of `connect()`. Only the last 20 turns go to the supervisor,
tool turns filtered out. Assistant Mode does **not** use `conversation_persist`. Hang up and
the conversation is gone.

**No single-instance guard.** `tauri-plugin-single-instance` is not a dependency. Two copies
of the app would both try to bind a fixed bridge port.

**Every new frontend capability needs a line in `generate_handler!`** in `lib.rs:91-172`.
That list is the entire Rust/JS contract.

**The Quick Actions popup must never be created or destroyed at runtime** - `popup.ts` only
toggles visibility on the statically declared window. Any new agent-facing UI must follow the
same rule.

---

## 3. The options

### A. Companion as MCP client of Claude Code

**Can it work?** Not directly. Claude Code is an MCP *client*; it exposes no MCP server
endpoint to connect to. But the question is the wrong shape, because Claude Code does not
need to *be* an MCP server - something on the Rob side can be one on its behalf.

**A1 - a Rob-side stdio MCP shim.** A small server (Node, ~150 lines) that Claude Code's
ecosystem owns, exposing tools like:

```
ask_claude(question, session?) -> string
claude_sessions()              -> [{name, cwd, status}]
send_to_session(name, text)    -> ack
job_status(job_id)             -> {state, result?}
```

Internally it does whatever is most reliable on that side - `claude -p` for one-shots, or a
directed message to a named `claude --bg` session for stateful work.

The companion connects to it with the **MCP wiring that already exists**: Settings -> MCP
Servers -> stdio, `command: node`, `args: [C:\Rob\...\claude-bridge.mjs]`. Nothing in this
repository changes.

And because `realtime_build_tools` flattens every connected MCP server's tools into the
realtime session, **the tool appears to the live voice call as well as to the Prompt panel**
- one shim lights up both surfaces. But only under the conditions in §1.2.1: "Enable MCP
tools" on, "Use supervisor agent" off, the server showing `connected`, and the call started
after the server connected. With the default toggles the realtime model receives an empty
tool list and will simply say it has no such tool.

- **Effort here: zero.** All of it lands on the Rob side, roughly half a day.
- **Risk: latency inside a synchronous tool call.** A `claude -p` that takes 40 s blocks the
  realtime turn with no audio at all. Two mitigations, ideally both: (a) make the blocking
  tool fast-path only and add an async `claude_start` / `claude_poll` pair for real work;
  (b) have the realtime model say a filler line before calling. Also unverified: rmcp 3.1's
  default client request timeout, which may cut off a long tool call before Claude Code
  finishes. Test a deliberately slow tool early.
- **Risk: transcription quality.** `DEFAULT_TRANSCRIPTION_MODEL = 'gpt-4o-transcribe'` feeds
  the tool arguments. Project names and file paths spoken aloud will arrive mangled.

**A2 - same shim, reached through the supervisor instead.** Note that `consult_supervisor`
cannot be swapped for `ask_claude`: in `needed` mode `toolsToSend` is hardcoded to
`[SUPERVISOR_TOOL]` (§1.2.1). The escalation path is therefore two hops, not one - the
realtime model calls `consult_supervisor`, and GPT (running `chat_complete_with_mcp`, which
sees every connected MCP server) decides whether to call `mcp__Rob__ask_claude`. Steer that
decision from the **Prompt-section system prompt**, not from the Assistant Mode instructions,
since it is GPT making the call.

Both A1-direct and A2-via-supervisor produce "an ongoing call with Claude Code" out of parts
that already exist. A2 costs one extra model round-trip but gets a far better tool-caller,
and the supervisor already receives the last 20 turns of transcript. Try A1-direct first
because it is one less moving part, and fall back to A2 if the realtime model calls the tool
badly - which, given it is the weakest model in the chain and its arguments come from
speech-to-text, is likely.

### B. Companion exposes an MCP server that Claude Code connects to

This is the push direction, and the peer session is right that it is the compelling one:
`speak(text)`, `notify(text)`, `ask_user(question)`, `insert_into_focused_app(text)`,
`capture_selection()`, `capture_screen()`, `say_in_call(text)`. It turns this app into Claude
Code's eyes, ears and mouth.

**B1 - in-process rmcp server over streamable HTTP.** Real work:

- add `rmcp` server features and implement a `ServerHandler`;
- rmcp's streamable-HTTP server transport is **axum-based**, so axum joins hyper in the
  binary (or the MCP HTTP framing gets hand-rolled on the existing hyper server, which is
  worse);
- a **fixed, discoverable port** is required, because Claude Code's `.mcp.json` needs a
  stable URL - which means port-conflict handling and a single-instance guard;
- authentication is **not optional**: any local process could otherwise type into Alex's
  desktop and read his screen.

Estimate: 1-2 days for a first cut, plus hardening. Stdio is not an option - this is a GUI
process, its stdin/stdout are not a pipe to Claude Code.

**B2 - loopback REST + a thin external MCP shim. Recommended over B1.**

The companion exposes a small plain-JSON surface on its own hyper server; a tiny stdio MCP
server on the Rob side translates MCP calls into HTTP calls against it. Claude Code spawns
the shim.

Why this is better:

- no `rmcp` server feature, no axum, no MCP protocol code in Rust;
- the tool list can be iterated on the Rob side without touching or releasing this app;
- the same REST surface serves option D unchanged;
- the code to copy already exists in `tts_streaming_server.rs`.

Sketch:

```
POST /speak            {text, voice?}       -> tts_start / tts_openai_stream_start
POST /notify           {text, kind}         -> busy pill, or a toast
POST /say-in-call      {text}               -> emits a Tauri event; AssistantMode injects it
                                              into the live realtime session
POST /ask              {question, timeout}  -> blocks, returns Alex's spoken/typed answer
POST /insert           {text}               -> insert_text_into_focused_app  (gated, see §5)
GET  /selection                             -> last_selected_text
POST /capture                               -> region capture, returns a path
GET  /state                                 -> {call: idle|live, busy: n, focused_app}
GET  /events           (SSE)                -> user speech, hang-up, answers to /ask
```

Discovery and auth: write `%APPDATA%\AiDesktopCompanion\bridge.json` with `{port, token}` at
startup, `0600`-equivalent ACL; bind `127.0.0.1` only; require the bearer token on every
call; send **no** CORS headers and reject requests carrying an `Origin` header, so a web page
cannot reach it. Off by default behind a settings toggle.

Estimate: **0.5-1 day** for the endpoint set and the settings toggle, on top of an existing,
tested server pattern.

### C. Companion drives the `claude` CLI as a subprocess

The companion owns a long-lived `claude` session directly, the way `rob-app` does.

**Verdict: wrong layer.** It duplicates something Rob already does well, and it drags CLI
concerns into a Tauri GUI: session resume, permission prompts, per-project working
directories, and the CLI's known traps (a newline in a positional prompt is silently
swallowed; `--allowedTools` eats the positional argument). The interesting sessions are
per-project and belong to whatever manages projects - that is Rob, not this app.

The only defensible narrow version is a stateless `claude -p` one-shot for "ask a question",
and that is just option A1's shim living in the wrong repository.

### D. Loopback HTTP bridge with SSE push

Not really a separate option - it is the transport underneath B2, and the answer to the push
direction. Worth stating explicitly:

- **Companion -> Claude Code:** `GET /events` as SSE. The Rob side holds it open and receives
  user speech, call state changes, and answers to pending `ask_user` calls.
- **Claude Code -> Companion:** plain `POST`s as above.
- Use **SSE, not WebSocket**, unless someone edits the CSP (§2). SSE also survives the
  Tauri/WebView boundary without ceremony, and the companion is not the client here anyway -
  the Rob-side shim is.

### E. Command Mode hook as the cheap first step

Real, and cheaper than anything else, but bounded by §2: **the hook has no return channel.**
Anything the user is meant to hear, the hook must speak itself.

Smallest useful version, replacing the current `command.ps1`:

```powershell
if ($transcript -notmatch '^(?i)Rob\b[\s,\.;:!?-]*(.+)$') { exit 0 }
$text = $Matches[1].Trim()

# Detach: the app kills this process after command_hook_timeout_secs (120 s default).
Start-Process -WindowStyle Hidden powershell -ArgumentList @('-NoProfile','-Command', @"
  `$answer = & claude -p '$($text -replace "'","''")' 2>&1 | Out-String
  Add-Type -AssemblyName System.Speech
  (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak(`$answer.Trim())
"@)
exit 0
```

Notes that matter:

- **`Start-Process` to detach is mandatory**, not stylistic. A synchronous `claude -p` that
  takes longer than 120 s gets killed mid-answer, and the single-flight `AtomicBool` blocks
  `C` until it finishes. Raising `command_hook_timeout_secs` (clamped to 3600 s) helps, but
  it also means `C` stays blocked for that whole time, so detaching is still the right shape.
- Speaking is done in the hook via `System.Speech`, or by calling Rob's existing local TTS
  MCP server (`mcp__tts__sapi_tts`). The companion's own TTS is *not* reachable from a script
  today - that is exactly what option B2's `POST /speak` would fix.
- No focus stealing, no clipboard clobber, no UI Automation. Strictly better than the current
  hook on all three counts.
- Ceiling: one-shot, no conversation, no interruption, no follow-up. It proves the wiring; it
  is not the "ongoing call".

### F. The option not on the list: make Claude Code the supervisor

Given §1.2, this is the shortest line between here and what Alex asked for.

**F1 - configuration only, zero code.** Register the A1 shim as an MCP server and pick one of
the two routings in §1.2.1: either "Enable MCP tools" on with the supervisor off, so the
realtime model calls `mcp__Rob__ask_claude` directly, or the supervisor on in `needed` mode,
so GPT calls it on the realtime model's behalf. Either way the realtime voice reads the
answer back and the call keeps running. **This is A1+A2 and it is the recommendation.**

**F2 - a settings-driven supervisor base URL.** Add `prompt_base_url` to settings, following
the `*_from_settings_or_env` pattern that STT and TTS already use, and let
`chat_complete_with_mcp` POST there instead of `api.openai.com`. Rob then serves an
OpenAI-compatible `POST /v1/chat/completions` backed by a resident Claude Code session, and
**every** supervisor turn is Claude Code, with the full conversation history the composable
already assembles.

- Cost here: **2-4 hours** (one setting, one URL, one settings-UI field). Genuinely small.
- Cost on the Rob side: a non-streaming, no-tool-calls completions shim is maybe 100 lines;
  supporting streaming and `tool_calls` properly is considerably more.
- Caveat: in `always` mode this puts Claude Code in the path of *every* utterance including
  "hello", which will feel terrible. Use `needed`, or the hybrid where GPT stays the
  supervisor and simply holds `ask_claude` as one more tool.

---

## 4. The "ongoing call" question, honestly assessed

| Requirement | State today | Work to close |
|---|---|---|
| Continuous open microphone | **Works.** `mic_mode: 'open'` | none |
| Push-to-talk and hold | **Works.** Global hotkey slot, down/up events, 60 s hold guard, media pause | none |
| VAD / endpointing | **Works.** Server-side turn detection, configurable silence, idle timeout to 30 s, optional noise reduction | none |
| Barge-in | **Works.** Native to Realtime/WebRTC | none |
| Streaming partial output to TTS | **Works for the realtime voice** (it is the audio stream). **Absent for supervisor answers** - `askSupervisor` awaits a complete string, then the model reads it back | **Moderate.** Chunk the supervisor answer and inject incremental `response.create` calls |
| Turn-taking while the brain is slow | **Missing.** A 40 s Claude Code answer is 40 s of dead air with no indication anything is happening | **Small for a filler line** ("say you are looking into it" before awaiting). **Moderate** for real progress interjections, which need the push channel |
| Conversation state across turns | **Partial.** In-memory, last 20 turns to the supervisor, wiped on every connect, never persisted | **Small** if Claude Code holds the state instead (a named `--bg` session) - which is an argument for F1. **Moderate** to persist Assistant Mode conversations locally |
| Wake word | **Does not exist.** No always-listening keyword spotter anywhere in the codebase | **New subsystem** (openWakeWord / Porcupine + a resident capture loop + a settings surface). Not a rewrite of the app, but genuinely new. PTT plus open-mic-with-idle-timeout is the pragmatic substitute and it is already built |
| Survives the window being hidden | **Works** hidden to tray. **Dies** if WebView2 dies (#14) | Watch `webview_health.rs`; an always-on call makes this failure mode routine |
| Cost | Realtime bills by the minute. `auto_close_minutes` exists for a reason | A literally always-on call is expensive; expect to keep auto-close |

**Nothing in this list is a rewrite.** The hard parts of a voice loop - WebRTC, VAD,
barge-in, PTT, the call pill, media ducking - are done. The two real gaps are **filling the
silence while a slow brain thinks** and **streaming a long answer instead of waiting for
it**, and both are contained inside `useAssistantRealtime.ts`.

---

## 5. The push direction: which surfaces are reusable

| Surface | Reusable? | Notes |
|---|---|---|
| **Spoken TTS, headless** | **As-is.** | `tts_start` -> `tts_win_native::local_tts_start` needs no window and steals no focus. `tts_openai_stream_start` gives the better cloud voice. Both only need an entry point Claude Code can reach - that is the whole of `POST /speak`. |
| **Busy-indicator pill** | **As-is.** | `busy::start` / `finish` / `with_indicator`. Always-on-top, `WS_EX_NOACTIVATE`, bottom-right of the monitor under the cursor, elapsed counter, error state that persists until clicked. Exactly right for "Claude Code is working on X". One deliberate behaviour to know: it stays hidden when the main window is already in front. |
| **Assistant call pill** | **Pattern reusable**, code is call-specific. | Separate always-on-top window, stacks above the busy pill, `armed`/`live` states with a hang-up button. A "Claude wants you" pill would clone this rather than extend it. |
| **Tray** | Exists, minimal. | `TrayIconBuilder` with Show/Exit and click-to-toggle. No balloon/notification API is used. |
| **Real Windows toast** | **Not available.** | `tauri-plugin-notification` is not a dependency. Adding it is easy but it is an addition, not a reuse. |
| **Interruptible prompt / `ask_user`** | **Does not exist.** | The Quick Actions window is the natural home - already static, always-on-top, non-focus-stealing, never destroyed - but it has no "question from an agent" mode. New UI plus a pending-question state machine. Moderate. |
| **Speaking into a live call** | **Cheap and the best of the lot.** | If a call is up, `conversation.item.create` + `response.create` on the existing data channel makes Claude Code's message something the assistant *says*, mid-conversation, in the same voice. A Tauri event listener in `AssistantMode.vue` plus a `POST /say-in-call` endpoint. Roughly 20 lines of frontend and one endpoint. |

---

## 6. Recommendation

### First path: A1 + F1 (the Rob-side MCP shim, consumed by the existing supervisor seam)

Because it costs **nothing in this repository**, it uses wiring that is already built and
tested, and it puts the real question - *what does the latency of a Claude Code answer feel
like inside a live voice call?* - in front of Alex before anyone writes Rust.

**Smallest end-to-end slice that proves it:**

1. Rob side: a stdio MCP server exposing exactly two tools - `ask_claude(question)`
   (routed to a named `claude --bg` session, returns text) and `claude_status()`.
2. Companion: Settings -> MCP Servers -> add it as a stdio server, and **connect it** (the
   chip must read `connected`; the tool list is built from live clients only).
3. Assistant Mode: tick **"Enable MCP tools"** and leave **"Use supervisor agent"** off.
   Confirm the header badge reads **"Tools N"** with N greater than zero *before* testing -
   both toggles default off and are not persisted, so this is step zero on every app start
   (§1.2.1).
4. Hold the push-to-talk key, say *"use ask_claude to tell me what is on my calendar
   tomorrow"*, and hear the answer in the realtime voice. Refer to the tool by its function,
   not by the server name: it is registered as `mcp__Rob__ask_claude`, and there is no tool
   called "Rob".

**Effort: about half a day, all of it on the Rob side.**

What this slice will expose, and what to watch for: the dead air while `ask_claude` runs;
whether rmcp's client request timeout cuts off a slow tool call; and how badly
`gpt-4o-transcribe` mangles project names and paths on the way in.

### Second path, once the first proves out: B2 + D (the loopback bridge)

This is where the push direction lives, and where this repository finally does change.

- New `bridge.rs`: hyper server on `127.0.0.1`, port and token written to
  `%APPDATA%\AiDesktopCompanion\bridge.json`, endpoints per §3 B2, off by default behind a
  settings toggle.
- Frontend: a listener in `AssistantMode.vue` for `bridge:say-in-call`, injecting into the
  live realtime session.
- Rob side: a stdio MCP shim translating MCP tool calls into HTTP calls against it.

**Effort: 1 to 1.5 days in this repository** (Rust plus a small frontend listener plus a
settings field), on top of an existing server pattern.

Start with `POST /speak`, `POST /notify` and `POST /say-in-call`. Those three are pure reuse
of `tts_start`, `busy::start` and the realtime data channel, they carry no security weight
beyond the token, and they deliver the entire "Claude Code speaks up when a background job
finishes" story on their own.

### Third, only if the first two land: polish

Supervisor filler line and answer streaming (§4); `ask_user` in the Quick Actions window
(§5); persisted Assistant Mode conversations; wake word.

### Explicitly not recommended

- **C** - the companion should not own Claude Code's lifecycle.
- **B1** - an in-process rmcp MCP server buys nothing over B2 and costs axum, a fixed port,
  a single-instance guard and MCP protocol code in Rust.
- **F2** as the *first* step - it is genuinely cheap here (2-4 hours) but it front-loads real
  work onto the Rob side (an OpenAI-compatible completions endpoint) to prove the same thing
  F1 proves for free. Revisit it if `ask_claude`-as-a-tool turns out to be too indirect.

---

## 7. Things that will fight this architecture

Flagged bluntly, because they will each cost an afternoon if discovered late.

1. **CSP allows `http://127.0.0.1` but not `ws://127.0.0.1`.** SSE and fetch are fine.
   A WebSocket needs edits to both `csp` and `devCsp` in `tauri.conf.json` or the request is
   silently blocked.
2. **The existing loopback server uses an ephemeral port** (`127.0.0.1:0`). A bridge needs a
   discovery file or a fixed configurable port.
3. **No single-instance guard.** Two copies of the app would fight over a fixed bridge port.
   `tauri-plugin-single-instance` if a fixed port is chosen.
4. **Security.** A loopback server that can speak, type into whatever window has focus, and
   screenshot the desktop is a local privilege surface. Bearer token, loopback bind, reject
   requests with an `Origin` header, no CORS headers, off by default. Not optional.
5. **`insert_text_into_focused_app` is genuinely dangerous to expose.** It sets the clipboard,
   sends Ctrl+V to whatever is focused *right now*, and restores the clipboard afterwards.
   Handing that to an agent means it can type into whatever Alex happens to be looking at,
   including a password field or a half-written mail. Gate it: confirmation, or only permit
   it while the popup or an active call is the current context. `refocus_previous_app` calls
   `SetForegroundWindow` on a stored HWND, with the same class of problem.
6. **The `generate_handler!` list in `lib.rs` is the whole Rust/JS contract.** Every new
   frontend-visible capability needs a line there.
7. **The Quick Actions window is never created or destroyed at runtime** - `popup.ts` only
   toggles visibility. Any `ask_user` UI must obey the same rule, on this window or a new
   statically declared one.
8. **`rmcp` is client-only** in this build. Anything needing an MCP *server* is a dependency
   and feature change, which is the main argument for the external-shim shape.
9. **`chat_complete` hardcodes the OpenAI URL** - there is no base-URL setting for the Prompt
   or supervisor path, unlike STT and TTS.
10. **Realtime limits.** `MAX_TOOL_ROUNDS = 6`; `idle_timeout_ms` is capped at 30 s by the
    API and a rejected `session.update` **discards every other setting with it**; the voice is
    baked into the ephemeral token and cannot change once audio has been produced.
11. **Command Mode single-flight plus a 120 s kill.** Any hook-based path must `Start-Process`
    to detach, or slow answers get killed and `C` stays blocked.
12. **WebView2 fragility (#14).** `webview_health.rs` exists because the WebView2 process
    fails in the wild. An always-on call makes that a routine failure rather than a rare one.
13. **Transcription quality feeds tool arguments.** Spoken project names, file paths and
    ticket ids will arrive mangled from `gpt-4o-transcribe`. Any tool taking an identifier
    needs fuzzy resolution on the Claude Code side.
14. **"Enable MCP tools" and "Use supervisor agent" are not persisted.** Every other Assistant
    Mode control is written to `assistant_realtime` on change; these two are not, so they
    reset to `false` on every app start and the voice session silently comes up with no tools.
    This is a small, self-contained fix (two fields in the save block at
    `AssistantMode.vue:421-437` and two in the load block above it) and it should be done
    before any of this is used in anger - otherwise the first symptom of every session is
    "the assistant says it has no such tool".
