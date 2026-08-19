<script setup lang="ts">
import QuickPromptsEditor from '../QuickPromptsEditor.vue'
import CollapsibleCard from '../ui/CollapsibleCard.vue'

const props = defineProps<{
  settings: any
  models?: { list: string[]; loading: boolean; error: string | null }
  onRefreshModels?: () => any
  notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void
}>()
</script>

<template>
  <!-- The nine prompts are what people come here to edit, so they lead and are
       not foldable. The knobs that govern how they run sit behind them. -->
  <section class="card">
    <div class="card-head">
      <span class="card-heading">
        <span class="card-title">The nine prompts</span>
        <span class="card-desc">Reachable with 1-9 in the Quick Actions popup, and from the Select-All hotkey.</span>
      </span>
    </div>
    <div class="card-body">
      <QuickPromptsEditor :notify="props.notify" />
      <p class="field-hint">
        Each template is appended to the effective system prompt - the one below if set, otherwise the global one
        from Settings → General.
      </p>
    </div>
  </section>

  <CollapsibleCard
    id="settings.quickprompts.behaviour"
    title="How they run"
    desc="Model, system prompt and what happens to the result."
  >
    <div class="field">
      <label class="field-label">Model</label>
      <div class="actions">
        <select v-model="props.settings.quick_prompt_model" class="input">
          <option :value="''">Use global ({{ props.settings.openai_chat_model }})</option>
          <option v-for="m in (props.models?.list || [])" :key="m" :value="m">{{ m }}</option>
          <option
            v-if="props.settings.quick_prompt_model && !(props.models?.list || []).includes(props.settings.quick_prompt_model)"
            :value="props.settings.quick_prompt_model"
          >{{ props.settings.quick_prompt_model }} (current)</option>
        </select>
        <button
          v-if="props.onRefreshModels"
          class="btn ghost"
          type="button"
          :disabled="props.models?.loading"
          @click="props.onRefreshModels"
        >{{ props.models?.loading ? 'Fetching…' : 'Fetch models' }}</button>
      </div>
      <p v-if="props.models?.error" class="field-hint error">{{ props.models?.error }}</p>
      <p class="field-hint">Quick prompts are usually short and mechanical, so a smaller model than the chat one often does.</p>
    </div>

    <div class="field">
      <label class="field-label">System prompt</label>
      <textarea
        v-model="props.settings.quick_prompt_system_prompt"
        class="input"
        rows="5"
        placeholder="Overrides the global system prompt whenever a Quick Prompt runs."
        autocomplete="off"
        spellcheck="false"
      />
      <p class="field-hint">Leave empty to use the global system prompt from Settings → General.</p>
    </div>

    <label class="switch row">
      <input type="checkbox" v-model="props.settings.show_quick_prompt_result_in_popup" />
      <span class="switch-text">
        <span class="switch-label">Show the result in the popup instead of inserting it</span>
        <span class="switch-hint">
          Pressing 1-9 shows the answer in place with Copy (c) and Insert (v). Inserting returns focus to the previous
          app, pastes, and closes the popup.
        </span>
      </span>
    </label>
  </CollapsibleCard>
</template>
