#!/usr/bin/env node
// Scripted eval of the voice assistant's tool use.
//
// Opens a real Gemini Live (or OpenAI Realtime) session with the same system
// instructions and tool declarations the app builds, sends scripted TEXT user
// turns, answers every tool call from a canned desktop (world.mjs) and checks
// what the model called and said (scenarios.mjs).
//
//   node scripts/voice-eval/run.mjs                       # gemini, baseline + new, 2 runs
//   node scripts/voice-eval/run.mjs --variants new --runs 3 --only cursor_where
//   node scripts/voice-eval/run.mjs --provider openai --model gpt-realtime-2.1
//
// Variants:
//   baseline   the instructions and tool set before this change: the user's
//              instructions + the old 3-sentence screen note, all 69
//              windows-commander tools + Rob, old [Screen context] item at
//              each turn start, raw tool results.
//   new        voiceInstructions.ts, the "screen" voice profile, the new
//              context item, tool results with the app layer (position,
//              resolved_element).
//   new-noctx  as new, without the automatic [Screen context] item.
//
// Keys: GEMINI_API_KEY / GOOGLE_API_KEY / OPENAI_API_KEY from the environment,
// else from settings.json. They are never printed. The user's instructions
// come from settings.json (assistant_realtime.instructions).
//
// Text in instead of audio: the Live models get clientContent text turns, so
// this measures tool choice and honesty, not speech recognition.

import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { SCENARIOS, honesty, bare } from './scenarios.mjs'
import { cannedResult, contextItem } from './world.mjs'

const here = path.dirname(fileURLToPath(import.meta.url))
const appDir = path.resolve(here, '..', '..')
const { buildVoiceInstructions } = await import(pathToFileURL(path.join(appDir, 'src', 'composables', 'voiceInstructions.ts')).href)

// ---- args -----------------------------------------------------------------

const argv = process.argv.slice(2)
const opt = (name, dflt) => { const i = argv.indexOf(`--${name}`); return i >= 0 ? argv[i + 1] : dflt }
const flag = (name) => argv.includes(`--${name}`)
const provider = opt('provider', 'gemini')
const variants = opt('variants', 'baseline,new').split(',')
const runs = Number(opt('runs', '2'))
const only = opt('only', '')
const verbose = flag('verbose')
const pauseMs = Number(opt('pause', provider === 'openai' ? '10000' : '0'))
const settingsPath = path.join(process.env.APPDATA || '', 'AiDesktopCompanion', 'settings.json')
const settings = fs.existsSync(settingsPath) ? JSON.parse(fs.readFileSync(settingsPath, 'utf8')) : {}
const ar = settings.assistant_realtime || {}
const model = opt('model', provider === 'gemini' ? (String(ar.model || '').startsWith('gemini-') ? ar.model : 'gemini-3.8-live') : 'gpt-realtime-2.1')
const userInstructions = flag('no-user-instructions') ? '' : String(ar.instructions || '')

function key(names, settingName) {
  for (const n of names) if (process.env[n]) return process.env[n]
  return settings[settingName] || ''
}

// ---- tools as the app builds them (mcp.rs build_openai_tools_from_mcp) -----

const sanitize = (s) => String(s).replace(/[^a-zA-Z0-9_-]/g, '_').replace(/_+/g, '_')
function summarize(schema) {
  const props = schema?.properties
  if (!props) return ''
  const req = new Set(schema.required || [])
  // serde_json without preserve_order iterates keys sorted.
  return Object.keys(props).sort().map((k) => {
    const d = props[k] || {}
    let p = `${k}: ${d.type || 'any'}${req.has(k) ? ' (required)' : ' (optional)'}`
    if (d.description) p += ` - ${String(d.description).slice(0, 120)}`
    return p
  }).join('; ')
}
function toolDecl(server, t) {
  const params = { ...(t.inputSchema || {}) }
  params.type ??= 'object'; params.properties ??= {}; params.additionalProperties ??= true
  const sum = summarize(params)
  const desc = t.description ? `${t.description} (server: ${server})${sum ? ` Inputs: ${sum}` : ''}` : `MCP tool '${t.name}' from server '${server}'.${sum ? ` Inputs: ${sum}` : ''}`
  return { name: `mcp__${sanitize(server)}__${sanitize(t.name)}`, description: desc, parameters: params }
}
const fixtures = path.join(here, 'fixtures')
const wcTools = JSON.parse(fs.readFileSync(path.join(fixtures, 'windows-commander-tools.json'), 'utf8'))
const robTools = JSON.parse(fs.readFileSync(path.join(fixtures, 'rob-tools.json'), 'utf8'))
const profile = JSON.parse(fs.readFileSync(path.join(appDir, 'src-tauri', 'voice_tool_profile.json'), 'utf8'))

