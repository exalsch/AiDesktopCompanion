<script setup lang="ts">
import { computed, onMounted, onBeforeUnmount, reactive, ref, watch, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAssistantRealtime } from '../../composables/useAssistantRealtime'
import { useSettings } from '../../composables/useSettings'
import CollapsibleCard from '../ui/CollapsibleCard.vue'

const props = defineProps<{
  mcpServers: any[]
  notify: (msg: string, kind?: 'error'|'success', ms?: number) => void
  /** Increments when something outside the panel asks for a session. */
  autostart?: number
}>()

const ui = reactive({
  connecting: false,
  connected: false,
  error: '' as string | null,
  enableTools: false,
  useSupervisor: false,
  showDebug: false,
})

// Keep realtime session in sync when toggling tools/supervisor while connected
watch(() => ui.enableTools, syncSession)

watch(() => ui.useSupervisor, syncSession)

const statusText = ref('Idle')
const remoteAudioElRef = ref<HTMLAudioElement | null>(null)

const transcript = computed<Array<{ role: string, content: string }>>(
  () => (realtime as any).transcript?.value ?? []
)

const micEnabled = computed(() => (realtime as any).micEnabled?.value !== false)

function toggleMic() {
  ;(realtime as any).setMicEnabled?.(!micEnabled.value)
}

// Realtime audio is billed by the minute, so how long a session has been open
// is the number worth putting on screen.
const elapsedSeconds = ref(0)
let elapsedTimer: any = 0

function startElapsed() {
  elapsedSeconds.value = 0
  if (elapsedTimer) clearInterval(elapsedTimer)
  elapsedTimer = setInterval(() => { elapsedSeconds.value += 1 }, 1000)
}

function stopElapsed() {
  if (elapsedTimer) { clearInterval(elapsedTimer); elapsedTimer = 0 }
}

const elapsedLabel = computed(() => {
  const total = elapsedSeconds.value
  const m = Math.floor(total / 60)
  const sec = total % 60
  return `${m}:${String(sec).padStart(2, '0')}`
})

/**
 * The conversation as plain text, ready to copy or paste elsewhere.
 *
 * Tool calls are left out: they are diagnostics, not something anyone wants
 * pasted into a document.
 */
const transcriptText = ref('')
watch(transcript, (turns) => {
  transcriptText.value = turns
    .filter((t) => t.role !== 'tool')
    .map((t) => `${t.role === 'user' ? 'You' : 'Assistant'}: ${t.content}`)
    .join('\n\n')
}, { deep: true })

async function copyTranscript() {
  try {
    await invoke('copy_text_to_clipboard', { text: transcriptText.value })
    props.notify?.('Transcript copied', 'success')
  } catch (e: any) {
    props.notify?.(e?.message || 'Copy failed', 'error')
  }
}

/**
 * Paste the transcript into whatever the user was last working in.
 *
 * The main window has focus while this button is being clicked, so the previous
 * application has to be brought forward first - the same dance the Quick
 * Actions popup does. The clipboard copy runs first regardless, so the text is
 * never lost if the paste does not land.
 */
async function insertTranscript() {
  const text = transcriptText.value.trim()
  if (!text) return
  try { await invoke('copy_text_to_clipboard', { text }) } catch {}
  try {
    await invoke('refocus_previous_app')
    await new Promise((r) => setTimeout(r, 80))
    await invoke('insert_text_into_focused_app', { text, safe_mode: false })
  } catch (e: any) {
    props.notify?.((e?.message || 'Insert failed') + ' - the text is on the clipboard.', 'error')
  }
}

const debugLines = ref<string[]>([])
const rateLimits = ref<any[]>([])

const debugLogBoxRef = ref<HTMLDivElement | null>(null)
const debugLogBottomRef = ref<HTMLDivElement | null>(null)
const debugAutoScroll = ref(true)

function updateDebugAutoScrollFromEl(el: HTMLElement) {
  // Consider "near bottom" as within 32px of the end.
  const thresholdPx = 32
  const distanceToBottom = el.scrollHeight - el.scrollTop - el.clientHeight
  debugAutoScroll.value = distanceToBottom <= thresholdPx
}

