<script setup lang="ts">
// Floating status pill shown while a background operation runs.
//
// Quick prompts triggered by a global hotkey, TTS on the selection and STT
// transcription all run with no visible app window: the popup hides itself
// first and the main window may never open. This window is the only feedback
// the user gets, so it reports what is running, how long it has been running,
// and - crucially - the error when a request fails or times out, which the
// calling code would otherwise swallow.
import { onMounted, onBeforeUnmount, ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'

type BusyState = {
  state: 'idle' | 'running' | 'error'
  label: string
  detail: string
  started_ms: number
}

const state = ref<BusyState>({ state: 'idle', label: '', detail: '', started_ms: 0 })
const nowMs = ref<number>(Date.now())

// Hide the error state on its own after a while so a stale message never sticks
// around on top of everything else.
const ERROR_AUTO_HIDE_MS = 8000

let ticker: number | null = null
let errorTimer: number | null = null
let unlisten: UnlistenFn | null = null

const elapsedSeconds = computed(() => {
  if (state.value.state !== 'running' || !state.value.started_ms) return 0
  return Math.max(0, Math.floor((nowMs.value - state.value.started_ms) / 1000))
})

const elapsedText = computed(() => {
  const s = elapsedSeconds.value
  if (s < 60) return `${s}s`
  const m = Math.floor(s / 60)
  return `${m}m ${s % 60}s`
})

// The backend gives up at 120s; warn before that so a hang is recognisable.
const slow = computed(() => state.value.state === 'running' && elapsedSeconds.value >= 20)

const errorText = computed(() => {
  const d = (state.value.detail || '').trim()
  if (!d) return 'Request failed.'
  return d.length > 220 ? `${d.slice(0, 217)}…` : d
})

function clearErrorTimer() {
  if (errorTimer !== null) {
    clearTimeout(errorTimer)
    errorTimer = null
  }
}

// The window is only hidden, never destroyed, so the elapsed-time ticker is
// started and stopped with the running state instead of left running forever.
function startTicker() {
  if (ticker !== null) return
  ticker = window.setInterval(() => { nowMs.value = Date.now() }, 500)
}

function stopTicker() {
  if (ticker === null) return
  clearInterval(ticker)
  ticker = null
}

function applyState(next: BusyState) {
  state.value = next
  nowMs.value = Date.now()
  clearErrorTimer()
  if (next.state === 'running') startTicker()
  else stopTicker()
  if (next.state === 'error') {
    errorTimer = window.setTimeout(() => { void dismiss() }, ERROR_AUTO_HIDE_MS)
  }
}

async function dismiss() {
  clearErrorTimer()
  try {
    await invoke('busy_hide')
  } catch (err) {
    console.warn('[busy] hide failed', err)
  }
}

onMounted(async () => {
  try {
    unlisten = await listen<BusyState>('busy:state', (event) => {
      if (event?.payload) applyState(event.payload)
    })
  } catch (err) {
    console.warn('[busy] listen failed', err)
  }
  // The window can finish loading after the operation already started, so pull
  // the current state instead of waiting for the next event.
  try {
    const current = await invoke<BusyState>('busy_get_state')
    if (current && current.state !== 'idle') applyState(current)
  } catch (err) {
    console.warn('[busy] initial state failed', err)
  }
})

onBeforeUnmount(() => {
  stopTicker()
  clearErrorTimer()
  if (unlisten) { try { unlisten() } catch {} }
})
</script>

<template>
  <div
    class="busy-root"
    :class="{ 'is-error': state.state === 'error' }"
    role="status"
    aria-live="polite"
    title="Click to dismiss"
    @click="dismiss"
  >
    <div v-if="state.state === 'error'" class="dot-error" aria-hidden="true" />
    <div v-else class="spinner" aria-hidden="true" />
    <div class="text">
      <div class="label">{{ state.label || 'Working' }}</div>
      <div v-if="state.state === 'error'" class="detail">{{ errorText }}</div>
      <div v-else class="detail">
        {{ elapsedText }}<span v-if="slow"> · still working</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.busy-root {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  height: 100%;
  box-sizing: border-box;
  background: var(--adc-surface, #1e1f24);
  color: var(--adc-fg, #f2f3f5);
  border: 1px solid var(--adc-border, #33363d);
  border-radius: 12px;
  user-select: none;
  cursor: pointer;
  overflow: hidden;
}
.busy-root.is-error {
  border-color: #b3413b;
}
.spinner {
  flex: 0 0 auto;
  width: 16px;
  height: 16px;
  border: 2px solid var(--adc-border, #33363d);
  border-top-color: var(--adc-accent, #6ea8fe);
  border-radius: 50%;
  animation: spin 1s linear infinite;
}
.dot-error {
  flex: 0 0 auto;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #e5534b;
}
.text {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.label {
  font-size: 12px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.detail {
  font-size: 11px;
  color: var(--adc-fg-muted, #a9adb8);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.is-error .detail {
  color: #f0a6a2;
}
@keyframes spin { from { transform: rotate(0); } to { transform: rotate(360deg); } }
</style>
