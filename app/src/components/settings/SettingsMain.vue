<script setup lang="ts">
import SettingsGeneral from './SettingsGeneral.vue'
import SettingsMcpServers from './SettingsMcpServers.vue'
import SettingsQuickPrompts from './SettingsQuickPrompts.vue'
import SettingsSpeechToText from './SettingsSpeechToText.vue'
import { onMounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'

const props = defineProps<{
  settings: any
  models: any
  settingsSubview: 'General' | 'Speech To Text' | 'Quick Prompts' | 'MCP Servers'
  onSave: (map?: any) => any
  onRefreshModels: () => any
  onClearConversations: () => any
  onAdd: () => any
  onRemove: (idx: number) => any
  onConnect: (server: any) => any
  onDisconnect: (server: any) => any
  onPing: (server: any) => any
  onListTools: (server: any) => any
  onFillArgsTemplate: (server: any) => any
  onValidateEnvJsonInput: (server: any) => any
  onCallTool: (payload: any) => any
  selectedToolObj: (server: any) => any
}>()

async function refreshMcpStatuses() {
  try {
    const arr = Array.isArray(props.settings?.mcp_servers) ? props.settings.mcp_servers : []
    for (const s of arr) {
      try {
        if (!s || !s.id) continue
        const ok = await invoke<boolean>('mcp_is_connected', { serverId: s.id })
        if (ok) { s.status = 'connected'; s.connecting = false; s.error = null }
        else { s.status = 'disconnected'; s.connecting = false }
      } catch {}
    }
  } catch {}
}

onMounted(() => { if (props.settingsSubview === 'MCP Servers') { refreshMcpStatuses() } })
watch(() => props.settingsSubview, (sub) => { if (sub === 'MCP Servers') { refreshMcpStatuses() } })
</script>

<template>
  <div class="settings">
    <div class="section-title">Settings</div>
    <SettingsGeneral
      v-if="props.settingsSubview === 'General'"
      :settings="props.settings"
      :models="props.models"
      :onSave="props.onSave"
      :onRefreshModels="props.onRefreshModels"
      :onClearConversations="props.onClearConversations"
    />

    <SettingsSpeechToText
      v-else-if="props.settingsSubview === 'Speech To Text'"
      :settings="props.settings"
    />

    <SettingsQuickPrompts
      v-else-if="props.settingsSubview === 'Quick Prompts'"
      :settings="props.settings"
      :models="props.models"
      :onRefreshModels="props.onRefreshModels"
    />

    <SettingsMcpServers
      v-else
      :settings="props.settings"
      :onAdd="props.onAdd"
      :onRemove="props.onRemove"
      :onSave="props.onSave"
      :onConnect="props.onConnect"
      :onDisconnect="props.onDisconnect"
      :onPing="props.onPing"
      :onListTools="props.onListTools"
      :onFillArgsTemplate="props.onFillArgsTemplate"
      :onValidateEnvJsonInput="props.onValidateEnvJsonInput"
      :onCallTool="props.onCallTool"
      :selectedToolObj="props.selectedToolObj"
    />
  </div>
</template>

<style scoped>
.settings { margin: 0px auto; max-width: none; color: var(--adc-fg); }
.section-title { font-weight: 700; margin-bottom: 8px; font-size: 18px; }
</style>