async function scrollDebugToBottomIfEnabled() {
  if (!ui.showDebug) return
  if (!debugAutoScroll.value) return
  await nextTick()
  try {
    const bottom = debugLogBottomRef.value
    if (bottom) bottom.scrollIntoView({ block: 'end' })
  } catch {
    try {
      const el = debugLogBoxRef.value
      if (el) el.scrollTop = el.scrollHeight
    } catch {}
  }
}

/**
 * Fallback realtime models.
 *
 * `gpt-realtime` is an alias OpenAI repoints at the current GA snapshot, so it
 * is the default. The list is only a fallback: `refreshModels` replaces it with
 * whatever the account can actually see, because the previous hardcoded entries
 * (`gpt-4o-realtime-preview` and its mini) were retired and every session
 * request against them failed.
 */
const FALLBACK_REALTIME_MODELS = [
  'gpt-realtime',
  'gpt-realtime-2.1',
  'gpt-realtime-2.1-mini',
  'gpt-realtime-2',
  'gpt-realtime-mini',
  'gpt-realtime-1.5',
]

const DEFAULT_REALTIME_MODEL = 'gpt-realtime'

const models = ref<string[]>([...FALLBACK_REALTIME_MODELS])

/**
 * Replace the model list with the realtime models this API key can reach.
 *
 * `-translate` and `-whisper` are realtime-family ids that are not
 * speech-to-speech models, so they would only fail if picked.
 */
async function refreshModels() {
  try {
    const all = await invoke<string[]>('list_openai_models')
    const realtime = (Array.isArray(all) ? all : [])
      .filter((m) => m.includes('realtime'))
      .filter((m) => !m.includes('translate') && !m.includes('whisper'))
    if (realtime.length) models.value = realtime
  } catch {
    // Offline or no key yet - the fallback list still lets the panel render.
  }
}

// Voices accepted by the realtime API. `aria` and `tenor` used to be listed
// here and are not OpenAI voices at all; a session minted with one is rejected.
const voices = [
  'alloy','ash','ballad','cedar','coral','echo','marin','sage','shimmer','verse'
]

// Effort levels the realtime API accepts. Only the reasoning-capable models
// take the field at all, which is why the control is disabled for the others.
const REASONING_EFFORTS = ['minimal', 'low', 'medium', 'high', 'xhigh']

function modelSupportsReasoning(model: string): boolean {
  return /^gpt-realtime-2(\.|$|-)/.test(String(model || ''))
}

const reasoningSupported = computed(() => modelSupportsReasoning(session.model))

const session = reactive({
  model: DEFAULT_REALTIME_MODEL,
  voice: 'alloy',
  supervisorMode: 'always' as 'always' | 'needed',
  instructions: 'You are a voice assistant running on the user\'s desktop. Speak naturally and keep replies short - this is a conversation, not a document. Say when you do not know something rather than guessing. IMPORTANT: Always reply in the same language the user is speaking. If you are unsure, reply in English, and do not switch languages mid-conversation unless the user clearly switches.',
  silenceDurationMs: 2000,
  idleTimeoutMs: null as number | null,
  inputAudioNoiseReduction: true,
  reasoningEffort: null as string | null,
  // A live session bills per minute with an open microphone, so a forgotten
  // window closes itself rather than running until somebody notices.
  autoCloseMinutes: 2,
})

/**
 * Push the whole session config to the live connection.
 *
 * Every control in this panel needs to do exactly this on change. The call was
 * previously pasted inline in the template eight times, once per control, each
 * copy listing all ten fields - so adding a field meant editing nine places and
 * missing one was silent.
 *
 * No-ops when not connected, so callers do not have to check.
 */
async function syncSession() {
  if (!ui.connected) return
  await (realtime as any).updateSession({
    enableTools: ui.enableTools,
    useSupervisor: ui.useSupervisor,
    supervisorMode: session.supervisorMode,
    model: session.model,
    voice: session.voice,
    instructions: session.instructions,
    silenceDurationMs: session.silenceDurationMs,
    idleTimeoutMs: session.idleTimeoutMs,
    inputAudioNoiseReduction: session.inputAudioNoiseReduction,
    reasoningEffort: session.reasoningEffort,
    autoCloseMs: Math.max(0, session.autoCloseMinutes) * 60000,
  })
}

