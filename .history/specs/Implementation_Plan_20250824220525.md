# Implementation Plan and Status

Last updated: 2025-08-24

## Objectives
- Deliver a Windows-first AI Desktop Companion with a Quick Actions popup (P/T/S/I) and main prompt panel.
- Baseline cloud integrations: OpenAI TTS and Whisper STT. Image capture and Quick Prompts to follow.
- Keep macOS feasibility in mind; implement hotkeys cross-platform.

## Status Dashboard
- [x] Global hotkeys: toggle Quick Actions popup (Windows; macOS planned)
- [x] Prompt action: aggressive copy-restore, focus main window, selection preview
- [x] Position Quick Actions window near cursor
- [x] Windows-first TTS (PowerShell/.NET)
- [ ] Quick Prompts 1–9: insert AI result (‼️ TODO)
- [ ] Image region capture with overlay and open main window (‼️ TODO)
- [~] Speech-to-Text: push-to-talk via Whisper (in progress)
  - Frontend recording with MediaRecorder: DONE (app/src/stt.ts)
  - Popup push-to-talk wiring (S key + mouse): DONE (app/src/QuickActions.vue)
  - Backend Whisper request: DONE (app/src-tauri/src/lib.rs: stt_transcribe)
  - Open prompt with transcription: DONE (open_prompt_with_text)
  - Error/empty handling: DONE (opens prompt with message)
  - Settings: toggle vs push-to-talk: TODO
- [ ] Settings: hotkey customization & Safe Mode toggle: TODO

## Current Architecture
- Frontend: Vue 3 + Vite.
  - Main window UI: `app/src/App.vue`, `components/PromptPanel.vue`.
  - Quick Actions popup: `app/src/QuickActions.vue`.
  - STT recorder: `app/src/stt.ts` (MediaRecorder, WEBM/Opus).
  - Window management: `app/src/popup.ts`.
- Backend: Tauri v2 (Rust) in `app/src-tauri/`.
  - Commands: `prompt_action`, `position_quick_actions`, `tts_selection`, `stt_transcribe`, `open_prompt_with_text`.
  - Whisper integration: `reqwest` multipart to `https://api.openai.com/v1/audio/transcriptions` with model `whisper-1`.

## Environment / Keys
- Set `OPENAI_API_KEY` before running the app. Example (PowerShell):
  - `$env:OPENAI_API_KEY = "<your key>"`


## Next Steps
- Image region capture (Windows-first): overlay UI, capture, temp store, open main window.
- Quick Prompts 1–9: wire mappings, run prompt, insert AI result, close popup.
- Settings view: hotkeys customization, Safe Mode toggle for aggressive copy.
- macOS parity (global shortcut registration and popup position).
- Optional: offline STT model toggle.

## Known Considerations / Risks
- Mic permission: must be allowed in OS privacy settings. Tauri app runs as secure context for MediaRecorder.
- Network/API failures: we show friendly messages in the prompt panel; consider retry and toasts later.
- Telemetry: disabled by default per design.

## Changelog (recent)
- Implemented push-to-talk STT with MediaRecorder and Whisper request.
- Popup remains open during recording; opens prompt on success, failure, or no speech.
- Added `reqwest` (rustls) for backend.
