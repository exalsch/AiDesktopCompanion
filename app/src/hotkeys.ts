// Hotkeys initializer for global shortcuts
// Registers cross-platform hotkeys and emits a DOM event when triggered.
//
// Two independent shortcuts are supported:
//   - `global_hotkey`      -> toggles the Quick Actions popup
//   - `select_all_hotkey`  -> enlarges the selection in the focused app (Ctrl+A,
//                             Ctrl+Shift+Home, or nothing - see
//                             `select_all_capture_mode`) and immediately runs
//                             the configured quick prompt on it
//   - `push_to_talk_hotkey` -> hold to unmute the Assistant Mode microphone
// All are registered through the same helpers so re-registration after a
// Windows sleep/resume, availability checks and cleanup behave identically.

import { register, unregisterAll, unregister, isRegistered } from '@tauri-apps/plugin-global-shortcut'
import { invoke } from '@tauri-apps/api/core'

let initialized = false

// DOM events dispatched when a shortcut fires. `main.ts` owns the reactions.
export const HOTKEY_EVENT_POPUP = 'ai-desktop:hotkey'
export const HOTKEY_EVENT_SELECT_ALL = 'ai-desktop:hotkey-select-all'
export const HOTKEY_EVENT_PTT_DOWN = 'ai-desktop:hotkey-ptt-down'
export const HOTKEY_EVENT_PTT_UP = 'ai-desktop:hotkey-ptt-up'

type SlotName = 'popup' | 'selectAll' | 'pushToTalk'

type Slot = {
  /// Currently registered shortcut for this slot, in plugin format.
  current: string | null
  /// DOM event dispatched on the window when the shortcut fires.
  event: string
  /// Dispatched when the key is released. Only push-to-talk needs this; a slot
  /// without one behaves exactly as before and ignores the release entirely.
  releaseEvent?: string
}

const slots: Record<SlotName, Slot> = {
  popup: { current: null, event: HOTKEY_EVENT_POPUP },
  selectAll: { current: null, event: HOTKEY_EVENT_SELECT_ALL },
  pushToTalk: { current: null, event: HOTKEY_EVENT_PTT_DOWN, releaseEvent: HOTKEY_EVENT_PTT_UP },
}

// Normalize UI modifier tokens to plugin format (maps 'Win' -> 'Super')
export function normalizeModifier(mod: string): string {
  const m = (mod || '').trim()
  if (!m) return ''
  if (m.toLowerCase() === 'win') return 'Super'
  return m
}

function toPluginShortcut(shortcut: string | null | undefined): string {
  return (typeof shortcut === 'string' ? shortcut.trim() : '').replace(/\bWin\b/gi, 'Super')
}

// Names the plugin treats as modifiers rather than keys.
const MODIFIER_TOKENS = new Set([
  'control', 'ctrl', 'alt', 'option', 'shift', 'super', 'meta', 'command', 'cmd', 'commandorcontrol',
])

/// True when a shortcut is only modifiers, i.e. still being assembled in the
/// picker. The plugin rejects these outright, so there is nothing to register.
function isIncompleteShortcut(shortcut: string): boolean {
  const parts = shortcut.split('+').map((p) => p.trim()).filter(Boolean)
  if (!parts.length) return true
  return MODIFIER_TOKENS.has(parts[parts.length - 1].toLowerCase())
}

function isOwnedByUs(shortcut: string): boolean {
  return Object.values(slots).some((s) => s.current === shortcut)
}