watch(() => session.supervisorMode, syncSession)

// Load Prompt section settings (temperature, etc.) for supervisor alignment
const { settings: appSettings, loadSettings } = useSettings()

const realtime = useAssistantRealtime({
  getEphemeralToken: async () => {
    try {
      return await invoke<string>('realtime_create_ephemeral_token', { model: session.model, voice: session.voice })
    } catch (e: any) {
      // Report what actually failed. This used to append "Backend command
      // realtime_create_ephemeral_token is missing" to every error - a command
      // that has always been registered - so a live OpenAI error was presented
      // as a missing-command bug.
      const msg = typeof e === 'string' ? e : (e?.message || 'Ephemeral token request failed')
      throw new Error('Could not mint a realtime token: ' + msg)
    }
  },
  onConnected: () => { ui.connected = true; ui.connecting = false; ui.error = null; statusText.value = 'Connected'; startElapsed() },
  onDisconnected: () => { ui.connected = false; ui.connecting = false; statusText.value = 'Idle'; stopElapsed() },
  onError: (err: string) => { ui.error = err; props.notify?.(err, 'error'); ui.connecting = false; ui.connected = false; statusText.value = 'Error'; try { debugLines.value.push(`[error] ${err}`) } catch {} },
  // Surfaced but does not change connection state: the call is still up.
  onWarn: (msg: string) => { props.notify?.(msg, 'error'); try { debugLines.value.push(`[warn] ${msg}`) } catch {} },
  onLog: (msg: string) => {
    try {
      debugLines.value.push(msg)
      if (debugLines.value.length > 200) debugLines.value.shift()
    } catch {}
    void scrollDebugToBottomIfEnabled()
  },
  onRateLimits: (limits: any[]) => { rateLimits.value = limits },
})

async function activate() {
  if (ui.connecting || ui.connected) return
  try {
    debugLines.value.splice(0, debugLines.value.length)
    rateLimits.value.splice(0, rateLimits.value.length)
    debugAutoScroll.value = true
  } catch {}
  ui.connecting = true
  statusText.value = 'Connecting…'
  await realtime.connect({
    enableTools: ui.enableTools,
    useSupervisor: ui.useSupervisor,
    supervisorMode: session.supervisorMode,
    model: session.model,
    voice: session.voice,
    instructions: session.instructions,
    silenceDurationMs: session.silenceDurationMs,
    idleTimeoutMs: session.idleTimeoutMs,
    inputAudioNoiseReduction: session.inputAudioNoiseReduction,
    reasoningEffort: session.reasoningEffort,
    autoCloseMs: Math.max(0, session.autoCloseMinutes) * 60000,
  })
}

async function deactivate() {
  await realtime.disconnect()
}

async function toggle() {
  if (ui.connected || ui.connecting) {
    await deactivate()
  } else {
    await activate()
  }
}

onMounted(async () => {
  // Attach the hidden audio element for reliable playback in WebView
  try { if (remoteAudioElRef.value) realtime.attachAudioElement(remoteAudioElRef.value) } catch {}
  // Load assistant_realtime persisted settings
  try {
    const v: any = await invoke('get_settings')
    const ar = (v && typeof v === 'object') ? (v as any).assistant_realtime : null
    if (ar && typeof ar === 'object') {
      // Retired preview ids were persisted by older versions and no longer
      // exist, so a stale value has to be dropped rather than sent.
      if (typeof ar.model === 'string' && ar.model && !ar.model.includes('-preview')) session.model = ar.model
      if (typeof ar.voice === 'string' && voices.includes(ar.voice)) session.voice = ar.voice
      if (typeof ar.supervisor_mode === 'string') session.supervisorMode = (String(ar.supervisor_mode).toLowerCase() === 'needed') ? 'needed' : 'always'
      if (typeof ar.instructions === 'string') session.instructions = ar.instructions
      if (typeof ar.silence_duration_ms === 'number') session.silenceDurationMs = ar.silence_duration_ms
      if (ar.idle_timeout_ms === null || typeof ar.idle_timeout_ms === 'number') session.idleTimeoutMs = ar.idle_timeout_ms
      if (typeof ar.input_audio_noise_reduction === 'boolean') session.inputAudioNoiseReduction = ar.input_audio_noise_reduction
      if (ar.reasoning_effort === null || REASONING_EFFORTS.includes(ar.reasoning_effort)) session.reasoningEffort = ar.reasoning_effort ?? null
      if (typeof ar.auto_close_minutes === 'number' && ar.auto_close_minutes >= 0) session.autoCloseMinutes = ar.auto_close_minutes
      if (typeof ar.show_debug === 'boolean') ui.showDebug = ar.show_debug
    }
  } catch (e) {
    debugLines.value.push('[warn] failed to load assistant_realtime settings')
  }
  void refreshModels()
  // Do not reload global settings here; App.vue already loads them.
  // Reloading would rehydrate settings and could inadvertently reset MCP runtime statuses.
  // try { await loadSettings() } catch {}
})

