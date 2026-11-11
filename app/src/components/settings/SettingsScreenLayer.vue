<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// The shared windows-commander instance (screen_layer.rs): the app starts it
// at launch and never stops it on exit, because other clients (Claude Code,
// Hermes) use the same instance. Stopping is this card's button only.

const props = defineProps<{ settings: any }>()

interface Status {
  autostart: boolean
  exe_path: string
  exe_found: boolean
  running: boolean
  version: string | null
  url: string | null
  pid: number | null
  detail: string
}

const status = ref<Status | null>(null)
const busy = ref(false)
const error = ref<string | null>(null)

const state = computed<'ok' | 'warn' | 'err' | ''>(() => {
  const s = status.value
  if (!s) return ''
  if (!s.exe_found) return 'err'
  return s.running ? 'ok' : 'warn'
})

const stateLabel = computed(() => {
  const s = status.value
  if (!s) return 'checking'
  if (!s.exe_found) return 'exe not found'
  return s.running ? 'running' : 'not running'
})

async function run(cmd: 'screen_layer_status' | 'screen_layer_start' | 'screen_layer_stop') {
  busy.value = true
  error.value = null
  try {
    status.value = await invoke<Status>(cmd)
  } catch (e: any) {
    error.value = typeof e === 'string' ? e : (e?.message || String(e))
    // Show what is true now, whatever the button did.
    try { status.value = await invoke<Status>('screen_layer_status') } catch {}
  } finally {
    busy.value = false
  }
}

onMounted(() => { void run('screen_layer_status') })
</script>

<template>
  <section class="card">
    <div class="card-head">
      <span class="card-heading">
        <span class="card-title">Screen layer</span>
        <span class="card-desc">The shared windows-commander instance: screen annotations, activity timeline, pointer context.</span>
      </span>
      <span class="badge" :class="state">
        <span class="dot" :class="state"></span>
        {{ stateLabel }}
      </span>
    </div>
    <div class="card-body">
      <dl class="facts" v-if="status">
        <dt>Executable</dt>
        <dd class="mono">{{ status.exe_path }}</dd>
        <template v-if="status.running">
          <dt>Version</dt>
          <dd>{{ status.version || 'unknown' }}<template v-if="status.pid"> (pid {{ status.pid }})</template></dd>
          <dt>URL</dt>
          <dd class="mono">{{ status.url || 'unknown' }}</dd>
        </template>
      </dl>
      <p v-if="status?.detail && !status.running" class="field-hint">{{ status.detail }}</p>
      <p v-if="error" class="field-hint error">{{ error }}</p>

      <label class="switch row">
        <input type="checkbox" v-model="props.settings.screen_layer_autostart" />
        <span class="switch-text">
          <span class="switch-label">Start the shared instance with the app</span>
          <span class="switch-hint">
            At launch the app checks <code>--status</code> and starts <code>--http</code> in the background when
            nothing runs. It is left running when the app exits, because other clients share it.
          </span>
        </span>
      </label>

      <div class="field">
        <label class="field-label">Executable</label>
        <input
          class="input"
          v-model="props.settings.screen_layer_exe_path"
          placeholder="%LOCALAPPDATA%\WindowsCommander\server\WindowsCommander.McpServer.exe"
          spellcheck="false"
        />
        <p class="field-hint">Empty uses the default location.</p>
      </div>

      <div class="actions">
        <button class="btn ghost" type="button" :disabled="busy" @click="run('screen_layer_status')">Refresh</button>
        <button class="btn" type="button" :disabled="busy || !status?.exe_found || status?.running" @click="run('screen_layer_start')">Start</button>
        <span class="spacer"></span>
        <button
          class="btn danger"
          type="button"
          :disabled="busy || !status?.running"
          title="Every client connected to it (Claude Code, Hermes, this app) loses its connection"
          @click="run('screen_layer_stop')"
        >Stop shared instance</button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.facts {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: var(--sp-1) var(--sp-3);
  margin: 0;
  font-size: var(--fs-sm);
}
.facts dt { color: var(--adc-fg-muted); }
.facts dd { margin: 0; overflow-wrap: anywhere; }
.mono { font-family: var(--font-mono); }
</style>
