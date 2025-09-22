import { reactive, toRefs } from 'vue'

// Simple reactive cache for image metadata (width/height) by src URL
// Usage: const { getMeta } = useImageMeta(); const meta = getMeta(src)
// meta is reactive: { width, height, ok, loaded }

interface ImageMeta {
  width: number
  height: number
  ok: boolean
  loaded: boolean
}

const cache: Record<string, ImageMeta> = reactive({}) as any

function load(src: string) {
  if (!src) return
  if (cache[src] && cache[src].loaded) return
  if (!cache[src]) cache[src] = { width: 0, height: 0, ok: false, loaded: false }
  try {
    const img = new Image()
    img.onload = () => {
      cache[src].width = img.naturalWidth || img.width || 0
      cache[src].height = img.naturalHeight || img.height || 0
      cache[src].ok = (cache[src].width > 0 && cache[src].height > 0)
      cache[src].loaded = true
    }
    img.onerror = () => {
      cache[src].ok = false
      cache[src].loaded = true
    }
    img.src = src
  } catch {
    cache[src].ok = false
    cache[src].loaded = true
  }
}

export function useImageMeta() {
  function getMeta(src: string): ImageMeta {
    if (!cache[src]) cache[src] = { width: 0, height: 0, ok: false, loaded: false }
    load(src)
    return cache[src]
  }
  function getMany(sources: string[]): ImageMeta[] {
    const out: ImageMeta[] = []
    for (const s of sources || []) out.push(getMeta(s))
    return out
  }
  return { getMeta, getMany }
}
