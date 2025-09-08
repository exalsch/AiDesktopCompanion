// Hotkeys initializer for global shortcuts
// Registers cross-platform hotkeys and emits a DOM event when triggered.

import { register, unregisterAll, unregister, isRegistered } from '@tauri-apps/plugin-global-shortcut'
import { invoke } from '@tauri-apps/api/core'

let initialized = false
let currentShortcut: string | null = null

// Normalize UI modifier tokens to plugin format (maps 'Win' -> 'Super')
export function normalizeModifier(mod: string): string {
  const m = (mod || '').trim()
  if (!m) return ''
  if (m.toLowerCase() === 'win') return 'Super'
  return m
}

// Quick check: attempts to register a shortcut temporarily, verifies registration and immediately unregisters it again.
// Returns true if the shortcut can be registered by this app; false otherwise.
export async function checkShortcutAvailable(shortcut: string): Promise<boolean> {
  const s = (shortcut || '').trim().replace(/\bWin\b/gi, 'Super')
  if (!s) return false
  try {
    // If we already have it, consider it available
    const already = await isRegistered(s).catch(() => false)
    if (already) return true
    // Temporary registration check
    await register(s, () => {})
    const ok = await isRegistered(s).catch(() => false)
    await unregister(s).catch(() => {})
    return !!ok
  } catch {
    try { await unregister(s) } catch {}
    return false
  }
}

export async function initGlobalHotkeys(): Promise<void> {
  if (initialized) return
  initialized = true

  console.info('[hotkeys] Initializing global shortcuts…')

  // Load user-configured hotkey from persisted settings (if any)
  try {
    const v: any = await invoke('get_settings')
    const shortcut = (v && typeof v.global_hotkey === 'string' && v.global_hotkey.trim()) ? v.global_hotkey.trim() : ''
    if (shortcut) {
      await applyGlobalHotkey(shortcut)
      return
    }
  } catch (e) {
    console.warn('[hotkeys] get_settings failed, falling back to defaults', e)
  }

  // Fallback legacy behavior: try multiple candidates to avoid conflicts
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
    console.error('[hotkeys] No global hotkeys could be registered. Another app may be using them. Try running as admin or change the hotkey in settings.')
  } else {
    currentShortcut = successes[0] || null
    console.info('[hotkeys] Registered:', successes.join(', '))
  }

  // Clean up on hot reload / window unload during dev
  window.addEventListener('beforeunload', () => {
    unregisterAll().catch(() => {})
  })
}

// Re-register to a specific shortcut at runtime (called after saving settings)
export async function applyGlobalHotkey(shortcut: string | null | undefined): Promise<void> {
  const sRaw = (typeof shortcut === 'string') ? shortcut.trim() : ''
  const s = sRaw.replace(/\bWin\b/gi, 'Super')
  if (!s) {
    // Clear all existing shortcuts
    try { await unregisterAll() } catch {}
    currentShortcut = null
    console.info('[hotkeys] cleared (no global hotkey set)')
    return
  }
  // Fast path: no change
  if (currentShortcut && currentShortcut === s) return

  // Try to register the new shortcut FIRST; only switch over if successful
  try {
    await register(s, (event) => {
      if (event.state === 'Pressed') {
        console.log(`[hotkeys] ${event.shortcut} pressed`)
        window.dispatchEvent(new CustomEvent('ai-desktop:hotkey'))
      }
    })
    const ok = await isRegistered(s).catch(() => false)
    if (!ok) {
      // Clean up attempted registration
      try { await unregister(s) } catch {}
      throw new Error('Shortcut not registered (possibly in use by another app)')
    }
    // Success: remove previous shortcut (if any) and commit
    if (currentShortcut) {
      try { await unregister(currentShortcut) } catch {}
    }
    currentShortcut = s
    console.info(`[hotkeys] active -> ${s}`)
  } catch (err) {
    console.error(`[hotkeys] failed to register configured shortcut "${s}"`, err)
    throw err
  }
}
