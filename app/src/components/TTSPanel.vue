<script setup lang="ts">
import { reactive, ref, onMounted, onBeforeUnmount, watch, nextTick } from 'vue'
import { emit as emitTauri, listen } from '@tauri-apps/api/event'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'

const props = defineProps<{ notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void }>()
const emit = defineEmits<{ (e: 'busy', v: boolean): void }>()

const form = reactive({
  text: '' as string,
  voice: '' as string,
  rate: -2 as number, // -10..10
  volume: 100 as number, // 0..100
  // OpenAI TTS fields
  openaiVoice: 'alloy' as string,
  openaiModel: 'gpt-4o-mini-tts' as string,
  openaiFormat: 'wav' as 'wav' | 'mp3' | 'opus',
  openaiStreaming: false as boolean,
  openaiInstructions: '' as string,
})

const speaking = ref(false)
const busy = ref(false) // shows LoadingDots during synthesis (OpenAI play or any synth operation)
const engine = ref<'local' | 'openai'>('local')
const wavPath = ref('')
const wavSrc = ref('')
const voices = ref<string[]>([])
const loadingVoices = ref(false)
const err = ref('')
const playerRef = ref<HTMLAudioElement | null>(null)
let cleanupTimer: any = 0
const lastPlayTempPath = ref('') // temp wav created by onPlay (OpenAI) only; safe to delete after playback/stop

// Streaming state (OpenAI) - using local HTTP proxy (no MSE)
let streamSessionUrl = ref('')
let streamSessionId: string | null = null

// OpenAI voices (static list; API does not expose a public voices endpoint)
const openaiVoiceOptions = ref<string[]>([
  'alloy','verse','amber','onyx','coral','sage','nova','shimmer','pebble'
])
// OpenAI models (load from backend; fallback defaults)
const openaiModelOptions = ref<string[]>(['gpt-4o-mini-tts', 'tts-1', 'tts-1-hd'])
const openaiFormatOptions = ref<Array<'wav'|'mp3'|'opus'>>(['wav','mp3','opus'])

// OpenAI rate/volume are applied server-side into the saved WAV to keep playback and export consistent.

async function loadVoices() {
  loadingVoices.value = true
  err.value = ''
  try {
    const list = await invoke<string[]>('tts_list_voices')
    voices.value = list
    if (list.length && !voices.value.includes(form.voice)) {
      form.voice = list[0]
    }
  } catch (e: any) {
    err.value = e?.message || String(e) || 'Failed to list voices'
    props.notify?.(`TTS voices failed: ${err.value}`, 'error')
  } finally {
    loadingVoices.value = false
  }
}

async function onPlay() {
  if (!form.text.trim()) {
    props.notify?.('Enter some text to speak', 'error')
    return
  }
  try {
    if (engine.value === 'local') {
      await invoke('tts_start', { text: form.text, voice: form.voice || null, rate: form.rate, volume: form.volume })
      speaking.value = true
    } else {
      if (form.openaiStreaming) {
        // Streaming via local HTTP proxy
        await startProxyStreaming()
      } else {
        // Non-streaming synth-then-play
        busy.value = true
        const fmt = form.openaiFormat || 'wav'
        const path = await invoke<string>('tts_openai_synthesize_file', { text: form.text, voice: form.openaiVoice || 'alloy', model: form.openaiModel || 'gpt-4o-mini-tts', format: fmt, rate: form.rate, volume: form.volume, instructions: form.openaiInstructions || null })
        busy.value = false
        wavPath.value = path
        wavSrc.value = convertFileSrc(path)
        lastPlayTempPath.value = path
        // Auto-play
        requestAnimationFrame(() => {
          const a = playerRef.value
          if (a) {
            // Apply rate/volume locally for non-streaming path
            const factor = Math.max(0.25, Math.min(4, Math.pow(2, (form.rate || 0) / 10)))
            a.playbackRate = factor
            a.volume = Math.max(0, Math.min(1, (form.volume || 100) / 100))
            a.currentTime = 0
            a.play().catch(() => {})
            speaking.value = true
            a.onended = async () => {
              speaking.value = false
              // Cleanup temp WAV only if it was created by Play (OpenAI)
              const p = lastPlayTempPath.value
              if (p) {
                try { await invoke<boolean>('tts_delete_temp_wav', { path: p }) } catch {}
                if (wavPath.value === p) { wavPath.value = ''; wavSrc.value = '' }
                lastPlayTempPath.value = ''
              }
            }
          }
        })
      }
    }
  } catch (e: any) {
    const msg = e?.message || String(e) || 'TTS start failed'
    props.notify?.(msg, 'error')
    busy.value = false
  }
}

