# AiDesktopCompanion

A Windows-first desktop companion built with Vue 3 + Vite and Tauri 2. It provides fast "Quick Actions" for Prompting, Text‑to‑Speech (TTS), Speech‑to‑Text (STT), and an Image capture overlay.


## Repository layout

- `app/` — Frontend (Vue 3) and Desktop shell (Tauri 2)
  - `src/` — UI code
  - `src-tauri/` — Rust side, Tauri config and capabilities
- `specs/` — Design docs and implementation plans


## Prerequisites (Windows)

- Node.js LTS (18+ recommended)
- Rust (stable) via rustup: https://rustup.rs/
- Visual Studio Build Tools 2022
  - Workload: "Desktop development with C++"
  - Includes the Windows 10/11 SDK
- Microsoft Edge WebView2 Runtime (usually preinstalled)
- Tauri prerequisites for Windows: https://tauri.app/start/prerequisites/


## Setup

Install dependencies in the `app/` folder:

```powershell
# from the repository root
cd app
npm ci   # or: npm install
```


## Development

- Run the desktop app in development mode (recommended):

```powershell
npm run tauri dev
```

- Run only the web dev server (for UI-only iteration):

```powershell
npm run dev
```


## Build

- Build the frontend bundle:

```powershell
npm run build
```

- Build the packaged desktop app:

```powershell
npm run tauri build
```

Artifacts are produced under `app/src-tauri/target/` (e.g. `release/`).


## Features

- Quick Actions popup with keyboard/mouse interactions:
  - Prompt (selection capture + action)
  - TTS (open TTS panel with selection)
  - STT push‑to‑talk (hold S / mouse, release to transcribe)
  - Image capture overlay
- Aggressive clipboard copy‑restore for reliable text insertion
- Auto‑sizing popup and robust window focus management


## Troubleshooting

- Missing script errors like `npm error Missing script: "build"`:
  - Ensure you are in the `app/` directory before running npm scripts.
- Rust toolchain or linker errors:
  - Install Rust via rustup and ensure VS Build Tools (with Desktop C++) are installed.
- WebView2 not found:
  - Install the Microsoft Edge WebView2 Runtime.
- TypeScript errors during `npm run build`:
  - Run the build from `app/` and check the reported file paths (`.vue`, `.ts`).
- Path issues in PowerShell:
  - Use quotes around paths with spaces and prefer running from the project root + `cd app`.


## Security notes

- Do not hardcode API keys or secrets in the repo. Use environment variables or OS‑level secure storage.
- Follow Tauri capability permissions and APIs.


## Contributing

‼️ Contribution guidelines TBD. Open an issue first to discuss substantial changes.


## License

MIT
