// AudioWorklet for Gemini Live: turns the microphone into 16-bit PCM chunks.
//
// The AudioContext it runs in is created at the rate the server wants (16 kHz),
// so the browser has already resampled by the time samples arrive here. All
// this does is convert float to int16 and batch ~100ms per message - sending
// every 128-frame render quantum would be 125 WebSocket messages a second.

class PcmCaptureProcessor extends AudioWorkletProcessor {
  constructor() {
    super()
    this.chunkFrames = Math.round(sampleRate / 10)
    this.buf = new Int16Array(this.chunkFrames)
    this.len = 0
  }

  process(inputs) {
    const ch = inputs[0] && inputs[0][0]
    if (!ch) return true
    for (let i = 0; i < ch.length; i++) {
      const s = Math.max(-1, Math.min(1, ch[i]))
      this.buf[this.len++] = s < 0 ? s * 0x8000 : s * 0x7fff
      if (this.len === this.chunkFrames) {
        this.port.postMessage(this.buf.buffer, [this.buf.buffer])
        this.buf = new Int16Array(this.chunkFrames)
        this.len = 0
      }
    }
    return true
  }
}

registerProcessor('pcm-capture', PcmCaptureProcessor)
