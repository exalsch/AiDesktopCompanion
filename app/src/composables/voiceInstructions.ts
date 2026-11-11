// The system instructions of a voice session, shared by the OpenAI Realtime
// and the Gemini Live clients, and loaded as is by scripts/voice-eval.mjs and
// tests/voice-instructions.test.ts. Keep this file free of imports so Node can
// run it with type stripping.
//
// Realtime voice models follow short, explicit rules far better than prose,
// and they act on the last thing they read. So: the user's own instructions
// first, then these rules, then the one-line arrangement note.
//
// The rules came out of a real call (specs/ScreenPresence_design.md, "Voice
// tool rules") in which the model said "I've checked your calendar" without a
// tool call, guessed the cursor position, agreed with a correction instead of
// looking again, and asked Claude Code about the event on screen, which it
// cannot see. Every rule below answers one of those failures; the eval checks
// each of them.

export const HERMES_TOOL_NAME = 'delegate_to_hermes'
export const SUPERVISOR_TOOL_NAME = 'consult_supervisor'

/** Prefix of every machine-written screen context message. */
export const SCREEN_CONTEXT_PREFIX = '[Screen context'

export type VoiceArrangement = 'tools' | 'no-tools' | 'supervisor-always' | 'supervisor-needed' | 'supervisor-voice-only'

export const DEFAULT_VOICE_INSTRUCTIONS =
  'You are an assistant in Assistant Mode. Speak clearly and concisely. IMPORTANT: Always reply in the same language the user is speaking. If you are unsure, reply in English. Do not switch languages mid-conversation unless the user clearly switches.'

/** Tool names arrive mangled (`mcp__server__point_at`), so match by suffix. */
export function hasTool(toolNames: string[], tool: string): boolean {
  return toolNames.some((n) => n === tool || n.endsWith(`__${tool}`))
}

/** The name the model must use for `tool` in this session, or the bare name. */
function nameOf(toolNames: string[], tool: string): string {
  return toolNames.find((n) => n === tool || n.endsWith(`__${tool}`)) || tool
}

const HONESTY_RULES = [
  'Honesty',
  '- Never say you checked, looked, saw, ran, used or started something unless a tool call in this turn did it.',
  '- If you say "let me check", call the tool right away in the same turn. Never announce a check and then stop.',
  '- When the user asks how you did something or which tool you used, name the tools you really called in this conversation. If you called none, say you answered without a tool.',
  '- Use only what a tool returned. Never invent names, times, IDs or tickets.',
  '- If none of your tools can do what the user asks, say so in one short sentence.',
].join('\n')

/**
 * The screen rules, one line per kind of question, with the exact tool names
 * this session has. Only rules whose tools exist are included.
 */
function screenRules(toolNames: string[]): string {
  const has = (t: string) => hasTool(toolNames, t)
  const n = (t: string) => nameOf(toolNames, t)
  const lines: string[] = []
  if (has('get_pointer_context')) {
    const fallback: string[] = []
    if (has('get_focused_ui_element')) fallback.push(`call ${n('get_focused_ui_element')} (the item the user clicked)`)
    if (has('ocr_screen')) fallback.push(`call ${n('ocr_screen')} with target "full_screen" and a region of about 500 x 200 pixels centred on the cursor x and y`)
    lines.push(
      `- "this", "that", "here", "this event / mail / button": call ${n('get_pointer_context')} first and use the element under the cursor ("resolved_element" when the result has one).` +
      (fallback.length ? ` If its name is empty or it is only a window or pane, ${fallback.join(', then ')}.` : ''),
    )
    lines.push('- Start the answer with the name of what you read, so the user hears that you looked at the right thing.')
    lines.push(`- "where is my cursor / mouse": call ${n('get_pointer_context')} and answer only from its "position" text. Never guess a position.`)
    lines.push(`- "what does it hover / point at": call ${n('get_pointer_context')} and say the name of the element.`)
  }
  if (has('get_current_context')) {
    lines.push(`- "which window / app is in front / focused": call ${n('get_current_context')}.`)
  }
  if (has('point_at')) {
    const elementHint = has('find_ui_element') ? ` or an element_ref from ${n('find_ui_element')} or ${n('get_pointer_context')}` : ''
    lines.push(`- "show me X" / "where is X": call ${n('point_at')} with text "X"${elementHint}. Never pass guessed coordinates. Then say one short sentence, for example "There, the arrow points at it."`)
  }
  if (has('clear_annotations')) {
    lines.push(`- Call ${n('clear_annotations')} when the user moves on or asks you to remove the marks.`)
  }
  if (!lines.length) return ''
  return [
    'The screen',
    ...lines,
    '- When the user says you are wrong about the screen, call the tool again and answer from the new result. Do not simply agree.',
    '- Tools that work on the user\'s mail, calendar or files cannot see the screen. Read the screen first, then put the name you read into their request.',
    '- Do not read out element_refs, coordinates, handles or class names.',
  ].join('\n')
}

