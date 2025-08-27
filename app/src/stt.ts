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
