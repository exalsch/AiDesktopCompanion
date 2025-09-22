<script setup lang="ts">
import { computed, ref } from 'vue'
import ConversationView from '../ConversationView.vue'
import PromptComposer from '../PromptComposer.vue'

const props = defineProps<{
  messages: any[]
  hideToolCalls: boolean
  mcpServers: any[]
  ttsPlaying: boolean
  ttsPlayingId: string
  quickPrompts: Record<string, string>
  activeQuickPrompt: number | null
  systemPromptText: string
  composerText: string
}>()

const emit = defineEmits<{
  (e: 'list-tools', payload: any): void
  (e: 'toggle-tool', payload: any): void
  (e: 'toggle-quick-prompt', index: number): void
  (e: 'update:composerText', v: string): void
  (e: 'busy', v: boolean): void
}>()

const composerTextModel = computed({
  get: () => props.composerText,
  set: (v: string) => emit('update:composerText', v)
})

const innerComposerRef = ref<InstanceType<typeof PromptComposer> | null>(null)

// Pending image attachments to be sent with the next user prompt
const pendingImages = ref<Array<{ path: string; src: string }>>([])

function addImage(path: string, src: string) {
  if (!path || !src) return
  // Avoid duplicates by path
  if (pendingImages.value.some(i => i.path === path)) return
  pendingImages.value.push({ path, src })
}

function removeImage(idx: number) {
  if (idx >= 0 && idx < pendingImages.value.length) pendingImages.value.splice(idx, 1)
}

function clearImages() {
  pendingImages.value = []
}

defineExpose({
  focus() { try { (innerComposerRef.value as any)?.focus?.() } catch {} },
  // Allow external event handlers (e.g., image capture) to add images
  addImage,
  removeImage,
  clearImages,
})
</script>

<template>
  <div class="prompt-layout">
    <div class="main-content">
      <ConversationView
        :messages="messages"
        :hide-tool-details="hideToolCalls"
        :mcp-servers="mcpServers"
        :tts-playing-id="ttsPlayingId"
        :tts-playing="ttsPlaying"
        @list-tools="$emit('list-tools', $event)"
        @toggle-tool="$emit('toggle-tool', $event)"
      />
    </div>
    <div class="quick-prompt-bar">
      <div class="qp-left">
        <button
          v-for="i in 9"
          :key="i"
          :class="['qp-btn', { active: activeQuickPrompt === i }]"
          :disabled="!quickPrompts[String(i)]"
          :title="quickPrompts[String(i)] || 'Empty'"
          @click="$emit('toggle-quick-prompt', i)"
        >{{ i }}</button>
      </div>
      <div class="attachments" v-if="pendingImages.length">
        <div
          v-for="(img, idx) in pendingImages"
          :key="img.path"
          class="thumb"
        >
          <img :src="img.src" alt="attachment" />
          <button class="remove" title="Remove" @click="removeImage(idx)">×</button>
        </div>
      </div>
    </div>
    <PromptComposer
      ref="innerComposerRef"
      v-model="composerTextModel"
      :systemPromptText="systemPromptText"
      :pendingImages="pendingImages"
      @busy="$emit('busy', $event)"
      @clear-attachments="clearImages()"
    />
  </div>
</template>

<style scoped>
/* Prompt layout with scrolling conversation */
.prompt-layout { display: flex; flex-direction: column; gap: 5px; height: 95%; }
/* Restore expected padding/overflow for the conversation container */
.main-content { flex: 1; min-height: 180px; overflow: auto; padding: 12px 12px; }
/* Ensure the ConversationView inner list has a visible area even when empty */
:deep(.convo-wrap) { min-height: 160px; }

/* Quick Prompt buttons above composer */
.quick-prompt-bar { display: flex; align-items: center; justify-content: space-between; gap: 8px; flex-wrap: nowrap; padding: 0 12px; }
.qp-left { display: flex; gap: 3px; align-items: center; flex-wrap: wrap; }
/* Make numeric buttons compact and consistent */
.qp-btn { padding: 2px 6px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; font-size: 12px; width: 26px; text-align: center; }
.qp-btn:hover:not(:disabled) { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qp-btn.active { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qp-btn:disabled { opacity: 0.5; cursor: not-allowed; }

/* Attachment thumbnails (right aligned) */
.attachments { display: flex; gap: 6px; align-items: center; margin-left: auto; }
.thumb { position: relative; width: 36px; height: 36px; border: 1px solid var(--adc-border); border-radius: 6px; overflow: hidden; }
.thumb img { width: 100%; height: 100%; object-fit: cover; display: block; }
.thumb .remove { position: absolute; top: -2px; right: -2px; width: 16px; height: 16px; border-radius: 20%; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; line-height: 12px; font-size: 12px; padding: 0; display: flex; align-items: center; justify-content: center; }
.thumb .remove:hover { background: var(--adc-danger); color: #fff; border-color: var(--adc-danger); }
</style>
