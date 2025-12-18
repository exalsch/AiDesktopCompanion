<script setup lang="ts">
import LoadingDots from '../LoadingDots.vue'

const props = defineProps<{
  sections: ReadonlyArray<'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings'>
  activeSection: 'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings'
  promptSubview: 'Chat' | 'History'
  settingsSubview: 'General' | 'Speech To Text' | 'Quick Prompts' | 'MCP Servers'
  sidebarOpen: boolean
  busy: boolean
}>()

const emit = defineEmits<{
  (e: 'toggle-sidebar'): void
  (e: 'set-section', section: 'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings'): void
  (e: 'open-history'): void
  (e: 'set-settings-subview', sub: 'General' | 'Speech To Text' | 'Quick Prompts' | 'MCP Servers'): void
}>()
</script>

<template>
  <aside class="sidebar" :class="{ collapsed: !props.sidebarOpen }">
    <button class="burger" title="Toggle menu" @click="$emit('toggle-sidebar')">☰</button>
    <template v-for="s in props.sections" :key="s">
      <button
        class="side-tab"
        :class="{ active: props.activeSection === s }"
        @click="$emit('set-section', s)"
        :title="s"
      >
        <!-- Icons -->
        <template v-if="s === 'Prompt'">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="m12 19 7-7 3 3-7 7-3-3z"/>
            <path d="m18 13-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/>
            <path d="m2 2 7.586 7.586"/>
            <circle cx="11" cy="11" r="2"/>
          </svg>
        </template>
        <template v-else-if="s === 'Assistant'">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M3 18v-6a9 9 0 0 1 18 0v6"/>
            <rect x="3" y="18" width="18" height="3" rx="1.5"/>
            <path d="M8 21v-3"/>
            <path d="M16 21v-3"/>
          </svg>
        </template>
        <template v-else-if="s === 'TTS'">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/>
            <path d="M19.07 4.93a10 10 0 0 1 0 14.14"/>
            <path d="M15.54 8.46a5 5 0 0 1 0 7.07"/>
          </svg>
        </template>
        <template v-else-if="s === 'STT'">
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/>
            <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
            <line x1="12" x2="12" y1="19" y2="22"/>
          </svg>
        </template>
        <template v-else>
          <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="3"/>
            <rect x="11" y="0" width="2" height="4" rx="1"/>
            <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(60 12 12)"/>
            <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(120 12 12)"/>
            <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(180 12 12)"/>
            <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(240 12 12)"/>
            <rect x="11" y="0" width="2" height="4" rx="1" transform="rotate(300 12 12)"/>
          </svg>
        </template>
        <span v-if="props.sidebarOpen">{{ s }}</span>
      </button>

      <!-- Sublinks under Prompt: History -->
      <button
        v-if="s === 'Prompt'"
        class="side-subtab"
        :class="{ active: props.activeSection === 'Prompt' && props.promptSubview === 'History' }"
        @click="$emit('open-history')"
        title="Conversation History"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <circle cx="12" cy="12" r="8"/>
          <path d="M12 8v4l3 3"/>
          <path d="M3 12a9 9 0 1 0 9-9"/>
          <polyline points="3 12 3 7 8 7"/>
        </svg>
        <span v-if="props.sidebarOpen">History</span>
      </button>

      <!-- Sublinks under Settings: submenus -->
      <button
        v-if="s === 'Settings'"
        class="side-subtab"
        :class="{ active: props.activeSection === 'Settings' && props.settingsSubview === 'General' }"
        @click="$emit('set-settings-subview', 'General')"
        title="General Settings"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <line x1="4" y1="21" x2="4" y2="14"/>
          <line x1="4" y1="10" x2="4" y2="3"/>
          <line x1="12" y1="21" x2="12" y2="12"/>
          <line x1="12" y1="8" x2="12" y2="3"/>
          <line x1="20" y1="21" x2="20" y2="16"/>
          <line x1="20" y1="12" x2="20" y2="3"/>
          <line x1="2" y1="14" x2="6" y2="14"/>
          <line x1="10" y1="8" x2="14" y2="8"/>
          <line x1="18" y1="16" x2="22" y2="16"/>
        </svg>
        <span v-if="props.sidebarOpen">General</span>
      </button>

      <button
        v-if="s === 'Settings'"
        class="side-subtab"
        :class="{ active: props.activeSection === 'Settings' && props.settingsSubview === 'Speech To Text' }"
        @click="$emit('set-settings-subview', 'Speech To Text')"
        title="Speech To Text"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/>
          <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
          <line x1="12" x2="12" y1="19" y2="22"/>
        </svg>
        <span v-if="props.sidebarOpen">Speech To Text</span>
      </button>

      <button
        v-if="s === 'Settings'"
        class="side-subtab"
        :class="{ active: props.activeSection === 'Settings' && props.settingsSubview === 'Quick Prompts' }"
        @click="$emit('set-settings-subview', 'Quick Prompts')"
        title="Quick Prompts"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
        </svg>
        <span v-if="props.sidebarOpen">Quick Prompts</span>
      </button>
      <button
        v-if="s === 'Settings'"
        class="side-subtab"
        :class="{ active: props.activeSection === 'Settings' && props.settingsSubview === 'MCP Servers' }"
        @click="$emit('set-settings-subview', 'MCP Servers')"
        title="MCP Servers"
      >
        <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
          <rect x="2" y="2" width="20" height="8" rx="2"/>
          <rect x="2" y="14" width="20" height="8" rx="2"/>
          <line x1="6" y1="6" x2="6.01" y2="6"/>
          <line x1="6" y1="18" x2="6.01" y2="18"/>
        </svg>
        <span v-if="props.sidebarOpen">MCP Servers</span>
      </button>
    </template>
    <div class="side-spacer"></div>
    <div class="side-status"><LoadingDots v-if="props.busy" text="Working" /></div>
  </aside>
</template>

<style scoped>
.sidebar { width: 220px; background: var(--adc-sidebar-bg); border-right: 1px solid var(--adc-border); padding: 10px 8px; display: flex; flex-direction: column; gap: 6px; transition: width 0.2s ease; }
.sidebar.collapsed { width: 64px; }
.burger { padding: 8px 10px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; }
.side-tab { padding: 10px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; text-align: left; display: flex; align-items: center; gap: 8px; }
.side-tab svg, .side-subtab svg { width: 16px; height: 16px; }
.side-tab.active { background: var(--adc-accent); border-color: var(--adc-accent); }
.side-subtab { margin-left: 14px; padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); cursor: pointer; text-align: left; font-size: 12px; display: flex; align-items: center; gap: 8px; }
.side-subtab.active { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.side-spacer { flex: 1; }
.side-status { padding-top: 8px; }
</style>
