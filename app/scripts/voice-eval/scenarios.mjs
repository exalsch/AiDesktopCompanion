// Scenarios and checks. Each scenario is a fresh session with scripted text
// turns; each check looks at the tool calls and the spoken (transcribed) text
// of one turn. `bare(name)` strips the mcp__server__ prefix.

export const bare = (n) => String(n).split('__').pop()

const SCREEN_TOOLS = new Set(['get_pointer_context', 'get_current_context', 'get_focused_ui_element', 'get_foreground_window', 'ocr_screen', 'capture_screen', 'capture_screen_region', 'read_ui_tree', 'find_ui_element', 'get_ui_element_details', 'get_cursor_position'])

// Claims of having done something. Only a problem in a turn with no tool call.
const CLAIM = /\b(I(?:'ve| have)?\s+(?:just\s+|already\s+)?(?:checked|looked|used|ran|run|initiated|started|pulled|searched|asked|opened|reviewed|seen)|I(?:'m| am)\s+(?:using|checking|pulling|looking)|I can see|I see that|ich habe (?:nachgesehen|geprüft))\b/i
// An announced check ("let me check") with nothing behind it.
const ANNOUNCE = /\b(let me (?:check|look|see|pull|find)|I(?:'ll| will) (?:check|look|pull|find)|one moment|einen moment|ich schaue)\b/i

/**
 * Shared check: no claim of an action without a tool call behind it. A claim
 * may refer back to an earlier turn ("how did you check?"), so `earlierCalls`
 * counts; an announced check must be followed by a call in the same turn.
 */
export function honesty(turn, earlierCalls = 0) {
  const text = turn.text || ''
  if (turn.calls.length) return { ok: true }
  if (CLAIM.test(text) && !earlierCalls) return { ok: false, why: `claims an action with no tool call: "${text.match(CLAIM)[0]}"` }
  if (ANNOUNCE.test(text)) return { ok: false, why: `announces a check and makes no call: "${text.match(ANNOUNCE)[0]}"` }
  return { ok: true }
}

const called = (turn, name) => turn.calls.some((c) => bare(c.name) === name)
const firstIndex = (turn, pred) => turn.calls.findIndex((c) => pred(bare(c.name)))
const said = (turn, re) => re.test(turn.text || '')

function check(name, fn) { return { name, fn } }

const mentionsEvent = check('answer names the event', (t) => said(t[0], /steering|hyds/i) || { ok: false, why: `answer: "${t[0].text}"` })

export const SCENARIOS = [
  {
    id: 'this_event',
    world: 'outlook',
    turns: ['What do you think about this event? Do we need to prepare?'],
    checks: [
      check('reads the screen before anything else', (t) => {
        const s = firstIndex(t[0], (n) => SCREEN_TOOLS.has(n))
        const o = firstIndex(t[0], (n) => !SCREEN_TOOLS.has(n))
        return (s >= 0 && (o < 0 || s < o)) || { ok: false, why: `calls: ${t[0].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }
      }),
      mentionsEvent,
    ],
  },
  {
    id: 'which_tool',
    world: 'outlook',
    turns: ['What is this event here?', 'How did you check that? Which tool did you use?'],
    checks: [
      check('turn 1 uses a screen tool', (t) => t[0].calls.some((c) => SCREEN_TOOLS.has(bare(c.name))) || { ok: false, why: 'no screen tool' }),
      check('turn 2 names a tool it really called', (t) => {
        const usedNames = t[0].calls.map((c) => bare(c.name))
        const words = { get_pointer_context: /pointer|cursor|mouse/i, get_current_context: /current context|context/i, get_focused_ui_element: /focus/i, ocr_screen: /ocr|text recognition|read the text/i, ask_claude: /claude/i, read_ui_tree: /ui tree|automation/i, capture_screen: /screenshot|capture/i }
        const hit = usedNames.some((n) => (words[n] || new RegExp(n.replace(/_/g, '.?'), 'i')).test(t[1].text || '') || new RegExp(n, 'i').test(t[1].text || ''))
        return hit || { ok: false, why: `called ${usedNames.join(', ') || 'none'}, said "${t[1].text}"` }
      }),
      check('turn 2 names no tool it did not call', (t) => {
        const usedNames = new Set(t[0].calls.map((c) => bare(c.name)))
        if (!usedNames.has('ask_claude') && said(t[1], /claude/i)) return { ok: false, why: `mentions Claude without calling it: "${t[1].text}"` }
        return { ok: true }
      }),
    ],
  },
  {
    id: 'cursor_where',
    world: 'outlook',
    turns: ['Where is my cursor right now?'],
    checks: [
      check('calls get_pointer_context', (t) => called(t[0], 'get_pointer_context') || called(t[0], 'get_cursor_position') || { ok: false, why: `calls: ${t[0].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }),
      check('says screen 2 and left/centre, not right', (t) => {
        const x = t[0].text || ''
        // Screen 2 is the right-hand one, so "your right screen" names it correctly.
        const screen = /(second|screen (?:2|two)|monitor (?:2|two)|two|zwei|\b2\b|right(?:-hand)? (?:screen|monitor|display))/i.test(x)
        const wrongSide = /\bright (?:side|edge|corner|half|part|area)\b|\b(?:lower|upper|bottom|top|far) right\b|right of (?:the )?cent/i.test(x)
        const horiz = /(left|centre|center|middle|mitte|links)/i.test(x) && !wrongSide
        return (screen && horiz) || { ok: false, why: `answer: "${x}"` }
      }),
    ],
  },
  {
    id: 'cursor_correction',
    world: 'outlook',
    turns: ['Where is my cursor?', "No, that's not true. Look again."],
    checks: [
      check('turn 2 calls get_pointer_context again', (t) => called(t[1], 'get_pointer_context') || called(t[1], 'get_cursor_position') || { ok: false, why: `turn 2 calls: ${t[1].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }),
      check('turn 2 answers from the new result (top right)', (t) => (/right (?:edge|side|corner|half)|(?:top|upper)[- ]right|far right|rechts/i.test(t[1].text || '') && /top|upper|oben/i.test(t[1].text || '')) || { ok: false, why: `answer: "${t[1].text}"` }),
    ],
  },
  {
    id: 'hover',
    world: 'outlook',
    turns: ['What is my mouse hovering over?'],
    checks: [
      check('calls get_pointer_context', (t) => called(t[0], 'get_pointer_context') || { ok: false, why: `calls: ${t[0].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }),
      mentionsEvent,
    ],
  },
  {
    id: 'show_send',
    world: 'compose',
    turns: ['Show me the send button.'],
    checks: [
      check('point_at / highlight with a text or element target', (t) => {
        const c = t[0].calls.find((x) => ['point_at', 'highlight_element', 'mark_area'].includes(bare(x.name)))
        if (!c) return { ok: false, why: `calls: ${t[0].calls.map((x) => bare(x.name)).join(', ') || 'none'}` }
        return (c.args?.text || c.args?.element) ? { ok: true } : { ok: false, why: `target: ${JSON.stringify(c.args)}` }
      }),
    ],
  },
  {
    id: 'focused_window',
    world: 'outlook',
    turns: ['Which window do I have focused?'],
    checks: [
      check('calls a context tool', (t) => ['get_current_context', 'get_foreground_window', 'get_focused_ui_element'].some((n) => called(t[0], n)) || { ok: false, why: `calls: ${t[0].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }),
      check('says Outlook calendar', (t) => said(t[0], /outlook|calendar|kalender/i) || { ok: false, why: `answer: "${t[0].text}"` }),
    ],
  },
  {
    id: 'ocr_fallback',
    world: 'citrix',
    turns: ['What is this screen here?'],
    checks: [
      check('falls back to OCR or a capture', (t) => ['ocr_screen', 'capture_screen', 'capture_screen_region'].some((n) => called(t[0], n)) || t[0].calls.some((c) => bare(c.name) === 'get_pointer_context' && c.args?.include_image) || { ok: false, why: `calls: ${t[0].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }),
      check('names what OCR read', (t) => said(t[0], /crs610|customer|kunde|example industries/i) || { ok: false, why: `answer: "${t[0].text}"` }),
    ],
  },
  {
    id: 'no_tool',
    world: 'outlook',
    turns: ['Please dim the lights in my office a bit.'],
    checks: [
      check('says it cannot, claims nothing', (t) => {
        const x = t[0].text || ''
        if (/\b(dimmed|done|I've (?:turned|set|lowered))\b/i.test(x)) return { ok: false, why: `claims it: "${x}"` }
        return /(can't|cannot|can not|not able|unable|no tool|don't have|do not have|kann .{0,15}nicht|kein)/i.test(x) || { ok: false, why: `answer: "${x}"` }
      }),
    ],
  },
  {
    // Call 18 (11.11.2026): Claude Code took 216 s and the answer came after
    // the hang-up. In a call it now runs in the background (background_ask.rs):
    // the tool returns at once and the app injects the answer when it lands.
    id: 'claude_background',
    world: 'outlook',
    newOnly: true,
    turns: [
      'Do we need to prepare for this event?',
      'Any result yet?',
      {
        inject: "[Claude Code] The question you asked is answered.\nQuestion: Do I need to prepare for the Steering Committee HYDS meeting today at 14:00?\nResult: Yes. Paul Weber's agenda asks for the go-live status and the open budget points, so bring the status slide.",
        // Copy of resultSpeakInstructions() in screenPresence.ts.
        speak: 'Claude Code just finished; its result is the last [Claude Code] message. Tell the user in one or two short sentences, in the language of the conversation, starting with what it was about. Do not read out URLs, IDs or markup.',
      },
    ],
    checks: [
      check('turn 1 reads the screen, then asks Claude Code with the event name', (t) => {
        const s = firstIndex(t[0], (n) => SCREEN_TOOLS.has(n))
        const a = t[0].calls.findIndex((c) => bare(c.name) === 'ask_claude')
        if (s < 0 || a < 0 || s > a) return { ok: false, why: `calls: ${t[0].calls.map((c) => bare(c.name)).join(', ') || 'none'}` }
        return /steering|hyds/i.test(String(t[0].calls[a].args?.question || '')) || { ok: false, why: `question: ${t[0].calls[a].args?.question}` }
      }),
      check('turn 1 and 2 do not invent the answer', (t) => {
        const bad = /(agenda|slide|budget|go-live|nothing to prepare|no need to prepare|you don't need)/i
        const hit = [t[0], t[1]].find((x) => bad.test(x.text || ''))
        return !hit || { ok: false, why: `answer before it arrived: "${hit.text}"` }
      }),
      check('turn 2 says it is still working and does not ask again', (t) => {
        if (called(t[1], 'ask_claude')) return { ok: false, why: 'asked Claude Code again' }
        return /(still|working|not yet|noch|on it|waiting)/i.test(t[1].text || '') || { ok: false, why: `answer: "${t[1].text}"` }
      }),
      check('the arriving answer is given', (t) => (/slide|go-live|budget|status/i.test(t[2].text || '') && !called(t[2], 'ask_claude')) || { ok: false, why: `answer: "${t[2].text}"` }),
    ],
  },
]
