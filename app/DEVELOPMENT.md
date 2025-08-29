# Development Guide

This project is a Tauri v2 + Vue 3 app. Use Node.js (LTS), Rust (stable), and the Tauri CLI.

## Prerequisites
- Node.js (LTS)
- Rust toolchain (stable)
- Tauri CLI (installed via `npm i -D @tauri-apps/cli` already in this repo)
- Windows: WebView2 Runtime, Visual Studio Build Tools (C++), .NET Desktop runtime per Tauri docs

## Install and Run (web dev server)
```powershell
npm i
npm run dev
```
- Opens Vite dev server at http://localhost:5173 with HMR.

## Run as Desktop App (Tauri dev)
```powershell
# Pass through args after --
npm run tauri dev
```
- Launches the Tauri desktop app with HMR for the frontend.

## Build
```powershell
npm run build  # Vite build + type check
# Packaging via Tauri (later): npm run tauri build
```

## Environment
Set required keys before running (PowerShell example):
```powershell
$env:OPENAI_API_KEY = "<your key>"
```
Optional:
```powershell
$env:OPENAI_CHAT_MODEL = "gpt-4o-mini"
```

## UI Styles
The app supports multiple UI styles with per-style CSS.

- Base CSS: `app/src/style.css` (always loaded)
- Style CSS (main window only): injected dynamically by `app/src/App.vue`
- Current styles: `sidebar` (default) and `tabs`
- Files:
  - `app/src/styles/sidebar/style.css`
  - `app/src/styles/tabs/style.css`

Add a new style:
1) Create `app/src/styles/<styleName>/style.css`
2) In `app/src/App.vue` import with `?url` and extend the map:
```ts
import myStyleUrl from './styles/myStyle/style.css?url'
const styleCssMap = { sidebar: sidebarStyleUrl, tabs: tabsStyleUrl, myStyle: myStyleUrl }
```
3) (Optional) Add `<option value="myStyle">My Style</option>` to the Settings UI, and extend the TypeScript union for `ui_style`.
4) Switch UI Style in Settings to test; HMR should update without reload.

Notes:
- The loader only runs in the main window. Quick Actions and the Capture Overlay skip per-style CSS by design.
- See `specs/UI_Styles.md` for detailed guidance.

## System Tray and Close-to-Tray
- Implemented in Rust: `app/src-tauri/src/lib.rs`
- Tray menu: Show (brings main window to front), Exit (quits app)
- Left-click tray icon shows/focuses the main window
- Close main window ➜ hides to tray (prevents app exit)

## Troubleshooting
- If CSS does not apply, check for `<link id="theme-style-css">` in DevTools
- Verify `OPENAI_API_KEY` is set for API-backed features
- Windows-specific build toolchain issues: consult Tauri Windows setup docs
