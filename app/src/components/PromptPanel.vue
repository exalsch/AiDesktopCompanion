<script setup lang="ts">
import { computed } from 'vue'

interface Props {
  selection: string
  preview: string
  length: number
}

const props = defineProps<Props>()

const isTruncated = computed(() => props.selection.length > props.preview.length)
</script>

<template>
  <div class="prompt-panel" role="dialog" aria-label="Prompt">
    <div class="header">
      <div class="title">Prompt</div>
      <div class="spacer" />
      <div class="badge" :title="`Characters: ${selection.length}`">{{ selection.length }}</div>
      <button class="close" @click="$emit('close')" aria-label="Close">✕</button>
    </div>

    <div class="content">
      <div class="label">Selection preview</div>
      <div class="preview" :title="selection">
        <span>{{ preview }}</span>
        <span v-if="isTruncated" class="ellipsis">…</span>
      </div>
    </div>

    <div class="footer">
      <button class="primary" @click="$emit('close')">OK</button>
    </div>
  </div>
</template>

<style scoped>
.prompt-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px;
  margin: 12px auto;
  max-width: 720px;
  border: 1px solid #3a3a44;
  border-radius: 10px;
  background: #1f1f26;
  color: #fff;
}
.header {
  display: flex;
  align-items: center;
  gap: 8px;
}
.title { font-weight: 700; }
.spacer { flex: 1; }
.badge {
  min-width: 28px;
  text-align: center;
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 999px;
  background: #2a2a31;
  border: 1px solid #3a3a44;
  color: #e0e0ea;
}
.close {
  background: transparent;
  border: none;
  color: #c0c0cb;
  font-size: 16px;
  cursor: pointer;
}
.content { display: flex; flex-direction: column; gap: 6px; }
.label { font-size: 12px; color: #c8c8d0; }
.preview {
  white-space: pre-wrap;
  background: #23232b;
  border: 1px solid #3a3a44;
  border-radius: 8px;
  padding: 10px;
  min-height: 60px;
}
.ellipsis { color: #9fa0aa; }
.footer { display: flex; justify-content: flex-end; }
.primary {
  padding: 8px 12px;
  border-radius: 8px;
  border: 1px solid #3a3a44;
  background: #2e5cff;
  color: white;
  cursor: pointer;
}
.primary:hover { filter: brightness(1.05); }
</style>
