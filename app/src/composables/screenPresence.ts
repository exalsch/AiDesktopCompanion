import { invoke } from '@tauri-apps/api/core'

// Screen presence for the voice session: what windows-commander says the user
// is looking at, the note that tells the model it can point at things, and
// Hermes results coming back. Shared by the OpenAI Realtime and Gemini Live
// clients so both say the same thing.

export const HERMES_TOOL_NAME = 'delegate_to_hermes'

const ANNOTATION_TOOLS = ['point_at', 'mark_area', 'highlight_element', 'clear_annotations']

/** A finished Hermes run, as hermes.rs emits it. */
export interface HermesOutcome {
  runId: string
  task: string
  status: string
  output: string
  error: string
  speakResult: boolean
}

/**
 * Lines appended to the system instructions, depending on which tools this
 * session actually has. Tool names arrive mangled (`mcp__server__point_at`),
 * so they are matched by suffix.
 */
export function screenPresenceNote(toolNames: string[]): string {
  const has = (t: string) => toolNames.some((n) => n === t || n.endsWith(`__${t}`))
  const lines: string[] = []
  if (ANNOTATION_TOOLS.some(has)) {
    lines.push(
      'You can point at things on the user\'s screen: point_at draws an arrow with a short label, mark_area and highlight_element frame something. ' +
      'Prefer an element target (an element_ref from get_pointer_context or the screen context) or a text target over raw coordinates. ' +
      'Use it when showing beats describing, keep labels to a few words, and call clear_annotations when you are done or the user moves on.'
    )
  }
  if (has('get_current_context') || has('get_pointer_context')) {
    lines.push(
      'Messages that start with [Screen context] describe what is on the user\'s screen right now. They come from the system, not from the user. ' +
      'Use them to resolve "this", "here" or "that window", never read them out, and do not mention them unless it helps.'
    )
  }
  if (has(HERMES_TOOL_NAME)) {
    lines.push(
      `For anything that takes more than a few seconds or many steps (research, mail and calendar work, multi-step jobs), call ${HERMES_TOOL_NAME} with a complete, standalone task. ` +
      'It returns at once: say in a few words that Hermes is on it, then carry on. Its result arrives later as a [Hermes] message; report it briefly in your own words.'
    )
  }
  return lines.join('\n')
}

/**
 * Context item for the turn that is starting, or null when the screen has not
 * changed since the last one, windows-commander is not connected, or screen
 * context is switched off. `reset` belongs to the first turn of a session.
 */
export async function fetchTurnContext(reset: boolean): Promise<string | null> {
  try {
    const text = await invoke<string | null>('assistant_screen_context', { reset })
    return typeof text === 'string' && text.trim() ? text : null
  } catch {
    return null
  }
}

/** What the model is told when a Hermes run ends. */
export function hermesResultText(o: HermesOutcome): string {
  if (o.status === 'completed') {
    return `[Hermes] The task you handed over is done.\nTask: ${o.task}\nResult: ${o.output || '(no text returned)'}`
  }
  return `[Hermes] The task you handed over did not finish (${o.status}).\nTask: ${o.task}\nReason: ${o.error || 'unknown'}`
}

export const HERMES_SPEAK_INSTRUCTIONS =
  'A Hermes task just finished; its result is the last [Hermes] message. Tell the user in one or two short sentences, in the language of the conversation. Do not read out URLs, IDs or markup.'

/** Resolves with `p`, or with null after `ms`. */
export function withinMs<T>(p: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([p, new Promise<null>((r) => setTimeout(() => r(null), ms))])
}
