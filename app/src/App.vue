<script setup lang="ts">
import QuickActions from './QuickActions.vue'
import PromptPanel from './components/PromptPanel.vue'
import CaptureOverlay from './components/CaptureOverlay.vue'
import ConversationHistory from './components/ConversationHistory.vue'
import PromptMain from './components/prompt/PromptMain.vue'
import TTSPanel from './components/TTSPanel.vue'
import STTPanel from './components/STTPanel.vue'
import SidebarNav from './components/sidebar/SidebarNav.vue'
import SettingsMain from './components/settings/SettingsMain.vue'
import conversation, { appendMessage, clearAllConversations, newConversation, updateMessage, getPersistState } from './state/conversation'
import { onMounted, onBeforeUnmount, reactive, ref, watch, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { applyGlobalHotkey, checkShortcutAvailable } from './hotkeys'
import { useToast } from './composables/useToast'
import { useQuickPrompts } from './composables/useQuickPrompts'
import { useSettings } from './composables/useSettings'
import { parseArgs, normalizeEnvInput, parseJsonObject } from './composables/utils'
import { useMcp } from './composables/useMcp'
import { useTtsBackground } from './composables/useTtsBackground'
import { useAppEvents } from './composables/useAppEvents'
import { useThemeStyle } from './composables/useThemeStyle'
import { useWindowMode } from './composables/useWindowMode'
import { useBusy } from './composables/useBusy'
import { useConversationPersist } from './composables/useConversationPersist'
import { useSettingsAutosave } from './composables/useSettingsAutosave'
import { useSettingsSave } from './composables/useSettingsSave'
import { preloadTokenizer } from './composables/useTokenizer'

const { isQuickActions, isCaptureOverlay, addBodyClass, removeBodyClass } = useWindowMode()

// Reactive state for Prompt flow in the main window
const prompt = reactive({
  visible: false,
  selection: '',
  preview: '',
  length: 0,
})

// Simple section navigation for Main Window
const ui = reactive({
  sections: ['Prompt', 'TTS', 'STT', 'Settings'] as const,
  activeSection: 'Prompt' as 'Prompt' | 'TTS' | 'STT' | 'Settings',
  promptSubview: 'Chat' as 'Chat' | 'History',
  settingsSubview: 'General' as 'General' | 'Quick Prompts' | 'MCP Servers',
})

// Layout state for sidebar
const layout = reactive({ sidebarOpen: true })

// Aggregate busy state from child sections (via composable)
// Note: models is from useSettings below; we initialize busy after settings

// Bindable input value for the PromptComposer so other sections can prefill it
const composerInput = ref('')
// Ref to TTS panel for programmatic control
const ttsRef = ref<InstanceType<typeof TTSPanel> | null>(null)
// Hidden background TTS controller via composable
const ttsBgRef = ref<InstanceType<typeof TTSPanel> | null>(null)
const { ttsBg, registerBackgroundTtsEvents } = useTtsBackground(ttsBgRef as any)
// Ref to PromptMain to allow programmatic focus on composer
const composerRef = ref<InstanceType<typeof PromptMain> | null>(null)
// (Quick Prompts editor now encapsulated in SettingsQuickPrompts)
const appVersion = ref('')

// Toast state via composable
const { toast, showToast } = useToast()

// Persistence wiring via composable

let unsubs: Array<() => void> = []

onMounted(async () => {
  // For QuickActions popup, strip global app padding/min-width via body class
  try { addBodyClass() } catch {}
  try {
    const unsubApp = await registerAppEvents()
    const unsubTtsBg = await registerBackgroundTtsEvents()
    const unsubPersist = registerConversationPersist()
    unsubs.push(unsubApp)
    unsubs.push(unsubTtsBg)
    unsubs.push(unsubPersist)
  } catch (err) {
    console.error('[app] event listen failed', err)
  }
  // Load prompt settings on mount
  try { await loadSettings() } catch {} finally { setSettingsLoaded(true) }
  // Preload tokenizer if requested by settings; show hint if unavailable
  try {
    if (settings.tokenizer_mode === 'tiktoken') {
      const ok = await preloadTokenizer()
      if (!ok) showToast('Tokenizer library not installed. Run "npm install" in app/ to enable accurate token counts.', 'error', 3200)
    }
  } catch {}
  // Apply style-specific CSS after loading settings (all windows)
  try { applyStyleCss(settings.ui_style) } catch {}

  // Load persisted conversation if enabled
  try { await loadPersistedConversation() } catch {}
  // Load Quick Prompts for composer buttons
  try { await loadQuickPrompts() } catch {}
  // MCP events are handled inside registerAppEvents()
  // Attempt auto-connect for MCP servers after settings load
  try {
    await autoConnectServers()
  } catch {}
  // App version for footer
  try { appVersion.value = await getVersion() } catch {}
})

onBeforeUnmount(() => {
  try { unsubs.forEach(u => u()); } finally { unsubs = [] }
  try { removeBodyClass() } catch {}
})

// Removed legacy Quick Prompts helpers (now handled inside SettingsQuickPrompts)

// Quick Prompts via composable (read-only for main UI)
const qp = useQuickPrompts(composerInput as any, composerRef as any)
const quickPrompts = qp.quickPrompts
const loadQuickPrompts = qp.loadQuickPrompts
const insertQuickPrompt = qp.insertQuickPrompt
const activeQuickPrompt = qp.activeQuickPrompt
const selectedSystemPrompt = qp.selectedSystemPrompt
// Combine system prompt for chat: when a quick prompt is active, prefer the
// Quick Prompts specific system prompt (if set), otherwise fall back to global.
// Always append the active quick prompt template when active.
const combinedSystemPrompt = computed(() => {
  const qpText = (selectedSystemPrompt.value || '').trim()
  const hasQuick = !!qpText
  const baseCandidate = hasQuick
    ? (settings.quick_prompt_system_prompt || settings.system_prompt || '')
    : (settings.system_prompt || '')
  const base = (baseCandidate || '').trim()
  return [base, qpText].filter(Boolean).join('\n\n')
})
const toggleQuickPrompt = qp.toggleQuickPrompt

// ---------------------------
// Prompt Settings state & actions (via composable)
// ---------------------------
const { settings, models, loadSettings } = useSettings()
// Aggregate busy state now that models is available
const { busy, isBusy } = useBusy(computed(() => models.loading))
// Conversation persistence via composable
const { loadPersistedConversation, registerConversationPersist } = useConversationPersist(computed(() => settings.persist_conversations), showToast)
// Theme/style loader via composable
const { applyStyleCss } = useThemeStyle(computed(() => settings.ui_style))

// Initialize settings auto-save (silent on success)
const { setLoaded: setSettingsLoaded } = useSettingsAutosave(settings as any, showToast)
// Manual save helper (with success toast)
const { saveSettingsNow } = useSettingsSave(settings as any, showToast)

// MCP composable (provides server helpers and actions)
const mcp = useMcp(settings, showToast)

// App-wide event wiring (prompt, images, TTS open, MCP lifecycle, chat tool events)
const { registerAppEvents } = useAppEvents({
  prompt,
  ui,
  ttsRef,
  composerInput,
  composerRef,
  appendMessage,
  newConversation,
  updateMessage,
  findServerById: (id: string) => mcp.findServerById(id),
  showToast,
  setSection: (s: 'Prompt' | 'TTS' | 'STT' | 'Settings') => { ui.activeSection = s; if (s === 'Prompt') ui.promptSubview = 'Chat' },
})

async function saveSettings() { try { await saveSettingsNow() } catch {} }

async function connectServer(s: any) { await mcp.connectServer(s) }

async function disconnectServer(s: any) { await mcp.disconnectServer(s) }

async function pingServer(s: any) { await mcp.pingServer(s) }

async function listTools(s: any) { await mcp.listTools(s) }

// If user switches tokenizer mode at runtime, preload tokenizer
watch(() => settings.tokenizer_mode, async (mode) => {
  if (mode === 'tiktoken') {
    try {
      const ok = await preloadTokenizer()
      if (!ok) showToast('Tokenizer library not installed. Run "npm install" in app/ to enable accurate token counts.', 'error', 3200)
    } catch {}
  }
})

// ConversationView event handlers
function onListTools(serverId: string) {
  const s = mcp.findServerById(serverId)
  if (s) listTools(s)
}

// Wrap onToggleTool to persist immediately for privacy-first UX
function onToggleTool(payload: { serverId: string; tool: string; enabled: boolean }) {
  mcp.onToggleTool(payload)
  try { saveSettings() } catch {}
}

function makeArgsTemplateFromSchema(schema: any): any {
  try {
    const props = schema?.properties || schema?.inputSchema?.properties || schema?.input_schema?.properties
    if (!props || typeof props !== 'object') return {}
    const obj: any = {}
    for (const [k, v] of Object.entries<any>(props)) {
      const typ = (v && v.type) || 'string'
      obj[k] = typ === 'number' || typ === 'integer' ? 0
        : typ === 'boolean' ? false
        : typ === 'array' ? []
        : typ === 'object' ? {}
        : ''
    }
    return obj
  } catch { return {} }
}

function fillArgsTemplate(s: any) {
  try {
    const t = selectedToolObj(s)
    const schema = t?.inputSchema ?? t?.input_schema ?? t?.schema
    const tmpl = makeArgsTemplateFromSchema(schema || {})
    s.toolArgsJson = JSON.stringify(tmpl, null, 2)
  } catch {}
}

function addMcpServer() {
  const id = `server-${(settings.mcp_servers.length + 1)}`
  settings.mcp_servers.push({
    id,
    transport: 'stdio',
    command: '',
    args: [],
    argsText: '',
    cwd: '',
    env: {},
    envJson: '{ "LOG_LEVEL": "info" }',
    auto_connect: false,
    status: 'disconnected',
    connecting: false,
    error: null,
    tools: [],
    toolsOpen: false,
    selectedTool: '',
    toolArgsJson: '{}',
    toolArgsError: null,
    toolResults: [],
    envError: null,
  })
}

function removeMcpServer(idx: number) {
  if (idx >= 0 && idx < settings.mcp_servers.length) settings.mcp_servers.splice(idx, 1)
}

async function refreshModels() {
  models.loading = true; models.error = null
  models.list = []
  try {
    const ids = await invoke<string[]>('list_openai_models')
    models.list = ids
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    models.error = msg
    showToast(`Model list failed: ${msg}`, 'error')
  } finally {
    models.loading = false
  }
}

async function onClearConversations() {
  // Reset history to a brand new conversation
  clearAllConversations()
  showToast('Conversation cleared.', 'success')
  if (settings.persist_conversations) {
    try { await invoke<string>('save_conversation_state', { state: getPersistState() }) } catch {}
  } else {
    try { await invoke<string>('clear_conversations') } catch {}
  }
}

function handleUseAsPrompt(text: string) {
  try {
    composerInput.value = text
    ui.activeSection = 'Prompt'
    showToast('Transcript inserted into prompt input. Edit then press Enter to send.', 'success', 1800)
  } catch (e) {
    console.warn('[stt] use-as-prompt failed', e)
  }
}

function setSection(s: 'Prompt' | 'TTS' | 'STT' | 'Settings') {
  ui.activeSection = s
  if (s === 'Prompt') ui.promptSubview = 'Chat'
}

// Attempt auto-connecting MCP servers based on per-server flag only
// Non-blocking: kick off connects concurrently and add a timeout guard per server
async function autoConnectServers() {
  try {
    for (const s of settings.mcp_servers) {
      const want = s.auto_connect === true
      if (!want) continue
      if (s.connecting || s.status === 'connected') continue
      if (!s || !s.id || !s.command) continue
      // Fire-and-forget connect; backend events will update state on success/failure
      try { connectServer(s) } catch {}
      // Timeout guard to avoid indefinite "connecting" state
      const timeoutMs = 10000
      setTimeout(() => {
        if (s.connecting) {
          s.connecting = false
          s.error = 'Connect timed out'
          showToast(`Connect timed out: ${s.id}`, 'error')
        }
      }, timeoutMs)
    }
  } catch (e) {
    console.warn('[mcp] autoConnectServers failed', e)
  }
}

</script>

<template>
  <QuickActions v-if="isQuickActions" />
  <CaptureOverlay v-else-if="isCaptureOverlay" />
  <div v-else>
    <PromptPanel
      v-if="prompt.visible"
      :selection="prompt.selection"
      :preview="prompt.preview"
      :length="prompt.length"
      @close="prompt.visible = false"
    />

    <!-- Sidebar layout (dark/light) -->
    <div v-if="settings.ui_style === 'sidebar-dark' || settings.ui_style === 'sidebar-light'" class="shell">
      <SidebarNav
        :sections="ui.sections as any"
        :active-section="ui.activeSection"
        :prompt-subview="ui.promptSubview"
        :settings-subview="ui.settingsSubview"
        :sidebar-open="layout.sidebarOpen"
        :busy="isBusy()"
        @toggle-sidebar="layout.sidebarOpen = !layout.sidebarOpen"
        @set-section="setSection($event)"
        @open-history="ui.activeSection = 'Prompt'; ui.promptSubview = 'History'"
        @set-settings-subview="(s) => { ui.activeSection = 'Settings'; ui.settingsSubview = s }"
      />

      <div class="main">
        <div class="main-content">
          <template v-if="ui.activeSection === 'Prompt'">          
            <div class="section"><div class="section-title">Prompt</div></div>
            <template v-if="ui.promptSubview === 'History'">
              <div class="section">
                <div class="section-title">History</div>
                <ConversationHistory @open="ui.activeSection = 'Prompt'; ui.promptSubview = 'Chat'" />
              </div>
            </template>
            <template v-else>
              <PromptMain
                ref="composerRef"
                :messages="conversation.currentConversation.messages"
                :hideToolCalls="settings.hide_tool_calls_in_chat"
                :mcpServers="settings.mcp_servers"
                :ttsPlayingId="ttsBg.currentMessageId"
                :ttsPlaying="ttsBg.playing"
                :quickPrompts="quickPrompts"
                :activeQuickPrompt="activeQuickPrompt"
                :systemPromptText="combinedSystemPrompt"
                v-model:composerText="composerInput"
                @list-tools="onListTools"
                @toggle-tool="onToggleTool"
                @toggle-quick-prompt="toggleQuickPrompt"
                @busy="busy.prompt = $event"
              />
            </template>
          </template>

          <div v-show="ui.activeSection === 'TTS'" class="section">
            <div class="section-title">Text To Speech</div>
            <TTSPanel ref="ttsRef" :notify="showToast" @busy="busy.tts = $event" />
          </div>

          <div v-if="ui.activeSection === 'STT'" class="section">
            <div class="section-title">Speech To Text</div>
            <STTPanel :notify="showToast" @use-as-prompt="handleUseAsPrompt" @busy="busy.stt = $event" />
          </div>

          <div v-if="ui.activeSection === 'Settings'" class="section">
            <SettingsMain
              :settings="settings"
              :models="models"
              :settings-subview="ui.settingsSubview"
              :onSave="saveSettings"
              :onRefreshModels="refreshModels"
              :onClearConversations="onClearConversations"
              :onAdd="addMcpServer"
              :onRemove="removeMcpServer"
              :onConnect="connectServer"
              :onDisconnect="disconnectServer"
              :onPing="pingServer"
              :onListTools="listTools"
              :onFillArgsTemplate="fillArgsTemplate"
              :onValidateEnvJsonInput="mcp.validateEnvJsonInput"
              :onCallTool="mcp.callTool"
              :selectedToolObj="mcp.selectedToolObj"
            />
          </div>
        </div>
      </div>
    </div>
    
    <div class="version-badge" v-if="appVersion">v{{ appVersion }}</div>
    </div>

    <!-- Hidden background TTS controller (non-disruptive) -->
    <div style="position: fixed; width: 0; height: 0; overflow: hidden; opacity: 0; pointer-events: none;">
      <TTSPanel ref="ttsBgRef" :lightMount="true" />
    </div>

    <!-- Toast -->
    <div v-if="toast.visible" class="toast" :class="toast.kind">{{ toast.message }}</div>

</template>

<style scoped>
/* Section layout */
.content { padding: 12px 0; overflow: auto; }
.section { margin: 0 auto; max-width: none; }
.section-title { font-weight: 700; margin-bottom: 8px; font-size: 18px; }
.section-hint { font-size: 12px; color: var(--adc-fg-muted); }


.settings { margin: 0px auto; max-width: none; color: var(--adc-fg); }
.settings-section { border: 1px solid var(--adc-border); border-radius: 10px; padding: 14px; background: var(--adc-surface); }
.settings-title { font-weight: 700; margin-bottom: 8px; }
.settings-row { display: flow; gap: 10px; align-items: center; margin: 8px 0; }
.settings-row.col { flex-direction: column; align-items: flex-start; }
.settings-hint { font-size: 12px; color: var(--adc-fg-muted); margin-top: 6px; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-accent); color: #fff; cursor: pointer; }
.btn:hover { filter: brightness(1.05); }
.btn.ghost { background: transparent; color: var(--adc-fg); border-color: var(--adc-border); }
.btn.danger { background: var(--adc-danger); border-color: var(--adc-border); }
.row-inline { display: flex; gap: 8px; width: 100%; }
.label { font-size: 12px; color: var(--adc-fg-muted); }
.input { flex: 1; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); }
.checkbox { display: flex; gap: 8px; align-items: center; }
.settings-hint.error { color: #f2b8b8; }

/* Ensure styles apply inside child settings components */
.settings :deep(.settings-section) {padding-left: 10px; padding-right: 10px; border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-surface); }
.settings :deep(.settings-title) { font-weight: 700; margin-bottom: 8px; }
.settings :deep(.settings-row) { display: flex; gap: 10px; align-items: center; margin: 8px 0; }
.settings :deep(.settings-row.col) { flex-direction: column; align-items: flex-start; }
.settings :deep(.settings-hint) { font-size: 12px; color: var(--adc-fg-muted); margin-top: 6px; }
.settings :deep(.btn) { padding: 8px 12px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-accent); color: #fff; cursor: pointer; }
.settings :deep(.btn:hover) { filter: brightness(1.05); }
.settings :deep(.btn.ghost) { background: transparent; color: var(--adc-fg); border-color: var(--adc-border); }
.settings :deep(.btn.danger) { background: var(--adc-danger); border-color: var(--adc-border); }
.settings :deep(.row-inline) { display: flex; gap: 8px; width: 100%; }
.settings :deep(.label) { font-size: 12px; color: var(--adc-fg-muted); }
.settings :deep(.input) { flex: 1; padding: 8px 10px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); }
.settings :deep(.checkbox) { display: flex; gap: 8px; align-items: center; }
.settings :deep(.settings-hint.error) { color: #f2b8b8; }

/* Version badge bottom-right */
.version-badge { position: fixed; left: 12px; bottom: 12px; font-size: 12px; color: var(--adc-fg-muted); background: var(--adc-surface); border: 1px solid var(--adc-border); border-radius: 8px; padding: 4px 8px; opacity: 0.95; z-index: 1000; }

.toast { position: fixed; left: 50%; bottom: 24px; transform: translateX(-50%); padding: 10px 14px; border-radius: 8px; border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); white-space: pre-line; box-shadow: 0 6px 24px rgba(0,0,0,0.3); }
.toast.success { border-color: #285c2a; background: #1e3b21; }
.toast.error { border-color: #5c2828; background: #3b1e1e; }

.shell { display: flex; gap: 0; height: 100vh; text-align: left; }
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
.main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.main-content { flex: 1; min-height: 0; overflow: auto; padding: 12px 12px; }

/* Prompt layout with scrolling conversation */
/* Prompt layout and quick prompt button styles moved to components/prompt/PromptMain.vue */
</style>

<!-- Global overrides for QuickActions window only -->
<style>
body.qa-window #app {
  max-width: none;
  min-width: 0;
  padding: 0;
}
</style>