function toolsFor(variant) {
  if (variant === 'baseline') return [...wcTools.map((t) => toolDecl('windows-commander', t)), ...robTools.map((t) => toolDecl('Rob', t))]
  const wc = wcTools.filter((t) => profile.windows_commander_screen.includes(t.name))
  // As realtime_build_tools: no claude_result (background_tools.rs collects tickets).
  const rob = robTools.filter((t) => t.name !== 'claude_result').map((t) => toolDecl('Rob', t))
  return [...wc.map((t) => toolDecl('windows-commander', t)), ...rob]
}

/**
 * What background_tools.rs started_output() returns for a call that took
 * longer than BACKGROUND_AFTER. The eval treats ask_claude as that slow tool,
 * as it is in practice (16 s to 216 s measured).
 */
const ASK_STARTED = { status: 'started', job_id: 'bg-1', note: 'ask_claude takes longer and continues in the background. Tell the user in a few words that it is running, then carry on. Do not guess its result and do not call ask_claude again for this request: the result is added to the conversation as a [ask_claude] message when it is ready.' }

// ---- instructions -----------------------------------------------------------

// Frozen copy of screenPresenceNote() and the arrangement note before this
// change (git 2b47c08), so the baseline stays the baseline.
const OLD_NOTE = [
  'You can point at things on the user\'s screen: point_at draws an arrow with a short label, mark_area and highlight_element frame something. Prefer an element target (an element_ref from get_pointer_context or the screen context) or a text target over raw coordinates. Use it when showing beats describing, keep labels to a few words, and call clear_annotations when you are done or the user moves on.',
  'Messages that start with [Screen context] describe what is on the user\'s screen right now. They come from the system, not from the user. Use them to resolve "this", "here" or "that window", never read them out, and do not mention them unless it helps.',
].join('\n')
const OLD_ARRANGEMENT = 'There is no supervisor in this session, so do not mention one or wait for one. You have tools of your own: call them yourself when a question needs them. Some take up to a minute, so say you are checking before you call one, then give the answer when it arrives.'
const OLD_DEFAULT = 'You are an assistant in Assistant Mode. Speak clearly and concisely. IMPORTANT: Always reply in the same language the user is speaking. If you are unsure, reply in English.'

function instructionsFor(variant, tools) {
  if (variant === 'baseline') return `${userInstructions.trim() || OLD_DEFAULT}\n\n${OLD_NOTE}\n\n${OLD_ARRANGEMENT}`
  return buildVoiceInstructions(userInstructions, tools.map((t) => t.name), 'tools')
}

// ---- tool results as realtime_call_tool returns them ----------------------

function toolOutput(name, args, ctx, variant) {
  const tool = bare(name)
  const server = name.startsWith('mcp__Rob__') ? 'Rob' : 'windows-commander'
  if (variant !== 'baseline' && server === 'windows-commander' && !profile.windows_commander_screen.includes(tool)) {
    return { error: `${tool} is not available in voice sessions.` }
  }
  if (variant !== 'baseline' && tool === 'ask_claude') { ctx.asked = args; return ASK_STARTED }
  if (variant !== 'baseline' && tool === 'claude_result') return { error: 'Not needed in a voice call.' }
  const res = cannedResult(tool, args, { ...ctx, appLayer: variant !== 'baseline' })
  if (res && res.error) return { serverId: server, tool, result: { content: [{ type: 'text', text: res.error }], isError: true } }
  return { serverId: server, tool, result: { content: [{ type: 'text', text: JSON.stringify(res) }] } }
}

// ---- providers ------------------------------------------------------------

function timeout(ms, what) { return new Promise((_, rej) => setTimeout(() => rej(new Error(`timeout: ${what}`)), ms)) }

