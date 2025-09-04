<script setup lang="ts">
import QuickActions from './QuickActions.vue'
import PromptPanel from './components/PromptPanel.vue'
import CaptureOverlay from './components/CaptureOverlay.vue'
import ConversationView from './components/ConversationView.vue'
import ConversationHistory from './components/ConversationHistory.vue'
import PromptComposer from './components/PromptComposer.vue'
import TTSPanel from './components/TTSPanel.vue'
import STTPanel from './components/STTPanel.vue'
import LoadingDots from './components/LoadingDots.vue'
import SettingsGeneral from './components/settings/SettingsGeneral.vue'
import SettingsMcpServers from './components/settings/SettingsMcpServers.vue'
import SettingsQuickPrompts from './components/settings/SettingsQuickPrompts.vue'
import conversation, { appendMessage, getPersistState, setPersistState, clearAllConversations, newConversation, updateMessage } from './state/conversation'
import { onMounted, onBeforeUnmount, reactive, ref, watch, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
// Per-style CSS asset URLs (bundler-resolved)
// Add new styles by importing their style.css with ?url and extending styleCssMap below
import sidebarDarkStyleUrl from './styles/sidebar-dark/style.css?url'
import sidebarLightStyleUrl from './styles/sidebar-light/style.css?url'

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
  promptSubview: 'Chat' as 'Chat' | 'History',
  settingsSubview: 'General' as 'General' | 'Quick Prompts' | 'MCP Servers',
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
// Ref to PromptComposer to allow programmatic send
const composerRef = ref<InstanceType<typeof PromptComposer> | null>(null)
// (Quick Prompts editor now encapsulated in SettingsQuickPrompts)

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
  // For QuickActions popup, strip global app padding/min-width via body class
  try { if (isQuickActions) document.body.classList.add('qa-window') } catch {}
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
    // Only in MAIN window; keep this strictly silent (no section switch, no toast)
    if (!isQuickActions && !isCaptureOverlay) {
      const u4 = await listen<{ text: string }>('prompt:insert', (e) => {
        const p = e.payload || ({} as any)
        if (typeof p.text === 'string') {
          composerInput.value = p.text
        }
      })
      unsubs.push(u4)
    }

    // Direct insert + start a fresh conversation (from Quick Actions Prompt)
    if (!isQuickActions && !isCaptureOverlay) {
      const u10 = await listen<{ text: string }>('prompt:new-conversation', (e) => {
        const p = e.payload || ({} as any)
        const text = typeof p.text === 'string' ? p.text : ''
        // Suppress legacy preview panel
        prompt.visible = false
        // Start a fresh conversation context
        newConversation()
        // Go to Prompt section
        setSection('Prompt')
        // Replace composer content
        composerInput.value = text
        // Focus input so the user can review/edit before sending
        requestAnimationFrame(() => {
          try { (composerRef.value as any)?.focus?.() } catch {}
        })
      })
      unsubs.push(u10)
    }

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
  // Apply style-specific CSS after loading settings (all windows)
  try { applyStyleCss(settings.ui_style) } catch {}

  // Load persisted conversation if enabled
  try { await loadPersistedConversation() } catch {}
  // Load Quick Prompts for composer buttons
  try { await loadQuickPrompts() } catch {}
  // MCP events
  try {
    const u7 = await listen<{ serverId: string }>('mcp:connected', (e) => {
      const id = (e?.payload as any)?.serverId
      const s = id ? findServerById(id) : null
      if (s) { s.status = 'connected'; s.error = null; s.connecting = false; showToast(`MCP connected: ${id}`, 'success', 1500) }
    }); unsubs.push(u7)
    const u8 = await listen<{ serverId: string; existed?: boolean }>('mcp:disconnected', (e) => {
      const id = (e?.payload as any)?.serverId
      const s = id ? findServerById(id) : null
      if (s) { s.status = 'disconnected'; s.connecting = false; showToast(`MCP disconnected: ${id}`, 'success', 1500) }
    }); unsubs.push(u8)
    const u9 = await listen<{ serverId: string; message: string }>('mcp:error', (e) => {
      const p: any = e?.payload || {}
      const s = p.serverId ? findServerById(p.serverId) : null
      if (s) s.error = p.message || 'Unknown error'
      showToast(`MCP error: ${p.message || 'Unknown error'}`, 'error')
    }); unsubs.push(u9)

    // Tool call lifecycle events from chat_complete()
    const u11 = await listen<any>('chat:tool-call', (e) => {
      try {
        const p: any = e?.payload || {}
        const id: string = typeof p.id === 'string' ? p.id : ''
        const mid = id ? `tool_${id}` : undefined
        const patch = {
          role: 'tool' as const,
          type: 'tool' as const,
          tool: {
            id,
            function: typeof p.function === 'string' ? p.function : undefined,
            serverId: typeof p.serverId === 'string' ? p.serverId : undefined,
            tool: typeof p.tool === 'string' ? p.tool : undefined,
            args: p.args,
            status: 'started' as const,
          }
        }
        if (mid) {
          const updated = updateMessage(mid, patch as any)
          if (!updated) {
            appendMessage({ id: mid, ...patch })
          }
        } else {
          appendMessage(patch as any)
        }
      } catch (err) {
        console.warn('[chat] tool-call event handling failed', err)
      }
    }); unsubs.push(u11)

    const u12 = await listen<any>('chat:tool-result', (e) => {
      try {
        const p: any = e?.payload || {}
        const id: string = typeof p.id === 'string' ? p.id : ''
        const mid = id ? `tool_${id}` : ''
        const patch = {
          role: 'tool' as const,
          type: 'tool' as const,
          tool: {
            id,
            function: typeof p.function === 'string' ? p.function : undefined,
            serverId: typeof p.serverId === 'string' ? p.serverId : undefined,
            tool: typeof p.tool === 'string' ? p.tool : undefined,
            ok: p.ok === true,
            result: (p.ok === true) ? p.result : undefined,
            error: (p.ok === false && typeof p.error === 'string') ? p.error : (typeof p.error === 'string' ? p.error : undefined),
            status: 'finished' as const,
          }
        }
        const updated = mid ? updateMessage(mid, patch as any) : null
        if (!updated) {
          appendMessage({ id: mid || undefined, ...patch })
        }
        // Notify if tool call was blocked due to being disabled by settings
        if (p && p.ok === false && typeof p.error === 'string' && p.error.toLowerCase().includes('disabled')) {
          const sid = typeof p.serverId === 'string' ? p.serverId : 'server'
          const tname = typeof p.tool === 'string' ? p.tool : 'tool'
          showToast(`MCP tool blocked by settings: ${sid}/${tname}`, 'error', 2500)
        }
      } catch (err) {
        console.warn('[chat] tool-result event handling failed', err)
      }
    }); unsubs.push(u12)
  } catch (e) {
    console.warn('[mcp] event listen failed', e)
  }
  // Attempt auto-connect for MCP servers after settings load
  try {
    await autoConnectServers()
  } catch {}
})

