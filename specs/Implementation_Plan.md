# Implementation Plan and Status

Last updated: 2025-08-26

## Objectives
- Deliver a Windows-first AI Desktop Companion with a Quick Actions popup (P/T/S/I) and main prompt panel.
- Baseline cloud integrations: OpenAI TTS and Whisper STT. Image capture and Quick Prompts to follow.
- Keep macOS feasibility in mind; implement hotkeys cross-platform.
## Current Architecture
- Frontend: Vue 3 + Vite.
  - Main window UI: `app/src/App.vue`, `components/PromptPanel.vue`.
  - Quick Actions popup: `app/src/QuickActions.vue`.
  - STT recorder: `app/src/stt.ts` (MediaRecorder, WEBM/Opus).
  - Window management: `app/src/popup.ts`.
- Backend: Tauri v2 (Rust) in `app/src-tauri/`.
  - Commands: `prompt_action`, `position_quick_actions`, `tts_selection`, `stt_transcribe`, `open_prompt_with_text`, `run_quick_prompt`, `chat_complete`, `get_settings`, `save_settings`, `list_openai_models`, `load_conversation_state`, `save_conversation_state`, `clear_conversations`, `get_quick_prompts`, `save_quick_prompts`, `generate_default_quick_prompts`.
  - Whisper integration: `reqwest` multipart to `https://api.openai.com/v1/audio/transcriptions` with model `whisper-1`.
  - Persistence paths: Settings at `%APPDATA%/AiDesktopCompanion/settings.json`; Conversations at `%APPDATA%/AiDesktopCompanion/conversations.json` when `persist_conversations` is enabled.

## Scope Alignment with Overview

- [__Global Hotkey Activation__]
  - [x] Windows global hotkey toggles popup near cursor (default Alt+A)
  - [ ] macOS global hotkey + popup positioning (‼️ TODO)
  - [ ] Configurable hotkeys UI in Settings (‼️ TODO)

- [__Quick Actions Popup (P/T/S/I)__]
  - [x] Popup layout near cursor; P/T/S/I mnemonics; click-away close; no idle auto-dismiss
  - [x] Aggressive copy-restore selection with preview (default ON)
  - [x] Prompt (P) opens main window with selection
  - [x] TTS (T) via OpenAI; voice `nova`, speed `0.25`; clipboard fallback; 
  - [ ] toast on empty TTS selection (‼️ TODO)
  - [~] STT (S) push-to-talk wired; toggle mode via Settings (‼️ TODO)
  - [ ] Image (I) region capture overlay with 2px red outline; open main window (‼️ TODO)
  - [x] Quick Prompts 1–9 mapped; run and insert AI result; close popup

- [__Main Window Components__]
  - [x] Prompt section: Conversation with AI — `ConversationHistory.vue` above `ConversationView.vue`; `PromptComposer.vue` integrated; multi-conversation + New Conversation.
  - [ ] MCP tools selection UI in conversation header (‼️ TODO placeholder present in `ConversationView.vue`).
  - [ ] Vision support for image messages in prompts (‼️ TODO placeholder present in `PromptComposer.vue`).
  - [ ] TTS section: play/stop/download, speed/voice selector, download/save as option.
  - [ ] STT section: transcript view, copy/use-as-prompt
  - [x] Settings section (Prompt settings, Quick Prompts editor). Persistence toggle wired and QAed.

- [__Settings Configuration__]
  - [x] Prompt settings: OpenAI API Key, Model selection (via available models query), Temperature, Conversation persistence toggle, "Clean all conversations" button.
  - [ ] Provider URL/API key; STT engine & Whisper model selection
  - [ ] Global hotkey config + direct STT hotkey
  - [ ] Aggressive copy Safe Mode toggle; selection preview toggle
  - [ ] Autostart (Windows); macOS if possible
  - [ ] Additional settings: system prompts and exclude AI thoughts
  - [x] Set Quick Prompts 1–9 (via Quick Prompts Editor in Settings)