function contextRule(toolNames: string[]): string {
  if (!hasTool(toolNames, 'get_current_context') && !hasTool(toolNames, 'get_pointer_context')) return ''
  return `Messages that start with ${SCREEN_CONTEXT_PREFIX} are written by the app, not said by the user. They can be a few seconds old: use them as a hint, but call the tool for the cursor, the focus and "this". Never read them out.`
}

function hermesRule(toolNames: string[]): string {
  if (!hasTool(toolNames, HERMES_TOOL_NAME)) return ''
  return `Long jobs: for research, mail and calendar work or anything with many steps, call ${HERMES_TOOL_NAME} with a complete, standalone task that includes what you read from the screen. It returns at once: say in a few words that Hermes is on it, then carry on. Its result arrives later as a [Hermes] message; report it in your own short words.`
}

function examples(toolNames: string[]): string {
  const has = (t: string) => hasTool(toolNames, t)
  const n = (t: string) => nameOf(toolNames, t)
  const ex: string[] = []
  if (has('get_pointer_context')) {
    ex.push(`User: "What is this meeting?" -> call ${n('get_pointer_context')} -> element "Budget review, 10:00 to 10:30, ..." -> "That is the budget review at ten."`)
    ex.push(`User: "How did you check?" -> "I called ${n('get_pointer_context')} and read the event under your cursor."`)
    ex.push(`User: "No, the cursor is on the left." -> call ${n('get_pointer_context')} again -> answer from the new result.`)
  }
  if (has('point_at')) ex.push(`User: "Show me the save button." -> call ${n('point_at')} with text "Save" -> "There, the arrow points at it."`)
  ex.push('User: "Dim the lights." -> "I cannot do that. I have no tool for the lights."')
  return ['Examples', ...ex.map((e) => `- ${e}`)].join('\n')
}

/**
 * The tool and screen part of the instructions, depending on the tools this
 * session really has. Empty when it has none.
 */
export function voiceToolNote(toolNames: string[]): string {
  if (!toolNames.length) return ''
  const blocks = [HONESTY_RULES, screenRules(toolNames), contextRule(toolNames), hermesRule(toolNames), examples(toolNames)]
  return ['# Tools', ...blocks.filter(Boolean)].join('\n\n')
}

/** One line on how this session is arranged. It goes last, so it wins. */
export function arrangementNote(arrangement: VoiceArrangement): string {
  switch (arrangement) {
    case 'supervisor-always':
      return `For every request the user makes, call ${SUPERVISOR_TOOL_NAME} with it and read its answer back word for word. Do not answer on your own, and do not describe this arrangement to the user.`
    case 'supervisor-needed':
      return `Call ${SUPERVISOR_TOOL_NAME} whenever a question needs current information, the user's files or applications, or careful reasoning, and read its answer back. It is the only tool you have; there are no others in this session.`
    case 'supervisor-voice-only':
      return 'A supervisor model answers on your behalf. Do not describe that arrangement to the user.'
    case 'tools':
      return 'There is no supervisor in this session, so do not mention one or wait for one. Call your tools yourself when a question needs them. Screen tools answer in under a second: call them without announcing it. Tools that take long (Claude Code, Hermes): say a few words first, then call the tool in the same turn, and give the answer when it arrives.'
    case 'no-tools':
    default:
      return 'There is no supervisor and no tools in this session. Answer from your own knowledge, and say plainly when something is beyond it.'
  }
}

/**
 * The full system instructions: the user's own text (kept as written), then
 * the tool rules, then the arrangement note.
 */
export function buildVoiceInstructions(userInstructions: string | undefined, toolNames: string[], arrangement: VoiceArrangement): string {
  const base = (userInstructions && userInstructions.trim()) ? userInstructions.trim() : DEFAULT_VOICE_INSTRUCTIONS
  const note = arrangement === 'tools' ? voiceToolNote(toolNames) : ''
  return [base, note, arrangementNote(arrangement)].filter(Boolean).join('\n\n')
}