onBeforeUnmount(() => {
  try { unsubs.forEach(u => u()); } finally { unsubs = [] }
  try { document.body.classList.remove('qa-window') } catch {}
})

// Removed legacy Quick Prompts helpers (now handled inside SettingsQuickPrompts)

// Quick Prompts state and helpers (read-only for main UI)
const quickPrompts = reactive<Record<string, string>>({
  '1': '', '2': '', '3': '',
  '4': '', '5': '', '6': '',
  '7': '', '8': '', '9': ''
})

async function loadQuickPrompts() {
  try {
    const data = await invoke<any>('get_quick_prompts')
    for (let i = 1; i <= 9; i++) {
      const k = String(i)
      ;(quickPrompts as any)[k] = (data && typeof (data as any)[k] === 'string') ? (data as any)[k] : ''
    }
  } catch (err) {
    console.warn('[quick-prompts] load failed', err)
  }
}

function insertQuickPrompt(i: number) {
  try {
    const k = String(i)
    const text = (quickPrompts as any)[k] || ''
    if (!text || !text.trim()) return
    const cur = composerInput.value || ''
    // Prepend quick prompt text to the current input (pre-add, not append)
    composerInput.value = cur ? `${text} ${cur}` : text
    // Focus composer for immediate editing
    requestAnimationFrame(() => {
      try { (composerRef.value as any)?.focus?.() } catch {}
    })
  } catch (e) {
    console.warn('[quick-prompts] insert failed', e)
  }
}

