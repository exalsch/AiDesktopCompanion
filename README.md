# AiDesktopCompanion
### Local STT (Whisper)

- Select Local (Whisper) in Settings → Speech To Text.
- Choose a model preset or paste a custom URL to a `ggml-*.bin` file.
- Click “Prefetch Whisper model” to download with a progress indicator.
- Models are stored at `%APPDATA%/AiDesktopCompanion/models/whisper/` using the file name derived from the URL, allowing multiple models to co‑exist.
- The app transcodes microphone recordings to WAV 16 kHz mono on the frontend to maximize backend compatibility.


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

Optional for Local STT (Whisper):

- CMake (needed by some audio crates)
- LLVM/Clang for bindgen (required to build whisper-rs on Windows)
  - Install: `winget install -e --id LLVM.LLVM`
  - Then set `LIBCLANG_PATH` so bindgen can find `clang.dll`:
    - PowerShell: `setx LIBCLANG_PATH "C:\\Program Files\\LLVM\\bin"`
  - Open a new terminal after setting the environment variable.
  - Tip: run `Test-Path "C:\\Program Files\\LLVM\\bin\\clang.dll"` to verify.


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
# from the repository root
cd app
npm run tauri dev
```

- Run with Local STT (Whisper) enabled:

```powershell
# from the repository root
cd app
npm run tauri -- dev --features local-stt
```

Notes (Local STT):

- On first transcription, the app downloads the selected Whisper model (`ggml-*.bin`) into `%APPDATA%/AiDesktopCompanion/models/whisper/`.
- In Settings → Speech To Text, you can pick a preset (e.g., `base.en`, `small`, `medium`, `large-v3`) and use the “Prefetch Whisper model” button to download in advance with a progress indicator.
- You can override the model URL via `settings.stt_whisper_model_url` or environment `AIDC_WHISPER_MODEL_URL`.

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

- Build the packaged desktop app with Local STT (Whisper):

```powershell
npm run tauri -- build --features local-stt
```

When shipping Local STT, ensure your users meet the Windows prerequisites (LLVM/Clang etc.) noted above.


## CI/CD

Two GitHub Actions workflows are provided under `.github/workflows/`:

- `build-windows.yml` — Builds Windows installers on every push to `main` and uploads the bundles as artifacts.
- `release-windows.yml` — Builds on tag pushes (`v*`) and publishes the installers to a GitHub Release.

Both workflows use caching for npm and Cargo to speed up builds. Windows code signing is supported as an optional step via GitHub Secrets (see below).

### Triggering a release

Create and push a semver-like tag (e.g. `v0.1.3`) to trigger the release workflow:

```powershell
git tag v0.1.3
git push origin v0.1.3
```

The workflow will build the installers and attach all files from `app/src-tauri/target/*/release/bundle/` to the GitHub Release.

### Optional: Windows code signing

If you provide a code signing certificate, the workflows will sign `.exe` and `.msi` artifacts after the build using `signtool`.

Set these GitHub Secrets at the repository level:

- `WINDOWS_CERT_PFX_BASE64` — Base64-encoded `.pfx` certificate content.
- `WINDOWS_CERT_PASSWORD` — Password for the `.pfx` file.
- `WINDOWS_TIMESTAMP_URL` — Optional timestamp server (default: `http://timestamp.digicert.com`).

To produce the base64 string on Windows PowerShell:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes('C:\path\to\cert.pfx'))
```

Notes:

- The workflows import the certificate temporarily on the runner and sign artifacts post-build. This keeps secrets out of the repo and avoids permanent cert installation.
- If you need the inner binaries inside MSIs to be signed during creation, integrate a custom sign command into Tauri bundling. ‼️ This is optional and not configured here.


## Conventional Commits

This repository follows the [Conventional Commits](https://www.conventionalcommits.org/) specification to enable automated release notes.

Commit (or PR title) format:

```
<type>(<optional-scope>): <short summary>

<optional body>

<optional footer(s)>
```

Supported types (non-exhaustive): `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.

Example PR titles / commits:

- `feat(app): add quick action for prompt insertion`
- `fix(tauri): handle clipboard restore failure on Windows`
- `chore(ci): speed up cache for cargo`

Scopes are optional; suggested scopes: `app`, `tauri`, `ci`, `release`, `deps`.

PR Title enforcement:

- The workflow `/.github/workflows/semantic-pr.yml` checks PR titles for Conventional Commits compliance and will warn/fail if they don’t match.

Release notes:

- On tag pushes (e.g. `v0.1.3`), `/.github/workflows/release-windows.yml` generates release notes using Conventional Commits and attaches the installers to the GitHub Release.

## Features

- Quick Actions popup with keyboard/mouse interactions:
  - Prompt (selection capture + action)
  - TTS (open TTS panel with selection)
  - Speech-to-Text push‑to‑talk (hold S / mouse, release to transcribe)
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

Local STT (Whisper) tips:

- Bindgen/Clang errors while compiling `whisper-rs`:
  - Install LLVM via `winget install -e --id LLVM.LLVM` and set `LIBCLANG_PATH` to `C:\\Program Files\\LLVM\\bin`.
  - Open a new terminal, then try `cargo clean` under `app/src-tauri/` followed by the dev/build command with `--features local-stt`.
- Wrong platform bindings / struct size mismatch errors:
  - Ensure you are not forcing “do not generate bindings”. On Windows, bindings must be generated to match the platform.
  - Clean the target dir: `cargo clean` under `app/src-tauri/`.
- Audio decode errors for WebM/Opus captures:
  - The frontend transcodes to WAV 16 kHz mono before sending to the backend when `stt_engine = local`. If you customized this, restore the default behavior in `app/src/components/STTPanel.vue`.


## Security notes

- Do not hardcode API keys or secrets in the repo. Use environment variables or OS‑level secure storage.
- Follow Tauri capability permissions and APIs.


## Contributing

‼️ Contribution guidelines TBD. Open an issue first to discuss substantial changes.


## License

MIT