async function onStop() {
  try {
    if (engine.value === 'local') {
      await invoke('tts_stop')
    } else {
      // Stop streaming if active (proxy)
      await stopProxyStreaming()
      const a = playerRef.value
      if (a) { a.pause(); a.currentTime = 0 }
      // Cleanup temp WAV if stopping during OpenAI playback
      const p = lastPlayTempPath.value
      if (p) {
        try { await invoke<boolean>('tts_delete_temp_wav', { path: p }) } catch {}
        if (wavPath.value === p) { wavPath.value = ''; wavSrc.value = '' }
        lastPlayTempPath.value = ''
      }
    }
  } catch {}
  finally {
    speaking.value = false
  }
}

async function startProxyStreaming() {
  busy.value = true
  await nextTick()
  const a = playerRef.value
  if (!a) { busy.value = false; props.notify?.('Audio element not ready', 'error'); return }
  // Ask backend to create a streaming session and return a local URL
  let url = ''
  try {
    // Verify the selected format is playable; if not, fallback to MP3 for progressive streaming
    const desiredFmt = (form.openaiFormat || 'mp3') as 'wav'|'mp3'|'opus'
    const fmtToMime: Record<string, string> = { wav: 'audio/wav', mp3: 'audio/mpeg', opus: 'audio/ogg' }
    let chosenFmt: 'wav'|'mp3'|'opus' = desiredFmt
    const mime = fmtToMime[desiredFmt] || 'audio/mpeg'
    try {
      const support = a.canPlayType(mime)
      if (!support) chosenFmt = 'mp3'
    } catch {
      chosenFmt = 'mp3'
    }
    if (chosenFmt !== desiredFmt) {
      props.notify?.(`Selected format ${desiredFmt.toUpperCase()} not supported for streaming. Falling back to MP3.`, 'error', 3000)
    }
    url = await invoke<string>('tts_create_stream_session', {
      text: form.text,
      voice: form.openaiVoice || 'alloy',
      model: form.openaiModel || 'gpt-4o-mini-tts',
      format: chosenFmt || 'mp3',
      instructions: form.openaiInstructions || null,
    })
  } catch (e: any) {
    busy.value = false
    props.notify?.(e?.message || String(e) || 'Failed to start streaming session', 'error')
    return
  }
  streamSessionUrl.value = url
  streamSessionId = (url.split('/').pop() || '').trim() || null
  // Apply rate/volume and begin playback
  const factor = Math.max(0.25, Math.min(4, Math.pow(2, (form.rate || 0) / 10)))
  a.playbackRate = factor
  a.volume = Math.max(0, Math.min(1, (form.volume || 100) / 100))
  a.src = url
  a.currentTime = 0
  a.play().then(() => {
    speaking.value = true
    busy.value = false
  }).catch(err => {
    busy.value = false
    props.notify?.(String(err) || 'Failed to start playback', 'error')
  })
  // Auto-fallback if the audio element reports a playback error (e.g., unsupported container or non-audio response)
  a.onerror = async () => {
    try { await stopProxyStreaming() } catch {}
    // Fallback to non-streaming MP3 synth to maximize compatibility
    try {
      busy.value = true
      const path = await invoke<string>('tts_openai_synthesize_file', {
        text: form.text,
        voice: form.openaiVoice || 'alloy',
        model: form.openaiModel || 'gpt-4o-mini-tts',
        format: 'mp3',
        rate: form.rate,
        volume: form.volume,
        instructions: form.openaiInstructions || null,
      })
      busy.value = false
      wavPath.value = path
      wavSrc.value = convertFileSrc(path)
      lastPlayTempPath.value = path
      requestAnimationFrame(() => {
        const el = playerRef.value
        if (el) {
          const factor2 = Math.max(0.25, Math.min(4, Math.pow(2, (form.rate || 0) / 10)))
          el.playbackRate = factor2
          el.volume = Math.max(0, Math.min(1, (form.volume || 100) / 100))
          el.currentTime = 0
          el.play().catch(() => {})
          speaking.value = true
        }
      })
    } catch (e: any) {
      busy.value = false
      props.notify?.(e?.message || String(e) || 'Fallback playback failed', 'error')
    }
  }
  a.onended = () => {
    speaking.value = false
    // Best-effort: stop session to free memory
    if (streamSessionId) {
      invoke('tts_stop_stream_session', { session_id: streamSessionId }).catch(() => {})
    }
    streamSessionId = null
    streamSessionUrl.value = ''
  }
}

