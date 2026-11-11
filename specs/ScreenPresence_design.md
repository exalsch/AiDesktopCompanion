# Screen presence: voice, screen and long jobs

Status: implemented on `feat/screen-presence` (2026-11-11).

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
  first input transcription of a turn (Gemini open mic).
- **What.** `assistant_screen_context` calls `get_current_context` and
  `get_pointer_context` (`include_image: false`, 3 s timeout each) on the
  windows-commander client and formats one item (`format_context_item`):
  foreground app and title, URL, focused element, element under the cursor
  (with its `element_ref` when short, so the model can `highlight_element`
  it), and up to 5 earlier context switches with dwell time. Capped at 600
  characters; history is shortened first. Element values are never included.
- **Skip when unchanged.** Rust keeps the last item; an identical one returns
  null. A new session resets it.
- **How it enters the call.** OpenAI: a `role: system` message item; the
  push-to-talk commit waits up to 400 ms for the fetch so the item precedes the
  audio. Gemini: `clientContent` with `turnComplete: false` (Live has no
  mid-session system role).
- **Setting:** `assistant_realtime.screen_context` (default on). Off also
  stops screen context going to Hermes.
- **Instructions.** `screenPresenceNote()` appends, depending on the tools
  the session really has: how to point (`point_at` / `mark_area` /
  `highlight_element`, prefer element or text targets over coordinates,
  `clear_annotations` when done), what `[Screen context]` messages are, and
  when to use `delegate_to_hermes`.

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
- **Cost.** The item is capped at 600 characters and only sent when it changed.

## Open points

- Gemini Live: injecting `clientContent` while an `activityStart` is open
  (push-to-talk held) has not been tested against the live API.
- No UI to answer a Hermes approval from this app; by design for now.
- Supervisor modes do not get `delegate_to_hermes`.
