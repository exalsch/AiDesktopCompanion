import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// Where the SDP offer is exchanged for an answer. The beta endpoint this used
// to post to, `POST /v1/realtime?model=...`, was removed with the rest of the
// realtime beta; the GA endpoint takes no query parameters because the model is
// already fixed by the ephemeral token.
const REALTIME_CALLS_URL = 'https://api.openai.com/v1/realtime/calls'

// Model used for `audio.input.transcription`, i.e. the text of what the user
// said. It drives the supervisor, the conversation history and the debug log.
//
// `whisper-1` was the only option when this was written and is now the legacy
// model, noticeably weaker on names and technical terms - which matters because
// the supervisor routes tool calls off this text. `gpt-live-transcribe` is the
// newer purpose-built streaming model and is also accepted here if you want to
// try it.
const DEFAULT_TRANSCRIPTION_MODEL = 'gpt-4o-transcribe'

// How many rounds of "model calls a tool, we answer, model calls another" to
// allow before giving up. Mirrors the cap in chat.rs so a confused model cannot
// spin forever on a live microphone.
const MAX_TOOL_ROUNDS = 6

// The API's hard ceiling for turn_detection.idle_timeout_ms. Anything larger is
// rejected, and a rejected session.update discards every other setting with it.
const MAX_IDLE_TIMEOUT_MS = 30000

// Escalation to the supervisor, expressed as something the model can choose to
// do. This replaces a keyword list that only recognised English - the model
// knows when a question is beyond it regardless of the language it is asked in.
const SUPERVISOR_TOOL_NAME = 'consult_supervisor'
const SUPERVISOR_TOOL = {
  type: 'function',
  name: SUPERVISOR_TOOL_NAME,
  description:
    'Ask the supervisor model, which is slower but far more capable and has access to tools, '
    + 'files and the internet. Call this for anything you cannot answer confidently from your own '
    + 'knowledge: current events, the user\'s files or applications, calculations, look-ups, or '
    + 'multi-step reasoning. Pass the question in the language the user asked it. '
    + 'Read the answer back to the user.',
  parameters: {
    type: 'object',
    properties: {
      question: {
        type: 'string',
        description: 'The full question, with any context needed to answer it standalone.',
      },
    },
    required: ['question'],
    additionalProperties: false,
  },
}

// Reasoning effort is only accepted by the reasoning-capable realtime models.
// Sending it to `gpt-realtime` or `gpt-realtime-1.5` fails the whole session
// with "Unsupported option for this model".
function modelSupportsReasoning(model?: string): boolean {
  return /^gpt-realtime-2(\.|$|-)/.test(String(model || ''))
}

export interface AssistantRealtimeOptions {
  getEphemeralToken: () => Promise<string>
  onConnected?: () => void
  onDisconnected?: () => void
  /** Fatal: the session is gone. */
  onError?: (msg: string) => void
  /** Non-fatal: something was rejected but the call is still up. */
  onWarn?: (msg: string) => void
  onLog?: (msg: string) => void
  onRateLimits?: (limits: any[]) => void
}

export interface ConnectParams {
  enableTools?: boolean
  useSupervisor?: boolean
  supervisorMode?: 'always' | 'needed'
  // Session config
  model?: string
  voice?: string
  instructions?: string
  silenceDurationMs?: number
  idleTimeoutMs?: number | null
  inputAudioNoiseReduction?: boolean
  /** 'minimal' | 'low' | 'medium' | 'high' | 'xhigh', or null for the model default. */
  reasoningEffort?: string | null
  /** Disconnect after this long with nothing said. 0 or null disables it. */
  autoCloseMs?: number | null
  // Overrides DEFAULT_TRANSCRIPTION_MODEL. Useful for OpenAI-compatible
  // endpoints that only implement whisper-1.
  transcriptionModel?: string
}

export interface HistoryTurn { role: 'user' | 'assistant' | 'tool', content: string }

