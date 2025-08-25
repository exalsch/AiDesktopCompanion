# Implementation Plan and Status

Last updated: 2025-08-24

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
  - Commands: `prompt_action`, `position_quick_actions`, `tts_selection`, `stt_transcribe`, `open_prompt_with_text`.
  - Whisper integration: `reqwest` multipart to `https://api.openai.com/v1/audio/transcriptions` with model `whisper-1`.

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
  - [ ] Quick Prompts 1–9 mapped; run and insert AI result; close popup (‼️ TODO)

- [__Main Window Components__]
  - [ ] Prompt section: Conversation with AI. Start new conversation button. 
  - [ ] TTS section: play/stop/download, speed/voice selector
  - [ ] STT section: transcript view, copy/use-as-prompt
  - [ ] Settings section (‼️ TODO)
  - [ ] Tools Panel (MCP): Add tools to prompt (‼️ TODO)

- [__Settings Configuration__]
  - [ ] Provider URL/API key; STT engine & Whisper model selection
  - [ ] Global hotkey config + direct STT hotkey
  - [ ] Aggressive copy Safe Mode toggle; selection preview toggle
  - [ ] Autostart (Windows); macOS if possible
  - [ ] Additional settings: system prompts and exclude AI thoughts
  - [ ] Set Quick Prompts 1–9

- [__MCP Tools Integration__]
  - [ ] Server mgmt (name, command, args, cwd, env) via stdio, sse and http
  - [ ] Lazy-connect, status, retry with backoff; permissions prompts
  - [ ] Persist last-used tool set optionally (History) (‼️ TODO)

- [__Conversation Model__]
  - [ ] Conversation thread with context; New conversation action
  - [ ] Popup Prompt behavior: append vs always-new (setting)
  - [ ] Optional History: messages, images, transcripts, tool usage

- [__Cross-cutting__]
  - [ ] i18n readiness; Themes: Light/Dark/High Contrast
  - [ ] Privacy defaults (no persistence); History toggle OFF by default
  - [ ] Telemetry disabled
  - [ ] UAC/admin overlay blocked ➜ toast
  - [ ] Windows 10/11 target; macOS planned
  - [ ] Offline STT models option

## Next Steps
 - Image region capture overlay and main window integration (Windows-first). (‼️ TODO)
 - Quick Prompts 1–9: mapping, execution, insert AI result, close popup; configurable in Settings. (‼️ TODO)
 - Settings UI: hotkeys (global + direct STT), STT mode (push-to-talk vs toggle), Safe Mode, selection preview, provider/keys, autostart. (‼️ TODO)
 - MCP Tools: server config screen; Tools Panel skeleton; connect/disconnect; per-prompt enable. (‼️ TODO)
 - Conversation threading: New conversation action; popup P append/new setting; History scaffolding (OFF by default). (‼️ TODO)
 - Accessibility/themes/i18n scaffolding; theme switcher. (‼️ TODO)
 - macOS parity: global hotkey registration + popup positioning. (‼️ TODO)
 - Offline STT toggle placeholder and download flow. (‼️ TODO)
 - UAC overlay detection + toast. (‼️ TODO)

## Known Considerations / Risks
- Mic permission: must be allowed in OS privacy settings. Tauri app runs as secure context for MediaRecorder.
- Network/API failures: we show friendly messages in the prompt panel; consider retry and toasts later.
- Telemetry: disabled by default per design.
## Changelog (recent)
- Implemented push-to-talk STT with MediaRecorder and Whisper request.
- Popup remains open during recording; opens prompt on success, failure, or no speech.
- Added `reqwest` (rustls) for backend.
