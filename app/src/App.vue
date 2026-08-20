<script setup lang="ts">
import QuickActions from './QuickActions.vue'
import PromptPanel from './components/PromptPanel.vue'
import CaptureOverlay from './components/CaptureOverlay.vue'
import BusyIndicator from './components/BusyIndicator.vue'
import ConversationHistory from './components/ConversationHistory.vue'
import PromptMain from './components/prompt/PromptMain.vue'
import AssistantMode from './components/assistant/AssistantMode.vue'
import TTSPanel from './components/TTSPanel.vue'
import STTPanel from './components/STTPanel.vue'
import SidebarNav from './components/sidebar/SidebarNav.vue'
import SettingsMain from './components/settings/SettingsMain.vue'
import conversation, { appendMessage, clearAllConversations, newConversation, updateMessage, getPersistState } from './state/conversation'
import { onMounted, onBeforeUnmount, reactive, ref, watch, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getVersion } from '@tauri-apps/api/app'
import { useToast } from './composables/useToast'
import { useQuickPrompts } from './composables/useQuickPrompts'
import { useSettings } from './composables/useSettings'
import { useMcp } from './composables/useMcp'
import { useTtsBackground } from './composables/useTtsBackground'
import { useAppEvents } from './composables/useAppEvents'
import { useThemeStyle } from './composables/useThemeStyle'
import { useWindowMode } from './composables/useWindowMode'
import { useBusy } from './composables/useBusy'
import { useConversationPersist } from './composables/useConversationPersist'
import { useSettingsAutosave } from './composables/useSettingsAutosave'
import { useSettingsSave } from './composables/useSettingsSave'
import { preloadTokenizer, tokenizerLastError } from './composables/useTokenizer'

const { isQuickActions, isCaptureOverlay, isBusyIndicator, addBodyClass, removeBodyClass } = useWindowMode()

// Reactive state for Prompt flow in the main window
const prompt = reactive({
  visible: false,
  selection: '',
  preview: '',
  length: 0,
})

// Simple section navigation for Main Window
const ui = reactive({
  sections: ['Prompt', 'Assistant', 'TTS', 'STT', 'Settings'] as const,
  activeSection: 'Prompt' as 'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings',
  promptSubview: 'Chat' as 'Chat' | 'History',
  settingsSubview: 'General' as 'General' | 'Speech To Text' | 'Quick Prompts' | 'MCP Servers',
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
  // The busy indicator is a passive status pill: it drives itself from backend
  // events and must not pull in settings, MCP auto-connect or the tokenizer.
  if (isBusyIndicator.value) return
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

  // Defer non-critical startup work to keep UI responsive.
  setTimeout(() => {
    // Preload tokenizer if requested by settings; show hint if unavailable
    if (settings.tokenizer_mode === 'tiktoken') {
      preloadTokenizer().then((ok) => {
        if (!ok) {
          try { console.warn('[tokenizer] init failed; using approximate counts', tokenizerLastError?.value) } catch {}
          showToast('Accurate tokenizer unavailable; using approximate counts.', 'error', 4200)
        }
      }).catch(() => {})
    }

    // Load persisted conversation if enabled
    loadPersistedConversation().catch(() => {})
    // Load Quick Prompts for composer buttons
    loadQuickPrompts().catch(() => {})
    // MCP events are handled inside registerAppEvents()
    // Attempt auto-connect for MCP servers after settings load
    autoConnectServers().catch(() => {})
    // App version for footer
    getVersion().then((v) => { appVersion.value = v }).catch(() => {})
  }, 0)
})

onBeforeUnmount(() => {
  try { flushSettings() } catch {}
  try { flushPersist() } catch {}
  try { unsubs.forEach(u => u()); } finally { unsubs = [] }
  try { removeBodyClass() } catch {}
})

// Removed legacy Quick Prompts helpers (now handled inside SettingsQuickPrompts)

// Quick Prompts via composable (read-only for main UI)
const qp = useQuickPrompts(composerInput as any, composerRef as any)
const quickPrompts = qp.quickPrompts
const loadQuickPrompts = qp.loadQuickPrompts
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
const { loadPersistedConversation, registerConversationPersist, flushPersist } = useConversationPersist(computed(() => settings.persist_conversations), showToast)
// Theme/style loader via composable
useThemeStyle(computed(() => settings.ui_style))

// Initialize settings auto-save (silent on success)
const { setLoaded: setSettingsLoaded, flush: flushSettings } = useSettingsAutosave(settings as any, showToast)
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
  updateMessage: (id: string, patch: any) => { try { return !!updateMessage(id as any, patch) } catch { return false } },
  findServerById: (id: string) => mcp.findServerById(id),
  showToast,
  setSection: (s: 'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings') => { ui.activeSection = s; if (s === 'Prompt') ui.promptSubview = 'Chat' },
  openAssistant,
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
      if (!ok) {
        try { console.warn('[tokenizer] init failed; using approximate counts', tokenizerLastError?.value) } catch {}
        showToast('Accurate tokenizer unavailable; using approximate counts.', 'error', 4200)
      }
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
    const t = mcp.selectedToolObj(s)
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

// Set by the assistant:open event so the panel starts once it is on screen.
const assistantAutostart = ref(0)

function openAssistant(autostart: boolean) {
  ui.activeSection = 'Assistant'
  // A counter rather than a boolean: pressing the hotkey twice has to start a
  // session twice, and a boolean that is already true emits no change.
  if (autostart) assistantAutostart.value += 1
}

