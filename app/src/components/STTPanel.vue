<script setup lang="ts">
import { reactive, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { startRecording, stopRecording, isRecording } from '../stt'

const emit = defineEmits<{
  (e: 'use-as-prompt', text: string): void
}>()

const props = defineProps<{ notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void }>()

const state = reactive({
  recording: false,
  mime: '' as string,
  transcript: '' as string,
  busy: false,
  error: '' as string,
})

async function onRecordToggle() {
  try {
    if (!state.recording) {
      await startRecording('audio/webm;codecs=opus')
      state.recording = true
      state.error = ''
      state.transcript = ''
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
    const arrayBuffer = await blob.arrayBuffer()
    const bytes = Array.from(new Uint8Array(arrayBuffer))
    const text: string = await invoke('stt_transcribe', { audio: bytes, mime })
    state.transcript = (text || '').trim()
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
</script>

<template>
  <div class="stt">
    <div class="row inline">
      <button class="btn" :class="{ danger: state.recording }" @click="onRecordToggle">
        {{ state.recording ? 'Stop & Transcribe' : 'Record' }}
      </button>
      <div class="hint">Recording format uses MediaRecorder (WEBM/Opus). Requires mic permission.</div>
    </div>

    <div v-if="state.busy" class="hint">Transcribing…</div>
    <div v-if="state.error" class="hint error">{{ state.error }}</div>

    <div class="row" v-if="state.transcript">
      <label class="label">Transcript</label>
      <textarea :value="state.transcript" rows="6" readonly />
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
textarea { width: 100%; resize: vertical; min-height: 140px; padding: 8px; border-radius: 8px; border: 1px solid #3a3a44; background: #14141a; color: #e0e0ea; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.btn.danger { background: #a42828; border-color: #7c1f1f; }
.hint { font-size: 12px; color: #9fa0aa; white-space: pre-line; }
.hint.error { color: #f2b8b8; }
</style>
