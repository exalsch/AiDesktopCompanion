import { invoke } from '@tauri-apps/api/core'
import { applyGlobalHotkey, checkShortcutAvailable } from '../hotkeys'
import { parseArgs, normalizeEnvInput } from './utils'
import { getPersistState } from '../state/conversation'

// Manual Settings Save with success toast
export function useSettingsSave(settings: any, showToast: (msg: string, kind?: 'error'|'success', ms?: number) => void) {
  async function saveSettingsNow() {
    try {
      // Validate global hotkey before saving & applying
      if (settings.global_hotkey && settings.global_hotkey.trim()) {
        const ok = await checkShortcutAvailable(settings.global_hotkey)
        if (!ok) {
          showToast('Global hotkey is unavailable or already in use. Please choose another.', 'error')
          return
        }
      }

      // Prepare clean MCP servers array for persistence (strip UI-only fields)
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

      const path = await invoke<string>('save_settings', { map: mapToSave })
      showToast(`Settings saved:\n${path}`, 'success')

      // Re-apply global hotkey immediately when changed
      try { await applyGlobalHotkey(settings.global_hotkey) } catch {}

      // Persist/clear conversations immediately according to toggle for privacy
      try {
        if (settings.persist_conversations) {
          await invoke<string>('save_conversation_state', { state: getPersistState() })
        } else {
          await invoke<string>('clear_conversations')
        }
      } catch (e) {
        console.warn('[persist] post-save action failed', e)
      }
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : err?.message ? err.message : 'Unknown error'
      showToast(`Failed to save settings: ${msg}`, 'error')
    }
  }

  return { saveSettingsNow }
}