function setSection(s: 'Prompt' | 'Assistant' | 'TTS' | 'STT' | 'Settings') {
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
  <BusyIndicator v-else-if="isBusyIndicator" />
  <div v-else>
    <PromptPanel
      v-if="prompt.visible"
      :selection="prompt.selection"
      :preview="prompt.preview"
      :length="prompt.length"
      @close="prompt.visible = false"
    />

    <!-- Sidebar layout is the only supported main-window layout; always render it as a safe fallback. -->
    <div class="shell">
      <SidebarNav
        :sections="ui.sections as any"
        :active-section="ui.activeSection"
        :prompt-subview="ui.promptSubview"
        :settings-subview="ui.settingsSubview"
        :sidebar-open="layout.sidebarOpen"
        :busy="isBusy"
        :version="appVersion"
        @toggle-sidebar="layout.sidebarOpen = !layout.sidebarOpen"
        @set-section="setSection($event)"
        @open-history="ui.activeSection = 'Prompt'; ui.promptSubview = 'History'"
        @set-settings-subview="(s) => { ui.activeSection = 'Settings'; ui.settingsSubview = s }"
      />

      <div class="main">
        <div class="main-content">
          <template v-if="ui.activeSection === 'Prompt'">
            <div v-if="ui.promptSubview === 'History'" class="page">
              <header class="page-head">
                <div>
                  <h1 class="page-title">History</h1>
                  <p class="page-desc">Earlier conversations. Opening one makes it the current chat.</p>
                </div>
              </header>
              <ConversationHistory @open="ui.activeSection = 'Prompt'; ui.promptSubview = 'Chat'" />
            </div>
            <div v-show="ui.promptSubview !== 'History'" class="page fill">
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
            </div>
          </template>

          <div v-show="ui.activeSection === 'Assistant'" class="page">
            <header class="page-head">
              <div>
                <h1 class="page-title">Assistant Mode</h1>
                <p class="page-desc">A live voice session over WebRTC. Speak and it answers; tools and the supervisor are optional.</p>
              </div>
            </header>
            <AssistantMode :mcpServers="settings.mcp_servers" :notify="showToast" :autostart="assistantAutostart" />
          </div>

          <div v-show="ui.activeSection === 'TTS'" class="page">
            <header class="page-head">
              <div>
                <h1 class="page-title">Text To Speech</h1>
                <p class="page-desc">Read text aloud with a Windows voice or an OpenAI one.</p>
              </div>
            </header>
            <TTSPanel ref="ttsRef" :notify="showToast" @busy="busy.tts = $event" />
          </div>

          <div v-if="ui.activeSection === 'STT'" class="page">
            <header class="page-head">
              <div>
                <h1 class="page-title">Speech To Text</h1>
                <p class="page-desc">Record from the microphone and transcribe it. Engine and model live in Settings.</p>
              </div>
            </header>
            <STTPanel :notify="showToast" @use-as-prompt="handleUseAsPrompt" @busy="busy.stt = $event" />
          </div>

          <div v-if="ui.activeSection === 'Settings'" class="page">
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
              :notify="showToast"
            />
          </div>
        </div>
      </div>
    </div>

    <!-- Hidden background TTS controller (non-disruptive) -->
    <div style="position: fixed; width: 0; height: 0; overflow: hidden; opacity: 0; pointer-events: none;">
      <TTSPanel ref="ttsBgRef" :lightMount="true" />
    </div>

    <!-- Toast -->
    <div v-if="toast.visible" class="toast" :class="toast.kind">{{ toast.message }}</div>
    </div>

</template>

<style scoped>
/* Layout only. Cards, fields, inputs, buttons and the rest come from the
   shared design layer in src/style.css, which is why the long `:deep()` copy
   of every settings rule that used to live here is gone: the child components
   now pick those classes up globally instead of needing them piped through. */

.shell {
  display: flex;
  height: 100vh;
  text-align: left;
  background: var(--adc-bg);
  color: var(--adc-fg);
}

.main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.main-content {
  flex: 1;
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  padding: var(--sp-5) var(--sp-5) var(--sp-6);
}

/* The chat page is the one section that should consume the leftover height
   rather than sit at its natural size, so the composer stays pinned. */
.main-content > .page.fill {
  flex: 1;
  min-height: 0;
}

.toast {
  position: fixed;
  left: 50%;
  bottom: var(--sp-5);
  transform: translateX(-50%);
  z-index: 1000;
  max-width: min(560px, calc(100vw - 48px));
  padding: var(--sp-3) var(--sp-4);
  border-radius: var(--radius);
  border: 1px solid var(--adc-border-strong);
  background: var(--adc-surface);
  color: var(--adc-fg);
  font-size: var(--fs-base);
  text-align: left;
  white-space: pre-line;
  box-shadow: var(--shadow-2);
}
.toast.success {
  background: var(--adc-ok-bg);
  border-color: var(--adc-ok-border);
  color: var(--adc-ok-fg);
}
.toast.error {
  background: var(--adc-err-bg);
  border-color: var(--adc-err-border);
  color: var(--adc-err-fg);
}
</style>

<!-- Global overrides for QuickActions window only -->
<style>
body.qa-window #app {
  max-width: none;
  min-width: 0;
  padding: 0;
}
</style>