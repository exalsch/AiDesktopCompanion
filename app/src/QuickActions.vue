<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { LogicalSize } from '@tauri-apps/api/dpi'
import { invoke } from '@tauri-apps/api/core'
import { startRecording as sttStart, stopRecording as sttStop, isRecording as sttIsRecording, transcodeToWav16kMono } from './stt'

// Debug helper (enable by setting sessionStorage.setItem('qa_debug', '1'))
const isDev = (import.meta as any)?.env?.DEV === true
function dbg(...args: any[]) {
  try {
    if (isDev && sessionStorage.getItem('qa_debug') === '1') console.debug('[QA]', ...args)
  } catch {}
}

// Encapsulated hide logic so we can call it from multiple places
async function hidePopup(reason?: string, force: boolean = false): Promise<void> {
  try {
    dbg('hidePopup()', reason || '')
    lastHideReason.value = reason || ''
    // Hard guard: during preview we do not allow auto-close unless explicitly forced
    if (!force) {
      if (uiMode.value === 'preview' || captureInProgress.value || Date.now() < suppressCloseUntil.value) {
        dbg('hidePopup() suppressed', { uiMode: uiMode.value, captureInProgress: captureInProgress.value, suppress: suppressCloseUntil.value - Date.now() })
        return
      }
    }
    const w = getCurrentWebviewWindow()
    try {
      await w.hide()
      dbg('hidePopup -> hidden')
    } catch (e) {
      console.warn('[quick-actions] hide failed, trying close()', e)
      try {
        await w.close()
        dbg('hidePopup -> closed')
      } catch (e2) {
        console.error('[quick-actions] close failed', e2)
      }
    }
  } catch (err) {
    // Fail loud in dev, but don't crash UI
    console.error('[quick-actions] hide failed', err)
  }
}

async function onClosePreview(): Promise<void> {
  try {
    clearPreviewState()
    await hidePopup('close', true)
  } catch (err) {
    console.error('[quick-actions] close failed', err)
  }
}

const sttRecording = ref(false)
const sttPending = ref(false) // true while requesting mic permission / starting
const rootRef = ref<HTMLElement | null>(null)
const debugOn = ref(false)
const lastHideReason = ref('')
const allowPreviewHotkeys = true

function clearPreviewState(): void {
  try { sessionStorage.removeItem('qa_show_preview') } catch {}
  try { sessionStorage.removeItem('qa_preview_text') } catch {}
  uiMode.value = 'home'
  previewText.value = ''
  previewBusy.value = false
  resetOnFocus.value = true
}

// Settings-driven behavior: show result preview in popup for quick prompts
const showPreviewInPopup = ref(false)
// Local UI mode for this popup: 'home' shows action buttons; 'preview' shows result and copy/insert controls
const uiMode = ref<'home' | 'preview'>('home')
const previewBusy = ref(false)
const previewText = ref('')
// Control whether focus handler resets the UI; when we re-show for preview, we skip one reset
const resetOnFocus = ref(true)
// During preview capture and re-show, ignore blur-triggered auto-close
const suppressCloseUntil = ref(0)
// Explicit state to ignore blur while selection capture is happening
const captureInProgress = ref(false)
// Guard to avoid resetting to home during focus churn right after re-show
const skipResetUntil = ref(0)
let unlistenBlur: null | (() => void) = null
let unlistenFocus: null | (() => void) = null
let blurCloseTimer: number | null = null

