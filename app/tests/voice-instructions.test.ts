import { test } from 'node:test'
import assert from 'node:assert/strict'

import { buildVoiceInstructions, voiceToolNote, arrangementNote, SCREEN_CONTEXT_PREFIX } from '../src/composables/voiceInstructions.ts'
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
