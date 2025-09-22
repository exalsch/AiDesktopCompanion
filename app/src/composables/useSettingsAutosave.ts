import { watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { applyGlobalHotkey, checkShortcutAvailable } from '../hotkeys'
import { parseArgs, normalizeEnvInput } from './utils'
import { getPersistState } from '../state/conversation'

export function useSettingsAutosave(settings: any, showToast: (msg: string, kind?: 'error'|'success', ms?: number) => void) {
  let loaded = false
  let timer: any = 0

  function setLoaded(v: boolean = true) { loaded = !!v }

  function schedule() {
    if (!loaded) return
    if (timer) clearTimeout(timer)
    timer = setTimeout(() => { save().catch(err => console.warn('[settings] auto-save failed', err)) }, 600)
  }

  watch(settings, schedule, { deep: true })

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

      // Clean MCP servers
      const cleanServers = (settings.mcp_servers || []).map((s: any) => {
        let args: string[] = []
        if (typeof s.argsText === 'string' && s.argsText.trim()) {
          args = parseArgs(s.argsText)
        } else if (Array.isArray(s.args)) {
          args = s.args.filter((x: any) => typeof x === 'string')
        }
        const env = normalizeEnvInput(typeof s.envJson === 'string' ? s.envJson : s.env)
        return {
          id: String(s.id || ''),
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
      // Remove UI-only helper fields if present
      if (Array.isArray(mapToSave.mcp_servers)) {
        for (const srv of mapToSave.mcp_servers) { delete srv.argsText; delete srv.envJson }
      }

      await invoke<string>('save_settings', { map: mapToSave })

      // Re-apply hotkey silently
      try { await applyGlobalHotkey(settings.global_hotkey) } catch {}

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

  return { setLoaded }
}
