**Project Overview: AI Desktop Companion**

The AI Desktop Companion is a new multi-operating system desktop application designed for both Windows and Mac, utilizing the Tauri framework with Vue.js as frontend framework. The application aims to enhance user productivity by integrating various AI functionalities through a simple interface.

**Functionality:**

1. **Global Hotkey Activation:**
   - The application listens for a configurable global hotkey. When the hotkey is pressed, a small pop-up will appear next to the cursor location.
   - **Cross-platform baseline:** Global hotkey registration and the ability to open/close the popup must work on both Windows and macOS from the beginning. Other features can be delivered Windows-first.

2. **Features Accessible via Pop-Up:**
   - **Prompt Suggestions:** The application will use any text that was selected in the active application before the hotkey was triggered as a suggestion for the main prompt.
   - **Text-to-Speech:** Selected text will be sent to a speech engine (e.g., OpenAI) for vocal playback.
   - **Speech-to-Text:** Activating this feature starts a recording that will be sent to an API (like OpenAI) for transcription.
   - **Image Capture:** Users can select a region of their screen to capture a screenshot. This image will also be sent to the main prompt interface for further use.
 
   - **Quick Actions Popup (P/T/S/I):** A small, borderless popup appears near the cursor on global hotkey. It provides four actions with fixed mnemonic keys: `P` (Prompt), `T` (TTS), `S` (STT), `I` (Image). Design matches `specs/QuickActions_Popup_design_mockup.html` (horizontal layout with icons).
     - **Platform:** Windows first; keep macOS in mind.
     - **Behavior:** Closes immediately after any action, and on click outside (losing focus). No idle auto-dismiss timer.
     - **Hotkeys while open:** P/T/S/I are fixed and active only when the popup is focused/visible (button labels underline these letters). Number keys `1–9` trigger configured Quick Prompts and also close the popup after completion, inserting the AI result.
     - **Selection acquisition:** Default is Aggressive copy-restore (simulate Ctrl+C, read clipboard, then restore previous clipboard). Optional Safe mode uses current clipboard only.
     - **Selection preview (optional):** Default ON. Show up to 200 characters of the current selection inside the popup for context, with a count badge. Can be disabled in Settings.
     - **Prompt (P):** Opens the main window with the selection prefilled in the Prompt Section; popup closes.
     - **TTS (T):** Sends selection to the configured provider (default OpenAI). Default voice: `nova`; default speed: `0.25`. If no selection, read clipboard; if clipboard empty, show a toast indicating no text.
     - **STT (S):** Recording can be "toggle to start/stop" or "push-to-talk (hold S)" selectable in Settings (Windows may constrain key-up detection globally). Default model: Whisper `base`. Offline mode supported via downloadable local models.
     - **Image (I):** Region selection with crosshair + drag rectangle; grayish screen overlay; 2px red outline; cursor not included. Image is kept in memory/temp; when user chooses to preserve a conversation, the image is saved alongside it. After capture, open the main window with the image attached so the user can add a prompt.
     - **Accessibility & themes:** English initially; prepare for localization. Themes: Light, Dark, High Contrast.
     - **Privacy:** No persistence by default. Optional History (conversations, transcripts, generated audio) can be enabled; default OFF.
     - **Telemetry:** None (disabled).
     - **UAC/admin windows:** If overlay is blocked, show a notification/toast.
     - **Support:** Windows 10/11 tested; macOS current versions planned.

3. **Main Window Components:**
  - **Prompt Section:** Displays the prompt text and suggested modifications.
  - **Text-to-Speech Section:** Allows users to input text manually, with controls to play, stop, download, adjust speed, and select voice options from the audio provider.
  - **Speech-to-Text Section:** Transcribes audio recordings into text, which can then be copied or used as a prompt.
  - **Tools Panel (MCP):** Sidebar/panel to enable tools for the current conversation or prompt.
    - States: Disabled (default), Enabled with 0 tools selected, Enabled with N tools selected.
    - Actions: Enable/disable per tool, view tool description, open server status, retry connect.
    - Persistence: Option to remember last-used tool set per device; can be disabled in Settings.

4. **Settings Configuration:**
   - Users can set the API provider URL (defaulting to OpenAI) and API key.
   - Options for configuring the global hotkey, auto-start behaviors (e.g., automatic recording for speech-to-text after the hotkey is triggered), and the direct speech-to-text hotkey will be available.
   - Users can select the speech-to-text engine and configure the Whisper model.
   - An autostart option will be provided for Windows, and potentially for Mac as well if possible.
   - Additional settings will be available for system prompts and excluding AI thoughts in responses.

5. **Quick Prompts:**
  - Users can define quick prompt replacements in the settings, mapped to keys 1-9 (e.g., pressing the global hotkey then hitting “2” will execute the associated quick prompt on the selected text).

6. **MCP Tools Integration:**
  - **Purpose:** Allow adding Model Context Protocol (MCP) servers and selecting their tools for use within prompts and quick prompts.
  - **Server configuration:** Add/edit/remove servers with fields: name, command, args, (optional) working directory, environment variables. StdIO transport only.
  - **Tool selection per prompt:** In the main window, a Tools panel lets users enable specific tools from connected servers for the current prompt/session. Persist last-used set optionally.
  - **Permissions & security:** Tools are disabled by default until explicitly enabled. Show first-use confirmation and per-call errors clearly. No telemetry.
  - **Cross-platform:** Paths validated on Windows first; keep macOS path semantics in mind.
  - **Performance:** Lazy-connect servers on demand; reconnect with backoff.
  - **History:** When History is enabled, store which tools were used with each conversation.

7. **Conversation Model:**
  - **Default behavior:** Interactions in the Prompt section occur within a conversation thread that retains context across turns.
  - **New Conversation:** The main window provides a "New conversation" action that clears context and starts a fresh thread.
  - **Prompt action from popup (P):** By default, appends the captured selection/question to the current conversation. A setting allows changing this to "always start a new conversation" when using P.
  - **History linkage:** When optional History is enabled, each saved conversation preserves its messages, attached images, transcripts, and tool usage metadata.
