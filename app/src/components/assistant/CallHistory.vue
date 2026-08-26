<script setup lang="ts">
/**
 * Past voice calls: when, how long, what it cost, and what was said.
 *
 * A realtime call is billed per audio token, so the only honest answer to "what
 * am I spending on this" is a record of the calls themselves. The transcript
 * comes along because it is already in memory when the call ends and is the
 * thing that makes a row recognisable a week later.
 *
 * Rows are collapsed by default. A transcript can be thousands of words and
 * twenty of them stacked would bury the list this card exists to show.
 */
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import CollapsibleCard from '../ui/CollapsibleCard.vue'
import {
  useCallHistory,
  formatDuration,
  formatUsd,
  formatWhen,
  parseTranscript,
  transcriptToText,
  type CallEntry,
} from '../../composables/useCallHistory'

const props = defineProps<{
  notify: (msg: string, kind?: 'error' | 'success', ms?: number) => void
  /** Whether calls are recorded at all. Persisted by the parent. */
  recording: boolean
  /** One of keep_all | days_7 | days_30 | months_3 | last_50. */
  retention: string
}>()

const emit = defineEmits<{
  (e: 'update:recording', value: boolean): void
  (e: 'update:retention', value: string): void
}>()

const history = useCallHistory()

/** Ids whose transcript is unfolded. */
const expanded = ref<Set<number>>(new Set())