// Active quick prompt used as a System message.
// Single-select: clicking an active button toggles it off.
const activeQuickPrompt = ref<number | null>(null)
const selectedSystemPrompt = computed(() => {
  const i = activeQuickPrompt.value
  if (!i) return ''
  const k = String(i)
  const text = (quickPrompts as any)[k] || ''
  return (typeof text === 'string' ? text.trim() : '')
})

function toggleQuickPrompt(i: number) {
  try {
    const k = String(i)
    const text = (quickPrompts as any)[k] || ''
    if (!text || !text.trim()) return
    activeQuickPrompt.value = (activeQuickPrompt.value === i) ? null : i
    // Keep focus on composer for a smooth UX
    requestAnimationFrame(() => {
      try { (composerRef.value as any)?.focus?.() } catch {}
    })
  } catch (e) {
    console.warn('[quick-prompts] toggle failed', e)
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
  hide_tool_calls_in_chat: false as boolean,
  ui_style: 'sidebar-dark' as 'sidebar-dark' | 'sidebar-light',
  // Global MCP auto-connect toggle
  auto_connect: false as boolean,
  // MCP servers persisted configuration
  // Each item persisted as: { id, transport: 'stdio'|'http', command, args: string[], cwd?: string, env?: Record<string,string> }
  mcp_servers: [] as Array<any>,
})
const models = reactive<{ list: string[]; loading: boolean; error: string | null }>({ list: [], loading: false, error: null })

async function loadSettings() {
  const v = await invoke<any>('get_settings')
  if (v && typeof v === 'object') {
    if (typeof v.openai_api_key === 'string') settings.openai_api_key = v.openai_api_key
    if (typeof v.openai_chat_model === 'string' && v.openai_chat_model.trim()) settings.openai_chat_model = v.openai_chat_model
    if (typeof v.temperature === 'number') settings.temperature = v.temperature
    if (typeof v.persist_conversations === 'boolean') settings.persist_conversations = v.persist_conversations
    if (typeof (v as any).hide_tool_calls_in_chat === 'boolean') settings.hide_tool_calls_in_chat = (v as any).hide_tool_calls_in_chat
    {
      let ui: any = (v as any).ui_style
      // Back-compat: map legacy keys to new ones
      if (ui === 'sidebar') ui = 'sidebar-dark'
      if (ui === 'light') ui = 'sidebar-light'
      if (ui === 'tabs') ui = 'sidebar-dark'
      if (ui === 'sidebar-dark' || ui === 'sidebar-light') settings.ui_style = ui
    }
    if (typeof (v as any).auto_connect === 'boolean') settings.auto_connect = (v as any).auto_connect
    // Load MCP servers and derive UI fields
    if (Array.isArray(v.mcp_servers)) {
      settings.mcp_servers = v.mcp_servers.map((s: any) => {
        const envObj = normalizeEnvInput(s?.env)
        const envJsonStr = Object.keys(envObj).length ? JSON.stringify(envObj, null, 0) : '{ "LOG_LEVEL": "info" }'
        return {
          id: String(s.id || ''),
          // Normalize legacy 'sse' to 'http'
          transport: (s.transport === 'http' || s.transport === 'sse') ? 'http' : 'stdio',
          command: String(s.command || ''),
          args: Array.isArray(s.args) ? s.args.filter((x: any) => typeof x === 'string') : [],
          argsText: Array.isArray(s.args) ? s.args.join(' ') : (typeof s.args === 'string' ? s.args : ''),
          cwd: typeof s.cwd === 'string' ? s.cwd : '',
          env: envObj,
          envJson: envJsonStr,
          // Per-server auto connect (persisted)
          auto_connect: s.auto_connect === true,
          // Persisted list of disabled MCP tool names for this server
          disabled_tools: Array.isArray(s.disabled_tools) ? s.disabled_tools.filter((x: any) => typeof x === 'string') : [],
          status: 'disconnected',
          connecting: false,
          error: null as string | null,
          // Tools & calls UI state (not persisted)
          tools: [],
          toolsOpen: false,
          selectedTool: '',
          toolArgsJson: '{}',
          toolArgsError: null as string | null,
          toolResults: [] as Array<any>,
          // Inline validation state
          envError: null as string | null,
        }
      })
    }
  }
}

