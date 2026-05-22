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
  <div class="stt">
    <div class="row">
      <label class="checkbox" style="margin: 0;">
        <input type="checkbox" v-model="settings.stt_post_process_enabled" />
        Improve transcribed text with AI
      </label>
      <div v-if="settings.stt_post_process_enabled" class="row" style="margin-top: 2px;">
        <label class="label">STT Post-Processing Prompt</label>
        <textarea
          v-model="settings.stt_post_process_prompt"
          rows="3"
          placeholder="You are an STT post-processor..."
          style="min-height: 88px;"
        />
        <div class="hint">Model selection for post-processing stays in Settings → Speech To Text.</div>
      </div>
    </div>

    <div class="row inline">
      <button class="btn" :disabled="state.busy" :class="{ danger: state.recording }" @click="onRecordToggle">
        {{ state.recording ? 'Stop & Transcribe' : 'Record' }}
      </button>
      <div class="hint">Recording format uses MediaRecorder (WEBM/Opus). Requires mic permission.</div>
    </div>

    <!-- Busy indicator is now shown globally in App.vue -->
    <div v-if="state.error" class="hint error">{{ state.error }}</div>

    <div class="row" v-if="state.transcript">
      <template v-if="showOriginalTranscript">
        <label class="label">Original Transcript</label>
        <textarea :value="state.originalTranscript" rows="5" readonly />
      </template>

      <label class="label">Transcript</label>
      <textarea :value="state.transcript" rows="6" readonly />
      <div v-if="postProcessStatusHint" class="hint" :class="{ error: !!state.postProcessError }">{{ postProcessStatusHint }}</div>
      <div class="hint">{{ sttTokenHint }}</div>
      <div class="row inline">
        <button class="btn" @click="onCopy">Copy</button>
        <button class="btn" @click="onUseAsPrompt">Use as Prompt</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stt { display: flex; flex-direction: column; gap: 10px; }
.row { display: flex; flex-direction: column; gap: 6px; }
.row.inline { flex-direction: row; align-items: center; gap: 10px; flex-wrap: wrap; }
.label { font-size: 12px; color: #c8c9d3; }
textarea { width: 100%; resize: vertical; min-height: 140px; padding: 8px; border-radius: 8px; border: 1px solid #3a3a44; background: #14141a; color: #e0e0ea; box-sizing: border-box; }
.checkbox { display: inline-flex; align-items: center; gap: 8px; font-size: 13px; color: #c8c9d3; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.btn.danger { background: #a42828; border-color: #7c1f1f; }
.hint { font-size: 12px; color: #9fa0aa; white-space: pre-line; }
.hint.error { color: #f2b8b8; }
</style>
