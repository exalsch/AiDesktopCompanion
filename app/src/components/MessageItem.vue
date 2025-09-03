<script setup lang="ts">
import type { Message } from '../state/conversation'

const props = defineProps<{ message: Message; hideToolDetails?: boolean }>()
const emit = defineEmits<{
  (e: 'image-click', payload: { images: { path: string; src: string }[]; index: number }): void
}>()
</script>

<template>
  <div class="row" :class="props.message.role">
    <div class="bubble" :data-type="props.message.type">
      <div v-if="props.message.type === 'text'" class="text">{{ props.message.text }}</div>
      <div v-else-if="props.message.type === 'image'" class="images">
        <img v-for="(img, i) in props.message.images || []"
             :key="img.path"
             :src="img.src"
             alt="Captured image"
             class="thumb"
             @click="emit('image-click', { images: (props.message.images || []), index: i })"
        />
      </div>
      <div v-else-if="props.message.type === 'tool'" class="tool">
        <div class="tool-header">
          <span class="tool-name">{{ props.message.tool?.serverId || 'mcp' }} › {{ props.message.tool?.tool || props.message.tool?.function }}</span>
          <span class="status" :data-ok="props.message.tool?.ok === true" :data-finished="props.message.tool?.status === 'finished'">
            {{ props.message.tool?.status === 'finished' ? (props.message.tool?.ok ? 'ok' : 'error') : 'running' }}
          </span>
        </div>
        <template v-if="!props.hideToolDetails">
          <div v-if="props.message.tool?.args" class="section">
            <div class="label">args</div>
            <pre class="code">{{ JSON.stringify(props.message.tool?.args, null, 2) }}</pre>
          </div>
          <div v-if="props.message.tool?.ok && props.message.tool?.result !== undefined" class="section">
            <div class="label">result</div>
            <pre class="code">{{ JSON.stringify(props.message.tool?.result, null, 2) }}</pre>
          </div>
          <div v-else-if="props.message.tool?.status === 'finished' && props.message.tool?.error" class="section">
            <div class="label">error</div>
            <pre class="code error">{{ props.message.tool?.error }}</pre>
          </div>
        </template>
      </div>
      <div class="meta-line">
        <span class="time">{{ new Date(props.message.createdAt).toLocaleTimeString() }}</span>
        <span v-if="props.message.type === 'image'" class="badge">Image</span>
        <span v-else-if="props.message.type === 'tool'" class="badge">Tool</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
/* Row aligns bubble left/right */
.row { display: flex; padding: 6px 10px; }
.row.assistant { justify-content: flex-start; }
.row.user { justify-content: flex-end; }

/* Bubble styles */
.bubble {
  max-width: 70%;
  background: var(--adc-surface);
  border: 1px solid var(--adc-border);
  color: var(--adc-fg);
  border-radius: 16px;
  padding: 8px 10px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.assistant .bubble { border-top-left-radius: 6px; }
.user .bubble {
  background: var(--adc-accent);
  border-color: var(--adc-accent);
  color: #ffffff;
  border-top-right-radius: 6px;
}

/* Text inside bubbles is always left-aligned */
.text { white-space: pre-wrap; text-align: left; }

/* Images inside a bubble */
.images { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 8px; }
.thumb { width: 100%; height: auto; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-bg); object-fit: contain; cursor: zoom-in; }
.thumb:hover { filter: brightness(1.05); }

/* Meta line (time, badges) styled subtly */
.meta-line { display: flex; align-items: center; gap: 8px; font-size: 11px; opacity: 0.9; }
.assistant .meta-line { justify-content: flex-start; color: var(--adc-fg-muted); }
.user .meta-line { justify-content: flex-end; color: #e9ebff; }
.badge { background: var(--adc-accent); color: #fff; border-radius: 6px; padding: 1px 6px; font-size: 10px; }

/* Tool call rendering */
.tool { display: flex; flex-direction: column; gap: 8px; }
.tool-header { display: flex; align-items: center; gap: 8px; }
.tool-name { font-weight: 600; }
.status { margin-left: auto; font-size: 11px; padding: 2px 6px; border-radius: 6px; background: var(--adc-border); color: var(--adc-fg-muted); }
.status[data-finished="true"][data-ok="true"] { background: #0f9d58; color: #fff; }
.status[data-finished="true"][data-ok="false"] { background: #d93025; color: #fff; }
.section { display: flex; flex-direction: column; gap: 4px; }
.label { font-size: 11px; color: var(--adc-fg-muted); }
.code { padding: 8px; border-radius: 8px; background: #0b0b0b; color: #e8e8e8; border: 1px solid var(--adc-border); white-space: pre-wrap; overflow: auto; max-height: 260px; }
.code.error { background: #290f12; color: #ffd8d8; border-color: #bf3b42; }
</style>
