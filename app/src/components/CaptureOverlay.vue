<script setup lang="ts">
import { onMounted, onBeforeUnmount, reactive, computed, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
import { listen } from '@tauri-apps/api/event'

// Maintain both window-relative coords (for drawing) and screen coords (for capture)
const state = reactive({
  dragging: false,
  startClientX: 0,
  startClientY: 0,
  curClientX: 0,
  curClientY: 0,
  startScreenX: 0,
  startScreenY: 0,
  curScreenX: 0,
  curScreenY: 0,
})

// When true, overlay UI is hidden and interaction is disabled while the window is being closed
const closing = ref(false)

const visualRect = computed(() => {
  const x1 = Math.min(state.startClientX, state.curClientX)
  const y1 = Math.min(state.startClientY, state.curClientY)
  const x2 = Math.max(state.startClientX, state.curClientX)
  const y2 = Math.max(state.startClientY, state.curClientY)
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 }
})

const screenRect = computed(() => {
  const x1 = Math.min(state.startScreenX, state.curScreenX)
  const y1 = Math.min(state.startScreenY, state.curScreenY)
  const x2 = Math.max(state.startScreenX, state.curScreenX)
  const y2 = Math.max(state.startScreenY, state.curScreenY)
  return { x: x1, y: y1, w: x2 - x1, h: y2 - y1 }
})

function toScreenCoords(evt: MouseEvent) {
  // Convert client (CSS px) -> screen (physical px): add outerPosition (physical) + multiply by DPR
  const dpr = window.devicePixelRatio || 1
  return getCurrentWebviewWindow().outerPosition().then((pos: any) => {
    const baseX = (pos?.x ?? 0)
    const baseY = (pos?.y ?? 0)
    return { x: Math.round(evt.clientX * dpr + baseX), y: Math.round(evt.clientY * dpr + baseY) }
  }).catch(() => ({ x: Math.round(evt.clientX * dpr), y: Math.round(evt.clientY * dpr) }))
}

async function onMouseDown(e: MouseEvent) {
  e.preventDefault()
  const p = await toScreenCoords(e)
  state.dragging = true
  state.startClientX = e.clientX
  state.startClientY = e.clientY
  state.curClientX = e.clientX
  state.curClientY = e.clientY
  state.startScreenX = p.x
  state.startScreenY = p.y
  state.curScreenX = p.x
  state.curScreenY = p.y
}

async function onMouseMove(e: MouseEvent) {
  if (!state.dragging) return
  const p = await toScreenCoords(e)
  state.curClientX = e.clientX
  state.curClientY = e.clientY
  state.curScreenX = p.x
  state.curScreenY = p.y
}

async function onMouseUp(_e: MouseEvent) {
  if (!state.dragging) return
  const r = screenRect.value
  state.dragging = false
  if (r.w < 2 || r.h < 2) {
    try { await getCurrentWebviewWindow().close() } catch {}
    return
  }
  console.info('[capture-overlay] capturing region', r)
  try {
    // Hide overlay before capture so it doesn't get into the screenshot
    const w = getCurrentWebviewWindow()
    // Visually disable immediately to avoid lingering UI even if close is delayed
    closing.value = true
    try { document.documentElement.style.pointerEvents = 'none'; document.body.style.pointerEvents = 'none' } catch {}
    try { await w.hide() } catch {}
    await invoke<string>('capture_region', { x: r.x, y: r.y, width: r.w, height: r.h })
    console.info('[capture-overlay] capture invoked ok')
  } catch (err) {
    console.error('[capture-overlay] capture failed', err)
    const msg = typeof err === 'string' ? err : (err && (err as any).message) ? (err as any).message : 'Unknown capture error'
    // Open main window with error so the user sees feedback
    try { await invoke('open_prompt_with_text', { text: `Image capture failed: ${msg}` }) } catch {}
  } finally {
    // Clear selection to remove rectangle immediately in case window doesn't close instantly
    state.startClientX = 0; state.startClientY = 0; state.curClientX = 0; state.curClientY = 0;
    state.startScreenX = 0; state.startScreenY = 0; state.curScreenX = 0; state.curScreenY = 0;
    // Detach interaction listeners to prevent further drags if close is delayed
    window.removeEventListener('mousedown', onMouseDown)
    window.removeEventListener('mousemove', onMouseMove)
    window.removeEventListener('mouseup', onMouseUp)
    const w = getCurrentWebviewWindow()
    // Drop flags that can interfere with immediate close on Windows
    try { await w.setAlwaysOnTop(false) } catch {}
    try { await (w as any).setFullscreen?.(false) } catch {}
    try { await w.unmaximize() } catch {}
    // Small delay to let the window state settle before closing
    await new Promise(res => setTimeout(res, 50))
    try { await w.close() } catch (e) {
      console.error('[capture-overlay] close failed, hiding instead', e)
      try { await w.hide() } catch {}
    }
  }
}

