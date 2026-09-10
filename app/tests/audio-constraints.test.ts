import { test } from 'node:test'
import assert from 'node:assert/strict'

import { buildAudioConstraints, isVoiceIsolationSupported } from '../src/audioConstraints.ts'

test('pins the processing flags to the Chromium defaults when nothing is configured', () => {
  const c = buildAudioConstraints({})

  assert.equal(c.echoCancellation, true)
  assert.equal(c.noiseSuppression, true)
  assert.equal(c.autoGainControl, true)
})

test('honours a flag the user turned off', () => {
  const c = buildAudioConstraints({ stt_auto_gain_control: false })

  assert.equal(c.autoGainControl, false)
  assert.equal(c.echoCancellation, true)
})

test('carries a chosen input device through as an exact constraint', () => {
  const c = buildAudioConstraints({}, 'device-abc')

  assert.deepEqual(c.deviceId, { exact: 'device-abc' })
})

test('omits deviceId entirely when no device is chosen', () => {
  const c = buildAudioConstraints({}, '')

  assert.equal('deviceId' in c, false)
})

test('treats a whitespace-only device id as no device', () => {
  const c = buildAudioConstraints({}, '   ')

  assert.equal('deviceId' in c, false)
})

test('omits voiceIsolation when the device does not support it', () => {
  const c = buildAudioConstraints({ stt_voice_isolation: true }, '', false)

  assert.equal('voiceIsolation' in c, false)
})

test('omits voiceIsolation when it is supported but switched off', () => {
  const c = buildAudioConstraints({ stt_voice_isolation: false }, '', true)

  assert.equal('voiceIsolation' in c, false)
})

test('requests voiceIsolation only when it is both enabled and supported', () => {
  const c = buildAudioConstraints({ stt_voice_isolation: true }, '', true)

  assert.equal((c as Record<string, unknown>).voiceIsolation, true)
})

test('defaults voiceIsolation to off even where it is supported', () => {
  const c = buildAudioConstraints({}, '', true)

  assert.equal('voiceIsolation' in c, false)
})

test('ignores non-boolean values stored in settings by an older build', () => {
  const c = buildAudioConstraints({ stt_echo_cancellation: 'yes' as unknown as boolean })

  assert.equal(c.echoCancellation, true)
})

/**
 * Run `fn` with `globalThis.navigator` replaced, then put the real one back.
 *
 * Node defines `navigator` as a configurable accessor, so this swaps the whole
 * property rather than assigning through it.
 */
function withNavigator(stub: unknown, fn: () => void): void {
  const original = Object.getOwnPropertyDescriptor(globalThis, 'navigator')
  Object.defineProperty(globalThis, 'navigator', { value: stub, configurable: true, writable: true })
  try {
    fn()
  } finally {
    if (original) Object.defineProperty(globalThis, 'navigator', original)
    else delete (globalThis as Record<string, unknown>).navigator
  }
}

test('reports voice isolation as unsupported where the runtime has no mediaDevices', () => {
  withNavigator({}, () => {
    assert.equal(isVoiceIsolationSupported(), false)
  })
})

test('reports voice isolation as unsupported when the runtime does not list it', () => {
  withNavigator(
    { mediaDevices: { getSupportedConstraints: () => ({ echoCancellation: true }) } },
    () => { assert.equal(isVoiceIsolationSupported(), false) },
  )
})

test('reports voice isolation as supported when the runtime lists it', () => {
  withNavigator(
    { mediaDevices: { getSupportedConstraints: () => ({ voiceIsolation: true }) } },
    () => { assert.equal(isVoiceIsolationSupported(), true) },
  )
})

test('reports voice isolation as unsupported when the probe itself throws', () => {
  withNavigator(
    { mediaDevices: { getSupportedConstraints: () => { throw new Error('no') } } },
    () => { assert.equal(isVoiceIsolationSupported(), false) },
  )
})
