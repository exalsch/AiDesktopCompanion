<script setup lang="ts">
import { ref, defineProps } from 'vue'

const props = defineProps<{
  settings: any
  models: { list: string[]; loading: boolean; error: string | null }
  onSave: () => void
  onRefreshModels: () => void
  onClearConversations: () => void
}>()

const showApiKey = ref(false)
</script>

<template>
  <div class="settings-section">
    <div class="settings-title">General Settings</div>  
    <button class="btn" @click="props.onSave">Save</button>        
    <div class="settings-row col">
      <label class="label">OpenAI API Key</label>
      <div class="row-inline">
        <input
          :type="showApiKey ? 'text' : 'password'"
          v-model="props.settings.openai_api_key"
          class="input"
          placeholder="sk-..."
          autocomplete="off"
          spellcheck="false"
        />
        <button class="btn ghost" @click="showApiKey = !showApiKey">{{ showApiKey ? 'Hide' : 'Show' }}</button>
      </div>
    </div>

    <div class="settings-row col">
      <label class="label">Model</label>
      <div class="row-inline">
        <select v-model="props.settings.openai_chat_model" class="input">
          <option v-if="!props.models.list.includes(props.settings.openai_chat_model)" :value="props.settings.openai_chat_model">{{ props.settings.openai_chat_model }} (current)</option>
          <option v-for="m in props.models.list" :key="m" :value="m">{{ m }}</option>
        </select>
        <button class="btn" :disabled="props.models.loading" @click="props.onRefreshModels">{{ props.models.loading ? 'Fetching…' : 'Fetch Models' }}</button>
      </div>
      <div v-if="props.models.error" class="settings-hint error">{{ props.models.error }}</div>
    </div>

    <div class="settings-row col">
      <label class="label">Temperature: {{ props.settings.temperature.toFixed(2) }}</label>
      <input type="range" min="0" max="2" step="0.05" v-model.number="props.settings.temperature" />
      <div class="settings-hint">Lower = deterministic, Higher = creative. Default 1.0</div>
    </div>

    <div class="settings-row col">
      <label class="label">UI Style</label>
      <select v-model="props.settings.ui_style" class="input">
        <option value="sidebar-dark">Sidebar Dark (default)</option>
        <option value="sidebar-light">Sidebar Light</option>
      </select>
      <div class="settings-hint">Switch between Sidebar Dark or Sidebar Light.</div>
    </div>

    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.persist_conversations"/> Persist conversations</label>
      <button class="btn danger" @click="props.onClearConversations">Clear All Conversations</button>      
    </div>
    <div class="settings-hint">When enabled, conversation history is saved locally only.</div>
    <div class="settings-row">
      <label class="checkbox"><input type="checkbox" v-model="props.settings.hide_tool_calls_in_chat"/> Hide tool call details in chat</label>
    </div>
  </div>
</template>