function onKey(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    e.preventDefault()
    getCurrentWebviewWindow().close().catch(() => {})
  }
}

let prevHtmlBg = ''
let prevBodyBg = ''
let unlistenImage: (() => void) | null = null

onMounted(() => {
  window.addEventListener('mousedown', onMouseDown)
  window.addEventListener('mousemove', onMouseMove)
  window.addEventListener('mouseup', onMouseUp)
  window.addEventListener('keydown', onKey)
  window.addEventListener('keyup', onKey)
  // Right-click to cancel quickly
  window.addEventListener('contextmenu', (e) => { e.preventDefault(); getCurrentWebviewWindow().close().catch(() => {}) })
  // Make the underlying page fully transparent so the desktop shows through
  prevHtmlBg = (document.documentElement.style.background || '')
  prevBodyBg = (document.body.style.background || '')
  document.documentElement.style.background = 'transparent'
  document.body.style.background = 'transparent'

  // Ensure always on top, focused and maximized
  const w = getCurrentWebviewWindow()
  w.setAlwaysOnTop(true).catch(() => {})
  w.maximize().catch(() => {})
  w.setFocus().catch(() => {})

  // Backend emits 'image:capture' on success; force-close overlay when received
  listen<{ path: string }>('image:capture', async () => {
    try { await getCurrentWebviewWindow().close() } catch {}
  }).then((un) => { unlistenImage = un }).catch(() => {})
})

onBeforeUnmount(() => {
  window.removeEventListener('mousedown', onMouseDown)
  window.removeEventListener('mousemove', onMouseMove)
  window.removeEventListener('mouseup', onMouseUp)
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('keyup', onKey)
  document.documentElement.style.background = prevHtmlBg
  document.body.style.background = prevBodyBg
  try { unlistenImage && unlistenImage() } catch {}
})
</script>

<template>
  <div class="overlay-root" v-show="!closing">
    <!-- Dim background -->
    <div class="dim"></div>
    <!-- Selection rectangle -->
    <div
      v-if="state.dragging"
      class="selection"
      :style="{ left: visualRect.x + 'px', top: visualRect.y + 'px', width: visualRect.w + 'px', height: visualRect.h + 'px' }"
    ></div>
    <!-- Crosshair hint -->
    <div class="hint">Drag to select region. Esc to cancel.</div>
  </div>
</template>

<style scoped>
.overlay-root {
  position: fixed;
  inset: 0;
  pointer-events: auto;
  cursor: crosshair;
}
.dim {
  position: absolute;
  inset: 0;
  background: rgba(0,0,0,0.35);
}
.selection {
  position: absolute;
  box-sizing: border-box;
  border: 2px solid #ff2d2d; /* 2px red outline */
  background: rgba(255,255,255,0.08);
}
.hint {
  position: fixed;
  left: 50%;
  bottom: 20px;
  transform: translateX(-50%);
  color: #fff;
  font-size: 14px;
  padding: 6px 10px;
  background: rgba(0,0,0,0.6);
  border-radius: 6px;
}
</style>

<style>
/* Ensure the desktop shows through for the overlay window */
html, body, #app {
  background: transparent !important;
}
</style>
