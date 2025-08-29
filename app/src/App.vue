<script setup lang="ts">
import QuickActions from './QuickActions.vue'
import PromptPanel from './components/PromptPanel.vue'
import QuickPromptsEditor from './components/QuickPromptsEditor.vue'
import CaptureOverlay from './components/CaptureOverlay.vue'
import ConversationView from './components/ConversationView.vue'
import ConversationHistory from './components/ConversationHistory.vue'
import PromptComposer from './components/PromptComposer.vue'
import TTSPanel from './components/TTSPanel.vue'
import STTPanel from './components/STTPanel.vue'
import LoadingDots from './components/LoadingDots.vue'
import conversation, { appendMessage, getPersistState, setPersistState, clearAllConversations } from './state/conversation'
import { onMounted, onBeforeUnmount, reactive, ref, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
// Per-style CSS asset URLs (bundler-resolved)
// Add new styles by importing their style.css with ?url and extending styleCssMap below
import sidebarStyleUrl from './styles/sidebar/style.css?url'
import tabsStyleUrl from './styles/tabs/style.css?url'

const winParam = new URLSearchParams(window.location.search).get('window')
const isQuickActions = winParam === 'quick-actions'
const isCaptureOverlay = winParam === 'capture-overlay'

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
})

// Layout state for sidebar
const layout = reactive({ sidebarOpen: true })

// Aggregate busy state from child sections
const busy = reactive({ prompt: false, tts: false, stt: false })
const isBusy = () => busy.prompt || busy.tts || busy.stt || models.loading

// Bindable input value for the PromptComposer so other sections can prefill it
const composerInput = ref('')
// Ref to TTS panel for programmatic control
const ttsRef = ref<InstanceType<typeof TTSPanel> | null>(null)

// Simple toast state
const toast = reactive({
  visible: false,
  message: '',
  kind: 'error' as 'error' | 'success',
  hideTimer: 0 as any
})

function showToast(message: string, kind: 'error' | 'success' = 'error', ms = 3500) {
  toast.message = message
  toast.kind = kind
  toast.visible = true
  if (toast.hideTimer) clearTimeout(toast.hideTimer)
  toast.hideTimer = setTimeout(() => { toast.visible = false }, ms)
}

// ---------------------------
// Persistence: load on startup and auto-save on changes
// ---------------------------
async function loadPersistedConversation() {
  if (!settings.persist_conversations) return
  try {
    const v = await invoke<any>('load_conversation_state')
    if (v && typeof v === 'object' && Object.keys(v).length > 0) {
      const ok = setPersistState(v)
      if (ok) showToast('Loaded conversation history.', 'success', 2000)
    }
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`Failed to load conversation history: ${msg}`, 'error')
  }
}

let saveDebounce: any = 0
function schedulePersistSave() {
  if (!settings.persist_conversations) return
  if (saveDebounce) clearTimeout(saveDebounce)
  saveDebounce = setTimeout(async () => {
    try {
      await invoke<string>('save_conversation_state', { state: getPersistState() })
    } catch (e) {
      console.warn('[persist] save failed', e)
    }
  }, 300)
}

watch(() => conversation.currentConversation.messages.length, () => schedulePersistSave())
// persist when switching current conversation (so currentId is saved)
watch(() => conversation.currentConversation.id, () => schedulePersistSave())
// persist when conversations are added/removed
watch(() => conversation.conversations.length, () => schedulePersistSave())

let unsubs: Array<() => void> = []

