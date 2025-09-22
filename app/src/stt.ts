// Simple MediaRecorder-based audio capture for STT
// - startRecording(): requests mic, starts capturing into WEBM/Opus
// - stopRecording(): stops and returns { blob, mime }
// NOTE: Requires user gesture and OS permission to use microphone.

let mediaStream: MediaStream | null = null
let recorder: MediaRecorder | null = null
let chunks: BlobPart[] = []
let recording = false

export async function startRecording(preferredMime = 'audio/webm;codecs=opus'): Promise<void> {
  if (recording) return
  // Pick supported mime
  const mime = MediaRecorder.isTypeSupported(preferredMime)
    ? preferredMime
    : (MediaRecorder.isTypeSupported('audio/webm') ? 'audio/webm' : '')
  if (!mime) {
    throw new Error('No supported audio recording format (MediaRecorder)')
  }
  mediaStream = await navigator.mediaDevices.getUserMedia({ audio: true })
  chunks = []
  recorder = new MediaRecorder(mediaStream, { mimeType: mime })
  recorder.ondataavailable = (e) => {
    if (e.data && e.data.size > 0) chunks.push(e.data)
  }
  recorder.start()
  recording = true
}

export async function stopRecording(): Promise<{ blob: Blob, mime: string } | null> {
  if (!recording || !recorder) return null
  const mime = recorder.mimeType
  const r = recorder
  return await new Promise((resolve) => {
    r.onstop = async () => {
      try {
        const blob = new Blob(chunks, { type: mime })
        resolve({ blob, mime })
      } catch (e) {
        console.error('[stt] finalize failed', e)
        resolve(null)
      } finally {
        cleanup()
      }
    }
    r.stop()
    // Let recorder flush; tracks will be stopped in cleanup
  })
}

export function isRecording(): boolean { return recording }

function cleanup() {
  try { recorder && recorder.stream.getTracks().forEach(t => t.stop()) } catch {}
  try { mediaStream && mediaStream.getTracks().forEach(t => t.stop()) } catch {}
  recorder = null
  mediaStream = null
  chunks = []
  recording = false
}

// Transcode arbitrary audio blob to WAV 16kHz mono using WebAudio.
// This allows us to support WebM/Opus recordings on browsers that don't expose an Opus decoder on the Rust side.
export async function transcodeToWav16kMono(blob: Blob): Promise<Uint8Array> {
  const arrayBuf = await blob.arrayBuffer()
  // Decode using a regular AudioContext first to leverage platform decoders
  const ac = new (window.AudioContext || (window as any).webkitAudioContext)()
  const decoded = await ac.decodeAudioData(arrayBuf.slice(0) as ArrayBuffer)
  const duration = decoded.duration
  const targetSampleRate = 16000
  // Number of frames at 16kHz
  const length = Math.max(1, Math.round(duration * targetSampleRate))
  const oac = new OfflineAudioContext({ numberOfChannels: 1, length, sampleRate: targetSampleRate })
  const src = oac.createBufferSource()
  src.buffer = decoded
  src.connect(oac.destination)
  src.start(0)
  const rendered = await oac.startRendering()
  const ch = rendered.getChannelData(0)
  return encodeWav16kMonoFromFloat32(ch, targetSampleRate)
}

function encodeWav16kMonoFromFloat32(samples: Float32Array, sampleRate = 16000): Uint8Array {
  // 16-bit PCM WAV
  const numChannels = 1
  const bytesPerSample = 2
  const blockAlign = numChannels * bytesPerSample
  const byteRate = sampleRate * blockAlign
  const dataSize = samples.length * bytesPerSample
  const buffer = new ArrayBuffer(44 + dataSize)
  const view = new DataView(buffer)

  // RIFF header
  writeString(view, 0, 'RIFF')
  view.setUint32(4, 36 + dataSize, true)
  writeString(view, 8, 'WAVE')
  // fmt chunk
  writeString(view, 12, 'fmt ')
  view.setUint32(16, 16, true) // PCM chunk size
  view.setUint16(20, 1, true)  // Audio format: PCM
  view.setUint16(22, numChannels, true)
  view.setUint32(24, sampleRate, true)
  view.setUint32(28, byteRate, true)
  view.setUint16(32, blockAlign, true)
  view.setUint16(34, 16, true) // bits per sample
  // data chunk
  writeString(view, 36, 'data')
  view.setUint32(40, dataSize, true)
  // PCM samples
  let offset = 44
  for (let i = 0; i < samples.length; i++) {
    // clamp [-1,1] and convert to 16-bit
    let s = Math.max(-1, Math.min(1, samples[i]))
    view.setInt16(offset, (s < 0 ? s * 0x8000 : s * 0x7FFF) | 0, true)
    offset += 2
  }
  return new Uint8Array(buffer)
}

function writeString(view: DataView, offset: number, str: string) {
  for (let i = 0; i < str.length; i++) {
    view.setUint8(offset + i, str.charCodeAt(i))
  }
}
