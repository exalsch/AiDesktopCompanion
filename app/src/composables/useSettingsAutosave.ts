import { watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { applyGlobalHotkey, applySelectAllHotkey, applyPushToTalkHotkey, checkShortcutAvailable } from '../hotkeys'
import { serializeMcpServer } from './utils'
import { getPersistState } from '../state/conversation'

// MCP runtime-only fields that should NOT trigger autosave
const MCP_RUNTIME_KEYS = new Set([
  'status', 'connecting', 'error', 'tools', 'toolsOpen',
  'selectedTool', 'toolArgsJson', 'toolArgsError', 'toolResults', 'envError',
  'headersShown', 'connectedMode',
])

/** Return a JSON-serializable snapshot of only the saveable MCP server config */
function serializableMcpServers(servers: any[]): any[] {
  return (servers || []).map((s: any) => {
    const out: any = {}
    for (const key of Object.keys(s)) {
      if (!MCP_RUNTIME_KEYS.has(key)) out[key] = s[key]
    }
    return out
  })
}

export function useSettingsAutosave(settings: any, showToast: (msg: string, kind?: 'error'|'success', ms?: number) => void) {
  let loaded = false
  let timer: any = 0

  function setLoaded(v: boolean = true) { loaded = !!v }

  function schedule() {
    if (!loaded) return
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => { save().catch(err => console.warn('[settings] auto-save failed', err)) }, 600)
  }

  // Watch only user-facing config — returns a serialized snapshot so Vue
  // compares by value.  MCP runtime fields (status, tools, etc.) are excluded,
  // preventing unnecessary save cycles during MCP operations.
  watch(() => {
    const { mcp_servers, ...rest } = settings
    return JSON.stringify({ ...rest, mcp_servers: serializableMcpServers(mcp_servers) })
  }, schedule)

  async function save() {
    try {
      // Validate hotkey before saving
      if (settings.global_hotkey && settings.global_hotkey.trim()) {
        const ok = await checkShortcutAvailable(settings.global_hotkey)
        if (!ok) {
          showToast('Global hotkey is unavailable or already in use. Please choose another.', 'error')
          return
        }
      }
      if (settings.select_all_hotkey && settings.select_all_hotkey.trim()) {
        const ok = await checkShortcutAvailable(settings.select_all_hotkey)
        if (!ok) {
          showToast('Select-all hotkey is unavailable or already in use. Please choose another.', 'error')
          return
        }
      }

      // Clean MCP servers
      const cleanServers = (settings.mcp_servers || []).map(serializeMcpServer)
      const mapToSave: any = { ...settings, mcp_servers: cleanServers }

      await invoke<string>('save_settings', { map: mapToSave })

      // Re-apply hotkeys silently
      try { await applyGlobalHotkey(settings.global_hotkey) } catch {}
      try { await applySelectAllHotkey(settings.select_all_hotkey) } catch {}
      try { await applyPushToTalkHotkey(settings.push_to_talk_hotkey) } catch {}

      // Persist/clear conversation state based on toggle
      try {
        if (settings.persist_conversations) {
          await invoke<string>('save_conversation_state', { state: getPersistState() })
        } else {
          await invoke<string>('clear_conversations')
        }
      } catch (e) { console.warn('[persist] post-auto-save failed', e) }
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
      showToast(`Failed to auto-save settings: ${msg}`, 'error')
    }
  }

  /** Flush any pending debounced save immediately */
  function flush() {
    if (timer) {
      clearTimeout(timer)
      timer = 0
      save().catch(err => console.warn('[settings] auto-save flush failed', err))
    }
  }

  /** Cancel pending save without executing */
  function cancel() {
    if (timer) { clearTimeout(timer); timer = 0 }
  }

  return { setLoaded, flush, cancel }
}
