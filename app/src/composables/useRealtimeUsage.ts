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
 * but are not fetched from anywhere, and both providers change realtime
 * pricing often. `RATES_CHECKED` is what tells you how far to trust the figure.
 */

/**
 * When the rate table below was last verified against the published pricing:
 * https://developers.openai.com/api/docs/pricing and
 * https://aistudio.google.com/docs/pricing (paid tier).
 */
export const RATES_CHECKED = '2026-11-16'

/** USD per million tokens. */
export interface ModelRates {
  audioIn: number
  audioInCached: number
  audioOut: number
  textIn: number
  textInCached: number
  textOut: number
  /** Images (and Gemini video frames), e.g. screenshots a tool sent into the call. */
  imageIn: number
}

const GPT_REALTIME: ModelRates = { audioIn: 32, audioInCached: 0.4, audioOut: 64, textIn: 4, textInCached: 0.4, textOut: 16, imageIn: 5 }
const GPT_REALTIME_2: ModelRates = { ...GPT_REALTIME, textOut: 24 }
const GPT_REALTIME_MINI: ModelRates = { audioIn: 10, audioInCached: 0.3, audioOut: 20, textIn: 0.6, textInCached: 0.06, textOut: 2.4, imageIn: 0.8 }
// Gemini lists no cached-input rate for the Live models; nothing is counted
// as cached there (see addGeminiUsage), so those two fields are never used.
const GEMINI_LIVE: ModelRates = { audioIn: 3, audioInCached: 3, audioOut: 12, textIn: 0.75, textInCached: 0.75, textOut: 4.5, imageIn: 1 }
const GEMINI_25_NATIVE_AUDIO: ModelRates = { audioIn: 3, audioInCached: 3, audioOut: 12, textIn: 0.5, textInCached: 0.5, textOut: 2, imageIn: 3 }

/**
 * Rates by model prefix, longest match first.
 *
 * Keyed on prefix because both providers ship dated snapshots and variants
 * behind moving names (`gpt-realtime-2.1-2026-...`,
 * `gemini-3.8-live-extended-thinking`), and a table of exact ids would silently
 * miss every one of them and report a call as unpriced.
 *
 * All figures are the published ones, except the Gemini 2.5 native-audio image
 * rate: Google prices "audio/video" input together at $3, and an image is
 * charged as video there.
 */
const RATE_TABLE: Array<{ prefix: string, rates: ModelRates }> = [
  { prefix: 'gpt-realtime-2.1-mini', rates: GPT_REALTIME_MINI },
  { prefix: 'gpt-realtime-mini', rates: GPT_REALTIME_MINI },
  { prefix: 'gpt-realtime-2.1', rates: GPT_REALTIME_2 },
  { prefix: 'gpt-realtime-2', rates: GPT_REALTIME_2 },
  // gpt-realtime-1.5 and the original gpt-realtime.
  { prefix: 'gpt-realtime', rates: GPT_REALTIME },
  { prefix: 'gemini-3.8-live', rates: GEMINI_LIVE },
  { prefix: 'gemini-3.1-flash-live', rates: GEMINI_LIVE },
  { prefix: 'gemini-2.5-flash-native-audio', rates: GEMINI_25_NATIVE_AUDIO },
]

/** Rates for a model id, or null when it is not in the table. */
export function ratesFor(model?: string): ModelRates | null {
  const m = String(model || '').trim().toLowerCase().replace(/^models\//, '')
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
  imageIn: number
  responses: number
}

function emptyTotals(): UsageTotals {
  return { audioIn: 0, audioInCached: 0, audioOut: 0, textIn: 0, textInCached: 0, textOut: 0, imageIn: 0, responses: 0 }
}

/** USD for `t` at `rates`. */
export function costUsd(t: UsageTotals, rates: ModelRates): number {
  return (t.audioIn * rates.audioIn
    + t.audioInCached * rates.audioInCached
    + t.audioOut * rates.audioOut
    + t.textIn * rates.textIn
    + t.textInCached * rates.textInCached
    + t.textOut * rates.textOut
    + t.imageIn * rates.imageIn) / 1_000_000
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
    estimatedUsd.value = costUsd(totals.value, rates)
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
    const imageInTotal = num(inDetails?.image_tokens)
    const audioInCached = Math.min(num(cached.audio_tokens), audioInTotal)
    const textInCached = Math.min(num(cached.text_tokens), textInTotal)
    const imageInCached = Math.min(num(cached.image_tokens), imageInTotal)

    const t = { ...totals.value }
    t.audioIn += Math.max(0, audioInTotal - audioInCached)
    t.audioInCached += audioInCached
    t.textIn += Math.max(0, textInTotal - textInCached)
    // OpenAI publishes no cached image rate; a cached image is a repeat of
    // earlier context, charged like cached text.
    t.textInCached += textInCached + imageInCached
    t.imageIn += Math.max(0, imageInTotal - imageInCached)
    t.audioOut += num(outDetails?.audio_tokens)
    t.textOut += num(outDetails?.text_tokens)
    t.responses += 1
    totals.value = t

    recompute()
    return true
  }

  /**
   * Fold one Gemini Live `usageMetadata` block into the totals.
   *
   * Gemini sends one per model turn, broken down by modality. Thinking tokens
   * are billed as output and reported apart from the response, so they are
   * added to text out. There is no cached split per modality, so nothing is
   * counted as cached. Images and video frames (a screenshot a tool sent) are
   * image input. A model missing from the rate table leaves the cost at null
   * ("unpriced") rather than a made-up figure.
   */
  function addGeminiUsage(meta: any): boolean {
    if (!meta || typeof meta !== 'object') return false
    const byModality = (details: any, modality: string) =>
      (Array.isArray(details) ? details : [])
        .filter((d: any) => String(d?.modality || '').toUpperCase() === modality)
        .reduce((sum: number, d: any) => sum + num(d?.tokenCount), 0)
    const prompt = meta.promptTokensDetails
    const response = meta.responseTokensDetails
    if (!prompt && !response && !num(meta.totalTokenCount)) return false

    const t = { ...totals.value }
    t.audioIn += byModality(prompt, 'AUDIO')
    t.textIn += byModality(prompt, 'TEXT')
    t.imageIn += byModality(prompt, 'IMAGE') + byModality(prompt, 'VIDEO')
    t.audioOut += byModality(response, 'AUDIO')
    t.textOut += byModality(response, 'TEXT') + num(meta.thoughtsTokenCount)
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

  return { totals, estimatedUsd, reset, addResponse, addGeminiUsage, formatUsd, ratesChecked: RATES_CHECKED }
}
