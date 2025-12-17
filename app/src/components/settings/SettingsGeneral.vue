<script setup lang="ts">
import { ref, watch, computed } from 'vue'
import { checkShortcutAvailable } from '../../hotkeys'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

const props = defineProps<{
  settings: any
  models: { list: string[]; loading: boolean; error: string | null }
  onSave: () => void
  onRefreshModels: () => void
  onClearConversations: () => void
}>()

const showApiKey = ref(false)
const showSttCloudKey = ref(false)

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

// ----- Local Whisper (STT) model selection + prefetch
const whisperPresets = [
  { label: 'base (multi)', value: 'base', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin' },
  { label: 'base.en (English)', value: 'base.en', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin' },
  { label: 'small (multi)', value: 'small', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin' },
  { label: 'small.en (English)', value: 'small.en', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin' },
  { label: 'medium (multi)', value: 'medium', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin' },
  { label: 'medium.en (English)', value: 'medium.en', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin' },
  { label: 'large-v3', value: 'large-v3', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin' },
  { label: 'large-v3-turbo', value: 'large-v3-turbo', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin' },
]

const cloudSttModelPresets = [
  { label: 'OpenAI Whisper (whisper-1)', value: 'whisper-1' },
  { label: 'NVIDIA Parakeet (parakeet-tdt-0.6b-v2)', value: 'parakeet-tdt-0.6b-v2' },
  { label: 'NVIDIA Parakeet (parakeet-tdt-0.6b-v3)', value: 'parakeet-tdt-0.6b-v3' },
]

const localSttModelPresets = [
  { label: 'Whisper (built-in local)', value: 'whisper' },
  { label: 'Parakeet (parakeet-tdt-0.6b-v2)', value: 'parakeet-tdt-0.6b-v2' },
]

function urlForPreset(preset: string): string {
  const p = whisperPresets.find(p => p.value === preset)
  return p ? p.url : 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin'
}

watch(() => props.settings.stt_whisper_model_preset, (v: string) => {
  try {
    props.settings.stt_whisper_model_url = urlForPreset(String(v || 'base'))
  } catch {}
})

const prefetchBusy = ref(false)
const prefetchReceived = ref(0)
const prefetchTotal = ref(0)
const prefetchDonePath = ref('')
const prefetchError = ref('')

const prefetchParakeetBusy = ref(false)
const prefetchParakeetReceived = ref(0)
const prefetchParakeetTotal = ref(0)
const prefetchParakeetDonePath = ref('')
const prefetchParakeetError = ref('')

async function prefetchWhisperModel() {
  prefetchBusy.value = true
  prefetchReceived.value = 0
  prefetchTotal.value = 0
  prefetchDonePath.value = ''
  prefetchError.value = ''
  let unlisten: null | (() => void) = null
  try {
    unlisten = await listen('stt-model-download', (e: any) => {
      try {
        const p = e?.payload || {}
        if (p.kind === 'progress') {
          prefetchReceived.value = Number(p.received || 0)
          prefetchTotal.value = Number(p.total || 0)
        } else if (p.kind === 'done') {
          prefetchDonePath.value = String(p.path || '')
        }
      } catch {}
    })
    const url = (props.settings.stt_whisper_model_url || '').trim() || undefined
    const path = await invoke<string>('stt_prefetch_whisper_model', { url })
    if (path) prefetchDonePath.value = path
  } catch (e: any) {
    prefetchError.value = e?.message || String(e) || 'Download failed'
  } finally {
    if (unlisten) { try { unlisten() } catch {} }
    prefetchBusy.value = false
  }
}

async function prefetchParakeetModel() {
  prefetchParakeetBusy.value = true
  prefetchParakeetReceived.value = 0
  prefetchParakeetTotal.value = 0
  prefetchParakeetDonePath.value = ''
  prefetchParakeetError.value = ''
  let unlisten: null | (() => void) = null
  try {
    unlisten = await listen('stt-parakeet-model-download', (e: any) => {
      try {
        const p = e?.payload || {}
        if (p.kind === 'progress') {
          prefetchParakeetReceived.value = Number(p.received || 0)
          prefetchParakeetTotal.value = Number(p.total || 0)
        } else if (p.kind === 'done') {
          prefetchParakeetDonePath.value = String(p.path || '')
        }
      } catch {}
    })
    const path = await invoke<string>('stt_prefetch_parakeet_model')
    if (path) prefetchParakeetDonePath.value = path
  } catch (e: any) {
    prefetchParakeetError.value = e?.message || String(e) || 'Download failed'
  } finally {
    if (unlisten) { try { unlisten() } catch {} }
    prefetchParakeetBusy.value = false
  }
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
      <label class="label">Temperature: {{ props.settings.temperature.toFixed(2) }}</label>
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
    <div class="settings-title">Speech To Text</div>
    <div class="settings-row col">
      <label class="label">Engine</label>
      <div class="row-inline">
        <select v-model="props.settings.stt_engine" class="input" style="max-width: 220px;">
          <option value="openai">OpenAI (cloud)</option>
          <option value="local">Local</option>
        </select>
      </div>
      <div class="settings-hint">
        Local runs fully on-device. Choose Whisper or Parakeet below. Models are downloaded into your app data folder on first use.
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'local'" class="settings-row col">
      <label class="label">Local STT Model</label>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <select v-model="props.settings.stt_local_model" class="input" style="max-width: 320px;">
          <option v-if="!localSttModelPresets.some(p => p.value === props.settings.stt_local_model)" :value="props.settings.stt_local_model">{{ props.settings.stt_local_model }} (current)</option>
          <option v-for="p in localSttModelPresets" :key="p.value" :value="p.value">{{ p.label }}</option>
        </select>
        <input v-model="props.settings.stt_local_model" class="input" style="min-width: 260px;" placeholder="whisper or parakeet-tdt-0.6b-v2" />
      </div>
      <div class="settings-hint">
        Whisper uses a single ggml model file. Parakeet uses ONNX model files (large download).
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="settings-row col">
      <label class="label">Cloud STT Model</label>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <select v-model="props.settings.stt_cloud_model" class="input" style="max-width: 340px;">
          <option v-if="!cloudSttModelPresets.some(p => p.value === props.settings.stt_cloud_model)" :value="props.settings.stt_cloud_model">{{ props.settings.stt_cloud_model }} (current)</option>
          <option v-for="p in cloudSttModelPresets" :key="p.value" :value="p.value">{{ p.label }}</option>
        </select>
        <input v-model="props.settings.stt_cloud_model" class="input" style="min-width: 260px;" placeholder="Model (e.g. whisper-1 or parakeet-tdt-0.6b-v2)" />
      </div>
      <div class="settings-hint">
        Cloud STT sends audio to the configured endpoint (<code>POST /v1/audio/transcriptions</code>).
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="settings-row col">
      <label class="label">Cloud STT Base URL</label>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <input v-model="props.settings.stt_cloud_base_url" class="input" style="min-width: 360px;" placeholder="https://api.openai.com" />
        <button class="btn" @click="props.settings.stt_cloud_base_url = 'https://api.openai.com'">Use OpenAI</button>
      </div>
      <div class="settings-hint">
        Endpoint must support <code>POST /v1/audio/transcriptions</code>. If base URL is OpenAI, <code>OPENAI_API_KEY</code> (or Settings API key) is required.
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="settings-row col">
      <label class="label">Cloud STT API Key (optional)</label>
      <div class="row-inline">
        <input
          :type="showSttCloudKey ? 'text' : 'password'"
          v-model="props.settings.stt_cloud_api_key"
          class="input"
          placeholder="(optional)"
          autocomplete="off"
          spellcheck="false"
        />
        <button class="btn ghost" @click="showSttCloudKey = !showSttCloudKey">{{ showSttCloudKey ? 'Hide' : 'Show' }}</button>
      </div>
      <div class="settings-hint">
        Only used for non-OpenAI base URLs (e.g., a hosted Parakeet server that requires auth). For OpenAI, the OpenAI API key above is used.
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'local' && !String(props.settings.stt_local_model || 'whisper').toLowerCase().includes('parakeet')" class="settings-row col">
      <label class="label">Local Whisper Model</label>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <select v-model="props.settings.stt_whisper_model_preset" class="input" style="max-width: 260px;">
          <option v-for="p in whisperPresets" :key="p.value" :value="p.value">{{ p.label }}</option>
        </select>
        <input v-model="props.settings.stt_whisper_model_url" class="input" style="min-width: 360px;" placeholder="Model URL (ggml-*.bin)" />
        <button class="btn" :disabled="prefetchBusy" @click="prefetchWhisperModel">{{ prefetchBusy ? prefetchTotal ? (`Prefetching… ${Math.floor((prefetchReceived / Math.max(1, prefetchTotal)) * 100)}%`) : 'Prefetching…' : 'Prefetch Whisper model' }}</button>
      </div>
      <div class="settings-hint">
        Default folder: <code>%APPDATA%/AiDesktopCompanion/models/whisper</code>
        Set env <code>AIDC_WHISPER_MODEL_URL</code> to override the model URL.
      </div>
      <div v-if="prefetchError" class="settings-hint error">{{ prefetchError }}</div>
      <div v-else-if="prefetchBusy && prefetchTotal" class="settings-hint">Downloading: {{ (prefetchReceived/1024/1024).toFixed(1) }} / {{ (prefetchTotal/1024/1024).toFixed(1) }} MB</div>
      <div v-else-if="prefetchDonePath" class="settings-hint">Downloaded to: <code>{{ prefetchDonePath }}</code></div>
    </div>

    <div v-if="props.settings.stt_engine === 'local' && String(props.settings.stt_local_model || '').toLowerCase().includes('parakeet')" class="settings-row col">
      <label class="label">Local Parakeet Model</label>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <button class="btn" :disabled="prefetchParakeetBusy" @click="prefetchParakeetModel">{{ prefetchParakeetBusy ? prefetchParakeetTotal ? (`Prefetching… ${Math.floor((prefetchParakeetReceived / Math.max(1, prefetchParakeetTotal)) * 100)}%`) : 'Prefetching…' : 'Prefetch Parakeet model' }}</button>
      </div>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <label class="label" style="margin: 0; display: inline-flex; gap: 8px; align-items: center;">
          <input type="checkbox" v-model="props.settings.stt_parakeet_has_cuda" />
          Use CUDA (if available)
        </label>
      </div>
      <div class="settings-hint">
        Default folder: <code>%APPDATA%/AiDesktopCompanion/models/parakeet/parakeet-tdt-0.6b-v2</code>
      </div>
      <div class="settings-hint">
        Enable only if you have an NVIDIA CUDA-capable GPU and the model files required for GPU mode.
      </div>
      <div v-if="prefetchParakeetError" class="settings-hint error">{{ prefetchParakeetError }}</div>
      <div v-else-if="prefetchParakeetBusy && prefetchParakeetTotal" class="settings-hint">Downloading: {{ (prefetchParakeetReceived/1024/1024).toFixed(1) }} / {{ (prefetchParakeetTotal/1024/1024).toFixed(1) }} MB</div>
      <div v-else-if="prefetchParakeetDonePath" class="settings-hint">Downloaded to: <code>{{ prefetchParakeetDonePath }}</code></div>
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
