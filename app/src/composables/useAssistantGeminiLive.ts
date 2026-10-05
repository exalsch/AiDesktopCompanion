import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useRealtimeUsage } from './useRealtimeUsage'
import { buildAudioConstraints, isVoiceIsolationSupported } from '../audioConstraints'
import {
  SUPERVISOR_TOOL, SUPERVISOR_TOOL_NAME, askSupervisorChat,
  type AssistantRealtimeOptions, type ConnectParams, type HistoryTurn,
} from './useAssistantRealtime'

// Served from public/ as a real file. Imported through Vite it was inlined as a
// data: URL, which the CSP's script-src 'self' refuses to load as a worklet.
const captureWorkletUrl = '/pcm-capture.worklet.js'

/**
 * Assistant Mode over the Gemini Live API.
 *
 * Same contract as `useAssistantRealtime` (the panel does not know which one it
 * is driving) but a different transport. OpenAI Realtime is WebRTC: the browser
 * moves the audio and a data channel carries events. Gemini Live is a single
 * WebSocket carrying JSON, with audio as base64 PCM inside it - 16 kHz going
 * up, 24 kHz coming back - so capture and playback are done here with Web
 * Audio.
 *
 * Things that differ from OpenAI and shape the code below:
 * - The session config (`setup`) is fixed for the life of a socket. Changing a
 *   setting mid-call reconnects with a session-resumption handle, which keeps
 *   the conversation; Google also closes long sockets with `goAway`, handled
 *   the same way.
 * - There is no "hold the model silent" switch, so the supervisor's "always"
 *   mode is the escalation tool plus an instruction to use it every turn.
 * - Push-to-talk turns automatic activity detection off and marks turns with
 *   `activityStart` / `activityEnd`.
 * - Transcripts arrive in fragments and are assembled per turn.
 * - The token is minted in Rust (`gemini_live_create_token`), so the API key
 *   never reaches the WebView.
 */

const LIVE_URL = 'wss://generativelanguage.googleapis.com/ws/google.ai.generativelanguage.v1beta.GenerativeService.BidiGenerateContentConstrained'
const INPUT_RATE = 16000
const OUTPUT_RATE = 24000
const MAX_TOOL_ROUNDS = 6
const MAX_TALK_MS = 60000
const MIN_TALK_MS = 200

export const DEFAULT_GEMINI_LIVE_MODEL = 'gemini-3.8-live'
export const DEFAULT_GEMINI_LIVE_VOICE = 'Kore'

export function isGeminiLiveModel(model?: string): boolean {
  return String(model || '').trim().toLowerCase().startsWith('gemini-')
}

function bytesToBase64(bytes: Uint8Array): string {
  let s = ''
  const step = 0x8000
  for (let i = 0; i < bytes.length; i += step) s += String.fromCharCode(...bytes.subarray(i, i + step))
  return btoa(s)
}

function base64ToBytes(b64: string): Uint8Array {
  const s = atob(b64)
  const out = new Uint8Array(s.length)
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i)
  return out
}