onMounted(async () => {
  try {
    const u1 = await listen<{ selection: string; preview: string; length: number }>('prompt:open', (e) => {
      const p = e.payload || ({} as any)
      prompt.selection = typeof p.selection === 'string' ? p.selection : ''
      prompt.preview = typeof p.preview === 'string' ? p.preview : prompt.selection.slice(0, 200)
      prompt.length = typeof p.length === 'number' ? p.length : prompt.preview.length
      prompt.visible = true
    })
    unsubs.push(u1)

    const u2 = await listen<{ message: string; path?: string }>('settings:quick-prompts-error', (e) => {
      const p = e.payload || ({} as any)
      const where = p.path ? `\n${p.path}` : ''
      showToast(`Quick Prompts config error: ${p.message}${where}`, 'error')
    })
    unsubs.push(u2)

    const u3 = await listen<{ path: string }>('image:capture', async (e) => {
      const p = e.payload || ({} as any)
      if (p.path) {
        showToast(`Image captured:\n${p.path}`, 'success')
        // Try to close overlay window if it somehow remained
        try {
          const ow = await WebviewWindow.getByLabel('capture-overlay')
          if (ow) await ow.close()
        } catch {}
        // Append image message to conversation and switch to Prompt section
        try {
          const src = convertFileSrc(p.path)
          appendMessage({ role: 'user', type: 'image', images: [{ path: p.path, src }] })
          ui.activeSection = 'Prompt'
        } catch {}
      }
    })
    unsubs.push(u3)

    // Insert text directly into the Prompt composer (from Quick Actions STT)
    const u4 = await listen<{ text: string }>('prompt:insert', (e) => {
      const p = e.payload || ({} as any)
      if (typeof p.text === 'string') {
        composerInput.value = p.text
        ui.activeSection = 'Prompt'
        showToast('Transcript inserted into prompt input. Edit then press Enter to send.', 'success', 1800)
      }
    })
    unsubs.push(u4)

    // TTS errors surfaced from backend (Quick Actions TTS or TTS panel)
    const u5 = await listen<{ message: string }>('tts:error', (e) => {
      const p = e.payload || ({} as any)
      const msg = typeof p.message === 'string' && p.message.trim() ? p.message : 'TTS error'
      showToast(msg, 'error')
    })
    unsubs.push(u5)

    // Open TTS panel with provided text (from Quick Actions) and optionally autoplay
    const u6 = await listen<{ text: string; autoplay?: boolean }>('tts:open', (e) => {
      const p = e.payload || ({} as any)
      const text = typeof p.text === 'string' ? p.text : ''
      const autoplay = !!p.autoplay
      ui.activeSection = 'TTS'
      requestAnimationFrame(() => {
        const c = ttsRef.value as any
        if (!c) return
        if (autoplay) {
          c.setTextAndPlay(text)
        } else {
          c.setText(text)
          showToast('Text inserted into TTS. Press Play to start.', 'success', 1800)
        }
      })
    })
    unsubs.push(u6)
  } catch (err) {
    console.error('[app] event listen failed', err)
  }
  // Load prompt settings on mount
  try { await loadSettings() } catch {}
  // Apply style-specific CSS after loading settings (only for main window)
  if (!isQuickActions && !isCaptureOverlay) {
    try { applyStyleCss(settings.ui_style) } catch {}
  }
  // Load persisted conversation if enabled
  try { await loadPersistedConversation() } catch {}
})

onBeforeUnmount(() => {
  try { unsubs.forEach(u => u()); } finally { unsubs = [] }
})

async function generateDefaults() {
  try {
    const path = await invoke<string>('generate_default_quick_prompts')
    showToast(`Default quick_prompts.json written:\n${path}`, 'success')
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`Failed to write defaults: ${msg}`, 'error')
  }
}

// ---------------------------
// Prompt Settings state & actions
// ---------------------------
const settings = reactive({
  openai_api_key: '',
  openai_chat_model: 'gpt-4o-mini',
  temperature: 1.0 as number,
  persist_conversations: false as boolean,
  ui_style: 'sidebar' as 'sidebar' | 'tabs',
})
const models = reactive<{ list: string[]; loading: boolean; error: string | null }>({ list: [], loading: false, error: null })
const showApiKey = ref(false)

async function loadSettings() {
  const v = await invoke<any>('get_settings')
  if (v && typeof v === 'object') {
    if (typeof v.openai_api_key === 'string') settings.openai_api_key = v.openai_api_key
    if (typeof v.openai_chat_model === 'string' && v.openai_chat_model.trim()) settings.openai_chat_model = v.openai_chat_model
    if (typeof v.temperature === 'number') settings.temperature = v.temperature
    if (typeof v.persist_conversations === 'boolean') settings.persist_conversations = v.persist_conversations
    if (v.ui_style === 'tabs' || v.ui_style === 'sidebar') settings.ui_style = v.ui_style
  }
}

async function saveSettings() {
  try {
    const path = await invoke<string>('save_settings', { map: { ...settings } })
    showToast(`Settings saved:\n${path}`, 'success')
    // Persist/clear conversations immediately according to toggle for privacy
    try {
      if (settings.persist_conversations) {
        const p = await invoke<string>('save_conversation_state', { state: getPersistState() })
        console.info('[persist] conversation saved to', p)
      } else {
        await invoke<string>('clear_conversations')
        console.info('[persist] conversations cleared')
      }
    } catch (e) {
      console.warn('[persist] post-save action failed', e)
    }
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`Failed to save settings: ${msg}`, 'error')
  }
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

