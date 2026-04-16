import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { initGlobalHotkeys } from './hotkeys'
import { toggleQuickActionsWindow } from './popup'

// Sub-window class detection (moved from inline scripts to comply with CSP)
const winSearch = window.location.search
if (winSearch.includes('window=')) document.documentElement.classList.add('sub-window')
if (winSearch.includes('window=quick-actions')) document.body.classList.add('qa-window')
if (winSearch.includes('window=capture-overlay')) document.body.classList.add('overlay-window')

const app = createApp(App)
app.mount('#app')

// Only initialize hotkeys and popup toggle in the main window (not in QuickActions or CaptureOverlay)
const winParam = new URLSearchParams(window.location.search).get('window')
if (!winParam) {
  initGlobalHotkeys().catch((err) => console.error('[hotkeys] init failed', err))

  window.addEventListener('ai-desktop:hotkey', () => {
    toggleQuickActionsWindow().catch((err) => console.error('[popup] toggle failed', err))
  })
}