function toggleExpanded(id: number) {
  const next = new Set(expanded.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  expanded.value = next
}

const RETENTION_OPTIONS = [
  { value: 'days_7', label: 'Keep for 7 days' },
  { value: 'days_30', label: 'Keep for 30 days' },
  { value: 'months_3', label: 'Keep for 3 months' },
  { value: 'last_50', label: 'Keep the last 50 calls' },
  { value: 'keep_all', label: 'Keep everything' },
]

const summary = computed(() => {
  const s = history.stats.value
  if (!s || s.calls === 0) return 'Nothing recorded yet'
  const spend = formatUsd(s.cost_usd)
  const parts = [`${s.calls} call${s.calls === 1 ? '' : 's'}`, formatDuration(s.duration_ms)]
  if (spend) parts.push(`~${spend}${s.unpriced > 0 ? '+' : ''}`)
  return parts.join(' · ')
})

/**
 * Warn when the total is known to be incomplete rather than quietly showing a
 * smaller number. A call on a model with no rate table is not a free call.
 */
const summaryTitle = computed(() => {
  const s = history.stats.value
  if (!s) return ''
  const lines = [`${s.calls} recorded calls, ${formatDuration(s.duration_ms)} in total.`]
  if (s.unpriced > 0) {
    lines.push(
      `${s.unpriced} of them ran on a model with no known rates and are not included in the total.`,
    )
  }
  lines.push('Cost is an estimate. Token counts are exact.')
  return lines.join('\n')
})

function turnsOf(entry: CallEntry) {
  return parseTranscript(entry.transcript)
}

function tokenLine(entry: CallEntry): string {
  const n = (v: number) => v.toLocaleString()
  return [
    `Audio in ${n(entry.audio_in)} (+${n(entry.audio_in_cached)} cached)`,
    `Audio out ${n(entry.audio_out)}`,
    `Text in ${n(entry.text_in)} (+${n(entry.text_in_cached)} cached)`,
    `Text out ${n(entry.text_out)}`,
    `${entry.responses} response${entry.responses === 1 ? '' : 's'}`,
  ].join(' · ')
}

async function copyOne(entry: CallEntry) {
  const text = transcriptToText(turnsOf(entry))
  if (!text.trim()) { props.notify('Nothing was said on that call', 'error'); return }
  try {
    await invoke('copy_text_to_clipboard', { text })
    props.notify('Transcript copied', 'success')
  } catch (e: any) {
    props.notify(e?.message || 'Copy failed', 'error')
  }
}

/**
 * Paste a past transcript into whatever the user was last working in.
 *
 * The same dance the live panel does: this window holds focus while the button
 * is clicked, so the previous application has to be brought forward first. The
 * clipboard copy happens regardless, so the text is never lost if the paste
 * does not land.
 */
async function insertOne(entry: CallEntry) {
  const text = transcriptToText(turnsOf(entry)).trim()
  if (!text) { props.notify('Nothing was said on that call', 'error'); return }
  try { await invoke('copy_text_to_clipboard', { text }) } catch {}
  try {
    await invoke('refocus_previous_app')
    await new Promise((r) => setTimeout(r, 80))
    await invoke('insert_text_into_focused_app', { text })
  } catch (e: any) {
    props.notify((e?.message || 'Insert failed') + ' - the text is on the clipboard.', 'error')
  }
}

/**
 * Deleting is not undoable, so it asks - but only once per press, and the
 * question names what is about to go.
 */
const confirmingClear = ref<'' | 'unpinned' | 'all'>('')

async function doClear(which: 'unpinned' | 'all') {
  if (confirmingClear.value !== which) { confirmingClear.value = which; return }
  confirmingClear.value = ''
  const removed = await history.clear(which === 'unpinned')
  props.notify(`Deleted ${removed} call${removed === 1 ? '' : 's'}`, 'success')
}

/** Reload from disk. Exposed so the parent can call it after a call ends. */
async function reload() {
  await history.refresh()
}

defineExpose({ reload })

onMounted(() => { void history.refresh() })
</script>

<template>
  <CollapsibleCard
    id="assistant.history"
    title="Call history"
    :desc="summary"
    :default-open="false"
  >
    <div class="field-grid">
      <div class="field">
        <label class="switch row">
          <input
            type="checkbox"
            :checked="props.recording"
            @change="emit('update:recording', ($event.target as HTMLInputElement).checked)"
          />
          <span class="switch-text">
            <span class="switch-label">Record calls</span>
            <span class="switch-hint">
              Saves the duration, model, token counts, estimated cost and full transcript of each
              call to <code>calls.db</code> beside your settings. Transcripts are stored as plain
              text. Turning this off stops new calls being recorded and leaves what is already
              there.
            </span>
          </span>
        </label>
      </div>

      <div class="field">
        <label class="field-label">Keep calls for</label>
        <select
          class="input"
          :value="props.retention"
          :disabled="!props.recording"
          @change="emit('update:retention', ($event.target as HTMLSelectElement).value)"
        >
          <option v-for="o in RETENTION_OPTIONS" :key="o.value" :value="o.value">{{ o.label }}</option>
        </select>
        <p class="field-hint">
          Applied after every call and once when the app starts. Pinned calls are never removed by
          this.
        </p>
      </div>
    </div>

    <p v-if="history.error.value" class="field-hint error">{{ history.error.value }}</p>

    <div class="divider"></div>

    <p v-if="history.loaded.value && !history.entries.value.length" class="field-hint">
      No calls recorded yet. One appears here as soon as you hang up.
    </p>

    <ol v-else class="calls">
      <li v-for="entry in history.entries.value" :key="entry.id" class="call" :class="{ open: expanded.has(entry.id) }">
        <div class="call-head">
          <button
            type="button"
            class="call-summary"
            :aria-expanded="expanded.has(entry.id)"
            @click="toggleExpanded(entry.id)"
          >
            <svg class="chevron" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="m6 4 4 4-4 4" />
            </svg>
            <span class="call-when">{{ formatWhen(entry.started_at) }}</span>
            <span class="call-title">{{ entry.title }}</span>
            <span class="call-meta">
              <span class="badge">{{ formatDuration(entry.duration_ms) }}</span>
              <span class="badge" :title="tokenLine(entry)">
                {{ formatUsd(entry.cost_usd) ?? 'unpriced' }}
              </span>
            </span>
          </button>

          <button
            type="button"
            class="icon-btn"
            :class="{ on: entry.saved }"
            :title="entry.saved ? 'Pinned - kept regardless of the retention setting' : 'Pin this call so it is never expired'"
            :aria-pressed="entry.saved"
            @click="history.togglePin(entry)"
          >
            <svg viewBox="0 0 16 16" :fill="entry.saved ? 'currentColor' : 'none'" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" aria-hidden="true">
              <path d="M8 1.8 10 6l4.4.5-3.3 3 .9 4.4L8 11.7 4 13.9l.9-4.4-3.3-3L6 6z" />
            </svg>
          </button>

          <button type="button" class="icon-btn danger" title="Delete this call" @click="history.remove(entry)">
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true">
              <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.2h5.8l.6-8.2" />
            </svg>
          </button>
        </div>

        <div v-if="expanded.has(entry.id)" class="call-body">
          <p class="call-facts">
            <span class="badge">{{ entry.model }}</span>
            <span class="badge">{{ entry.voice }}</span>
            <span v-if="entry.tools_enabled" class="badge">MCP tools</span>
            <span v-if="entry.supervisor" class="badge">supervisor: {{ entry.supervisor }}</span>
          </p>
          <p class="call-tokens">{{ tokenLine(entry) }}</p>

          <ol class="turns">
            <li v-for="(t, i) in turnsOf(entry)" :key="i" class="turn" :class="t.role">
              <span class="turn-who">{{ t.role === 'user' ? 'You' : (t.role === 'tool' ? 'Tool' : 'Assistant') }}</span>
              <span class="turn-text">{{ t.content }}</span>
            </li>
          </ol>
          <p v-if="!turnsOf(entry).length" class="field-hint">No transcript was captured for this call.</p>

          <div class="actions">
            <button class="btn ghost" type="button" @click="copyOne(entry)">Copy transcript</button>
            <button class="btn ghost" type="button" @click="insertOne(entry)">Insert into previous app</button>
          </div>
        </div>
      </li>
    </ol>

    <div class="actions" v-if="history.entries.value.length">
      <button
        v-if="history.hasMore.value"
        class="btn ghost"
        type="button"
        :disabled="history.loading.value"
        @click="history.loadMore()"
      >
        {{ history.loading.value ? 'Loading…' : 'Load more' }}
      </button>
      <span class="spacer"></span>
      <!-- Neutral until armed, red once a second click would actually delete.
           The button that is one press from destroying something should not
           look the same as the one that only asks. -->
      <button
        :class="confirmingClear === 'unpinned' ? 'btn danger' : 'btn ghost'"
        type="button"
        @click="doClear('unpinned')"
        @blur="confirmingClear = ''"
      >
        {{ confirmingClear === 'unpinned' ? 'Click again to delete unpinned' : 'Delete unpinned' }}
      </button>
      <button
        :class="confirmingClear === 'all' ? 'btn danger' : 'btn ghost'"
        type="button"
        @click="doClear('all')"
        @blur="confirmingClear = ''"
      >
        {{ confirmingClear === 'all' ? 'Click again to delete everything' : 'Delete all' }}
      </button>
    </div>

    <p class="field-hint" :title="summaryTitle">
      Costs are estimates from a rate table in this app, not from your OpenAI invoice. Token counts
      are exactly what the API reported.
    </p>
  </CollapsibleCard>
</template>

<style scoped>
.calls {
  list-style: none;
  margin: 0 0 var(--sp-3);
  padding: 0;
  display: flex;
  flex-direction: column;
}
.call { border-bottom: 1px solid var(--adc-border); }
.call:last-child { border-bottom: 0; }

.call-head {
  display: flex;
  align-items: center;
  gap: var(--sp-1);
}

.call-summary {
  flex: 1 1 auto;
  min-width: 0;
  display: grid;
  /* Chevron, timestamp, title, then the numbers pinned to the right. The
     timestamp column is fixed so the titles line up down the list instead of
     stepping in and out with the length of each date. */
  grid-template-columns: 16px minmax(0, max-content) minmax(0, 1fr) max-content;
  align-items: center;
  gap: var(--sp-2);
  appearance: none;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  padding: var(--sp-2) var(--sp-1);
  cursor: pointer;
  border-radius: var(--radius-sm);
}
.call-summary:hover { background: var(--adc-hover); }
.call-summary:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--adc-accent); }

