import { test } from 'node:test'
import assert from 'node:assert/strict'
import { costUsd, ratesFor, type UsageTotals } from '../src/composables/useRealtimeUsage.ts'

const zero: UsageTotals = { audioIn: 0, audioInCached: 0, audioOut: 0, textIn: 0, textInCached: 0, textOut: 0, imageIn: 0, responses: 0 }

test('OpenAI realtime: the minis never get the flagship rate', () => {
  assert.equal(ratesFor('gpt-realtime-2.1-mini')?.audioIn, 10)
  assert.equal(ratesFor('gpt-realtime-mini-2025-12-15')?.audioIn, 10)
  assert.equal(ratesFor('gpt-realtime-2.1-2026-10-01')?.audioIn, 32)
})

test('OpenAI realtime: 2.x text output is $24, 1.5 and the original $16', () => {
  assert.equal(ratesFor('gpt-realtime-2.1')?.textOut, 24)
  assert.equal(ratesFor('gpt-realtime-2')?.textOut, 24)
  assert.equal(ratesFor('gpt-realtime-1.5')?.textOut, 16)
  assert.equal(ratesFor('gpt-realtime')?.textOut, 16)
})

test('Gemini Live models are priced, variants and models/ prefix included', () => {
  for (const id of ['gemini-3.8-live', 'gemini-3.8-live-extended-thinking', 'models/gemini-3.1-flash-live-preview']) {
    const r = ratesFor(id)
    assert.ok(r, id)
    assert.equal(r.audioIn, 3)
    assert.equal(r.audioOut, 12)
    assert.equal(r.textIn, 0.75)
    assert.equal(r.textOut, 4.5)
  }
  assert.equal(ratesFor('gemini-2.5-flash-native-audio-preview-12-2025')?.textOut, 2)
})

test('unknown models stay unpriced', () => {
  assert.equal(ratesFor('gemini-3.5-live-translate-preview'), null)
  assert.equal(ratesFor(''), null)
  assert.equal(ratesFor(undefined), null)
})

test('cost adds every modality per million tokens', () => {
  const t: UsageTotals = { ...zero, audioIn: 1_000_000, audioOut: 500_000, textIn: 200_000, textOut: 100_000, imageIn: 10_000 }
  // 3 + 6 + 0.15 + 0.45 + 0.01
  assert.equal(Number(costUsd(t, ratesFor('gemini-3.8-live')!).toFixed(4)), 9.61)
  assert.equal(costUsd(zero, ratesFor('gpt-realtime-2.1')!), 0)
})
