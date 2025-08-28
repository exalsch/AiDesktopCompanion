<script setup lang="ts">
import { reactive, ref, onMounted, onBeforeUnmount, watch } from 'vue'
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

// OpenAI voices (static list; API does not expose a public voices endpoint)
const openaiVoiceOptions = ref<string[]>([
  'alloy','verse','amber','onyx','coral','sage','nova','shimmer','pebble'
])
// OpenAI models (load from backend; fallback defaults)
const openaiModelOptions = ref<string[]>(['gpt-4o-mini-tts', 'tts-1', 'tts-1-hd'])

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
      // OpenAI: synthesize to WAV then play via HTML audio
      busy.value = true
      const path = await invoke<string>('tts_openai_synthesize_wav', { text: form.text, voice: form.openaiVoice || 'alloy', model: form.openaiModel || 'gpt-4o-mini-tts', rate: form.rate, volume: form.volume })
      busy.value = false
      wavPath.value = path
      wavSrc.value = convertFileSrc(path)
      lastPlayTempPath.value = path
      // Auto-play
      requestAnimationFrame(() => {
        const a = playerRef.value
        if (a) {
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

async function onSynthesize() {
  if (!form.text.trim()) {
    props.notify?.('Enter some text to synthesize', 'error')
    return
  }
  try {
    busy.value = true
    const path = engine.value === 'local'
      ? await invoke<string>('tts_synthesize_wav', { text: form.text, voice: form.voice || null, rate: form.rate, volume: form.volume })
      : await invoke<string>('tts_openai_synthesize_wav', { text: form.text, voice: (form.openaiVoice || 'alloy'), model: (form.openaiModel || 'gpt-4o-mini-tts'), rate: form.rate, volume: form.volume })
    busy.value = false
    wavPath.value = path
    wavSrc.value = convertFileSrc(path)

    // Prompt Save As... dialog
    const suggested = 'speech.wav'
    const dest = await saveDialog({
      defaultPath: suggested,
      filters: [{ name: 'WAV audio', extensions: ['wav'] }],
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
      props.notify?.(`WAV created in temp folder:\n${path}`, 'success')
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
      <textarea v-model="form.text" rows="4" placeholder="Type something to speak…" />
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
        <div class="hint">Pick a model or type a custom one. Default is gpt-4o-mini-tts. ‼️ Options are suggestions.</div>
      </div>
      <div class="cell" v-if="engine === 'openai'">
        <label class="label">Voice (OpenAI)</label>
        <input class="input" v-model="form.openaiVoice" list="openai-voices" placeholder="alloy" />
        <datalist id="openai-voices">
          <option v-for="v in openaiVoiceOptions" :key="v" :value="v" />
        </datalist>
        <div class="hint">Select a voice or type a custom one. Default is "alloy". ‼️ Suggestions list not exhaustive.</div>
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
      <button class="btn" @click="onSynthesize">Save to WAV</button>
    </div>

    <div v-if="wavPath" class="row">
      <div class="label">Last output</div>
      <div class="hint">{{ wavPath }}</div>
      <audio ref="playerRef" v-if="wavSrc" :src="wavSrc" controls preload="none" />
    </div>

    <div class="hint">Note: Local engine uses Windows PowerShell System.Speech. OpenAI engine synthesizes to WAV using your OpenAI API key from Settings. Rate/Volume affect both OpenAI playback and saved WAV. (Speed changes will also change pitch.)</div>
  </div>
</template>

<style scoped>
.tts { display: flex; flex-direction: column; gap: 10px; }
.row { display: flex; flex-direction: column; gap: 6px; }
.row.inline { flex-direction: row; align-items: center; gap: 10px; flex-wrap: wrap; }
.cell { display: flex; flex-direction: column; gap: 6px; }
.label { font-size: 12px; color: #c8c9d3; }
.input { padding: 8px 10px; border-radius: 8px; border: 1px solid #3a3a44; background: #1f1f26; color: #fff; }
textarea { width: 100%; resize: vertical; min-height: 100px; padding: 8px; border-radius: 8px; border: 1px solid #3a3a44; background: #14141a; color: #e0e0ea; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.btn.ghost { background: transparent; }
.btn.danger { background: #a42828; border-color: #7c1f1f; }
.hint { font-size: 12px; color: #9fa0aa; white-space: pre-line; }
.hint.error { color: #f2b8b8; }
audio { width: 100%; margin-top: 6px; }
</style>
