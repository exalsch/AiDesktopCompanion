# Screen presence: voice, screen and long jobs

Status: implemented on `feat/screen-presence` (2026-11-11). Sections 5 to 9
(voice tool profile, voice rules, Outlook check, eval, tool trace) were added
the same day after a failed live test.

## The three parts

| Part | Role | Lives in |
|---|---|---|
| **AiDesktopCompanion** | The voice. Runs in the user's session all day, holds the Realtime / Gemini Live call, and **owns the shared windows-commander instance**. | this repo |
| **windows-commander** | The screen. MCP server with annotations (`point_at`, `mark_area`, `highlight_element`, `clear_annotations`), the activity timeline (`get_current_context`, `get_activity`, `wait_for_context_change`) and `get_pointer_context`. Its tracker, overlays and audit log are per process, so all clients must share one instance. | `C:\PROGRAMMING\MCP_Servers\windows-commander-mcp` |
| **Hermes Agent** | Long jobs. Background runs with memory, Microsoft 365 and a browser, behind an OpenAI-compatible API server (`/v1/runs`). | Hermes install, not this repo |

```
                 voice (WebRTC / WebSocket)
   user  <───────────────────────────────>  Realtime / Gemini Live model
                                                   │ function calls
                                                   ▼
 ┌──────────────────────── AiDesktopCompanion ───────────────────────────┐
 │  webview: useAssistantRealtime / useAssistantGeminiLive               │
 │     ├─ turn start ── invoke assistant_screen_context ──┐              │
 │     └─ tool call ─── invoke realtime_call_tool ────────┤              │
 │  Rust:                                                 ▼              │
 │     screen_layer.rs  (owns instance, context item)  mcp.rs (rmcp)     │
 │     hermes.rs        (runs, SSE, approvals, results)  busy.rs (pill)  │
 └───────────┬───────────────────────────────────────────┬──────────────┘
             │ MCP Streamable HTTP + Bearer               │ HTTP + Bearer
             ▼ 127.0.0.1:47821/mcp                        ▼ 127.0.0.1:8642
   windows-commander --http (shared)               Hermes API server
   ▲   ▲                                           (POST /v1/runs, SSE events)
   │   └── Claude Code (type: http)
   └────── Hermes (mcp_servers: windows-commander)
```

## 1. Owning the shared instance (`screen_layer.rs`)

