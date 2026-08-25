import { ref } from 'vue'

/**
 * Token and cost accounting for a Realtime session.
 *
 * Every `response.done` carries a usage block, split by modality and by cached
 * versus fresh input. It was previously discarded, so a call that had cost real
 * money reported nothing at all - and realtime audio is the most expensive
 * thing this app does.
 *
 * Tokens are counted exactly, from what the API reports. Money is an estimate
 * and is labelled as one: the rates below are correct at the time of writing
 * but are not fetched from anywhere, and OpenAI has cut realtime pricing
 * repeatedly. `RATES_CHECKED` is what tells you how far to trust the figure.
 */

/** When the rate table below was last verified against OpenAI's pricing page. */
export const RATES_CHECKED = '2026-08-25'

/** USD per million tokens. */
interface ModelRates {
  audioIn: number
  audioInCached: number
  audioOut: number
  textIn: number
  textInCached: number
  textOut: number
}

/**
 * Rates by model prefix, longest match first.
 *
 * Keyed on prefix because OpenAI ships dated snapshots behind moving aliases
 * (`gpt-realtime-2.1-2026-...`), and a table of exact ids would silently miss
 * every one of them and report a call as free.
 *
 * Audio, cached audio and text rates are taken from OpenAI's published pricing.
 * `textInCached` is the one inferred figure - cached text is priced at a tenth
 * of fresh text, matching the ratio used elsewhere - and it moves the total by
 * almost nothing, because audio dominates a voice call by two orders of
 * magnitude.
 */
const RATE_TABLE: Array<{ prefix: string, rates: ModelRates }> = [
  {
    prefix: 'gpt-realtime-2.1-mini',
    rates: { audioIn: 10, audioInCached: 0.3, audioOut: 20, textIn: 0.6, textInCached: 0.06, textOut: 2.4 },
  },
  {
    prefix: 'gpt-realtime-2.1',
    rates: { audioIn: 32, audioInCached: 0.4, audioOut: 64, textIn: 4, textInCached: 0.4, textOut: 16 },
  },
  {
    prefix: 'gpt-realtime-2',
    rates: { audioIn: 32, audioInCached: 0.4, audioOut: 64, textIn: 4, textInCached: 0.4, textOut: 16 },
  },
  {
    prefix: 'gpt-realtime',
    rates: { audioIn: 32, audioInCached: 0.4, audioOut: 64, textIn: 4, textInCached: 0.4, textOut: 16 },
  },
]

function ratesFor(model?: string): ModelRates | null {
  const m = String(model || '')
  // Longest prefix wins, so the mini is never charged at the flagship rate.
  const sorted = [...RATE_TABLE].sort((a, b) => b.prefix.length - a.prefix.length)
  for (const entry of sorted) {
    if (m.startsWith(entry.prefix)) return entry.rates
  }
  return null
}

export interface UsageTotals {
  audioIn: number
  audioInCached: number
  audioOut: number
  textIn: number
  textInCached: number
  textOut: number
  responses: number
}

function emptyTotals(): UsageTotals {
  return { audioIn: 0, audioInCached: 0, audioOut: 0, textIn: 0, textInCached: 0, textOut: 0, responses: 0 }
}

function num(v: any): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : 0
}

export function useRealtimeUsage() {
  const totals = ref<UsageTotals>(emptyTotals())
  /** null when the session's model has no known rates. */
  const estimatedUsd = ref<number | null>(0)
  const model = ref<string>('')

  function reset(nextModel?: string) {
    totals.value = emptyTotals()
    model.value = nextModel || ''
    estimatedUsd.value = ratesFor(model.value) ? 0 : null
  }

  function recompute() {
    const rates = ratesFor(model.value)
    if (!rates) { estimatedUsd.value = null; return }
    const t = totals.value
    estimatedUsd.value =
      (t.audioIn * rates.audioIn
        + t.audioInCached * rates.audioInCached
        + t.audioOut * rates.audioOut
        + t.textIn * rates.textIn
        + t.textInCached * rates.textInCached
        + t.textOut * rates.textOut) / 1_000_000
  }

  /**
   * Fold one `response.done` usage block into the running totals.
   *
   * Cached input is reported inside the input totals rather than alongside
   * them, so it is subtracted out before the uncached remainder is charged at
   * the full rate - counting both would roughly double the input cost.
   */
  function addResponse(usage: any): boolean {
    if (!usage || typeof usage !== 'object') return false

    const inDetails = usage.input_token_details
    const outDetails = usage.output_token_details
    // Reported as false rather than counted as zero. Silently showing $0.00 for
    // a call that cost money is worse than showing nothing, and this is exactly
    // the shape that would change under us.
    if (!inDetails && !outDetails) return false

    const cached = (inDetails && inDetails.cached_tokens_details) || {}

    const audioInTotal = num(inDetails?.audio_tokens)
    const textInTotal = num(inDetails?.text_tokens)
    const audioInCached = Math.min(num(cached.audio_tokens), audioInTotal)
    const textInCached = Math.min(num(cached.text_tokens), textInTotal)

    const t = { ...totals.value }
    t.audioIn += Math.max(0, audioInTotal - audioInCached)
    t.audioInCached += audioInCached
    t.textIn += Math.max(0, textInTotal - textInCached)
    t.textInCached += textInCached
    t.audioOut += num(outDetails?.audio_tokens)
    t.textOut += num(outDetails?.text_tokens)
    t.responses += 1
    totals.value = t

    recompute()
    return true
  }

  /** "$0.0412", or null when the model has no known rates. */
  function formatUsd(value: number | null): string | null {
    if (value === null) return null
    if (value < 0.01) return `$${value.toFixed(4)}`
    return `$${value.toFixed(2)}`
  }

  return { totals, estimatedUsd, reset, addResponse, formatUsd, ratesChecked: RATES_CHECKED }
}
