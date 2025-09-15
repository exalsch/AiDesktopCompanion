// Thin wrapper around @dqbd/tiktoken/lite for browser-friendly tokenization.
// We still lazy-initialize at runtime, but use static imports to avoid Vite
// dev server edge-cases with dynamic WASM module transforms.
import { ref } from 'vue'
import { init as initLite, Tiktoken } from '@dqbd/tiktoken/lite/init'
import wasmInit from '@dqbd/tiktoken/lite/tiktoken_bg.wasm?init'
import cl100k from '@dqbd/tiktoken/encoders/cl100k_base.json'

let _encodeFn: ((s: string) => number) | null = null
let _loading = false
export const tokenizerReady = ref(false)

export function getTokenizer(): ((s: string) => number) | null {
  if (_encodeFn) return _encodeFn
  if (!_loading) void preloadTokenizer()
  return null
}

export async function preloadTokenizer(): Promise<boolean> {
  if (_encodeFn) return true
  if (_loading) return false
  _loading = true
  try {
    // Initialize WASM via Vite '?init' loader; pass the init function to tiktoken's init
    try {
      try { console.log('[tokenizer] initializing wasm via ?init') } catch {}
      await initLite(wasmInit as any)
    } catch (e) {
      try { console.warn('[tokenizer] wasm init error', e) } catch {}
      throw e
    }
    const enc = new Tiktoken(
      (cl100k as any).bpe_ranks,
      (cl100k as any).special_tokens,
      (cl100k as any).pat_str,
    )
    _encodeFn = (s: string) => {
      try {
        const arr = enc.encode(s)
        return Array.isArray(arr) ? arr.length : (arr?.length || 0)
      } catch { return 0 }
    }
    tokenizerReady.value = true
    try { console.log('[tokenizer] tiktoken lite loaded and ready (cl100k_base)') } catch {}
    return true
  } catch (e) {
    try { console.warn('[tokenizer] failed to load tiktoken lite; staying in approx mode', e) } catch {}
  } finally {
    _loading = false
  }
  return false
}