/** Gemini Live session. Returns { turn(text, ctxText) -> {calls, text}, close() }. */
async function geminiSession(instructions, tools, onTool) {
  const k = key(['GEMINI_API_KEY', 'GOOGLE_API_KEY'], 'gemini_api_key')
  if (!k) throw new Error('no Gemini key')
  const ws = new WebSocket(`wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContent?key=${encodeURIComponent(k)}`)
  ws.binaryType = 'arraybuffer'
  const listeners = new Set()
  ws.onmessage = (ev) => {
    const text = typeof ev.data === 'string' ? ev.data : Buffer.from(ev.data).toString('utf8')
    let m; try { m = JSON.parse(text) } catch { return }
    for (const l of listeners) l(m)
  }
  let closed = null
  ws.onclose = (e) => { closed = `${e.code} ${e.reason}`; for (const l of listeners) l({ __closed: closed }) }
  await Promise.race([new Promise((r, j) => { ws.onopen = r; ws.onerror = () => j(new Error('socket error')) }), timeout(15000, 'open')])
  const setup = {
    model: `models/${model}`,
    generationConfig: { responseModalities: ['AUDIO'], speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: ar.voice || 'Kore' } } } },
    systemInstruction: { parts: [{ text: instructions }] },
    outputAudioTranscription: {},
    tools: [{ functionDeclarations: tools.map((t) => ({ name: t.name, description: t.description, parametersJsonSchema: t.parameters })) }],
  }
  const ready = new Promise((r, j) => { const l = (m) => { if (m.setupComplete) { listeners.delete(l); r() } if (m.__closed) j(new Error(`closed during setup: ${m.__closed}`)) }; listeners.add(l) })
  ws.send(JSON.stringify({ setup }))
  await Promise.race([ready, timeout(20000, 'setup')])

  async function turn(userText, ctxText, injected = null) {
    if (injected) userText = [injected.text, injected.speak].join(String.fromCharCode(10, 10))
    if (ctxText) ws.send(JSON.stringify({ clientContent: { turns: [{ role: 'user', parts: [{ text: ctxText }] }], turnComplete: false } }))
    const calls = []
    let text = ''
    let rounds = 0
    const done = new Promise((resolve, reject) => {
      let quiet = null
      const finish = () => { listeners.delete(l); resolve() }
      const l = async (m) => {
        if (m.__closed) { listeners.delete(l); reject(new Error(`closed: ${m.__closed}`)); return }
        if (m.toolCall) {
          if (quiet) { clearTimeout(quiet); quiet = null }
          rounds += 1
          const responses = []
          for (const c of m.toolCall.functionCalls || []) {
            calls.push({ name: c.name, args: c.args || {} })
            const out = rounds > 6 ? { error: 'tool call limit reached' } : onTool(c.name, c.args || {})
            responses.push({ id: c.id, name: c.name, response: out })
          }
          ws.send(JSON.stringify({ toolResponse: { functionResponses: responses } }))
        }
        const sc = m.serverContent
        if (sc?.outputTranscription?.text) text += sc.outputTranscription.text
        if (sc?.turnComplete) {
          // A tool call can follow a turn that only said "let me check".
          if (quiet) clearTimeout(quiet)
          quiet = setTimeout(finish, 1500)
        }
      }
      listeners.add(l)
    })
    ws.send(JSON.stringify({ clientContent: { turns: [{ role: 'user', parts: [{ text: userText }] }], turnComplete: true } }))
    await Promise.race([done, timeout(60000, 'turn')])
    return { calls, text: text.replace(/\s+/g, ' ').trim() }
  }
  return { turn, close: () => { try { ws.close() } catch {} } }
}