async function stopProxyStreaming() {
  const a = playerRef.value
  if (a) {
    try { a.pause() } catch {}
    if (streamSessionUrl.value && a.src === streamSessionUrl.value) {
      a.src = ''
    }
  }
  if (streamSessionId) {
    try { await invoke('tts_stop_stream_session', { session_id: streamSessionId }) } catch {}
  }
  streamSessionId = null
  streamSessionUrl.value = ''
  speaking.value = false
  busy.value = false
}

async function onSynthesize() {
  if (!form.text.trim()) {
    props.notify?.('Enter some text to synthesize', 'error')
    return
  }
  try {
    busy.value = true
    const path = engine.value === 'local'
      ? await invoke<string>('tts_synthesize_wav', { text: form.text, voice: form.voice || null, rate: form.rate, volume: form.volume })
      : await invoke<string>('tts_openai_synthesize_file', { text: form.text, voice: (form.openaiVoice || 'alloy'), model: (form.openaiModel || 'gpt-4o-mini-tts'), format: (form.openaiFormat || 'wav'), rate: form.rate, volume: form.volume, instructions: form.openaiInstructions || null })
    busy.value = false
    wavPath.value = path
    wavSrc.value = convertFileSrc(path)

    // Prompt Save As... dialog
    const suggested = `speech.${engine.value === 'openai' ? form.openaiFormat : 'wav'}`
    const filters = engine.value === 'openai'
      ? (
        form.openaiFormat === 'mp3'
          ? [{ name: 'MP3 audio', extensions: ['mp3'] }]
          : form.openaiFormat === 'opus'
            ? [{ name: 'OPUS audio', extensions: ['opus', 'ogg'] }]
            : [{ name: 'WAV audio', extensions: ['wav'] }]
      )
      : [{ name: 'WAV audio', extensions: ['wav'] }]
    const dest = await saveDialog({
      defaultPath: suggested,
      filters,
      title: 'Save synthesized audio as...'
    } as any)
    if (dest && typeof dest === 'string') {
      try {
        const out = await invoke<string>('copy_file_to_path', { src: path, dest, overwrite: true })
        props.notify?.(`Saved to:\n${out}`, 'success')
        wavPath.value = out
        wavSrc.value = convertFileSrc(out)
      } catch (e: any) {
        props.notify?.(e?.message || String(e) || 'Copy failed', 'error')
      }
    } else {
      props.notify?.(`File instead created in temp folder:\n${path}` , 'success')
    }
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Synthesize failed'
    props.notify?.(msg, 'error')
  } finally {
    busy.value = false
  }
}

// Persist/restore TTS selections via settings
let saveDebounce: any = 0
async function loadTtsSettings() {
  try {
    const v = await invoke<any>('get_settings')
    if (v && typeof v === 'object') {
      if (typeof v.tts_engine === 'string' && (v.tts_engine === 'local' || v.tts_engine === 'openai')) engine.value = v.tts_engine
      if (typeof v.tts_rate === 'number') form.rate = v.tts_rate
      if (typeof v.tts_volume === 'number') form.volume = v.tts_volume
      if (typeof v.tts_voice_local === 'string') form.voice = v.tts_voice_local
      if (typeof v.tts_openai_voice === 'string') form.openaiVoice = v.tts_openai_voice
      if (typeof v.tts_openai_model === 'string') form.openaiModel = v.tts_openai_model
      if (typeof (v as any).tts_openai_format === 'string') {
        const f = String((v as any).tts_openai_format).toLowerCase()
        if (['wav','mp3','opus'].includes(f)) form.openaiFormat = f as any
      }
      if (typeof (v as any).tts_openai_streaming === 'boolean') form.openaiStreaming = !!(v as any).tts_openai_streaming
    }
  } catch {}
}