async function handleAction(action: 'prompt' | 'tts' | 'stt' | 'image'): Promise<void> {
  dbg('handleAction', action)
  try {
    if (action === 'prompt') {
      // Close first so focus returns to previous app; then capture selection
      await hidePopup()
      // Aggressive copy-restore default ON; safe mode can be added from settings later
      await invoke<string>('prompt_action', { safe_mode: false })
      return
    } else if (action === 'tts') {
      // Simplified flow: capture selection, open main window TTS panel, insert and autoplay
      await hidePopup()
      // Give focus a moment to return to the previous app so Ctrl+C captures correctly
      await new Promise((r) => setTimeout(r, 100))
      await invoke('tts_open_with_selection', { safe_mode: false, autoplay: true })
      return
    } else if (action === 'stt') {
      // Push-to-talk: start recording on demand; do not close popup yet
      if (!sttRecording.value) await startSTT()
      return
    } else if (action === 'image') {
      // Open capture overlay window (transparent, full-screen, always on top)
      const label = 'capture-overlay'
      const base = `${window.location.origin}${window.location.pathname}`
      const url = `${base}?window=capture-overlay`
      let win: WebviewWindow | null = await WebviewWindow.getByLabel(label)

      const createOverlay = () => {
        console.info('[quick-actions] creating capture overlay', url)
        const w = new WebviewWindow(label, {
          url,
          center: true,
          decorations: false,
          transparent: true,
          focus: true,
          alwaysOnTop: true,
        })
        // Do not await these events; just log if they fire
        try { w.once('tauri://created', () => console.info('[quick-actions] overlay created')) } catch {}
        try { w.once('tauri://error', (e) => console.error('[quick-actions] overlay window error', e)) } catch {}
        return w
      }

      if (!win) {
        win = createOverlay()
      }

      // If focusing/showing fails (stale handle), recreate
      let ready = false
      try { if (win) { await win.show(); ready = true } else { ready = false } } catch { ready = false }
      if (!ready) {
        try { win = createOverlay(); await win.show(); ready = true } catch { ready = false }
      }
      try { if (win) await win.setAlwaysOnTop(true) } catch {}
      try { if (win) await win.setFocus() } catch {}

      // Now hide the popup
      await hidePopup()
    }
  } catch (err) {
    console.error('[quick-actions] action error', err)
  } finally {
    // Close the popup immediately per spec, except for STT which keeps the popup open while recording
    // STT popup will close when recording stops
    // no-op here for STT
  }
}

function onKeydown(e: KeyboardEvent): void {
  // Only react to single keys when this window is focused
  const key = e.key.toLowerCase()
  // Toggle debug logging with Ctrl+D
  if (key === 'd' && (e.ctrlKey || e.metaKey)) {
    try {
      const on = sessionStorage.getItem('qa_debug') === '1'
      if (on) sessionStorage.removeItem('qa_debug')
      else sessionStorage.setItem('qa_debug', '1')
      console.log('[QA] debug', on ? 'OFF' : 'ON')
      debugOn.value = !on
    } catch {}
    return
  }
  // In preview mode, handle copy/insert hotkeys
  if (uiMode.value === 'preview' && allowPreviewHotkeys) {
    if (key === 'c' && !previewBusy.value && !e.ctrlKey && !e.metaKey && !e.altKey) { e.preventDefault(); void onCopy(); return }
    if (key === 'v' && !previewBusy.value && !e.ctrlKey && !e.metaKey && !e.altKey) { e.preventDefault(); void onInsert(); return }
  }
  if (['p', 't', 's', 'i'].includes(key)) {
    e.preventDefault()
    if (key === 'p') handleAction('prompt')
    else if (key === 't') handleAction('tts')
    else if (key === 's') void startSTT()
    else if (key === 'i') handleAction('image')
    return
  }
  // Number keys 1–9 trigger quick prompts (future)
  if (key >= '1' && key <= '9') {
    e.preventDefault()
    const index = Number(key)
    dbg('number key pressed', index, { showPreviewInPopup: showPreviewInPopup.value })
    if (showPreviewInPopup.value) {
      // Show preview UI and keep this window visible; backend briefly refocuses previous app to copy selection
      uiMode.value = 'preview'
      previewText.value = ''
      previewBusy.value = true
      // Guard against premature reset if the window re-shows before we finish
      resetOnFocus.value = false
      skipResetUntil.value = Date.now() + 5000
      ;(async () => {
        try {
          // Same-window flow: briefly refocus previous app to copy selection,
          // then compute preview using the captured selection. No hide/show.
          suppressCloseUntil.value = Date.now() + 2500
          captureInProgress.value = true
          try { sessionStorage.setItem('qa_preview_pending', '1') } catch {}
          dbg('invoke focus_prev_then_copy_selection start')
          const selection = await invoke<string>('focus_prev_then_copy_selection', { safe_mode: false })
          dbg('invoke focus_prev_then_copy_selection done')
          dbg('invoke run_quick_prompt_with_selection start', { index })
          const text = await invoke<string>('run_quick_prompt_with_selection', { index, selection })
          dbg('invoke run_quick_prompt_with_selection done')
          previewText.value = text || ''
          try {
            sessionStorage.setItem('qa_preview_text', previewText.value)
            sessionStorage.setItem('qa_show_preview', '1')
            sessionStorage.removeItem('qa_preview_pending')
          } catch {}
        } catch (err) {
          console.error('[quick-actions] quick prompt result failed', err)
          previewText.value = String(err)
          dbg('preview error', err)
          try {
            sessionStorage.setItem('qa_preview_text', previewText.value)
            sessionStorage.setItem('qa_show_preview', '1')
            sessionStorage.removeItem('qa_preview_pending')
          } catch {}
        } finally {
          dbg('preview flow finally', { uiMode: uiMode.value })
          previewBusy.value = false
          captureInProgress.value = false
          // Keep a short guard after finishing
          const now = Date.now()
          if (suppressCloseUntil.value < now + 800) {
            suppressCloseUntil.value = now + 800
          }
        }
      })()
    } else {
      // Close popup immediately per spec, then run quick prompt on backend which inserts result
      void hidePopup('non-preview quick prompt path')
      void invoke('run_quick_prompt', { index, safe_mode: false })
    }
  }
  // Escape closes the popup
  if (key === 'escape') {
    e.preventDefault()
    dbg('ESC pressed')
    if (sttRecording.value) {
      // Cancel recording and close without transcribe
      void cancelSTT()
    } else {
      clearPreviewState()
      void hidePopup('esc', true)
    }
  }
}

