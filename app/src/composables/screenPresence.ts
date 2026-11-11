import { invoke } from '@tauri-apps/api/core'
import { guessLanguage, replyLanguageRule } from './voiceInstructions'

// Screen presence for the voice session: what windows-commander says the user
// is looking at, and Hermes results coming back. Shared by the OpenAI Realtime and Gemini Live
// clients so both say the same thing.

// The instructions about tools and the screen live in voiceInstructions.ts.
export { HERMES_TOOL_NAME } from './voiceInstructions'

/**
 * A finished background job, as hermes.rs emits it: a Hermes run, or a voice
 * tool call that took longer and went to the background (background_tools.rs).
 * `agent` names which.
 */
export interface HermesOutcome {
  /** "Hermes" or the tool's name; missing from results older than the field. */
  agent?: string
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

/** Who did the work: "Hermes" for older results without the field. */
export function agentOf(o: HermesOutcome): string {
  return (o.agent && o.agent.trim()) || 'Hermes'
}

/** What the model is told when a background job ends. */
export function hermesResultText(o: HermesOutcome): string {
  const agent = agentOf(o)
  const what = agent === 'Hermes' ? 'The task you handed over' : `The ${agent} call you started earlier`
  const label = agent === 'Hermes' ? 'Task' : 'Request'
  if (o.status === 'completed') {
    return `[${agent}] ${what} has finished.\n${label}: ${o.task}\nResult: ${o.output || '(no text returned)'}`
  }
  return `[${agent}] ${what} did not finish (${o.status}).\n${label}: ${o.task}\nReason: ${o.error || 'unknown'}`
}

/**
 * How the model is asked to speak a result that just arrived. `userTexts` are
 * the user's transcribed turns, for the language (voiceInstructions.ts).
 */
export function resultSpeakInstructions(o: HermesOutcome, userTexts: string[] = []): string {
  const agent = agentOf(o)
  return `${agent} just finished; its result is the last [${agent}] message. Tell the user in one or two short sentences, starting with what it was about. Do not read out URLs, IDs or markup. ${replyLanguageRule(guessLanguage(userTexts))}`
}

/** Resolves with `p`, or with null after `ms`. */
export function withinMs<T>(p: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([p, new Promise<null>((r) => setTimeout(() => r(null), ms))])
}
