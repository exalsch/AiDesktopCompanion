<script setup lang="ts">
import { ref, watch, nextTick, onMounted, reactive, computed } from 'vue'
import MessageItem from './MessageItem.vue'
import ImageViewer from './ImageViewer.vue'
import type { Message } from '../state/conversation'
import { newConversation } from '../state/conversation'

const props = defineProps<{
  messages: Message[]
  hideToolDetails?: boolean
  mcpServers?: any[]
  ttsPlayingId?: string
  ttsPlaying?: boolean
}>()

const emit = defineEmits<{
  (e: 'list-tools', serverId: string): void
  (e: 'toggle-tool', payload: { serverId: string; tool: string; enabled: boolean }): void
}>()

const listRef = ref<HTMLElement | null>(null)

// Image viewer state
const viewer = reactive({
  open: false,
  images: [] as { src: string; path?: string }[],
  index: 0,
})

function onImageClick(payload: { images: { path: string; src: string }[]; index: number }) {
  try {
    viewer.images = (payload.images || []).map(i => ({ src: i.src, path: i.path }))
    viewer.index = Math.max(0, Math.min(payload.index || 0, viewer.images.length - 1))
    viewer.open = viewer.images.length > 0
  } catch {
    viewer.open = false
  }
}

// MCP tool selection modal state
const toolUi = reactive({ open: false, filter: '' })
const servers = computed(() => Array.isArray(props.mcpServers) ? props.mcpServers : [])

function openToolSelector() {
  toolUi.open = true
}
function closeToolSelector() {
  toolUi.open = false
}

function ensureToolsLoaded(serverId: string) {
  const s = servers.value.find((x: any) => x.id === serverId)
  if (!s) return
  const has = Array.isArray(s.tools) && s.tools.length > 0
  if (!has && s.status === 'connected') emit('list-tools', serverId)
}

function isToolEnabled(s: any, name: string) {
  const disabled = Array.isArray(s?.disabled_tools) ? s.disabled_tools : []
  return !disabled.includes(name)
}

function onToggle(serverId: string, tool: string, enabled: boolean) {
  emit('toggle-tool', { serverId, tool, enabled })
}

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
      <button
        class="btn secondary"
        :title="servers.length === 0 ? 'Add an MCP server in Settings to enable this' : 'Select which MCP tools are allowed to be used'"
        :disabled="servers.length === 0"
        @click="openToolSelector()"
      >MCP Tools</button>
      <button class="btn" @click="newConversation()">New conversation</button>
    </div>


    <div ref="listRef" class="list convo-wrap">
      <MessageItem v-for="m in messages" :key="m.id" :message="m" :hide-tool-details="props.hideToolDetails" :is-playing="!!props.ttsPlaying && props.ttsPlayingId === m.id" @image-click="onImageClick" />
    </div>
  </div>
  <!-- MCP Tool Selection Modal -->
  <div v-if="toolUi.open" class="modal-backdrop" @click.self="closeToolSelector()">
    <div class="modal">
      <div class="modal-header">
        <div class="modal-title">Select MCP Tools</div>
        <button class="icon-btn" title="Close" @click="closeToolSelector()">✕</button>
      </div>
      <div class="modal-body">
        <div class="row">
          <input class="input" v-model="toolUi.filter" placeholder="Filter tools by name" />
        </div>
        <div class="servers">
          <div v-for="s in servers" :key="s.id" class="server">
            <div class="server-header">
              <div class="server-id">{{ s.id }}</div>
              <div class="server-status" :class="s.status">{{ s.status }}</div>
              <button class="btn tiny" :disabled="s.status !== 'connected'" @click="ensureToolsLoaded(s.id)">Show tools</button>
            </div>
            <div v-if="Array.isArray(s.tools) && s.tools.length" class="tools">
              <label
                v-for="t in s.tools.filter((t:any) => !toolUi.filter || String(t.name||'').toLowerCase().includes(toolUi.filter.toLowerCase()))"
                :key="t.name || t.id"
                class="tool-row"
                :title="t.description || ''"
              >
                <input
                  type="checkbox"
                  :checked="isToolEnabled(s, t.name || t.id)"
                  @change="onToggle(s.id, (t.name||t.id), ($event.target as HTMLInputElement).checked)"
                />
                <span class="tool-name">{{ t.name || t.id }}</span>
                <span class="tool-desc" v-if="t.description">— {{ t.description }}</span>
              </label>
            </div>
            <div v-else class="tools empty">No tools loaded.</div>
          </div>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn" @click="closeToolSelector()">Done</button>
      </div>
    </div>
  </div>
  <ImageViewer :open="viewer.open" :images="viewer.images" :index="viewer.index" @close="viewer.open = false" />
</template>

<style scoped>
.conversation-root { display: flex; flex-direction: column; gap: 10px; height: 100%; max-width: var(--content-max); margin: 0 auto; }
.header { display: flex; align-items: center; gap: 10px; }
.title { font-weight: 700; font-size: 20px; }
.spacer { flex: 1; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-accent); background: var(--adc-accent); color: #fff; cursor: pointer; }
.btn:hover { filter: brightness(1.05); }
.btn.secondary { background: transparent; color: var(--adc-accent); }
.btn:disabled { opacity: 0.6; cursor: not-allowed; }
.list { flex: 1; min-height: 0; overflow: auto; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-surface); }

/* Modal */
.modal-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.4); display: flex; align-items: center; justify-content: center; z-index: 50; }
.modal { width: min(800px, 90vw); max-height: 80vh; display: flex; flex-direction: column; background: var(--adc-surface); border: 1px solid var(--adc-border); border-radius: 12px; box-shadow: 0 10px 30px rgba(0,0,0,0.3); }
.modal-header { display: flex; align-items: center; padding: 12px; border-bottom: 1px solid var(--adc-border); }
.modal-title { font-weight: 700; font-size: 16px; }
.icon-btn { margin-left: auto; border: 1px solid var(--adc-border); background: transparent; border-radius: 6px; padding: 6px 8px; cursor: pointer; }
.modal-body { padding: 12px; overflow: auto; display: flex; flex-direction: column; gap: 10px; }
.modal-footer { display: flex; justify-content: flex-end; padding: 10px 12px; border-top: 1px solid var(--adc-border); }
.row { display: flex; gap: 10px; }
.input { flex: 1; padding: 8px 10px; border: 1px solid var(--adc-border); border-radius: 8px; background: var(--adc-bg); color: var(--adc-fg); }
.servers { display: flex; flex-direction: column; gap: 12px; }
.server { border: 1px solid var(--adc-border); border-radius: 10px; }
.server-header { display: flex; align-items: center; gap: 8px; padding: 10px; border-bottom: 1px solid var(--adc-border); background: var(--adc-surface-2, transparent); }
.server-id { font-weight: 600; }
.server-status { font-size: 12px; color: var(--adc-fg-muted); }
.server-status.connected { color: #0a0; }
.server-status.disconnected { color: #a00; }
.tools { padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; }
.tools.empty { font-size: 12px; color: var(--adc-fg-muted); }
.tool-row { display: flex; align-items: center; gap: 8px; cursor: pointer; }
.tool-name { font-weight: 500; }
.tool-desc { font-size: 12px; color: var(--adc-fg-muted); }
</style>