function onKeyup(e: KeyboardEvent): void {
  const key = e.key.toLowerCase()
  if (key === 's') {
    e.preventDefault()
    void stopSTTAndTranscribe()
  }
}

function onWindowBlur(): void {
  // Ignore transient blurs during preview capture re-show window
  if (captureInProgress.value) return
  if (Date.now() < suppressCloseUntil.value) return
  // Debounce close to avoid bounce
  if (blurCloseTimer) { clearTimeout(blurCloseTimer); blurCloseTimer = null }
  blurCloseTimer = window.setTimeout(() => {
    dbg('onWindowBlur timer fire', { captureInProgress: captureInProgress.value, suppress: suppressCloseUntil.value - Date.now(), uiMode: uiMode.value })
    if (captureInProgress.value) return
    if (Date.now() < suppressCloseUntil.value) return
    if (uiMode.value === 'preview') return
    if (!sttRecording.value && !sttPending.value) void hidePopup('blur')
  }, 220)
}

function onWindowFocus(): void {
  if (blurCloseTimer) { clearTimeout(blurCloseTimer); blurCloseTimer = null }
  // Refresh the flag from settings so toggles apply immediately
  try {
    invoke<any>('get_settings').then((v) => {
      if (v && typeof v === 'object') {
        const flag = (v as any).show_quick_prompt_result_in_popup
        showPreviewInPopup.value = typeof flag === 'boolean' ? flag : false
      }
    }).catch(() => {})
  } catch {}
  // If preview is active, do not reset and do not toggle resetOnFocus
  if (uiMode.value === 'preview') return
  if (Date.now() < skipResetUntil.value) return
  // Reset UI only for a fresh session (not when we re-show after preview capture)
  if (resetOnFocus.value) {
    uiMode.value = 'home'
    previewText.value = ''
    previewBusy.value = false
  } else {
    // consume the skip once
    resetOnFocus.value = true
  }
}

function onWindowMouseup(): void {
  if (sttRecording.value) void stopSTTAndTranscribe()
}

