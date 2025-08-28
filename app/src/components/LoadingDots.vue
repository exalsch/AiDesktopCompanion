<script setup lang="ts">
const props = defineProps<{ text?: string }>()
</script>

<template>
  <div class="loading">
    <span v-if="props.text" class="label">{{ props.text }}</span>
    <span class="dots" aria-hidden="true">
      <i></i><i></i><i></i>
    </span>
  </div>
</template>

<style scoped>
.loading { display: inline-flex; align-items: center; gap: 8px; }
.label { font-size: 12px; color: #9fa0aa; }
.dots { display: inline-flex; gap: 4px; }
.dots i { width: 6px; height: 6px; border-radius: 50%; background: #9fa0aa; opacity: 0.4; animation: pulse 1.2s infinite ease-in-out; }
.dots i:nth-child(2) { animation-delay: 0.2s; }
.dots i:nth-child(3) { animation-delay: 0.4s; }
@keyframes pulse { 0%, 80%, 100% { transform: scale(0.6); opacity: 0.4; } 40% { transform: scale(1); opacity: 1; } }
</style>