async function saveSettings() {
  try {
    // Prepare clean MCP servers array for persistence (strip UI-only fields)
    const cleanServers = settings.mcp_servers.map((s: any) => {
      // Robust args/env parsing
      let args: string[] = []
      if (typeof s.argsText === 'string' && s.argsText.trim()) {
        args = parseArgs(s.argsText)
      } else if (Array.isArray(s.args)) {
        args = s.args.filter((x: any) => typeof x === 'string')
      }

      const env = normalizeEnvInput(typeof s.envJson === 'string' ? s.envJson : s.env)
      return {
        id: String(s.id || ''),
        // Persist only 'stdio' or 'http'; map legacy 'sse' to 'http'
        transport: (s.transport === 'http' || s.transport === 'sse') ? 'http' : 'stdio',
        command: String(s.command || ''),
        args,
        cwd: typeof s.cwd === 'string' ? s.cwd : '',
        env,
        disabled_tools: Array.isArray(s.disabled_tools) ? s.disabled_tools.filter((x: any) => typeof x === 'string') : [],
        auto_connect: s.auto_connect === true,
      }
    })

    const mapToSave: any = { ...settings, mcp_servers: cleanServers }
    // Remove UI-only fields that may be present on root settings
    delete mapToSave.mcp_servers[0]?.argsText // harmless if undefined
    delete mapToSave.mcp_servers[0]?.envJson

    const path = await invoke<string>('save_settings', { map: mapToSave })
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

// ---------------------------
// MCP helpers and actions
// ---------------------------
function parseArgs(text: string): string[] {
  // Robust, shell-like argument parsing supporting quotes and escapes.
  if (!text || !text.trim()) return []
  const out: string[] = []
  let cur = ''
  let i = 0
  let quote: '"' | "'" | null = null
  while (i < text.length) {
    const ch = text[i]
    if (quote) {
      if (ch === quote) {
        quote = null
        i++
        continue
      }
      if (quote === '"' && ch === '\\' && i + 1 < text.length) {
        // Basic escapes inside double quotes: \\ \" \n \t
        const n = text[i + 1]
        if (n === '"') { cur += '"'; i += 2; continue }
        if (n === '\\') { cur += '\\'; i += 2; continue }
        if (n === 'n') { cur += '\n'; i += 2; continue }
        if (n === 't') { cur += '\t'; i += 2; continue }
      }
      cur += ch
      i++
      continue
    }
    // not in quotes
    if (ch === '"' || ch === '\'') { quote = ch as any; i++; continue }
    if (ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r') {
      if (cur.length) { out.push(cur); cur = '' }
      i++
      // collapse consecutive whitespace
      while (i < text.length && /\s/.test(text[i])) i++
      continue
    }
    if (ch === '\\' && i + 1 < text.length) {
      const n = text[i + 1]
      if (n === ' ' || n === '"' || n === '\'' || n === '\\') { cur += n; i += 2; continue }
    }
    cur += ch
    i++
  }
  if (cur.length) out.push(cur)
  return out
}

function parseJsonObject(text: string): Record<string, string> {
  if (!text || !text.trim()) return {}
  const v = JSON.parse(text)
  if (v && typeof v === 'object' && !Array.isArray(v)) return v as Record<string, string>
  throw new Error('ENV must be a JSON object of { key: value }')
}

// Parse simple KEY=VALUE pairs from text. Supports separators: newlines, semicolons, commas
function parseKeyValuePairs(text: string): Record<string, string> {
  const out: Record<string, string> = {}
  if (!text || !text.trim()) return out
  // Split on newlines, semicolons, or commas
  const parts = text
    .split(/\r?\n|;|,/)
    .map(s => s.trim())
    .filter(Boolean)
  for (const p of parts) {
    const idx = p.indexOf('=')
    if (idx === -1) continue
    const k = p.slice(0, idx).trim()
    const v = p.slice(idx + 1).trim().replace(/^"|"$/g, '')
    if (k) out[k] = v
  }
  return out
}

// Normalize various env representations (string JSON, KEY=VALUE lines, arrays, or objects)
function normalizeEnvInput(input: any): Record<string, string> {
  try {
    if (typeof input === 'string') {
      const t = input.trim()
      if (!t) return {}
      // Try JSON first
      try {
        const v = JSON.parse(t)
        if (v && typeof v === 'object' && !Array.isArray(v)) {
          // ensure string values
          return Object.fromEntries(Object.entries(v).map(([k, val]) => [k, String(val)]))
        }
        if (Array.isArray(v)) {
          // Accept arrays of pairs or array of "K=V" strings
          const out: Record<string, string> = {}
          for (const item of v) {
            if (Array.isArray(item) && item.length >= 2) {
              out[String(item[0])] = String(item[1])
            } else if (typeof item === 'string' && item.includes('=')) {
              const idx = item.indexOf('=')
              const k = item.slice(0, idx).trim()
              const val = item.slice(idx + 1).trim()
              if (k) out[k] = val
            }
          }
          return out
        }
      } catch {}
      // Fallback to KEY=VALUE parsing
      return parseKeyValuePairs(t)
    }
    if (Array.isArray(input)) {
      const out: Record<string, string> = {}
      for (const item of input) {
        if (Array.isArray(item) && item.length >= 2) {
          out[String(item[0])] = String(item[1])
        } else if (typeof item === 'string' && item.includes('=')) {
          const idx = item.indexOf('=')
          const k = item.slice(0, idx).trim()
          const val = item.slice(idx + 1).trim()
          if (k) out[k] = val
        }
      }
      return out
    }
    if (input && typeof input === 'object') {
      return Object.fromEntries(Object.entries(input).map(([k, val]) => [k, String(val)]))
    }
  } catch {}
  return {}
}

function findServerById(id: string) {
  return settings.mcp_servers.find((s: any) => s.id === id)
}

async function connectServer(s: any) {
  if (!s || !s.id) { showToast('Server name is required', 'error'); return }
  try {
    s.connecting = true; s.error = null
    const args = s.transport === 'stdio'
      ? parseArgs(typeof s.argsText === 'string' ? s.argsText : (Array.isArray(s.args) ? s.args.join(' ') : ''))
      : []
    const env = s.transport === 'stdio'
      ? normalizeEnvInput(typeof s.envJson === 'string' ? s.envJson : s.env)
      : {}
    await invoke<string>('mcp_connect', {
      serverId: s.id,
      command: s.command,
      args,
      cwd: s.transport === 'stdio' ? (s.cwd || null) : null,
      env,
      transport: s.transport
    })
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    s.error = msg
    showToast(`Connect failed: ${msg}`, 'error')
  } finally {
    s.connecting = false
  }
}

async function disconnectServer(s: any) {
  if (!s || !s.id) return
  try {
    await invoke<string>('mcp_disconnect', { serverId: s.id })
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`Disconnect failed: ${msg}`, 'error')
  }
}

async function pingServer(s: any) {
  if (!s || !s.id) return
  try {
    const res = await invoke<string>('mcp_ping', { serverId: s.id })
    showToast(`Ping: ${res}`, 'success', 1500)
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`Ping failed: ${msg}`, 'error')
  }
}

