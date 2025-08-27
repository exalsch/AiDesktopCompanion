<script setup lang="ts">
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import conversation, { appendMessage } from '../state/conversation'

const props = defineProps<{ modelValue: string }>()
const emit = defineEmits<{ (e: 'update:modelValue', v: string): void }>()

const input = computed({
  get: () => props.modelValue,
  set: (v: string) => emit('update:modelValue', v)
})
const sending = ref(false)

function buildChatMessages(): Array<{ role: string; content: string }> {
  const msgs: Array<{ role: string; content: string }> = []
  // Optional system primer for clarity
  msgs.push({ role: 'system', content: 'You are a helpful assistant. Be concise and clear.' })

  for (const m of conversation.currentConversation.messages) {
    if (m.type === 'text') {
      msgs.push({ role: m.role, content: m.text || '' })
    } else if (m.type === 'image') {
      // ‼️ TODO: Vision support. For now, include a placeholder note so the model knows an image was referenced.
      const names = (m.images || []).map((i) => i.path.split(/[\/\\]/).pop()).join(', ')
      msgs.push({ role: m.role, content: names ? `[Image attached: ${names}]` : `[Image attached]` })
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
  }
}
</script>

<template>
  <div class="composer">
    <textarea
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
.composer { display: flex; flex-direction: column; gap: 8px; margin: 10px auto; max-width: 920px; }
.input {
  width: 100%;
  resize: vertical;
  background: #1f1f26;
  color: #fff;
  border: 1px solid #3a3a44;
  border-radius: 10px;
  padding: 10px;
}
.row { display: flex; align-items: center; gap: 10px; }
.hint { font-size: 12px; color: #9fa0aa; }
.send { margin-left: auto; padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.send[disabled] { opacity: 0.6; cursor: not-allowed; }
</style>
