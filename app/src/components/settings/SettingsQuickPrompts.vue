<script setup lang="ts">
import QuickPromptsEditor from '../QuickPromptsEditor.vue'

const props = defineProps<{
  settings: any
  notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void
}>()
</script>

<template>
  <div class="settings-section">
    <div class="settings-title">Quick Prompts</div>    
    <div class="settings-row col">
      <label class="label">Quick Prompts System Prompt (optional)</label>
      <textarea
        v-model="props.settings.quick_prompt_system_prompt"
        class="input"
        rows="5"
        placeholder="Overrides the global System Prompt when a Quick Prompt is used (Quick Actions and Quick Prompt bar)."
        autocomplete="off"
        spellcheck="false"
        style="width: 100%; max-width: 100%; box-sizing: border-box; flex: 0 0 auto; align-self: stretch;"
      />
      <div class="settings-hint">If left empty, the global System Prompt is used for Quick Prompts.</div>
    </div>
    <QuickPromptsEditor :notify="props.notify" />
    <div class="settings-hint">Note: Each quick template is appended to the effective System Prompt: the Quick Prompts System Prompt (if set), otherwise the Global System Prompt.</div>
  </div>
</template>
