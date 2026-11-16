import { invoke } from '@tauri-apps/api/core'
import { guessLanguage, replyLanguageRule } from './voiceInstructions'

// Screen presence for the voice session: what windows-commander says the user
// is looking at, and results of voice tool calls that finished in the
// background. Shared by the OpenAI Realtime and Gemini Live clients so both
// say the same thing. The instructions about tools and the screen live in
// voiceInstructions.ts.

/**
 * A voice tool call that took longer, went to the background
 * (background_tools.rs) and has now finished, as background_results.rs emits
 * it. `agent` is the tool's name.
 */
export interface BackgroundResult {
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

/** Which tool did the work. */
export function agentOf(o: BackgroundResult): string {
  return (o.agent && o.agent.trim()) || 'background job'
}

/** What the model is told when a background job ends. */
export function backgroundResultText(o: BackgroundResult): string {
  const agent = agentOf(o)
  const what = `The ${agent} call you started earlier`
  if (o.status === 'completed') {
    return `[${agent}] ${what} has finished.\nRequest: ${o.task}\nResult: ${o.output || '(no text returned)'}`
  }
  return `[${agent}] ${what} did not finish (${o.status}).\nRequest: ${o.task}\nReason: ${o.error || 'unknown'}`
}

/**
 * How the model is asked to speak a result that just arrived. `userTexts` are
 * the user's transcribed turns, for the language (voiceInstructions.ts).
 */
export function resultSpeakInstructions(o: BackgroundResult, userTexts: string[] = []): string {
  const agent = agentOf(o)
  return `${agent} just finished; its result is the last [${agent}] message. Tell the user in one or two short sentences, starting with what it was about. Do not read out URLs, IDs or markup. ${replyLanguageRule(guessLanguage(userTexts))}`
}

/** Resolves with `p`, or with null after `ms`. */
export function withinMs<T>(p: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([p, new Promise<null>((r) => setTimeout(() => r(null), ms))])
}
