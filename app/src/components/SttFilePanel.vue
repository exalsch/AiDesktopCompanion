<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { useSettings } from '../composables/useSettings'
import { useSttFileJob, speakerLabel, clock, type ExportFormat } from '../composables/useSttFileJob'

const emit = defineEmits<{
  (e: 'use-as-prompt', text: string): void
}>()

const props = defineProps<{ notify?: (msg: string, kind?: 'error' | 'success', ms?: number) => void }>()

const { settings } = useSettings()
const job = useSttFileJob()
const { state, turns } = job

const AUDIO_EXTENSIONS = ['mp3', 'm4a', 'mp4', 'aac', 'wav', 'flac', 'ogg', 'oga']

onMounted(() => {
  // First visit: follow whichever local model Settings has picked.
  if (!state.engine) {
    state.engine = String(settings.stt_local_model || '').toLowerCase().includes('parakeet') ? 'parakeet' : 'whisper'
  }
  if (!state.diarizer.checked) job.refreshDiarizerStatus()
})

const fileName = computed(() => state.path.split(/[\\/]/).pop() || '')

async function onChooseFile() {
  try {
    const picked = await openDialog({
      multiple: false,
      directory: false,
      title: 'Choose an audio file',
      filters: [{ name: 'Audio', extensions: AUDIO_EXTENSIONS }, { name: 'All files', extensions: ['*'] }],
    })
    if (typeof picked === 'string' && picked) {
      state.path = picked
      state.error = ''
    }
  } catch (e: any) {
    props.notify?.(e?.message || String(e) || 'Could not open the file dialog', 'error')
  }
}

async function onTranscribe() {
  const outcome = await job.start()
  if (outcome === 'cancelled') props.notify?.('Transcription stopped', 'success', 1500)
  else if (outcome === 'error' && state.error) props.notify?.(state.error, 'error')
  else if (outcome === 'ok' && state.result && state.result.segments.length === 0) {
    props.notify?.('No speech found in this file', 'error')
  }
}

function percent(received: number, total: number): string {
  if (!total) return ''
  return `${Math.min(100, Math.round((received / total) * 100))}%`
}

const progressText = computed(() => {
  const p = state.progress
  if (state.stopping) return 'Stopping…'
  if (!p) return 'Starting…'
  switch (p.stage) {
    case 'decoding': return 'Reading the audio file…'
    case 'model': return 'Loading the speech model. The first run downloads it…'
    case 'speaker-model': {
      const pct = percent(state.diarizer.received, state.diarizer.total)
      return pct ? `Downloading the speaker model… ${pct}` : 'Loading the speaker model…'
    }
    case 'transcribing': return `Transcribing part ${p.chunk + 1} of ${p.chunks} (${clock(p.duration_s)} of audio)`
    case 'speakers': return 'Telling the speakers apart…'
    default: return 'Working…'
  }
})

/** 0..1 for the bar, or null for an indeterminate one. */
const progressValue = computed(() => {
  const p = state.progress
  if (!p || p.stage !== 'transcribing' || !p.chunks) return null
  return p.chunk / p.chunks
})

const resultDesc = computed(() => {
  const r = state.result
  if (!r) return ''
  const engine = r.engine === 'parakeet' ? 'Parakeet V3' : 'Whisper'
  const parts = [clock(r.duration_s), engine]
  if (r.diarized) parts.push(r.speaker_count === 1 ? '1 speaker' : `${r.speaker_count} speakers`)
  return parts.join(' · ')
})

const speakerIds = computed(() => {
  const r = state.result
  if (!r || !r.diarized) return [] as number[]
  return Array.from({ length: r.speaker_count }, (_, i) => i)
})

async function onCopy() {
  try {
    await navigator.clipboard.writeText(job.plainText())
    props.notify?.('Copied to clipboard', 'success', 1200)
  } catch {
    props.notify?.('Copy failed', 'error')
  }
}

function onUseAsPrompt() {
  const t = job.plainText().trim()
  if (!t) { props.notify?.('Nothing to use', 'error'); return }
  emit('use-as-prompt', t)
}

async function onSave() {
  const r = state.result
  if (!r) return
  const base = r.file_name.replace(/\.[^.]+$/, '') || 'transcript'
  try {
    const dest = await saveDialog({
      title: 'Save transcript as…',
      defaultPath: `${base}.txt`,
      filters: [
        { name: 'Text', extensions: ['txt'] },
        { name: 'Markdown', extensions: ['md'] },
        { name: 'Subtitles (SRT)', extensions: ['srt'] },
      ],
    })
    if (!dest) return
    const ext = (dest.split('.').pop() || '').toLowerCase()
    const format: ExportFormat = ext === 'md' || ext === 'srt' ? ext : 'txt'
    await job.saveAs(format, dest)
    props.notify?.('Transcript saved', 'success', 1500)
  } catch (e: any) {
    props.notify?.(e?.message || String(e) || 'Save failed', 'error')
  }
}
</script>