// Quick check: attempts to register a shortcut temporarily, verifies registration and immediately unregisters it again.
// Returns true if the shortcut can be registered by this app; false otherwise.
export async function checkShortcutAvailable(shortcut: string): Promise<boolean> {
  const s = toPluginShortcut(shortcut)
  if (!s) return false
  try {
    // If we already own this shortcut, it's available
    if (isOwnedByUs(s)) return true
    // If someone else holds it, it's NOT available
    const already = await isRegistered(s).catch(() => false)
    if (already) return false
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

// Register `shortcut` so that pressing it dispatches the slot's DOM event.
async function registerForSlot(slot: Slot, shortcut: string): Promise<void> {
  await register(shortcut, (event) => {
    if (event.state === 'Pressed') {
      console.log(`[hotkeys] ${event.shortcut} pressed`)
      window.dispatchEvent(new CustomEvent(slot.event))
    } else if (event.state === 'Released' && slot.releaseEvent) {
      window.dispatchEvent(new CustomEvent(slot.releaseEvent))
    }
  })
}

export async function initGlobalHotkeys(): Promise<void> {
  if (initialized) return
  initialized = true

  console.info('[hotkeys] Initializing global shortcuts…')

  // Load user-configured hotkeys from persisted settings (if any). If the popup
  // hotkey fails to register (typically because another app already owns it),
  // fall through to the default candidate list instead of leaving the app with
  // NO global hotkey.
  let configuredPopup = ''
  let configuredSelectAll = ''
  let configuredPushToTalk = ''
  try {
    const v: any = await invoke('get_settings')
    configuredPopup = (v && typeof v.global_hotkey === 'string') ? v.global_hotkey.trim() : ''
    configuredSelectAll = (v && typeof v.select_all_hotkey === 'string') ? v.select_all_hotkey.trim() : ''
    configuredPushToTalk = (v && typeof v.push_to_talk_hotkey === 'string') ? v.push_to_talk_hotkey.trim() : ''
  } catch (e) {
    console.warn('[hotkeys] get_settings failed, falling back to defaults', e)
  }

  if (configuredSelectAll) {
    try {
      await applySelectAllHotkey(configuredSelectAll)
    } catch (err) {
      console.warn(`[hotkeys] select-all shortcut "${configuredSelectAll}" failed to register`, err)
    }
  }

  if (configuredPushToTalk) {
    try {
      await applyPushToTalkHotkey(configuredPushToTalk)
    } catch (err) {
      console.warn(`[hotkeys] push-to-talk shortcut "${configuredPushToTalk}" failed to register`, err)
    }
  }

  registerLifecycleHandlers()

  if (configuredPopup) {
    try {
      await applyGlobalHotkey(configuredPopup)
      return
    } catch (err) {
      console.warn(`[hotkeys] configured shortcut "${configuredPopup}" failed to register; trying defaults`, err)
    }
  }

  // Fallback legacy behavior: try multiple candidates to avoid conflicts
  const candidates = [
    'Alt+A',
    'Alt+Shift+A',
    'Ctrl+Alt+A',
    'Command+Shift+A',
    'Command+Shift+G'
  ]

  for (const s of candidates) {
    try {
      await applyGlobalHotkey(s)
      console.info('[hotkeys] Registered:', s)
      return
    } catch (err) {
      console.warn(`[hotkeys] failed to register ${s}`, err)
    }
  }
  console.error('[hotkeys] No global hotkeys could be registered. Another app may be using them. Try running as admin or change the hotkey in settings.')
}

function registerLifecycleHandlers(): void {
  // Clean up on hot reload / window unload during dev
  window.addEventListener('beforeunload', () => {
    unregisterAll().catch(() => {})
  })

  // Re-register hotkeys after Windows sleep/resume (OS can drop global shortcuts)
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
      reRegisterAll().catch(() => {})
    }
  })
  // Also listen to Tauri window focus as a fallback resume detection
  void (async () => {
    try {
      const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow')
      const w = getCurrentWebviewWindow()
      w.listen('tauri://focus', () => {
        reRegisterAll().catch(() => {})
      }).catch(() => {})
    } catch {}
  })()
}

// Re-register any shortcut that was lost (e.g. after Windows sleep/resume).
async function reRegisterAll(): Promise<void> {
  for (const slot of Object.values(slots)) {
    const shortcut = slot.current
    if (!shortcut) continue
    try {
      const still = await isRegistered(shortcut).catch(() => false)
      if (still) continue // still alive, nothing to do
      console.info(`[hotkeys] re-registering lost shortcut: ${shortcut}`)
      await registerForSlot(slot, shortcut)
      console.info(`[hotkeys] re-registered OK: ${shortcut}`)
    } catch (err) {
      console.warn(`[hotkeys] re-register failed for ${shortcut}`, err)
    }
  }
}

// Re-register one slot to a specific shortcut at runtime (called after saving settings).
async function applyForSlot(name: SlotName, shortcut: string | null | undefined): Promise<void> {
  const slot = slots[name]
  const s = toPluginShortcut(shortcut)
  if (!s) {
    if (slot.current) {
      try { await unregister(slot.current) } catch {}
    }
    slot.current = null
    console.info(`[hotkeys] ${name} cleared (no hotkey set)`)
    return
  }
  // Fast path: no change
  if (slot.current === s) return

  // Half-built combination from the picker: keep whatever is bound now and wait
  // for the user to finish rather than reporting a failure they did not cause.
  if (isIncompleteShortcut(s)) {
    console.info(`[hotkeys] ${name} shortcut "${s}" is incomplete, keeping ${slot.current || 'none'}`)
    return
  }

  // The two slots must not fight over the same combination: the OS would hand
  // the shortcut to whichever registered first and the other would look broken.
  for (const [otherName, other] of Object.entries(slots)) {
    if (otherName !== name && other.current === s) {
      throw new Error(`Shortcut ${s} is already used by the ${otherName} hotkey`)
    }
  }

  // Try to register the new shortcut FIRST; only switch over if successful
  try {
    await registerForSlot(slot, s)
    const ok = await isRegistered(s).catch(() => false)
    if (!ok) {
      // Clean up attempted registration
      try { await unregister(s) } catch {}
      throw new Error('Shortcut not registered (possibly in use by another app)')
    }
    // Success: remove previous shortcut (if any) and commit
    if (slot.current && slot.current !== s) {
      try { await unregister(slot.current) } catch {}
    }
    slot.current = s
    console.info(`[hotkeys] ${name} active -> ${s}`)
  } catch (err) {
    console.error(`[hotkeys] failed to register configured ${name} shortcut "${s}"`, err)
    throw err
  }
}

/// Hotkey that toggles the Quick Actions popup.
export async function applyGlobalHotkey(shortcut: string | null | undefined): Promise<void> {
  return applyForSlot('popup', shortcut)
}

/// Hotkey that selects all text in the focused app and runs a quick prompt on it.
export async function applySelectAllHotkey(shortcut: string | null | undefined): Promise<void> {
  return applyForSlot('selectAll', shortcut)
}

export async function applyPushToTalkHotkey(shortcut: string | null | undefined): Promise<void> {
  return applyForSlot('pushToTalk', shortcut)
}
