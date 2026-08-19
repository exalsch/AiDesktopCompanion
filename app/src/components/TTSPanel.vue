<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount, watch, computed } from 'vue'
import { emit as emitTauri } from '@tauri-apps/api/event'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { useTtsPlayback, OPENAI_TTS_MAX_INPUT_CHARS } from '../composables/useTtsPlayback'
import { useSettings } from '../composables/useSettings'
import { estimateTextTokens, formatTokenInfo } from '../composables/useTokenEstimate'
import { tokenizerReady } from '../composables/useTokenizer'
import CollapsibleCard from './ui/CollapsibleCard.vue'

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
const hasSavableOutput = computed(() => !!String(wavPath.value || '').trim())
const openaiInputLength = computed(() => form.text.trim().length)
const openaiTextTooLong = computed(() => engine.value === 'openai' && openaiInputLength.value > OPENAI_TTS_MAX_INPUT_CHARS)

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
      if (typeof (v as any).tts_openai_instructions === 'string') form.openaiInstructions = (v as any).tts_openai_instructions
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
  if (saveDebounce) { clearTimeout(saveDebounce); saveDebounce = 0 }
  if (speaking.value) onStop().catch(() => {})
})

watch(busy, (v) => emit('busy', !!v))

watch([
  () => form.text,
  () => engine.value,
  () => form.voice,
  () => form.openaiVoice,
  () => form.openaiModel,
  () => form.openaiFormat,
  () => form.openaiInstructions,
  () => form.rate,
  () => form.volume,
], () => {
  if (busy.value || speaking.value) return
  if (wavPath.value) {
    wavPath.value = ''
    wavSrc.value = ''
  }
})

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
  <section class="card">
    <div class="card-body">
      <div class="field-grid">
        <div class="field">
          <label class="field-label">Engine</label>
          <select v-model="engine" class="input">
            <option value="local">Local (Windows)</option>
            <option value="openai">OpenAI</option>
          </select>
        </div>
      </div>

      <div class="field">
        <label class="field-label">Text</label>
        <textarea
          v-model="form.text"
          class="input"
          rows="5"
          placeholder="Type something to speak…"
          @keydown.enter.exact.prevent="onPlay"
        />
        <p class="field-hint">
          {{ ttsTokenHint }}
          <template v-if="engine === 'openai'">
            &nbsp;·&nbsp;
            <span :class="{ error: openaiTextTooLong }">
              {{ openaiInputLength }} / {{ OPENAI_TTS_MAX_INPUT_CHARS }} characters
            </span>
          </template>
        </p>
      </div>

      <div class="actions">
        <button
          class="btn"
          type="button"
          :class="{ danger: speaking }"
          :disabled="(busy && !speaking) || openaiTextTooLong"
          @click="speaking ? onStop() : onPlay()"
        >{{ speaking ? 'Stop' : (busy && engine === 'openai' ? 'Synthesizing…' : 'Play') }}</button>
        <button class="btn ghost" type="button" :disabled="!hasSavableOutput || busy" @click="onSynthesizeWithSave">Save to file</button>
      </div>

      <p v-if="err" class="field-hint error">{{ err }}</p>
    </div>
  </section>

  <CollapsibleCard
    id="tts.voice"
    title="Voice and output"
    :desc="engine === 'openai' ? 'Model, voice, format and delivery for the OpenAI engine.' : 'Windows System.Speech voice and delivery.'"
  >
    <div class="field-grid">
      <div class="field" v-if="engine === 'local'">
        <label class="field-label">Voice</label>
        <div class="actions">
          <select v-model="form.voice" class="input">
            <option value="">(Default)</option>
            <option v-for="v in voices" :key="v" :value="v">{{ v }}</option>
          </select>
          <button class="btn ghost" type="button" :disabled="loadingVoices" @click="loadVoices">
            {{ loadingVoices ? 'Loading…' : 'Reload' }}
          </button>
        </div>
        <p class="field-hint">Installed Windows voices, read through PowerShell System.Speech.</p>
      </div>

      <div class="field" v-if="engine === 'openai'">
        <label class="field-label">Model</label>
        <input class="input" v-model="form.openaiModel" list="openai-models" placeholder="gpt-4o-mini-tts" />
        <datalist id="openai-models">
          <option v-for="m in openaiModelOptions" :key="m" :value="m" />
        </datalist>
        <p class="field-hint">Pick one or type your own. Not every model in the list speaks.</p>
      </div>

      <div class="field" v-if="engine === 'openai'">
        <label class="field-label">Voice</label>
        <input class="input" v-model="form.openaiVoice" list="openai-voices" placeholder="alloy" />
        <datalist id="openai-voices">
          <option v-for="v in openaiVoiceOptions" :key="v" :value="v" />
        </datalist>
        <p class="field-hint">Suggestions may be incomplete; a custom name is fine.</p>
      </div>

      <div class="field" v-if="engine === 'openai'">
        <label class="field-label">Tone</label>
        <input class="input" v-model="form.openaiInstructions" placeholder="e.g. Cheerful and positive" />
        <p class="field-hint">Optional hint influencing speaking style.</p>
      </div>

      <div class="field" v-if="engine === 'openai'">
        <label class="field-label">Format</label>
        <select v-model="(form.openaiFormat as any)" class="input">
          <option v-for="f in openaiFormatOptions" :key="f" :value="f">{{ f.toUpperCase() }}</option>
        </select>
        <p class="field-hint">OPUS is smaller and starts sooner; WAV is PCM16 for Windows playback.</p>
      </div>

      <div class="field">
        <label class="field-label">Rate <span class="range-value">{{ form.rate }}</span></label>
        <input class="range" type="range" min="-10" max="10" step="1" v-model.number="form.rate" />
      </div>

      <div class="field">
        <label class="field-label">Volume <span class="range-value">{{ form.volume }}</span></label>
        <input class="range" type="range" min="0" max="100" step="1" v-model.number="form.volume" />
      </div>
    </div>

    <label class="switch row" v-if="engine === 'openai'">
      <input type="checkbox" v-model="form.openaiStreaming" />
      <span class="switch-text">
        <span class="switch-label">Stream audio when supported <span class="badge warn">experimental</span></span>
        <span class="switch-hint">
          Plays through a local HTTP proxy as the audio arrives instead of waiting for the whole file.
          Falls back to synthesize-then-play automatically if streaming is unsupported or fails.
        </span>
      </span>
    </label>
  </CollapsibleCard>

  <section class="card" v-if="wavPath || (engine === 'openai' && form.openaiStreaming)">
    <div class="card-body">
      <div class="field" v-if="wavPath">
        <label class="field-label">Last output</label>
        <p class="field-hint path">{{ wavPath }}</p>
      </div>
      <audio ref="playerRef" :src="wavSrc || ''" controls preload="none" />
    </div>
  </section>
</template>

<style scoped>
audio {
  width: 100%;
  height: 36px;
  border-radius: var(--radius-sm);
}
/* A file path should stay copyable and wrap at any character rather than
   pushing the card wider than the page. */
.path {
  font-family: var(--font-mono);
  word-break: break-all;
  user-select: text;
}
</style>
