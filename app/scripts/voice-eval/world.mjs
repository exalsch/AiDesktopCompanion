// The canned desktop the eval runs against: what windows-commander, the Rob
// bridge (Claude Code) and the app layer would return, per scenario world.
//
// The raw windows-commander answers copy what the real tools return for the
// new Outlook (olk, WebView2): get_pointer_context and get_focused_ui_element
// only ever name the top-level Window, read_ui_tree / find_ui_element on the
// olk handle see the 14 elements of the window frame, and OCR of a region
// around the cursor reads the event text. Measured on 11.11.2026, see
// specs/ScreenPresence_design.md, "Outlook reality check".
//
// `appLayer: true` adds what realtime_call_tool now adds on top of the raw
// result (voice_tools.rs): the `position` sentence and, for a WebView2 host,
// `resolved_element` from the content window's UIA tree.

const OLK_HWND = 67564
const OLK_TITLE = 'Calendar - Sam Example - Outlook'
export const EVENT_NAME = 'Steering Committee HYDS, 14:00 to 15:00, Wednesday, November 11, 2026, Microsoft Teams Meeting, By Paul Weber, Busy'
export const EVENT_REF = 'NDIuNjc3NDQuNC42LjQuOTk5OQ=='

const SCREEN2 = { monitorId: 'screen-2', bounds: { x: 1920, y: 0, width: 2560, height: 1440, right: 4480, bottom: 1440 }, workingArea: { x: 1920, y: 0, width: 2560, height: 1392, right: 4480, bottom: 1392 }, dpiScale: 1 }

function pointer(x, y, window, element, monitor = SCREEN2) {
  return { x, y, monitor: { ...monitor, localX: x - monitor.bounds.x, localY: y - monitor.bounds.y }, window, element }
}

const olkWindow = { hwnd: OLK_HWND, pid: 24908, processName: 'olk', title: OLK_TITLE, className: 'Outlook Host', bounds: { x: 1912, y: -8, width: 2576, height: 1408, right: 4488, bottom: 1400 } }
const olkWindowElement = { elementRef: 'NDIuNjc1NjQ=', name: OLK_TITLE, automationId: '', controlType: 'Window', className: 'Outlook Host', bounds: olkWindow.bounds, isEnabled: true, isOffscreen: false, value: null }

/** Same buckets as voice_tools.rs `describe_position`. */
export function describePosition(p) {
  const m = p.monitor
  if (!m?.bounds?.width || !m?.bounds?.height) return null
  const fx = (p.x - m.bounds.x) / m.bounds.width
  const fy = (p.y - m.bounds.y) / m.bounds.height
  const h = fx < 0.2 ? 'near the left edge' : fx < 0.4 ? 'left of centre' : fx <= 0.6 ? 'in the centre' : fx < 0.8 ? 'right of centre' : 'near the right edge'
  const v = fy < 0.2 ? 'near the top' : fy < 0.4 ? 'above the middle' : fy <= 0.6 ? 'in the middle' : fy < 0.8 ? 'below the middle' : 'near the bottom'
  const n = String(m.monitorId || '').match(/(\d+)$/)?.[1]
  const screen = n ? `screen ${n}` : 'the screen'
  return `On ${screen}: horizontally ${h} (${Math.round(fx * 100)} %), vertically ${v} (${Math.round(fy * 100)} %).`
}

function calendarOcrFull() {
  const items = [
    'Daily stand-up 08:45', 'Deep work 09:00', 'Team one-to-one 11:00', 'Lunch 12:30', 'Steering Committee HYDS 14:00',
    'Architecture forum 15:00', 'Tech planning 14:30', 'Release sync 16:00', 'Sprint review 10:00',
    'Platform refresh briefing 13:00', 'Customer call 09:30', 'Weekly ops 08:30',
  ]
  return { blocks: items.map((text, i) => ({ text, confidence: 1, bounds: { x: 2000 + (i % 5) * 480, y: 300 + Math.floor(i / 5) * 300, width: 200, height: 14 } })), combinedText: items.join(' ') }
}

