import { reactive, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

/**
 * Audio file transcription job. State lives at module level, not in the
 * panel, so a long file keeps running and its result stays put while the user
 * looks at another section. See specs/STT_FileTranscription_design.md.
 */

export type FileEngine = 'parakeet' | 'whisper'

export type FileSegment = { start: number, end: number, speaker: number | null, text: string }

export type FileTranscript = {
  file_name: string
  duration_s: number
  engine: FileEngine
  diarized: boolean
  speaker_count: number
  segments: FileSegment[]
}

export type Turn = { start: number, end: number, speaker: number | null, text: string }

type Progress = { stage: string, chunk: number, chunks: number, duration_s: number }
type DownloadEvent = { kind: 'progress' | 'done', received?: number, total?: number }

/** Must match `stt_file::CANCELLED_MESSAGE` on the Rust side. */
export const CANCELLED_MESSAGE = 'Transcription cancelled'

const PREFS_KEY = 'adc.sttFile.prefs'

function loadPrefs(): { engine?: FileEngine, diarize?: boolean } {
  try { return JSON.parse(localStorage.getItem(PREFS_KEY) || '{}') || {} } catch { return {} }
}

const prefs = loadPrefs()

const state = reactive({
  path: '' as string,
  engine: (prefs.engine || '') as FileEngine | '',
  diarize: prefs.diarize === true,
  running: false,
  stopping: false,
  progress: null as Progress | null,
  error: '' as string,
  result: null as FileTranscript | null,
  /** Speaker number -> name the user typed. Missing means "Speaker N". */
  speakerNames: {} as Record<number, string>,
  diarizer: { checked: false, downloaded: false, downloading: false, received: 0, total: 0, error: '' },
})

let listening = false
async function ensureListeners() {
  if (listening) return
  listening = true
  try {
    await listen<Progress>('stt-file:progress', (e) => { if (state.running) state.progress = e.payload })
    await listen<DownloadEvent>('stt-diarizer-download', (e) => {
      const p = e.payload || ({} as DownloadEvent)
      if (p.kind === 'progress') {
        state.diarizer.received = Number(p.received || 0)
        state.diarizer.total = Number(p.total || 0)
      } else if (p.kind === 'done') {
        state.diarizer.downloaded = true
      }
    })
  } catch (err) {
    listening = false
    console.warn('[stt-file] listen failed', err)
  }
}

function savePrefs() {
  try { localStorage.setItem(PREFS_KEY, JSON.stringify({ engine: state.engine, diarize: state.diarize })) } catch {}
}

async function refreshDiarizerStatus() {
  try {
    const s = await invoke<{ downloaded: boolean, path: string }>('stt_diarizer_status')
    state.diarizer.downloaded = !!s?.downloaded
    state.diarizer.error = ''
  } catch (e: any) {
    state.diarizer.error = e?.message || String(e)
  } finally {
    state.diarizer.checked = true
  }
}

async function downloadDiarizer() {
  if (state.diarizer.downloading) return
  await ensureListeners()
  state.diarizer.downloading = true
  state.diarizer.error = ''
  state.diarizer.received = 0
  state.diarizer.total = 0
  try {
    await invoke('stt_prefetch_diarizer_model')
    state.diarizer.downloaded = true
  } catch (e: any) {
    state.diarizer.error = e?.message || String(e) || 'Download failed'
  } finally {
    state.diarizer.downloading = false
  }
}

async function start(): Promise<'ok' | 'cancelled' | 'error'> {
  if (state.running || !state.path || !state.engine) return 'error'
  await ensureListeners()
  savePrefs()
  state.running = true
  state.stopping = false
  state.error = ''
  state.progress = null
  try {
    const r = await invoke<FileTranscript>('stt_file_transcribe', {
      path: state.path,
      engine: state.engine,
      diarize: state.diarize,
    })
    state.result = r
    state.speakerNames = {}
    if (r.diarized) state.diarizer.downloaded = true
    return 'ok'
  } catch (e: any) {
    const msg = e?.message || String(e) || 'Transcription failed'
    if (msg === CANCELLED_MESSAGE) return 'cancelled'
    state.error = msg
    return 'error'
  } finally {
    state.running = false
    state.stopping = false
    state.progress = null
  }
}

async function cancel() {
  if (!state.running || state.stopping) return
  state.stopping = true
  try { await invoke('stt_file_cancel') } catch {}
}

// ---- Presentation ----

export function speakerLabel(n: number | null): string {
  if (n === null || n === undefined) return 'Unknown'
  const named = (state.speakerNames[n] || '').trim()
  return named || `Speaker ${n + 1}`
}

/** `m:ss`, or `h:mm:ss` from an hour on. */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds))
  const h = Math.floor(s / 3600)
  const m = Math.floor((s % 3600) / 60)
  const sec = String(s % 60).padStart(2, '0')
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`
}

/**
 * Consecutive segments merged into readable blocks: a new block on every
 * change of speaker. Without speakers, a pause of 2 s or a block reaching 45 s
 * starts a new paragraph, so the text does not arrive as one wall.
 */
export function toTurns(t: FileTranscript): Turn[] {
  const turns: Turn[] = []
  for (const seg of t.segments) {
    const last = turns[turns.length - 1]
    const join = last && (t.diarized
      ? last.speaker === seg.speaker
      : seg.start - last.end < 2 && seg.end - last.start <= 45)
    if (join) {
      last.end = seg.end
      last.text += ' ' + seg.text
    } else {
      turns.push({ start: seg.start, end: seg.end, speaker: seg.speaker, text: seg.text })
    }
  }
  return turns
}

const turns = computed(() => (state.result ? toTurns(state.result) : []))

function srtTime(seconds: number): string {
  const ms = Math.max(0, Math.round(seconds * 1000))
  const h = Math.floor(ms / 3_600_000)
  const m = Math.floor((ms % 3_600_000) / 60_000)
  const s = Math.floor((ms % 60_000) / 1000)
  const pad = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${pad(h)}:${pad(m)}:${pad(s)},${pad(ms % 1000, 3)}`
}

export type ExportFormat = 'txt' | 'md' | 'srt'

function render(format: ExportFormat): string {
  const r = state.result
  if (!r) return ''
  const who = (n: number | null) => (r.diarized ? `${speakerLabel(n)}: ` : '')
  if (format === 'srt') {
    return r.segments
      .map((s, i) => `${i + 1}\n${srtTime(s.start)} --> ${srtTime(s.end)}\n${who(s.speaker)}${s.text}\n`)
      .join('\n')
  }
  if (format === 'md') {
    const head = `# ${r.file_name}\n\n`
    return head + turns.value
      .map(t => r.diarized
        ? `**${speakerLabel(t.speaker)}** [${clock(t.start)}]\n\n${t.text}\n`
        : `[${clock(t.start)}] ${t.text}\n`)
      .join('\n')
  }
  return turns.value.map(t => `[${clock(t.start)}] ${who(t.speaker)}${t.text}`).join('\n\n') + '\n'
}

/** Plain text without timestamps, for the clipboard and "Use as prompt". */
function plainText(): string {
  const r = state.result
  if (!r) return ''
  return turns.value.map(t => (r.diarized ? `${speakerLabel(t.speaker)}: ${t.text}` : t.text)).join('\n\n')
}

async function saveAs(format: ExportFormat, dest: string) {
  await invoke('stt_file_save_text', { path: dest, contents: render(format) })
}

export function useSttFileJob() {
  return { state, turns, start, cancel, refreshDiarizerStatus, downloadDiarizer, render, plainText, saveAs }
}