<template>
  <section class="card">
    <div class="card-body">
      <div class="field">
        <label class="field-label">Audio file</label>
        <div class="actions">
          <button class="btn ghost" type="button" :disabled="state.running" @click="onChooseFile">Choose file…</button>
          <span class="file-name" :title="state.path">{{ fileName || 'No file chosen' }}</span>
        </div>
        <p class="field-hint">MP3, M4A, MP4 audio, AAC, WAV, FLAC or OGG. Everything runs on this machine.</p>
      </div>

      <div class="field">
        <label class="field-label">Engine</label>
        <select v-model="state.engine" class="input w-md" :disabled="state.running">
          <option value="parakeet">Parakeet V3 (fast, 25 languages)</option>
          <option value="whisper">Whisper (model chosen in Settings)</option>
        </select>
      </div>

      <label class="switch row">
        <input type="checkbox" v-model="state.diarize" :disabled="state.running" />
        <span class="switch-text">
          <span class="switch-label">Identify speakers</span>
          <span class="switch-hint">Labels who is talking, for up to 4 different voices. Parakeet splits speakers more cleanly than Whisper.</span>
        </span>
      </label>

      <div v-if="state.diarize && state.diarizer.checked && !state.diarizer.downloaded" class="actions">
        <span class="field-hint">The speaker model is a one-time download of about 490 MB.</span>
        <button
          class="btn ghost sm"
          type="button"
          :disabled="state.diarizer.downloading || state.running"
          @click="job.downloadDiarizer()"
        >{{ state.diarizer.downloading
          ? `Downloading… ${percent(state.diarizer.received, state.diarizer.total)}`
          : 'Download now' }}</button>
      </div>
      <p v-if="state.diarizer.error" class="field-hint error">{{ state.diarizer.error }}</p>

      <div class="divider"></div>

      <div class="actions">
        <button
          v-if="!state.running"
          class="btn"
          type="button"
          :disabled="!state.path || !state.engine"
          @click="onTranscribe"
        >Transcribe</button>
        <button
          v-else
          class="btn ghost"
          type="button"
          :disabled="state.stopping"
          @click="job.cancel()"
        >{{ state.stopping ? 'Stopping…' : 'Stop' }}</button>
        <span v-if="state.running" class="field-hint">{{ progressText }}</span>
      </div>
      <progress
        v-if="state.running"
        class="file-progress"
        :value="progressValue ?? undefined"
        max="1"
      ></progress>

      <p v-if="state.error" class="field-hint error">{{ state.error }}</p>
    </div>
  </section>

  <section class="card" v-if="state.result && state.result.segments.length">
    <div class="card-head">
      <span class="card-heading">
        <span class="card-title">{{ state.result.file_name }}</span>
        <span class="card-desc">{{ resultDesc }}</span>
      </span>
      <span class="actions">
        <button class="btn ghost sm" type="button" @click="onCopy">Copy</button>
        <button class="btn ghost sm" type="button" @click="onSave">Save as…</button>
        <button class="btn sm" type="button" @click="onUseAsPrompt">Use as prompt</button>
      </span>
    </div>
    <div class="card-body">
      <div v-if="speakerIds.length" class="field">
        <label class="field-label">Speaker names</label>
        <div class="speaker-names">
          <input
            v-for="n in speakerIds"
            :key="n"
            v-model="state.speakerNames[n]"
            class="input sm"
            type="text"
            :placeholder="`Speaker ${n + 1}`"
            :aria-label="`Name for speaker ${n + 1}`"
          />
        </div>
      </div>

      <div class="transcript">
        <div v-for="(t, i) in turns" :key="i" class="turn">
          <div class="turn-meta">
            <span class="turn-time">{{ clock(t.start) }}</span>
            <span
              v-if="state.result.diarized"
              class="turn-speaker"
              :data-speaker="t.speaker ?? 'none'"
            >{{ speakerLabel(t.speaker) }}</span>
          </div>
          <p class="turn-text">{{ t.text }}</p>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.file-name {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--adc-fg-muted);
}

.file-progress {
  width: 100%;
  height: 6px;
  accent-color: var(--adc-accent);
}

.speaker-names {
  display: flex;
  flex-wrap: wrap;
  gap: var(--sp-2);
}
.speaker-names .input { width: 12rem; }

/* The transcript can run to an hour of speech. It scrolls inside the card so
   the actions above stay in reach. */
.transcript {
  max-height: 60vh;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  padding-right: var(--sp-2);
}

.turn-meta {
  display: flex;
  align-items: baseline;
  gap: var(--sp-2);
  font-size: var(--fs-sm);
}
.turn-time {
  color: var(--adc-fg-muted);
  font-variant-numeric: tabular-nums;
}
.turn-speaker {
  font-weight: 600;
  color: var(--adc-accent);
}
.turn-speaker[data-speaker="1"] { color: var(--adc-ok-fg); }
.turn-speaker[data-speaker="2"] { color: var(--adc-warn-fg); }
.turn-speaker[data-speaker="none"] { color: var(--adc-fg-muted); font-weight: 500; }

.turn-text {
  margin: 2px 0 0;
  line-height: 1.5;
  white-space: pre-wrap;
  user-select: text;
}
</style>
