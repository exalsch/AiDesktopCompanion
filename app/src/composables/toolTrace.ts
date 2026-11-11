// One short line per tool call of a voice session, for the live transcript,
// the call pill and the saved call history: "get_pointer_context -> Button
// "Steering Committee, 14:00 to 15:00, ..."". It is what the user reads to see
// what the assistant really did, so it shows the real result, not the model's
// account of it.
//
// No imports, so tests/tool-trace.test.ts runs it under Node directly.

const MAX = 160

function clip(s: string, max = MAX): string {
  const t = String(s ?? '').replace(/\s+/g, ' ').trim()
  return t.length > max ? t.slice(0, max - 1).trimEnd() + '…' : t
}

/** `mcp__windows-commander__point_at` -> `point_at`. */
export function bareToolName(name: string): string {
  return String(name || '').split('__').pop() || String(name || '')
}

function parse(s: string): any {
  try { return JSON.parse(s) } catch { return undefined }
}

/**
 * The tool's own payload: the first text block that is JSON, else all the
 * text. A tool that also returned an image has the image's note
 * (mcp::split_tool_images) as one of its text blocks, so "the first block"
 * would be the note, not the result.
 */
function payloadOf(out: any): { value: any, text: string, isError: boolean } {
  const result = out?.result
  const blocks = Array.isArray(result?.content) ? result.content : []
  const texts: string[] = blocks.filter((b: any) => b?.type === 'text').map((b: any) => String(b.text ?? ''))
  const value = texts.map(parse).find((v) => v !== undefined)
  return { value, text: texts.join(' '), isError: result?.isError === true }
}

function element(el: any): string {
  if (!el || typeof el !== 'object') return ''
  const name = String(el.name ?? '').trim()
  return name ? `${el.controlType || 'element'} "${name}"` : String(el.controlType || '')
}

function summary(tool: string, v: any, text: string): string {
  if (v === undefined) return text
  switch (tool) {
    case 'get_pointer_context': {
      if (v.blocked) return `${v.window?.processName || 'a private window'} (details withheld)`
      const what = element(v.resolved_element) || element(v.element) || v.window?.title || ''
      return [what, v.position].filter(Boolean).join(' · ')
    }
    case 'get_current_context': {
      const c = v.current || {}
      if (c.blocked) return `${c.processName || 'a private window'} (details withheld)`
      return [c.processName, c.title ? `"${c.title}"` : ''].filter(Boolean).join(' ')
    }
    case 'get_focused_ui_element':
      return [element(v.element), v.processName ? `in ${v.processName}` : ''].filter(Boolean).join(' ')
    case 'get_foreground_window':
      return [v.processName, v.title ? `"${v.title}"` : ''].filter(Boolean).join(' ')
    case 'find_ui_element': {
      const list = Array.isArray(v) ? v : (Array.isArray(v.elements) ? v.elements : [])
      return list.length ? `${list.length} found: ${list.slice(0, 3).map(element).join(', ')}` : 'nothing found'
    }
    case 'ocr_screen':
      return v.combinedText ? `read "${v.combinedText}"` : 'no text read'
    case 'point_at':
    case 'mark_area':
    case 'highlight_element':
      return v.error ? `failed: ${v.error}` : 'drawn on screen'
    case 'clear_annotations':
      return 'cleared'
    case 'capture_screen':
    case 'capture_screen_region':
      return 'screenshot taken'
    case 'ask_claude':
    case 'claude_result':
      return v.answer || v.error || v.status || text
    default:
      return text
  }
}

/** The argument worth showing next to the name, if any. */
function argHint(tool: string, args: any): string {
  if (!args || typeof args !== 'object') return ''
  const a = args.text ?? args.question ?? args.task ?? args.name_contains ?? (args.element ? 'element' : undefined)
  if (a !== undefined && a !== null && String(a).trim()) return `(${clip(String(a), 60)})`
  if (tool === 'ocr_screen' && args.region) return '(around the cursor)'
  return ''
}

/**
 * The trace line. `output` is what realtime_call_tool returned (a JSON string
 * `{serverId, tool, result}` or `{error}`); anything unparseable is shown
 * clipped as it is.
 */
export function toolTraceLine(name: string, argsJson: string, output: string): string {
  const tool = bareToolName(name)
  const args = parse(argsJson) ?? {}
  const head = `${tool}${argHint(tool, args) ? ' ' + argHint(tool, args) : ''}`
  const out = parse(output)
  if (out === undefined) return `${head} -> ${clip(output)}`
  if (out && typeof out === 'object' && 'error' in out && !('result' in out)) return `${head} -> error: ${clip(String(out.error))}`
  if (out && typeof out === 'object' && 'answer' in out) return `${head} -> ${clip(String(out.answer))}`
  // background_tools.rs / hermes.rs: the job runs on, its result comes later.
  if (out && typeof out === 'object' && out.status === 'started') return `${head} -> started in the background`
  const p = payloadOf(out)
  if (p.isError) return `${head} -> error: ${clip(p.text)}`
  const s = summary(tool, p.value, p.text)
  return `${head} -> ${clip(s || '(no result)')}`
}

/** Prefix a trace line carries in the plain-text transcript. */
export const TOOL_LINE_PREFIX = '[tool]'
