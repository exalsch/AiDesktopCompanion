import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { initGlobalHotkeys } from './hotkeys'
import { toggleQuickActionsWindow } from './popup'

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