async function listTools(s: any) {
  if (!s || !s.id) return
  try {
    const v = await invoke<any>('mcp_list_tools', { serverId: s.id })
    const tools = Array.isArray(v?.tools) ? v.tools : (Array.isArray(v) ? v : [])
    s.tools = tools
    s.toolsOpen = true
    const count = Array.isArray(tools) ? tools.length : 0
    showToast(`Tools: ${count} loaded.`, 'success', 1500)
    console.log('[mcp] list_tools', v)
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`list_tools failed: ${msg}`, 'error')
  }
}

// ConversationView event handlers
function onListTools(serverId: string) {
  const s = findServerById(serverId)
  if (s) listTools(s)
}

function onToggleTool(payload: { serverId: string; tool: string; enabled: boolean }) {
  const s = findServerById(payload.serverId)
  if (!s || !payload.tool) return
  const name = String(payload.tool)
  s.disabled_tools = Array.isArray(s.disabled_tools) ? s.disabled_tools : []
  const idx = s.disabled_tools.indexOf(name)
  if (payload.enabled) {
    if (idx !== -1) s.disabled_tools.splice(idx, 1)
  } else {
    if (idx === -1) s.disabled_tools.push(name)
  }
  // Persist immediately for privacy-first UX
  try { saveSettings() } catch {}
}

