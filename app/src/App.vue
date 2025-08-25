<script setup lang="ts">
import QuickActions from './QuickActions.vue'
import PromptPanel from './components/PromptPanel.vue'
import QuickPromptsEditor from './components/QuickPromptsEditor.vue'
import CaptureOverlay from './components/CaptureOverlay.vue'
import ConversationView from './components/ConversationView.vue'
import conversation, { appendMessage } from './state/conversation'
import { onMounted, onBeforeUnmount, reactive } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'

const winParam = new URLSearchParams(window.location.search).get('window')
const isQuickActions = winParam === 'quick-actions'
const isCaptureOverlay = winParam === 'capture-overlay'

// Reactive state for Prompt flow in the main window
const prompt = reactive({
  visible: false,
  selection: '',
  preview: '',
  length: 0,
})

// Simple section navigation for Main Window
const ui = reactive({
  sections: ['Prompt', 'TTS', 'STT', 'Settings'] as const,
  activeSection: 'Prompt' as 'Prompt' | 'TTS' | 'STT' | 'Settings',
})

// Simple toast state
const toast = reactive({
  visible: false,
  message: '',
  kind: 'error' as 'error' | 'success',
  hideTimer: 0 as any
})

function showToast(message: string, kind: 'error' | 'success' = 'error', ms = 3500) {
  toast.message = message
  toast.kind = kind
  toast.visible = true
  if (toast.hideTimer) clearTimeout(toast.hideTimer)
  toast.hideTimer = setTimeout(() => { toast.visible = false }, ms)
}

let unsubs: Array<() => void> = []

onMounted(async () => {
  try {
    const u1 = await listen<{ selection: string; preview: string; length: number }>('prompt:open', (e) => {
      const p = e.payload || ({} as any)
      prompt.selection = typeof p.selection === 'string' ? p.selection : ''
      prompt.preview = typeof p.preview === 'string' ? p.preview : prompt.selection.slice(0, 200)
      prompt.length = typeof p.length === 'number' ? p.length : prompt.preview.length
      prompt.visible = true
    })
    unsubs.push(u1)

    const u2 = await listen<{ message: string; path?: string }>('settings:quick-prompts-error', (e) => {
      const p = e.payload || ({} as any)
      const where = p.path ? `\n${p.path}` : ''
      showToast(`Quick Prompts config error: ${p.message}${where}`, 'error')
    })
    unsubs.push(u2)

    const u3 = await listen<{ path: string }>('image:capture', async (e) => {
      const p = e.payload || ({} as any)
      if (p.path) {
        showToast(`Image captured:\n${p.path}`, 'success')
        // Try to close overlay window if it somehow remained
        try {
          const ow = WebviewWindow.getByLabel('capture-overlay')
          if (ow) await ow.close()
        } catch {}
        // Append image message to conversation and switch to Prompt section
        try {
          const src = convertFileSrc(p.path)
          appendMessage({ role: 'user', type: 'image', images: [{ path: p.path, src }] })
          ui.activeSection = 'Prompt'
        } catch {}
      }
    })
    unsubs.push(u3)
  } catch (err) {
    console.error('[app] event listen failed', err)
  }
})

onBeforeUnmount(() => {
  try { unsubs.forEach(u => u()); } finally { unsubs = [] }
})

async function generateDefaults() {
  try {
    const path = await invoke<string>('generate_default_quick_prompts')
    showToast(`Default quick_prompts.json written:\n${path}`, 'success')
  } catch (err) {
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown error'
    showToast(`Failed to write defaults: ${msg}`, 'error')
  }
}
</script>

<template>
  <QuickActions v-if="isQuickActions" />
  <CaptureOverlay v-else-if="isCaptureOverlay" />
  <template v-else>
    <PromptPanel
      v-if="prompt.visible"
      :selection="prompt.selection"
      :preview="prompt.preview"
      :length="prompt.length"
      @close="prompt.visible = false"
    />

    <!-- Top Navigation -->
    <div class="nav">
      <button
        v-for="s in ui.sections"
        :key="s"
        class="tab"
        :class="{ active: ui.activeSection === s }"
        @click="ui.activeSection = s"
      >{{ s }}</button>
    </div>

    <!-- Section Content -->
    <div class="content">
      <ConversationView
        v-if="ui.activeSection === 'Prompt'"
        :messages="conversation.currentConversation.messages"
      />

      <div v-else-if="ui.activeSection === 'TTS'" class="section">
        <div class="section-title">TTS</div>
        <div class="section-hint">Play/stop, speed/voice selector, download/save as option. ‼️ TODO</div>
      </div>

      <div v-else-if="ui.activeSection === 'STT'" class="section">
        <div class="section-title">STT</div>
        <div class="section-hint">Transcript view, copy/use-as-prompt. ‼️ TODO</div>
      </div>

      <div v-else-if="ui.activeSection === 'Settings'" class="section">
        <div class="settings">
          <div class="settings-section">
            <div class="settings-title">Settings (Quick Prompts)</div>
            <div class="settings-row">
              <button class="btn" @click="generateDefaults">Generate default quick_prompts.json</button>
            </div>
            <div class="settings-hint">Writes defaults to %APPDATA%/AiDesktopCompanion/quick_prompts.json</div>
            <QuickPromptsEditor :notify="showToast" />
          </div>
        </div>
      </div>
    </div>

    <!-- Toast -->
    <div v-if="toast.visible" class="toast" :class="toast.kind">{{ toast.message }}</div>
  </template>
</template>

<style scoped>
/* Top navigation */
.nav { display: flex; gap: 8px; padding: 10px 0; border-bottom: 1px solid #2c2c36; }
.tab { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #1f1f26; color: #fff; cursor: pointer; }
.tab.active { background: #2e5cff; border-color: #2e5cff; }
.tab:hover { filter: brightness(1.05); }

.content { padding: 12px 0; }
.section { margin: 0 auto; max-width: 920px; }
.section-title { font-weight: 700; margin-bottom: 8px; font-size: 18px; }
.section-hint { font-size: 12px; color: #9fa0aa; }

.settings { margin: 24px auto; max-width: 720px; color: #e0e0ea; }
.settings-section { border: 1px solid #3a3a44; border-radius: 10px; padding: 14px; background: #1f1f26; }
.settings-title { font-weight: 700; margin-bottom: 8px; }
.settings-row { display: flex; gap: 10px; align-items: center; }
.settings-hint { font-size: 12px; color: #9fa0aa; margin-top: 6px; }
.btn { padding: 8px 12px; border-radius: 8px; border: 1px solid #3a3a44; background: #2e5cff; color: #fff; cursor: pointer; }
.btn:hover { filter: brightness(1.05); }

.toast { position: fixed; left: 50%; bottom: 24px; transform: translateX(-50%); padding: 10px 14px; border-radius: 8px; border: 1px solid #3a3a44; background: #2a2a31; color: #fff; white-space: pre-line; box-shadow: 0 6px 24px rgba(0,0,0,0.3); }
.toast.success { border-color: #285c2a; background: #1e3b21; }
.toast.error { border-color: #5c2828; background: #3b1e1e; }
</style>
