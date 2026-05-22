# AiDesktopCompanion — Command Mode (design)

**Date:** 2026-05-21
**Status:** Proposed, pre-implementation
**Target:** `app/src-tauri` (Tauri 2 / Rust) + `app/src` (Vue 3). Windows first; macOS deferred to match the platform stance in `Overview_AiDesktopCompanion.md`.

## Problem

AiDesktopCompanion currently turns speech into text and inserts it into the focused application (STT path in `quick_actions.rs` → `stt_transcribe` → frontend paste). That is great for dictation but it does not let the user *act* on speech: there is no way to say "open my morning briefing" and have that route to a script, an AI agent, or a terminal command. Every voice utterance ends up as pasted characters, regardless of intent.

A sibling project — Handy, on branch `feat/command-mode` — solved this with a second voice path that records and transcribes exactly like normal STT, but pipes the raw transcript to a user-owned script instead of pasting. The script is the versatility layer. That contract has proven enough to drive a "Rob, …" voice-to-Claude pipeline in personal use (see `%APPDATA%\com.pais.handy\hooks\command.ps1`). This spec ports that capability to AiDesktopCompanion.

## Goal

Add **Command Mode** as a fifth Quick Action — keyboard `C` — alongside the existing P/T/S/I. Activating `C` records and transcribes using the existing pipeline, then — instead of pasting — pipes the transcript to a user-selected script in `%APPDATA%\AiDesktopCompanion\hooks\`. The script can route the utterance to an LLM, a shell command, a terminal tab, an MCP server invocation, anything the user wants. Keeping the routing in a script (not in the app) keeps the feature versatile and avoids hardcoding agent integrations.

## Non-goals (v1)

- **Paste-back.** AiDesktopCompanion does not capture the script's stdout and does not insert it into the focused app. If the script wants to produce visible output it inserts text itself (the existing `insert_text_into_focused_app` command can be reused from a child process via `wt`/`pwsh`/etc., or the script can use its own clipboard/SendKeys logic).
- **Queueing.** Overlapping command invocations are not queued; while a script is running, the `C` action in the popup is **disabled** and re-pressing `C` is a no-op.
- **Dedicated global hotkey.** Command Mode is reached only via the Quick Actions popup. There is no second global accelerator.
- **Result-display widget.** No UI panel to show what the script returned. Possible v2.
- **No changes** to the normal STT flow (`stt_transcribe`, `stt_post_process_text`) or to the existing P/T/S/I actions.
- **No bundled scripts.** AiDesktopCompanion ships with no default `hooks/command*` file. The settings section provides a button to write one on demand.

## Design

### Trigger — fifth Quick Action

The Quick Actions popup (`quick-actions` webview window) gains a fifth button labelled **Command** with the fixed mnemonic `C`. Layout matches the existing horizontal P/T/S/I row; the button is hidden entirely when `command_enabled` is `false` so users who never set it up don't see an empty slot.

Behavior of `C`:

1. The popup verifies command is not already running (see "Block re-trigger" below). If it is, the button is rendered disabled and the press is a no-op.
2. Recording starts using the **same MediaRecorder path** the `S` action uses today (WAV 16 kHz mono via the existing frontend transcoder). Push-to-talk vs toggle behavior mirrors the user's STT mode setting — no separate setting.
3. On stop, the audio is sent to the existing `stt_transcribe` Tauri command with `apply_post_process: false`. Command Mode uses the **raw transcript** (`original_text`) — the script is itself the processing layer; an LLM should not rewrite "open my briefing" before the script sees it.
4. The popup calls a new Tauri command `run_command_hook(transcript, context)`. The backend spawns the configured script, writes the transcript to its stdin, closes the pipe, returns immediately.
5. The popup closes (consistent with "closes immediately after any action" in the Quick Actions spec). The user's only signal that the script is still running is that re-opening the popup shows a disabled `C` button — see next section.

### Block re-trigger while running

A `COMMAND_RUNNING: AtomicBool` lives in the new `command_hook` module. Set true when a child is spawned, cleared by the watcher thread when the child exits or is killed by timeout.

The popup learns about this state in two ways:

- **On open / mount:** the popup calls a new lightweight `command_is_running()` Tauri command synchronously alongside the existing prep calls. If true, the `C` button mounts in disabled state with a "Command running…" tooltip.
- **Live update while open:** the popup listens for a `command:state` Tauri event (`{ state: "running" | "idle" }`) so a script finishing while the popup is open re-enables the button without requiring a reopen.

When `C` is disabled, the `1`–`9` Quick Prompt keys and the other letter mnemonics (P/T/S/I) continue to work — only the `C` path is blocked.

### Pipeline

Record → transcribe → run hook. The transcription branch is the existing `stt_transcribe` flow, used unchanged. The new code starts at "got a raw transcript, hand it to a hook":

1. Resolve the configured script (see "Hook resolution" below). If none is configured or the file is missing, emit a toast ("No command script configured — see Settings → Command Mode") and stop.
2. Build a `CommandContext` (active app, clipboard, selection — see "Context env vars" below).
3. Spawn the child **detached** (`Stdio::null()` for stdout/stderr by default; both also tee'd to a rotating log file in `%APPDATA%\AiDesktopCompanion\logs\command-hook.log`).
4. Write the transcript to the child's stdin and close the pipe.
5. Start the watcher thread and return — do not `wait_with_output()`.

### Hook resolution

Scripts live in `%APPDATA%\AiDesktopCompanion\hooks\`. The settings dropdown stores **only the filename** (e.g. `"command.ps1"`, `"rob.ps1"`) in `command_active_script`; the backend resolves it back to the full path at spawn time.

Supported extensions on Windows (v1): `.cmd`, `.bat`, `.ps1`, `.exe`. Dispatch by extension:

- `.ps1` → `powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "<full path>"`
- `.cmd` / `.bat` → `cmd.exe /c "<full path>"`
- `.exe` → spawned directly
- macOS / Linux (deferred): bare executable with the exec bit set.

If `command_active_script` is unset or points to a missing file, command mode is treated as "no script configured" — the toast above fires; no spawn is attempted.

### Discovering scripts

A new Tauri command `list_command_scripts()` returns the filenames of every file in `%APPDATA%\AiDesktopCompanion\hooks\` whose extension is in the supported list. Hidden files and subdirectories are skipped. The settings dropdown is built from this list; an empty list means "no scripts yet" and the dropdown is disabled with the placeholder text "(none — click *Create default script* to start)".

### Create default script

A new Tauri command `create_default_command_script()` writes a starter file at `hooks/command.ps1` (only if it doesn't already exist), then returns the resulting filename. The frontend selects the new filename in the dropdown automatically and persists `command_active_script`.

Default template (illustrative):

```powershell
# AiDesktopCompanion — Command Mode hook (default template)
# Receives the transcribed utterance on stdin. Edit this file to route your
# voice commands wherever you want (LLM, shell, terminal tab, etc.).

