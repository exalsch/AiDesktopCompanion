<script setup lang="ts">
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import conversation, { appendMessage } from '../state/conversation'

const props = defineProps<{ modelValue: string; systemPromptText?: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: string): void; (e: 'busy', v: boolean): void }>()

const input = computed({
  get: () => props.modelValue,
  set: (v: string) => emit('update:modelValue', v)
})
const sending = ref(false)
const textareaRef = ref<HTMLTextAreaElement | null>(null)

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

async function onSend() {
  const text = input.value.trim()
  if (!text || sending.value) return

  // append user message
  appendMessage({ role: 'user', type: 'text', text })
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
    <div class="row">
      <div class="hint">Press Enter to send</div>
      <button class="send" :disabled="sending || !input.trim()" @click="onSend">
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
</style>
