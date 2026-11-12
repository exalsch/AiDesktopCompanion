import { test } from 'node:test'
import assert from 'node:assert/strict'

import { buildVoiceInstructions, voiceToolNote, arrangementNote, SCREEN_CONTEXT_PREFIX, guessLanguage, replyLanguageRule } from '../src/composables/voiceInstructions.ts'
import { toolTraceLine, bareToolName } from '../src/composables/toolTrace.ts'

const SCREEN = [
  'mcp__windows-commander__get_current_context',
  'mcp__windows-commander__get_pointer_context',
  'mcp__windows-commander__get_focused_ui_element',
  'mcp__windows-commander__ocr_screen',
  'mcp__windows-commander__point_at',
  'mcp__windows-commander__clear_annotations',
  'mcp__Rob__ask_claude',
]

test('user instructions stay first and unchanged, the arrangement note last', () => {
  const user = 'You are Alex\'s desktop wingman.\nKeep it short.'
  const text = buildVoiceInstructions(user, SCREEN, 'tools')
  assert.ok(text.startsWith(user))
  assert.ok(text.endsWith(arrangementNote('tools')))
})

test('the rules name the exact tool names of the session', () => {
  const note = voiceToolNote(SCREEN)
  assert.match(note, /mcp__windows-commander__get_pointer_context first/)
  assert.match(note, /call mcp__windows-commander__point_at with text "X"/)
  assert.match(note, /Never say you checked/)
  assert.ok(note.includes(SCREEN_CONTEXT_PREFIX))
})

test('background tools are described generically, without naming any server', () => {
  const note = voiceToolNote(SCREEN)
  assert.match(note, /status "started" means the work continues in the background/)
  assert.match(note, /starts with \[tool name\]/)
  // The rules must not depend on one user's tools.
  assert.doesNotMatch(note, /Claude Code|ask_claude/)
  assert.match(voiceToolNote(['mcp__files__search']), /Tools that take longer/)
})

test('the reply language comes from what the user said', () => {
  assert.equal(guessLanguage(['This meeting I am looking at, is there anything I should prepare for?']), 'English')
  assert.equal(guessLanguage(['Was muss ich für diesen Termin vorbereiten?']), 'German')
  // The last turn counts most.
  assert.equal(guessLanguage(['Was ist das?', 'Can you check this meeting for me please?']), 'English')
  // Call 19: Gemini transcribed the user as Spanish. Neither shows: English.
  assert.equal(guessLanguage(['¿Qué debo preparar para esta reunión?']), null)
  assert.match(replyLanguageRule(null), /English only/)
  assert.match(replyLanguageRule('German'), /Antworte auf Deutsch/)
})

test('a short message in another language counts as misheard', () => {
  // Call 25: "what?" heard as "¿Qué?", answered in Spanish.
  const note = voiceToolNote(['mcp__windows-commander__get_pointer_context'])
  assert.match(note, /most of this call/)
  assert.match(note, /misheard/)
  assert.match(note, /¿Qué\?/)
})

test('a draft stays unsent, also through the desktop agent', () => {
  // Call 25: "draft the message in Teams" ended with the message sent.
  const note = voiceToolNote(['mcp__windows-commander__type_text', 'desktop_agent'])
  assert.match(note, /leave it unsent/)
  assert.match(note, /end the task with "Do not send it\."/)
  assert.match(note, /Ask about the step that will really happen/)
  assert.doesNotMatch(voiceToolNote(['mcp__windows-commander__type_text']), /end the task with/)
})

test('trace line: a background job shows as started', () => {
  assert.equal(toolTraceLine('mcp__Rob__ask_claude', '{"question":"Prepare?"}', '{"status":"started","job_id":"cc-1","note":"..."}'),
    'ask_claude (Prepare?) -> started in the background')
})

test('action rules: click by element_ref, confirm before outward actions, agent for many steps', () => {
  const tools = [...SCREEN, 'mcp__windows-commander__invoke_ui_element', 'mcp__windows-commander__type_text', 'mcp__windows-commander__send_hotkey', 'mcp__windows-commander__find_ui_element', 'desktop_agent']
  const note = voiceToolNote(tools)
  assert.match(note, /mcp__windows-commander__invoke_ui_element with the element_ref of resolved_element/)
  assert.match(note, /wait for a clear yes from the user/)
  assert.match(note, /call desktop_agent with a complete task/)
  // No action tools, no action rules.
  assert.doesNotMatch(voiceToolNote(SCREEN), /Doing things/)
})