watch(() => debugLines.value.length, async () => {
  await scrollDebugToBottomIfEnabled()
})

watch(session, async () => {
  // Persist assistant_realtime settings immediately on change
  try {
    await invoke('save_settings', {
      map: {
        assistant_realtime: {
          model: session.model,
          voice: session.voice,
          supervisor_mode: session.supervisorMode,
          instructions: session.instructions,
          silence_duration_ms: session.silenceDurationMs,
          idle_timeout_ms: session.idleTimeoutMs,
          input_audio_noise_reduction: session.inputAudioNoiseReduction,
          reasoning_effort: session.reasoningEffort,
          auto_close_minutes: session.autoCloseMinutes,
          show_debug: ui.showDebug,
        }
      }
    })
  } catch (e) {
    debugLines.value.push('[warn] failed to save assistant_realtime settings')
  }
}, { deep: true })

// Quick Actions raises this to start a session without the user clicking.
watch(() => props.autostart, (n, old) => {
  if (typeof n === 'number' && typeof old === 'number' && n > old) void activate()
})

onBeforeUnmount(() => {
  stopElapsed()
  try { realtime.disconnect() } catch {}
})
</script>

<template>
  <section class="card">
    <div class="card-body">
      <p class="warn-banner" role="status">
        <strong>Experimental.</strong>
        Assistant Mode is still under active development and may be unstable.
      </p>

      <div class="actions">
        <button class="btn" type="button" :class="{ ghost: ui.connected || ui.connecting }" @click="toggle">
          {{ ui.connected || ui.connecting ? 'Stop' : 'Start' }}
        </button>
        <button
          v-if="ui.connected"
          class="btn ghost"
          type="button"
          :title="micEnabled ? 'Mute the microphone' : 'Unmute the microphone'"
          @click="toggleMic"
        >
          {{ micEnabled ? 'Mute' : 'Unmute' }}
        </button>
        <span class="badge" :class="ui.error ? 'err' : (ui.connected ? 'ok' : (ui.connecting ? 'warn' : ''))">
          <span class="dot" :class="ui.error ? 'err' : (ui.connected ? 'ok' : (ui.connecting ? 'warn' : ''))"></span>
          {{ statusText }}
        </span>
        <span v-if="ui.connected && !micEnabled" class="badge warn">Mic muted</span>
        <span v-if="ui.connected" class="badge" title="Session length - realtime audio is billed per minute">
          {{ elapsedLabel }}
        </span>
        <span class="spacer"></span>
        <span class="badge">Tools {{ (realtime as any)?.status?.value?.toolsCount ?? 0 }}</span>
        <span class="badge" v-if="ui.useSupervisor">
          supervisor: {{ appSettings.quick_prompt_model || appSettings.openai_chat_model || 'default' }} @ {{ appSettings.temperature ?? 'n/a' }}
        </span>
      </div>

      <p v-if="ui.error" class="field-hint error">{{ ui.error }}</p>

      <div class="divider"></div>

      <label class="switch row">
        <input type="checkbox" v-model="ui.enableTools" />
        <span class="switch-text">
          <span class="switch-label">Enable MCP tools</span>
          <span class="switch-hint">Exposes your connected MCP servers to the voice session.</span>
        </span>
      </label>

      <label class="switch row">
        <input type="checkbox" v-model="ui.useSupervisor" />
        <span class="switch-text">
          <span class="switch-label">Use supervisor agent</span>
          <span class="switch-hint">
            Routes tool decisions through the Prompt-section model instead of letting the realtime model call them directly.
          </span>
        </span>
      </label>

      <p class="field-hint">The microphone is captured only while connected. Replies play automatically.</p>
    </div>
  </section>

  <CollapsibleCard
    id="assistant.config"
    title="Session configuration"
    desc="Applied live - changing anything here while connected updates the running session."
  >
    <div class="field-grid">
      <div class="field">
        <label class="field-label">Model</label>
        <select class="input" v-model="session.model">
          <option v-for="m in models" :key="m" :value="m">{{ m }}</option>
        </select>
        <p class="field-hint">Applied when you connect. Restart the session to change it.</p>
      </div>

      <div class="field">
        <label class="field-label">Voice</label>
        <select class="input" v-model="session.voice">
          <option v-for="v in voices" :key="v" :value="v">{{ v }}</option>
        </select>
        <p class="field-hint">Fixed for the life of a session. Restart to switch voice.</p>
      </div>

      <div class="field" v-if="ui.useSupervisor">
        <label class="field-label">Supervisor mode</label>
        <select class="input" v-model="session.supervisorMode">
          <option value="always">Always</option>
          <option value="needed">Only when needed</option>
        </select>
      </div>

      <div class="field">
        <label class="field-label">Reasoning effort</label>
        <select class="input" v-model="session.reasoningEffort" :disabled="!reasoningSupported" @change="syncSession">
          <option :value="null">Model default</option>
          <option v-for="e in REASONING_EFFORTS" :key="e" :value="e">{{ e }}</option>
        </select>
        <p class="field-hint">
          {{ reasoningSupported
            ? 'Higher effort thinks longer before speaking, at the cost of latency.'
            : 'Only the gpt-realtime-2.x models accept this; it is not sent for ' + session.model + '.' }}
        </p>
      </div>

      <div class="field">
        <label class="field-label">Silence before reply</label>
        <input class="input" type="number" min="0" step="50" v-model.number="session.silenceDurationMs" @change="syncSession" />
        <p class="field-hint">Milliseconds of quiet that end your turn.</p>
      </div>

      <div class="field">
        <label class="field-label">Idle turn timeout</label>
        <input
          class="input"
          type="number"
          min="0"
          max="30000"
          step="100"
          :value="session.idleTimeoutMs ?? ''"
          @change="(e:any) => { const v = e?.target?.value; session.idleTimeoutMs = v === '' ? null : Number(v); syncSession() }"
        />
        <p class="field-hint">Milliseconds of quiet after which the model takes its turn anyway. Blank for none; the API maximum is 30000.</p>
      </div>

      <div class="field">
        <label class="field-label">Close session after</label>
        <input class="input" type="number" min="0" step="1" v-model.number="session.autoCloseMinutes" @change="syncSession" />
        <p class="field-hint">Minutes of silence before the session disconnects. 0 never closes - but an open session holds a live microphone and bills per minute.</p>
      </div>
    </div>

    <div class="field">
      <label class="field-label">Instructions</label>
      <textarea class="input" rows="4" v-model="session.instructions" @change="syncSession" placeholder="Custom system instructions..." />
    </div>

    <label class="switch row">
      <input type="checkbox" v-model="session.inputAudioNoiseReduction" @change="syncSession" />
      <span class="switch-text">
        <span class="switch-label">Input audio noise reduction</span>
        <span class="switch-hint">Server-side cleanup tuned for a close microphone.</span>
      </span>
    </label>
  </CollapsibleCard>

  <CollapsibleCard
    v-if="rateLimits?.length"
    id="assistant.ratelimits"
    title="Rate limits"
    :desc="rateLimits.length + ' reported by the API'"
    :default-open="false"
  >
    <dl class="rates">
      <template v-for="(r, i) in rateLimits" :key="i">
        <dt>{{ r.name }}</dt>
        <dd>{{ r.remaining }} / {{ r.limit }} left, resets in {{ r.reset_seconds }}s</dd>
      </template>
    </dl>
  </CollapsibleCard>

  <CollapsibleCard
    v-if="transcript.length"
    id="assistant.transcript"
    title="Transcript"
    :desc="transcript.length + ' turns'"
  >
    <ol class="turns">
      <li v-for="(t, i) in transcript" :key="i" class="turn" :class="t.role">
        <span class="turn-who">{{ t.role === 'user' ? 'You' : (t.role === 'tool' ? 'Tool' : 'Assistant') }}</span>
        <span class="turn-text">{{ t.content }}</span>
      </li>
    </ol>

    <div class="field">
      <label class="field-label">Plain text</label>
      <textarea class="input" rows="6" v-model="transcriptText"></textarea>
      <p class="field-hint">Editable before you send it anywhere. Tool calls are left out.</p>
    </div>

    <div class="actions">
      <button class="btn ghost" type="button" :disabled="!transcriptText.trim()" @click="copyTranscript">Copy</button>
      <button class="btn" type="button" :disabled="!transcriptText.trim()" @click="insertTranscript">
        Insert into previous app
      </button>
    </div>
  </CollapsibleCard>

  <CollapsibleCard
    id="assistant.debug"
    title="Diagnostics"
    desc="Raw audio element and the realtime event log."
    :default-open="false"
  >
    <div class="field">
      <label class="field-label">Audio output</label>
      <audio ref="remoteAudioElRef" controls></audio>
      <p class="field-hint">Exposed for troubleshooting; playback happens whether or not this is visible.</p>
    </div>

    <label class="switch row">
      <input type="checkbox" v-model="ui.showDebug" />
      <span class="switch-text">
        <span class="switch-label">Record the realtime event log</span>
      </span>
    </label>

    <div
      v-if="ui.showDebug"
      class="log-box"
      ref="debugLogBoxRef"
      @scroll="(e:any) => { const el = e?.target as HTMLElement; if (el) updateDebugAutoScrollFromEl(el) }"
    >
      <div v-for="(l, i) in debugLines" :key="i" class="log-line">{{ l }}</div>
      <div ref="debugLogBottomRef" style="height: 1px;"></div>
    </div>
  </CollapsibleCard>
