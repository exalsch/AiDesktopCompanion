// Hotkeys initializer for global shortcuts
// Registers cross-platform hotkeys and emits a DOM event when triggered.

import { register, unregisterAll, isRegistered } from '@tauri-apps/plugin-global-shortcut'

let initialized = false

export async function initGlobalHotkeys(): Promise<void> {
  if (initialized) return
  initialized = true

  console.info('[hotkeys] Initializing global shortcuts…')

  // Our target shortcuts (try multiple to avoid conflicts):
  // - Windows first candidates
  // - macOS candidates included; harmless on Windows (registration will just fail)
  const shortcuts = [
    'Alt+A',
    'Alt+Shift+A',
    'Ctrl+Alt+A',
    'Command+Shift+A',
    'Command+Shift+G'
  ]

  const successes: string[] = []
  for (const s of shortcuts) {
    try {
      await register(s, (event) => {
        if (event.state === 'Pressed') {
          console.log(`[hotkeys] ${event.shortcut} pressed`)
          window.dispatchEvent(new CustomEvent('ai-desktop:hotkey'))
        }
      })
      const ok = await isRegistered(s).catch(() => false)
      console.info(`[hotkeys] tried ${s} -> ${ok ? 'OK' : 'NO'}`)
      if (ok) successes.push(s)
    } catch (err) {
      console.warn(`[hotkeys] failed to register ${s}`, err)
    }
  }
  if (successes.length === 0) {
    console.error('[hotkeys] No global hotkeys could be registered. Another app may be using them. Try running as admin or change the hotkey in settings (todo).')
  } else {
    console.info('[hotkeys] Registered:', successes.join(', '))
  }

  // Clean up on hot reload / window unload during dev
  window.addEventListener('beforeunload', () => {
    unregisterAll().catch(() => {})
  })
}
