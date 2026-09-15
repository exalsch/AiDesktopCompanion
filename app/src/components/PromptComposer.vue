<script setup lang="ts">
import { ref, reactive, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import conversation, { appendMessage } from '../state/conversation'
import { useSettings } from '../composables/useSettings'
import { estimateTextTokens, estimateImageTokensFromMeta, formatTokenInfo } from '../composables/useTokenEstimate'
import { useImageMeta } from '../composables/useImageMeta'
import { tokenizerReady } from '../composables/useTokenizer'
import { startRecording, stopRecording, transcodeToWav16kMono } from '../stt'

const props = defineProps<{
  modelValue: string
  systemPromptText?: string
  pendingImages?: Array<{ path: string; src: string }>
  notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void
}>()
const emit = defineEmits<{ (e: 'update:modelValue', v: string): void; (e: 'busy', v: boolean): void; (e: 'clear-attachments'): void }>()

const input = computed({
  get: () => props.modelValue,
  set: (v: string) => emit('update:modelValue', v)
})
const sending = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)

// Token estimate model source
const { settings } = useSettings()
const modelName = computed(() => settings.openai_chat_model)
const tokenizerMode = computed(() => settings.tokenizer_mode)
const { getMany } = useImageMeta()

type ContentPart =
  | { type: 'input_text'; text: string }
  | { type: 'input_image'; path: string; mime?: string }

function guessMimeFromPath(path: string): string | undefined {
  const p = path.toLowerCase()
  if (p.endsWith('.png')) return 'image/png'
  if (p.endsWith('.jpg') || p.endsWith('.jpeg')) return 'image/jpeg'
  if (p.endsWith('.webp')) return 'image/webp'
  if (p.endsWith('.gif')) return 'image/gif'
  if (p.endsWith('.bmp')) return 'image/bmp'
  if (p.endsWith('.tif') || p.endsWith('.tiff')) return 'image/tiff'
  return undefined
}

function buildChatMessages(): Array<{ role: string; content: string | ContentPart[] }> {
  const msgs: Array<{ role: string; content: string | ContentPart[] }> = []
  // Optional system primer for clarity
  const systemText = (props.systemPromptText && props.systemPromptText.trim()) ? props.systemPromptText.trim() : ''
  if (systemText) {
    msgs.push({ role: 'system', content: [{ type: 'input_text', text: systemText }] })
  }

  for (const m of conversation.currentConversation.messages) {
    if (m.type === 'text') {
      msgs.push({ role: m.role, content: [{ type: 'input_text', text: m.text || '' }] })
    } else if (m.type === 'image') {
      const parts: ContentPart[] = []
      for (const img of (m.images || [])) {
        const mime = guessMimeFromPath(img.path)
        parts.push({ type: 'input_image', path: img.path, mime })
      }
      if (parts.length) msgs.push({ role: m.role, content: parts })
    }
  }
  return msgs
}

// Live token estimate for unsent input and pending images
const pendingImageCount = computed(() => Array.isArray(props.pendingImages) ? props.pendingImages.length : 0)
const inputTextTokens = computed(() => {
  // depend on readiness so we recompute when tokenizer finishes loading
  const _ready = tokenizerReady.value
  return estimateTextTokens(input.value || '', modelName.value, tokenizerMode.value).tokens
})
const pendingImageMetas = computed(() => {
  const imgs = Array.isArray(props.pendingImages) ? props.pendingImages : []
  return getMany(imgs.map(i => i.src))
})
const imageTokens = computed(() => estimateImageTokensFromMeta(pendingImageMetas.value))
const tokenHint = computed(() => {
  return formatTokenInfo([
    { label: 'text', tokens: inputTextTokens.value },
    { label: pendingImageCount.value ? `images×${pendingImageCount.value}` : 'images', tokens: imageTokens.value },
  ])
})

// (Removed user-facing tokenization mode badge)

async function onSend() {
  const text = input.value.trim()
  const imgs = Array.isArray(props.pendingImages) ? props.pendingImages : []
  if ((text.length === 0 && imgs.length === 0) || sending.value) return

  // If there are pending image attachments, append them first as a separate user image message
  try {
    if (imgs.length) {
      appendMessage({ role: 'user', type: 'image', images: imgs.map(i => ({ path: i.path, src: i.src })) })
      emit('clear-attachments')
    }
  } catch {}

  // append user text message
  if (text.length > 0) {
    appendMessage({ role: 'user', type: 'text', text })
  }
  input.value = ''

  // call backend
  sending.value = true
  emit('busy', true)
  try {
    const msgs = buildChatMessages()
    const resp: string = await invoke('chat_complete', { messages: msgs })
    const clean = (resp || '').trim()
    appendMessage({ role: 'assistant', type: 'text', text: clean || 'No response received.' })
  } catch (e: any) {
    const msg = typeof e === 'string' ? e : e?.message || 'Unknown error'
    appendMessage({ role: 'assistant', type: 'text', text: `Error: ${msg}` })
  } finally {
    sending.value = false
    emit('busy', false)
  }
}

// --- In-composer dictation ---
// Lets a user dictate straight into the prompt textarea instead of switching
// to the separate STT section and using "Use as prompt".
const stt = reactive({ recording: false, busy: false })
const micDisabled = computed(() => sending.value || stt.busy)

