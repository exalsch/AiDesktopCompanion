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
  state: 'hidden' | 'armed' | 'calling' | 'live'
  started_ms: number
  mic_open: boolean
  hotkey: string
  ptt: boolean
}

const state = ref<PillState>({ state: 'hidden', started_ms: 0, mic_open: false, hotkey: '', ptt: false })
/** Whether this window's own pointer is down on the button. */
const holding = ref(false)

/**
 * Whether the microphone is open, however it was opened.
 *
 * The button is not the only way to talk - the global hotkey is the main one -
 * so its appearance follows `mic_open` from the backend rather than this
 * window's own pointer state. Holding the hotkey now lights the button too.
 */
const talking = computed(() => state.value.mic_open || holding.value)
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

/**
 * Hold-to-talk, for when the keyboard shortcut is not to hand.
 *
 * The pointer is captured so that releasing anywhere - having slid off the
 * button, or off the window entirely - still ends the turn. A hold that never
 * gets its release would leave the microphone open, which is the one failure
 * push-to-talk exists to prevent.
 *
 * `preventDefault` on pointerdown stops the button taking focus. The window is
 * already non-activating, and a call is meant to be used while working in
 * another application.
 */
function pttDown(e: PointerEvent) {
  if (holding.value) return
  e.preventDefault()
  holding.value = true
  try { (e.currentTarget as HTMLElement)?.setPointerCapture?.(e.pointerId) } catch {}
  void emit('assistant:ptt-down')
}

function pttUp() {
  if (!holding.value) return
  holding.value = false
  void emit('assistant:ptt-up')
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

    <template v-else-if="state.state === 'calling'">
      <span class="ring" aria-hidden="true"></span>
      <span class="text">
        <strong>Calling</strong>
        <span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>
      </span>
      <button class="btn hangup" type="button" title="Cancel the call" @click="hangUp">Cancel</button>
    </template>

    <template v-else-if="state.state === 'live'">
      <span class="dot" :class="{ open: state.mic_open }" aria-hidden="true"></span>
      <span class="text">
        <strong>{{ state.mic_open ? 'Listening' : 'On call' }}</strong>
        <span class="time">{{ elapsed }}</span>
      </span>
      <button
        v-if="state.ptt"
        class="btn talk"
        :class="{ talking }"
        type="button"
        :title="state.hotkey ? `Hold to talk (or hold ${state.hotkey})` : 'Hold to talk'"
        @pointerdown="pttDown"
        @pointerup="pttUp"
        @pointercancel="pttUp"
        @lostpointercapture="pttUp"
      >{{ talking ? 'Talking' : 'Talk' }}</button>
      <button class="btn hangup" type="button" title="End the call" @click="hangUp">End</button>
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
.pill.calling { border-color: rgba(122, 162, 255, 0.5); }
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

/* One button shape for the pill, so Talk and End read as a pair rather than as
   two unrelated controls. Only the accent colour differs. */
.btn {
  flex: 0 0 auto;
  border-radius: 999px;
  padding: 3px 12px;
  font-size: 11.5px;
  line-height: 1.4;
  cursor: pointer;
  border: 1px solid rgba(255, 255, 255, 0.16);
  background: rgba(255, 255, 255, 0.08);
  color: #e8eaf0;
  transition: background 120ms ease, border-color 120ms ease, color 120ms ease;
}
.btn:hover { background: rgba(255, 255, 255, 0.14); }

/* Neutral at rest so it sits with the rest of the pill, and only takes the
   microphone's green while actually talking - the one moment it matters. */
.talk { touch-action: none; }
.talk.talking {
  background: rgba(74, 222, 128, 0.22);
  border-color: rgba(74, 222, 128, 0.55);
  color: #a7f3c4;
}

.hangup { color: #f0b8b8; border-color: rgba(255, 255, 255, 0.16); }
.hangup:hover {
  background: rgba(220, 60, 60, 0.32);
  border-color: rgba(255, 120, 120, 0.5);
  color: #fff;
}

/* Calling: a ring that pulses outward, and three dots that cycle. Motion is the
   point - it says "working on it" without the pill having to say so. */
.ring {
  position: relative;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #7aa2ff;
  flex: 0 0 auto;
}
.ring::after {
  content: '';
  position: absolute;
  inset: -4px;
  border-radius: 50%;
  border: 1px solid rgba(122, 162, 255, 0.7);
  animation: ring-pulse 1.4s ease-out infinite;
}
@keyframes ring-pulse {
  0% { transform: scale(0.6); opacity: 0.9; }
  100% { transform: scale(1.6); opacity: 0; }
}

.dots { display: inline-flex; gap: 3px; align-items: center; }
.dots i {
  width: 3px;
  height: 3px;
  border-radius: 50%;
  background: #9aa3b2;
  animation: dot-fade 1.2s ease-in-out infinite;
}
.dots i:nth-child(2) { animation-delay: 0.15s; }
.dots i:nth-child(3) { animation-delay: 0.3s; }
@keyframes dot-fade {
  0%, 60%, 100% { opacity: 0.25; }
  30% { opacity: 1; }
}

/* Motion is decoration here; the words already carry the state. */
@media (prefers-reduced-motion: reduce) {
  .ring::after, .dots i { animation: none; }
}
</style>
