<script setup lang="ts">
import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import HotkeyPicker from './HotkeyPicker.vue'
import CollapsibleCard from '../ui/CollapsibleCard.vue'

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

// All three hotkeys go through the same OS registration, so a duplicate means
// one of them silently never fires.
function duplicateHotkey(): string {
  const entries = [
    String(props.settings.global_hotkey || '').trim(),
    String(props.settings.select_all_hotkey || '').trim(),
    String(props.settings.push_to_talk_hotkey || '').trim(),
  ].filter(Boolean)
  for (let i = 0; i < entries.length; i++) {
    if (entries.indexOf(entries[i]) !== i) return entries[i]
  }
  return ''
}

const hotkeysCollide = computed(() => !!duplicateHotkey())

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
  <!-- Each group is its own card and a sibling of the others, so `.page` owns
       the spacing between them. The groups that are set once and forgotten
       start folded; the ones people actually come here to change do not. -->

  <CollapsibleCard
    id="settings.general.hotkeys"
    title="Hotkeys"
    desc="Global shortcuts, registered with Windows while the app runs."
  >
    <div class="field">
      <label class="field-label">Global hotkey</label>
      <HotkeyPicker v-model="props.settings.global_hotkey" />
      <p class="field-hint">
        Opens the Quick Actions popup. Leave every part empty to disable.
        Current: <code>{{ props.settings.global_hotkey || 'disabled' }}</code>
      </p>
    </div>

    <div class="divider"></div>

    <div class="field">
      <label class="field-label">Select-All hotkey</label>
      <HotkeyPicker v-model="props.settings.select_all_hotkey" />
      <p class="field-hint">
        Selects text in the focused application, runs the quick prompt below on it and pastes the result back over it -
        no popup in between. Leave every part empty to disable.
        Current: <code>{{ props.settings.select_all_hotkey || 'disabled' }}</code>
      </p>
    </div>

    <div class="field-grid">
      <div class="field">
        <label class="field-label">What it selects</label>
        <select v-model="props.settings.select_all_capture_mode" class="input" :disabled="!props.settings.select_all_hotkey">
          <option value="ctrl_shift_home">Everything above the cursor (Ctrl + Shift + Home)</option>
          <option value="ctrl_a">Whole document (Ctrl + A)</option>
          <option value="none">Off - use what is already selected</option>
        </select>
      </div>

      <div class="field">
        <label class="field-label">Prompt it runs</label>
        <select v-model.number="props.settings.select_all_quick_prompt" class="input" :disabled="!props.settings.select_all_hotkey">
          <option v-for="n in 9" :key="'sa-qp-' + n" :value="n">{{ n }} - {{ quickPromptLabels[n - 1] || '(empty)' }}</option>
        </select>
      </div>
    </div>
    <p class="field-hint">
      <em>Ctrl + Shift + Home</em> is the default because it fits the correct-what-I-just-typed case: in a chat box or
      comment field it grabs your draft without also selecting the conversation above it. Pick <em>Ctrl + A</em> to
      rewrite a whole document instead. Prompts are edited under Settings → Quick Prompts.
    </p>

    <div class="divider"></div>

    <div class="field">
      <label class="field-label">Push-to-talk hotkey</label>
      <HotkeyPicker v-model="props.settings.push_to_talk_hotkey" />
      <p class="field-hint">
        Hold to open the Assistant Mode microphone, release to close it. Only does anything while a session is running
        and the microphone is set to push-to-talk. Leave every part empty to disable.
        Current: <code>{{ props.settings.push_to_talk_hotkey || 'disabled' }}</code>
      </p>
    </div>

    <p v-if="hotkeysCollide" class="field-hint error">
      <code>{{ duplicateHotkey() }}</code> is assigned to more than one action - only one of them will work.
    </p>
  </CollapsibleCard>

  <CollapsibleCard
    id="settings.general.provider"
    title="AI Provider"
    desc="Credentials and the model used for chat and quick prompts."
  >
    <div class="field">
      <label class="field-label">OpenAI API key</label>
      <div class="actions">
        <input
          :type="showApiKey ? 'text' : 'password'"
          v-model="props.settings.openai_api_key"
          class="input"
          placeholder="sk-..."
          autocomplete="off"
          spellcheck="false"
        />
        <button class="btn ghost" type="button" @click="showApiKey = !showApiKey">{{ showApiKey ? 'Hide' : 'Show' }}</button>
      </div>
      <p class="field-hint">Stored in settings.json. Leave empty to fall back to the <code>OPENAI_API_KEY</code> environment variable.</p>
    </div>

    <div class="field">
      <label class="field-label">Model</label>
      <div class="actions">
        <select v-model="props.settings.openai_chat_model" class="input">
          <option v-if="!props.models.list.includes(props.settings.openai_chat_model)" :value="props.settings.openai_chat_model">{{ props.settings.openai_chat_model }} (current)</option>
          <option v-for="m in props.models.list" :key="m" :value="m">{{ m }}</option>
        </select>
        <button class="btn ghost" type="button" :disabled="props.models.loading" @click="props.onRefreshModels">
          {{ props.models.loading ? 'Fetching…' : 'Fetch models' }}
        </button>
      </div>
      <p v-if="props.models.error" class="field-hint error">{{ props.models.error }}</p>
    </div>

    <div class="field-grid">
      <div class="field">
        <label class="field-label">Tokenizer</label>
        <select v-model="props.settings.tokenizer_mode" class="input">
          <option value="approx">Approximate (fast, lightweight)</option>
          <option value="tiktoken">Tokenizer (more accurate)</option>
        </select>
        <p class="field-hint">Approximate uses a character heuristic; the tokenizer is exact but adds a little overhead.</p>
      </div>

      <div class="field">
        <label class="field-label">
          Temperature
          <span class="range-value">{{ Number(props.settings.temperature).toFixed(2) }}</span>
        </label>
        <input class="range" type="range" min="0" max="2" step="0.05" v-model.number="props.settings.temperature" />
        <p class="field-hint">Lower is more deterministic, higher is more creative. Default 1.00.</p>
      </div>
    </div>

    <div class="field">
      <label class="field-label">System prompt</label>
      <textarea
        v-model="props.settings.system_prompt"
        class="input"
        rows="6"
        placeholder="Write global instructions for the assistant. This text is sent as a system message for every chat and Quick Prompt."
        autocomplete="off"
        spellcheck="false"
      />
      <p class="field-hint">
        Sent as the system message for every chat. When a Quick Prompt is active its text is appended to the end of this.
      </p>
    </div>
  </CollapsibleCard>

  <CollapsibleCard
    id="settings.general.interface"
    title="Interface"
    desc="Appearance and what the app does at startup."
  >
    <div class="field">
      <label class="field-label">UI style</label>
      <select v-model="props.settings.ui_style" class="input w-md">
        <option value="sidebar-dark">Sidebar Dark (default)</option>
        <option value="sidebar-light">Sidebar Light</option>
      </select>
    </div>

    <label class="switch row">
      <input type="checkbox" v-model="props.settings.start_in_tray" />
      <span class="switch-text">
        <span class="switch-label">Start in tray</span>
        <span class="switch-hint">The main window stays hidden on startup until you open it from the tray icon.</span>
      </span>
    </label>

    <label class="switch row">
      <input type="checkbox" v-model="props.settings.show_busy_indicator" />
      <span class="switch-text">
        <span class="switch-label">Show busy indicator</span>
        <span class="switch-hint">
          A small always-on-top pill showing elapsed time while a background action runs - quick prompts, speech, transcription -
          and the error if one fails or times out. Hidden while the main window is in front.
        </span>
      </span>
    </label>
  </CollapsibleCard>

  <CollapsibleCard
    id="settings.general.conversation"
    title="Conversation"
    desc="How chat history is stored and displayed."
  >
    <label class="switch row">
      <input type="checkbox" v-model="props.settings.persist_conversations" />
      <span class="switch-text">
        <span class="switch-label">Persist conversations</span>
        <span class="switch-hint">History is saved locally only, never uploaded anywhere.</span>
      </span>
    </label>

    <label class="switch row">
      <input type="checkbox" v-model="props.settings.hide_tool_calls_in_chat" />
      <span class="switch-text">
        <span class="switch-label">Hide tool call details in chat</span>
        <span class="switch-hint">Collapses MCP request and response blocks so the conversation reads as prose.</span>
      </span>
    </label>

    <div class="divider"></div>

    <div class="actions">
      <button class="btn danger" type="button" @click="props.onClearConversations">Clear all conversations</button>
      <span class="field-hint">Deletes every saved conversation. This cannot be undone.</span>
    </div>
  </CollapsibleCard>
</template>
