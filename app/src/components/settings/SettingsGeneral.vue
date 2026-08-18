<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import HotkeyPicker from './HotkeyPicker.vue'

const props = defineProps<{
  settings: any
  models: { list: string[]; loading: boolean; error: string | null }

  onSave: () => void
  onRefreshModels: () => void
  onClearConversations: () => void
}>()

const showApiKey = ref(false)

// Quick prompt titles for the select-all hotkey dropdown, so the user picks a
// recognisable prompt instead of a bare number.
const quickPromptLabels = ref<string[]>([])

function shorten(text: string): string {
  const t = (text || '').replace(/\s+/g, ' ').trim()
  if (!t) return '(empty)'
  return t.length > 48 ? `${t.slice(0, 45)}…` : t
}

onMounted(async () => {
  try {
    const data = await invoke<any>('get_quick_prompts')
    quickPromptLabels.value = Array.from({ length: 9 }, (_, i) => shorten(String(data?.[String(i + 1)] || '')))
  } catch (err) {
    console.warn('[settings] quick prompt titles unavailable', err)
    quickPromptLabels.value = Array.from({ length: 9 }, () => '')
  }
})
</script>

<template>
  <div class="settings-section">
    <div class="settings-title">General Settings</div>

    <div class="settings-row col">
      <label class="label">Global Hotkey</label>
      <HotkeyPicker v-model="props.settings.global_hotkey" />
      <div class="settings-hint">Opens the Quick Actions popup. Example: Alt + Shift + A. Leave all empty to disable. Current: <code>{{ props.settings.global_hotkey || 'disabled' }}</code></div>
    </div>

    <div class="settings-row col">
      <label class="label">Select-All Hotkey</label>
      <HotkeyPicker v-model="props.settings.select_all_hotkey" />
      <div class="settings-hint">
        Selects all text in the focused application (Ctrl + A), runs the quick prompt below on it and pastes the result back over it - no popup.
        Leave all empty to disable. Current: <code>{{ props.settings.select_all_hotkey || 'disabled' }}</code>
      </div>
    </div>

    <div class="settings-row col">
      <label class="label">Select-All Quick Prompt</label>
      <select v-model.number="props.settings.select_all_quick_prompt" class="input" :disabled="!props.settings.select_all_hotkey">
        <option v-for="n in 9" :key="'sa-qp-'+n" :value="n">{{ n }} - {{ quickPromptLabels[n - 1] || '(empty)' }}</option>
      </select>
      <div class="settings-hint">Which of the nine quick prompts the Select-All hotkey runs. Edit the prompts under Settings → Quick Prompts.</div>
    </div>

    <div class="settings-title">AI Provider</div>
    <div class="settings-row col">
      <label class="label">OpenAI API Key</label>
      <div class="row-inline">
        <input
          :type="showApiKey ? 'text' : 'password'"
          v-model="props.settings.openai_api_key"
          class="input"
          placeholder="sk-..."
          autocomplete="off"
          spellcheck="false"
        />
        <button class="btn ghost" @click="showApiKey = !showApiKey">{{ showApiKey ? 'Hide' : 'Show' }}</button>
      </div>
    </div>

    <div class="settings-row col">
      <label class="label">Model</label>
      <div class="row-inline">
        <select v-model="props.settings.openai_chat_model" class="input">
          <option v-if="!props.models.list.includes(props.settings.openai_chat_model)" :value="props.settings.openai_chat_model">{{ props.settings.openai_chat_model }} (current)</option>
          <option v-for="m in props.models.list" :key="m" :value="m">{{ m }}</option>
        </select>
        <button class="btn" :disabled="props.models.loading" @click="props.onRefreshModels">{{ props.models.loading ? 'Fetching…' : 'Fetch Models' }}</button>
      </div>
      <div v-if="props.models.error" class="settings-hint error">{{ props.models.error }}</div>
    </div>

    <div class="settings-row col">
      <label class="label">Tokenizer</label>
      <div class="row-inline">
        <select v-model="props.settings.tokenizer_mode" class="input" style="max-width: 220px;">
          <option value="approx">Approximate (fast, lightweight)</option>
          <option value="tiktoken">Tokenizer (more accurate)</option>
        </select>
      </div>
      <div class="settings-hint">Approx uses a character heuristic. Tokenizer uses a library for higher accuracy and may add slight overhead.</div>
    </div>

    <div class="settings-row col">
      <label class="label">Temperature: {{ Number(props.settings.temperature).toFixed(2) }}</label>
      <input type="range" min="0" max="2" step="0.05" v-model.number="props.settings.temperature" />
      <div class="settings-hint">Lower = deterministic, Higher = creative. Default 1.0</div>
    </div>

    <div class="settings-row col">
      <label class="label">System Prompt</label>
      <textarea
        v-model="props.settings.system_prompt"
        class="input"
        rows="6"
        placeholder="Write global instructions for the assistant. This text is sent as a system message for every chat and Quick Prompt."
        autocomplete="off"
        spellcheck="false"
      />
      <div class="settings-hint">
        Used as the global system instruction for chat. When a Quick Prompt is active, its text is appended to the end of this system prompt.
      </div>
    </div>
    <div class="settings-title">UI</div>
    <div class="settings-row col">
      <label class="label">UI Style</label>
      <select v-model="props.settings.ui_style" class="input">
        <option value="sidebar-dark">Sidebar Dark (default)</option>
        <option value="sidebar-light">Sidebar Light</option>
      </select>
      <div class="settings-hint">Switch between Sidebar Dark or Sidebar Light.</div>
    </div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.start_in_tray"/> Start in tray</label>
    </div>
    <div class="settings-hint">When enabled, the main window stays hidden on app startup until you open it from the tray.</div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.show_busy_indicator"/> Show busy indicator</label>
    </div>
    <div class="settings-hint">
      Shows a small always-on-top pill with elapsed time while a background action runs (quick prompts, text to speech, transcription),
      and the error message if the request fails or times out. Hidden while the main window is in front.
    </div>
    <div class="settings-title">Conversation</div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.persist_conversations"/> Persist conversations</label>
      <button class="btn danger" @click="props.onClearConversations">Clear All Conversations</button>
    </div>
    <div class="settings-hint">When enabled, conversation history is saved locally only.</div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.hide_tool_calls_in_chat"/> Hide tool call details in chat</label>
    </div>
  </div>
</template>

<style scoped>
/* Constrain the System Prompt textarea within the settings card */
.settings-section :deep(textarea.input) {
  display: block;
  width: 100% !important;
  max-width: 100% !important;
  box-sizing: border-box;
  flex: 0 0 auto !important; /* override global flex:1 on .input */
  align-self: stretch;
  overflow-x: hidden;
}
</style>
