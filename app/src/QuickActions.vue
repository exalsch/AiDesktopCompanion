<script setup lang="ts">
import { onMounted, onBeforeUnmount, ref } from 'vue'
import { getCurrentWebviewWindow, WebviewWindow } from '@tauri-apps/api/webviewWindow'
import { LogicalSize } from '@tauri-apps/api/dpi'
import { invoke } from '@tauri-apps/api/core'
import { startRecording as sttStart, stopRecording as sttStop, isRecording as sttIsRecording } from './stt'

// Encapsulated hide logic so we can call it from multiple places
async function hidePopup(): Promise<void> {
  try {
    await getCurrentWebviewWindow().hide()
  } catch (err) {
    // Fail loud in dev, but don't crash UI
    console.error('[quick-actions] hide failed', err)
  }
}

const sttRecording = ref(false)
const sttPending = ref(false) // true while requesting mic permission / starting
const rootRef = ref<HTMLElement | null>(null)

async function handleAction(action: 'prompt' | 'tts' | 'stt' | 'image'): Promise<void> {
  console.info(`[quick-actions] action: ${action}`)
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
    console.info(`[quick-actions] quick prompt #${index}`)
    // Close popup immediately per spec, then run quick prompt on backend
    void hidePopup()
    void invoke('run_quick_prompt', { index, safe_mode: false })
  }
  // Escape closes the popup
  if (key === 'escape') {
    e.preventDefault()
    if (sttRecording.value) {
      // Cancel recording and close without transcribe
      void cancelSTT()
    } else {
      void hidePopup()
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
  if (!sttRecording.value && !sttPending.value) void hidePopup()
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
      await invoke('insert_prompt_text', { text: 'Transcription canceled.' })
      return
    }
    const { blob, mime } = res
    const buf = new Uint8Array(await blob.arrayBuffer())
    // Close popup before transcribing
    await hidePopup()
    let text = ''
    try {
      text = await invoke<string>('stt_transcribe', { audio: Array.from(buf), mime })
    } catch (err) {
      console.error('[stt] transcribe failed', err)
      const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown STT error'
      await invoke('insert_prompt_text', { text: `Transcription failed: ${msg}` })
      return
    }
    if (text && text.trim().length > 0) {
      await invoke('insert_prompt_text', { text })
    } else {
      await invoke('insert_prompt_text', { text: 'No speech detected.' })
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
        try { w.setSize(new LogicalSize(width, height)) } catch {}
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
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKeydown)
  window.removeEventListener('keyup', onKeyup)
  window.removeEventListener('blur', onWindowBlur)
  window.removeEventListener('mouseup', onWindowMouseup)
})
</script>

<template>
  <div class="qa-root" ref="rootRef" role="dialog" aria-label="Quick Actions">
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
  </div>
</template>

<style scoped>
.qa-root {
  width: max-content; /* shrink-wrap */
  height: max-content; /* shrink-wrap */
  display: flex;
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
</style>
