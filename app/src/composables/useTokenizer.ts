// Thin wrapper around @dqbd/tiktoken/lite for browser-friendly tokenization.
// We still lazy-initialize at runtime, but use static imports to avoid Vite
// dev server edge-cases with dynamic WASM module transforms.
import { ref } from 'vue'
import { init as initLite, Tiktoken } from '@dqbd/tiktoken/lite/init'
import wasmUrl from '@dqbd/tiktoken/lite/tiktoken_bg.wasm?url'
import cl100k from '@dqbd/tiktoken/encoders/cl100k_base.json'

let _encodeFn: ((s: string) => number) | null = null
let _loading = false
let _loadingPromise: Promise<boolean> | null = null
export const tokenizerReady = ref(false)
export const tokenizerLastError = ref<string | null>(null)

export function getTokenizer(): ((s: string) => number) | null {
  if (_encodeFn) return _encodeFn
  if (!_loading) void preloadTokenizer()
  return null
}

export async function preloadTokenizer(): Promise<boolean> {
  if (_encodeFn) return true
  if (_loading && _loadingPromise) return _loadingPromise
  _loading = true
  _loadingPromise = (async () => {
    try {
      // Initialize WASM via explicit URL. Try instantiateStreaming, fall back to arrayBuffer -> instantiate.
      try {
        try { console.log('[tokenizer] initializing wasm via instantiateStreaming', wasmUrl) } catch {}
        await initLite((imports: WebAssembly.Imports) => WebAssembly.instantiateStreaming(fetch(wasmUrl as unknown as string), imports) as any)
      } catch (e1) {
        try { console.warn('[tokenizer] instantiateStreaming failed; falling back to arrayBuffer', e1) } catch {}
        try {
          const resp = await fetch(wasmUrl as unknown as string)
          const buf = await resp.arrayBuffer()
          await initLite((imports: WebAssembly.Imports) => WebAssembly.instantiate(buf, imports) as any)
        } catch (e2) {
          try { console.warn('[tokenizer] wasm init error (instantiate fallback failed)', e2) } catch {}
          try { tokenizerLastError.value = (e2 as any)?.message || String(e2) } catch {}
          throw e2
        }
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
      try { tokenizerLastError.value = null } catch {}
      return true
    } catch (e) {
      try { console.warn('[tokenizer] failed to load tiktoken lite; staying in approx mode', e) } catch {}
      try { tokenizerLastError.value = (e as any)?.message || String(e) } catch {}
      return false
    } finally {
      _loading = false
    }
  })()
  return _loadingPromise
}