- [__MCP Tools Integration__]
  - [ ] Server mgmt (name, command, args, cwd, env) via stdio, sse and http
  - [ ] Lazy-connect, status, retry with backoff; permissions prompts
  - [ ] Persist last-used tool set optionally (History) (‼️ TODO)

- [__Conversation Model__]
  - [x] Conversation thread with context; New conversation action
  - [ ] Popup Prompt behavior: append vs always-new (setting)
  - [~] Optional History: On-disk persistence toggle implemented for conversations (messages + images). Transcripts/tool usage TBD.

- [__Cross-cutting__]
  - [ ] i18n readiness; Themes: Light/Dark/High Contrast
  - [x] Privacy defaults (no persistence); History toggle OFF by default
  - [ ] Telemetry disabled
  - [ ] UAC/admin overlay blocked ➜ toast
  - [ ] Windows 10/11 target; macOS planned
  - [ ] Offline STT models option

## UI Styles

- Mechanism: Base styles live in `app/src/style.css`. The main window dynamically loads a style‑specific CSS via a `<link id="theme-style-css">` injected by `applyStyleCss()` in `app/src/App.vue`.
- Styles directory: `app/src/styles/<styleName>/style.css`.
- Current styles: `sidebar` (default) and `tabs` (legacy). ‼️ Both ship with placeholder overrides — customize as needed.
- Switching: `Settings` ➜ `UI Style` updates `settings.ui_style`; a watcher updates the injected CSS at runtime (HMR-friendly).
- Windows excluded: Quick Actions and Capture Overlay do not load style CSS by default to remain minimal and consistent.

Add a new style:
1. Create `app/src/styles/<styleName>/style.css`.
2. In `app/src/App.vue`, import the CSS with `?url` and extend `styleCssMap`:
   ```ts
   import myStyleUrl from './styles/myStyle/style.css?url'
   const styleCssMap = { sidebar: sidebarStyleUrl, tabs: tabsStyleUrl, myStyle: myStyleUrl }
   ```
3. (Optional) Add `<option value="myStyle">My Style</option>` in the Settings UI and extend the TypeScript union for `ui_style`.

See also: `specs/UI_Styles.md` for detailed guidance.

## Security: Content Security Policy (CSP)

- What: Strict CSP enforced via `app/src-tauri/tauri.conf.json` under `app.security.csp` and `app.security.devCsp`.
- Why: Reduce XSS risk and limit resource origins while supporting app features.
- Production CSP (summary):
  - default-src: `'self' asset:`
  - connect-src: `ipc:` `http://ipc.localhost` `https://api.openai.com`
  - img-src / media-src: `'self' asset:` `http://asset.localhost` `blob:` `data:`
  - font-src: `'self' data:`
  - style-src: `'self' 'unsafe-inline'`
  - worker-src: `'self' blob:`
  - Hardening: `object-src 'none'`, `frame-ancestors 'none'`, `base-uri 'none'`, `form-action 'self'`
- Development CSP (adds HMR):
  - connect-src also allows `http://localhost:5173` and `ws://localhost:5173`
- Notes:
  - Tauri injects nonces/hashes for scripts/styles; we intentionally omit explicit `script-src`.
  - Asset protocol is enabled and scoped to `$RESOURCE/**`, `$APP/**`, `$TEMP/**`; frontend uses `convertFileSrc`.

- Optional later (‼️ TODO):
  - Add HTTP headers in `app.security.headers`:
    - `Cross-Origin-Opener-Policy`: `same-origin`
    - `Cross-Origin-Embedder-Policy`: `require-corp`
    - Consider `Timing-Allow-Origin` for selected domains
  - Evaluate `freezePrototype: true` under `app.security` (compatibility testing required)

## QA Results (2025-08-26)
- __Persistence ON__: History saved on message send and restored on restart (debounced auto-save). Current conversation tracked by `currentId`.
- __Clear All while ON__: Resets UI to a fresh thread and updates `conversations.json` accordingly.
- __Persistence OFF__: Deletes `conversations.json`; no further writes occur; app restarts with a fresh thread.
- __Regression__: Prompt flow, model selection, and temperature respected by completions.
- __Files__: `%APPDATA%/AiDesktopCompanion/conversations.json`, `%APPDATA%/AiDesktopCompanion/settings.json`.

