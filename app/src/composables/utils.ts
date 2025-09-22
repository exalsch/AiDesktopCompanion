// Generic utilities extracted from App.vue

export function parseArgs(text: string): string[] {
  if (!text || !text.trim()) return []
  const out: string[] = []
  let cur = ''
  let i = 0
  let quote: '"' | "'" | null = null
  while (i < text.length) {
    const ch = text[i]
    if (quote) {
      if (ch === quote) { quote = null; i++; continue }
      if (quote === '"' && ch === '\\' && i + 1 < text.length) {
        const n = text[i + 1]
        if (n === '"') { cur += '"'; i += 2; continue }
        if (n === '\\') { cur += '\\'; i += 2; continue }
        if (n === 'n') { cur += '\n'; i += 2; continue }
        if (n === 't') { cur += '\t'; i += 2; continue }
      }
      cur += ch; i++; continue
    }
    if (ch === '"' || ch === "'") { quote = ch as any; i++; continue }
    if (ch === ' ' || ch === '\t' || ch === '\n' || ch === '\r') {
      if (cur.length) { out.push(cur); cur = '' }
      i++
      while (i < text.length && /\s/.test(text[i])) i++
      continue
    }
    if (ch === '\\' && i + 1 < text.length) {
      const n = text[i + 1]
      if (n === ' ' || n === '"' || n === "'" || n === '\\') { cur += n; i += 2; continue }
    }
    cur += ch
    i++
  }
  if (cur.length) out.push(cur)
  return out
}

export function parseJsonObject(text: string): Record<string, string> {
  if (!text || !text.trim()) return {}
  const v = JSON.parse(text)
  if (v && typeof v === 'object' && !Array.isArray(v)) return v as Record<string, string>
  throw new Error('ENV must be a JSON object of { key: value }')
}

export function parseKeyValuePairs(text: string): Record<string, string> {
  const out: Record<string, string> = {}
  if (!text || !text.trim()) return out
  const parts = text
    .split(/\r?\n|;|,/)
    .map(s => s.trim())
    .filter(Boolean)
  for (const p of parts) {
    const idx = p.indexOf('=')
    if (idx === -1) continue
    const k = p.slice(0, idx).trim()
    const v = p.slice(idx + 1).trim().replace(/^"|"$/g, '')
    if (k) out[k] = v
  }
  return out
}

export function normalizeEnvInput(input: any): Record<string, string> {
  try {
    if (typeof input === 'string') {
      const t = input.trim()
      if (!t) return {}
      try {
        const v = JSON.parse(t)
        if (v && typeof v === 'object' && !Array.isArray(v)) {
          return Object.fromEntries(Object.entries(v).map(([k, val]) => [k, String(val)]))
        }
        if (Array.isArray(v)) {
          const out: Record<string, string> = {}
          for (const item of v) {
            if (Array.isArray(item) && item.length >= 2) out[String(item[0])] = String(item[1])
            else if (typeof item === 'string' && item.includes('=')) {
              const idx = item.indexOf('=')
              const k = item.slice(0, idx).trim()
              const val = item.slice(idx + 1).trim()
              if (k) out[k] = val
            }
          }
          return out
        }
      } catch {}
      return parseKeyValuePairs(t)
    }
    if (Array.isArray(input)) {
      const out: Record<string, string> = {}
      for (const item of input) {
        if (Array.isArray(item) && item.length >= 2) out[String(item[0])] = String(item[1])
        else if (typeof item === 'string' && item.includes('=')) {
          const idx = item.indexOf('=')
          const k = item.slice(0, idx).trim()
          const val = item.slice(idx + 1).trim()
          if (k) out[k] = val
        }
      }
      return out
    }
    if (input && typeof input === 'object') {
      return Object.fromEntries(Object.entries(input).map(([k, val]) => [k, String(val)]))
    }
  } catch {}
  return {}
}
