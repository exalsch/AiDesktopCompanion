import { reactive, ref, watch, nextTick } from 'vue'
import { invoke, convertFileSrc } from '@tauri-apps/api/core'
import type { Ref } from 'vue'

export interface NotifyFn { (msg: string, kind?: 'error' | 'success', ms?: number): void }

export const OPENAI_TTS_MAX_INPUT_CHARS = 3500

export function useTtsPlayback(notify?: NotifyFn) {
  const engine = ref<'local' | 'openai'>('local')

  const form = reactive({
    text: '' as string,
    voice: '' as string,
    rate: -2 as number,
    volume: 100 as number,
    openaiVoice: 'alloy' as string,
    openaiModel: 'gpt-4o-mini-tts' as string,
    openaiFormat: 'wav' as 'wav' | 'mp3' | 'opus',
    openaiStreaming: false as boolean,
    openaiInstructions: '' as string,
  })

  const speaking = ref(false)
  const busy = ref(false)
  const wavPath = ref('')
  const wavSrc = ref('')
  const lastPlayTempPath = ref('')
  const playerRef = ref<HTMLAudioElement | null>(null)
  let localPollHandle: ReturnType<typeof setInterval> | null = null

  // Streaming state
  const streamSessionUrl = ref('')
  let streamSessionId: string | null = null

  function validateTtsInput(): boolean {
    const text = form.text.trim()
    if (!text) {
      notify?.('Enter some text to speak', 'error')
      return false
    }
    if (engine.value === 'openai' && text.length > OPENAI_TTS_MAX_INPUT_CHARS) {
      notify?.(`OpenAI TTS input is limited to ${OPENAI_TTS_MAX_INPUT_CHARS} characters.`, 'error')
      return false
    }
    return true
  }

  async function onPlay() {
    if (!validateTtsInput()) { return }
    try {
      if (engine.value === 'local') {
        await invoke('tts_start', { text: form.text, voice: form.voice || null, rate: form.rate, volume: form.volume })
        speaking.value = true
        // Clear any existing poll before starting a new one
        if (localPollHandle) { clearInterval(localPollHandle); localPollHandle = null }
        // Poll for PowerShell process completion since local TTS has no end callback
        localPollHandle = setInterval(async () => {
          try {
            const still = await invoke<boolean>('tts_is_speaking')
            if (!still) { speaking.value = false; if (localPollHandle) { clearInterval(localPollHandle); localPollHandle = null } }
          } catch { speaking.value = false; if (localPollHandle) { clearInterval(localPollHandle); localPollHandle = null } }
        }, 500)
      } else {
        if (form.openaiStreaming) {
          await startProxyStreaming()
        } else {
          busy.value = true
          const fmt = form.openaiFormat || 'wav'
          const path = await invoke<string>('tts_openai_synthesize_file', {
            text: form.text,
            voice: form.openaiVoice || 'alloy',
            model: form.openaiModel || 'gpt-4o-mini-tts',
            format: fmt,
            rate: form.rate,
            volume: form.volume,
            instructions: form.openaiInstructions || null,
          })
          busy.value = false
          wavPath.value = path
          wavSrc.value = convertFileSrc(path)
          lastPlayTempPath.value = path
          requestAnimationFrame(() => {
            const a = playerRef.value
            if (a) {
              let fallbackTried = false
              const tryWavFallback = async (): Promise<boolean> => {
                if (fallbackTried) return false
                fallbackTried = true
                try {
                  const fallbackPath = await invoke<string>('tts_openai_synthesize_file', {
                    text: form.text,
                    voice: form.openaiVoice || 'alloy',
                    model: form.openaiModel || 'gpt-4o-mini-tts',
                    format: 'wav',
                    rate: form.rate,
                    volume: form.volume,
                    instructions: form.openaiInstructions || null,
                  })
                  const oldPath = lastPlayTempPath.value
                  wavPath.value = fallbackPath
                  wavSrc.value = convertFileSrc(fallbackPath)
                  lastPlayTempPath.value = fallbackPath
                  if (oldPath && oldPath !== fallbackPath) {
                    try { await invoke<boolean>('tts_delete_temp_wav', { path: oldPath }) } catch {}
                  }
                  a.src = wavSrc.value
                  a.currentTime = 0
                  await a.play()
                  speaking.value = true
                  notify?.('Selected TTS format was not playable. Retried with WAV.', 'error', 3200)
                  return true
                } catch {
                  return false
                }
              }

              const factor = Math.max(0.25, Math.min(4, Math.pow(2, (form.rate || 0) / 10)))
              a.playbackRate = factor
              a.volume = Math.max(0, Math.min(1, (form.volume || 100) / 100))
              a.currentTime = 0
              a.onerror = async () => {
                const recovered = await tryWavFallback()
                if (!recovered) {
                  speaking.value = false
                  notify?.('Failed to load synthesized audio in this format.', 'error')
                }
              }
              a.play().then(() => {
                speaking.value = true
              }).catch(async () => {
                const recovered = await tryWavFallback()
                if (!recovered) {
                  speaking.value = false
                  notify?.('Failed to play synthesized audio in this format.', 'error')
                }
              })
              a.onended = async () => {
                speaking.value = false
                const p = lastPlayTempPath.value
                if (p) {
                  try { await invoke<boolean>('tts_delete_temp_wav', { path: p }) } catch {}
                  if (wavPath.value === p) { wavPath.value = ''; wavSrc.value = '' }
                  lastPlayTempPath.value = ''
                }
              }
            }
          })
        }
      }
    } catch (e: any) {
      const msg = e?.message || String(e) || 'TTS start failed'
      notify?.(msg, 'error')
      busy.value = false
    }
  }

  async function onStop() {
    // Clear local TTS poll if active
    if (localPollHandle) { clearInterval(localPollHandle); localPollHandle = null }
    try {
      if (engine.value === 'local') {
        await invoke('tts_stop')
      } else {
        await stopProxyStreaming()
        const a = playerRef.value
        if (a) { a.pause(); a.currentTime = 0; a.onended = null; a.onerror = null }
        const p = lastPlayTempPath.value
        if (p) {
          try { await invoke<boolean>('tts_delete_temp_wav', { path: p }) } catch {}
          if (wavPath.value === p) { wavPath.value = ''; wavSrc.value = '' }
          lastPlayTempPath.value = ''
        }
      }
    } catch {}
    finally { speaking.value = false }
  }

  async function startProxyStreaming() {
    if (!validateTtsInput()) { return }
    busy.value = true
    await nextTick()
    const a = playerRef.value
    if (!a) { busy.value = false; notify?.('Audio element not ready', 'error'); return }
    let url = ''
    try {
      const desiredFmt = (form.openaiFormat || 'mp3') as 'wav'|'mp3'|'opus'
      const fmtToMime: Record<string, string> = { wav: 'audio/wav', mp3: 'audio/mpeg', opus: 'audio/ogg' }
      let chosenFmt: 'wav'|'mp3'|'opus' = desiredFmt
      const mime = fmtToMime[desiredFmt] || 'audio/mpeg'
      try {
        const support = a.canPlayType(mime)
        if (!support) chosenFmt = 'mp3'
      } catch { chosenFmt = 'mp3' }
      if (chosenFmt !== desiredFmt) {
        notify?.(`Selected format ${desiredFmt.toUpperCase()} not supported for streaming. Falling back to MP3.`, 'error', 3000)
      }
      url = await invoke<string>('tts_create_stream_session', {
        text: form.text,
        voice: form.openaiVoice || 'alloy',
        model: form.openaiModel || 'gpt-4o-mini-tts',
        format: chosenFmt || 'mp3',
        instructions: form.openaiInstructions || null,
      })
    } catch (e: any) {
      busy.value = false
      notify?.(e?.message || String(e) || 'Failed to start streaming session', 'error')
      return
    }
    streamSessionUrl.value = url
    streamSessionId = (url.split('/').pop() || '').trim() || null
    const factor = Math.max(0.25, Math.min(4, Math.pow(2, (form.rate || 0) / 10)))
    a.playbackRate = factor
    a.volume = Math.max(0, Math.min(1, (form.volume || 100) / 100))
    a.src = url
    a.currentTime = 0
    a.play().then(() => { speaking.value = true; busy.value = false }).catch(err => { busy.value = false; notify?.(String(err) || 'Failed to start playback', 'error') })
    a.onerror = async () => {
      try { await stopProxyStreaming() } catch {}
      try {
        busy.value = true
        const path = await invoke<string>('tts_openai_synthesize_file', {
          text: form.text,
          voice: form.openaiVoice || 'alloy',
          model: form.openaiModel || 'gpt-4o-mini-tts',
          format: 'mp3',
          rate: form.rate,
          volume: form.volume,
          instructions: form.openaiInstructions || null,
        })
        busy.value = false
        wavPath.value = path
        wavSrc.value = convertFileSrc(path)
        lastPlayTempPath.value = path
        requestAnimationFrame(() => {
          const el = playerRef.value
          if (el) {
            const factor2 = Math.max(0.25, Math.min(4, Math.pow(2, (form.rate || 0) / 10)))
            el.playbackRate = factor2
            el.volume = Math.max(0, Math.min(1, (form.volume || 100) / 100))
            el.currentTime = 0
            el.play().catch(() => {})
            speaking.value = true
            el.onended = async () => {
              speaking.value = false
              const p = lastPlayTempPath.value
              if (p) {
                try { await invoke<boolean>('tts_delete_temp_wav', { path: p }) } catch {}
                if (wavPath.value === p) { wavPath.value = ''; wavSrc.value = '' }
                lastPlayTempPath.value = ''
              }
            }
          }
        })
      } catch (e: any) {
        busy.value = false
        notify?.(e?.message || String(e) || 'Fallback playback failed', 'error')
      }
    }
    a.onended = () => {
      speaking.value = false
      if (streamSessionId) { invoke('tts_stop_stream_session', { session_id: streamSessionId }).catch(() => {}) }
      streamSessionId = null
      streamSessionUrl.value = ''
    }
  }

  async function stopProxyStreaming() {
    const a = playerRef.value
    if (a) {
      try { a.pause() } catch {}
      // Prevent error handler from firing fallback synth after manual stop
      try { (a as any).onerror = null } catch {}
      try { (a as any).onended = null } catch {}
      if (streamSessionUrl.value && a.src === streamSessionUrl.value) { a.src = '' }
    }
    if (streamSessionId) { try { await invoke('tts_stop_stream_session', { session_id: streamSessionId }) } catch {} }
    streamSessionId = null
    streamSessionUrl.value = ''
    speaking.value = false
    busy.value = false
  }

  async function onSynthesize() {
    if (!validateTtsInput()) { return }
    try {
      busy.value = true
      const path = engine.value === 'local'
        ? await invoke<string>('tts_synthesize_wav', { text: form.text, voice: form.voice || null, rate: form.rate, volume: form.volume })
        : await invoke<string>('tts_openai_synthesize_file', { text: form.text, voice: (form.openaiVoice || 'alloy'), model: (form.openaiModel || 'gpt-4o-mini-tts'), format: (form.openaiFormat || 'wav'), rate: form.rate, volume: form.volume, instructions: form.openaiInstructions || null })
      busy.value = false
      wavPath.value = path
      wavSrc.value = convertFileSrc(path)
    } catch (e: any) {
      const msg = e?.message || String(e) || 'Synthesize failed'
      notify?.(msg, 'error')
    } finally {
      busy.value = false
    }
  }

  return {
    engine,
    form,
    speaking,
    busy,
    wavPath,
    wavSrc,
    validateTtsInput,
    lastPlayTempPath,
    playerRef,
    onPlay,
    onStop,
    onSynthesize,
    startProxyStreaming,
    stopProxyStreaming,
  }
}
