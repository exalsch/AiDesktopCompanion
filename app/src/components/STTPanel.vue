<script setup lang="ts">
import { reactive, watch, computed, onMounted, onBeforeUnmount } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'
import { startRecording, stopRecording, transcodeToWav16kMono } from '../stt'
import { useSettings } from '../composables/useSettings'
import { estimateTextTokens, formatTokenInfo } from '../composables/useTokenEstimate'
import { tokenizerReady } from '../composables/useTokenizer'

type SttTranscriptionResult = {
  original_text: string
  final_text: string
  post_process_applied?: boolean
  post_process_error?: string | null
}

/** Backend record of the most recent transcription, whatever started it. */
type LastTranscript = SttTranscriptionResult & { at_ms: number }

/** Must match `stt_session::CANCELLED_MESSAGE` on the Rust side. */
const CANCELLED_MESSAGE = 'Transcription cancelled'

const emit = defineEmits<{
  (e: 'use-as-prompt', text: string): void
  (e: 'busy', v: boolean): void
}>()

const props = defineProps<{ notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void }>()

const state = reactive({
  recording: false,
  mime: '' as string,
  originalTranscript: '' as string,
  transcript: '' as string,
  postProcessApplied: false,
  postProcessError: '' as string,
  busy: false,
  stopping: false,
  error: '' as string,
  at: 0,
})

function applyTranscript(r: SttTranscriptionResult, atMs?: number) {
  state.originalTranscript = String(r?.original_text || '').trim()
  state.transcript = String(r?.final_text || '').trim()
  state.postProcessApplied = r?.post_process_applied === true
  state.postProcessError = String(r?.post_process_error || '').trim()
  state.at = atMs || Date.now()
}

// Most transcriptions start from a hotkey with this window closed. The backend
// keeps the last one and announces each new one, so this panel always shows
// what was said last and what the AI pass made of it, not just the recordings
// made from here.
let unlistenLast: UnlistenFn | null = null
onMounted(async () => {
  try {
    unlistenLast = await listen<LastTranscript>('stt:last-transcript', (e) => {
      if (e?.payload && !state.recording) applyTranscript(e.payload, e.payload.at_ms)
    })
  } catch (err) {
    console.warn('[stt] listen for last transcript failed', err)
  }
  try {
    const last = await invoke<LastTranscript | null>('stt_get_last_transcript')
    if (last && !state.transcript && !state.recording) applyTranscript(last, last.at_ms)
  } catch (err) {
    console.warn('[stt] load last transcript failed', err)
  }
})
onBeforeUnmount(() => {
  if (unlistenLast) { try { unlistenLast() } catch {} }
})

async function onStopTranscription() {
  if (!state.busy || state.stopping) return
  state.stopping = true
  try {
    await invoke('stt_cancel')
  } catch (e: any) {
    props.notify?.(e?.message || String(e) || 'Could not stop transcription', 'error')
  }
}

async function onRecordToggle() {
  try {
    if (!state.recording) {
      await startRecording('audio/webm;codecs=opus', String(settings.stt_input_device_id || ''), settings)
      state.recording = true
      state.error = ''
      state.originalTranscript = ''
      state.transcript = ''
      state.postProcessApplied = false
      state.postProcessError = ''
      props.notify?.('Recording… click Stop to transcribe.', 'success', 1500)
    } else {
      const res = await stopRecording()
      state.recording = false
      if (!res) { props.notify?.('No audio captured', 'error'); return }
      state.mime = res.mime
      await transcribeBlob(res.blob, res.mime)
    }
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Recording failed'
    state.error = msg
    props.notify?.(msg, 'error')
    state.recording = false
  }
}

