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


    <div ref="listRef" class="list convo-wrap">
      <MessageItem v-for="m in messages" :key="m.id" :message="m" />
    </div>
  </div>
  <div class="mcp-hint">‼️ Select tools (MCP) to be used — placeholder UI</div>
</template>

<style scoped>
.conversation-root { display: flex; flex-direction: column; gap: 10px; height: 100%; max-width: var(--content-max); margin: 0 auto; }
.header { display: flex; align-items: center; gap: 10px; }
.title { font-weight: 700; font-size: 20px; }
.spacer { flex: 1; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-accent); background: var(--adc-accent); color: #fff; cursor: pointer; }
.btn:hover { filter: brightness(1.05); }
.mcp-hint { font-size: 12px; color: var(--adc-fg-muted); }
.list { flex: 1; min-height: 0; overflow: auto; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-surface); }
</style>
