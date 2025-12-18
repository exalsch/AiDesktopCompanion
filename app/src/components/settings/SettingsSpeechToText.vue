
<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

const props = defineProps<{
  settings: any
}>()

const showSttCloudKey = ref(false)

const localSttProviders = [
  { label: 'Whisper', value: 'whisper', hint: 'On-device Whisper. Uses a ggml model file.' },
  { label: 'Parakeet', value: 'parakeet', hint: 'On-device Parakeet. Downloads ONNX model files.' },
]

const parakeetVariants = [
  { label: 'Parakeet V2', value: 'parakeet-tdt-0.6b-v2', hint: 'Fast and accurate. Smaller download.' },
  { label: 'Parakeet V3', value: 'parakeet-tdt-0.6b-v3', hint: 'Multilingual (25 languages). Newer variant.' },
]

const cloudSttModelPresetsBase = [
  { label: 'Whisper (whisper-1)', value: 'whisper-1', hint: 'OpenAI Whisper via OpenAI-compatible endpoint.' },
  { label: 'Parakeet V2 (parakeet-tdt-0.6b-v2)', value: 'parakeet-tdt-0.6b-v2', hint: 'Parakeet via OpenAI-compatible endpoint.' },
  { label: 'Parakeet V3 (parakeet-tdt-0.6b-v3)', value: 'parakeet-tdt-0.6b-v3', hint: 'Newer Parakeet variant via OpenAI-compatible endpoint.' },
]