async function transcribeBlob(blob: Blob, mime: string) {
  state.busy = true
  state.error = ''
  try {
    // For local STT, transcode to WAV 16kHz mono on the frontend to ensure broad compatibility.
    let payloadBytes: Uint8Array
    let payloadMime: string = mime
    const engine = String((settings as any).stt_engine || 'openai')
    const baseUrl = String((settings as any).stt_cloud_base_url || 'https://api.openai.com').trim()
    const isOpenAi = baseUrl.startsWith('https://api.openai.com')
    const shouldTranscode = engine === 'local' || (engine !== 'local' && !isOpenAi)
    if (shouldTranscode) {
      try {
        payloadBytes = await transcodeToWav16kMono(blob)
        payloadMime = 'audio/wav'
      } catch {
        const arrayBuffer = await blob.arrayBuffer()
        payloadBytes = new Uint8Array(arrayBuffer)
        payloadMime = mime
      }
    } else {
      const arrayBuffer = await blob.arrayBuffer()
      payloadBytes = new Uint8Array(arrayBuffer)
    }
    const bytes = Array.from(payloadBytes)
    const result: SttTranscriptionResult = await invoke('stt_transcribe', { audio: bytes, mime: payloadMime })
    applyTranscript(result)

    if (settings.stt_post_process_enabled && state.postProcessError) {
      props.notify?.(state.postProcessError, 'error', 4200)
    }
    if (!state.transcript) props.notify?.('No transcription returned', 'error')
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Transcription failed'
    if (msg === CANCELLED_MESSAGE) {
      props.notify?.('Transcription stopped', 'success', 1500)
    } else {
      state.error = msg
      props.notify?.(msg, 'error')
    }
  } finally {
    state.busy = false
    state.stopping = false
  }
}

async function onCopy() {
  try {
    await navigator.clipboard.writeText(state.transcript)
    props.notify?.('Copied to clipboard', 'success', 1200)
  } catch {
    props.notify?.('Copy failed', 'error')
  }
}

function onUseAsPrompt() {
  const t = state.transcript.trim()
  if (!t) { props.notify?.('Nothing to use', 'error'); return }
  emit('use-as-prompt', t)
}

watch(() => state.busy, (v) => emit('busy', !!v))