function validateEnvJsonInput(s: any) {
  try {
    const env = normalizeEnvInput(typeof s.envJson === 'string' ? s.envJson : s.env)
    if (typeof s.envJson === 'string' && s.envJson.trim() && Object.keys(env).length === 0) {
      s.envError = 'Could not parse ENV. Use JSON like {"KEY":"VALUE"} or lines KEY=VALUE'
    } else {
      s.envError = null
    }
  } catch (e) {
    s.envError = (e as Error).message
  }
}

async function callTool(s: any) {
  if (!s || !s.id) return
  if (!s.selectedTool) { showToast('Select a tool first', 'error'); return }
  try {
    s.toolArgsError = null
    const args = (typeof s.toolArgsJson === 'string' && s.toolArgsJson.trim()) ? parseJsonObject(s.toolArgsJson) : {}
    const res = await invoke<any>('mcp_call_tool', { serverId: s.id, name: s.selectedTool, args })
    const entry = { tool: s.selectedTool, args, result: res, at: new Date().toISOString() }
    s.toolResults = [entry, ...(Array.isArray(s.toolResults) ? s.toolResults : [])].slice(0, 10)
    showToast('Tool executed.', 'success', 1200)
    console.log('[mcp] call_tool', entry)
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    s.toolArgsError = msg
    showToast(`call_tool failed: ${msg}`, 'error')
  }
}

