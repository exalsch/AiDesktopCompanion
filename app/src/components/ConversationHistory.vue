<script setup lang="ts">
import { computed } from 'vue'
import convoState, { getConversationsSorted, setCurrentConversation } from '../state/conversation'

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
}
</script>

<template>
  <div class="history">
    <div class="row-title">History</div>
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
          <span class="time">{{ new Date(it.updatedAt).toLocaleTimeString() }}</span>
        </div>
        <div v-if="it.subtitle" class="subtitle">{{ it.subtitle }}</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.history { margin: 0 auto 10px auto; max-width: 920px; padding: 8px; border: 1px solid #3a3a44; border-radius: 10px; background: #1b1b22; }
.row-title { font-weight: 700; margin-bottom: 6px; color: #d8d8e2; }
.list { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 8px; }
.item { border: 1px solid #2e2e36; border-radius: 8px; padding: 8px; background: #1f1f26; cursor: default; }
.item.active { border-color: #2e5cff; box-shadow: 0 0 0 1px #2e5cff44 inset; }
.title-line { display: flex; align-items: center; gap: 8px; }
.title { font-weight: 600; color: #e6e6f0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.time { margin-left: auto; font-size: 11px; color: #9fa0aa; }
.subtitle { margin-top: 4px; font-size: 12px; color: #b7b8c6; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