/** The worlds. `pointerAt` may change between turns (index = turn). */
export const WORLDS = {
  outlook: {
    foreground: olkWindow,
    focused: olkWindowElement,
    pointers: [pointer(2889, 911, olkWindow, olkWindowElement), pointer(4210, 180, olkWindow, olkWindowElement)],
    resolved: [{ elementRef: EVENT_REF, name: EVENT_NAME, controlType: 'Button', bounds: { x: 2700, y: 874, width: 448, height: 76 } }, { elementRef: 'NDIuNjc3NDQuNC42LjQuMTAwMDE=', name: 'Search', controlType: 'Edit', bounds: { x: 4000, y: 160, width: 400, height: 40 } }],
    ocrNearCursor: ['Steering Committee HYDS', '14:00 - 15:00', 'Microsoft Teams Meeting', 'Paul Weber'],
    ocrNearCursor2: ['Search', 'Meet now'],
    ocrFull: calendarOcrFull,
    uiTree: () => ({ elements: [olkWindowElement, { elementRef: 'NDIuNjgwMDg=', name: 'Caption Buttons', controlType: 'ToolBar' }, { elementRef: 'NDIuNjgwMDguNC4w', name: 'Minimize', controlType: 'Button' }, { elementRef: 'NDIuNjgwMDguNC4x', name: 'Restore Down', controlType: 'Button' }, { elementRef: 'NDIuNjgwMDguNC4y', name: 'Close', controlType: 'Button' }, { elementRef: 'NDIuNjc5NTY=', name: '', controlType: 'Pane' }, { elementRef: 'NC42NzU2NC4w', name: OLK_TITLE, controlType: 'TitleBar' }], truncated: false, elementCount: 14 }),
    findUi: () => [],
  },
  compose: {
    foreground: { hwnd: 6685460, pid: 24908, processName: 'olk', title: 'Re: Platform refresh in December - Sam Example - Outlook', className: 'Outlook Host' },
    focused: { elementRef: 'Ym9keQ==', name: 'Message body', controlType: 'Edit' },
    pointers: [pointer(2600, 700, { hwnd: 6685460, processName: 'olk', title: 'Re: Platform refresh in December - Sam Example - Outlook' }, { elementRef: 'NDIuNjY4NTQ2MA==', name: 'Re: Platform refresh in December - Sam Example - Outlook', controlType: 'Window' })],
    resolved: [{ elementRef: 'Ym9keQ==', name: 'Message body', controlType: 'Edit', bounds: { x: 2000, y: 400, width: 1400, height: 700 } }],
    ocrNearCursor: ['Thanks Paul, the refresh date works for us.'],
    ocrFull: () => ({ blocks: [{ text: 'Send', bounds: { x: 2010, y: 140, width: 40, height: 14 } }, { text: 'Discard', bounds: { x: 2090, y: 140, width: 50, height: 14 } }, { text: 'To Paul Weber', bounds: { x: 2010, y: 200, width: 120, height: 14 } }], combinedText: 'Send Discard To Paul Weber Thanks Paul, the refresh date works for us.' }),
    uiTree: () => ({ elements: [{ elementRef: 'NDIuNjY4NTQ2MA==', name: 'Re: Platform refresh', controlType: 'Window' }], truncated: false, elementCount: 12 }),
    findUi: (args) => (/send/i.test(String(args.name_contains || '')) ? [{ elementRef: 'c2VuZC5idXR0b24=', name: 'Send', controlType: 'Button', bounds: { x: 2000, y: 132, width: 64, height: 32 } }] : []),
    sendRef: 'c2VuZC5idXR0b24=',
  },
  citrix: {
    foreground: { hwnd: 9901, pid: 4400, processName: 'CDViewer', title: 'M3 Business Engine - Citrix Workspace', className: 'Transparent Windows Client' },
    focused: { elementRef: 'Y2l0cml4', name: '', controlType: 'Pane' },
    pointers: [pointer(800, 500, { hwnd: 9901, processName: 'CDViewer', title: 'M3 Business Engine - Citrix Workspace' }, { elementRef: 'Y2l0cml4', name: '', controlType: 'Pane' }, { monitorId: 'screen-1', bounds: { x: 0, y: 0, width: 1920, height: 1080, right: 1920, bottom: 1080 }, workingArea: { x: 0, y: 0, width: 1920, height: 1032 }, dpiScale: 1 })],
    resolved: [null],
    ocrNearCursor: ['CRS610/E Customer. Open', 'Customer 100234', 'Example Industries AS'],
    ocrFull: () => ({ blocks: [{ text: 'CRS610/E Customer. Open', bounds: { x: 600, y: 420, width: 260, height: 14 } }, { text: 'Customer 100234 Example Industries AS', bounds: { x: 600, y: 470, width: 300, height: 14 } }], combinedText: 'CRS610/E Customer. Open Customer 100234 Example Industries AS' }),
    uiTree: () => ({ elements: [{ elementRef: 'Y2l0cml4', name: '', controlType: 'Pane' }], truncated: false, elementCount: 1 }),
    findUi: () => [],
  },
}

function ocrBlocks(lines, cx, cy) {
  return { blocks: lines.map((text, i) => ({ text, confidence: 1, bounds: { x: cx - 200, y: cy - 30 + i * 18, width: 260, height: 14 } })), combinedText: lines.join(' ') }
}

/** The screen context item as the app sends it (old or new format). */
export function contextItem(world, turn, format) {
  const w = WORLDS[world]
  const p = w.pointers[Math.min(turn, w.pointers.length - 1)]
  const el = p.element
  const elText = el.name ? `${el.controlType} "${el.name.slice(0, format === 'new' ? 117 : 57)}${el.name.length > (format === 'new' ? 120 : 60) ? '...' : ''}"` : el.controlType
  const fg = `${w.foreground.processName} "${w.foreground.title}"`
  const focused = w.focused.name ? `${w.focused.controlType} "${w.focused.name}"` : w.focused.controlType
  if (format === 'old') {
    return `[Screen context: what the user has in front of them right now. Not said by the user; do not read it out.]\nForeground: ${fg}\nFocused: ${focused}\nUnder the cursor: ${elText} [element_ref ${el.elementRef}] in ${p.window.processName}\nBefore that: chrome "Claude Cockpit" (14s); AiDesktopCompanion "AiDesktopCompanion" (6s)`
  }
  return `[Screen context, written by the app, not said by the user. It can be a few seconds old.]\nForeground: ${fg}\nFocused: ${focused}\nCursor: ${describePosition(p)} Over ${elText} [element_ref ${el.elementRef}] in ${p.window.processName}\nBefore that: chrome "Claude Cockpit" (14s); AiDesktopCompanion "AiDesktopCompanion" (6s)`
}

