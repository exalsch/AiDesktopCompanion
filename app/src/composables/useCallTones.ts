/**
 * Ringback and connect tones for Assistant Mode.
 *
 * Starting a call takes a second or two - an ephemeral token, an SDP exchange,
 * ICE - and until the first audio arrives there is nothing to tell the user
 * whether anything is happening. A phone solved this decades ago: you hear
 * ringing while it connects, and it stops when the other end picks up.
 *
 * Synthesised rather than shipped as audio files: it is two oscillators, it
 * keeps the bundle free of binary assets, and there is no decode latency on a
 * path where latency is the whole complaint.
 *
 * German/CEPT ringback, since that is where this is used: a 425 Hz tone, one
 * second on, four off.
 */

const RING_FREQ_HZ = 425
const RING_ON_S = 1.0
const RING_PERIOD_S = 5.0
/** Deliberately quiet - this plays over whatever the user is working in. */
const RING_PEAK = 0.05
/** Bursts scheduled up front. Well past any connect that is going to succeed. */
const RING_BURSTS = 24

const BEEP_FREQ_HZ = 880
const BEEP_LEN_S = 0.13
const BEEP_PEAK = 0.07

export function useCallTones() {
  let ctx: AudioContext | null = null
  let ringOsc: OscillatorNode | null = null
  let ringGain: GainNode | null = null

  /**
   * The context is created on demand and kept.
   *
   * A call starts from a global hotkey, which is not a user gesture as far as
   * the WebView is concerned, so the context can come up suspended. Resuming is
   * best-effort: silent tones are a cosmetic loss, and must never take the call
   * down with them.
   */
  function ensureCtx(): AudioContext | null {
    try {
      if (!ctx) {
        const Ctor: typeof AudioContext | undefined =
          (window as any).AudioContext || (window as any).webkitAudioContext
        if (!Ctor) return null
        ctx = new Ctor()
      }
      if (ctx.state === 'suspended') void ctx.resume().catch(() => {})
      return ctx
    } catch {
      return null
    }
  }

  /**
   * Start ringing, and keep ringing until `stopRingback`.
   *
   * The whole pattern is scheduled on the audio clock up front rather than
   * driven by a timer, because the main window is usually hidden behind another
   * application while a call connects and background timers get throttled - the
   * ring would stutter exactly when it is the only feedback there is.
   */
  function startRingback() {
    stopRingback()
    const c = ensureCtx()
    if (!c) return
    try {
      const osc = c.createOscillator()
      osc.type = 'sine'
      osc.frequency.value = RING_FREQ_HZ

      const gain = c.createGain()
      gain.gain.value = 0
      osc.connect(gain)
      gain.connect(c.destination)

      const t0 = c.currentTime + 0.05
      for (let i = 0; i < RING_BURSTS; i++) {
        const on = t0 + i * RING_PERIOD_S
        // Ramped rather than switched: a square edge on a sine is an audible
        // click, and this is meant to be unobtrusive.
        gain.gain.setValueAtTime(0, on)
        gain.gain.linearRampToValueAtTime(RING_PEAK, on + 0.04)
        gain.gain.setValueAtTime(RING_PEAK, on + RING_ON_S - 0.04)
        gain.gain.linearRampToValueAtTime(0, on + RING_ON_S)
      }

      osc.start(t0)
      osc.stop(t0 + RING_BURSTS * RING_PERIOD_S)
      ringOsc = osc
      ringGain = gain
    } catch {
      stopRingback()
    }
  }

  /** Stop ringing immediately, with a short fade so it does not click. */
  function stopRingback() {
    const c = ctx
    const osc = ringOsc
    const gain = ringGain
    ringOsc = null
    ringGain = null
    if (!c || !osc || !gain) return
    try {
      const now = c.currentTime
      gain.gain.cancelScheduledValues(now)
      gain.gain.setValueAtTime(gain.gain.value, now)
      gain.gain.linearRampToValueAtTime(0, now + 0.05)
      osc.stop(now + 0.06)
    } catch {}
    // Detaching on end keeps a long session from accumulating dead nodes.
    try { osc.onended = () => { try { osc.disconnect(); gain.disconnect() } catch {} } } catch {}
  }

  /** One soft beep: the far end picked up. */
  function readyBeep() {
    const c = ensureCtx()
    if (!c) return
    try {
      const osc = c.createOscillator()
      osc.type = 'sine'
      osc.frequency.value = BEEP_FREQ_HZ

      const gain = c.createGain()
      gain.gain.value = 0
      osc.connect(gain)
      gain.connect(c.destination)

      const t0 = c.currentTime + 0.02
      gain.gain.setValueAtTime(0, t0)
      gain.gain.linearRampToValueAtTime(BEEP_PEAK, t0 + 0.02)
      gain.gain.exponentialRampToValueAtTime(0.0001, t0 + BEEP_LEN_S)

      osc.start(t0)
      osc.stop(t0 + BEEP_LEN_S + 0.02)
      osc.onended = () => { try { osc.disconnect(); gain.disconnect() } catch {} }
    } catch {}
  }

  /** Release the audio device. Called when the panel goes away. */
  function dispose() {
    stopRingback()
    const c = ctx
    ctx = null
    try { void c?.close() } catch {}
  }

  return { startRingback, stopRingback, readyBeep, dispose }
}
