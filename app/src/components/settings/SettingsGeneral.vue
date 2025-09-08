<script setup lang="ts">
import { ref, defineProps, watch, computed } from 'vue'
import { checkShortcutAvailable } from '../../hotkeys'
import { invoke } from '@tauri-apps/api/core'

const props = defineProps<{
  settings: any
  models: { list: string[]; loading: boolean; error: string | null }
  onSave: () => void
  onRefreshModels: () => void
  onClearConversations: () => void
}>()

const showApiKey = ref(false)

// ----- Global Hotkey UI state
const modOptions = [
  { label: 'None', value: '' },
  { label: 'Alt', value: 'Alt' },
  { label: 'Shift', value: 'Shift' },
  { label: 'Ctrl', value: 'Control' },
  { label: 'Win', value: 'Win' }, // will be normalized to 'Super'
]
const ghkMod1 = ref<string>('')
const ghkMod2 = ref<string>('')
const ghkKey = ref<string>('')
const ghkError = ref<string | null>(null)
const ghkChecking = ref<boolean>(false)

// Filtered modifier options to prevent selecting the same modifier in both dropdowns (except "None")
const modOptions1 = computed(() => {
  const other = ghkMod2.value
  return other ? modOptions.filter(o => o.value !== other) : modOptions
})
const modOptions2 = computed(() => {
  const other = ghkMod1.value
  return other ? modOptions.filter(o => o.value !== other) : modOptions
})

// Ensure selections do not end up identical (e.g., when loading existing settings)
watch(ghkMod1, (v) => {
  if (v && v === ghkMod2.value) ghkMod2.value = ''
})
watch(ghkMod2, (v) => {
  if (v && v === ghkMod1.value) ghkMod1.value = ''
})

function parseHotkeyToFields(hk: string) {
  try {
    const s = (hk || '').trim()
    if (!s) { ghkMod1.value = ''; ghkMod2.value = ''; ghkKey.value = ''; return }
    const parts = s.split('+').map(p => p.trim()).filter(Boolean)
    const mods: string[] = []
    let key = ''
    for (const p of parts) {
      const up = p.toLowerCase()
      if (up === 'alt' || up === 'shift' || up === 'control' || up === 'ctrl' || up === 'win' || up === 'super' || up === 'command' || up === 'cmd') {
        // normalize ctrl synonyms
        const norm = (up === 'ctrl') ? 'Control' : (up === 'super' ? 'Win' : (up === 'cmd' || up === 'command') ? 'Control' : p.charAt(0).toUpperCase() + p.slice(1))
        mods.push(norm)
      } else {
        key = p
      }
    }
    ghkMod1.value = mods[0] || ''
    ghkMod2.value = mods[1] || ''
    // Dedupe in case both parsed modifiers are identical
    if (ghkMod1.value && ghkMod1.value === ghkMod2.value) {
      ghkMod2.value = ''
    }
    ghkKey.value = key
  } catch { ghkMod1.value = ''; ghkMod2.value = ''; ghkKey.value = ''; }
}

function composeHotkey(): string {
  const mods = [ghkMod1.value, ghkMod2.value]
    .map(m => m.trim())
    .filter(Boolean)
    .map(m => m === 'Win' ? 'Super' : m) // normalize here for storage/consistency
  const key = (ghkKey.value || '').trim()
  const parts = [...mods, key].filter(Boolean)
  return parts.join('+')
}

// Initialize from existing setting
parseHotkeyToFields(props.settings.global_hotkey || '')

// Keep in sync if settings are loaded later or changed externally
watch(() => props.settings.global_hotkey, (v: string) => {
  parseHotkeyToFields(typeof v === 'string' ? v : '')
})

let checkTimer: any = 0
async function validateGhk() {
  const hk = composeHotkey()
  props.settings.global_hotkey = hk
  ghkError.value = null
  if (!hk) return
  ghkChecking.value = true
  try {
    const ok = await checkShortcutAvailable(hk)
    if (!ok) {
      ghkError.value = 'Hotkey appears unavailable or already in use.'
    }
  } catch {
    ghkError.value = 'Could not validate hotkey.'
  } finally {
    ghkChecking.value = false
  }
}

watch([ghkMod1, ghkMod2, ghkKey], () => {
  if (checkTimer) clearTimeout(checkTimer)
  checkTimer = setTimeout(validateGhk, 300)
})

// ----- TTS Proxy QA state
const ttsQA_Count = ref<number>(0)
const ttsQA_Busy = ref<boolean>(false)
const ttsQA_LastRemoved = ref<number | null>(null)