/**
 * Canned result for one tool call. Returns a JSON-able object (the parsed
 * text of the MCP result, as the model would see it inside `result`).
 */
export function cannedResult(tool, args, ctx) {
  const w = WORLDS[ctx.world]
  const turn = Math.min(ctx.turn, w.pointers.length - 1)
  const p = w.pointers[turn]
  switch (tool) {
    case 'get_pointer_context': {
      const out = { ...p }
      if (args?.include_image) out.image_note = 'The image follows in the next message.'
      if (ctx.appLayer) {
        out.position = describePosition(p)
        const r = w.resolved[Math.min(turn, w.resolved.length - 1)]
        if (r) out.resolved_element = { ...r, source: 'UI Automation tree of the WebView2 content window' }
      }
      return out
    }
    case 'get_cursor_position': return { x: p.x, y: p.y }
    case 'get_current_context': return { current: { sequence: 534, processName: w.foreground.processName, title: w.foreground.title, hwnd: w.foreground.hwnd, focusedElement: { name: w.focused.name, controlType: w.focused.controlType } }, currentForMs: 41000, recent: [] }
    case 'get_foreground_window': return { ...w.foreground, isVisible: true, isMinimized: false }
    case 'get_focused_ui_element': return { element: w.focused, processName: w.foreground.processName, windowTitle: w.foreground.title }
    case 'get_ui_element_details': return { element: args?.element_ref === p.element.elementRef ? p.element : (w.resolved.find((r) => r && r.elementRef === args?.element_ref) || { error: 'element not found' }) }
    case 'read_ui_tree': return w.uiTree()
    case 'find_ui_element': return { elements: w.findUi(args || {}) }
    case 'ocr_screen': {
      const r = args?.region
      if (r && p.x >= r.x - 60 && p.x <= r.x + r.width + 60 && p.y >= r.y - 60 && p.y <= r.y + r.height + 60) {
        const lines = turn === 1 && w.ocrNearCursor2 ? w.ocrNearCursor2 : w.ocrNearCursor
        return { ...ocrBlocks(lines, p.x, p.y), region: r }
      }
      if (r) return { blocks: [], combinedText: '', region: r }
      return w.ocrFull()
    }
    case 'capture_screen':
    case 'capture_screen_region':
      return { note: 'Screenshot captured. (Eval: no pixels are sent, so describe nothing you did not get as text.)' }
    case 'point_at':
    case 'mark_area':
    case 'highlight_element': {
      const target = args?.element || args?.text || (args?.x != null ? `${args.x},${args.y}` : args?.rect ? 'rect' : '')
      if (!target) return { error: 'Pass exactly one target: x+y, element or text.' }
      if (args?.text && !/send|steering|search|crs610|customer/i.test(args.text)) return { error: `Text "${args.text}" not found on screen.` }
      return { id: `a${ctx.annotations = (ctx.annotations || 0) + 1}`, resolved: { x: 2000, y: 132, width: 64, height: 32 } }
    }
    case 'clear_annotations': return { cleared: [], count: 0 }
    case 'invoke_ui_element':
      if (!args?.element_ref) return { error: 'element_ref is required.' }
      return { ok: true, element_ref: args.element_ref, action: args.action || 'invoke' }
    case 'set_ui_value':
      if (!args?.element_ref) return { error: 'element_ref is required.' }
      return { ok: true, element_ref: args.element_ref }
    case 'type_text': return { ok: true, typed: String(args?.text || '').length }
    case 'send_hotkey': return { ok: true, keys: [...(args?.modifiers || []), args?.key].join('+') }
    case 'ask_claude': {
      const q = String(args?.question || '')
      if (/steering|hyds/i.test(q)) return { answer: 'The Steering Committee HYDS today at 14:00 is with Paul Weber. The agenda asks for the go-live status and the open budget points, so bring the status slide.' }
      return { answer: 'I cannot see what is on the screen or selected in Outlook. Tell me the event name and I will look it up.' }
    }
    case 'claude_result': return { error: `I have no record of ticket ${args?.ticket}.`, isError: true }
    case 'claude_status': return { lanes: ['rob', 'cotire', 'desk', 'companion'], running: [] }
    case 'delegate_to_hermes': return { status: 'started', run_id: 'run_1', note: 'Say that Hermes is on it.' }
    default: return { error: `eval: ${tool} is not simulated` }
  }
}
