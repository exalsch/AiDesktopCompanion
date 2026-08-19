<script setup lang="ts">
import { reactive, watch, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
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
  error: '' as string,
})

async function onRecordToggle() {
  try {
    if (!state.recording) {
      await startRecording('audio/webm;codecs=opus', String(settings.stt_input_device_id || ''))
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
    state.originalTranscript = String(result?.original_text || '').trim()
    state.transcript = String(result?.final_text || '').trim()
    state.postProcessApplied = result?.post_process_applied === true
    state.postProcessError = String(result?.post_process_error || '').trim()

    if (settings.stt_post_process_enabled && state.postProcessError) {
      props.notify?.(state.postProcessError, 'error', 4200)
    }
    if (!state.transcript) props.notify?.('No transcription returned', 'error')
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Transcription failed'
    state.error = msg
    props.notify?.(msg, 'error')
  } finally {
    state.busy = false
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
const showOriginalTranscript = computed(() => settings.stt_post_process_enabled && !!state.originalTranscript)
const postProcessStatusHint = computed(() => {
  if (!settings.stt_post_process_enabled || !state.transcript) return ''
  if (state.postProcessError) return `Post-processing error: ${state.postProcessError}`
  if (state.postProcessApplied) return 'Post-processing applied.'
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
      </div>
    </div>
  </section>

  <section class="card" v-if="state.transcript">
    <div class="card-head">
      <span class="card-heading">
        <span class="card-title">Transcript</span>
        <span class="card-desc">{{ sttTokenHint }}</span>
      </span>
      <span class="actions">
        <button class="btn ghost sm" type="button" @click="onCopy">Copy</button>
        <button class="btn sm" type="button" @click="onUseAsPrompt">Use as prompt</button>
      </span>
    </div>
    <div class="card-body">
      <textarea class="input" :value="state.transcript" rows="6" readonly />

      <p
        v-if="postProcessStatusHint"
        class="field-hint"
        :class="{ error: !!state.postProcessError }"
      >{{ postProcessStatusHint }}</p>

      <!-- Only worth showing once post-processing has actually rewritten
           something, as a before/after for the cleanup. -->
      <div v-if="showOriginalTranscript" class="field">
        <label class="field-label">Before post-processing</label>
        <textarea class="input" :value="state.originalTranscript" rows="4" readonly />
      </div>
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