.chevron {
  width: 16px;
  height: 16px;
  color: var(--adc-fg-muted);
  transition: transform 0.18s ease;
}
.open .chevron { transform: rotate(90deg); }

.call-when {
  color: var(--adc-fg-muted);
  font-size: var(--fs-xs);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
}
.call-title {
  color: var(--adc-fg);
  font-size: var(--fs-sm);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.call-meta {
  display: flex;
  gap: var(--sp-1);
  align-items: center;
  font-variant-numeric: tabular-nums;
}

.icon-btn {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  appearance: none;
  border: 0;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--adc-fg-muted);
  cursor: pointer;
}
.icon-btn svg { width: 15px; height: 15px; }
.icon-btn:hover { background: var(--adc-hover); color: var(--adc-fg); }
.icon-btn:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--adc-accent); }
.icon-btn.on { color: var(--adc-accent); }
.icon-btn.danger:hover { color: var(--adc-danger); }

.call-body {
  padding: 0 var(--sp-1) var(--sp-3) calc(16px + var(--sp-2) + var(--sp-1));
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}
.call-facts { display: flex; flex-wrap: wrap; gap: var(--sp-1); margin: 0; }
.call-tokens {
  margin: 0;
  color: var(--adc-fg-muted);
  font-size: var(--fs-xs);
  font-variant-numeric: tabular-nums;
  overflow-wrap: anywhere;
}

.turns {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: var(--sp-2);
  max-height: 320px;
  overflow: auto;
  user-select: text;
}
.turn {
  display: grid;
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

@media (prefers-reduced-motion: reduce) {
  .chevron { transition: none; }
}
</style>