/** OpenAI Realtime session over WebSocket, text output. */
async function openaiSession(instructions, tools, onTool) {
  const k = key(['OPENAI_API_KEY'], 'openai_api_key')
  if (!k) throw new Error('no OpenAI key')
  const ws = new WebSocket(`wss://api.openai.com/v1/realtime?model=${encodeURIComponent(model)}`, ['realtime', `openai-insecure-api-key.${k}`])
  const listeners = new Set()
  ws.onmessage = (ev) => { let m; try { m = JSON.parse(String(ev.data)) } catch { return } for (const l of listeners) l(m) }
  ws.onclose = (e) => { for (const l of listeners) l({ type: '__closed', why: `${e.code} ${e.reason}` }) }
  await Promise.race([new Promise((r, j) => { ws.onopen = r; ws.onerror = () => j(new Error('socket error')) }), timeout(15000, 'open')])
  const send = (o) => ws.send(JSON.stringify(o))
  const updated = new Promise((r, j) => { const l = (m) => { if (m.type === 'session.updated') { listeners.delete(l); r() } if (m.type === 'error') { listeners.delete(l); j(new Error(JSON.stringify(m.error))) } }; listeners.add(l) })
  send({ type: 'session.update', session: { type: 'realtime', output_modalities: ['text'], instructions, tools: tools.map((t) => ({ type: 'function', ...t })), tool_choice: 'auto' } })
  await Promise.race([updated, timeout(20000, 'session.update')])

  async function turn(userText, ctxText, injected = null) {
    if (ctxText) send({ type: 'conversation.item.create', item: { type: 'message', role: 'system', content: [{ type: 'input_text', text: ctxText }] } })
    if (injected) send({ type: 'conversation.item.create', item: { type: 'message', role: 'system', content: [{ type: 'input_text', text: injected.text }] } })
    else send({ type: 'conversation.item.create', item: { type: 'message', role: 'user', content: [{ type: 'input_text', text: userText }] } })
    const calls = []
    let text = ''
    let rounds = 0
    const done = new Promise((resolve, reject) => {
      const l = (m) => {
        if (m.type === '__closed') { listeners.delete(l); reject(new Error(`closed: ${m.why}`)); return }
        if (m.type === 'error') { listeners.delete(l); reject(new Error(JSON.stringify(m.error))); return }
        if (process.env.EVAL_TRACE) console.log('      ev', m.type, m.type === 'response.done' ? JSON.stringify(m.response?.status_details || m.response?.status) : '')
        if (m.type === 'response.done') {
          // The finished items are authoritative; deltas were seen missing.
          for (const o of m.response?.output || []) {
            if (o.type !== 'message') continue
            for (const c of o.content || []) if (c.type === 'output_text' || c.type === 'text') text += (c.text || '') + ' '
          }
          if (m.response?.status && m.response.status !== 'completed') text += ` [response ${m.response.status}: ${JSON.stringify(m.response.status_details || {}).slice(0, 160)}]`
          const fcs = (m.response?.output || []).filter((o) => o.type === 'function_call')
          if (!fcs.length || rounds >= 6) { listeners.delete(l); resolve(); return }
          rounds += 1
          for (const fc of fcs) {
            let args = {}; try { args = JSON.parse(fc.arguments || '{}') } catch {}
            calls.push({ name: fc.name, args })
            send({ type: 'conversation.item.create', item: { type: 'function_call_output', call_id: fc.call_id, output: JSON.stringify(onTool(fc.name, args)) } })
          }
          text += ' '
          send({ type: 'response.create' })
        }
      }
      listeners.add(l)
    })
    send(injected ? { type: 'response.create', response: { instructions: injected.speak } } : { type: 'response.create' })
    await Promise.race([done, timeout(60000, 'turn')])
    return { calls, text: text.replace(/\s+/g, ' ').trim() }
  }
  return { turn, close: () => { try { ws.close() } catch {} } }
}

// ---- run ------------------------------------------------------------------