function selectedToolObj(s: any) {
  try {
    return (Array.isArray(s.tools) ? s.tools : []).find((t: any) => (t.name || t.id) === s.selectedTool)
  } catch { return null }
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

// Attempt auto-connecting MCP servers based on global/per-server flags
// Non-blocking: kick off connects concurrently and add a timeout guard per server
async function autoConnectServers() {
  try {
    const wantGlobal = settings.auto_connect === true
    for (const s of settings.mcp_servers) {
      const want = wantGlobal || s.auto_connect === true
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

// ---------------------------
// Per-style CSS loader
// ---------------------------
const themeCssLinkId = 'theme-style-css'
const styleCssMap: Record<string, string> = {
  'sidebar-dark': sidebarDarkStyleUrl,
  'sidebar-light': sidebarLightStyleUrl,
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
  const resolved = styleCssMap[String(styleName)]
  if (resolved) {
    el.href = resolved
  } else {
    // Unknown style: remove link to avoid 404s
    try { el.remove() } catch {}
  }
}

watch(() => settings.ui_style, (v) => {
  try { applyStyleCss(v) } catch {}
})

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
      <aside class="sidebar" :class="{ collapsed: !layout.sidebarOpen }">
        <button class="burger" title="Toggle menu" @click="layout.sidebarOpen = !layout.sidebarOpen">☰</button>
        <template v-for="s in ui.sections" :key="s">
          <button
            class="side-tab"
            :class="{ active: ui.activeSection === s }"
            @click="setSection(s)"
            :title="s"
          >
            <!-- Icon -->
            <template v-if="s === 'Prompt'">
              <!-- Pen Tool -->
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="m12 19 7-7 3 3-7 7-3-3z"/>
                <path d="m18 13-1.5-7.5L2 2l3.5 14.5L13 18l5-5z"/>
                <path d="m2 2 7.586 7.586"/>
                <circle cx="11" cy="11" r="2"/>
              </svg>
            </template>
            <template v-else-if="s === 'TTS'">
              <!-- Volume 2 -->
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <polygon points="11 5 6 9 2 9 2 15 6 15 11 19 11 5"/>
                <path d="M19.07 4.93a10 10 0 0 1 0 14.14"/>
                <path d="M15.54 8.46a5 5 0 0 1 0 7.07"/>
              </svg>
            </template>
            <template v-else-if="s === 'STT'">
              <!-- Mic -->
              <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/>
                <path d="M19 10v2a7 7 0 0 1-14 0v-2"/>
                <line x1="12" x2="12" y1="19" y2="22"/>
              </svg>
            </template>
            <template v-else>
              <!-- Settings (cog) -->
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
            <!-- Label -->
            <span v-if="layout.sidebarOpen">{{ s }}</span>
          </button>
          <!-- Sublink under Prompt: History -->
          <button
            v-if="s === 'Prompt'"
            class="side-subtab"
            :class="{ active: ui.activeSection === 'Prompt' && ui.promptSubview === 'History' }"
            @click="ui.activeSection = 'Prompt'; ui.promptSubview = 'History'"
            title="Conversation History"
          >
            <!-- History icon -->
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <circle cx="12" cy="12" r="8"/>
              <path d="M12 8v4l3 3"/>
              <path d="M3 12a9 9 0 1 0 9-9"/>
              <polyline points="3 12 3 7 8 7"/>
            </svg>
            <span v-if="layout.sidebarOpen">History</span>
          </button>
          <!-- Sublinks under Settings: submenus -->
          <button
            v-if="s === 'Settings'"
            class="side-subtab"
            :class="{ active: ui.activeSection === 'Settings' && ui.settingsSubview === 'General' }"
            @click="ui.activeSection = 'Settings'; ui.settingsSubview = 'General'"
            title="General Settings"
          >
            <!-- Sliders icon for General -->
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
            <span v-if="layout.sidebarOpen">General</span>
          </button>
          <button
            v-if="s === 'Settings'"
            class="side-subtab"
            :class="{ active: ui.activeSection === 'Settings' && ui.settingsSubview === 'Quick Prompts' }"
            @click="ui.activeSection = 'Settings'; ui.settingsSubview = 'Quick Prompts'"
            title="Quick Prompts"
          >
            <!-- Zap icon for Quick Prompts -->
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>
            </svg>
            <span v-if="layout.sidebarOpen">Quick Prompts</span>
          </button>
          <button
            v-if="s === 'Settings'"
            class="side-subtab"
            :class="{ active: ui.activeSection === 'Settings' && ui.settingsSubview === 'MCP Servers' }"
            @click="ui.activeSection = 'Settings'; ui.settingsSubview = 'MCP Servers'"
            title="MCP Servers"
          >
            <!-- Server icon for MCP Servers -->
            <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <rect x="2" y="2" width="20" height="8" rx="2"/>
              <rect x="2" y="14" width="20" height="8" rx="2"/>
              <line x1="6" y1="6" x2="6.01" y2="6"/>
              <line x1="6" y1="18" x2="6.01" y2="18"/>
            </svg>
            <span v-if="layout.sidebarOpen">MCP Servers</span>
          </button>
        </template>
        <div class="side-spacer"></div>
        <div class="side-status"><LoadingDots v-if="isBusy()" text="Working" /></div>
      </aside>

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
              <div class="prompt-layout">
                <div class="main-content">
                  <ConversationView
                    :messages="conversation.currentConversation.messages"
                    :hide-tool-details="settings.hide_tool_calls_in_chat"
                    :mcp-servers="settings.mcp_servers"
                    @list-tools="onListTools"
                    @toggle-tool="onToggleTool"
                  />
                </div>
                <div class="quick-prompt-bar">
                  <button
                    v-for="i in 9"
                    :key="i"
                    :class="['qp-btn', { active: activeQuickPrompt === i }]"
                    :disabled="!quickPrompts[String(i)]"
                    :title="quickPrompts[String(i)] || 'Empty'"
                    @click="toggleQuickPrompt(i)"
                  >{{ i }}</button>
                </div>
                <PromptComposer ref="composerRef" v-model="composerInput" :systemPromptText="selectedSystemPrompt" @busy="busy.prompt = $event" />
              </div>
            </template>
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
            <div class="section-title">Settings</div>
              <!-- Settings subview: General -->
              <SettingsGeneral
                v-if="ui.settingsSubview === 'General'"
                :settings="settings"
                :models="models"
                :onSave="saveSettings"
                :onRefreshModels="refreshModels"
                :onClearConversations="onClearConversations"
              />

              <!-- Settings subview: Quick Prompts -->
              <SettingsQuickPrompts v-else-if="ui.settingsSubview === 'Quick Prompts'" :notify="showToast" />

              <!-- Settings subview: MCP Servers -->
              <SettingsMcpServers
                v-else
                :settings="settings"
                :onAdd="addMcpServer"
                :onRemove="removeMcpServer"
                :onSave="saveSettings"
                :onConnect="connectServer"
                :onDisconnect="disconnectServer"
                :onPing="pingServer"
                :onListTools="listTools"
                :onFillArgsTemplate="fillArgsTemplate"
                :onValidateEnvJsonInput="validateEnvJsonInput"
                :onCallTool="callTool"
                :selectedToolObj="selectedToolObj"
              />
            </div>
          </div>
        </div>
      </div>
    </div>
    
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
.settings-row { display: flex; gap: 10px; align-items: center; margin: 8px 0; }
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
.settings :deep(.settings-section) { border: 1px solid var(--adc-border); border-radius: 10px; background: var(--adc-surface); }
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
.prompt-layout { display: flex; flex-direction: column; gap: 5px; height: 95%; }
.convo-wrap { flex: 1; min-height: 0; }

/* Quick Prompt buttons above composer */
.quick-prompt-bar { display: flex; gap: 3px; align-items: center; flex-wrap: wrap; padding: 0 12px; }
.qp-btn {
  padding: 4px 8px;
  border-radius: 8px;
  border: 1px solid var(--adc-border);
  background: var(--adc-surface);
  color: var(--adc-fg);
  cursor: pointer;
  font-size: 12px;
  min-width: 28px;
}
.qp-btn:hover:not(:disabled) { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qp-btn.active { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qp-btn:disabled { opacity: 0.5; cursor: not-allowed; }
</style>

<!-- Global overrides for QuickActions window only -->
<style>
body.qa-window #app {
  max-width: none;
  min-width: 0;
  padding: 0;
}
</style>