test('trace line: actions', () => {
  const ok = JSON.stringify({ serverId: 'w', tool: 'send_hotkey', result: { content: [{ type: 'text', text: '{"ok":true}' }] } })
  assert.equal(toolTraceLine('mcp__w__send_hotkey', '{"modifiers":["ctrl"],"key":"s"}', ok), 'send_hotkey (ctrl+s) -> done')
  assert.equal(toolTraceLine('mcp__w__invoke_ui_element', '{"element_ref":"abc","action":"invoke"}', ok), 'invoke_ui_element (element) -> done')
})

test('rules for missing tools are left out', () => {
  const note = voiceToolNote(['mcp__Rob__ask_claude'])
  assert.doesNotMatch(note, /get_pointer_context/)
  assert.doesNotMatch(note, /point_at/)
  assert.match(note, /Honesty/)
  assert.equal(voiceToolNote([]), '')
})

test('no tools or a supervisor gets no tool rules', () => {
  assert.doesNotMatch(buildVoiceInstructions('', SCREEN, 'no-tools'), /# Tools/)
  assert.doesNotMatch(buildVoiceInstructions('', SCREEN, 'supervisor-always'), /# Tools/)
  assert.match(buildVoiceInstructions('', [], 'no-tools'), /Assistant Mode/)
})

test('the examples do not give away the eval fixtures', () => {
  assert.doesNotMatch(voiceToolNote(SCREEN), /Steering|HYDS|CRS610/)
})

test('trace line: pointer context shows the resolved element and position', () => {
  const output = JSON.stringify({
    serverId: 'windows-commander', tool: 'get_pointer_context',
    result: { content: [{ type: 'text', text: JSON.stringify({
      x: 1, y: 2, element: { controlType: 'Window', name: 'Calendar - Outlook' },
      resolved_element: { controlType: 'Button', name: 'Steering Committee, 14:00 to 15:00' },
      position: 'On screen 2: horizontally left of centre (38 %), vertically below the middle (63 %).',
    }) }] },
  })
  const line = toolTraceLine('mcp__windows-commander__get_pointer_context', '{}', output)
  assert.match(line, /^get_pointer_context -> Button "Steering Committee, 14:00 to 15:00" · On screen 2/)
})

test('trace line: a result with an image still shows the element', () => {
  // Seen in call 18: get_pointer_context with include_image traced as the image note.
  const output = JSON.stringify({
    serverId: 'windows-commander', tool: 'get_pointer_context',
    result: { content: [
      { type: 'text', text: '[image/png image, 12 KB: sent to you as the next user message.]' },
      { type: 'text', text: JSON.stringify({ element: { controlType: 'Window', name: 'Calendar' }, resolved_element: { controlType: 'Button', name: 'Release sync, 16:00 to 16:30' } }) },
    ] },
  })
  assert.equal(toolTraceLine('mcp__windows-commander__get_pointer_context', '{"include_image":true}', output),
    'get_pointer_context -> Button "Release sync, 16:00 to 16:30"')
})

test('trace line: arguments, errors and plain answers', () => {
  const pointed = JSON.stringify({ serverId: 'w', tool: 'point_at', result: { content: [{ type: 'text', text: '{"id":"a1"}' }] } })
  assert.equal(toolTraceLine('mcp__w__point_at', '{"text":"Send"}', pointed), 'point_at (Send) -> drawn on screen')
  assert.equal(toolTraceLine('mcp__w__mouse_action', '{}', '{"error":"mcp__w__mouse_action is not available in voice sessions"}'),
    'mouse_action -> error: mcp__w__mouse_action is not available in voice sessions')
  const failed = JSON.stringify({ serverId: 'w', tool: 'point_at', result: { content: [{ type: 'text', text: 'Text not found' }], isError: true } })
  assert.equal(toolTraceLine('mcp__w__point_at', '{"text":"Nope"}', failed), 'point_at (Nope) -> error: Text not found')
  assert.equal(toolTraceLine('consult_supervisor', '{"question":"why"}', '{"answer":"Because."}'), 'consult_supervisor (why) -> Because.')
  assert.equal(bareToolName('delegate_to_hermes'), 'delegate_to_hermes')
  assert.ok(toolTraceLine('x', '', 'not json at all').endsWith('not json at all'))
})
