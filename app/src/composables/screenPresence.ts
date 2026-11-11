import { invoke } from '@tauri-apps/api/core'

// Screen presence for the voice session: what windows-commander says the user
// is looking at, and Hermes results coming back. Shared by the OpenAI Realtime and Gemini Live
// clients so both say the same thing.

// The instructions about tools and the screen live in voiceInstructions.ts.
export { HERMES_TOOL_NAME } from './voiceInstructions'

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