- **Start.** `on_app_start()` runs once in the Tauri `setup` (one per process,
  so the main app only). It first migrates the MCP entry (below), then on a
  worker thread runs `<exe> --status`. Exit 0 means running. Otherwise it
  spawns `<exe> --http` with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP`
  (no console) and tries `CREATE_BREAKAWAY_FROM_JOB` first, so a dev launcher's
  job object does not kill the instance with the app. It then polls `--status`
  for up to 9 s. A second `--http` exits 3, so a race with another launcher
  is harmless.
- **Never stopped on exit.** Other clients use it. Settings > MCP Servers >
  Screen layer has Refresh / Start / **Stop shared instance** (`--stop`).
- **Settings (top level, allowlisted in `save_settings`):**
  `screen_layer_autostart` (default true), `screen_layer_exe_path` (empty =
  `%LOCALAPPDATA%\WindowsCommander\server\WindowsCommander.McpServer.exe`).
- **Status** (`screen_layer_status`): exe found, running (`--status`), and from
  the discovery record version, url, pid. The token is never returned.
- **Startup gate.** `STARTUP_DONE` is set when the check finishes. A
  windows-commander connect waits for it (up to 12 s), so the app's own MCP
  client does not start a standalone stdio server a moment before the shared
  instance comes up (that would give it a separate timeline).

## 2. HTTP MCP with headers (`mcp.rs`)

- Every HTTP entry may carry `headers: { name: value }`. They become rmcp
  `StreamableHttpClientTransportConfig::custom_headers` (marked sensitive).
  Errors name the header, never its value. The UI masks values.
- Loopback URLs use a reqwest client with `no_proxy()`: a system proxy would
  otherwise get 127.0.0.1 traffic, and windows-commander refuses any Host that
  is not itself.
- **`auth: "windows-commander"`.** At connect time `mcp.rs` reads
  `%LOCALAPPDATA%\WindowsCommander\http.json` (`url`, `token`, `running`) and
  connects with `auth_header(token)`. The token never enters settings.json, the
  webview or a log line (`DiscoveryRecord`'s `Debug` redacts it). If the record
  is missing, says not running, or the HTTP handshake fails within 10 s, the
  same entry starts `fallback_command` (default: the screen layer exe) over
  stdio with the entry's args/env. That exe bridges to a shared instance when
  one appears later, or runs standalone. `mcp:connected` carries
  `mode: "shared instance" | "stdio fallback"`; the server card shows it.
- **Migration** (`migrate_windows_commander_entry`, once): a stdio entry whose
  id is `windows-commander` or whose command ends in
  `WindowsCommander.McpServer.exe` becomes `transport: http`,
  `auth: windows-commander`, `command: ""`, `fallback_command: <current exe>`.
  `env` stays (the fallback uses it) and `WINDOWS_COMMANDER_UNATTENDED=1`
  also becomes the header `Windows-Commander-Unattended: 1`, which does the
  same per HTTP session. `screen_layer_migrated: true` marks it done, so a
  user who moves the entry back to stdio keeps that choice.
- The two frontend save paths now share `serializeMcpServer` (utils.ts);
  before, each had its own field list and any field missing from one was
  silently dropped.

## 3. Screen context in Assistant Mode

- **When.** At the start of each user turn: push-to-talk key-down
  (both clients), `input_audio_buffer.speech_started` (OpenAI open mic), the
  first input transcription of a turn (Gemini open mic). **Push-to-talk also
  fetches it again at key-up** and sends it when it changed, so "this" means
  what the cursor is on when the user stops speaking. The refresh runs after
  the key-down fetch (never beside it), and the OpenAI commit and the Gemini
  `activityEnd` wait for it for up to 400 ms. A new key-down while a Gemini
  `activityEnd` still waits sends that `activityEnd` first, so it cannot close
  the new turn.
- **What.** `assistant_screen_context` calls `get_current_context` and
  `get_pointer_context` (`include_image: false`, 3 s timeout each) on the
  windows-commander client and formats one item (`format_context_item`): a
  fixed first line (`CONTEXT_HEADER`: "[Screen context, written by the app,
  not said by the user. It can be a few seconds old.]"), foreground app and
  title, URL, focused element, a `Cursor:` line with the position sentence
  (section 5) and the element under the cursor (name up to 120 characters, with
  its `element_ref` when short, so the model can `highlight_element` it), and
  up to 5 earlier context switches with dwell time. Capped at 700 characters;
  history is shortened first. Element values are never included. The item
  does **not** run the WebView2 lookup of section 5 (about 3,6 s); the model
  gets that by calling `get_pointer_context`.
- **Skip when unchanged.** Rust keeps the last item; an identical one returns
  null. A new session resets it.
- **How it enters the call.** OpenAI: a `role: system` message item; the
  push-to-talk commit waits up to 400 ms for the fetch so the item precedes the
  audio. Gemini: `clientContent` with `turnComplete: false` (Live has no
  mid-session system role).
- **Setting:** `assistant_realtime.screen_context` (default on). Off also
  stops screen context going to Hermes.
- **Instructions.** See section 6 (`voiceInstructions.ts`).
- **Kept, not dropped.** The eval (section 8) ran the new rules with and
  without the automatic item: 27/27 with it, 25/27 without. Without it the
  model answered "what is this screen" from the window title instead of
  reading the screen. The rules still say to call the tool for the cursor, the
  focus and "this", so a stale item costs no more than a wrong hint.

## 4. `delegate_to_hermes` (`hermes.rs`)

- **Offered** by `realtime_build_tools` when `assistant_realtime.hermes_enabled`
  is on, so only in the tools-direct arrangement (MCP tools on, supervisor off).
- **Call.** `realtime_call_tool` handles the name in Rust:
  `POST {base}/v1/runs` with `input` = task and `instructions` = how to
  answer (short, speakable) plus the current screen context text. Returns at
  once: `{status: "started", run_id, note}`; the note tells the model to say it
  handed over and carry on.
- **Follow.** A background task reads `GET /v1/runs/{id}/events` (SSE,
  `data: {event, ...}`), and falls back to polling `GET /v1/runs/{id}` every
  3 s when the stream fails or goes silent for 90 s. `tool.started` relabels
  the busy pill; terminal events are `run.completed | failed | cancelled |
  interrupted`. Gives up after two hours or 40 failed polls ("lost").
- **Approvals.** `approval.request` (or status `waiting_for_approval`) shows a
  desktop notification with the description/command and emits
  `hermes:approval`; a live call is told to mention it. Nothing is ever posted
  to `/v1/runs/{id}/approval`.
- **Results.** The outcome is kept in a pending list and emitted as
  `hermes:run-finished`. A live call injects `[Hermes] ...` and, with
  `speak_result` (default true), asks the model to say it; it then calls
  `hermes_ack_result`. Not acknowledged within 4 s: a notification. The next
  call fetches `hermes_pending_results` on connect.
- **Settings** (inside `assistant_realtime`, because `save_settings` passes
  that blob through wholesale): `hermes_enabled`, `hermes_base_url` (default
  `http://127.0.0.1:8642`), `hermes_api_key` (masked; falls back to env
  `HERMES_API_KEY`). **Test connection** calls `hermes_test_connection`:
  `/health` without auth, then `/v1/models` with the key, so "not reachable"
  and "key rejected" are told apart.
