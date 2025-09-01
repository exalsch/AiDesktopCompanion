<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref, watch } from 'vue'

const props = defineProps<{
  open: boolean
  images: { src: string; path?: string }[]
  index: number
}>()
const emit = defineEmits<{ (e: 'close'): void }>()

const idx = ref(props.index || 0)

watch(
  () => props.index,
  (v) => { idx.value = v || 0 }
)

function close() { emit('close') }
function prev() { if (!props.images.length) return; idx.value = (idx.value - 1 + props.images.length) % props.images.length }
function next() { if (!props.images.length) return; idx.value = (idx.value + 1) % props.images.length }

function onKey(e: KeyboardEvent) {
  if (!props.open) return
  if (e.key === 'Escape') { e.preventDefault(); close() }
  else if (e.key === 'ArrowLeft') { e.preventDefault(); prev() }
  else if (e.key === 'ArrowRight') { e.preventDefault(); next() }
}

onMounted(() => { window.addEventListener('keydown', onKey) })
onBeforeUnmount(() => { window.removeEventListener('keydown', onKey) })
</script>

<template>
  <div v-if="open" class="overlay" @click.self="close">
    <div class="chrome">
      <button class="icon-btn" title="Close (Esc)" @click="close">✕</button>
      <div class="spacer" />
      <div class="counter">{{ (idx + 1) }} / {{ images.length }}</div>
    </div>
    <button v-if="images.length > 1" class="nav prev" title="Previous (←)" @click.stop="prev">‹</button>
    <div class="image-wrap">
      <img v-if="images[idx]" class="image" :src="images[idx].src" :alt="images[idx].path || 'Image'" />
    </div>
    <button v-if="images.length > 1" class="nav next" title="Next (→)" @click.stop="next">›</button>
  </div>
</template>

<style scoped>
.overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.75);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.image-wrap {
  max-width: 92vw;
  max-height: 88vh;
  display: flex;
  align-items: center;
  justify-content: center;
}
.image {
  max-width: 100%;
  max-height: 100%;
  border-radius: 10px;
  border: 1px solid var(--adc-border);
  background: var(--adc-bg);
}
.nav {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  width: 42px;
  height: 42px;
  border-radius: 50%;
  border: 1px solid var(--adc-border);
  background: var(--adc-surface);
  color: var(--adc-fg);
  cursor: pointer;
  display: grid;
  place-items: center;
  font-size: 24px;
}
.nav.prev { left: 20px; }
.nav.next { right: 20px; }

.chrome {
  position: absolute;
  top: 14px;
  left: 14px;
  right: 14px;
  display: flex;
  align-items: center;
  gap: 10px;
}
.icon-btn {
  padding: 6px 10px;
  border-radius: 8px;
  border: 1px solid var(--adc-border);
  background: var(--adc-surface);
  color: var(--adc-fg);
  cursor: pointer;
}
.spacer { flex: 1; }
.counter { font-size: 12px; color: var(--adc-fg-muted); }
</style>
