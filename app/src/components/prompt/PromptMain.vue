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

defineExpose({
  focus() { try { (innerComposerRef.value as any)?.focus?.() } catch {} }
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
      <button
        v-for="i in 9"
        :key="i"
        :class="['qp-btn', { active: activeQuickPrompt === i }]"
        :disabled="!quickPrompts[String(i)]"
        :title="quickPrompts[String(i)] || 'Empty'"
        @click="$emit('toggle-quick-prompt', i)"
      >{{ i }}</button>
    </div>
    <PromptComposer
      ref="innerComposerRef"
      v-model="composerTextModel"
      :systemPromptText="systemPromptText"
      @busy="$emit('busy', $event)"
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
.quick-prompt-bar { display: flex; gap: 3px; align-items: center; flex-wrap: wrap; padding: 0 12px; }
/* Make numeric buttons compact and consistent */
.qp-btn { padding: 2px 6px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; font-size: 12px; width: 26px; text-align: center; }
.qp-btn:hover:not(:disabled) { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qp-btn.active { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qp-btn:disabled { opacity: 0.5; cursor: not-allowed; }
</style>
