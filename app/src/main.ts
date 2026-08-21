import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { initGlobalHotkeys, HOTKEY_EVENT_POPUP, HOTKEY_EVENT_SELECT_ALL } from './hotkeys'
import { toggleQuickActionsWindow } from './popup'
import { invoke } from '@tauri-apps/api/core'

// Sub-window class detection (moved from inline scripts to comply with CSP)
const winSearch = window.location.search
if (winSearch.includes('window=')) document.documentElement.classList.add('sub-window')
if (winSearch.includes('window=quick-actions')) document.body.classList.add('qa-window')
if (winSearch.includes('window=capture-overlay')) document.body.classList.add('overlay-window')
if (winSearch.includes('window=assistant-pill')) {
  // Same treatment as the busy pill: a transparent, chromeless overlay window.
  document.documentElement.classList.add('busy-window')
  document.body.classList.add('busy-window')
}
if (winSearch.includes('window=busy')) {
  document.documentElement.classList.add('busy-window')
  document.body.classList.add('busy-window')
}

const app = createApp(App)
app.mount('#app')

// Only initialize hotkeys and popup toggle in the main window (not in QuickActions or CaptureOverlay)
const winParam = new URLSearchParams(window.location.search).get('window')
if (!winParam) {
  initGlobalHotkeys().catch((err) => console.error('[hotkeys] init failed', err))

  window.addEventListener(HOTKEY_EVENT_POPUP, () => {
    toggleQuickActionsWindow().catch((err) => console.error('[popup] toggle failed', err))
  })

  // Select-all hotkey: no popup at all — enlarge the selection in the focused
  // app and run the configured quick prompt straight over it. How much gets
  // selected is read backend-side from `select_all_capture_mode`. The backend
  // reports progress and errors through the busy indicator window.
  window.addEventListener(HOTKEY_EVENT_SELECT_ALL, () => {
    void (async () => {
      try {
        const v: any = await invoke('get_settings')
        const raw = Number(v?.select_all_quick_prompt)
        const index = Number.isFinite(raw) ? Math.min(9, Math.max(1, Math.trunc(raw))) : 1
        await invoke('run_quick_prompt_select_all', { index, safe_mode: false })
      } catch (err) {
        console.error('[hotkeys] select-all quick prompt failed', err)
      }
    })()
  })
}
