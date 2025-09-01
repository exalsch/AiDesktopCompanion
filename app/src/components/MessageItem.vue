<script setup lang="ts">
import type { Message } from '../state/conversation'

const props = defineProps<{ message: Message }>()
</script>

<template>
  <div class="row" :class="props.message.role">
    <div class="bubble" :data-type="props.message.type">
      <div v-if="props.message.type === 'text'" class="text">{{ props.message.text }}</div>
      <div v-else class="images">
        <img v-for="img in props.message.images || []"
             :key="img.path"
             :src="img.src"
             alt="Captured image"
             class="thumb"
        />
      </div>
      <div class="meta-line">
        <span class="time">{{ new Date(props.message.createdAt).toLocaleTimeString() }}</span>
        <span v-if="props.message.type === 'image'" class="badge">Image</span>
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
.thumb { width: 100%; height: auto; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-bg); object-fit: contain; }

/* Meta line (time, badges) styled subtly */
.meta-line { display: flex; align-items: center; gap: 8px; font-size: 11px; opacity: 0.9; }
.assistant .meta-line { justify-content: flex-start; color: var(--adc-fg-muted); }
.user .meta-line { justify-content: flex-end; color: #e9ebff; }
.badge { background: var(--adc-accent); color: #fff; border-radius: 6px; padding: 1px 6px; font-size: 10px; }
</style>
