<script setup lang="ts">
import type { Message } from '../state/conversation'

const props = defineProps<{ message: Message }>()
</script>

<template>
  <div class="message-item" :data-role="props.message.role" :data-type="props.message.type">
    <div class="meta">
      <span class="role">{{ props.message.role }}</span>
      <span class="time">{{ new Date(props.message.createdAt).toLocaleTimeString() }}</span>
      <span v-if="props.message.type === 'image'" class="badge">Image</span>
    </div>

    <div v-if="props.message.type === 'text'" class="text">{{ props.message.text }}</div>

    <div v-else class="images">
      <img v-for="img in props.message.images || []"
           :key="img.path"
           :src="img.src"
           alt="Captured image"
           class="thumb"
      />
    </div>
  </div>
</template>

<style scoped>
.message-item { padding: 10px 12px; border-bottom: 1px solid #2c2c36; }
.meta { display: flex; align-items: center; gap: 8px; font-size: 12px; color: #9fa0aa; margin-bottom: 6px; }
.meta .role { text-transform: capitalize; font-weight: 600; color: #cfd0db; }
.meta .badge { background: #2e5cff; color: #fff; border-radius: 6px; padding: 2px 6px; font-size: 11px; }
.text { white-space: pre-wrap; color: #e6e6f0; }
.images { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 10px; }
.thumb { width: 100%; height: auto; border: 1px solid #3a3a44; border-radius: 8px; background: #15151a; object-fit: contain; }
</style>
