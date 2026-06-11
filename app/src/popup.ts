// Popup window management for Quick Actions
// The window is statically defined in tauri.conf.json and created at app startup.
// We only toggle visibility — never create/destroy.

import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { invoke } from '@tauri-apps/api/core'
import { unregister } from '@tauri-apps/plugin-global-shortcut'

async function getQaWindow(): Promise<WebviewWindow | null> {
  try {
    return await WebviewWindow.getByLabel('quick-actions')
  } catch {
    return null
  }
}

let toggling = false

export async function toggleQuickActionsWindow(): Promise<void> {
  if (toggling) return
  toggling = true
  try {
    const w = await getQaWindow()
    if (!w) { console.error('[popup] quick-actions window not found'); return }
    try {
      const visible = await w.isVisible()
      if (visible) {
        // Force-unregister all QA keys before hiding to prevent stuck keys
        const qaKeys = ['P', 'T', 'I', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'CommandOrControl+L']
        for (const k of qaKeys) { try { await unregister(k) } catch {} }
        // Disable S-key suppression via Win32 hook (deterministic)
        try { await invoke('suppress_s_key', { enable: false }) } catch {}
        await w.hide()
      } else {
        // Also cleanup stale keys before showing (catches stuck keys from previous session)
        const qaKeys = ['P', 'T', 'I', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'CommandOrControl+L']
        for (const k of qaKeys) { try { await unregister(k) } catch {} }
        // Ensure S-key hook is off on fresh show
        try { await invoke('suppress_s_key', { enable: false }) } catch {}
        try { await invoke('prepare_quick_actions') } catch (e) { console.warn('[popup] prepare_quick_actions failed', e) }
        try { await invoke('position_quick_actions') } catch (e) { console.warn('[popup] position_quick_actions failed', e) }
        await w.show()
        await w.setFocus()
      }
    } catch (err) {
      console.error('[popup] toggle failed', err)
    }
  } finally {
    toggling = false
  }
}