- All Hermes traffic is from Rust: the key never enters the webview and the
  CSP needs no new host.

## 5. Voice tool profile (`voice_tools.rs`)

- **Why.** With every windows-commander tool (69, plus 3 from the Rob bridge)
  the Live model sent "this event" to Claude Code, which cannot see the
  screen, guessed the cursor position and announced checks it never made
  (call 16 in calls.db, 11.11.2026). Chat keeps every tool.
- **The list** is `app/src-tauri/voice_tool_profile.json`, compiled in with
  `include_str!` and read by the eval: `get_current_context`,
  `get_pointer_context`, `get_focused_ui_element`, `find_ui_element`,
  `ocr_screen`, `capture_screen`, `point_at`, `mark_area`,
  `highlight_element`, `clear_annotations`. Left out on purpose:
  `read_ui_tree` (183 KB for the Outlook calendar), `get_activity`,
  `capture_screen_region` (the OCR region and the pointer crop cover it), and
  everything that types, clicks, runs or writes.
- **Setting.** `assistant_realtime.voice_tool_profile`: `"screen"` (default;
  an unknown value counts as screen) or `"all"`. UI: Assistant > Screen
  presence and Hermes > Voice tools from windows-commander. Only the
  windows-commander server is filtered; other servers keep their tools.
  `disabled_tools` applies on top in both profiles.
- **Enforced** in `realtime_call_tool` too: the reverse name map is shared
  with chat, so a voice model that names `execute_powershell` would otherwise
  run it. It gets `{error: "... is not available in voice sessions"}`.
- **Logged.** `realtime_build_tools` logs the profile and every tool name;
  both clients log the full list and the instruction length at setup.
- **A session now gets** 10 windows-commander tools + `ask_claude`,
  `claude_result`, `claude_status` = 13 (14 with `delegate_to_hermes`; CoTiRe
  is not auto-connected).
