<script setup lang="ts">
import QuickPromptsEditor from '../QuickPromptsEditor.vue'

const props = defineProps<{
  notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void
}>()
</script>

<template>
  <div class="section-title">Settings</div>
  <div class="settings-section">
    <div class="settings-title">Quick Prompts</div>
    <div class="settings-hint">Writes defaults to %APPDATA%/AiDesktopCompanion/quick_prompts.json</div>
    <QuickPromptsEditor :notify="props.notify" />
  </div>
</template>