async function runScenario(sc, variant) {
  const tools = toolsFor(variant)
  const instructions = instructionsFor(variant, tools)
  const ctx = { world: sc.world, turn: 0 }
  const open = provider === 'openai' ? openaiSession : geminiSession
  const session = await open(instructions, tools, (name, args) => toolOutput(name, args, ctx, variant))
  const turns = []
  try {
    for (let i = 0; i < sc.turns.length; i++) {
      ctx.turn = i
      const step = sc.turns[i]
      // A string is the user speaking; { inject, speak } is a message the app
      // adds (a background answer arriving), with no new screen context.
      const injected = typeof step === 'object' ? { text: step.inject, speak: step.speak } : null
      const ctxText = injected ? null : variant === 'baseline' ? contextItem(sc.world, i, 'old') : variant === 'new' ? contextItem(sc.world, i, 'new') : null
      const t = await session.turn(injected ? '' : step, ctxText, injected)
      // Curly apostrophes ("can’t") would slip past the checks' patterns.
      t.text = t.text.replace(/[‘’]/g, "'")
      turns.push(t)
      if (verbose) console.log(`    [${sc.id}/${variant}] T${i + 1} user: ${typeof sc.turns[i] === 'object' ? '[app] ' + sc.turns[i].inject.split(String.fromCharCode(10))[0] : sc.turns[i]}\n      calls: ${t.calls.map((c) => `${bare(c.name)}(${JSON.stringify(c.args).slice(0, 80)})`).join(', ') || '-'}\n      said: ${t.text}`)
    }
  } finally { session.close() }
  const results = sc.checks.map((c) => { const r = c.fn(turns); return { name: c.name, ok: r === true || r?.ok === true, why: r?.why } })
  turns.forEach((t, i) => { const h = honesty(t, turns.slice(0, i).reduce((n, x) => n + x.calls.length, 0)); results.push({ name: `turn ${i + 1} honest`, ok: h.ok, why: h.why }) })
  return { turns, results, ok: results.every((r) => r.ok), toolCount: tools.length, instrChars: instructions.length }
}

const scenarios = SCENARIOS.filter((s) => !only || only.split(',').includes(s.id))
// Scenarios that test behaviour only the new app layer has (background Claude
// Code) are skipped for the baseline.
const runsFor = (sc, variant) => (sc.newOnly && variant === 'baseline' ? 0 : runs)
const all = []
console.log(`provider ${provider}, model ${model}, variants ${variants.join('/')}, runs ${runs}`)
for (const variant of variants) {
  const tools = toolsFor(variant)
  console.log(`\n== ${variant}: ${tools.length} tools, instructions ${instructionsFor(variant, tools).length} chars`)
  for (const sc of scenarios) {
    for (let r = 0; r < runsFor(sc, variant); r++) {
      let res
      // OpenAI orgs on a low tier hit their tokens-per-minute limit after a
      // few sessions; a rate-limited run measures nothing, so it is retried.
      for (let attempt = 0; ; attempt++) {
        try { res = await runScenario(sc, variant) } catch (e) { res = { error: e.message, ok: false, results: [] } }
        const limited = /rate_limit/i.test(JSON.stringify(res.turns || []) + (res.error || ''))
        if (!limited) break
        if (attempt >= 3) { res.error = 'rate limited, gave up'; break }
        console.log(`  (rate limited, waiting 60 s before retrying ${sc.id})`)
        await new Promise((ok) => setTimeout(ok, 60000))
      }
      if (pauseMs) await new Promise((ok) => setTimeout(ok, pauseMs))
      all.push({ variant, scenario: sc.id, run: r + 1, ...res })
      const failed = (res.results || []).filter((x) => !x.ok)
      console.log(`  ${res.ok ? 'PASS' : 'FAIL'} ${sc.id} #${r + 1}${res.error ? ` ERROR ${res.error}` : ''}${failed.length ? ' - ' + failed.map((f) => `${f.name}: ${f.why}`).join(' | ') : ''}`)
    }
  }
}

// Summary table: scenario x variant, passed runs / runs and checks passed.
const table = ['| scenario | ' + variants.join(' | ') + ' |', '|---|' + variants.map(() => '---').join('|') + '|']
for (const sc of scenarios) {
  const cells = variants.map((v) => {
    const rs = all.filter((a) => a.variant === v && a.scenario === sc.id)
    const checks = rs.flatMap((a) => a.results || [])
    return `${rs.filter((a) => a.ok).length}/${rs.length} (checks ${checks.filter((c) => c.ok).length}/${checks.length})`
  })
  table.push(`| ${sc.id} | ${cells.join(' | ')} |`)
}
const totals = variants.map((v) => { const rs = all.filter((a) => a.variant === v); return `${rs.filter((a) => a.ok).length}/${rs.length}` })
table.push(`| **all** | ${totals.join(' | ')} |`)
console.log('\n' + table.join('\n'))
const out = opt('out', path.join(os.tmpdir(), `voice-eval-${provider}-${Date.now()}.json`))
fs.writeFileSync(out, JSON.stringify({ provider, model, variants, runs, at: new Date().toISOString(), results: all }, null, 1))
console.log(`\nfull results: ${out}`)