async function refreshTtsProxyCount() {
  ttsQA_Busy.value = true
  try {
    ttsQA_Count.value = await invoke<number>('tts_stream_session_count')
  } catch {
    // ignore
  } finally {
    ttsQA_Busy.value = false
  }
}

async function cleanupIdleTtsProxy() {
  ttsQA_Busy.value = true
  try {
    ttsQA_LastRemoved.value = await invoke<number>('tts_stream_cleanup_idle', { ttl_seconds: 60 })
    await refreshTtsProxyCount()
  } catch {
    // ignore
  } finally {
    ttsQA_Busy.value = false
  }
}
</script>

<template>
  <div class="settings-section">
    <div class="settings-title">General Settings</div>  
    <button class="btn" @click="props.onSave">Save</button>        

    <div class="settings-row col">
      <label class="label">Global Hotkey</label>
      <div class="row-inline">
        <select v-model="ghkMod1" class="input" style="max-width: 160px;">
          <option v-for="opt in modOptions1" :key="'m1-'+opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
        <select v-model="ghkMod2" class="input" style="max-width: 160px;">
          <option v-for="opt in modOptions2" :key="'m2-'+opt.value" :value="opt.value">{{ opt.label }}</option>
        </select>
        <input
          v-model="ghkKey"
          class="input"
          placeholder="A or F9 or Space"
          autocomplete="off"
          spellcheck="false"
        />
      </div>
      <div class="settings-hint">Example: Alt + Shift + A. Leave all empty to disable. Current: <code>{{ props.settings.global_hotkey || 'disabled' }}</code></div>
      <div v-if="ghkError" class="settings-hint error">{{ ghkError }}</div>
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
      <label class="label">Temperature: {{ props.settings.temperature.toFixed(2) }}</label>
      <input type="range" min="0" max="2" step="0.05" v-model.number="props.settings.temperature" />
      <div class="settings-hint">Lower = deterministic, Higher = creative. Default 1.0</div>
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
    <div class="settings-title">Conversation</div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.persist_conversations"/> Persist conversations</label>
      <button class="btn danger" @click="props.onClearConversations">Clear All Conversations</button>      
    </div>
    <div class="settings-hint">When enabled, conversation history is saved locally only.</div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.hide_tool_calls_in_chat"/> Hide tool call details in chat</label>
    </div>

    <div class="settings-title">Text-to-Speech Defaults</div>
    <div class="settings-row col">
      <label class="label">Streaming enabled by default</label>
      <label class="checkbox"><input type="checkbox" v-model="props.settings.tts_openai_streaming"/> Enable streaming for OpenAI TTS</label>
      <div class="settings-hint">When enabled, the TTS panel and background playback will prefer streaming.</div>
    </div>
    <div class="settings-row col">
      <label class="label">Default streaming format</label>
      <select v-model="props.settings.tts_openai_format" class="input" style="max-width: 180px;">
        <option value="mp3">MP3</option>
        <option value="wav">WAV</option>
        <option value="opus">OPUS</option>
      </select>
      <div class="settings-hint">MP3 offers broad compatibility for progressive playback. WAV is larger. OPUS is smaller but may not be supported everywhere.</div>
    </div>
    <div class="settings-row col">
      <label class="label">Default instructions</label>
      <input v-model="props.settings.tts_openai_instructions" class="input" placeholder="e.g. Cheerful and positive tone" />
      <div class="settings-hint">Optional style/delivery hint sent with TTS requests.</div>
    </div>

    <div class="settings-title">TTS Proxy QA</div>
    <div class="settings-row col">
      <div class="row-inline" style="gap: 10px; align-items: center;">
        <button class="btn" :disabled="ttsQA_Busy" @click="refreshTtsProxyCount">{{ ttsQA_Busy ? 'Checking…' : 'Count Active Sessions' }}</button>
        <div class="settings-hint">Active sessions: <code>{{ ttsQA_Count }}</code></div>
      </div>
      <div class="row-inline" style="gap: 10px; align-items: center; margin-top: 6px;">
        <button class="btn" :disabled="ttsQA_Busy" @click="cleanupIdleTtsProxy">{{ ttsQA_Busy ? 'Cleaning…' : 'Cleanup Idle (>60s)' }}</button>
        <div class="settings-hint">Last removed: <code>{{ ttsQA_LastRemoved ?? 0 }}</code></div>
      </div>
    </div>
  </div>
</template>