// Ensure settings are applied before playback when triggered programmatically
let ttsSettingsLoaded = false
let ttsSettingsLoading: Promise<void> | null = null
async function ensureTtsSettingsLoaded() {
  if (ttsSettingsLoaded) return
  if (!ttsSettingsLoading) {
    ttsSettingsLoading = (async () => {
      try {
        await loadTtsSettings()
      } finally {
        ttsSettingsLoaded = true
      }
    })()
  }
  await ttsSettingsLoading
}

function scheduleSaveTtsSettings() {
  if (saveDebounce) clearTimeout(saveDebounce)
  saveDebounce = setTimeout(async () => {
    try {
      await invoke<string>('save_settings', { map: {
        tts_engine: engine.value,
        tts_rate: form.rate,
        tts_volume: form.volume,
        tts_voice_local: form.voice,
        tts_openai_voice: form.openaiVoice,
        tts_openai_model: form.openaiModel,
        tts_openai_format: form.openaiFormat,
        tts_openai_streaming: form.openaiStreaming,
        tts_openai_instructions: form.openaiInstructions,
      } })
    } catch {}
  }, 300)
}

watch(engine, scheduleSaveTtsSettings)
watch(() => form.rate, scheduleSaveTtsSettings)
watch(() => form.volume, scheduleSaveTtsSettings)
watch(() => form.voice, scheduleSaveTtsSettings)
watch(() => form.openaiVoice, scheduleSaveTtsSettings)
watch(() => form.openaiModel, scheduleSaveTtsSettings)
watch(() => form.openaiFormat, scheduleSaveTtsSettings)
watch(() => form.openaiStreaming, scheduleSaveTtsSettings)

onMounted(() => {
  loadVoices().catch(() => {})
  ensureTtsSettingsLoaded().catch(() => {})
  loadOpenAIModels().catch(() => {})
  // Kick off stale cleanup now and periodically (every 30 minutes)
  invoke('cleanup_stale_tts_wavs', { maxAgeMinutes: 240 }).catch(() => {})
  cleanupTimer = setInterval(() => { invoke('cleanup_stale_tts_wavs', { maxAgeMinutes: 240 }).catch(() => {}) }, 30 * 60 * 1000)
})
onBeforeUnmount(() => {
  if (cleanupTimer) clearInterval(cleanupTimer)
  if (speaking.value) onStop().catch(() => {})
})

watch(busy, (v) => emit('busy', !!v))

// Broadcast speaking state so other parts of the app (e.g., background controller) can react
watch(speaking, (v) => {
  try { emitTauri('tts:speaking', { speaking: !!v }) } catch {}
})

async function loadOpenAIModels() {
  try {
    const models = await invoke<string[]>('list_openai_models')
    // Keep only likely TTS-capable models
    const filtered = (models || []).filter(m => m.includes('tts') || m.includes('4o') || m.includes('audio'))
    if (filtered.length) {
      openaiModelOptions.value = Array.from(new Set([...filtered, ...openaiModelOptions.value]))
      if (!openaiModelOptions.value.includes(form.openaiModel)) {
        form.openaiModel = 'gpt-4o-mini-tts'
      }
    }
  } catch {}
}

// Expose programmatic API for parent (App.vue)
defineExpose({
  setText(text: string) { form.text = text || '' },
  async play() { await ensureTtsSettingsLoaded(); await onPlay() },
  async stop() { await onStop() },
  async setTextAndPlay(text: string) { form.text = text || ''; await ensureTtsSettingsLoaded(); await onPlay() },
})

</script>