$ErrorActionPreference = 'Continue'
$log = "$env:APPDATA\AiDesktopCompanion\logs\command-hook.log"
$transcript = [Console]::In.ReadToEnd().Trim()
Add-Content -Path $log -Value "[$(Get-Date -Format o)] in=$transcript"
Add-Content -Path $log -Value "  active=$env:AIDC_ACTIVE_APP clip=($($env:AIDC_CLIPBOARD.Length) chars) sel=($($env:AIDC_SELECTED_TEXT.Length) chars)"

# Example: forward utterances that start with "Rob" to a Claude session in a
# Windows Terminal tab named ROB. Delete this block to start with a clean slate.
if ($transcript -match '^(?i)Rob\b[\s,\.;:!?-]*(.+)$') {
    $msg = $Matches[1].Trim()
    # ... your routing logic ...
}

exit 0
```

The exact body is open to taste — point being, the button writes a working file the user can verify with a single voice test.

### Context env vars

The script reads the transcript on stdin. Additional context is supplied via environment variables, prefixed `AIDC_` to match the existing `AIDC_WHISPER_MODEL_URL` convention:

- `AIDC_TRANSCRIPT` — same content as stdin, for scripts that find env vars easier than stdin.
- `AIDC_ACTIVE_APP` — process name (e.g. `chrome.exe`, `WindowsTerminal.exe`) of the foreground window at trigger time, resolved on Windows from the existing `LAST_FOREGROUND` HWND in `quick_actions.rs` plus `GetWindowThreadProcessId` + `QueryFullProcessImageNameW`.
- `AIDC_CLIPBOARD` — current clipboard text, captured via `arboard::Clipboard::get_text()`. Best-effort; empty if unavailable.
- `AIDC_SELECTED_TEXT` — selected text in the foreground app *before* the popup was opened. The popup already captures this for its preview via `focus_prev_then_copy_selection` (aggressive copy-restore against `LAST_FOREGROUND`); the same captured value is forwarded into the hook's env so we don't touch the clipboard a second time.

Empty values are passed as empty strings (not absent vars) to keep the script's null-handling simple.

### Concurrency

- `COMMAND_RUNNING: AtomicBool` guards command mode against itself.
- The **regular STT path stays fully usable** while a command script runs — the command process is detached and the AtomicBool is independent from STT state.
- The popup's other actions (P, T, S, I, 1–9) remain unaffected while `C` is running.

### Lifecycle / watcher thread

After spawning the detached child, the runner starts a lightweight watcher thread that:

1. Calls `child.wait()` (or `kill()` on timeout).
2. Clears `COMMAND_RUNNING`.
3. Emits `command:state` event with `{ state: "idle" }` so any open popup re-enables `C`.
4. Appends exit code + stderr tail to `command-hook.log`.

A configurable timeout (`command_hook_timeout_secs`, default 120) kills hung children so `C` can never get permanently disabled. The kill uses `child.kill()`; we don't try to reap subprocesses the script itself spawned (that's the script author's problem — they can `Start-Process` to fully detach long-running work, mirroring how Handy's `command.ps1` launches a new `wt` and exits).

### Settings — new "Command Mode" section

The Settings panel gains a new section, placed near the STT settings since they share the pipeline. Contents:

- **Short description** (static text):
  > Command Mode adds a fifth Quick Action — press `C` in the popup to record a voice command. Instead of pasting the transcript, AiDesktopCompanion runs a script of your choice from `%APPDATA%\AiDesktopCompanion\hooks\`, passing the transcript on stdin. Use this to route speech to AI agents, terminal commands, or any other automation.

- **Enable Command Mode** — checkbox bound to `command_enabled`. When unchecked, the `C` button is hidden from the popup and the dropdown / create-default button are visibly disabled. Default `false`.

- **Active script** — dropdown bound to `command_active_script`. Options are filenames returned by `list_command_scripts()`. A "Refresh" affordance (small reload icon next to the dropdown) re-runs the listing without reopening Settings. Disabled when the hooks directory is empty.

- **Create default script** — button. Calls `create_default_command_script()`, refreshes the dropdown, selects the new filename, persists. Replaced with "Open hooks folder" once at least one script exists (clicking opens the folder in Explorer via Tauri's shell plugin).

- **Hook timeout (seconds)** — number input bound to `command_hook_timeout_secs`, default 120, range 5–3600.

Settings keys persisted via the existing `config` module:

| Key                          | Type   | Default | Notes                                                    |
| ---------------------------- | ------ | ------- | -------------------------------------------------------- |
| `command_enabled`            | bool   | `false` | Master switch; hides the `C` button when off.            |
| `command_active_script`      | string | `""`    | Filename inside `hooks/`. Empty = no script configured.  |
| `command_hook_timeout_secs`  | number | `120`   | Watcher kill timeout.                                    |

### Error handling

Every failure path logs and clears `COMMAND_RUNNING`; none disrupts normal STT or the popup's other actions:

- `command_enabled` is `false` → `C` button is hidden; no path to execute.
- No script configured / file missing → toast: "No command script configured — see Settings → Command Mode". No spawn.
- Spawn fails → log error, toast, clear flag, emit idle event.
- Transcription fails → existing STT error path applies; do not spawn the hook.
- Empty transcript (post-VAD) → log info, do not spawn the hook, re-enable `C` immediately.
- Timeout exceeded → kill child, log warning, clear flag, emit idle event.
- Non-zero exit → log error with exit code + stderr tail, clear flag, emit idle event. No toast by default (avoid noise for scripts that intentionally exit non-zero).

## Affected code

- `app/src-tauri/src/command_hook.rs` *(new)* — `COMMAND_RUNNING`, `list_command_scripts`, `create_default_command_script`, `resolve_script_path`, `CommandContext` builder, detached spawn, watcher with timeout, `command:state` event emission, `command_is_running` getter.
- `app/src-tauri/src/lib.rs` — register the new Tauri commands (`run_command_hook`, `command_is_running`, `list_command_scripts`, `create_default_command_script`), `mod command_hook;` plus invoke handler entries.
- `app/src-tauri/src/config.rs` — defaults + getters for `command_enabled`, `command_active_script`, `command_hook_timeout_secs`.
- `app/src-tauri/src/quick_actions.rs` — small helper to expose the captured-selection string into `CommandContext` without duplicating the copy-restore logic (factor or expose via static).
- `app/src/components/QuickActionsPopup.vue` (or equivalent) — add the `C` button to the layout, mount-time `command_is_running()` check, `command:state` listener, recording flow that ends in `run_command_hook` instead of `insert_text_into_focused_app`.
- `app/src/components/SettingsPanel.vue` (or wherever STT settings live) — add the Command Mode section described above.
- `app/src/composables/useCommandMode.ts` *(new)* — small wrapper for invoking the Tauri commands and exposing reactive `isRunning` to the popup.

No new heavy Rust dependencies expected; reuse `arboard`, `enigo`, `windows`. The shell plugin (already enabled for other features) is reused for the "Open hooks folder" affordance.

## Reference hook (illustrative, not bundled)

A working Windows hook that logs every utterance and forwards utterances beginning with "Rob" to a Windows Terminal tab named "ROB" (auto-spawning `wt -d C:\rob --title ROB claude` if missing) is the reference implementation. See Handy's `%APPDATA%\com.pais.handy\hooks\command.ps1`; it ports verbatim to AiDesktopCompanion once paths and env-var prefixes are switched (`HANDY_*` → `AIDC_*`, `com.pais.handy` → `AiDesktopCompanion`).

## Testing

- **Rust unit tests:** `list_command_scripts` filters by extension and ignores hidden / directory entries; `resolve_script_path` rejects path traversal in `command_active_script`; `CommandContext` env assembly from a known fixture; watcher kills a `sleep 5` child with a 1 s timeout and emits the idle event.
- **Popup integration test:** with `command_enabled=true` and a configured script, pressing `C` records, transcribes, and spawns the script; verify stdin and env vars arrive correctly via a logging script.
- **Re-trigger block test:** spawn a script that sleeps 10 s; reopen the popup mid-run — `C` is visibly disabled, pressing it is a no-op, P/T/S/I and `1`–`9` still work. After the script exits, reopening (or staying open via the live event) shows `C` re-enabled.
- **No-script test:** remove `command_active_script`; press `C` — toast appears, no spawn, no crash.
- **Disabled test:** `command_enabled=false`; the `C` button is absent from the popup; settings dropdown / create button disabled.
- **Create-default test:** click "Create default script" with empty hooks dir → file created, dropdown refreshes, new filename auto-selected and persisted.
- **Timeout test:** script that sleeps past the timeout — child killed, idle event emitted, button re-enables.

## Open questions

1. **STT engine for command mode.** Should command mode always use a fast model (e.g. force Whisper `base` regardless of user setting) to keep latency low for short voice commands, or honor the user's STT engine setting? v1 plan: honor the setting; revisit if `large-v3` latency proves painful in practice.
2. **Toast on non-zero exit.** Off by default per the spec above. Revisit if early users prefer a visible failure signal.
3. **macOS port.** Deferred until the rest of the Windows experience is stable, matching the project's existing stance.

## v1 acceptance criteria

- With `command_enabled=true` and a configured script, pressing `C` in the popup records the user, transcribes, and invokes the script with the raw transcript on stdin and the env vars populated, within ~1 s of speech-end (excluding STT model time).
- While a script is running, reopening the popup shows the `C` button disabled; pressing it is a no-op; other actions (P/T/S/I and 1–9) work normally.
- The button re-enables automatically when the script exits or the timeout kills it.
- The Settings section provides the description, the enable checkbox, the script dropdown, the create-default button, and the timeout field — and changes round-trip through the existing settings store.
- Removing or renaming the configured script while it's selected surfaces a clear toast on next `C` press without crashing.
- The existing P/T/S/I Quick Actions and the normal STT pipeline are unaffected.
