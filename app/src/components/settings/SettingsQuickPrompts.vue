<script setup lang="ts">
import QuickPromptsEditor from '../QuickPromptsEditor.vue'

const props = defineProps<{
  settings: any
  models?: { list: string[]; loading: boolean; error: string | null }
  onRefreshModels?: () => any
  notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void
}>()
</script>

<template>
  <div class="settings-section">
    <div class="settings-title">Quick Prompts</div>    
    <div class="settings-row col">
      <label class="label">Model for Quick Actions Quick Prompts</label>
      <div class="row-inline">
        <select v-model="props.settings.quick_prompt_model" class="input">
          <option :value="''">Use Global ({{ props.settings.openai_chat_model }})</option>
          <option v-for="m in (props.models?.list || [])" :key="m" :value="m">{{ m }}</option>
          <option v-if="props.settings.quick_prompt_model && !(props.models?.list || []).includes(props.settings.quick_prompt_model)" :value="props.settings.quick_prompt_model">{{ props.settings.quick_prompt_model }} (current)</option>
        </select>
        <button v-if="props.onRefreshModels" class="btn" :disabled="props.models?.loading" @click="props.onRefreshModels">{{ props.models?.loading ? 'Fetching…' : 'Fetch Models' }}</button>
      </div>
      <div v-if="props.models?.error" class="settings-hint error">{{ props.models?.error }}</div>
      <div class="settings-hint">Leave empty to use the global chat model.</div>
    </div>
    <div class="settings-row">
      <label class="checkbox">
        <input type="checkbox" v-model="props.settings.show_quick_prompt_result_in_popup" />
        <span>Show result in Quick Actions popup instead of inserting</span>
      </label>
    </div>
    <div class="settings-hint">When enabled, pressing 1–9 in the Quick Actions popup will show the AI result in-place with Copy (c) and Insert (v) controls. Inserting will briefly return focus to the previous app, paste the text, and close the popup.</div>
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