async function startSTT(): Promise<void> {
  if (sttRecording.value || sttIsRecording() || sttPending.value) return
  try {
    sttPending.value = true
    await sttStart()
    sttRecording.value = true
    console.info('[stt] recording started')
  } catch (err) {
    console.error('[stt] start failed', err)
    // Close popup so user can retry
    await hidePopup()
  }
  finally {
    sttPending.value = false
  }
}

async function stopSTTAndTranscribe(): Promise<void> {
  if (!sttRecording.value) return
  try {
    const res = await sttStop()
    sttRecording.value = false
    if (!res) {
      await hidePopup()
      return
    }
    const { blob, mime } = res
    // Close popup before transcribing
    await hidePopup()
    let text = ''
    try {
      let payloadBytes: Uint8Array
      let payloadMime: string = mime
      try {
        const settings = await invoke<any>('get_settings')
        const engine = String(settings?.stt_engine || 'openai')
        const baseUrl = String(settings?.stt_cloud_base_url || 'https://api.openai.com').trim()
        const isOpenAi = baseUrl.startsWith('https://api.openai.com')
        const shouldTranscode = engine === 'local' || (engine !== 'local' && !isOpenAi)
        if (shouldTranscode) {
          payloadBytes = await transcodeToWav16kMono(blob)
          payloadMime = 'audio/wav'
        } else {
          payloadBytes = new Uint8Array(await blob.arrayBuffer())
          payloadMime = mime
        }
      } catch {
        payloadBytes = new Uint8Array(await blob.arrayBuffer())
        payloadMime = mime
      }
      text = await invoke<string>('stt_transcribe', { audio: Array.from(payloadBytes), mime: payloadMime })
    } catch (err) {
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown STT error'
      console.error('[stt] transcribe failed:', msg, err)
      return
    }
    // Only paste non-empty transcription into the currently focused application.
    if (text && text.trim().length > 0) {
      // Use aggressive copy-restore (safe_mode=false) so the clipboard is restored after paste.
      await invoke('insert_text_into_focused_app', { text, safe_mode: false })
    }
  } finally {
    sttRecording.value = false
  }
}

async function cancelSTT(): Promise<void> {
  try {
    if (sttIsRecording()) await sttStop()
  } catch {}
  sttRecording.value = false
  await hidePopup()
}

