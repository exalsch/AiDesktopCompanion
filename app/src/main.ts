import { createApp } from 'vue'
import './style.css'
import App from './App.vue'
import { initGlobalHotkeys } from './hotkeys'
import { toggleQuickActionsWindow } from './popup'

const app = createApp(App)
app.mount('#app')

// Initialize global hotkeys (non-blocking)
initGlobalHotkeys().catch((err) => console.error('[hotkeys] init failed', err))

// Bind hotkey event to toggle the Quick Actions popup
window.addEventListener('ai-desktop:hotkey', () => {
  toggleQuickActionsWindow().catch((err) => console.error('[popup] toggle failed', err))
})