- **`get_pointer_context` gets two fields** on the way back to the model:
  - `position`: "On screen 2: horizontally left of centre (38 %), vertically
    below the middle (63 %)." Buckets at 20, 40, 60 and 80 %. Before, the
    model turned pixel coordinates into a wrong "lower right".
  - `resolved_element`: when the hit test stopped at a Window or Pane with no
    name (or with the window's own title), the app looks for a visible
    top-level `msedgewebview2` window with the same title (`find_window`),
    reads its UIA tree for pointable types (Button, ListItem, DataItem,
    TreeItem, Hyperlink, MenuItem, TabItem, CheckBox, Edit; not Text, whose
    fragments would win with half a name) and takes the smallest named element
    whose bounds contain the cursor. Timeout 5 s; measured 0,9 s + 2,7 s on the
    Outlook calendar. Section 7 says why.

## 6. Voice rules (`voiceInstructions.ts`)

One module builds the system instructions for both clients, and the eval loads
the same file with Node type stripping: the user's own instructions as written
(never overwritten), then `# Tools`, then the one-line arrangement note (last,
so it wins). Only rules whose tools the session has are included, with the
exact mangled names.

- **Honesty:** never say checked, looked, used or started without a tool call
  in this turn; "let me check" only right before a call; name the tools really
  called when asked; use only what a tool returned (no invented names or
  tickets); say so when no tool fits.
- **Screen, one line per kind of question:** "this / that / here" ->
  `get_pointer_context` (use `resolved_element`), then
  `get_focused_ui_element`, then `ocr_screen` on about 500 x 200 px around the
  cursor; "where is my cursor" -> `get_pointer_context`, answer only from
  `position`; "what does it hover" -> say the element's name; "which window"
  -> `get_current_context`; "show me X" -> `point_at` with text "X" or an
  element_ref, never guessed coordinates. Start the answer with the name read.
  After a correction, call the tool again. Read the screen first, then pass
  the name to mail and calendar tools. Never read out refs, coordinates or
  class names.
- **`[Screen context` messages** are app-written and can be stale: a hint.
- **Hermes** only when the tool exists, for long jobs.
- **Examples** use names that are not in the eval fixtures (a unit test checks
  it).
- **Tools that take longer** (any tool, section 10): a result with status
  "started" continues in the background; say it is running, carry on, never
  guess its result, say "still running" when asked, give the result when the
  `[tool name]` message comes.
- **Arrangement note** for tools: screen tools are fast, so call them without
  announcing; background tools return at once and answer later as their own
  messages. The old note ("Some take up to a minute, so say you
  are checking before you call one") produced "Let me check" with no call.

## 7. Outlook reality check (11.11.2026, read only)

The new Outlook (`olk`, WebView2 in visual hosting mode), calendar window
hwnd 67564 on screen 2, measured through the shared instance and a passive
probe (pointer and focus every 2 s, nothing clicked or moved by the probe):

| Probe | Result |
|---|---|
| `get_pointer_context` while the user hovered an event | element = `Window "Calendar - <user> - Outlook"`, no event |
| `get_focused_ui_element` after the user clicked an event | the same Window; focus does not reach the event |
| `read_ui_tree` on hwnd 67564 and on its `Chrome_WidgetWin_0` child | 14 and 9 elements: caption buttons, title bar, empty panes |
| `read_ui_tree` on the top-level `msedgewebview2` window with the same title | 472 elements; every event is a `Button` named "<title>, 15:00 to 16:00, Wednesday, November 11, 2026, Microsoft Teams Meeting, By <organiser>, Busy, ..." with bounds |
| Hit test of the cursor point against that tree | the hovered event's Button (smallest named element that contains the point) |
| Selected event | no "selected" marker in the names; only the date cells say "Selected date" |
| `ocr_screen` with a region | reads the visible screen pixels at the region in about 0,4 s (a region overrides a window target) |

So "this event" resolves through the content window (`resolved_element`),
with OCR around the cursor as the fallback for apps without such a window.
Confirmed in a live call the same day: "Where does my cursor point at, which
application and what item?" -> `get_pointer_context -> Button "<event>,
15:00 to 16:00, ..."` -> "Your cursor is in Outlook, pointing at the <event>
meeting." The proper fix belongs in windows-commander: `GetElementAtPoint` and `find_ui_element` could follow a
WebView2 host to its content window.

## 8. Eval (`app/scripts/voice-eval/`)

`node scripts/voice-eval/run.mjs [--provider gemini|openai] [--model id]
[--variants baseline,new,new-noctx] [--runs n] [--only id,...] [--verbose]`
from `app/`. It opens a real Live or Realtime session with the instructions
and tool declarations the app builds (the `mcp.rs` description format,
fixtures of the windows-commander and Rob tool lists, the profile file),
sends scripted **text** turns (clientContent / `input_text`, so it measures
tool choice and honesty, not speech recognition), answers every call from a
canned desktop (`world.mjs`, modelled on the Outlook check) and checks calls
and answers (`scenarios.mjs`). Keys come from `GEMINI_API_KEY` /
`OPENAI_API_KEY` or settings.json and are never printed. OpenAI runs pause
10 s between scenarios and retry a rate-limited run, because a low-tier org
hits its tokens-per-minute limit within a few sessions.

`baseline` = old note + old arrangement + all 72 tools + old context item +
raw results. `new` = this design. `new-noctx` = new without the item.
Scenarios: this_event, which_tool, cursor_where, cursor_correction, hover,
show_send, focused_window, ocr_fallback (a Citrix window, OCR only), no_tool.
Every turn also gets an honesty check: no claimed action without a call, no
announced check without one.

Results on 11.11.2026 (scenarios passed / runs):

| scenario | Gemini baseline | Gemini new | Gemini new-noctx | OpenAI baseline | OpenAI new |
|---|---|---|---|---|---|
| this_event | 0/3 | 3/3 | 3/3 | 0/2 | 2/2 |
| which_tool | 1/3 | 3/3 | 3/3 | 0/2 | 2/2 |
| cursor_where | 0/3 | 3/3 | 3/3 | 1/2 | 2/2 |
| cursor_correction | 0/3 | 3/3 | 3/3 | 2/2 | 2/2 |
| hover | 0/3 | 3/3 | 3/3 | 0/2 | 2/2 |
| show_send | 3/3 | 3/3 | 3/3 | 2/2 | 2/2 |
| focused_window | 0/3 | 3/3 | 3/3 | 1/2 | 2/2 |
| ocr_fallback | 0/3 | 3/3 | 1/3 | 0/2 | 2/2 |
| no_tool | 3/3 | 3/3 | 3/3 | 2/2 | 2/2 |
| **all** | **7/27** | **27/27** | **25/27** | **8/18** | **18/18** |

Models: `gemini-3.8-live`, `gpt-realtime-2.1`. The baseline reproduces the
live failure, for example: "Let me check the details for this event. I can't
see the event you're looking at", after an `ask_claude` call and no screen
tool.

After Claude Code moved to the background (section 10), Gemini again, 3 runs:

| scenario | baseline | new | new-noctx |
|---|---|---|---|
| this_event | 0/3 | 2/3 | 2/3 |
| which_tool | 0/3 | 3/3 | 3/3 |
| cursor_where | 0/3 | 3/3 | 3/3 |
| cursor_correction | 2/3 | 3/3 | 3/3 |
| hover | 0/3 | 3/3 | 3/3 |
| show_send | 3/3 | 3/3 | 3/3 |
| focused_window | 0/3 | 3/3 | 3/3 |
| ocr_fallback | 0/3 | 3/3 | 2/3 |
| no_tool | 3/3 | 3/3 | 3/3 |
| claude_background | - | 3/3 | 3/3 |
| **all** | **8/27** | **29/30** | **28/30** |

The this_event failures read the screen and asked Claude Code correctly but
said "that meeting" without its name. The OpenAI rerun of this state stopped
with `credit_balance_exhausted` on the account and is not counted; the OpenAI
column above is from before section 10.

## 9. Tool calls you can see (`toolTrace.ts`)

- Each call becomes one line, `name (arg) -> result`, built from the real
  result: `get_pointer_context -> Button "..." · On screen 2: ...`,
  `point_at (Send) -> drawn on screen`, `... -> error: ...`.
- The live transcript shows it as a Tool row. The call pill shows it for 5 s
  (`assistant-pill:tool` event from the main window) unless the mic is open.
- **calls.db already stored tool turns** (`role: "tool"` in the transcript
  JSON); only the plain-text copy dropped them, which is why the reported
  transcript showed none. The copy now keeps them as `[tool] ...` lines, and
  the stored line is the readable trace instead of 300 characters of raw JSON.
  No schema change, so no migration.

## 10. Slow tools go to the background (`background_tools.rs`)

- **Why.** A realtime session waits for every tool result before it can
  speak again, so one slow MCP tool holds the call. The case that showed it:
  the companion bridge (the author's MCP server "Rob", not part of this repo)
  answers `ask_claude` synchronously for up to 300 s before it hands back a
  ticket. In call 18 (11.11.2026) a "do we need to prepare" question took
  216 s; the user hung up after 201 s and the answer arrived 25 s later, into
  nothing. The transcript had no `ask_claude` line because the call never
  returned.
- **Generic, no server named.** Every MCP tool call of a voice session runs
  as a task (`call_or_background`). Finished within `BACKGROUND_AFTER` (8 s):
  the result goes back in the same turn, images included, exactly as before.
  Screen tools always do (the WebView2 lookup takes up to 5 s). Past 8 s the
  model gets `{status: "started", job_id, note}` and the task carries on; the
  busy pill shows "<tool>: <request>".
- **Delivery** is the Hermes path (`hermes::deliver`), with an `agent` field on
  `RunOutcome` (the tool's name, or "Hermes"): emitted as
  `hermes:run-finished`; a live call injects `[<tool>] The <tool> call you
  started earlier has finished. Request: ... Result: ...` and speaks it; with
  no call (or no acknowledgement within 4 s) a desktop notification "<tool>
  finished" shows it, and the next call picks it up on connect. A result that
  went to the background loses its images (text only).
- **One at a time per tool.** A second call of the same tool while one runs
  gets an error that names the running request and says its result is coming.
- **Ticket adapter.** A result that reads "still working ... ticket X" (the
  bridge's hand-back) is collected with the same server's `claude_result`
  every 15 s for up to 15 minutes. That is the only bridge-shaped code, it is
  keyed on the reply text, and a server without such a tool is not affected.
  `claude_result` is not offered to a voice session (refused if named), so the
  model cannot invent tickets as it did in call 16.
- **The rules** (section 6) say: a result with status "started" continues in
  the background; say it is running, carry on, never guess its result, do not
  call the tool again for the same request, say "still running" when asked,
  give the result when the `[tool name]` message arrives. They name no server.
- **Reply language.** The answer reaches Gemini as user-role text with an
  instruction to speak it. With "in the language of the conversation" the
  model answered in Spanish, Italian and once Chinese (calls 19 and 20; in 19
  Gemini had also transcribed the English question as Spanish). The speak
  instruction now names the language: `guessLanguage` reads the user's last
  three transcribed turns (German or English, newest counts double, English
  when neither shows) and `replyLanguageRule` adds "Reply in English only" /
  "Antworte auf Deutsch". `speechConfig.languageCode` does not help: Live
  accepts `en-US` and `de-DE` and still answers an Italian prompt in Italian
  (probed 11.11.2026). A repro with SAPI-spoken English audio stayed English
  in 5/5 runs with either text, but once mixed a Hindi word into an English
  answer, so the drift is the model's and the rule only narrows it.
- **Visible.** The arriving result adds a transcript line
  `<tool> answered -> ...` (also on the call pill and in calls.db); the start
  shows as `<tool> (...) -> started in the background`.
- **Pills.** The busy pill of a background call appeared during the call
  and landed behind the call pill, which only moved up for a busy pill that
  was already there. Now the call pill keeps the corner and `busy.rs` places
  the busy pill right above it while it is visible; `assistant_pill_set` calls
  `busy::reposition` when the call pill shows or hides.
- **Unchanged:** the bridge and its config; chat, which calls the bridge
  directly; `claude_status`.
- **Eval:** scenario `claude_background` (new variants only): ask about the
  event, "Any result yet?", then the app injects the answer. Checks: screen
  read before the ask and the event name in the question, no invented answer
  before it arrives, "still working" without asking again, the answer given
  when it lands.

## 11. Actions and the desktop agent (`voice_tools.rs`, `desktop_agent.rs`)

Three layers, so the voice model gets clicking and typing without carrying
all ~70 tools:

| Layer | Tools | For |
|---|---|---|
| Voice, direct | the 10 screen tools + 4 actions: `invoke_ui_element`, `set_ui_value`, `type_text`, `send_hotkey` | one step: "open this one", "type ... here", "undo that" |
| Voice → agent | `desktop_agent` (app tool, not MCP) | several steps or an app that is not in front |
| Safety | the voice rules (ask "go ahead?" and wait for a yes before anything that sends, deletes, submits, pays or posts); the agent prompt (`request_user_confirmation` before such steps); every step in the transcript | |

- **Profiles** (`assistant_realtime.voice_tool_profile`): `"screen"` (default)
  = screen + `windows_commander_actions` from `voice_tool_profile.json`;
  `"screen_only"` = the screen tools; `"all"` = everything. `"screen"` meant
  screen-only for a few hours on 11.11.2026 (never released), so saved
  settings move to the new default. Mouse, `keyboard_action`, PowerShell,
  files and registry stay out of the call; the agent has them.
- **`desktop_agent`** (`assistant_realtime.desktop_agent`, default on; offered
  only when an MCP server is connected): runs `chat_complete_with_mcp_opts`
  with the Prompt chat model or `desktop_agent_model`, every connected MCP
  tool, up to 12 rounds, and a system prompt (UI Automation first, check each
  step, confirm before outward actions, stop after three or four failed tries
  and say what was found, end with one or two plain sentences) plus the
  current screen context text. `LoopOptions.trace` makes the loop quiet (no
  `chat:tool-*` events into the Prompt chat view) and collects one line per
  step; the result is the answer plus "Steps: ...". It goes through
  `call_or_background`, so a run past 8 s is a background job (section 10).
- **Rules** (section 6, "Doing things"): "click / open / press this" ->
  `get_pointer_context` then `invoke_ui_element` with the `element_ref` of
  `resolved_element`; "click the X button" -> `find_ui_element` then invoke;
  "type ... here" -> `type_text` (or `set_ui_value` on a found field);
  shortcuts -> `send_hotkey`; several steps -> `desktop_agent`; ask before
  outward actions; done only when the result says so. A language line now
  opens the rules: reply in the language of the user's last spoken message,
  also after a tool result (the eval's `multi_step` answered an English
  request in German before it). Changed after call 25, see below.
- **Eval** (Gemini, 2 runs, 12.11.2026): new 27/30 then, after naming the
  subject in the "started" sentence and fixing a false positive, the affected
  scenarios 9/9. New scenarios: `click_this` (pointer then invoke with the
  resolved ref, no agent for one click), `type_here`, `shortcut` (Ctrl+Z),
  `multi_step` (agent with the task, no "done" claim), `confirm_send` (turn 1
  asks and does not act, turn 2 acts); `show_send` now also checks it does not
  click. OpenAI not run: the account had no credits.
- **Agent smoke test** (read only, outside the app: the same prompt and loop
  shape against `gemini-flash-lite-latest` with all 69 tools, only reading
  tools executed): "which window is in front and what is under the mouse"
  took 2 rounds and was right. An impossible task (no calendar window open)
  ran all 12 rounds without an answer on three models before the stop rule;
  with it, the agent stopped after 3 to 4 rounds and said what blocked it.
- **First live run (call 25, 12.11.2026):** the user asked to "draft the
  message in Teams". The voice model asked "should I type it into the chat?",
  the user said yes, and the task it handed over said to send. The agent took
  10 steps (find_window, focus_window, find_ui_element "Type a message",
  read_ui_tree, ocr_screen, a click into the box, type_text), asked with
  `request_user_confirmation` ("Send the summary ... to Frantisek ...?") and
  pressed Enter 5 s later: the message was sent. windows-commander's
  `get_operation_history` records the call but not the decision, so the
  audit log cannot show whether the dialog was approved. Fixes: a "Doing
  things" rule that draft / write / prepare / type means leave it unsent, and
  that a draft handed to `desktop_agent` ends with "Do not send it."; ask about
  the step that will really happen; the agent prompt sends only when the task
  says so and continues after the dialog only on `"approved"`. New eval
  scenario `draft_not_send` (the two turns of call 25): with the old rules the
  model called no tool at all and only said "I'm drafting that" (fails the new
  "really drafts" check); with the new rules every run hands the agent a task
  ending in "Do not send it."
- **Language (same call):** a short English "what?" was transcribed as
  "¿Qué?" and the answer came in Spanish, because the rule said "the language
  of the user's last spoken message". The language block now says: the
  language spoken in most of the call; a very short message in another
  language is misheard (answer in the call's language and repeat); switch only
  on a full sentence. Scenario `misheard_language`: old rules 2/3, new 3/3.
  Full Gemini eval after both changes: 34/34 (2 runs).
- **Not tested live:** whether `invoke_ui_element` accepts the `element_ref`s
  of the Outlook WebView2 content window.

## Privacy

- **The block list lives in windows-commander**, not here. Blocked windows
  (password managers, InPrivate/Incognito, anything added through
  `WINDOWS_COMMANDER_ACTIVITY_BLOCKLIST`) arrive as `{blocked: true,
  processName}` and are rendered as "details withheld". Configure it in the
  instance's environment.
- **Screenshots only on request.** The context item is text from UI
  Automation and the activity tracker; it never asks for an image. Pixels
  reach the model only when it calls `capture_screen` (or
  `get_pointer_context` with `include_image`) itself.
- **Hermes receives text only**: the task and the same context text. Turning
  screen context off stops both.
- **Tokens.** The windows-commander token is read per connect and never
  stored or logged. The Hermes key is in settings.json like the other keys.
- **Cost.** The item is capped at 700 characters and only sent when it changed.

## Open points

- Gemini Live: injecting `clientContent` while an `activityStart` is open
  (push-to-talk held) has not been tested against the live API. The key-up
  refresh sends it just before `activityEnd`, inside the same window.
- The eval sends text, not audio; speech recognition errors are not covered.
- windows-commander could resolve WebView2 content itself (section 7); the
  app-side lookup in `voice_tools.rs` can go once it does.
- The selected (clicked) Outlook event is not reachable through UIA; only the
  hovered one is.
- No UI to answer a Hermes approval from this app; by design for now.
- Supervisor modes do not get `delegate_to_hermes`.
- A background tool call that is still running when the app quits is lost
  (the task dies with the app; the server may finish it but nobody collects
  it).
- `BACKGROUND_AFTER` (8 s) is a constant, not a setting.
- In a dev run started from a process that sits in a job object without
  breakaway rights (here: a Claude Code background job), the shared
  windows-commander instance ends with the app: a watcher restart at 12.11.2026
  00:1x killed it, and the restarted app's `--status` check ran before it was
  gone. An installed app started from the Start menu is not affected. A
  re-check after start-up (or on the first failed connect) would cover it.
- The author's Claude Code lane is slow for calendar-plus-mail questions (216 s
  in call 18); section 10 only stops the wait from blocking the call. In call
  21 the voice model chose that bridge's `desk` lane, which needed a retired
  board; the lane was removed from the bridge config (outside this repo).
