
<script setup lang="ts">
import { computed, ref, watch, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import CollapsibleCard from '../ui/CollapsibleCard.vue'
import { listen } from '@tauri-apps/api/event'

const props = defineProps<{
  settings: any
  models?: { list: string[]; loading: boolean; error: string | null }
  onRefreshModels?: () => any
}>()

const showSttCloudKey = ref(false)
const inputDevicesBusy = ref(false)
const inputDevicesError = ref('')
const inputDevices = ref<Array<{ id: string; label: string }>>([])
const commandScriptsBusy = ref(false)
const commandScriptsError = ref('')
const commandScripts = ref<string[]>([])
const commandScriptOpBusy = ref(false)

function selectedInputDeviceExists(): boolean {
  const wanted = String(props.settings.stt_input_device_id || '').trim()
  if (!wanted) return true
  return inputDevices.value.some((d) => d.id === wanted)
}

function inputDeviceLabel(raw: string, fallbackIndex: number): string {
  const v = String(raw || '').trim()
  if (v) return v
  return `Microphone ${fallbackIndex + 1}`
}

async function refreshInputDevices() {
  inputDevicesBusy.value = true
  inputDevicesError.value = ''
  try {
    if (!navigator.mediaDevices || !navigator.mediaDevices.enumerateDevices) {
      throw new Error('Media device enumeration is not supported in this environment.')
    }
    try {
      await navigator.mediaDevices.getUserMedia({ audio: true })
    } catch {}

    const all = await navigator.mediaDevices.enumerateDevices()
    const mics = all
      .filter((d) => d.kind === 'audioinput')
      .map((d, idx) => ({ id: String(d.deviceId || ''), label: inputDeviceLabel(d.label, idx) }))
      .filter((d) => d.id)

    inputDevices.value = mics
    if (!selectedInputDeviceExists()) {
      props.settings.stt_input_device_id = ''
    }
  } catch (e: any) {
    inputDevicesError.value = e?.message || String(e) || 'Failed to list microphone devices.'
    inputDevices.value = []
  } finally {
    inputDevicesBusy.value = false
  }
}

async function refreshCommandScripts() {
  commandScriptsBusy.value = true
  commandScriptsError.value = ''
  try {
    const list = await invoke<string[]>('list_command_scripts')
    const next = Array.isArray(list)
      ? list.filter((x) => typeof x === 'string' && String(x).trim()).map((x) => String(x).trim())
      : []
    commandScripts.value = next
    if (next.length > 0 && !next.includes(String(props.settings.command_active_script || '').trim())) {
      props.settings.command_active_script = next[0]
    }
    if (next.length === 0) {
      props.settings.command_active_script = ''
    }
  } catch (e: any) {
    commandScriptsError.value = e?.message || String(e) || 'Failed to list command scripts.'
    commandScripts.value = []
  } finally {
    commandScriptsBusy.value = false
  }
}

async function createDefaultCommandScript() {
  if (commandScriptOpBusy.value) return
  commandScriptOpBusy.value = true
  commandScriptsError.value = ''
  try {
    const fileName = await invoke<string>('create_default_command_script')
    await refreshCommandScripts()
    if (fileName && fileName.trim()) {
      props.settings.command_active_script = fileName.trim()
    }
  } catch (e: any) {
    commandScriptsError.value = e?.message || String(e) || 'Failed to create default command script.'
  } finally {
    commandScriptOpBusy.value = false
  }
}

async function openCommandHooksFolder() {
  if (commandScriptOpBusy.value) return
  commandScriptOpBusy.value = true
  commandScriptsError.value = ''
  try {
    await invoke('open_command_hooks_folder')
  } catch (e: any) {
    commandScriptsError.value = e?.message || String(e) || 'Failed to open hooks folder.'
  } finally {
    commandScriptOpBusy.value = false
  }
}

const localSttProviders = [
  { label: 'Whisper', value: 'whisper', hint: 'On-device Whisper. Uses a ggml model file.' },
  { label: 'Parakeet', value: 'parakeet', hint: 'On-device Parakeet. Downloads ONNX model files.' },
]

const parakeetVariants = [
  { label: 'Parakeet V3', value: 'parakeet-tdt-0.6b-v3', hint: 'Multilingual - 25 languages including German.' },
]

// Models offered for the cloud endpoint. Only models that answer POST
// /v1/audio/transcriptions with a plain `{ "text": ... }` body belong here:
// the backend reads that field and nothing else. That rules out
// gpt-4o-transcribe-diarize (needs response_format=diarized_json) and the
// realtime/streaming models (gpt-live-transcribe, gpt-realtime-whisper).
const cloudSttModelPresetsBase = [
  { label: 'GPT Transcribe (gpt-transcribe)', value: 'gpt-transcribe', hint: 'OpenAI current recommendation for recorded audio and the replacement for whisper-1. Best accuracy.' },
  { label: 'GPT-4o Transcribe (gpt-4o-transcribe)', value: 'gpt-4o-transcribe', hint: 'Previous generation GPT-4o speech-to-text. Still available.' },
  { label: 'GPT-4o mini Transcribe (gpt-4o-mini-transcribe)', value: 'gpt-4o-mini-transcribe', hint: 'Cheaper and faster than gpt-4o-transcribe, slightly lower accuracy.' },
  { label: 'Whisper (whisper-1)', value: 'whisper-1', hint: 'Legacy OpenAI model. Keep it for OpenAI-compatible servers that only implement whisper-1.' },
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
  { label: 'Whisper Large V3 Turbo', value: 'large-v3-turbo', hint: 'Balanced accuracy and speed (1.6 GB)', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo.bin' },
  { label: 'Whisper Large V3 Turbo (Q8)', value: 'large-v3-turbo-q8_0', hint: 'Turbo quantized to 8-bit: ~874 MB, accuracy close to full Turbo', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin' },
  { label: 'Whisper Large V3 Turbo (Q5)', value: 'large-v3-turbo-q5_0', hint: 'Turbo quantized to 5-bit: ~574 MB, about a third of the full download', url: 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin' },
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
        props.settings.stt_local_model = 'parakeet-tdt-0.6b-v3'
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

const postProcessModelOptions = computed(() => {
  const fromSettings = Array.isArray(props.models?.list)
    ? props.models!.list.filter((x: any) => typeof x === 'string' && String(x).trim()).map((x: string) => String(x).trim())
    : []
  const deduped = Array.from(new Set(fromSettings))
  const fallback = ['gpt-4o-mini', 'gpt-4o', 'gpt-4.1-mini']
  const base = deduped.length ? deduped : fallback
  const current = String(props.settings.stt_post_process_model || '').trim()
  if (current && !base.includes(current)) {
    return [current, ...base]
  }
  return base
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

    const path = await invoke<string>('stt_prefetch_parakeet_model', { localModel: String(props.settings.stt_local_model || '') })
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
      localModel,
      whisperUrl: String(props.settings.stt_whisper_model_url || ''),
      parakeetHasCuda: Boolean(props.settings.stt_parakeet_has_cuda),
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

watch(() => props.settings.command_enabled, (v: any) => {
  if (v === true) {
    void refreshCommandScripts()
  }
})

onMounted(() => {
  void refreshInputDevices()
  void refreshCommandScripts()
})

function selectCloudModel(v: string) {
  props.settings.stt_cloud_model = v
}

/**
 * A model OpenAI does not host, pointed at OpenAI.
 *
 * The Parakeet entries below are legitimate: plenty of OpenAI-compatible servers
 * serve them. They are only wrong against api.openai.com, and that combination
 * fails at transcription time with a remote error that says nothing about the
 * cause. Any other base URL is somebody else's server and none of our business.
 */
const cloudModelNotOnOpenai = computed(() => {
  const url = String(props.settings.stt_cloud_base_url || '').toLowerCase()
  if (!/(^|\/\/)api\.openai\.com/.test(url)) return false
  const v = String(props.settings.stt_cloud_model || '').toLowerCase()
  if (!v) return false
  return !/^(gpt-|whisper-1$)/.test(v)
})

/**
 * Whisper's `.en` builds are English-only by construction - they cannot
 * transcribe another language, they transcribe it badly as English.
 */
const whisperIsEnglishOnly = computed(() =>
  String(props.settings.stt_whisper_model_preset || '').toLowerCase().includes('.en')
)

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
  <CollapsibleCard
    id="settings.stt.engine"
    title="Engine and input"
    desc="Where transcription happens and which microphone feeds it."
  >
    <div class="field">
      <div class="row-label">
        <label class="field-label">Engine</label>
        <span class="info-icon" :title="infoTitle('Local runs fully on-device. Cloud sends audio to the configured endpoint (POST /v1/audio/transcriptions).')">i</span>
      </div>
      <div class="actions">
        <select v-model="props.settings.stt_engine" class="input w-md">
          <option value="openai">Cloud (OpenAI compatible)</option>
          <option value="local">Local (on-device)</option>
        </select>
      </div>
    </div>

    <label class="switch row">
      <input type="checkbox" v-model="props.settings.pause_media_on_stt" />
      <span class="switch-text">
        <span class="switch-label">Pause playback while recording</span>
        <span class="switch-hint">
          Pauses whatever is playing - Spotify, a browser tab, anything that registers with Windows media controls -
          for the length of the recording, then resumes it. Music the microphone can hear ends up in the transcript.
          Nothing is resumed that was not playing to begin with.
        </span>
      </span>
    </label>

    <div class="field">
      <div class="row-label">
        <label class="field-label">Microphone Input</label>
        <span class="info-icon" :title="infoTitle('Select which microphone is used for STT recording in both the main STT panel and Quick Actions (S/C).')">i</span>
      </div>
      <div class="actions">
        <select v-model="props.settings.stt_input_device_id" class="input">
          <option value="">System default microphone</option>
          <option v-for="d in inputDevices" :key="d.id" :value="d.id">{{ d.label }}</option>
        </select>
        <button class="btn ghost" :disabled="inputDevicesBusy" @click="refreshInputDevices">{{ inputDevicesBusy ? 'Refreshing…' : 'Refresh' }}</button>
      </div>
      <div class="field-hint">If another audio app hijacks your mic (e.g. virtual devices), select the physical input here.</div>
      <div class="field-hint" v-if="inputDevicesError">{{ inputDevicesError }}</div>
    </div>

  </CollapsibleCard>

  <CollapsibleCard
    v-if="props.settings.stt_engine === 'local'"
    id="settings.stt.local"
    title="On-device models"
    desc="Downloaded once, then run entirely on this machine."
  >
    <div class="field">
      <div class="row-label">
        <label class="field-label">Provider</label>
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

    <div v-if="props.settings.stt_engine === 'local' && !isParakeetLocal" class="field">
      <div class="row-label">
        <label class="field-label">Whisper Model File</label>
        <span class="info-icon" :title="infoTitle('Select a Whisper model and download it. The file is stored in your app data folder.')">i</span>
      </div>

      <p v-if="whisperIsEnglishOnly" class="field-hint error">
        <code>{{ props.settings.stt_whisper_model_preset }}</code> is an English-only build. Speaking another language
        into it does not fail - it returns a poor English transcription of what it heard. Pick a preset without
        <code>.en</code> for anything else.
      </p>

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

      <div class="field-hint">
        Default folder: <code>%APPDATA%/AiDesktopCompanion/models/whisper</code>
      </div>
      <div class="field-hint" v-if="localModelStatusError">{{ localModelStatusError }}</div>
      <div class="field-hint" v-else>
        Status:
        <span class="badge" :class="localModelDownloaded ? 'ok' : 'err'">{{ localModelDownloaded ? 'Downloaded' : 'Not downloaded' }}</span>
        <span v-if="localModelPath">(<code>{{ localModelPath }}</code>)</span>
        <span v-if="!localModelDownloaded && localModelMissing.length">Missing: <code>{{ localModelMissing.join(', ') }}</code></span>
      </div>
      <div v-if="prefetchWhisperError" class="field-hint error">{{ prefetchWhisperError }}</div>
      <div v-else-if="prefetchWhisperBusy && prefetchWhisperTotal" class="field-hint">
        Downloading: {{ (prefetchWhisperReceived/1024/1024).toFixed(1) }} / {{ (prefetchWhisperTotal/1024/1024).toFixed(1) }} MB
      </div>
      <div v-else-if="prefetchWhisperDonePath" class="field-hint">Downloaded to: <code>{{ prefetchWhisperDonePath }}</code></div>
    </div>

    <div v-if="props.settings.stt_engine === 'local' && isParakeetLocal" class="field">
      <div class="row-label">
        <label class="field-label">Parakeet Model</label>
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

      <div class="actions">
        <button class="btn ghost" :disabled="prefetchParakeetBusy" @click="prefetchParakeetModel">
          {{ parakeetDownloadLabel() }}
        </button>
        <label class="checkbox">
          <input type="checkbox" v-model="props.settings.stt_parakeet_has_cuda" :disabled="parakeetCudaCheckBusy" />
          Use CUDA (if available)
        </label>
        <span class="info-icon" :title="infoTitle('Requires NVIDIA driver + CUDA runtime (CUDA/cuDNN DLLs). If missing, the toggle will auto-disable.')">i</span>
      </div>

      <div class="field-hint">
        Default folder:
        <code>%APPDATA%/AiDesktopCompanion/models/parakeet/parakeet-tdt-0.6b-v3</code>
      </div>
      <div class="field-hint" v-if="localModelStatusError">{{ localModelStatusError }}</div>
      <div class="field-hint" v-else>
        Status:
        <span class="badge" :class="localModelDownloaded ? 'ok' : 'err'">{{ localModelDownloaded ? 'Downloaded' : 'Not downloaded' }}</span>
        <span v-if="localModelPath">(<code>{{ localModelPath }}</code>)</span>
        <span v-if="!localModelDownloaded && localModelMissing.length">Missing: <code>{{ localModelMissing.join(', ') }}</code></span>
      </div>
      <div v-if="parakeetCudaCheckBusy" class="field-hint">Checking CUDA availability…</div>
      <div v-if="parakeetCudaCheckError" class="field-hint error">{{ parakeetCudaCheckError }}</div>
      <div v-if="prefetchParakeetError" class="field-hint error">{{ prefetchParakeetError }}</div>
      <div v-else-if="prefetchParakeetBusy && prefetchParakeetTotal" class="field-hint">
        Downloading: {{ (prefetchParakeetReceived/1024/1024).toFixed(1) }} / {{ (prefetchParakeetTotal/1024/1024).toFixed(1) }} MB
      </div>
      <div v-else-if="prefetchParakeetDonePath" class="field-hint">Downloaded to: <code>{{ prefetchParakeetDonePath }}</code></div>
    </div>

  </CollapsibleCard>

  <CollapsibleCard
    v-if="props.settings.stt_engine === 'openai'"
    id="settings.stt.cloud"
    title="Cloud endpoint"
    desc="Any server implementing POST /v1/audio/transcriptions."
  >
    <div class="field">
      <div class="row-label">
        <label class="field-label">Model</label>
        <span class="info-icon" :title="infoTitle('Choose a suggested model. No free text field: the goal is to keep this predictable.')">i</span>
      </div>

      <p v-if="cloudModelNotOnOpenai" class="field-hint error">
        <code>{{ props.settings.stt_cloud_model }}</code> is not a model OpenAI hosts, and the base URL points at
        api.openai.com. The request will be rejected. Either pick a GPT model below, or point the base URL at a server
        that serves this one.
      </p>

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

    <div v-if="props.settings.stt_engine === 'openai'" class="field">
      <div class="row-label">
        <label class="field-label">Cloud STT Base URL</label>
        <span class="info-icon" :title="infoTitle('Must support POST /v1/audio/transcriptions (OpenAI compatible).')">i</span>
      </div>
      <div class="actions">
        <input v-model="props.settings.stt_cloud_base_url" class="input" placeholder="https://api.openai.com" />
        <button class="btn" @click="props.settings.stt_cloud_base_url = 'https://api.openai.com'">Use OpenAI</button>
      </div>
    </div>

    <div v-if="props.settings.stt_engine === 'openai'" class="field">
      <div class="row-label">
        <label class="field-label">Cloud STT API Key (optional)</label>
        <span class="info-icon" :title="infoTitle('Only used for non-OpenAI base URLs that require auth. For OpenAI base URL, the OpenAI API key is used.')">i</span>
      </div>
      <div class="actions">
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

  </CollapsibleCard>

  <CollapsibleCard
    id="settings.stt.postprocess"
    title="Post-processing"
    desc="Optional cleanup pass over the transcript. Switched on in the STT panel."
  >
    <div class="field">
      <div class="row-label">
        <label class="field-label">Model</label>
        <span class="info-icon" :title="infoTitle('Select the model used for STT post-processing. Enable/disable and prompt are configured in the STT view.')">i</span>
      </div>

      <div class="actions">
        <select v-model="props.settings.stt_post_process_model" class="input">
          <option v-for="m in postProcessModelOptions" :key="m" :value="m">{{ m }}</option>
        </select>
        <button class="btn" :disabled="props.models?.loading" @click="props.onRefreshModels?.()">
          {{ props.models?.loading ? 'Fetching…' : 'Fetch Models' }}
        </button>
      </div>
      <div v-if="props.models?.error" class="field-hint error">{{ props.models.error }}</div>
    </div>

  </CollapsibleCard>

  <CollapsibleCard
    id="settings.stt.command"
    title="Command Mode"
    desc="Run a script with the transcript instead of pasting it."
    :default-open="false"
  >
    <div class="field">
      <div class="field-hint">
        Command Mode adds a fifth Quick Action: press <code>C</code> in the popup to record a voice command. Instead of pasting the transcript, AiDesktopCompanion runs a script from <code>%APPDATA%/AiDesktopCompanion/hooks/</code> and passes the transcript on stdin.
      </div>
    </div>

    <div class="field-row">
      <label class="checkbox">
        <input type="checkbox" v-model="props.settings.command_enabled" />
        Enable Command Mode
      </label>
    </div>

    <div class="field" :style="{ opacity: props.settings.command_enabled ? 1 : 0.6 }">
      <div class="row-label">
        <label class="field-label">Active Script</label>
        <span class="info-icon" :title="infoTitle('Scripts are discovered from %APPDATA%/AiDesktopCompanion/hooks/. Supported extensions: .ps1, .cmd, .bat, .exe')">i</span>
      </div>
      <div class="actions">
        <select
          v-model="props.settings.command_active_script"
          class="input"
          :disabled="!props.settings.command_enabled || commandScriptsBusy || commandScripts.length === 0"
        >
          <option value="" :disabled="commandScripts.length > 0">(none - click Create default script to start)</option>
          <option v-for="name in commandScripts" :key="name" :value="name">{{ name }}</option>
        </select>
        <button class="btn ghost" :disabled="!props.settings.command_enabled || commandScriptsBusy" @click="refreshCommandScripts">
          {{ commandScriptsBusy ? 'Refreshing…' : 'Refresh' }}
        </button>
      </div>
      <div class="actions">
        <button
          v-if="commandScripts.length === 0"
          class="btn"
          :disabled="!props.settings.command_enabled || commandScriptOpBusy"
          @click="createDefaultCommandScript"
        >
          {{ commandScriptOpBusy ? 'Creating…' : 'Create default script' }}
        </button>
        <button
          v-else
          class="btn ghost"
          :disabled="!props.settings.command_enabled || commandScriptOpBusy"
          @click="openCommandHooksFolder"
        >
          {{ commandScriptOpBusy ? 'Opening…' : 'Open hooks folder' }}
        </button>
      </div>
      <div class="field-hint" v-if="!commandScriptsError && commandScripts.length === 0">No scripts found in hooks directory yet.</div>
      <div class="field-hint error" v-if="commandScriptsError">{{ commandScriptsError }}</div>
    </div>

    <div class="field" :style="{ opacity: props.settings.command_enabled ? 1 : 0.6 }">
      <div class="row-label">
        <label class="field-label">Hook timeout (seconds)</label>
        <span class="info-icon" :title="infoTitle('Maximum allowed runtime for a command hook process before it is killed.')">i</span>
      </div>
      <input
        type="number"
        class="input w-sm"
        min="5"
        max="3600"
        step="1"
        v-model.number="props.settings.command_hook_timeout_secs"
        :disabled="!props.settings.command_enabled"
        @blur="props.settings.command_hook_timeout_secs = Math.min(3600, Math.max(5, Math.floor(Number(props.settings.command_hook_timeout_secs || 120))))"
      />
    </div>
  </CollapsibleCard>
</template>

<style scoped>
/* Only the pieces with no equivalent in the shared layer live here: the
   model picker, the label-plus-info-icon row, and the disclosure "i". */

.row-label {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
}

/* A hoverable "i" rather than a paragraph of hint text under every control -
   this screen has fifteen of them and they would bury the controls. */
.info-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 15px;
  height: 15px;
  border-radius: 50%;
  border: 1px solid var(--adc-border-strong);
  color: var(--adc-fg-muted);
  font-size: 10px;
  font-style: italic;
  font-weight: 600;
  cursor: help;
  user-select: none;
  flex: 0 0 auto;
}
.info-icon:hover { color: var(--adc-fg); border-color: var(--adc-accent); }

/* Model picker: a radio group that needs room for a name, a rationale and an
   action, so it is a list of rows rather than a <select>. */
.model-list {
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
}

.model-item {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-3);
  border: 1px solid var(--adc-border);
  border-radius: var(--radius-sm);
  background: var(--adc-bg);
  cursor: pointer;
  transition: border-color 0.15s ease, background 0.15s ease;
}
.model-item:hover { border-color: var(--adc-border-strong); background: var(--adc-hover); }
.model-item.active {
  border-color: var(--adc-accent);
  background: var(--adc-hover);
}

.model-main { flex: 1; min-width: 0; }
.model-name { font-size: var(--fs-base); color: var(--adc-fg); font-weight: 500; }
.model-hint {
  font-size: var(--fs-sm);
  color: var(--adc-fg-muted);
  margin-top: 2px;
  line-height: 1.45;
}

.model-meta {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  flex: 0 0 auto;
}
.model-active {
  font-size: var(--fs-xs);
  font-weight: 600;
  color: var(--adc-accent);
  text-transform: uppercase;
  letter-spacing: 0.04em;
}

/* Paths appear inline in hints all over this screen and are long enough to
   force horizontal scroll if they are not allowed to break. */
code {
  font-family: var(--font-mono);
  font-size: 0.95em;
  background: var(--adc-surface-2);
  border-radius: var(--radius-sm);
  padding: 1px 5px;
  overflow-wrap: anywhere;
}
</style>