onMounted(() => {
  window.addEventListener('keydown', onKeydown)
  window.addEventListener('keyup', onKeyup)
  window.addEventListener('blur', onWindowBlur)
  window.addEventListener('focus', onWindowFocus)
  window.addEventListener('mouseup', onWindowMouseup)

  // Auto-size the popup to fit content (avoid scrollbars)
  try {
    const el = rootRef.value
    if (el) {
      const w = getCurrentWebviewWindow()
      const applySize = () => {
        const rect = el.getBoundingClientRect()
        const width = Math.ceil(rect.width)
        const height = Math.ceil(rect.height)
        try {
          // Avoid unhandled promise rejection when permission core:window:allow-set-size is not granted
          void w.setSize(new LogicalSize(width, height)).catch(() => {})
        } catch {}
      }
      // Initial sizing after mount
      applySize()
      // Observe for dynamic size changes (e.g., recording hint)
      const ro = new ResizeObserver(() => applySize())
      ro.observe(el)
      // Stop observing on unload
      window.addEventListener('beforeunload', () => { try { ro.disconnect() } catch {} })
    }
  } catch {}

  // Restore preview state if we had to reload (defensive against platform-specific window behavior)
  try {
    debugOn.value = sessionStorage.getItem('qa_debug') === '1'
    const showPrev = sessionStorage.getItem('qa_show_preview') === '1'
    const text = sessionStorage.getItem('qa_preview_text') || ''
    if (showPrev) {
      uiMode.value = 'preview'
      previewText.value = text
      previewBusy.value = false
      // Clear flags after restore
      sessionStorage.removeItem('qa_show_preview')
      sessionStorage.removeItem('qa_preview_text')
      // Avoid immediate reset on focus churn after restore
      resetOnFocus.value = false
      skipResetUntil.value = Date.now() + 2000
      suppressCloseUntil.value = Date.now() + 1000
    }
  } catch {}

  // Load minimal settings needed for this window (flag only)
  try {
    invoke<any>('get_settings').then((v) => {
      if (v && typeof v === 'object') {
        const flag = (v as any).show_quick_prompt_result_in_popup
        showPreviewInPopup.value = typeof flag === 'boolean' ? flag : false
      }
    }).catch(() => {})
  } catch {}

  // Also listen to Tauri-specific window events for robust focus/blur behaviors
  try {
    const w = getCurrentWebviewWindow()
    w.listen('tauri://blur', () => {
      // Ignore transient blurs during preview capture re-show window
      if (captureInProgress.value) return
      if (Date.now() < suppressCloseUntil.value) return
      if (blurCloseTimer) { clearTimeout(blurCloseTimer); blurCloseTimer = null }
      blurCloseTimer = window.setTimeout(() => {
        dbg('tauri://blur timer fire', { captureInProgress: captureInProgress.value, suppress: suppressCloseUntil.value - Date.now(), uiMode: uiMode.value })
        if (captureInProgress.value) return
        if (Date.now() < suppressCloseUntil.value) return
        if (uiMode.value === 'preview') return
        if (!sttRecording.value && !sttPending.value) void hidePopup('tauri://blur')
      }, 220)
    }).then((un) => { unlistenBlur = () => { try { un() } catch {} } }).catch(() => {})
    w.listen('tauri://focus', () => {
      // Mirror onWindowFocus logic
      captureInProgress.value = false
      dbg('tauri://focus')
      try {
        invoke<any>('get_settings').then((v) => {
          if (v && typeof v === 'object') {
            const flag = (v as any).show_quick_prompt_result_in_popup
            showPreviewInPopup.value = typeof flag === 'boolean' ? flag : false
          }
        }).catch(() => {})
      } catch {}
      // If preview is active, do not reset and do not toggle resetOnFocus
      if (uiMode.value === 'preview') return
      if (Date.now() < skipResetUntil.value) return
      if (resetOnFocus.value) {
        uiMode.value = 'home'
        previewText.value = ''
        previewBusy.value = false
      } else {
        resetOnFocus.value = true
      }
    }).then((un) => { unlistenFocus = () => { try { un() } catch {} } }).catch(() => {})

    // When the window is hidden via global toggle, mark next show as a fresh session.
    w.listen('tauri://hide', () => {
      if (captureInProgress.value) { dbg('tauri://hide during capture -> skip fresh mark'); return }
      dbg('tauri://hide -> mark fresh session')
      try { sessionStorage.removeItem('qa_show_preview') } catch {}
      try { sessionStorage.removeItem('qa_preview_text') } catch {}
      resetOnFocus.value = true
    }).catch(() => {})
  } catch {}
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('keyup', onKeyup)
  window.removeEventListener('blur', onWindowBlur)
  window.removeEventListener('focus', onWindowFocus)
  window.removeEventListener('mouseup', onWindowMouseup)
  try { if (unlistenBlur) unlistenBlur() } catch {}
  try { if (unlistenFocus) unlistenFocus() } catch {}
})

// Copy preview result to clipboard and close popup
async function onCopy(): Promise<void> {
  if (previewBusy.value) { dbg('onCopy ignored (busy)'); return }
  dbg('onCopy start')
  try {
    const text = previewText.value || ''
    await invoke('copy_text_to_clipboard', { text })
    dbg('onCopy backend done')
  } catch (err) {
    console.error('[quick-actions] copy failed', err)
  } finally {
    clearPreviewState()
    await hidePopup('copy', true)
  }
}