const whisperPresets = [
  { label: 'Whisper Base', value: 'base', hint: 'Fast, lower accuracy', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin' },
  { label: 'Whisper Base (English)', value: 'base.en', hint: 'Fast, better for English', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin' },
  { label: 'Whisper Small', value: 'small', hint: 'Fast and fairly accurate', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin' },
  { label: 'Whisper Small (English)', value: 'small.en', hint: 'Fast and fairly accurate (English)', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin' },
  { label: 'Whisper Medium', value: 'medium', hint: 'Good accuracy, medium speed', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin' },
  { label: 'Whisper Medium (English)', value: 'medium.en', hint: 'Good accuracy, medium speed (English)', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin' },
  { label: 'Whisper Large V3', value: 'large-v3', hint: 'High accuracy, slower', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin' },
  { label: 'Whisper Large V3 Turbo', value: 'large-v3-turbo', hint: 'Balanced accuracy and speed', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin' },
]

function isKnownCloudModel(model: string): boolean {
  return cloudSttModelPresetsBase.some(p => p.value === model)
}

const isParakeetLocal = computed(() => {
  return String(props.settings.stt_local_model || '').toLowerCase().includes('parakeet')
})

const localProvider = computed({
  get(): string {
    return isParakeetLocal.value ? 'parakeet' : 'whisper'
  },
  set(v: string) {
    if (v === 'parakeet') {
      const cur = String(props.settings.stt_local_model || '')
      if (!cur.toLowerCase().includes('parakeet')) {
        props.settings.stt_local_model = 'parakeet-tdt-0.6b-v2'
      }
    } else {
      props.settings.stt_local_model = 'whisper'
    }
  },
})

const cloudSttModelPresets = computed(() => {
  const cur = String(props.settings.stt_cloud_model || '').trim()
  if (cur && !isKnownCloudModel(cur)) {
    return [{ label: `${cur} (current)`, value: cur, hint: 'Current value is not in the suggested list.' }, ...cloudSttModelPresetsBase]
  }
  return cloudSttModelPresetsBase
})

function urlForPreset(preset: string): string {
  const p = whisperPresets.find(p => p.value === preset)
  return p ? p.url : whisperPresets[0].url
}

watch(
  () => props.settings.stt_whisper_model_preset,
  (v: string) => {
    try {
      props.settings.stt_whisper_model_url = urlForPreset(String(v || 'base'))
    } catch {}
  },
  { immediate: true }
)

const prefetchWhisperBusy = ref(false)
const prefetchWhisperReceived = ref(0)
const prefetchWhisperTotal = ref(0)
const prefetchWhisperDonePath = ref('')
const prefetchWhisperError = ref('')
const prefetchWhisperPreset = ref<string>('')

const prefetchParakeetBusy = ref(false)
const prefetchParakeetReceived = ref(0)
const prefetchParakeetTotal = ref(0)
const prefetchParakeetDonePath = ref('')
const prefetchParakeetError = ref('')

const parakeetCudaCheckBusy = ref(false)
const parakeetCudaCheckError = ref('')

function percent(received: number, total: number): string {
  if (!total) return ''
  const p = Math.floor((received / Math.max(1, total)) * 100)
  return `${p}%`
}

async function prefetchWhisperModel(preset: string) {
  if (prefetchWhisperBusy.value) return
  prefetchWhisperBusy.value = true
  prefetchWhisperPreset.value = preset
  prefetchWhisperReceived.value = 0
  prefetchWhisperTotal.value = 0
  prefetchWhisperDonePath.value = ''
  prefetchWhisperError.value = ''
  let unlisten: null | (() => void) = null
  try {
    props.settings.stt_whisper_model_preset = preset
    const url = urlForPreset(preset)
    props.settings.stt_whisper_model_url = url

    unlisten = await listen('stt-model-download', (e: any) => {
      try {
        const p = e?.payload || {}
        if (p.kind === 'progress') {
          prefetchWhisperReceived.value = Number(p.received || 0)
          prefetchWhisperTotal.value = Number(p.total || 0)
        } else if (p.kind === 'done') {
          prefetchWhisperDonePath.value = String(p.path || '')
        }
      } catch {}
    })

    const path = await invoke<string>('stt_prefetch_whisper_model', { url })
    if (path) prefetchWhisperDonePath.value = path
    await refreshLocalModelStatus()
  } catch (e: any) {
    prefetchWhisperError.value = e?.message || String(e) || 'Download failed'
  } finally {
    if (unlisten) { try { unlisten() } catch {} }
    prefetchWhisperBusy.value = false
    prefetchWhisperPreset.value = ''
  }
}

async function prefetchParakeetModel() {
  if (prefetchParakeetBusy.value) return
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

    const path = await invoke<string>('stt_prefetch_parakeet_model', { local_model: String(props.settings.stt_local_model || '') })
    if (path) prefetchParakeetDonePath.value = path
    await refreshLocalModelStatus()
  } catch (e: any) {
    prefetchParakeetError.value = e?.message || String(e) || 'Download failed'
  } finally {
    if (unlisten) { try { unlisten() } catch {} }
    prefetchParakeetBusy.value = false
  }
}

function selectParakeetVariant(v: string) {
  props.settings.stt_local_model = v
}

const isParakeetV3Local = computed(() => {
  const t = String(props.settings.stt_local_model || '').toLowerCase()
  return t.includes('0.6b-v3') || (t.includes('parakeet') && t.includes('v3'))
})

const localModelStatusBusy = ref(false)
const localModelStatusError = ref('')
const localModelDownloaded = ref(false)
const localModelPath = ref('')
const localModelMissing = ref<string[]>([])

async function refreshLocalModelStatus() {
  try {
    if (props.settings.stt_engine !== 'local') {
      return
    }
    if (localModelStatusBusy.value) return
    localModelStatusBusy.value = true
    localModelStatusError.value = ''

    const localModel = String(props.settings.stt_local_model || '')
    const res = await invoke<any>('stt_local_model_status', {
      local_model: localModel,
      whisper_url: String(props.settings.stt_whisper_model_url || ''),
      parakeet_has_cuda: Boolean(props.settings.stt_parakeet_has_cuda),
    })
    localModelDownloaded.value = Boolean(res?.downloaded)
    localModelPath.value = String(res?.path || '')
    localModelMissing.value = Array.isArray(res?.missing) ? res.missing.map((x: any) => String(x)) : []
  } catch (e: any) {
    localModelStatusError.value = e?.message || String(e) || 'Status check failed.'
    localModelDownloaded.value = false
    localModelPath.value = ''
    localModelMissing.value = []
  } finally {
    localModelStatusBusy.value = false
  }
}

watch(
  () => [props.settings.stt_engine, props.settings.stt_local_model, props.settings.stt_whisper_model_url, props.settings.stt_parakeet_has_cuda],
  () => refreshLocalModelStatus(),
  { immediate: true }
)

watch(() => props.settings.stt_parakeet_has_cuda, async (v: any) => {
  try {
    if (v !== true) {
      return
    }
    if (props.settings.stt_engine !== 'local' || !isParakeetLocal.value) {
      parakeetCudaCheckError.value = ''
      return
    }
    if (parakeetCudaCheckBusy.value) return

    parakeetCudaCheckBusy.value = true
    parakeetCudaCheckError.value = ''
    const res = await invoke<any>('stt_check_parakeet_cuda')
    if (!res?.ok) {
      parakeetCudaCheckError.value = String(res?.message || 'CUDA is not available.')
      props.settings.stt_parakeet_has_cuda = false
    }
  } catch (e: any) {
    parakeetCudaCheckError.value = e?.message || String(e) || 'CUDA check failed.'
    props.settings.stt_parakeet_has_cuda = false
  } finally {
    parakeetCudaCheckBusy.value = false
  }
})

function selectCloudModel(v: string) {
  props.settings.stt_cloud_model = v
}

const whisperCurrentPreset = computed(() => {
  const v = String(props.settings.stt_whisper_model_preset || '').trim() || 'base'
  const found = whisperPresets.some(p => p.value === v)
  return found ? v : 'base'
})

function whisperDownloadLabel(preset: string): string {
  if (!prefetchWhisperBusy.value) return 'Download'
  if (prefetchWhisperPreset.value !== preset) return 'Downloading…'
  return prefetchWhisperTotal.value ? `Downloading… ${percent(prefetchWhisperReceived.value, prefetchWhisperTotal.value)}` : 'Downloading…'
}

function parakeetDownloadLabel(): string {
  if (!prefetchParakeetBusy.value) return 'Download'
  return prefetchParakeetTotal.value ? `Downloading… ${percent(prefetchParakeetReceived.value, prefetchParakeetTotal.value)}` : 'Downloading…'
}

function modelItemClass(active: boolean): any {
  return { 'model-item': true, active }
}

function infoTitle(v: string): string {
  return v || 'Info'
}
</script>

<template>
  <div class="settings-section">
    <div class="settings-title">Speech To Text</div>

    <div class="settings-row col">
      <div class="row-label">
        <label class="label">Engine</label>
        <span class="info-icon" :title="infoTitle('Local runs fully on-device. Cloud sends audio to the configured endpoint (POST /v1/audio/transcriptions).')">i</span>
      </div>
      <div class="row-inline">
        <select v-model="props.settings.stt_engine" class="input" style="max-width: 220px;">
          <option value="openai">Cloud (OpenAI compatible)</option>
          <option value="local">Local (on-device)</option>
        </select>
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'local'" class="settings-row col">
      <div class="row-label">
        <label class="label">Local Provider</label>
        <span class="info-icon" :title="infoTitle('Choose the on-device speech-to-text engine. Additional options appear below.')">i</span>
      </div>

      <div class="model-list">
        <div
          v-for="p in localSttProviders"
          :key="p.value"
          :class="modelItemClass(localProvider === p.value)"
          @click="localProvider = p.value"
        >
          <div class="model-main">
            <div class="model-name">{{ p.label }}</div>
            <div class="model-hint">{{ p.hint }}</div>
          </div>
          <div class="model-meta">
            <span class="info-icon" :title="infoTitle(p.hint)" @click.stop>i</span>
            <span v-if="localProvider === p.value" class="model-active">Active</span>
          </div>
        </div>
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'local' && !isParakeetLocal" class="settings-row col">
      <div class="row-label">
        <label class="label">Whisper Model File</label>
        <span class="info-icon" :title="infoTitle('Select a Whisper model and download it. The file is stored in your app data folder.')">i</span>
      </div>

      <div class="model-list">
        <div
          v-for="p in whisperPresets"
          :key="p.value"
          :class="modelItemClass(whisperCurrentPreset === p.value)"
          @click="props.settings.stt_whisper_model_preset = p.value"
        >
          <div class="model-main">
            <div class="model-name">{{ p.label }}</div>
            <div class="model-hint">{{ p.hint }}</div>
          </div>
          <div class="model-meta">
            <span class="info-icon" :title="infoTitle(p.hint)" @click.stop>i</span>
            <button
              class="btn ghost"
              :disabled="prefetchWhisperBusy"
              @click.stop="prefetchWhisperModel(p.value)"
            >
              {{ whisperDownloadLabel(p.value) }}
            </button>
          </div>
        </div>
      </div>

      <div class="settings-hint">
        Default folder: <code>%APPDATA%/AiDesktopCompanion/models/whisper</code>
      </div>
      <div class="settings-hint" v-if="localModelStatusError">{{ localModelStatusError }}</div>
      <div class="settings-hint" v-else>
        Status:
        <span class="status-pill" :class="{ ok: localModelDownloaded, bad: !localModelDownloaded }">{{ localModelDownloaded ? 'Downloaded' : 'Not downloaded' }}</span>
        <span v-if="localModelPath">(<code>{{ localModelPath }}</code>)</span>
        <span v-if="!localModelDownloaded && localModelMissing.length">Missing: <code>{{ localModelMissing.join(', ') }}</code></span>
      </div>
      <div v-if="prefetchWhisperError" class="settings-hint error">{{ prefetchWhisperError }}</div>
      <div v-else-if="prefetchWhisperBusy && prefetchWhisperTotal" class="settings-hint">
        Downloading: {{ (prefetchWhisperReceived/1024/1024).toFixed(1) }} / {{ (prefetchWhisperTotal/1024/1024).toFixed(1) }} MB
      </div>
      <div v-else-if="prefetchWhisperDonePath" class="settings-hint">Downloaded to: <code>{{ prefetchWhisperDonePath }}</code></div>
    </div>

    <div v-if="props.settings.stt_engine === 'local' && isParakeetLocal" class="settings-row col">
      <div class="row-label">
        <label class="label">Parakeet Model</label>
        <span class="info-icon" :title="infoTitle('Select a Parakeet model variant. Then download the ONNX files into your app data folder.')">i</span>
      </div>

      <div class="model-list">
        <div
          v-for="p in parakeetVariants"
          :key="p.value"
          :class="modelItemClass(props.settings.stt_local_model === p.value)"
          @click="selectParakeetVariant(p.value)"
        >
          <div class="model-main">
            <div class="model-name">{{ p.label }}</div>
            <div class="model-hint">{{ p.hint }}</div>
          </div>
          <div class="model-meta">
            <span class="info-icon" :title="infoTitle(p.hint)" @click.stop>i</span>
            <span v-if="props.settings.stt_local_model === p.value" class="model-active">Active</span>
          </div>
        </div>
      </div>

      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <button class="btn ghost" :disabled="prefetchParakeetBusy" @click="prefetchParakeetModel">
          {{ parakeetDownloadLabel() }}
        </button>
        <label class="checkbox" style="margin: 0;">
          <input type="checkbox" v-model="props.settings.stt_parakeet_has_cuda" :disabled="parakeetCudaCheckBusy" />
          Use CUDA (if available)
        </label>
        <span class="info-icon" :title="infoTitle('Requires NVIDIA driver + CUDA runtime (CUDA/cuDNN DLLs). If missing, the toggle will auto-disable.')">i</span>
      </div>

      <div class="settings-hint">
        Default folder:
        <code v-if="isParakeetV3Local">%APPDATA%/AiDesktopCompanion/models/parakeet/parakeet-tdt-0.6b-v3</code>
        <code v-else>%APPDATA%/AiDesktopCompanion/models/parakeet/parakeet-tdt-0.6b-v2</code>
      </div>
      <div class="settings-hint" v-if="localModelStatusError">{{ localModelStatusError }}</div>
      <div class="settings-hint" v-else>
        Status:
        <span class="status-pill" :class="{ ok: localModelDownloaded, bad: !localModelDownloaded }">{{ localModelDownloaded ? 'Downloaded' : 'Not downloaded' }}</span>
        <span v-if="localModelPath">(<code>{{ localModelPath }}</code>)</span>
        <span v-if="!localModelDownloaded && localModelMissing.length">Missing: <code>{{ localModelMissing.join(', ') }}</code></span>
      </div>
      <div v-if="parakeetCudaCheckBusy" class="settings-hint">Checking CUDA availability…</div>
      <div v-if="parakeetCudaCheckError" class="settings-hint error">{{ parakeetCudaCheckError }}</div>
      <div v-if="prefetchParakeetError" class="settings-hint error">{{ prefetchParakeetError }}</div>
      <div v-else-if="prefetchParakeetBusy && prefetchParakeetTotal" class="settings-hint">
        Downloading: {{ (prefetchParakeetReceived/1024/1024).toFixed(1) }} / {{ (prefetchParakeetTotal/1024/1024).toFixed(1) }} MB
      </div>
      <div v-else-if="prefetchParakeetDonePath" class="settings-hint">Downloaded to: <code>{{ prefetchParakeetDonePath }}</code></div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="settings-row col">
      <div class="row-label">
        <label class="label">Cloud STT Model</label>
        <span class="info-icon" :title="infoTitle('Choose a suggested model. No free text field: the goal is to keep this predictable.')">i</span>
      </div>

      <div class="model-list">
        <div
          v-for="p in cloudSttModelPresets"
          :key="p.value"
          :class="modelItemClass(props.settings.stt_cloud_model === p.value)"
          @click="selectCloudModel(p.value)"
        >
          <div class="model-main">
            <div class="model-name">{{ p.label }}</div>
            <div class="model-hint">{{ p.hint }}</div>
          </div>
          <div class="model-meta">
            <span class="info-icon" :title="infoTitle(p.hint)" @click.stop>i</span>
            <span v-if="props.settings.stt_cloud_model === p.value" class="model-active">Active</span>
          </div>
        </div>
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="settings-row col">
      <div class="row-label">
        <label class="label">Cloud STT Base URL</label>
        <span class="info-icon" :title="infoTitle('Must support POST /v1/audio/transcriptions (OpenAI compatible).')">i</span>
      </div>
      <div class="row-inline" style="gap: 10px; align-items: center; flex-wrap: wrap;">
        <input v-model="props.settings.stt_cloud_base_url" class="input" style="min-width: 360px;" placeholder="https://api.openai.com" />
        <button class="btn" @click="props.settings.stt_cloud_base_url = 'https://api.openai.com'">Use OpenAI</button>
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="settings-row col">
      <div class="row-label">
        <label class="label">Cloud STT API Key (optional)</label>
        <span class="info-icon" :title="infoTitle('Only used for non-OpenAI base URLs that require auth. For OpenAI base URL, the OpenAI API key is used.')">i</span>
      </div>
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
    </div>
  </div>
</template>

<style scoped>
.row-label { display: flex; align-items: center; gap: 8px; }
.info-icon {
  width: 18px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--adc-border);
  border-radius: 999px;
  font-size: 11px;
  line-height: 1;
  color: var(--adc-fg-muted);
  background: var(--adc-surface);
  cursor: default;
  user-select: none;
}

.model-list { display: flex; flex-direction: column; gap: 8px; width: 100%; }
.model-item {
  border: 1px solid var(--adc-border);
  border-radius: 10px;
  padding: 10px 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  background: var(--adc-surface);
  cursor: pointer;
}
.model-item.active { border-color: var(--adc-accent); }
.model-main { min-width: 0; }
.model-name { font-weight: 700; }
.model-hint { font-size: 12px; color: var(--adc-fg-muted); margin-top: 2px; }
.model-meta { display: inline-flex; align-items: center; gap: 10px; flex: 0 0 auto; }
.model-active { color: var(--adc-accent); font-size: 12px; font-weight: 700; }

.status-pill {
  display: inline-flex;
  align-items: center;
  padding: 2px 8px;
  border-radius: 999px;
  border: 1px solid var(--adc-border);
  font-size: 12px;
  line-height: 1.4;
  margin: 0 6px;
}

.status-pill.ok {
  border-color: rgba(60, 180, 120, 0.7);
  color: rgba(60, 180, 120, 1);
}

.status-pill.bad {
  border-color: rgba(220, 90, 90, 0.7);
  color: rgba(220, 90, 90, 1);
}
</style>
