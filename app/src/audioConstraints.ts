//! Microphone capture constraints, in one place.
//
// The webview applies its own audio processing to a microphone stream - echo
// cancellation, noise suppression, automatic gain - and enables all three
// unless asked otherwise. Capture used to request a bare `audio: true` and
// inherit whatever the runtime happened to default to, which made the single
// setting that matters for dictation quality invisible and unfixable.
//
// The defaults here deliberately match what the webview already does, so
// pinning them changes nothing on its own. What it buys is that the flags are
// now stated, settable, and cannot drift with a WebView2 update.

/** The audio-processing half of the persisted STT settings. */
export interface SttAudioProcessingSettings {
  stt_echo_cancellation?: boolean
  stt_noise_suppression?: boolean
  stt_auto_gain_control?: boolean
  stt_voice_isolation?: boolean
}

/**
 * Read a persisted flag, falling back when it holds anything but a boolean.
 *
 * Settings written by an older build simply lack these keys, and a hand-edited
 * settings.json can hold anything at all. Neither should decide how the
 * microphone is opened.
 */
function flag(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback
}

/**
 * Build the `audio` constraints for a `getUserMedia` call.
 *
 * `deviceId` is passed separately rather than read from the settings blob: the
 * Assistant Mode microphone shares this processing configuration but not the
 * STT input-device choice, and folding the two together would silently move
 * that call onto a different device.
 *
 * `voiceIsolation` is an operating-system effect rather than a WebRTC one, so
 * it is the only flag here that can touch sound produced by another process.
 * It is also the only one that may not exist, hence the caller-supplied support
 * flag: an unsupported key is left out entirely rather than requested in hope.
 */
export function buildAudioConstraints(
  settings: SttAudioProcessingSettings | null | undefined,
  deviceId = '',
  voiceIsolationSupported = false,
): MediaTrackConstraints {
  const s = settings || {}
  const constraints: MediaTrackConstraints = {
    echoCancellation: flag(s.stt_echo_cancellation, true),
    noiseSuppression: flag(s.stt_noise_suppression, true),
    autoGainControl: flag(s.stt_auto_gain_control, true),
  }
  const id = String(deviceId || '').trim()
  if (id) constraints.deviceId = { exact: id }
  if (voiceIsolationSupported && flag(s.stt_voice_isolation, false)) {
    ;(constraints as Record<string, unknown>).voiceIsolation = true
  }
  return constraints
}

/**
 * Whether this machine's runtime knows the `voiceIsolation` constraint.
 *
 * Support needs both a recent enough webview and a device exposing the effect,
 * so on most machines this is simply false and the settings row says so rather
 * than offering a switch that does nothing. Never throws: a runtime without
 * `mediaDevices` at all must leave the settings page standing.
 */
export function isVoiceIsolationSupported(): boolean {
  try {
    const supported = navigator?.mediaDevices?.getSupportedConstraints?.()
    return !!(supported as Record<string, unknown> | undefined)?.voiceIsolation
  } catch {
    return false
  }
}