## Next Steps
 - MCP Tools: server config screen; Tools Panel skeleton; connect/disconnect; per-prompt enable. (‼️ TODO)
 - Image region capture overlay and main window integration (Windows-first). (‼️ TODO)
 - Vision support in `PromptComposer.vue` + `ConversationView.vue` for image messages. (‼️ TODO)
 - Conversation setting: popup Prompt behavior (append vs always-new). (‼️ TODO)
 - Settings UI: hotkeys (global + direct STT), STT mode (push-to-talk vs toggle), Safe Mode, selection preview, provider/keys, autostart. (‼️ TODO)
 - TTS section UI (play/stop/voice/speed/download). (‼️ TODO)
 - STT section UI (transcript view, copy/use-as-prompt). (‼️ TODO)
 - macOS parity: global hotkey registration + popup positioning. (‼️ TODO)
 - Offline STT toggle placeholder and download flow. (‼️ TODO)
 - UAC overlay detection + toast. (‼️ TODO)
 - Packaging/installer and smoke tests. (‼️ TODO)

## Milestones (proposed)
- __M1: MCP Tools Skeleton__ (target: short cycle)
  - [ ] Tools Panel UI placeholder under `ConversationView` (‼️)
  - [ ] Server config UI (name, transport, command/URL, args/env) (‼️)
  - [ ] Connect/disconnect lifecycle with basic status/errors (‼️)
  - [ ] Per-prompt enable/disable selectors (‼️)
- __M2: Image Capture Overlay__
  - [ ] Region capture overlay (Windows) and file save (‼️)
  - [ ] Post-capture: open Prompt tab and append image message (‼️)
- __M3: TTS/STT Sections__
  - [ ] TTS playback UI; voice/speed; download (‼️)
  - [ ] STT transcript view; copy/use-as-prompt (‼️)
- __M4: Packaging__
  - [ ] Windows installer + smoke tests (‼️)

## Acceptance Criteria (M1: MCP Tools Skeleton)
- [ ] Add/remove tool server entries in Settings; persisted to disk
- [ ] Connect/disconnect shows status and error toasts
- [ ] Selected tools visible in Prompt tab; persisted optional
- [ ] No regressions in prompt flow, persistence, or Quick Prompts

## Environment / Keys
- Set `OPENAI_API_KEY` before running the app. Example (PowerShell):
  - `$env:OPENAI_API_KEY = "<your key>"`
- Optional: `OPENAI_CHAT_MODEL` for Quick Prompts (default `gpt-4o-mini`).

## Known Considerations / Risks
- Mic permission: must be allowed in OS privacy settings. Tauri app runs as secure context for MediaRecorder.
- Network/API failures: we show friendly messages in the prompt panel; consider retry and toasts later.
- Telemetry: disabled by default per design.

## Changelog (recent)
- Implemented strict CSP and devCSP; verified HMR, asset previews, STT blob, and OpenAI calls.
- Implemented push-to-talk STT with MediaRecorder and Whisper request.
- Popup remains open during recording; opens prompt on success, failure, or no speech.
- Added `reqwest` (rustls) for backend.
- Implemented Quick Prompts 1–9: aggressive copy-restore selection, OpenAI Chat Completions, paste result into active app.
 - Integrated `PromptComposer.vue` and added backend `chat_complete` command; fixed scope and registration.
 - Implemented conversation persistence with Tauri commands (`load_conversation_state`, `save_conversation_state`, `clear_conversations`); debounced auto-save; respects `persist_conversations`.
 - Added `ConversationHistory.vue` with sorting by `updatedAt`, tooltips, and double-click switching; wired into `App.vue` above `ConversationView`.
 - Completed Prompt Settings UI: API key, model dropdown (fetched), temperature slider, persistence toggle, Clear All conversations; Quick Prompts Editor.
 - End-to-end QA passed for persistence ON/OFF, Clear All behavior, and regression of prompt flow and model/temperature usage.
