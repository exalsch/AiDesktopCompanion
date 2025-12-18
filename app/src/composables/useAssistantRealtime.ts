import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

export interface AssistantRealtimeOptions {
  getEphemeralToken: () => Promise<string>
  onConnected?: () => void
  onDisconnected?: () => void
  onError?: (msg: string) => void
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
  temperature?: number
  instructions?: string
  silenceDurationMs?: number
  idleTimeoutMs?: number | null
  inputAudioNoiseReduction?: boolean
}

export function useAssistantRealtime(opts: AssistantRealtimeOptions) {
  const pcRef = ref<RTCPeerConnection | null>(null)
  const micStreamRef = ref<MediaStream | null>(null)
  const statusRef = ref<{ toolsCount: number, supervisor: boolean, temperature?: number, voice?: string, silenceMs?: number, idleMs?: number }>({ toolsCount: 0, supervisor: false })
  let remoteAudioEl: HTMLAudioElement | null = document.createElement('audio')
  let currentUseSupervisor = false
  let currentSupervisorMode: 'always' | 'needed' = 'always'
  try {
    if (remoteAudioEl) {
      remoteAudioEl.autoplay = true
      remoteAudioEl.setAttribute('playsinline', 'true')
      remoteAudioEl.muted = false
      remoteAudioEl.volume = 1.0
    }
  } catch {}

  function shouldCallSupervisorForText(userText: string) {
    const t = (userText || '').trim().toLowerCase()
    if (!t) return false
    if (t.length >= 220) return true

    const keywords = [
      'tool', 'tools', 'mcp', 'server', 'open', 'launch', 'run', 'execute',
      'search', 'look up', 'browse', 'website', 'url', 'http',
      'file', 'folder', 'directory',
      'calendar', 'email', 'slack', 'teams',
      'weather', 'news', 'stock', 'price',
      'debug', 'error', 'stack trace', 'refactor', 'code'
    ]
    for (const k of keywords) {
      if (t.includes(k)) return true
    }
    return false
  }

  function respondDirectlyViaRealtime(userText: string) {
    try {
      if (!eventsDc || eventsDc.readyState !== 'open') return
      const payload = {
        type: 'response.create',
        response: {
          modalities: ['audio', 'text'],
          instructions: `Reply to the user's last message. The user said: """${userText}""" IMPORTANT: Always reply in the same language the user is speaking/writing. If you are unsure, reply in English. Do not switch languages unless the user clearly switches.`
        }
      }
      eventsDc.send(JSON.stringify(payload))
      opts.onLog?.('[supervisor-needed] realtime responded (no supervisor)')
    } catch (e) {
      try { opts.onLog?.('[supervisor-needed] realtime response.create failed: ' + (e as any)?.message) } catch {}
    }
  }

  async function supervisorRespond(userText: string) {
    try {
      // Log supervisor configuration from backend Prompt settings (if available)
      try {
        const s: any = await invoke('get_settings').catch(() => null)
        const promptModel = (s?.prompt && s.prompt.model) ? s.prompt.model : (s?.model || 'n/a')
        const promptTemp = (s?.prompt && typeof s.prompt.temperature === 'number') ? s.prompt.temperature : (typeof s?.temperature === 'number' ? s.temperature : 'n/a')
        opts.onLog?.(`[supervisor] using backend Prompt settings model=${promptModel}, temperature=${promptTemp}`)
      } catch {}
      // Use backend chat completion (Prompt section model/settings) to decide and produce text; it already integrates MCP tools.
      // The command expects ChatMessage[] with roles 'system'/'user'.
      // IMPORTANT: Always pin output language to the user's language to avoid model drift (e.g., replying in Italian to English speech).
      const messages = [
        {
          role: 'system',
          content: 'Reply in the same language the user is speaking/writing. If you are unsure, reply in English. Do not switch languages unless the user clearly switches.'
        },
        { role: 'user', content: userText }
      ] as any
      const text = await invoke<string>('chat_complete', { messages })
      if (!eventsDc || eventsDc.readyState !== 'open') return
      const payload = {
        type: 'response.create',
        response: {
          modalities: ['audio','text'],
          instructions: text || 'Okay.',
        }
      }
      eventsDc.send(JSON.stringify(payload))
      opts.onLog?.('[supervisor] injected response')
    } catch (e) {
      opts.onLog?.('[supervisor] failed: ' + (e as any)?.message)
    }
  }

  let eventsDc: RTCDataChannel | null = null
  // Removed WebAudio routing to avoid double playback. We rely on <audio> element only.
  const handledUserItems = new Set<string>()

  async function connect(params: ConnectParams = {}) {
    try {
      // Create RTCPeerConnection
      const pc = new RTCPeerConnection({
        iceServers: [
          { urls: 'stun:stun.l.google.com:19302' },
        ]
      })
      console.log('[realtime] pc created'); try { opts.onLog?.('pc created') } catch {}

      pc.oniceconnectionstatechange = () => {
        const s = pc.iceConnectionState
        console.log('[realtime] iceConnectionState:', s); try { opts.onLog?.('iceConnectionState: ' + s) } catch {}
        if (s === 'disconnected' || s === 'failed' || s === 'closed') {
          try { opts.onDisconnected?.() } catch {}
        }
      }
      pc.onconnectionstatechange = () => { console.log('[realtime] connectionState:', pc.connectionState); try { opts.onLog?.('connectionState: ' + pc.connectionState) } catch {} }
      pc.onicecandidate = (e) => { const msg = !!e.candidate ? 'candidate' : 'null (gathering complete)'; console.log('[realtime] icecandidate:', msg); try { opts.onLog?.('icecandidate: ' + msg) } catch {} }
      pc.ondatachannel = (e) => { console.log('[realtime] ondatachannel:', e.channel?.label); try { opts.onLog?.('ondatachannel: ' + (e.channel?.label || '')) } catch {} }

      pc.ontrack = (event) => {
        try {
          const [stream] = event.streams
          if (stream) {
            const el = remoteAudioEl
            if (el) {
              ;(el as any).srcObject = stream
              try { el.muted = false; el.volume = 1.0 } catch {}
              void el.play().catch((e: any) => { try { console.log('[realtime] el.play failed', e); opts.onLog?.('el.play failed: ' + (e?.message || e)) } catch {} })
              console.log('[realtime] remote audio track set'); try { opts.onLog?.('remote audio track set') } catch {}
            }
            // No WebAudio routing
          }
        } catch {}
      }

      // Bidirectional audio for WebRTC session
      pc.addTransceiver('audio', { direction: 'sendrecv' })

      // Capture microphone and add as sendonly track
      const mic = await navigator.mediaDevices.getUserMedia({ audio: true })
      mic.getAudioTracks().forEach((t) => pc.addTrack(t, mic))

      // Data channel for OpenAI Realtime events
      eventsDc = pc.createDataChannel('oai-events')
      eventsDc.onopen = () => {
        console.log('[realtime] datachannel open'); try { opts.onLog?.('datachannel open') } catch {}
        // Send session.update with current settings (tools added if enabled)
        sendSessionUpdate(params).catch(() => {})
      }
      eventsDc.onmessage = (e) => {
        try {
          console.log('[realtime] dc message:', e.data);
          opts.onLog?.('dc message: ' + String(e.data))
          try {
            const parsed = JSON.parse(String(e.data))
            if (parsed?.type === 'rate_limits.updated' && Array.isArray(parsed?.rate_limits)) {
              opts.onRateLimits?.(parsed.rate_limits)
            }
            if (parsed?.type === 'session.updated') {
              try { opts.onLog?.('[session.updated] ' + JSON.stringify(parsed?.session || {})) } catch {}
            }
            // Supervisor orchestration: when a user message with text arrives, decide whether to call supervisor or let realtime respond.
            if (currentUseSupervisor && parsed?.type === 'conversation.item.created' && parsed?.item?.role === 'user') {
              const item = parsed.item
              const itemId = String(item?.id || '')
              if (itemId && !handledUserItems.has(itemId)) {
                const contentArr = Array.isArray(item?.content) ? item.content : []
                let userText = ''
                for (const c of contentArr) {
                  // Try to derive text from input_text or transcript fields
                  if (c && typeof c === 'object') {
                    if (typeof c.text === 'string' && c.type && String(c.type).includes('text')) { userText = c.text; break }
                    if (typeof c.transcript === 'string' && c.type && String(c.type).includes('input')) { userText = c.transcript; break }
                  }
                }
                if (userText && userText.trim()) {
                  handledUserItems.add(itemId)
                  try { opts.onLog?.(`[supervisor] handle user item ${itemId}, text: ${userText.slice(0, 120)}`) } catch {}
                  if (currentSupervisorMode === 'needed' && !shouldCallSupervisorForText(userText)) {
                    respondDirectlyViaRealtime(userText)
                  } else {
                    supervisorRespond(userText).catch((e) => { try { opts.onLog?.('[supervisor] error in respond: ' + (e?.message || e)) } catch {} })
                  }
                }
              }
            }
            // When transcription completes for an input_audio item, trigger supervisor with the transcript
            if (currentUseSupervisor && parsed?.type === 'conversation.item.input_audio_transcription.completed') {
              const itemId = String(parsed?.item_id || '')
              const transcript = String(parsed?.transcript || '')
              if (itemId && transcript && !handledUserItems.has(itemId)) {
                handledUserItems.add(itemId)
                try { opts.onLog?.(`[supervisor] transcript ready for ${itemId}: ${transcript.slice(0, 120)}`) } catch {}
                if (currentSupervisorMode === 'needed' && !shouldCallSupervisorForText(transcript)) {
                  respondDirectlyViaRealtime(transcript)
                } else {
                  supervisorRespond(transcript).catch((e) => { try { opts.onLog?.('[supervisor] error in respond: ' + (e?.message || e)) } catch {} })
                }
              }
            }
          } catch {}
        } catch {}
      }
      eventsDc.onerror = (e) => { try { console.log('[realtime] dc error:', e); opts.onLog?.('dc error') } catch {} }
      eventsDc.onclose = () => { try { console.log('[realtime] dc close'); opts.onLog?.('dc close') } catch {} }

      // Create SDP offer
      const offer = await pc.createOffer()
      await pc.setLocalDescription(offer)
      console.log('[realtime] local description set (offer), iceGatheringState:', pc.iceGatheringState); try { opts.onLog?.('local desc set; iceGatheringState: ' + pc.iceGatheringState) } catch {}
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
            console.log('[realtime] iceGatheringState:', pc.iceGatheringState); try { opts.onLog?.('iceGatheringState: ' + pc.iceGatheringState) } catch {}
            check()
          }
          pc.addEventListener('icegatheringstatechange', onState)
          check()
          // Safety timeout in case some environments never emit complete
          setTimeout(() => { try { pc.removeEventListener('icegatheringstatechange', onState) } catch {}; resolve() }, 2000)
        })
      }
      const sdpToSend = pc.localDescription?.sdp || offer.sdp || ''
      console.log('[realtime] SDP offer size:', sdpToSend.length); try { opts.onLog?.('SDP offer size: ' + sdpToSend.length) } catch {}

      // Fetch ephemeral token from backend
      const token = await opts.getEphemeralToken()

      // Exchange SDP with OpenAI Realtime
      const modelForSdp = params.model || 'gpt-4o-realtime-preview'
      const baseUrl = `https://api.openai.com/v1/realtime?model=${encodeURIComponent(modelForSdp)}`
      const resp = await fetch(baseUrl, {
        method: 'POST',
        headers: {
          'Authorization': `Bearer ${token}`,
          'Content-Type': 'application/sdp',
          'Accept': 'application/sdp',
          'OpenAI-Beta': 'realtime=v1'
        },
        body: sdpToSend
      })
      if (!resp.ok) { const txt = await resp.text().catch(() => ''); throw new Error(`Realtime exchange failed: ${resp.status} ${txt}`) }
      const answerSdp = await resp.text()
      await pc.setRemoteDescription({ type: 'answer', sdp: answerSdp })
      console.log('[realtime] remote description set (answer)'); try { opts.onLog?.('remote desc set (answer)') } catch {}

      pcRef.value = pc
      micStreamRef.value = mic
      try { opts.onConnected?.() } catch {}
    } catch (err: any) {
      const msg = typeof err === 'string' ? err : (err?.message || 'connect failed')
      try { console.log('[realtime] connect error:', err); opts.onError?.(msg) } catch {}
      await disconnect()
    }
  }

  async function sendSessionUpdate(params: ConnectParams) {
    try {
      if (!eventsDc || eventsDc.readyState !== 'open') return
      // Store supervisor flag for event handlers
      currentUseSupervisor = params.useSupervisor === true
      currentSupervisorMode = (params.supervisorMode === 'needed') ? 'needed' : 'always'

      // Prefer backend to mirror Prompt section MCP tool filtering/settings
      let tools: any[] = []
      try {
        const res = await invoke<any>('realtime_build_tools')
        tools = Array.isArray(res) ? res : []
      } catch {
        // Fallback to client-side discovery if backend not available
        tools = await buildMcpToolsClientSide()
      }
      // Only include tools when explicitly enabled AND supervisor is off
      const includeTools = params.enableTools === true && params.useSupervisor !== true
      const toolsToSend = includeTools ? tools : []

      const supervisorNote = params.useSupervisor ? ' A lightweight supervisor (gpt-4o-mini) may be consulted for tool calls.' : ''
      const payload = {
        type: 'session.update',
        session: {
          instructions: (params.instructions && params.instructions.trim().length > 0)
            ? params.instructions
            : `You are an assistant in Assistant Mode. Speak clearly and concisely.${supervisorNote} IMPORTANT: Always reply in the same language the user is speaking/writing. If you are unsure, reply in English. Do not switch languages mid-conversation unless the user clearly switches.`,
          voice: params.voice || 'verse',
          temperature: typeof params.temperature === 'number' ? params.temperature : 0.8,
          tools: toolsToSend,
          tool_choice: 'auto',
          turn_detection: {
            type: 'server_vad',
            threshold: 0.5,
            prefix_padding_ms: 300,
            silence_duration_ms: typeof params.silenceDurationMs === 'number' ? params.silenceDurationMs : 2000,
            idle_timeout_ms: typeof params.idleTimeoutMs === 'number' ? params.idleTimeoutMs : null,
            create_response: params.useSupervisor ? false : true,
            interrupt_response: true,
          },
          // Map to API-supported values: near_field or far_field. Use near_field for a typical close mic.
          input_audio_noise_reduction: params.inputAudioNoiseReduction === true ? { type: 'near_field' } : null,
          // Ask server to produce input text transcripts so supervisor can act
          input_audio_transcription: params.useSupervisor ? { model: 'whisper-1' } : null,
        }
      }
      try {
        const toolNames = Array.isArray(toolsToSend) ? toolsToSend.map((t:any) => t?.function?.name || t?.name || '').filter(Boolean).slice(0, 8) : []
        const dbg = {
          temperature: (payload.session as any).temperature,
          silence_ms: (payload.session as any).turn_detection?.silence_duration_ms,
          idle_timeout_ms: (payload.session as any).turn_detection?.idle_timeout_ms,
          noise_reduction: (payload.session as any).input_audio_noise_reduction,
          useSupervisor: params.useSupervisor === true,
          enableTools: includeTools,
          tool_count: toolsToSend.length,
          tool_names_sample: toolNames,
          voice: (payload.session as any).voice,
        }
        opts.onLog?.('[session.update] ' + JSON.stringify(dbg))
        statusRef.value = {
          toolsCount: toolsToSend.length,
          supervisor: params.useSupervisor === true,
          temperature: (payload.session as any).temperature,
          voice: (payload.session as any).voice,
          silenceMs: (payload.session as any).turn_detection?.silence_duration_ms,
          idleMs: (payload.session as any).turn_detection?.idle_timeout_ms,
        }
      } catch {}
      eventsDc.send(JSON.stringify(payload))
    } catch {}
  }

  function promptSpeak(text?: string) {
    try {
      if (!eventsDc || eventsDc.readyState !== 'open') return
      const payload = {
        type: 'response.create',
        response: {
          modalities: ['audio','text'],
          instructions: text || 'Please say hello and confirm audio output is working.',
        }
      }
      eventsDc.send(JSON.stringify(payload))
      console.debug('[realtime] sent response.create')
    } catch (e) {
      try { console.debug('[realtime] promptSpeak failed', e) } catch {}
    }
  }

  async function buildMcpToolsClientSide(): Promise<any[]> {
    try {
      // Attempt to list tools from all connected servers referenced in settings
      // The frontend does not have direct access to server list here; rely on Tauri to return all servers via settings.get
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
            const params = t?.input_schema || t?.inputSchema || t?.schema || { type: 'object', properties: {}, additionalProperties: true }
            toolDefs.push({ type: 'function', function: { name: `mcp__${s.id}__${name}`.replace(/[^a-zA-Z0-9_-]/g, '_'), description: (t?.description || `MCP tool ${name} from ${s.id}`), parameters: params } })
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
    try { opts.onDisconnected?.() } catch {}
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

  return { connect, disconnect, attachAudioElement, promptSpeak, updateSession, status: statusRef }
}
