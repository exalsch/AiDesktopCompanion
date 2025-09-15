import type { Message } from '../state/conversation'
import { getTokenizer } from './useTokenizer'

// Lightweight token estimation with optional tokenizer (gpt-tokenizer) support.
// Text: ~4 chars/token fallback with model-aware tweaks; tokenizer mode uses cl100k encoding.
// Images: improved based on resolution; fallback to constant per image.

export type TokenizerMode = 'approx' | 'tiktoken'

const DEFAULT_CHARS_PER_TOKEN = 4.0
const IMAGE_APPROX_TOKENS = 300 // fallback per image when no metadata

function normalizeModelName(model?: string): string {
  const m = String(model || '').toLowerCase().trim()
  if (!m) return 'gpt-4o-mini'
  return m
}

function charsPerTokenForModel(model?: string): number {
  const m = normalizeModelName(model)
  if (m.includes('gpt-4o')) return 4.0
  if (m.includes('gpt-4.1') || m.includes('gpt-4.0')) return 3.8
  if (m.includes('gpt-3.5')) return 3.7
  return DEFAULT_CHARS_PER_TOKEN
}

export function estimateTextTokens(text: string, model?: string, mode: TokenizerMode = 'approx'): { tokens: number; chars: number } {
  const s = String(text || '')
  const chars = s.length
  if (mode === 'tiktoken') {
    try {
      const enc = getTokenizer()
      if (enc) {
        const tokens = enc(s)
        return { tokens, chars }
      }
    } catch {}
  }
  const cpt = charsPerTokenForModel(model)
  const tokens = Math.ceil(chars / Math.max(1, cpt))
  return { tokens, chars }
}

// Improved image estimate using resolution (megapixels) heuristic.
// Heuristic: per image tokens ≈ base + scale * MP, clamped.
// Defaults tuned conservatively. Adjust as needed once provider docs stabilize.
function estimateImageTokensForSize(w: number, h: number): number {
  const width = Math.max(0, w | 0)
  const height = Math.max(0, h | 0)
  if (!width || !height) return IMAGE_APPROX_TOKENS
  const mp = (width * height) / 1_000_000 // megapixels
  const base = 220
  const perMp = 180
  const est = Math.round(base + perMp * mp)
  return Math.max(120, Math.min(est, 2000))
}

export function estimateImageTokens(count = 1): number {
  const n = Math.max(0, count | 0)
  return n * IMAGE_APPROX_TOKENS
}

export function estimateImageTokensFromMeta(metas: Array<{ width?: number; height?: number; ok?: boolean }>): number {
  if (!Array.isArray(metas) || metas.length === 0) return 0
  let total = 0
  for (const m of metas) {
    const w = Math.max(0, (m?.width as any) | 0)
    const h = Math.max(0, (m?.height as any) | 0)
    total += estimateImageTokensForSize(w, h)
  }
  return total
}

export function estimateMessageTokens(message: Message, model?: string, mode: TokenizerMode = 'approx'): number {
  if (!message) return 0
  if (message.type === 'text') {
    return estimateTextTokens(message.text || '', model, mode).tokens
  }
  if (message.type === 'image') {
    const n = Array.isArray(message.images) ? message.images.length : 0
    return estimateImageTokens(n)
  }
  return 0
}

export function estimateConversationTokens(messages: Message[], model?: string, mode: TokenizerMode = 'approx'): { total: number; byRole: Record<string, number> } {
  const byRole: Record<string, number> = {}
  let total = 0
  for (const m of (messages || [])) {
    const t = estimateMessageTokens(m, model, mode)
    total += t
    const key = `${m.role}:${m.type}`
    byRole[key] = (byRole[key] || 0) + t
  }
  return { total, byRole }
}

export function formatTokenInfo(parts: Array<{ label: string; tokens: number }>, totalLabel = 'total'): string {
  const total = parts.reduce((acc, p) => acc + (p.tokens || 0), 0)
  const segs = parts.filter(p => p.tokens > 0).map(p => `${p.label} ${p.tokens}`)
  const details = segs.length ? ` (${segs.join(' + ')})` : ''
  return `≈ ${total} tokens${details}`
}
