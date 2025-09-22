<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch, computed } from 'vue'
import { emit as emitTauri } from '@tauri-apps/api/event'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { useTtsPlayback } from '../composables/useTtsPlayback'
import { useSettings } from '../composables/useSettings'
import { estimateTextTokens, formatTokenInfo } from '../composables/useTokenEstimate'
import { tokenizerReady } from '../composables/useTokenizer'

const props = defineProps<{ notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void; lightMount?: boolean }>()
const emit = defineEmits<{ (e: 'busy', v: boolean): void }>()

const { engine, form: formFromComposable, speaking, busy, wavPath, wavSrc, lastPlayTempPath, playerRef, onPlay, onStop, onSynthesize, startProxyStreaming, stopProxyStreaming } = useTtsPlayback(props.notify)
// Alias for local usage
const form = formFromComposable
const voices = ref<string[]>([])
const loadingVoices = ref(false)
const err = ref('')
let cleanupTimer: any = 0

// Streaming handled in composable when engine === 'openai' and form.openaiStreaming

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

async function onSynthesizeWithSave() {
  // Save only if we already have a synthesized file
  if (!wavPath.value || !String(wavPath.value).trim()) {
    props.notify?.('No output to save yet. Press Play or Synthesize first.', 'error')
    return
  }
  try {
    const fmt = engine.value === 'openai' ? form.openaiFormat : 'wav'
    const suggested = `speech.${fmt}`
    const filters = engine.value === 'openai'
      ? (form.openaiFormat === 'mp3' ? [{ name: 'MP3 audio', extensions: ['mp3'] }] : form.openaiFormat === 'opus' ? [{ name: 'OPUS audio', extensions: ['opus', 'ogg'] }] : [{ name: 'WAV audio', extensions: ['wav'] }])
      : [{ name: 'WAV audio', extensions: ['wav'] }]
    const dest = await saveDialog({ defaultPath: suggested, filters, title: 'Save synthesized audio as...' } as any)
    if (dest && typeof dest === 'string') {
      try {
        const out = await invoke<string>('copy_file_to_path', { src: wavPath.value, dest, overwrite: true })
        props.notify?.(`Saved to:\n${out}`, 'success')
        wavPath.value = out
        wavSrc.value = convertFileSrc(out)
      } catch (e: any) {
        props.notify?.(e?.message || String(e) || 'Copy failed', 'error')
      }
    }
  } catch {}
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
  if (!props.lightMount) {
    loadVoices().catch(() => {})
    ensureTtsSettingsLoaded().catch(() => {})
    // Kick off stale cleanup now and periodically (every 30 minutes)
    invoke('cleanup_stale_tts_wavs', { maxAgeMinutes: 240 }).catch(() => {})
    cleanupTimer = setInterval(() => { invoke('cleanup_stale_tts_wavs', { maxAgeMinutes: 240 }).catch(() => {}) }, 30 * 60 * 1000)
  }
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

defineExpose({
  setText(text: string) { form.text = text || '' },
  async play() { await ensureTtsSettingsLoaded(); await onPlay() },
  async stop() { await onStop() },
  async setTextAndPlay(text: string) { form.text = text || ''; await ensureTtsSettingsLoaded(); await onPlay() },
})

// Token hint for unsent TTS text (approximate or tokenizer-based)
const { settings } = useSettings()
const ttsModelName = computed(() => engine.value === 'openai' ? form.openaiModel : settings.openai_chat_model)
const tokenizerMode = computed(() => settings.tokenizer_mode)
const ttsTextTokens = computed(() => {
  const _ready = tokenizerReady.value
  return estimateTextTokens(form.text || '', ttsModelName.value, tokenizerMode.value).tokens
})
const ttsTokenHint = computed(() => formatTokenInfo([{ label: 'text', tokens: ttsTextTokens.value }]))

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
      <div class="hint">{{ ttsTokenHint }}</div>
    </div>

    <!-- Controls: Play/Stop + Save (just below text input) -->
    <div class="row inline">
      <button
        class="btn"
        :class="{ danger: speaking }"
        :disabled="busy && !speaking"
        @click="speaking ? onStop() : onPlay()"
      >{{ speaking ? 'Stop' : (busy && engine === 'openai' ? 'Synthesizing…' : 'Play') }}</button>
      <button class="btn" :disabled="!wavPath" @click="onSynthesizeWithSave">Save to file</button>
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
      <div class="cell" v-if="engine === 'openai'">
        <label class="label">Voice tone (optional)</label>
        <input class="input" v-model="form.openaiInstructions" placeholder="e.g. Cheerful and positive tone" />
        <div class="hint">Optional hint sent to OpenAI to influence speaking style/tone.</div>
      </div>
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
