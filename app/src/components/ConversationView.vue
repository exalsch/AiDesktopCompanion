<script setup lang="ts">
import { ref, watch, nextTick, onMounted } from 'vue'
import MessageItem from './MessageItem.vue'
import type { Message } from '../state/conversation'
import { newConversation } from '../state/conversation'

const props = defineProps<{
  messages: Message[]
}>()

const listRef = ref<HTMLElement | null>(null)

watch(
  () => props.messages.length,
  async () => {
    await nextTick()
    const el = listRef.value
    if (el) el.scrollTop = el.scrollHeight
  }
)

onMounted(async () => {
  await nextTick()
  const el = listRef.value
  if (el) el.scrollTop = el.scrollHeight
})
</script>

<template>
  <div class="conversation-root">
    <div class="header">
      <div class="title">Conversation</div>
      <div class="spacer" />
      <button class="btn" @click="newConversation()">New conversation</button>
    </div>

    <div class="mcp-hint">‼️ Select tools (MCP) to be used — placeholder UI</div>

    <div ref="listRef" class="list">
      <MessageItem v-for="m in messages" :key="m.id" :message="m" />
    </div>
  </div>
</template>

<style scoped>
.conversation-root { display: flex; flex-direction: column; gap: 10px; height: calc(30vh); max-width: 920px; margin: 0 auto; }
.header { display: flex; align-items: center; gap: 10px; }
.title { font-weight: 700; font-size: 20px; }
.spacer { flex: 1; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.btn:hover { filter: brightness(1.05); }
.mcp-hint { font-size: 12px; color: #9fa0aa; }
.list { overflow: auto; border: 1px solid #3a3a44; border-radius: 10px; background: #1f1f26; }
</style>
