import { invoke } from '@tauri-apps/api/core'

/** An image an MCP tool returned, e.g. a windows-commander screenshot. */
export interface ToolImage { mimeType: string, data: string }

export interface ToolCallOutput {
  /** What goes back to the model as the tool's result. Never holds image data. */
  output: string
  /** Images the tool returned, to be sent on as images in the next message. */
  images: ToolImage[]
}

/**
 * Run an MCP tool for a voice session.
 *
 * `realtime_call_tool` lifts image blocks out of the result (see
 * `mcp::split_tool_images`), so a screenshot no longer reaches the model as a
 * few hundred thousand characters of base64 it cannot see.
 */
export async function callMcpTool(name: string, argsJson: string): Promise<ToolCallOutput> {
  try {
    const res = await invoke<ToolCallOutput>('realtime_call_tool', { name, argsJson })
    return {
      output: String(res?.output ?? ''),
      images: Array.isArray(res?.images) ? res.images.filter((i) => i && typeof i.data === 'string' && i.data) : [],
    }
  } catch (e: any) {
    return { output: JSON.stringify({ error: e?.message || String(e) }), images: [] }
  }
}

/**
 * Re-encode an image as JPEG small enough to fit `maxBase64Chars`.
 *
 * Needed for OpenAI Realtime, whose events travel over a WebRTC data channel
 * with a per-message cap (256 KiB in Chromium). A full-size PNG screenshot is
 * over it, and an oversized message is dropped, not split. Quality is lowered
 * first and the size only after, because screen text survives JPEG
 * compression better than downscaling. Returns null when even the smallest
 * attempt does not fit.
 */
export async function shrinkImageToFit(img: ToolImage, maxBase64Chars: number): Promise<ToolImage | null> {
  // Already small enough: keep it as it is, PNG included - screen text is
  // sharpest untouched.
  if (img.data.length <= maxBase64Chars) return img
  const el = new Image()
  el.src = `data:${img.mimeType};base64,${img.data}`
  try { await el.decode() } catch { return null }
  const longest = Math.max(el.naturalWidth, el.naturalHeight) || 1
  for (const side of [1600, 1280, 1024, 800]) {
    const scale = Math.min(1, side / longest)
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(el.naturalWidth * scale))
    canvas.height = Math.max(1, Math.round(el.naturalHeight * scale))
    const ctx = canvas.getContext('2d')
    if (!ctx) return null
    ctx.drawImage(el, 0, 0, canvas.width, canvas.height)
    for (const quality of [0.85, 0.7, 0.55]) {
      const data = canvas.toDataURL('image/jpeg', quality).split(',')[1] || ''
      if (data && data.length <= maxBase64Chars) return { mimeType: 'image/jpeg', data }
    }
  }
  return null
}