export function useAssistantGeminiLive(opts: Omit<AssistantRealtimeOptions, 'getEphemeralToken'> & { getToken: () => Promise<string> }) {
  const statusRef = ref<{ toolsCount: number, supervisor: boolean, voice?: string, silenceMs?: number, idleMs?: number }>({ toolsCount: 0, supervisor: false })
  const micEnabled = ref(true)
  const history = ref<HistoryTurn[]>([])
  const usage = useRealtimeUsage()

  let ws: WebSocket | null = null
  let params: ConnectParams = {}
  // The setup the current socket was opened with, as JSON, so updateSession
  // can tell a change that needs a reconnect from one that does not.
  let activeSetupKey = ''
  let resumeHandle: string | null = null
  // Set while a socket is being replaced, so its close is not a hang-up.
  let replacing = false
  let connected = false
  let setupDone = false

  let micStream: MediaStream | null = null
  let captureCtx: AudioContext | null = null
  let captureNode: AudioWorkletNode | null = null
  let playCtx: AudioContext | null = null
  let playHead = 0
  const playing = new Set<AudioBufferSourceNode>()

  let micMode: 'open' | 'ptt' = 'open'
  let talkStartedAt = 0
  let talkTimeout: any = 0
  let mediaHeld = false
  let autoCloseMs = 0
  let autoCloseTimer: any = 0
  let toolRounds = 0
  let toolCalledThisTurn = false
  // Turn assembly. Gemini streams both transcripts in fragments.
  let userBuf = ''
  let assistantBuf = ''
  let interrupted = false
  let supervisorEpoch = 0
  const cancelledCalls = new Set<string>()

  function log(msg: string) { try { opts.onLog?.(msg) } catch {} }

  function resetAutoClose() {
    if (autoCloseTimer) { clearTimeout(autoCloseTimer); autoCloseTimer = 0 }
    if (!autoCloseMs || autoCloseMs <= 0) return
    autoCloseTimer = setTimeout(() => {
      log(`[session] no activity for ${Math.round(autoCloseMs / 1000)}s, closing`)
      void disconnect()
    }, autoCloseMs)
  }

  function send(payload: any): boolean {
    if (!ws || ws.readyState !== WebSocket.OPEN) return false
    try { ws.send(JSON.stringify(payload)); return true } catch (e: any) {
      log('[error] send failed: ' + (e?.message || e))
      return false
    }
  }

  // ---- playback -----------------------------------------------------------

  function playPcm(b64: string) {
    if (!playCtx) return
    const bytes = base64ToBytes(b64)
    const samples = new Int16Array(bytes.buffer, bytes.byteOffset, Math.floor(bytes.byteLength / 2))
    if (!samples.length) return
    const buf = playCtx.createBuffer(1, samples.length, OUTPUT_RATE)
    const ch = buf.getChannelData(0)
    for (let i = 0; i < samples.length; i++) ch[i] = samples[i] / 0x8000
    const src = playCtx.createBufferSource()
    src.buffer = buf
    src.connect(playCtx.destination)
    // Chunks are scheduled back to back on the context clock; starting each at
    // currentTime instead would leave a gap or an overlap at every boundary.
    playHead = Math.max(playHead, playCtx.currentTime + 0.02)
    src.start(playHead)
    playHead += buf.duration
    playing.add(src)
    src.onended = () => { playing.delete(src) }
  }

  /** Stop everything queued for the speaker. Used on barge-in. */
  function stopPlayback() {
    for (const s of playing) { try { s.stop() } catch {} }
    playing.clear()
    playHead = 0
  }

  // ---- turn assembly --------------------------------------------------------

  function flushUser() {
    const text = userBuf.trim()
    userBuf = ''
    if (!text) return
    history.value.push({ role: 'user', content: text })
    log(`[user] ${text}`)
    resetAutoClose()
  }

  function flushAssistant() {
    const raw = assistantBuf.trim()
    assistantBuf = ''
    if (!raw) return
    const text = interrupted ? `${raw} (interrupted)` : raw
    history.value.push({ role: 'assistant', content: text })
    log(`[assistant] ${text}`)
    resetAutoClose()
  }

  /** Barge-in: silence the speaker and drop answers to the abandoned turn. */
  function interruptResponse() {
    if (playing.size) log('[response] interrupted by the user')
    stopPlayback()
    supervisorEpoch += 1
  }

  // ---- tools ------------------------------------------------------------

  async function handleToolCall(calls: any[]) {
    flushUser()
    toolCalledThisTurn = true
    const responses: any[] = []
    const overLimit = toolRounds >= MAX_TOOL_ROUNDS
    if (overLimit) log(`[tools] round limit (${MAX_TOOL_ROUNDS}) reached, refusing further calls`)
    toolRounds += 1
    const epoch = supervisorEpoch
    for (const c of calls) {
      const id = String(c?.id || '')
      const name = String(c?.name || '')
      const args = c?.args && typeof c.args === 'object' ? c.args : {}
      const argsJson = JSON.stringify(args)
      log(`[tools] -> ${name} ${argsJson.slice(0, 200)}`)
      let output: string
      if (overLimit) {
        output = JSON.stringify({ error: 'tool call limit reached for this conversation' })
      } else if (name === SUPERVISOR_TOOL_NAME) {
        try {
          const answer = await askSupervisorChat(history.value, String(args?.question || '').trim(), log)
          output = JSON.stringify({ answer: answer || 'No answer available.' })
        } catch (e: any) {
          output = JSON.stringify({ error: e?.message || String(e) })
        }
      } else {
        try {
          output = await invoke<string>('realtime_call_tool', { name, argsJson })
        } catch (e: any) {
          output = JSON.stringify({ error: e?.message || String(e) })
        }
      }
      log(`[tools] <- ${name} ${output.slice(0, 200)}`)
      history.value.push({ role: 'tool', content: `${name} ${output.slice(0, 300)}` })
      if (cancelledCalls.has(id)) { cancelledCalls.delete(id); log(`[tools] ${name} was cancelled by the server, result dropped`); continue }
      // `response` must be an object; a tool's JSON object is passed as is,
      // anything else is wrapped.
      let response: any
      try { const parsed = JSON.parse(output); response = parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : { output: parsed } } catch { response = { output } }
      responses.push({ id, name, response })
    }
    if (epoch !== supervisorEpoch) { log('[tools] results discarded, the user interrupted that turn'); return }
    if (responses.length) send({ toolResponse: { functionResponses: responses } })
  }

  // ---- session config -------------------------------------------------------

  async function buildTools(): Promise<{ declarations: any[], note: string, count: number }> {
    const supervisorOn = params.useSupervisor === true
    const supervisorMode = params.supervisorMode === 'needed' ? 'needed' : 'always'
    const includeTools = params.enableTools === true && !supervisorOn
    let flat: any[] = []
    if (supervisorOn) {
      flat = [SUPERVISOR_TOOL]
    } else if (includeTools) {
      try {
        const res = await invoke<any>('realtime_build_tools')
        flat = Array.isArray(res) ? res : []
      } catch (e: any) {
        log('[warn] realtime_build_tools failed: ' + (e?.message || e))
      }
    }
    const note = supervisorOn
      ? (supervisorMode === 'always'
        ? `For every request the user makes, call ${SUPERVISOR_TOOL_NAME} with it and read its answer back word for word. Do not answer on your own, and do not describe this arrangement to the user.`
        : `Call ${SUPERVISOR_TOOL_NAME} whenever a question needs current information, the user's files or applications, or careful reasoning, and read its answer back. It is the only tool you have; there are no others in this session.`)
      : includeTools
        ? 'There is no supervisor in this session, so do not mention one or wait for one. You have tools of your own: call them yourself when a question needs them. Some take up to a minute, so say you are checking before you call one, then give the answer when it arrives.'
        : 'There is no supervisor and no tools in this session. Answer from your own knowledge, and say plainly when something is beyond it.'
    // Gemini takes the JSON Schema as is through `parametersJsonSchema`, so the
    // MCP tools' schemas need no conversion.
    const declarations = flat.map((t: any) => ({
      name: String(t?.name || t?.function?.name || ''),
      description: String(t?.description || t?.function?.description || ''),
      parametersJsonSchema: t?.parameters || t?.function?.parameters || { type: 'object', properties: {} },
    })).filter((d) => d.name)
    return { declarations, note, count: declarations.length }
  }

  async function buildSetup() {
    const { declarations, note, count } = await buildTools()
    const instructions = (params.instructions && params.instructions.trim())
      ? params.instructions.trim()
      : 'You are an assistant in Assistant Mode. Speak clearly and concisely. IMPORTANT: Always reply in the same language the user is speaking. If you are unsure, reply in English.'
    const ptt = params.micMode === 'ptt'
    const activity: Record<string, any> = ptt
      ? { disabled: true }
      : { silenceDurationMs: typeof params.silenceDurationMs === 'number' ? params.silenceDurationMs : 2000 }
    const setup: Record<string, any> = {
      model: `models/${params.model || DEFAULT_GEMINI_LIVE_MODEL}`,
      generationConfig: {
        responseModalities: ['AUDIO'],
        speechConfig: { voiceConfig: { prebuiltVoiceConfig: { voiceName: params.voice || DEFAULT_GEMINI_LIVE_VOICE } } },
      },
      systemInstruction: { parts: [{ text: `${instructions}\n\n${note}` }] },
      realtimeInputConfig: { automaticActivityDetection: activity },
      inputAudioTranscription: {},
      outputAudioTranscription: {},
      // Without compression an audio session is cut off after about fifteen
      // minutes of context.
      contextWindowCompression: { slidingWindow: {} },
      sessionResumption: {},
    }
    if (declarations.length) setup.tools = [{ functionDeclarations: declarations }]
    // Everything above, minus the resumption handle, is what a mid-call change
    // has to compare against.
    const key = JSON.stringify(setup)
    statusRef.value = {
      toolsCount: count,
      supervisor: params.useSupervisor === true,
      voice: params.voice,
      silenceMs: ptt ? undefined : activity.silenceDurationMs,
    }
    log('[setup] ' + JSON.stringify({
      model: setup.model, voice: params.voice, turn_detection: ptt ? 'manual (push-to-talk)' : 'automatic',
      silence_ms: ptt ? null : activity.silenceDurationMs, tool_count: count,
      tool_names_sample: declarations.slice(0, 8).map((d) => d.name),
    }))
    return { setup, key }
  }

  // ---- socket ---------------------------------------------------------------

  /** Open a socket and send `setup`. Resolves once the server confirms it. */
  async function openSocket(): Promise<void> {
    const { setup, key } = await buildSetup()
    if (resumeHandle) setup.sessionResumption = { handle: resumeHandle }
    const token = await opts.getToken()
    const sock = new WebSocket(`${LIVE_URL}?access_token=${encodeURIComponent(token)}`)
    ws = sock
    setupDone = false
    await new Promise<void>((resolve, reject) => {
      let settled = false
      sock.onopen = () => { log('socket open'); sock.send(JSON.stringify({ setup })) }
      sock.onmessage = async (ev) => {
        let text: string
        try { text = typeof ev.data === 'string' ? ev.data : await (ev.data as Blob).text() } catch { return }
        let m: any
        try { m = JSON.parse(text) } catch { return }
        if (m.setupComplete) {
          setupDone = true
          activeSetupKey = key
          if (!settled) { settled = true; resolve() }
          return
        }
        try { handleServerMessage(m) } catch (err: any) { log('[error] message handler threw: ' + (err?.message || err)) }
      }
      sock.onerror = () => { log('socket error') }
      sock.onclose = (e) => {
        log(`socket closed (${e.code}${e.reason ? ': ' + e.reason : ''})`)
        if (!settled) { settled = true; reject(new Error(e.reason || `Gemini Live closed the connection (${e.code})`)); return }
        if (ws !== sock || replacing) return
        // A close nobody asked for. The server sends goAway first when it
        // plans one, so this is a real drop: report it and hang up.
        if (connected) {
          if (e.code !== 1000) try { opts.onWarn?.(e.reason || `Gemini Live connection lost (${e.code})`) } catch {}
          void disconnect()
        }
      }
    })
  }

  /** Swap the socket for a new one, keeping the conversation if possible. */
  async function reopen(reason: string) {
    if (!connected) return
    if (!resumeHandle) log(`[session] ${reason}: no resumption handle yet, the new session starts without history`)
    else log(`[session] ${reason}: resuming on a new connection`)
    replacing = true
    const old = ws
    try {
      stopPlayback()
      flushUser(); flushAssistant()
      try { old?.close(1000, 'reconnecting') } catch {}
      await openSocket()
    } catch (e: any) {
      const msg = e?.message || String(e)
      log('[error] reconnect failed: ' + msg)
      replacing = false
      try { opts.onError?.(msg) } catch {}
      await disconnect()
      return
    }
    replacing = false
  }

  function handleServerMessage(m: any) {
    if (m.usageMetadata) {
      if (!usage.addGeminiUsage(m.usageMetadata)) log('[usage] usageMetadata carried no recognisable counts')
    }
    if (m.sessionResumptionUpdate) {
      const u = m.sessionResumptionUpdate
      if (u.resumable && u.newHandle) resumeHandle = String(u.newHandle)
      return
    }
    if (m.goAway) {
      log('[session] server will close this connection soon (' + (m.goAway.timeLeft || '?') + ')')
      void reopen('server goAway')
      return
    }
    if (m.toolCall) {
      const calls = Array.isArray(m.toolCall.functionCalls) ? m.toolCall.functionCalls : []
      handleToolCall(calls).catch((e) => log('[tools] batch failed: ' + (e?.message || e)))
      return
    }
    if (m.toolCallCancellation) {
      for (const id of (m.toolCallCancellation.ids || [])) cancelledCalls.add(String(id))
      log('[tools] server cancelled ' + JSON.stringify(m.toolCallCancellation.ids || []))
      return
    }
    const sc = m.serverContent
    if (!sc) return
    if (sc.inputTranscription?.text) userBuf += sc.inputTranscription.text
    const parts = sc.modelTurn?.parts
    if (Array.isArray(parts)) {
      // The model answering means the user's turn is over.
      flushUser()
      for (const p of parts) {
        const data = p?.inlineData
        if (data?.data && String(data.mimeType || '').startsWith('audio/')) playPcm(data.data)
      }
    }
    if (sc.outputTranscription?.text) { flushUser(); assistantBuf += sc.outputTranscription.text }
    if (sc.interrupted) {
      // The server heard the user over the assistant (open microphone).
      interrupted = true
      stopPlayback()
      supervisorEpoch += 1
      log('[response] interrupted by the user')
    }
    if (sc.turnComplete) {
      flushUser()
      flushAssistant()
      interrupted = false
      if (!toolCalledThisTurn) toolRounds = 0
      toolCalledThisTurn = false
    }
  }

  // ---- microphone ----------------------------------------------------------

  async function startCapture() {
    const audioSettings = await invoke<any>('get_settings').catch(() => null)
    micStream = await navigator.mediaDevices.getUserMedia({
      audio: buildAudioConstraints(audioSettings, '', isVoiceIsolationSupported()),
    })
    // A context at the server's rate makes the browser do the resampling.
    captureCtx = new AudioContext({ sampleRate: INPUT_RATE })
    await captureCtx.audioWorklet.addModule(captureWorkletUrl)
    const source = captureCtx.createMediaStreamSource(micStream)
    captureNode = new AudioWorkletNode(captureCtx, 'pcm-capture')
    captureNode.port.onmessage = (ev) => {
      if (!setupDone || !micEnabled.value) return
      send({ realtimeInput: { audio: { data: bytesToBase64(new Uint8Array(ev.data as ArrayBuffer)), mimeType: `audio/pcm;rate=${INPUT_RATE}` } } })
    }
    source.connect(captureNode)
    playCtx = new AudioContext({ sampleRate: OUTPUT_RATE })
    playHead = 0
  }

  function stopCapture() {
    try { captureNode?.port.close() } catch {}
    try { captureNode?.disconnect() } catch {}
    captureNode = null
    try { micStream?.getTracks().forEach((t) => t.stop()) } catch {}
    micStream = null
    try { void captureCtx?.close() } catch {}
    captureCtx = null
    stopPlayback()
    try { void playCtx?.close() } catch {}
    playCtx = null
  }

  /**
   * Mute or unmute. Nothing is sent while muted, so a muted microphone costs
   * nothing; the track stays open so unmuting is instant (same trade-off as
   * the OpenAI client).
   */
  function setMicEnabled(enabled: boolean) {
    const was = micEnabled.value
    micEnabled.value = enabled
    try { micStream?.getAudioTracks().forEach((t) => { t.enabled = enabled }) } catch {}
    // With automatic activity detection, closing the stream tells the server
    // the user is done rather than leaving it waiting for silence.
    if (was && !enabled && micMode === 'open' && setupDone) send({ realtimeInput: { audioStreamEnd: true } })
    log(enabled ? '[mic] unmuted' : '[mic] muted, no longer transmitting')
  }

  function startTalking() {
    if (!connected || micMode !== 'ptt') return
    interruptResponse()
    talkStartedAt = Date.now()
    send({ realtimeInput: { activityStart: {} } })
    invoke<boolean>('media_hold', { reason: 'assistant' }).then((held) => { mediaHeld = held === true }).catch(() => {})
    if (talkTimeout) { clearTimeout(talkTimeout); talkTimeout = 0 }
    talkTimeout = setTimeout(() => {
      log(`[mic] hold exceeded ${Math.round(MAX_TALK_MS / 1000)}s, closing the microphone`)
      stopTalking()
    }, MAX_TALK_MS)
    if (!micEnabled.value) setMicEnabled(true)
  }

  function stopTalking() {
    if (talkTimeout) { clearTimeout(talkTimeout); talkTimeout = 0 }
    if (mediaHeld) { mediaHeld = false; void invoke('media_release').catch(() => {}) }
    if (micMode !== 'ptt') return
    if (micEnabled.value) setMicEnabled(false)
    if (!talkStartedAt) return
    const heldMs = Date.now() - talkStartedAt
    talkStartedAt = 0
    // activityEnd is sent even for a stray tap: an activityStart left open
    // would hold the next turn hostage.
    send({ realtimeInput: { activityEnd: {} } })
    log(heldMs < MIN_TALK_MS ? `[mic] hold of ${heldMs}ms is very short` : `[mic] turn ended after ${heldMs}ms`)
  }

  // ---- lifecycle ------------------------------------------------------------

  async function connect(p: ConnectParams = {}) {
    params = { ...p }
    history.value = []
    usage.reset(params.model)
    resumeHandle = null
    toolRounds = 0
    toolCalledThisTurn = false
    userBuf = ''; assistantBuf = ''; interrupted = false
    cancelledCalls.clear()
    connected = false
    micMode = params.micMode === 'ptt' ? 'ptt' : 'open'
    micEnabled.value = micMode === 'open'
    try {
      await startCapture()
      try { micStream?.getAudioTracks().forEach((t) => { t.enabled = micEnabled.value }) } catch {}
      await openSocket()
      connected = true
      autoCloseMs = typeof params.autoCloseMs === 'number' ? params.autoCloseMs : 0
      resetAutoClose()
      try { opts.onConnected?.() } catch {}
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : (err?.message || 'connect failed')
      try { opts.onError?.(msg) } catch {}
      await disconnect()
    }
  }

  async function disconnect() {
    connected = false
    replacing = false
    const sock = ws
    ws = null
    setupDone = false
    try { sock?.close(1000, 'hang up') } catch {}
    stopCapture()
    flushUser(); flushAssistant()
    if (autoCloseTimer) { clearTimeout(autoCloseTimer); autoCloseTimer = 0 }
    if (talkTimeout) { clearTimeout(talkTimeout); talkTimeout = 0 }
    if (mediaHeld) { mediaHeld = false; void invoke('media_release').catch(() => {}) }
    supervisorEpoch += 1
    resumeHandle = null
    toolRounds = 0
    try { opts.onDisconnected?.() } catch {}
  }

  /**
   * Apply changed settings to a live call. Local ones (auto-close, mute
   * behaviour) apply at once; anything in `setup` needs a new socket, which
   * resumes the session so the conversation carries over.
   */
  async function updateSession(p: ConnectParams) {
    params = { ...p }
    autoCloseMs = typeof params.autoCloseMs === 'number' ? params.autoCloseMs : 0
    resetAutoClose()
    const nextMode = params.micMode === 'ptt' ? 'ptt' : 'open'
    if (nextMode !== micMode) {
      micMode = nextMode
      setMicEnabled(micMode === 'open')
    }
    if (!connected) return
    const { key } = await buildSetup()
    if (key !== activeSetupKey) await reopen('settings changed')
  }

  // Gemini audio is played through Web Audio, not an <audio> element.
  function attachAudioElement(_el: HTMLAudioElement) {}

  return { connect, disconnect, attachAudioElement, updateSession, setMicEnabled, startTalking, stopTalking, micEnabled, status: statusRef, transcript: history, usage }
}
