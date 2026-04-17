// Popup window management for Quick Actions
// The window is statically defined in tauri.conf.json and created at app startup.
// We only toggle visibility — never create/destroy.

import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { invoke } from '@tauri-apps/api/core'

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
        await w.hide()
      } else {
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
