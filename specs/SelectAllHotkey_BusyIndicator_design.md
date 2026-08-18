# Select-All Hotkey and Busy Indicator

Design notes for two features added on top of the existing Quick Actions flow.

## 1. Select-All hotkey

### Problem

Running a quick prompt over a whole document takes four steps today: select
everything by hand, press the popup hotkey, wait for the popup, press a digit.
For the common "rewrite this entire mail / note" case the popup adds nothing.

### Behaviour

A second, independently configurable global hotkey:

1. waits for the physical modifier keys of the shortcut to be released,
2. sends Ctrl+A to the focused application,
3. captures the selection through the clipboard (the existing copy-restore
   dance, so the user's clipboard survives),
4. runs the quick prompt configured in `select_all_quick_prompt` (1-9),
5. pastes the result back over the selection.

No popup is shown at any point. The result replaces the whole document, which
the target application's own undo (Ctrl+Z) can revert.

### Settings

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `select_all_hotkey` | string | `""` | Shortcut in plugin format (`Alt+Shift+R`). Empty disables the feature. |
| `select_all_quick_prompt` | number | `1` | Which quick prompt (1-9) the hotkey runs. Clamped on save. |

Both are edited in Settings → General. The hotkey field is the same
`HotkeyPicker` component used for the popup hotkey, so availability is verified
against the OS before the setting is saved.

### Code

- `app/src/hotkeys.ts` — two named slots (`popup`, `selectAll`) share one
  registration/re-registration path. Each slot dispatches its own DOM event.
- `app/src/main.ts` — reacts to `ai-desktop:hotkey-select-all` by reading the
  configured index and invoking `run_quick_prompt_select_all`.
- `app/src-tauri/src/quick_prompts.rs` — `run_quick_prompt` and
  `run_quick_prompt_select_all` are two thin wrappers over one
  `run_quick_prompt_inner(.., select_all: bool)`; selection capture, prompt
  composition and the chat-completion call are shared helpers.

### Why the modifier wait

Global shortcuts fire on key-down, so the modifiers are usually still held when
the handler runs. Synthesizing Ctrl+A at that moment reaches the target app as
Ctrl+Alt+A (or whatever the shortcut was). `wait_for_modifiers_released` polls
`GetAsyncKeyState` for up to 900 ms, then waits another 40 ms for the target app
to process the key-up events.

## 2. Busy indicator

### Problem

Quick prompts triggered by a hotkey, TTS on the selection and STT transcription
all run with no visible window: the popup hides itself first and the main window
may never be opened. A slow OpenAI call, or the 120 s client timeout, is
indistinguishable from "nothing happened". Errors were dropped entirely - the
popup calls `run_quick_prompt` as fire-and-forget.

### Behaviour

A small always-on-top pill in the bottom-right corner of the monitor under the
cursor shows:

- what is running (`Quick Prompt 3`, `Text to speech`, `Transcribing speech`),
- elapsed time, with a "still working" hint after 20 s,
- the error message if the operation fails, auto-hiding after 8 s or on click.

It is suppressed while the main window is visible and focused, because that
window renders its own busy and error state. `show_busy_indicator` (default
`true`) turns it off entirely.

### Code

- Window `busy-indicator` (`?window=busy`) is declared statically in
  `tauri.conf.json`: transparent, undecorated, `skipTaskbar`, `focus: false`.
  `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` is applied at startup so showing it
  never steals focus from the app the user is typing in.
- `app/src-tauri/src/busy.rs` — `start` / `finish` / `with_indicator`, a nesting
  counter so overlapping operations don't hide each other, and the current state
  behind `busy_get_state` for when the webview mounts after the operation began.
- `app/src/components/BusyIndicator.vue` — listens for `busy:state`, renders the
  pill, dismisses via `busy_hide`.

### Wired into

`run_quick_prompt`, `run_quick_prompt_select_all`, `tts_selection`,
`stt_transcribe`. The popup's inline preview flows keep their own spinner and
are deliberately not wrapped.
