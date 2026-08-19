# Select-All Hotkey and Busy Indicator

Design notes for two features added on top of the existing Quick Actions flow.

## 1. Select-All hotkey

### Problem

Running a quick prompt over a whole document takes four steps today: select
everything by hand, press the popup hotkey, wait for the popup, press a digit.
For the common "rewrite this entire mail / note" case the popup adds nothing.

### Behaviour

A second, independently configurable global hotkey:

1. waits for the physical modifier keys of the shortcut to be released (this
   happens before *any* synthetic keystroke, including the Ctrl+C, so the
   `none` mode is not silently broken by a still-held Alt),
2. enlarges the selection in the focused application according to
   `select_all_capture_mode`,
3. captures the selection through the clipboard (the existing copy-restore
   dance, so the user's clipboard survives),
4. runs the quick prompt configured in `select_all_quick_prompt` (1-9),
5. pastes the result back over the selection.

No popup is shown at any point. The result replaces the selection, which the
target application's own undo (Ctrl+Z) can revert.

Step 2 has three modes:

| Mode | Keystroke | Selects |
| --- | --- | --- |
| `ctrl_shift_home` (default) | Ctrl+Shift+Home | Everything from the caret back to the start. |
| `ctrl_a` | Ctrl+A | The whole document. |
| `none` | none | Whatever the user already highlighted. |

`ctrl_shift_home` exists for the correct-what-I-just-typed case. In a chat box,
a comment field or a reply editor, Ctrl+A selects the entire page or the whole
thread, not the draft; extending the selection from the caret to the start of
the field grabs exactly the draft and nothing else.

### Settings

| Key | Type | Default | Meaning |
| --- | --- | --- | --- |
| `select_all_hotkey` | string | `""` | Shortcut in plugin format (`Alt+Shift+R`). Empty disables the feature. |
| `select_all_quick_prompt` | number | `1` | Which quick prompt (1-9) the hotkey runs. Clamped on save. |
| `select_all_capture_mode` | string | `"ctrl_shift_home"` | `ctrl_shift_home`, `ctrl_a` or `none`. Normalized on save and on read, so an unknown value degrades to the default rather than disabling the feature. |

All three are edited in Settings → General. The hotkey field is the same
`HotkeyPicker` component used for the popup hotkey, so availability is verified
against the OS before the setting is saved.

### Code

- `app/src/hotkeys.ts` — two named slots (`popup`, `selectAll`) share one
  registration/re-registration path. Each slot dispatches its own DOM event.
- `app/src/main.ts` — reacts to `ai-desktop:hotkey-select-all` by reading the
  configured index and invoking `run_quick_prompt_select_all`.
- `app/src-tauri/src/quick_prompts.rs` — `run_quick_prompt` and
  `run_quick_prompt_select_all` are two thin wrappers over one
  `run_quick_prompt_inner(.., pre: PreSelect)`; selection capture, prompt
  composition and the chat-completion call are shared helpers.
  `run_quick_prompt_select_all` reads `select_all_capture_mode` itself instead
  of taking it as an argument, so the setting stays the single source of truth
  no matter which window invokes the command.

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
