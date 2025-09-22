<script setup lang="ts">
import { computed, reactive } from 'vue'
import convoState, { getConversationsSorted, setCurrentConversation, deleteConversation } from '../state/conversation'
const emit = defineEmits<{ (e: 'open', id: string): void }>()

interface ItemVM {
  id: string
  title: string
  subtitle: string
  tooltip: string
  updatedAt: number
}

const items = computed<ItemVM[]>(() => {
  const list = getConversationsSorted()
  return list.map((c) => {
    const messages = c.messages || []
    const count = messages.length
    const last = messages[messages.length - 1]
    const first = messages[0]
    const updated = c.updatedAt ?? (last?.createdAt ?? c.createdAt ?? Date.now())
    const title = count === 0
      ? 'New conversation'
      : (first?.text?.slice(0, 40) || (first?.type === 'image' ? '[Image]' : 'Conversation'))
    const lastUser = [...messages].reverse().find(m => m.role === 'user' && m.text)
    const lastAssistant = [...messages].reverse().find(m => m.role === 'assistant' && m.text)
    const subtitleParts: string[] = []
    if (lastUser?.text) subtitleParts.push(`You: ${lastUser.text.slice(0, 60)}`)
    if (lastAssistant?.text) subtitleParts.push(`AI: ${lastAssistant.text.slice(0, 60)}`)
    const subtitle = subtitleParts.join('  ·  ')
    const tooltip = [
      `Messages: ${count}`,
      `Created: ${new Date(c.createdAt ?? updated).toLocaleString()}`,
      `Updated: ${new Date(updated).toLocaleString()}`,
      lastUser?.text ? `Last You: ${lastUser.text}` : undefined,
      lastAssistant?.text ? `Last AI: ${lastAssistant.text}` : undefined,
    ].filter(Boolean).join('\n')
    return { id: c.id, title, subtitle, tooltip, updatedAt: updated }
  })
})

function isActive(id: string) {
  return convoState.currentConversation.id === id
}

function openConversation(id: string) {
  setCurrentConversation(id)
  emit('open', id)
}

// In-app confirmation UI state
const confirmUi = reactive<{ open: boolean; id: string }>({ open: false, id: '' })

function askDelete(id: string) {
  confirmUi.id = id
  confirmUi.open = true
}

function cancelDelete() {
  confirmUi.open = false
  confirmUi.id = ''
}

function confirmDelete() {
  if (!confirmUi.id) return cancelDelete()
  deleteConversation(confirmUi.id)
  cancelDelete()
}

// Format timestamp for history: show time if today, else date + short time
function formatHistoryTimestamp(ts: number): string {
  try {
    const d = new Date(ts)
    if (Number.isNaN(d.getTime())) return ''
    const now = new Date()
    const sameDay =
      d.getFullYear() === now.getFullYear() &&
      d.getMonth() === now.getMonth() &&
      d.getDate() === now.getDate()
    if (sameDay) return d.toLocaleTimeString()
    const datePart = d.toLocaleDateString()
    const timePart = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
    return `${datePart} ${timePart}`
  } catch {
    return ''
  }
}
</script>

<template>
  <div class="history">    
    <div class="list" role="list">
      <div
        v-for="it in items"
        :key="it.id"
        role="listitem"
        class="item"
        :class="{ active: isActive(it.id) }"
        :title="it.tooltip"
        @dblclick="openConversation(it.id)"
      >
        <div class="title-line">
          <span class="title">{{ it.title }}</span>
          <span class="time">{{ formatHistoryTimestamp(it.updatedAt) }}</span>
          <button
            class="icon-btn delete-btn"
            title="Delete conversation"
            aria-label="Delete conversation"
            @click.stop="askDelete(it.id)"
          >
            <svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true">
              <path fill="currentColor" d="M9 3h6a1 1 0 0 1 1 1v2h4v2h-1v11a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8H3V6h4V4a1 1 0 0 1 1-1zm6 4V5h-6v2h6zM6 8v11h12V8H6zm3 2h2v7H9v-7zm4 0h2v7h-2v-7z"/>
            </svg>
          </button>
        </div>
        <div v-if="it.subtitle" class="subtitle">{{ it.subtitle }}</div>
      </div>
      <!-- Confirm Delete Modal -->
      <div v-if="confirmUi.open" class="modal-backdrop" @click.self="cancelDelete()">
        <div class="modal">
          <div class="modal-header">
            <div class="modal-title">Delete conversation?</div>
            <button class="icon-btn" title="Close" @click="cancelDelete()">✕</button>
          </div>
          <div class="modal-body">
            <div class="confirm-text">This action cannot be undone.</div>
          </div>
          <div class="modal-footer">
            <button class="btn secondary" @click="cancelDelete()">Cancel</button>
            <button class="btn danger" @click="confirmDelete()">Delete</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history { margin: 0 auto 10px auto; padding: 8px; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-surface); }
.row-title { font-weight: 700; margin-bottom: 6px; color: var(--adc-fg); }
.list { display: flow-root; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 8px; }
.item { border: 1px solid var(--adc-border); border-radius: 8px; padding: 8px; background: var(--adc-surface); cursor: default; }
.item.active { border-color: var(--adc-accent); box-shadow: 0 0 0 3px var(--adc-focus-ring); }
.title-line { display: flex; align-items: center; gap: 8px; }
.title { font-weight: 600; color: var(--adc-fg); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.time { margin-left: auto; font-size: 11px; color: var(--adc-fg-muted); }
.subtitle { margin-top: 4px; font-size: 12px; color: var(--adc-fg-muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
/* Small icon button to the far right */
.icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 22px; height: 22px; padding: 0; border: none; border-radius: 6px; background: transparent; color: var(--adc-fg-muted); cursor: pointer; }
.icon-btn:hover { background: var(--adc-hover); color: var(--adc-fg); }
.icon-btn:focus { outline: 2px solid var(--adc-focus-ring); outline-offset: 2px; }
.delete-btn { margin-left: 4px; }

/* Modal */
.modal-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.4); display: flex; align-items: center; justify-content: center; z-index: 60; }
.modal { width: min(420px, 90vw); max-height: 80vh; display: flex; flex-direction: column; background: var(--adc-surface); border: 1px solid var(--adc-border); border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,0.3); }
.modal-header { display: flex; align-items: center; padding: 12px; border-bottom: 1px solid var(--adc-border); }
.modal-title { font-weight: 700; font-size: 16px; }
.modal-body { padding: 12px; }
.modal-footer { display: flex; justify-content: flex-end; gap: 8px; padding: 10px 12px; border-top: 1px solid var(--adc-border); }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-accent); background: var(--adc-accent); color: #fff; cursor: pointer; }
.btn.secondary { background: transparent; color: var(--adc-accent); }
.btn.danger { border-color: #a00; background: #a00; }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.confirm-text { color: var(--adc-fg); }
</style>
