**Project Overview: AI Desktop Companion**

The AI Desktop Companion is a new multi-operating system desktop application designed for both Windows and Mac, utilizing the Tauri framework with Vue.js as frontend framework. The application aims to enhance user productivity by integrating various AI functionalities through a simple interface.

**Functionality:**

1. **Global Hotkey Activation:**
   - The application listens for a configurable global hotkey. When the hotkey is pressed, a small pop-up will appear next to the cursor location.

2. **Features Accessible via Pop-Up:**
   - **Prompt Suggestions:** The application will use any text that was selected in the active application before the hotkey was triggered as a suggestion for the main prompt.
   - **Text-to-Speech:** Selected text will be sent to a speech engine (e.g., OpenAI) for vocal playback.
   - **Speech-to-Text:** Activating this feature starts a recording that will be sent to an API (like OpenAI) for transcription.
   - **Image Capture:** Users can select a region of their screen to capture a screenshot. This image will also be sent to the main prompt interface for further use.

3. **Main Window Components:**
   - **Prompt Section:** Displays the prompt text and suggested modifications.
   - **Text-to-Speech Section:** Allows users to input text manually, with controls to play, stop, download, adjust speed, and select voice options from the audio provider.
   - **Speech-to-Text Section:** Transcribes audio recordings into text, which can then be copied or used as a prompt.

4. **Settings Configuration:**
   - Users can set the API provider URL (defaulting to OpenAI) and API key.
   - Options for configuring the global hotkey, auto-start behaviors (e.g., automatic recording for speech-to-text after the hotkey is triggered), and the direct speech-to-text hotkey will be available.
   - Users can select the speech-to-text engine and configure the Whisper model.
   - An autostart option will be provided for Windows, and potentially for Mac as well.
   - Additional settings will be available for system prompts and excluding AI thoughts in responses.

5. **Quick Prompts:**
   - Users can define quick prompt replacements in the settings, mapped to keys 1-7 (e.g., pressing the global hotkey then hitting “2” will execute the associated quick prompt on the selected text).