</template>

<style scoped>
.warn-banner {
  margin: 0;
  padding: var(--sp-2) var(--sp-3);
  border-radius: var(--radius-sm);
  border: 1px solid var(--adc-warn-border);
  background: var(--adc-warn-bg);
  color: var(--adc-warn-fg);
  font-size: var(--fs-sm);
  line-height: 1.5;
}

.rates {
  display: grid;
  grid-template-columns: minmax(120px, max-content) 1fr;
  gap: var(--sp-1) var(--sp-3);
  margin: 0;
  font-size: var(--fs-sm);
}
.rates dt { color: var(--adc-fg); font-family: var(--font-mono); overflow-wrap: anywhere; }
.rates dd { margin: 0; color: var(--adc-fg-muted); font-variant-numeric: tabular-nums; }

audio { width: 100%; height: 36px; border-radius: var(--radius-sm); }

.log-box {
  max-height: 300px;
  overflow: auto;
  padding: var(--sp-3);
  border: 1px solid var(--adc-border);
  border-radius: var(--radius-sm);
  background: var(--adc-bg);
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  line-height: 1.6;
  user-select: text;
}
.log-line { white-space: pre-wrap; overflow-wrap: anywhere; color: var(--adc-fg-muted); }

.turns {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  max-height: 420px;
  overflow: auto;
  user-select: text;
}
.turn {
  display: grid;
  /* A fixed speaker column keeps the text left-aligned down the page rather
     than stepping in and out with the length of each label. */
  grid-template-columns: 72px 1fr;
  gap: var(--sp-3);
  font-size: var(--fs-sm);
  line-height: 1.55;
}
.turn-who {
  color: var(--adc-fg-muted);
  font-size: var(--fs-xs);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  padding-top: 2px;
}
.turn.user .turn-who { color: var(--adc-accent); }
.turn-text { color: var(--adc-fg); overflow-wrap: anywhere; white-space: pre-wrap; }
.turn.tool .turn-text {
  font-family: var(--font-mono);
  font-size: var(--fs-xs);
  color: var(--adc-fg-muted);
}
</style>
