// Popup window management for Quick Actions
// Creates/toggles a frameless always-on-top window.

import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { invoke } from '@tauri-apps/api/core'

let qaWindow: WebviewWindow | null = null

function inTauri(): boolean {
  return typeof (window as any).__TAURI_INTERNALS__ !== 'undefined'
}

export async function ensureQuickActionsWindow(): Promise<WebviewWindow> {
  if (!inTauri()) throw new Error('Not running inside Tauri')

  // If already created and not closed, reuse
  if (qaWindow) {
    try {
      // Accessing a method will throw if closed
      await qaWindow.isVisible()
      return qaWindow
    } catch {
      qaWindow = null
    }
  }

  qaWindow = new WebviewWindow('quick-actions', {
    url: '/?window=quick-actions',
    visible: false,
    width: 380,
    height: 220,
    decorations: false,
    alwaysOnTop: true,
    resizable: false,
    skipTaskbar: true,
    focus: true
  })

  // Best effort: center by default; later we will position near cursor.
  try {
    await qaWindow.center()
  } catch {}

  return qaWindow
}

export async function toggleQuickActionsWindow(): Promise<void> {
  const w = await ensureQuickActionsWindow()
  try {
    const visible = await w.isVisible()
    if (visible) {
      await w.hide()
    } else {
      // Ask backend to place the window near the current cursor before showing
      try { await invoke('position_quick_actions') } catch (e) { console.warn('[popup] position_quick_actions failed', e) }
      await w.show()
      await w.setFocus()
    }
  } catch (err) {
    console.error('[popup] toggle failed', err)
  }
}