<template>
  <div class="tts">
    <div class="row inline">
      <div class="cell">
        <label class="label">Engine</label>
        <select v-model="engine" class="input">
          <option value="local">Local (Windows)</option>
          <option value="openai">OpenAI</option>
        </select>
      </div>
    </div>

    <div class="row">
      <label class="label">Text</label>
      <textarea v-model="form.text" rows="4" class="input" placeholder="Type something to speak…" @keydown.enter.exact.prevent="onPlay" />
    </div>

    <div class="row inline">
      <div class="cell" v-if="engine === 'local'">
        <label class="label">Voice (local)</label>
        <div class="inline">
          <select v-model="form.voice" class="input">
            <option value="">(Default)</option>
            <option v-for="v in voices" :key="v" :value="v">{{ v }}</option>
          </select>
          <button class="btn ghost" :disabled="loadingVoices" @click="loadVoices">{{ loadingVoices ? 'Loading…' : 'Reload' }}</button>
        </div>
        <div v-if="err" class="hint error">{{ err }}</div>
      </div>
      <div class="cell" v-else>
        <label class="label">Model (OpenAI)</label>
        <input class="input" v-model="form.openaiModel" list="openai-models" placeholder="gpt-4o-mini-tts" />
        <datalist id="openai-models">
          <option v-for="m in openaiModelOptions" :key="m" :value="m" />
        </datalist>
        <div class="hint">Pick a model or type a custom one. Default is gpt-4o-mini-tts. Options might not be tts compatible.</div>
      </div>
      <div class="cell" v-if="engine === 'openai'">
        <label class="label">Voice (OpenAI)</label>
        <input class="input" v-model="form.openaiVoice" list="openai-voices" placeholder="alloy" />
        <datalist id="openai-voices">
          <option v-for="v in openaiVoiceOptions" :key="v" :value="v" />
        </datalist>
        <div class="hint">Select a voice or type a custom one. Default is "alloy". Suggestions list might be incomplete.</div>
      </div>
      <div class="cell" v-if="engine === 'openai'">
        <label class="label">Format</label>
        <select v-model="(form.openaiFormat as any)" class="input">
          <option v-for="f in openaiFormatOptions" :key="f" :value="f">{{ f.toUpperCase() }}</option>
        </select>
        <div class="hint">OPUS can reduce latency and size. WAV is PCM16-compatible with Windows playback.</div>
      </div>
      <div class="cell" v-if="engine === 'openai'">
        <label class="label">Instructions (optional)</label>
        <input class="input" v-model="form.openaiInstructions" placeholder="e.g. Cheerful and positive tone" />
        <div class="hint">Sent to OpenAI to influence speaking style.</div>
      </div>
      <div class="cell" v-if="engine === 'openai'">
        <label class="label" title="Streaming uses a local HTTP proxy to progressively play audio (MP3/WAV/OPUS). If playback isn’t supported or fails, it automatically falls back to non‑streaming synth‑then‑play.">Streaming (experimental)</label>
        <div class="checkbox" title="Streaming uses a local HTTP proxy to progressively play audio (MP3/WAV/OPUS). If playback isn’t supported or fails, it automatically falls back to non‑streaming synth‑then‑play.">
          <input type="checkbox" v-model="form.openaiStreaming" />
          <span>Enable streaming when supported</span>
        </div>
      </div>
      <div class="cell">
        <label class="label">Rate: {{ form.rate }}</label>
        <input type="range" min="-10" max="10" step="1" v-model.number="form.rate" />
      </div>
      <div class="cell">
        <label class="label">Volume: {{ form.volume }}</label>
        <input type="range" min="0" max="100" step="1" v-model.number="form.volume" />
      </div>
    </div>

    <div class="row inline">
      <button class="btn" :disabled="speaking || busy" @click="onPlay">{{ busy && engine === 'openai' ? 'Synthesizing…' : 'Play' }}</button>
      <button class="btn danger" :disabled="!speaking" @click="onStop">Stop</button>
      <button class="btn" @click="onSynthesize">Save</button>
    </div>

    <div v-if="wavPath || (engine === 'openai' && form.openaiStreaming)" class="row">
      <template v-if="wavPath">
        <div class="label">Last output</div>
        <div class="hint">{{ wavPath }}</div>
      </template>
      <audio ref="playerRef" :src="wavSrc || ''" controls preload="none" />
    </div>

    <div class="hint">Note: Local engine uses Windows PowerShell System.Speech. </div>
  </div>
</template>

<style scoped>
.tts { display: flex; flex-direction: column; gap: 10px; }
.row { display: flex; flex-direction: column; gap: 6px; }
.row.inline { flex-direction: row; align-items: center; gap: 10px; flex-wrap: wrap; }
.cell { display: flex; flex-direction: column; gap: 6px; }
.label { font-size: 12px; color: var(--adc-fg-muted); }
.input { padding: 8px 10px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); }
textarea { width: 100%; resize: vertical; min-height: 100px; padding: 8px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); box-sizing: border-box; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-accent); color: #fff; cursor: pointer; }
.btn.ghost { background: transparent; color: var(--adc-fg); }
.btn.danger { background: var(--adc-danger); border-color: var(--adc-danger); }
.hint { font-size: 12px; color: var(--adc-fg-muted); white-space: pre-line; }
.hint.error { color: #f2b8b8; }
audio { width: 100%; margin-top: 6px; }
</style>