async function onMicToggle() {
  try {
    if (!stt.recording) {
      await startRecording('audio/webm;codecs=opus', String((settings as any).stt_input_device_id || ''), settings as any)
      stt.recording = true
      props.notify?.('Recording… click the mic again to transcribe.', 'success', 1500)
    } else {
      stt.recording = false
      const res = await stopRecording()
      if (!res) { props.notify?.('No audio captured', 'error'); return }
      await transcribeAndInsert(res.blob, res.mime)
    }
  } catch (e: any) {
    stt.recording = false
    const msg = e?.message || String(e) || 'Recording failed'
    props.notify?.(msg, 'error')
  }
}

async function transcribeAndInsert(blob: Blob, mime: string) {
  stt.busy = true
  emit('busy', true)
  try {
    // Mirrors STTPanel: local engine (and any non-OpenAI cloud endpoint) gets
    // transcoded to WAV 16kHz mono on the frontend for broad compatibility.
    let payloadBytes: Uint8Array
    let payloadMime = mime
    const engine = String((settings as any).stt_engine || 'openai')
    const baseUrl = String((settings as any).stt_cloud_base_url || 'https://api.openai.com').trim()
    const isOpenAi = baseUrl.startsWith('https://api.openai.com')
    const shouldTranscode = engine === 'local' || (engine !== 'local' && !isOpenAi)
    if (shouldTranscode) {
      try {
        payloadBytes = await transcodeToWav16kMono(blob)
        payloadMime = 'audio/wav'
      } catch {
        payloadBytes = new Uint8Array(await blob.arrayBuffer())
        payloadMime = mime
      }
    } else {
      payloadBytes = new Uint8Array(await blob.arrayBuffer())
    }
    const bytes = Array.from(payloadBytes)
    const result: any = await invoke('stt_transcribe', { audio: bytes, mime: payloadMime })
    const text = String(result?.final_text || '').trim()
    if (!text) { props.notify?.('No transcription returned', 'error'); return }
    insertAtCursor(text)
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Transcription failed'
    props.notify?.(msg, 'error')
  } finally {
    stt.busy = false
    emit('busy', false)
  }
}

// Inserts dictated text at the caret (or replaces the current selection),
// rather than always appending, so dictation composes naturally with typing.
function insertAtCursor(text: string) {
  const el = textareaRef.value
  const current = input.value || ''
  if (!el) {
    input.value = current ? `${current} ${text}` : text
    return
  }
  const start = el.selectionStart ?? current.length
  const end = el.selectionEnd ?? current.length
  const before = current.slice(0, start)
  const after = current.slice(end)
  const needsSpaceBefore = before.length > 0 && !/\s$/.test(before)
  const insertion = `${needsSpaceBefore ? ' ' : ''}${text}`
  input.value = `${before}${insertion}${after}`
  const caret = before.length + insertion.length
  requestAnimationFrame(() => {
    try {
      el.focus()
      el.selectionStart = el.selectionEnd = caret
    } catch {}
  })
}

// Expose a method so parent components can trigger send programmatically
defineExpose({
  send: onSend,
  focus: () => {
    const el = textareaRef.value
    if (el) {
      el.focus()
      try { el.selectionStart = el.selectionEnd = el.value.length } catch {}
    }
  },
})
</script>

<template>
  <div class="composer">
    <textarea
      ref="textareaRef"
      v-model="input"
      class="input"
      placeholder="Type your prompt…"
      rows="3"
      @keydown.enter.exact.prevent="onSend"
    />
    <div class="hint" :title="tokenHint">{{ tokenHint }}</div>
    <div class="row">
      <button
        type="button"
        class="mic"
        :class="{ recording: stt.recording }"
        :disabled="micDisabled"
        :title="stt.recording ? 'Stop and transcribe' : 'Dictate into the prompt'"
        @click="onMicToggle"
      >
        <span class="rec-dot" :class="{ live: stt.recording }" aria-hidden="true"></span>
        {{ stt.busy ? 'Transcribing…' : (stt.recording ? 'Stop' : 'Dictate') }}
      </button>
      <div class="hint">Press Enter to send</div>
      <button class="send" :disabled="sending || (!input.trim() && pendingImageCount === 0)" @click="onSend">
        {{ sending ? 'Sending…' : 'Send' }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.composer { display: flex; flex-direction: column; gap: 8px; margin-left: 10px; margin-right: 10px; margin-bottom: 10px; max-width: var(--content-max); }
.input {
  width: 100%;
  resize: vertical;
  background: var(--adc-surface);
  color: var(--adc-fg);
  border: 1px solid var(--adc-border);
  border-radius: 10px;
  padding: 10px;
  box-sizing: border-box;
}
.row { display: flex; align-items: center; gap: 10px; }
.hint { font-size: 12px; color: var(--adc-fg-muted); }
.send { margin-left: auto; padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-accent); color: #fff; cursor: pointer; }
.send[disabled] { opacity: 0.6; cursor: not-allowed; }
.mic { display: flex; align-items: center; gap: 6px; padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; }
.mic.recording { border-color: var(--adc-danger); color: var(--adc-danger); }
.mic[disabled] { opacity: 0.6; cursor: not-allowed; }
.mic .rec-dot { width: 8px; height: 8px; border-radius: 50%; background: currentColor; opacity: 0.75; }
.mic .rec-dot.live { opacity: 1; animation: mic-rec-pulse 1.2s ease-in-out infinite; }
@keyframes mic-rec-pulse {
  0%, 100% { transform: scale(1); opacity: 1; }
  50% { transform: scale(0.72); opacity: 0.5; }
}
@media (prefers-reduced-motion: reduce) {
  .mic .rec-dot.live { animation: none; }
}
</style>