export function useAssistantRealtime(opts: AssistantRealtimeOptions) {
  const pcRef = ref<RTCPeerConnection | null>(null)
  const micStreamRef = ref<MediaStream | null>(null)
  const statusRef = ref<{ toolsCount: number, supervisor: boolean, voice?: string, silenceMs?: number, idleMs?: number }>({ toolsCount: 0, supervisor: false })
  let remoteAudioEl: HTMLAudioElement | null = document.createElement('audio')
  let currentUseSupervisor = false
  let currentSupervisorMode: 'always' | 'needed' = 'always'
  // The voice is baked into the ephemeral token and cannot be changed once the
  // model has produced audio, so it is only sent on the first session.update.
  let voiceSent = false
  let toolRounds = 0
  // The microphone stays open for the whole session, so muting has to be an
  // explicit control rather than something that only happens on disconnect.
  const micEnabled = ref(true)
  let autoCloseMs = 0
  let autoCloseTimer: any = 0
  // Whether the SDP exchange has completed. Server-side errors before that
  // point are fatal; after it, the audio call survives them.
  let connected = false
  try {
    if (remoteAudioEl) {
      remoteAudioEl.autoplay = true
      remoteAudioEl.setAttribute('playsinline', 'true')
      remoteAudioEl.muted = false
      remoteAudioEl.volume = 1.0
    }
  } catch {}

  let eventsDc: RTCDataChannel | null = null
  // Guards against handling the same spoken turn twice, since a transcript can
  // be reported by more than one event.
  const handledUserItems = new Set<string>()
  // Running transcript of the conversation. It serves two purposes: the
  // supervisor runs through a separate chat completion that would otherwise see
  // each utterance in isolation, and the panel renders it so a session is
  // readable rather than something you had to be listening to.
  const history = ref<HistoryTurn[]>([])

  function log(msg: string) {
    try { opts.onLog?.(msg) } catch {}
  }

  /**
   * Restart the inactivity countdown.
   *
   * Called whenever either side says something. An open realtime session holds
   * a live microphone and bills by the minute, so walking away from the window
   * should not keep costing money.
   */
  function resetAutoClose() {
    if (autoCloseTimer) { clearTimeout(autoCloseTimer); autoCloseTimer = 0 }
    if (!autoCloseMs || autoCloseMs <= 0) return
    autoCloseTimer = setTimeout(() => {
      log(`[session] no activity for ${Math.round(autoCloseMs / 1000)}s, closing`)
      void disconnect()
    }, autoCloseMs)
  }

  function send(payload: any): boolean {
    if (!eventsDc || eventsDc.readyState !== 'open') {
      log('[warn] data channel not open, dropped: ' + (payload?.type || 'event'))
      return false
    }
    try {
      eventsDc.send(JSON.stringify(payload))
      return true
    } catch (e: any) {
      log('[error] send failed (' + (payload?.type || 'event') + '): ' + (e?.message || e))
      return false
    }
  }

  /**
   * Run the supervisor over a user turn and have the realtime model speak the
   * answer.
   *
   * The supervisor is a normal chat completion (Prompt-section model, with MCP
   * tools already wired in), so it gets the running transcript rather than just
   * the latest utterance - without it every turn started from nothing and the
   * assistant could not follow up on its own previous answer.
   *
   * The result is spoken via `response.create` with a verbatim instruction. The
   * realtime model is the only thing holding the audio connection, so its voice
   * has to deliver the text; telling it to repeat the answer exactly is what
   * stops it from rewriting the supervisor's words.
   */
  /**
   * Put a question to the supervisor and return its answer as plain text.
   *
   * The supervisor is a normal chat completion using the Prompt-section model,
   * which already has the MCP tools wired in. It receives this session's
   * transcript so a follow-up question still makes sense on its own.
   */
  async function askSupervisor(userText: string): Promise<string> {
    // Logging the configuration is best-effort; failing to read it must not
    // stop the question being asked.
    try {
      const s: any = await invoke('get_settings').catch(() => null)
      const promptModel = (s?.prompt && s.prompt.model) ? s.prompt.model : (s?.model || 'n/a')
      const promptTemp = (s?.prompt && typeof s.prompt.temperature === 'number') ? s.prompt.temperature : (typeof s?.temperature === 'number' ? s.temperature : 'n/a')
      log(`[supervisor] using backend Prompt settings model=${promptModel}, temperature=${promptTemp}`)
    } catch {}

    const messages = [
      {
        role: 'system',
        content: 'You are the reasoning half of a voice assistant. Your reply is read aloud verbatim, so answer in plain spoken prose with no markdown, no code fences and no bullet lists. Keep it short unless asked for detail. Reply in the same language the user is speaking. If you are unsure, reply in English. Do not switch languages unless the user clearly switches.'
      },
      // Everything before the current turn, so the supervisor can follow up.
      // Tool entries are display-only; a chat completion would reject the role.
      ...history.value.slice(-20).filter((t) => t.role !== 'tool'),
      { role: 'user', content: userText }
    ] as any

    const text = await invoke<string>('chat_complete', { messages })
    return (text || '').trim()
  }

  /**
   * Ask the supervisor and have the realtime model read the answer out.
   *
   * Used by the "always" mode, where the realtime model is held silent
   * (`create_response: false`) and the supervisor answers every turn. The
   * verbatim instruction is what stops the voice model rewriting the answer in
   * its own words, which is what it did when this was phrased as a prompt.
   */
  async function supervisorRespond(userText: string) {
    try {
      const spoken = (await askSupervisor(userText)) || 'Sorry, I did not get a result for that.'
      history.value.push({ role: 'assistant', content: spoken })
      const ok = send({
        type: 'response.create',
        response: {
          instructions: `Say the following out loud, word for word, in the language it is written in. Do not summarise it, translate it, add to it, or comment on it:\n\n${spoken}`
        }
      })
      if (ok) log('[supervisor] injected response (' + spoken.length + ' chars)')
    } catch (e) {
      const msg = (e as any)?.message || String(e)
      log('[supervisor] failed: ' + msg)
      // Say something rather than leaving the user in silence.
      send({
        type: 'response.create',
        response: { instructions: 'Tell the user briefly that the supervisor could not answer that, then stop.' }
      })
    }
  }

  /**
   * Run the tool calls the model asked for and hand the results back.
   *
   * A realtime turn that contains a `function_call` is not finished: the model
   * is waiting for a matching `function_call_output` before it can speak. If
   * nothing answers, the conversation simply stalls - which is what happened
   * before, because nothing in this composable listened for tool calls at all.
   */
  async function handleToolCalls(calls: any[]) {
    if (!calls.length) return
    if (toolRounds >= MAX_TOOL_ROUNDS) {
      log(`[tools] round limit (${MAX_TOOL_ROUNDS}) reached, refusing further calls`)
      for (const c of calls) {
        send({
          type: 'conversation.item.create',
          item: {
            type: 'function_call_output',
            call_id: String(c?.call_id || ''),
            output: JSON.stringify({ error: 'tool call limit reached for this conversation' })
          }
        })
      }
      send({ type: 'response.create' })
      return
    }
    toolRounds += 1

    for (const c of calls) {
      const name = String(c?.name || '')
      const callId = String(c?.call_id || '')
      const argsJson = typeof c?.arguments === 'string' ? c.arguments : JSON.stringify(c?.arguments ?? {})
      if (!callId) { log('[tools] skipping call with no call_id: ' + name); continue }
      log(`[tools] -> ${name} ${argsJson.slice(0, 200)}`)
      let output: string
      if (name === SUPERVISOR_TOOL_NAME) {
        // Handled here rather than in Rust: the supervisor is a chat completion
        // that needs this session's transcript for context.
        try {
          const question = String(JSON.parse(argsJson || '{}')?.question || '').trim()
          const answer = await askSupervisor(question)
          output = JSON.stringify({ answer: answer || 'No answer available.' })
        } catch (e: any) {
          output = JSON.stringify({ error: (e?.message || String(e)) })
        }
      } else {
        try {
          output = await invoke<string>('realtime_call_tool', { name, argsJson })
        } catch (e: any) {
          output = JSON.stringify({ error: (e?.message || String(e)) })
        }
      }
      log(`[tools] <- ${name} ${output.slice(0, 200)}`)
      history.value.push({ role: 'tool', content: `${name} ${output.slice(0, 300)}` })
      send({
        type: 'conversation.item.create',
        item: { type: 'function_call_output', call_id: callId, output }
      })
    }
    // One response for the whole batch: the model now has every result.
    send({ type: 'response.create' })
  }

  function handleServerEvent(raw: string) {
    let parsed: any
    try { parsed = JSON.parse(raw) } catch { return }
    const type = String(parsed?.type || '')

    // The API reports rejected session updates and bad events here. These used
    // to be invisible, so a session.update the server threw out looked
    // identical to one it accepted.
    if (type === 'error') {
      const err = parsed?.error || {}
      const text = err.message || JSON.stringify(err)
      log(`[api error] ${err.type || 'error'}: ${text}`)
      // A rejected event does not drop the call, so it must not be reported as
      // a lost connection - that would show an Idle panel over live audio.
      try {
        if (connected) opts.onWarn?.(text)
        else opts.onError?.(text)
      } catch {}
      return
    }

    if (type === 'rate_limits.updated' && Array.isArray(parsed?.rate_limits)) {
      opts.onRateLimits?.(parsed.rate_limits)
      return
    }

    if (type === 'session.updated') {
      try { log('[session.updated] ' + JSON.stringify(parsed?.session || {})) } catch {}
      return
    }

    // What the user said. Drives history, the supervisor and the debug log.
    if (type === 'conversation.item.input_audio_transcription.completed') {
      const itemId = String(parsed?.item_id || '')
      const transcript = String(parsed?.transcript || '').trim()
      if (!transcript || (itemId && handledUserItems.has(itemId))) return
      if (itemId) handledUserItems.add(itemId)
      history.value.push({ role: 'user', content: transcript })
      log(`[user] ${transcript}`)
      resetAutoClose()
      // "needed" mode leaves the realtime model in charge; it escalates on its
      // own by calling consult_supervisor, so nothing to do here.
      if (currentUseSupervisor && currentSupervisorMode === 'always') {
        supervisorRespond(transcript).catch((e) => log('[supervisor] error in respond: ' + (e?.message || e)))
      }
      return
    }

    // What the model said, once its audio turn is transcribed.
    if (type === 'response.output_audio_transcript.done' || type === 'response.audio_transcript.done') {
      const text = String(parsed?.transcript || '').trim()
      if (text) {
        // The supervisor already recorded its own text; do not double-count it.
        const last = history.value[history.value.length - 1]
        if (!(last && last.role === 'assistant' && last.content === text)) {
          history.value.push({ role: 'assistant', content: text })
        }
        log(`[assistant] ${text}`)
      }
      resetAutoClose()
      return
    }

    if (type === 'response.done') {
      const output = Array.isArray(parsed?.response?.output) ? parsed.response.output : []
      const calls = output.filter((o: any) => o?.type === 'function_call')
      if (calls.length) {
        handleToolCalls(calls).catch((e) => log('[tools] batch failed: ' + (e?.message || e)))
      } else {
        // A turn that produced no tool call ends the tool chain.
        toolRounds = 0
      }
      const status = parsed?.response?.status
      if (status && status !== 'completed') {
        log(`[response.${status}] ` + JSON.stringify(parsed?.response?.status_details || {}))
      }
      return
    }
  }

  async function connect(params: ConnectParams = {}) {
    try {
      handledUserItems.clear()
      history.value = []
      voiceSent = false
      toolRounds = 0
      connected = false

      const pc = new RTCPeerConnection({
        iceServers: [
          { urls: 'stun:stun.l.google.com:19302' },
        ]
      })
      console.log('[realtime] pc created'); log('pc created')

      pc.oniceconnectionstatechange = () => {
        const s = pc.iceConnectionState
        console.log('[realtime] iceConnectionState:', s); log('iceConnectionState: ' + s)
        if (s === 'disconnected' || s === 'failed' || s === 'closed') {
          try { opts.onDisconnected?.() } catch {}
        }
      }
      pc.onconnectionstatechange = () => { console.log('[realtime] connectionState:', pc.connectionState); log('connectionState: ' + pc.connectionState) }
      pc.onicecandidate = (e) => { const msg = !!e.candidate ? 'candidate' : 'null (gathering complete)'; console.log('[realtime] icecandidate:', msg); log('icecandidate: ' + msg) }
      pc.ondatachannel = (e) => { console.log('[realtime] ondatachannel:', e.channel?.label); log('ondatachannel: ' + (e.channel?.label || '')) }

      pc.ontrack = (event) => {
        try {
          const [stream] = event.streams
          if (stream) {
            const el = remoteAudioEl
            if (el) {
              ;(el as any).srcObject = stream
              try { el.muted = false; el.volume = 1.0 } catch {}
              void el.play().catch((e: any) => { try { console.log('[realtime] el.play failed', e); log('el.play failed: ' + (e?.message || e)) } catch {} })
              console.log('[realtime] remote audio track set'); log('remote audio track set')
            }
          }
        } catch {}
      }

      // Bidirectional audio for WebRTC session
      pc.addTransceiver('audio', { direction: 'sendrecv' })

      // Capture microphone and add as sendonly track
      const mic = await navigator.mediaDevices.getUserMedia({ audio: true })
      mic.getAudioTracks().forEach((t) => pc.addTrack(t, mic))
      micEnabled.value = true

      // Data channel for OpenAI Realtime events
      eventsDc = pc.createDataChannel('oai-events')
      eventsDc.onopen = () => {
        console.log('[realtime] datachannel open'); log('datachannel open')
        void sendSessionUpdate(params)
      }
      eventsDc.onmessage = (e) => {
        const raw = String(e.data)
        // Dump the raw event, minus the per-token streams. A single spoken
        // sentence emits dozens of `.delta` events, which used to push
        // everything worth reading straight out of the 200-line debug buffer.
        if (!/"type"\s*:\s*"[^"]*\.delta"/.test(raw)) log('dc message: ' + raw)
        try { handleServerEvent(raw) } catch (err: any) {
          log('[error] event handler threw: ' + (err?.message || err))
        }
      }
      eventsDc.onerror = (e) => { try { console.log('[realtime] dc error:', e); log('dc error') } catch {} }
      eventsDc.onclose = () => { try { console.log('[realtime] dc close'); log('dc close') } catch {} }

      // Create SDP offer
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      console.log('[realtime] local description set (offer), iceGatheringState:', pc.iceGatheringState); log('local desc set; iceGatheringState: ' + pc.iceGatheringState)
      // Wait for ICE gathering to complete for non-trickle compatibility
      if (pc.iceGatheringState !== 'complete') {
        await new Promise<void>((resolve) => {
          const check = () => {
            if (pc.iceGatheringState === 'complete') {
              pc.removeEventListener('icegatheringstatechange', onState)
              resolve()
            }
          }
          const onState = () => {
            console.log('[realtime] iceGatheringState:', pc.iceGatheringState); log('iceGatheringState: ' + pc.iceGatheringState)
            check()
          }
          pc.addEventListener('icegatheringstatechange', onState)
          check()
          // Safety timeout in case some environments never emit complete
          setTimeout(() => { try { pc.removeEventListener('icegatheringstatechange', onState) } catch {}; resolve() }, 2000)
        })
      }
      const sdpToSend = pc.localDescription?.sdp || offer.sdp || ''
      console.log('[realtime] SDP offer size:', sdpToSend.length); log('SDP offer size: ' + sdpToSend.length)

      // Ephemeral token, minted in Rust so the API key never reaches the WebView.
      const token = await opts.getEphemeralToken()

      // The model and voice are already fixed by the token, so this carries no
      // query parameters.
      const resp = await fetch(REALTIME_CALLS_URL, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${token}`,
          'Content-Type': 'application/sdp',
          'Accept': 'application/sdp',
        },
        body: sdpToSend
      })
      if (!resp.ok) { const txt = await resp.text().catch(() => ''); throw new Error(`Realtime exchange failed: ${resp.status} ${txt}`) }
      const answerSdp = await resp.text()
      await pc.setRemoteDescription({ type: 'answer', sdp: answerSdp })
      console.log('[realtime] remote description set (answer)'); log('remote desc set (answer)')

      pcRef.value = pc
      micStreamRef.value = mic
      connected = true
      try { opts.onConnected?.() } catch {}
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : (err?.message || 'connect failed')
      try { console.log('[realtime] connect error:', err); opts.onError?.(msg) } catch {}
      await disconnect()
    }
  }

  /**
   * Push the session configuration over the data channel.
   *
   * The shape here is the GA realtime session: audio settings are nested under
   * `audio.input` / `audio.output` rather than sitting flat on the session, and
   * `temperature` no longer exists on a realtime session at all.
   */
  async function sendSessionUpdate(params: ConnectParams) {
    if (!eventsDc || eventsDc.readyState !== 'open') {
      log('[warn] session.update skipped: data channel not open')
      return
    }
    currentUseSupervisor = params.useSupervisor === true
    currentSupervisorMode = (params.supervisorMode === 'needed') ? 'needed' : 'always'
    autoCloseMs = typeof params.autoCloseMs === 'number' ? params.autoCloseMs : 0
    resetAutoClose()

    // Prefer the backend, which applies the same MCP tool filtering as the
    // Prompt section, and falls back to client-side discovery if it fails.
    let tools: any[] = []
    try {
      const res = await invoke<any>('realtime_build_tools')
      tools = Array.isArray(res) ? res : []
    } catch (e: any) {
      log('[warn] realtime_build_tools failed, falling back to client discovery: ' + (e?.message || e))
      tools = await buildMcpToolsClientSide()
    }
    // Three arrangements, one per mode:
    //  - supervisor "always": the realtime model is only a voice, no tools.
    //  - supervisor "needed": it answers what it can and escalates by tool.
    //  - supervisor off: MCP tools go directly to the realtime model.
    const supervisorNeeded = params.useSupervisor === true && currentSupervisorMode === 'needed'
    const includeTools = params.enableTools === true && params.useSupervisor !== true
    let toolsToSend: any[] = includeTools ? tools : []
    if (supervisorNeeded) toolsToSend = [SUPERVISOR_TOOL]

    const supervisorNote = supervisorNeeded
      ? ` Call ${SUPERVISOR_TOOL_NAME} whenever a question needs current information, the user's files or applications, or careful reasoning, and read its answer back.`
      : (params.useSupervisor ? ' A supervisor model answers on your behalf.' : '')

    const turnDetection: Record<string, any> = {
      type: 'server_vad',
      threshold: 0.5,
      prefix_padding_ms: 300,
      silence_duration_ms: typeof params.silenceDurationMs === 'number' ? params.silenceDurationMs : 2000,
      // Only "always" mode holds the model silent; in "needed" mode it has to
      // answer in order to decide whether it needs help at all.
      create_response: !(params.useSupervisor === true && currentSupervisorMode === 'always'),
      interrupt_response: true,
    }
    // Omit rather than send null: the field is optional and an explicit null is
    // rejected. Clamp rather than pass through - one value over the ceiling
    // fails the whole update, silently discarding every other setting.
    if (typeof params.idleTimeoutMs === 'number' && params.idleTimeoutMs > 0) {
      turnDetection.idle_timeout_ms = Math.min(params.idleTimeoutMs, MAX_IDLE_TIMEOUT_MS)
      if (params.idleTimeoutMs > MAX_IDLE_TIMEOUT_MS) {
        log(`[warn] idle timeout ${params.idleTimeoutMs}ms exceeds the ${MAX_IDLE_TIMEOUT_MS}ms maximum, clamped`)
      }
    }

    const audioInput: Record<string, any> = {
      turn_detection: turnDetection,
      // Always on. The transcript is what the debug log, the conversation
      // history and the supervisor all read; without it the session is a black
      // box even when it is working.
      transcription: { model: params.transcriptionModel || DEFAULT_TRANSCRIPTION_MODEL },
    }
    if (params.inputAudioNoiseReduction === true) {
      // near_field suits a typical desktop or headset microphone.
      audioInput.noise_reduction = { type: 'near_field' }
    }

    const audio: Record<string, any> = { input: audioInput }
    if (!voiceSent) {
      // The voice cannot be changed once the model has produced audio, so it is
      // sent once and then left alone.
      audio.output = { voice: params.voice || 'alloy' }
      voiceSent = true
    }

    const payload = {
      type: 'session.update',
      session: {
        type: 'realtime',
        output_modalities: ['audio'],
        instructions: (params.instructions && params.instructions.trim().length > 0)
          ? params.instructions
          : `You are an assistant in Assistant Mode. Speak clearly and concisely.${supervisorNote} IMPORTANT: Always reply in the same language the user is speaking/writing. If you are unsure, reply in English. Do not switch languages mid-conversation unless the user clearly switches.`,
        tools: toolsToSend,
        tool_choice: 'auto',
        audio,
        // Omitted unless both the model and the user asked for it: the
        // non-reasoning models reject the field outright.
        ...(params.reasoningEffort && modelSupportsReasoning(params.model)
          ? { reasoning: { effort: params.reasoningEffort } }
          : {}),
      }
    }

    try {
      const toolNames = toolsToSend.map((t: any) => t?.name || t?.function?.name || '').filter(Boolean).slice(0, 8)
      log('[session.update] ' + JSON.stringify({
        voice: audio.output?.voice ?? '(unchanged)',
        silence_ms: turnDetection.silence_duration_ms,
        idle_timeout_ms: turnDetection.idle_timeout_ms ?? null,
        noise_reduction: audioInput.noise_reduction ?? null,
        transcription: audioInput.transcription.model,
        reasoning_effort: (params.reasoningEffort && modelSupportsReasoning(params.model)) ? params.reasoningEffort : null,
        useSupervisor: params.useSupervisor === true,
        supervisorMode: currentSupervisorMode,
        enableTools: includeTools,
        tool_count: toolsToSend.length,
        tool_names_sample: toolNames,
      }))
      statusRef.value = {
        toolsCount: toolsToSend.length,
        supervisor: params.useSupervisor === true,
        voice: params.voice,
        silenceMs: turnDetection.silence_duration_ms,
        idleMs: turnDetection.idle_timeout_ms ?? undefined,
      }
    } catch {}

    send(payload)
  }

  async function buildMcpToolsClientSide(): Promise<any[]> {
    try {
      const settings = await invoke<any>('get_settings')
      const servers: any[] = Array.isArray(settings?.mcp_servers) ? settings.mcp_servers : []
      const toolDefs: any[] = []
      for (const s of servers) {
        if (!s || s.status !== 'connected' || !s.id) continue
        try {
          const v = await invoke<any>('mcp_list_tools', { serverId: s.id })
          const arr = Array.isArray(v?.tools) ? v.tools : (Array.isArray(v) ? v : [])
          for (const t of arr) {
            const name = (t?.name || '').toString()
            if (!name) continue
            const parameters = t?.input_schema || t?.inputSchema || t?.schema || { type: 'object', properties: {}, additionalProperties: true }
            // Flat shape, matching what the realtime session expects. The
            // nested `{function: {...}}` form is chat-completions only.
            toolDefs.push({
              type: 'function',
              name: `mcp__${s.id}__${name}`.replace(/[^a-zA-Z0-9_-]/g, '_'),
              description: (t?.description || `MCP tool ${name} from ${s.id}`),
              parameters,
            })
          }
        } catch {}
      }
      return toolDefs
    } catch {
      return []
    }
  }

  async function disconnect() {
    try { eventsDc?.close() } catch {}
    eventsDc = null
    try {
      micStreamRef.value?.getTracks().forEach(t => t.stop())
    } catch {}
    micStreamRef.value = null
    try {
      const pc = pcRef.value
      if (pc) {
        pc.getSenders().forEach(s => { try { s.track?.stop() } catch {} })
        pc.close()
      }
    } catch {}
    pcRef.value = null
    try { if (remoteAudioEl) (remoteAudioEl as any).srcObject = null } catch {}
    if (autoCloseTimer) { clearTimeout(autoCloseTimer); autoCloseTimer = 0 }
    handledUserItems.clear()
    voiceSent = false
    toolRounds = 0
    connected = false
    try { opts.onDisconnected?.() } catch {}
  }

  /**
   * Mute or unmute the microphone for the rest of the session.
   *
   * Disables the track rather than stopping it: a stopped track cannot be
   * restarted on the same connection, and re-negotiating just to unmute would
   * drop the conversation.
   */
  function setMicEnabled(enabled: boolean) {
    micEnabled.value = enabled
    try {
      micStreamRef.value?.getAudioTracks().forEach((t) => { t.enabled = enabled })
    } catch {}
    log(enabled ? '[mic] unmuted' : '[mic] muted')
  }

  function attachAudioElement(el: HTMLAudioElement) {
    remoteAudioEl = el
    try {
      remoteAudioEl.autoplay = true
      remoteAudioEl.setAttribute('playsinline', 'true')
      remoteAudioEl.muted = false
      remoteAudioEl.volume = 1.0
    } catch {}
  }

  async function updateSession(params: ConnectParams) {
    await sendSessionUpdate(params)
  }

  // The transcript deliberately survives disconnect, so the last conversation
  // is still readable after the session ends; `connect` clears it.
  return { connect, disconnect, attachAudioElement, updateSession, setMicEnabled, micEnabled, status: statusRef, transcript: history }
}