// Insert preview result into previously focused app: hide popup -> wait -> paste via backend
async function onInsert(): Promise<void> {
  if (previewBusy.value) { dbg('onInsert ignored (busy)'); return }
  dbg('onInsert start')
  try {
    const text = previewText.value || ''
    // Hide first to return focus to previous app
    clearPreviewState()
    await hidePopup('insert', true)
    // Brief wait to ensure focus change
    await new Promise((r) => setTimeout(r, 120))
    await invoke('insert_text_into_focused_app', { text, safe_mode: false })
    dbg('onInsert backend done')
  } catch (err) {
    console.error('[quick-actions] insert failed', err)
  }
}
</script>

<template>
  <div class="qa-root" ref="rootRef" role="dialog" aria-label="Quick Actions">
    <template v-if="uiMode === 'home'">
      <div class="qa-row">
        <button class="qa-btn" @click="() => handleAction('prompt')" aria-label="Prompt (P)">
          <span class="letter">P</span>
          <span class="label">Prompt</span>
        </button>
        <button class="qa-btn" @click="() => handleAction('tts')" aria-label="Text to Speech (T)">
          <span class="letter">T</span>
          <span class="label">TTS</span>
        </button>
        <button
          class="qa-btn"
          @mousedown="startSTT"
          @mouseup="stopSTTAndTranscribe"
          aria-label="Speech to Text (S)"
        >
          <span class="letter">S</span>
          <span class="label">STT</span>
        </button>
        <button class="qa-btn" @click="() => handleAction('image')" aria-label="Image (I)">
          <span class="letter">I</span>
          <span class="label">Image</span>
        </button>
      </div>
      <div class="qa-hint">
        <span v-if="sttRecording" class="rec">
          ● Recording... Release S or mouse to transcribe
        </span>
        <span v-else>Press P / T / S / I or 1–9 for quick prompts. Esc to close.</span>
      </div>
    </template>

    <template v-else>
      <div class="qa-result">
        <div class="qa-result-actions">
          <button class="icon-btn" :title="'Close (Esc)'" aria-label="Close (Esc)" @click="onClosePreview">✕</button>
          <template v-if="!previewBusy">
            <button class="icon-btn" :title="'Copy (c)'" aria-label="Copy (c)" @click="onCopy">📋</button>
            <button class="icon-btn" :title="'Insert (v)'" aria-label="Insert (v)" @click="onInsert">⎘</button>
          </template>
        </div>
        <div class="qa-result-body">
          <div v-if="previewBusy" class="qa-hint">Generating…</div>
          <pre v-else class="qa-pre">{{ previewText }}</pre>
        </div>
      </div>
    </template>
  </div>
</template>

<style scoped>
.qa-root {
  height: max-content; /* shrink-wrap */
  display: grid;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 12px;
  background: var(--adc-surface);
  color: var(--adc-fg);
  border: 1px solid var(--adc-border);
  border-radius: 10px;
  user-select: none;
}

.qa-row {
  display: flex;
  gap: 10px;
}

.qa-btn {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 14px;
  border-radius: 8px;
  background: var(--adc-surface);
  color: var(--adc-fg);
  border: 1px solid var(--adc-border);
  cursor: pointer;
  font-size: 14px;
}
.qa-btn:hover { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }

.letter {
  font-weight: 800;
  text-decoration: underline; /* mnemonic underline */
}

.qa-hint {
  font-size: 12px;
  color: var(--adc-fg-muted);
}
.rec { color: var(--adc-danger); font-weight: 600; }

.qa-result { display: flex; flex-direction: column; gap: 8px; max-width: 640px; }
.qa-result-actions { display: flex; gap: 6px; align-self: flex-end; }
.icon-btn { border: 1px solid var(--adc-border); background: var(--adc-surface); color: var(--adc-fg); border-radius: 8px; padding: 6px 8px; cursor: pointer; }
.icon-btn:hover { background: var(--adc-accent); border-color: var(--adc-accent); color: #fff; }
.qa-result-body { max-width: 640px; max-height: 360px; overflow: auto; border: 1px solid var(--adc-border); border-radius: 8px; padding: 8px; background: var(--adc-surface); }
.qa-pre { white-space: pre-wrap; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace; font-size: 12px; margin: 0; }
</style>