// ---------------------------
// Per-style CSS loader
// ---------------------------
const themeCssLinkId = 'theme-style-css'
const styleCssMap: Record<'sidebar' | 'tabs', string> = {
  sidebar: sidebarStyleUrl,
  tabs: tabsStyleUrl,
}
function ensureThemeLinkEl(): HTMLLinkElement {
  let el = document.getElementById(themeCssLinkId) as HTMLLinkElement | null
  if (!el) {
    el = document.createElement('link')
    el.id = themeCssLinkId
    el.rel = 'stylesheet'
    document.head.appendChild(el)
  }
  return el
}

function applyStyleCss(styleName: string) {
  const el = ensureThemeLinkEl()
  const resolved = styleCssMap[styleName as 'sidebar' | 'tabs']
  if (resolved) {
    el.href = resolved
  } else {
    // Unknown style: remove link to avoid 404s
    try { el.remove() } catch {}
  }
}

watch(() => settings.ui_style, (v) => {
  if (!isQuickActions && !isCaptureOverlay) {
    try { applyStyleCss(v) } catch {}
  }
})
</script>

<template>
  <QuickActions v-if="isQuickActions" />
  <CaptureOverlay v-else-if="isCaptureOverlay" />
  <template v-else>
    <PromptPanel
      v-if="prompt.visible"
      :selection="prompt.selection"
      :preview="prompt.preview"
      :length="prompt.length"
      @close="prompt.visible = false"
    />

    <!-- Sidebar layout (default) -->
    <div v-if="settings.ui_style === 'sidebar'" class="shell">
      <aside class="sidebar" :class="{ collapsed: !layout.sidebarOpen }">
        <button class="burger" title="Toggle menu" @click="layout.sidebarOpen = !layout.sidebarOpen">☰</button>
        <button
          v-for="s in ui.sections"
          :key="s"
          class="side-tab"
          :class="{ active: ui.activeSection === s }"
          @click="ui.activeSection = s"
          :title="s"
        >{{ layout.sidebarOpen ? s : s[0] }}</button>
        <div class="side-spacer"></div>
        <div class="side-status"><LoadingDots v-if="isBusy()" text="Working" /></div>
      </aside>

      <div class="main">
        <div class="main-content">
          <template v-if="ui.activeSection === 'Prompt'">
            <div class="prompt-layout">
              <ConversationHistory />
              <div class="convo-wrap">
                <ConversationView :messages="conversation.currentConversation.messages" />
              </div>
              <PromptComposer v-model="composerInput" @busy="busy.prompt = $event" />
            </div>
          </template>

          <div v-else-if="ui.activeSection === 'TTS'" class="section">
            <div class="section-title">TTS</div>
            <TTSPanel ref="ttsRef" :notify="showToast" @busy="busy.tts = $event" />
          </div>

          <div v-else-if="ui.activeSection === 'STT'" class="section">
            <div class="section-title">STT</div>
            <STTPanel :notify="showToast" @use-as-prompt="handleUseAsPrompt" @busy="busy.stt = $event" />
          </div>

          <div v-else-if="ui.activeSection === 'Settings'" class="section">
            <div class="settings">
              <div class="settings-section">
                <div class="settings-title">Prompt Settings</div>
                <div class="settings-row col">
                  <label class="label">OpenAI API Key</label>
                  <div class="row-inline">
                    <input
                      :type="showApiKey ? 'text' : 'password'"
                      v-model="settings.openai_api_key"
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
                    <select v-model="settings.openai_chat_model" class="input">
                      <option v-if="!models.list.includes(settings.openai_chat_model)" :value="settings.openai_chat_model">{{ settings.openai_chat_model }} (current)</option>
                      <option v-for="m in models.list" :key="m" :value="m">{{ m }}</option>
                    </select>
                    <button class="btn" :disabled="models.loading" @click="refreshModels">{{ models.loading ? 'Fetching…' : 'Fetch Models' }}</button>
                  </div>
                  <div v-if="models.error" class="settings-hint error">{{ models.error }}</div>
                </div>

                <div class="settings-row col">
                  <label class="label">Temperature: {{ settings.temperature.toFixed(2) }}</label>
                  <input type="range" min="0" max="2" step="0.05" v-model.number="settings.temperature" />
                  <div class="settings-hint">Lower = deterministic, Higher = creative. Default 1.0</div>
                </div>

                <div class="settings-row col">
                  <label class="label">UI Style</label>
                  <select v-model="settings.ui_style" class="input">
                    <option value="sidebar">Sidebar (default)</option>
                    <option value="tabs">Top Tabs</option>
                  </select>
                  <div class="settings-hint">Switch between left sidebar and legacy top tabs.</div>
                </div>

                <div class="settings-row">
                  <label class="checkbox"><input type="checkbox" v-model="settings.persist_conversations"/> Persist conversations (OFF by default)</label>
                </div>
                <div class="settings-hint">Privacy-first: history is not saved unless enabled. When enabled, conversation history is saved locally.</div>

                <div class="settings-row">
                  <button class="btn" @click="saveSettings">Save Settings</button>
                  <button class="btn danger" @click="onClearConversations">Clear All Conversations</button>
                </div>
              </div>

              <div class="settings-section">
                <div class="settings-title">Settings (Quick Prompts)</div>
                <div class="settings-row">
                  <button class="btn" @click="generateDefaults">Generate default quick_prompts.json</button>
                </div>
                <div class="settings-hint">Writes defaults to %APPDATA%/AiDesktopCompanion/quick_prompts.json</div>
                <QuickPromptsEditor :notify="showToast" />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>

    <!-- Top-tabs layout (legacy) -->
    <template v-else>
      <div class="nav">
        <button
          v-for="s in ui.sections"
          :key="s"
          class="tab"
          :class="{ active: ui.activeSection === s }"
          @click="ui.activeSection = s"
        >{{ s }}</button>
        <div class="spacer"></div>
        <LoadingDots v-if="isBusy()" text="Working" />
      </div>

      <div class="content">
        <template v-if="ui.activeSection === 'Prompt'">
          <div class="prompt-layout">
            <ConversationHistory />
            <div class="convo-wrap">
              <ConversationView :messages="conversation.currentConversation.messages" />
            </div>
            <PromptComposer v-model="composerInput" @busy="busy.prompt = $event" />
          </div>
        </template>

        <div v-else-if="ui.activeSection === 'TTS'" class="section">
          <div class="section-title">TTS</div>
          <TTSPanel ref="ttsRef" :notify="showToast" @busy="busy.tts = $event" />
        </div>

        <div v-else-if="ui.activeSection === 'STT'" class="section">
          <div class="section-title">STT</div>
          <STTPanel :notify="showToast" @use-as-prompt="handleUseAsPrompt" @busy="busy.stt = $event" />
        </div>

        <div v-else-if="ui.activeSection === 'Settings'" class="section">
          <div class="settings">
            <div class="settings-section">
              <div class="settings-title">Prompt Settings</div>
              <div class="settings-row col">
                <label class="label">OpenAI API Key</label>
                <div class="row-inline">
                  <input
                    :type="showApiKey ? 'text' : 'password'"
                    v-model="settings.openai_api_key"
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
                  <select v-model="settings.openai_chat_model" class="input">
                    <option v-if="!models.list.includes(settings.openai_chat_model)" :value="settings.openai_chat_model">{{ settings.openai_chat_model }} (current)</option>
                    <option v-for="m in models.list" :key="m" :value="m">{{ m }}</option>
                  </select>
                  <button class="btn" :disabled="models.loading" @click="refreshModels">{{ models.loading ? 'Fetching…' : 'Fetch Models' }}</button>
                </div>
                <div v-if="models.error" class="settings-hint error">{{ models.error }}</div>
              </div>

              <div class="settings-row col">
                <label class="label">Temperature: {{ settings.temperature.toFixed(2) }}</label>
                <input type="range" min="0" max="2" step="0.05" v-model.number="settings.temperature" />
                <div class="settings-hint">Lower = deterministic, Higher = creative. Default 1.0</div>
              </div>

              <div class="settings-row col">
                <label class="label">UI Style</label>
                <select v-model="settings.ui_style" class="input">
                  <option value="sidebar">Sidebar (default)</option>
                  <option value="tabs">Top Tabs</option>
                </select>
                <div class="settings-hint">Switch between left sidebar and legacy top tabs.</div>
              </div>

              <div class="settings-row">
                <label class="checkbox"><input type="checkbox" v-model="settings.persist_conversations"/> Persist conversations (OFF by default)</label>
              </div>
              <div class="settings-hint">Privacy-first: history is not saved unless enabled. When enabled, conversation history is saved locally.</div>

              <div class="settings-row">
                <button class="btn" @click="saveSettings">Save Settings</button>
                <button class="btn danger" @click="onClearConversations">Clear All Conversations</button>
              </div>
            </div>

            <div class="settings-section">
              <div class="settings-title">Settings (Quick Prompts)</div>
              <div class="settings-row">
                <button class="btn" @click="generateDefaults">Generate default quick_prompts.json</button>
              </div>
              <div class="settings-hint">Writes defaults to %APPDATA%/AiDesktopCompanion/quick_prompts.json</div>
              <QuickPromptsEditor :notify="showToast" />
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- Toast -->
    <div v-if="toast.visible" class="toast" :class="toast.kind">{{ toast.message }}</div>
  </template>
