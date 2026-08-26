import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

/**
 * The saved record of past Assistant Mode calls.
 *
 * Backed by SQLite in Rust (`call_history.rs`) rather than a JSON file: calls
 * arrive one at a time and are read back newest-first a page at a time, and the
 * running spend is a SUM rather than a full parse.
 *
 * Field names are snake_case because they are the database columns, and the
 * Rust struct is serialized as declared - only the *arguments* of a Tauri
 * command are camelCased on the way through.
 */
export interface CallEntry {
  id: number
  /** Unix seconds the call connected. */
  started_at: number
  duration_ms: number
  model: string
  voice: string
  /** Pinned. Pinned calls survive every retention sweep. */
  saved: boolean
  title: string
  /** JSON array of `{ role, content }`. */
  transcript: string
  turns: number
  responses: number
  audio_in: number
  audio_in_cached: number
  audio_out: number
  text_in: number
  text_in_cached: number
  text_out: number
  /** null when the model had no known rates - not the same as zero. */
  cost_usd: number | null
  tools_enabled: boolean
  supervisor: string | null
}

/** What the frontend supplies; the id and pin are the store's business. */
export type CallDraft = Omit<CallEntry, 'id' | 'saved'>

export interface CallStats {
  calls: number
  duration_ms: number
  /** Total over the calls that had known rates. */
  cost_usd: number
  /** How many calls could not be priced, so the total is known to be partial. */
  unpriced: number
}

export interface TranscriptTurn { role: string, content: string }

const PAGE_SIZE = 25

/** Longest title kept. Enough to recognise a call, short enough for one line. */
const TITLE_MAX = 90

/**
 * A one-line label for a call, taken from the first thing the user said.
 *
 * Falls back to the assistant's opening line, because a call where the user
 * only listened is still worth finding again.
 */
export function titleFor(turns: TranscriptTurn[]): string {
  const spoken = turns.filter((t) => t.role === 'user' || t.role === 'assistant')
  const first = spoken.find((t) => t.role === 'user') ?? spoken[0]
  const text = String(first?.content ?? '').replace(/\s+/g, ' ').trim()
  if (!text) return 'No speech'
  return text.length > TITLE_MAX ? text.slice(0, TITLE_MAX - 1).trimEnd() + '…' : text
}

/** "4:07", or "12s" under a minute. */
export function formatDuration(ms: number): string {
  const total = Math.max(0, Math.round(ms / 1000))
  if (total < 60) return `${total}s`
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${String(s).padStart(2, '0')}`
}

/**
 * "$0.0412" below a cent-ish, "$1.23" above.
 *
 * Four decimals matter here: a short call on the mini model genuinely costs
 * fractions of a cent, and rounding those to $0.00 would make the whole column
 * look free.
 */
export function formatUsd(value: number | null | undefined): string | null {
  if (value === null || value === undefined || !Number.isFinite(value)) return null
  return value < 0.01 ? `$${value.toFixed(4)}` : `$${value.toFixed(2)}`
}

/** Local date and time, to the minute. */
export function formatWhen(startedAtSeconds: number): string {
  const d = new Date(startedAtSeconds * 1000)
  if (Number.isNaN(d.getTime())) return ''
  return d.toLocaleString(undefined, {
    year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  })
}

export function parseTranscript(json: string): TranscriptTurn[] {
  try {
    const parsed = JSON.parse(json)
    if (!Array.isArray(parsed)) return []
    return parsed
      .filter((t: any) => t && typeof t === 'object')
      .map((t: any) => ({ role: String(t.role ?? ''), content: String(t.content ?? '') }))
  } catch {
    // A row written by a future version, or a truncated write. Showing nothing
    // is better than throwing inside a list render.
    return []
  }
}

/** The conversation as plain text, tool calls left out. */
export function transcriptToText(turns: TranscriptTurn[]): string {
  return turns
    .filter((t) => t.role !== 'tool')
    .map((t) => `${t.role === 'user' ? 'You' : 'Assistant'}: ${t.content}`)
    .join('\n\n')
}

export function useCallHistory() {
  const entries = ref<CallEntry[]>([])
  const stats = ref<CallStats | null>(null)
  const hasMore = ref(false)
  const loading = ref(false)
  const error = ref<string | null>(null)
  /** True once the list has been fetched, so an empty list can say "none yet". */
  const loaded = ref(false)

  function fail(e: any, what: string) {
    error.value = typeof e === 'string' ? e : (e?.message || what)
  }

  async function refreshStats() {
    try {
      stats.value = await invoke<CallStats>('call_history_stats')
    } catch (e) {
      // The list is the feature; a missing total should not blank it.
      stats.value = null
    }
  }

  /** Reload the first page, discarding anything already loaded. */
  async function refresh() {
    loading.value = true
    error.value = null
    try {
      const page = await invoke<{ entries: CallEntry[], has_more: boolean }>('call_history_list', {
        cursor: null,
        limit: PAGE_SIZE,
      })
      entries.value = page.entries ?? []
      hasMore.value = page.has_more === true
      loaded.value = true
    } catch (e) {
      fail(e, 'Could not read the call history')
    } finally {
      loading.value = false
    }
    void refreshStats()
  }

  async function loadMore() {
    const last = entries.value[entries.value.length - 1]
    if (!last || loading.value) return
    loading.value = true
    error.value = null
    try {
      const page = await invoke<{ entries: CallEntry[], has_more: boolean }>('call_history_list', {
        cursor: last.id,
        limit: PAGE_SIZE,
      })
      entries.value = entries.value.concat(page.entries ?? [])
      hasMore.value = page.has_more === true
    } catch (e) {
      fail(e, 'Could not read more calls')
    } finally {
      loading.value = false
    }
  }

  /**
   * Record a finished call.
   *
   * Resolves to the new id, or null when recording is switched off in settings -
   * the decision is made in Rust so the caller does not have to read the setting
   * to know whether to bother.
   */
  async function save(draft: CallDraft): Promise<number | null> {
    try {
      const id = await invoke<number | null>('call_history_save', { entry: { ...draft, id: 0, saved: false } })
      return typeof id === 'number' ? id : null
    } catch (e) {
      fail(e, 'Could not save the call')
      return null
    }
  }

  async function togglePin(entry: CallEntry) {
    const next = !entry.saved
    try {
      await invoke('call_history_set_saved', { id: entry.id, saved: next })
      entry.saved = next
    } catch (e) {
      fail(e, 'Could not pin the call')
    }
  }

  async function remove(entry: CallEntry) {
    try {
      await invoke('call_history_delete', { id: entry.id })
      entries.value = entries.value.filter((e) => e.id !== entry.id)
      void refreshStats()
    } catch (e) {
      fail(e, 'Could not delete the call')
    }
  }

  /** Delete everything, or everything not pinned. Returns how many went. */
  async function clear(keepSaved: boolean): Promise<number> {
    try {
      const removed = await invoke<number>('call_history_clear', { keepSaved })
      await refresh()
      return typeof removed === 'number' ? removed : 0
    } catch (e) {
      fail(e, 'Could not clear the call history')
      return 0
    }
  }

  return {
    entries, stats, hasMore, loading, loaded, error,
    refresh, loadMore, save, togglePin, remove, clear,
  }
}
