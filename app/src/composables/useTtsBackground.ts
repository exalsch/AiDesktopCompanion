import { reactive } from 'vue'
import type { Ref } from 'vue'
import { listen } from '@tauri-apps/api/event'

export interface TtsBgState {
  playing: boolean
  currentMessageId: string
}

export function useTtsBackground(ttsBgRef: Ref<any>) {
  const ttsBg = reactive<TtsBgState>({ playing: false, currentMessageId: '' })

  async function registerBackgroundTtsEvents() {
    const unsubs: Array<() => void> = []

    // Background TTS playback (non-disruptive)
    const uPlay = await listen<{ text: string; id?: string }>('tts:play-background', (e) => {
      const p = (e?.payload as any) || {}
      const text = typeof p.text === 'string' ? p.text : ''
      const id = typeof p.id === 'string' ? p.id : ''
      requestAnimationFrame(() => {
        const c = ttsBgRef.value as any
        // Enforce single-stream: stop any current playback before starting new
        if (c?.stop) {
          Promise.resolve(c.stop()).catch(() => {}).finally(() => {
            Promise.resolve(c.setTextAndPlay?.(text)).then(() => {
              ttsBg.playing = true
              ttsBg.currentMessageId = id
            }).catch(() => {})
          })
        } else {
          Promise.resolve(c?.setTextAndPlay?.(text)).then(() => {
            ttsBg.playing = true
            ttsBg.currentMessageId = id
          }).catch(() => {})
        }
      })
    })
    unsubs.push(uPlay)

    const uStop = await listen('tts:stop-background', () => {
      requestAnimationFrame(() => {
        const c = ttsBgRef.value as any
        Promise.resolve(c?.stop?.()).finally(() => { ttsBg.playing = false; ttsBg.currentMessageId = '' })
      })
    })
    unsubs.push(uStop)

    // Observe speaking state to auto-clear when background playback ends naturally
    const uSpeaking = await listen<{ speaking: boolean }>('tts:speaking', (e) => {
      try {
        const speaking = !!((e?.payload as any)?.speaking)
        if (!speaking) { ttsBg.playing = false; ttsBg.currentMessageId = '' }
      } catch {}
    })
    unsubs.push(uSpeaking)

    return () => { try { unsubs.forEach(u => u()) } catch {} }
  }

  return { ttsBg, registerBackgroundTtsEvents }
}