// Token hint for transcript text (approximate)
const { settings } = useSettings()
const sttModelName = computed(() => settings.openai_chat_model)
const tokenizerMode = computed(() => settings.tokenizer_mode)
const sttTextTokens = computed(() => {
  const _ready = tokenizerReady.value
  return estimateTextTokens(state.transcript || '', sttModelName.value, tokenizerMode.value).tokens
})
const sttTokenHint = computed(() => formatTokenInfo([{ label: 'text', tokens: sttTextTokens.value }]))
// Side by side whenever the AI pass actually produced the text, so a rewrite
// that went wrong (answering the dictation instead of cleaning it up) is plain
// to see next to what was really said.
const showComparison = computed(() => state.postProcessApplied && !!state.originalTranscript)
const transcriptTime = computed(() => {
  if (!state.at) return ''
  try {
    return new Date(state.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  } catch {
    return ''
  }
})
/**
 * Whether the vocabulary can reach the engine at all.
 *
 * Cloud and Whisper both take a vocabulary hint at decode time, so the list
 * works there whatever else is set. Parakeet takes none, which leaves the
 * post-processing pass as its only route - and that route is off by default.
 * Saying so beats letting the setting look broken.
 */
const vocabularyInert = computed(() =>
  !settings.stt_post_process_enabled
  && settings.stt_engine === 'local'
  && String(settings.stt_local_model || '').includes('parakeet')
)
const postProcessStatusHint = computed(() => {
  if (!state.transcript) return ''
  if (state.postProcessError) return `Post-processing error: ${state.postProcessError}`
  if (state.postProcessApplied) return ''
  if (!settings.stt_post_process_enabled) return ''
  return 'Post-processing enabled, but no changes were applied.'
})
</script>

<template>
  <!-- Two cards: what to record, and what came back. The result card only
       exists once there is a transcript, so the page is not padded out with an
       empty box before the first recording. -->
  <section class="card">
    <div class="card-body">
      <div class="actions">
        <button
          class="btn"
          type="button"
          :class="{ danger: state.recording }"
          :disabled="state.busy"
          @click="onRecordToggle"
        >
          <span class="rec-dot" :class="{ live: state.recording }" aria-hidden="true"></span>
          {{ state.recording ? 'Stop & transcribe' : 'Record' }}
        </button>
        <button
          v-if="state.busy"
          class="btn ghost"
          type="button"
          :disabled="state.stopping"
          @click="onStopTranscription"
        >{{ state.stopping ? 'Stopping…' : 'Stop transcribing' }}</button>
        <span class="field-hint">
          {{ state.busy ? 'Transcribing…' : 'Captured with MediaRecorder (WEBM/Opus). Needs microphone permission.' }}
        </span>
      </div>

      <p v-if="state.error" class="field-hint error">{{ state.error }}</p>

      <div class="divider"></div>

      <label class="switch row">
        <input type="checkbox" v-model="settings.stt_post_process_enabled" />
        <span class="switch-text">
          <span class="switch-label">Improve transcribed text with AI</span>
          <span class="switch-hint">Cleans up punctuation, casing and recognition artefacts after transcription.</span>
        </span>
      </label>

      <div v-if="settings.stt_post_process_enabled" class="field">
        <label class="field-label">Post-processing prompt</label>
        <textarea
          v-model="settings.stt_post_process_prompt"
          class="input mono"
          rows="4"
          placeholder="You are an STT post-processor..."
        />
        <p class="field-hint">Which model does this is set under Settings → Speech To Text.</p>
        <p class="field-hint">You can use <code v-pre>{{active_app}}</code> and <code v-pre>{{window_title}}</code> in the prompt - they're replaced with the process name and title of the window the text is about to be inserted into.</p>
      </div>

      <div class="field">
        <label class="field-label">Vocabulary</label>
        <textarea
          v-model="settings.stt_vocabulary"
          class="input mono"
          rows="4"
          placeholder="One name or term per line, spelled the way you want it&#10;Alex Schick&#10;Infor M3"
        />
        <p class="field-hint">
          Names and terms speech recognition keeps getting wrong. Cloud and Whisper take these
          as a decoding hint; Parakeet has no such hook, so there they are corrected afterwards.
        </p>
        <p v-if="vocabularyInert" class="field-hint error">
          Parakeet cannot use this list on its own. Turn on “Improve transcribed text with AI”
          above, or it has no effect.
        </p>
      </div>
    </div>
  </section>

  <section class="card" v-if="state.transcript">
    <div class="card-head">
      <span class="card-heading">
        <span class="card-title">Last transcript</span>
        <span class="card-desc">{{ transcriptTime ? `${transcriptTime} · ` : '' }}{{ sttTokenHint }}</span>
      </span>
      <span class="actions">
        <button class="btn ghost sm" type="button" @click="onCopy">Copy</button>
        <button class="btn sm" type="button" @click="onUseAsPrompt">Use as prompt</button>
      </span>
    </div>
    <div class="card-body">
      <!-- With the AI pass in play, show both: what the engine heard and what
           was inserted. Otherwise the one text is all there is. -->
      <template v-if="showComparison">
        <div class="field">
          <label class="field-label">Transcribed</label>
          <textarea class="input" :value="state.originalTranscript" rows="4" readonly />
        </div>
        <div class="field">
          <label class="field-label">AI corrected</label>
          <textarea class="input" :value="state.transcript" rows="4" readonly />
        </div>
      </template>
      <textarea v-else class="input" :value="state.transcript" rows="6" readonly />

      <p
        v-if="postProcessStatusHint"
        class="field-hint"
        :class="{ error: !!state.postProcessError }"
      >{{ postProcessStatusHint }}</p>
    </div>
  </section>
</template>

<style scoped>
/* The panel's own styles used hardcoded hex colours, which meant its
   textareas stayed dark under the light theme. Everything visual now comes
   from the shared layer; only the record dot is local. */
.rec-dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: currentColor;
  opacity: 0.75;
}
.rec-dot.live {
  opacity: 1;
  animation: rec-pulse 1.2s ease-in-out infinite;
}
@keyframes rec-pulse {
  0%, 100% { transform: scale(1); opacity: 1; }
  50% { transform: scale(0.72); opacity: 0.5; }
}
@media (prefers-reduced-motion: reduce) {
  .rec-dot.live { animation: none; }
}
</style>