</template>

<style scoped>
/* Top navigation */
.nav { display: flex; gap: 8px; padding: 10px 0; border-bottom: 1px solid #2c2c36; }
.tab { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #1f1f26; color: #fff; cursor: pointer; }
.tab.active { background: #2e5cff; border-color: #2e5cff; }
.tab:hover { filter: brightness(1.05); }

.spacer { flex: 1; }

.content { padding: 12px 0; }
.section { margin: 0 auto; max-width: 920px; }
.section-title { font-weight: 700; margin-bottom: 8px; font-size: 18px; }
.section-hint { font-size: 12px; color: #9fa0aa; }

.settings { margin: 24px auto; max-width: 720px; color: #e0e0ea; }
.settings-section { border: 1px solid #3a3a44; border-radius: 10px; padding: 14px; background: #1f1f26; }
.settings-title { font-weight: 700; margin-bottom: 8px; }
.settings-row { display: flex; gap: 10px; align-items: center; margin: 8px 0; }
.settings-row.col { flex-direction: column; align-items: flex-start; }
.settings-hint { font-size: 12px; color: #9fa0aa; margin-top: 6px; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.btn:hover { filter: brightness(1.05); }
.btn.ghost { background: transparent; }
.btn.danger { background: #a42828; border-color: #7c1f1f; }
.row-inline { display: flex; gap: 8px; width: 100%; }
.label { font-size: 12px; color: #c8c9d3; }
.input { flex: 1; padding: 8px 10px; border-radius: 8px; border: 1px solid #3a3a44; background: #1f1f26; color: #fff; }
.checkbox { display: flex; gap: 8px; align-items: center; }
.settings-hint.error { color: #f2b8b8; }

.toast { position: fixed; left: 50%; bottom: 24px; transform: translateX(-50%); padding: 10px 14px; border-radius: 8px; border: 1px solid #3a3a44; background: #2a2a31; color: #fff; white-space: pre-line; box-shadow: 0 6px 24px rgba(0,0,0,0.3); }
.toast.success { border-color: #285c2a; background: #1e3b21; }
.toast.error { border-color: #5c2828; background: #3b1e1e; }

/* Sidebar layout */
.shell { display: flex; gap: 0; height: 100vh; text-align: left; }
.sidebar { width: 220px; background: #1b1b22; border-right: 1px solid #2c2c36; padding: 10px 8px; display: flex; flex-direction: column; gap: 6px; transition: width 0.2s ease; }
.sidebar.collapsed { width: 64px; }
.burger { padding: 8px 10px; border-radius: 8px; border: 1px solid #3a3a44; background: #1f1f26; color: #fff; cursor: pointer; }
.side-tab { padding: 10px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #1f1f26; color: #fff; cursor: pointer; text-align: left; }
.side-tab.active { background: #2e5cff; border-color: #2e5cff; }
.side-spacer { flex: 1; }
.side-status { padding-top: 8px; }
.main { flex: 1; min-width: 0; display: flex; flex-direction: column; }
.main-content { flex: 1; min-height: 0; overflow: hidden; padding: 12px 12px; }

/* Prompt layout with scrolling conversation */
.prompt-layout { display: flex; flex-direction: column; gap: 10px; height: 100%; }
.convo-wrap { flex: 1; min-height: 0; }
</style>
