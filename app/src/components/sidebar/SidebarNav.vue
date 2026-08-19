<script setup lang="ts">
import { computed } from 'vue'
import LoadingDots from '../LoadingDots.vue'
import NavIcon, { type NavIconName } from './NavIcon.vue'

type Section = 'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings'
type SettingsSubview = 'General' | 'Speech To Text' | 'Quick Prompts' | 'MCP Servers'

const props = defineProps<{
  sections: ReadonlyArray<Section>
  activeSection: Section
  promptSubview: 'Chat' | 'History'
  settingsSubview: SettingsSubview
  sidebarOpen: boolean
  busy: boolean
  version?: string
}>()

const emit = defineEmits<{
  (e: 'toggle-sidebar'): void
  (e: 'set-section', section: Section): void
  (e: 'open-history'): void
  (e: 'set-settings-subview', sub: SettingsSubview): void
}>()

const SECTION_ICONS: Record<Section, NavIconName> = {
  Prompt: 'prompt',
  Assistant: 'assistant',
  TTS: 'tts',
  STT: 'stt',
  Settings: 'settings',
}

const SETTINGS_SUBVIEWS: ReadonlyArray<{ key: SettingsSubview, icon: NavIconName }> = [
  { key: 'General', icon: 'general' },
  { key: 'Speech To Text', icon: 'stt' },
  { key: 'Quick Prompts', icon: 'quick-prompts' },
  { key: 'MCP Servers', icon: 'mcp' },
]

/** Sub-items stay visible whichever section is active. Hiding them until their
 *  parent is selected would tidy the rail at the cost of a second click to
 *  reach a frequent destination like MCP Servers. */
function subItemsFor(section: Section) {
  if (section === 'Prompt') {
    return [{
      key: 'History',
      icon: 'history' as NavIconName,
      active: props.activeSection === 'Prompt' && props.promptSubview === 'History',
    }]
  }
  if (section === 'Settings') {
    return SETTINGS_SUBVIEWS.map(s => ({
      key: s.key,
      icon: s.icon,
      active: props.activeSection === 'Settings' && props.settingsSubview === s.key,
    }))
  }
  return []
}

function onSubItem(section: Section, key: string) {
  if (section === 'Prompt') emit('open-history')
  else emit('set-settings-subview', key as SettingsSubview)
}

const collapsed = computed(() => !props.sidebarOpen)
</script>

<template>
  <aside class="sidebar" :class="{ collapsed }" role="navigation" aria-label="Main navigation">
    <div class="side-head">
      <button
        class="burger"
        type="button"
        :title="props.sidebarOpen ? 'Collapse menu' : 'Expand menu'"
        :aria-expanded="props.sidebarOpen"
        aria-label="Toggle menu"
        @click="emit('toggle-sidebar')"
      >
        <NavIcon name="menu" />
      </button>
    </div>

    <nav class="side-nav">
      <template v-for="s in props.sections" :key="s">
        <button
          class="side-tab"
          type="button"
          :class="{ active: props.activeSection === s }"
          :aria-current="props.activeSection === s ? 'page' : undefined"
          :title="s"
          @click="emit('set-section', s)"
        >
          <NavIcon :name="SECTION_ICONS[s]" />
          <span v-if="props.sidebarOpen" class="side-label">{{ s }}</span>
        </button>

        <button
          v-for="sub in subItemsFor(s)"
          :key="s + '/' + sub.key"
          class="side-subtab"
          type="button"
          :class="{ active: sub.active }"
          :aria-current="sub.active ? 'page' : undefined"
          :title="sub.key"
          @click="onSubItem(s, sub.key)"
        >
          <NavIcon :name="sub.icon" />
          <span v-if="props.sidebarOpen" class="side-label">{{ sub.key }}</span>
        </button>
      </template>
    </nav>

    <div class="side-spacer"></div>

    <div class="side-foot">
      <div class="side-status"><LoadingDots v-if="props.busy" text="Working" /></div>
      <div v-if="props.version && props.sidebarOpen" class="side-version">v{{ props.version }}</div>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  width: 216px;
  flex: 0 0 auto;
  background: var(--adc-sidebar-bg);
  border-right: 1px solid var(--adc-border);
  padding: var(--sp-3) var(--sp-2);
  display: flex;
  flex-direction: column;
  gap: var(--sp-1);
  box-sizing: border-box;
  transition: width 0.18s ease;
  overflow: hidden;
}
.sidebar.collapsed { width: 60px; }

.side-head {
  display: flex;
  align-items: center;
  padding: 0 var(--sp-1) var(--sp-2);
}

/* An icon button, not a nav item: no fill, no border, so it stops reading as
   a sixth entry in the list. */
.burger {
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  padding: 0;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--adc-fg-muted);
  cursor: pointer;
}
.burger:hover { background: var(--adc-hover); color: var(--adc-fg); }
.burger:focus-visible { outline: none; box-shadow: 0 0 0 2px var(--adc-focus-ring); }
.burger :deep(svg) { width: 18px; height: 18px; }

.side-nav {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}

/* Nav items carry no border or surface of their own. Nine outlined boxes
   stacked in a rail is what made this look busy; hover and the active fill
   are enough to show state. */
.side-tab,
.side-subtab {
  appearance: none;
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  width: 100%;
  box-sizing: border-box;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--adc-fg);
  font-family: inherit;
  text-align: left;
  cursor: pointer;
  white-space: nowrap;
}

.side-tab {
  min-height: 36px;
  padding: 0 var(--sp-3);
  font-size: var(--fs-base);
  font-weight: 500;
}
.side-tab :deep(svg) { width: 17px; height: 17px; flex: 0 0 auto; }

.side-subtab {
  min-height: 30px;
  /* Indent aligns the sub-item's icon under the parent's label, so the
     hierarchy reads without a connector line. */
  padding: 0 var(--sp-3) 0 var(--sp-6);
  font-size: var(--fs-sm);
  color: var(--adc-fg-muted);
}
.side-subtab :deep(svg) { width: 14px; height: 14px; flex: 0 0 auto; }

.side-tab:hover,
.side-subtab:hover { background: var(--adc-hover); }

.side-tab.active {
  background: var(--adc-accent);
  color: #fff;
}
/* A sub-item is subordinate to its parent, so it gets a tint and accent text
   rather than the parent's solid fill. Two solid blue blocks stacked would
   read as two equals. */
.side-subtab.active {
  background: var(--adc-hover);
  color: var(--adc-accent);
  font-weight: 600;
}

.side-tab:focus-visible,
.side-subtab:focus-visible {
  outline: none;
  box-shadow: 0 0 0 2px var(--adc-focus-ring);
}

.side-label { overflow: hidden; text-overflow: ellipsis; }

/* Collapsed: icons centre themselves and the sub-item indent is dropped,
   otherwise the children sit visibly off-axis from their parents. */
.collapsed .side-tab,
.collapsed .side-subtab {
  justify-content: center;
  padding-left: 0;
  padding-right: 0;
}

.side-spacer { flex: 1 1 auto; }

.side-foot {
  display: flex;
  align-items: center;
  gap: var(--sp-2);
  padding: var(--sp-2) var(--sp-3) 0;
  border-top: 1px solid var(--adc-border);
  min-height: 32px;
}
.side-status { min-width: 0; flex: 1 1 auto; }
.side-version {
  font-size: var(--fs-xs);
  color: var(--adc-fg-muted);
  font-variant-numeric: tabular-nums;
}
.collapsed .side-foot { justify-content: center; padding-left: 0; padding-right: 0; }

@media (prefers-reduced-motion: reduce) {
  .sidebar { transition: none; }
}
</style>
