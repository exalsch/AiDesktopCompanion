<script setup lang="ts">
// Floating call pill for Assistant Mode.
//
// A voice session is meant to be used while working in another application, so
// the main window is normally behind whatever the user is actually doing. This
// is the only on-screen sign that a call is live, and the only way to hang up
// without going to find the window.
//
// Two states, driven entirely from the backend:
//   armed - the push-to-talk key was pressed with no session running. Starting a
//           call costs money and opens a microphone, so it asks for a second
//           press rather than acting on a stray keystroke.
//   live  - a call is up: elapsed time, whether the mic is open, and hang up.
import { onMounted, onBeforeUnmount, ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { emit, listen } from '@tauri-apps/api/event'
import type { UnlistenFn } from '@tauri-apps/api/event'

type PillState = {
  state: 'hidden' | 'armed' | 'live'
  started_ms: number
  mic_open: boolean
  hotkey: string
}

const state = ref<PillState>({ state: 'hidden', started_ms: 0, mic_open: false, hotkey: '' })
const nowMs = ref<number>(Date.now())

let ticker: number | null = null
let unlisten: UnlistenFn | null = null

const elapsed = computed(() => {
  if (state.value.state !== 'live' || !state.value.started_ms) return '0:00'
  const total = Math.max(0, Math.floor((nowMs.value - state.value.started_ms) / 1000))
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`
})

function startTicker() {
  if (ticker !== null) return
  ticker = window.setInterval(() => { nowMs.value = Date.now() }, 1000)
}

function stopTicker() {
  if (ticker !== null) { window.clearInterval(ticker); ticker = null }
}

function apply(next: PillState) {
  state.value = next
  if (next.state === 'live') startTicker()
  else stopTicker()
}

function hangUp() {
  // The session lives in the main window; this only asks for it to end.
  void emit('assistant:hangup')
}

onMounted(async () => {
  try {
    const s = await invoke<PillState>('assistant_pill_get_state')
    if (s) apply(s)
  } catch {
    // Nothing to show yet; the first event will populate it.
  }
  try {
    unlisten = await listen<PillState>('assistant-pill:state', (e) => {
      if (e?.payload) apply(e.payload)
    })
  } catch {}
})

onBeforeUnmount(() => {
  stopTicker()
  try { unlisten?.() } catch {}
})
</script>

<template>
  <div class="pill" :class="state.state">
    <template v-if="state.state === 'armed'">
      <span class="glyph" aria-hidden="true">🎙</span>
      <span class="text">
        Press
        <code v-if="state.hotkey">{{ state.hotkey }}</code>
        again to start an Assistant call
      </span>
    </template>

    <template v-else-if="state.state === 'live'">
      <span class="dot" :class="{ open: state.mic_open }" aria-hidden="true"></span>
      <span class="text">
        <strong>{{ state.mic_open ? 'Listening' : 'On call' }}</strong>
        <span class="time">{{ elapsed }}</span>
      </span>
      <button class="hangup" type="button" title="End the call" @click="hangUp">End</button>
    </template>
  </div>
</template>

<style scoped>
/* The window is transparent, so the pill itself has to draw the whole surface.
   Fixed dark colours rather than theme tokens: this floats over other people's
   applications, where the app's own light theme would read as a glitch. */
.pill {
  display: flex;
  align-items: center;
  gap: 10px;
  height: 100%;
  box-sizing: border-box;
  padding: 0 12px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.14);
  background: rgba(22, 24, 30, 0.94);
  color: #e8eaf0;
  font-size: 12.5px;
  line-height: 1.3;
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.45);
  user-select: none;
  overflow: hidden;
}
.pill.armed { border-color: rgba(122, 162, 255, 0.5); }
.pill.live { border-color: rgba(90, 200, 130, 0.45); }

.glyph { font-size: 14px; flex: 0 0 auto; }

.text {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

code {
  font-family: ui-monospace, Consolas, monospace;
  font-size: 11.5px;
  padding: 1px 5px;
  border-radius: 5px;
  background: rgba(255, 255, 255, 0.1);
}

.time { color: #9aa3b2; font-variant-numeric: tabular-nums; }

/* Green when the microphone is open, dim when it is not - the one thing worth
   knowing at a glance during a call. */
.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #6b7280;
  flex: 0 0 auto;
}
.dot.open {
  background: #4ade80;
  box-shadow: 0 0 0 3px rgba(74, 222, 128, 0.2);
}

.hangup {
  flex: 0 0 auto;
  border: 1px solid rgba(255, 120, 120, 0.45);
  background: rgba(220, 60, 60, 0.22);
  color: #ffb4b4;
  border-radius: 999px;
  padding: 3px 12px;
  font-size: 11.5px;
  cursor: pointer;
}
.hangup:hover { background: rgba(220, 60, 60, 0.38); color: #fff; }
</style>
