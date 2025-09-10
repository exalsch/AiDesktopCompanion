use std::io::Cursor;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, Duration};

use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

// ---------------------------
// Audio decoding and WAV processing helpers (generic)
// ---------------------------

pub fn write_pcm16_wav_from_any(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
  // Try WAV-specific fast path first
  if apply_wav_gain_and_rate(bytes, target_path, rate, volume).is_ok() {
    return Ok(());
  }

  // Fallback: generic decode using Symphonia
  let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
  let hint = Hint::new();
  let probed = symphonia::default::get_probe()
    .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
    .map_err(|e| format!("audio probe failed: {e}"))?;

  let mut format = probed.format;
  let track = format.default_track().ok_or_else(|| "no default track".to_string())?;
  let track_id = track.id;
  let codec_params = track.codec_params.clone();
  let mut decoder = symphonia::default::get_codecs()
    .make(&codec_params, &DecoderOptions::default())
    .map_err(|e| format!("decoder init failed: {e}"))?;

  let mut out_rate: u32 = codec_params.sample_rate.unwrap_or(44100);
  let mut out_channels: u16 = codec_params
    .channels
    .map(|c| c.count() as u16)
    .unwrap_or(1);

  let mut pcm: Vec<f32> = Vec::new();

  loop {
    let packet = match format.next_packet() { Ok(p) => p, Err(_) => break };
    if packet.track_id() != track_id { continue; }
    match decoder.decode(&packet) {
      Ok(buf) => {
        match buf {
          AudioBufferRef::F32(b) => {
            let spec = *b.spec();
            out_rate = spec.rate;
            out_channels = spec.channels.count() as u16;
            let mut sbuf = SampleBuffer::<f32>::new(b.capacity() as u64, spec);
            sbuf.copy_interleaved_ref(AudioBufferRef::F32(b));
            pcm.extend_from_slice(sbuf.samples());
          }
          AudioBufferRef::S16(b) => {
            let spec = *b.spec();
            out_rate = spec.rate;
            out_channels = spec.channels.count() as u16;
            let mut sbuf = SampleBuffer::<i16>::new(b.capacity() as u64, spec);
            sbuf.copy_interleaved_ref(AudioBufferRef::S16(b));
            pcm.extend(sbuf.samples().iter().map(|v| *v as f32 / 32768.0));
          }
          AudioBufferRef::S32(b) => {
            let spec = *b.spec();
            out_rate = spec.rate;
            out_channels = spec.channels.count() as u16;
            let mut sbuf = SampleBuffer::<i32>::new(b.capacity() as u64, spec);
            sbuf.copy_interleaved_ref(AudioBufferRef::S32(b));
            let max = i32::MAX as f32;
            pcm.extend(sbuf.samples().iter().map(|v| *v as f32 / max));
          }
          AudioBufferRef::U8(b) => {
            let spec = *b.spec();
            out_rate = spec.rate;
            out_channels = spec.channels.count() as u16;
            let mut sbuf = SampleBuffer::<u8>::new(b.capacity() as u64, spec);
            sbuf.copy_interleaved_ref(AudioBufferRef::U8(b));
            pcm.extend(sbuf.samples().iter().map(|v| (*v as f32 - 128.0) / 128.0));
          }
          _ => {}
        }
      }
      Err(_) => {}
    }
  }

  if pcm.is_empty() { return Err("decode produced no samples".into()); }

  let r = rate.clamp(-10, 10);
  if r != 0 {
    let factor = (2f32).powf(r as f32 / 10.0);
    let new_rate = ((out_rate as f32) * factor).round() as u32;
    out_rate = new_rate.clamp(8000, 192000);
  }
  let gain: f32 = (volume as f32 / 100.0).max(0.0);
  let mut writer = hound::WavWriter::create(target_path, hound::WavSpec {
    channels: out_channels,
    sample_rate: out_rate,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  }).map_err(|e| format!("wav writer create failed: {e}"))?;

  for v in pcm.into_iter() {
    let s = (v * gain).clamp(-1.0, 1.0);
    let i = (s * 32767.0).round() as i16;
    writer.write_sample(i).map_err(|e| format!("wav write sample failed: {e}"))?;
  }
  writer.finalize().map_err(|e| format!("wav finalize failed: {e}"))?;
  Ok(())
}

pub fn apply_wav_gain_and_rate(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
  let mut reader = hound::WavReader::new(Cursor::new(bytes))
    .map_err(|e| format!("wav decode failed: {e}"))?;
  let in_spec = reader.spec();

  let gain: f32 = (volume as f32 / 100.0).max(0.0);
  let r = rate.clamp(-10, 10);
  let mut out_rate = in_spec.sample_rate;
  if r != 0 {
    let factor = (2f32).powf(r as f32 / 10.0);
    out_rate = ((out_rate as f32) * factor).round() as u32;
    out_rate = out_rate.clamp(8000, 192000);
  }
  let out_spec = hound::WavSpec {
    channels: in_spec.channels,
    sample_rate: out_rate,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  };

  let mut writer = hound::WavWriter::create(target_path, out_spec)
    .map_err(|e| format!("wav writer create failed: {e}"))?;

  match in_spec.sample_format {
    hound::SampleFormat::Float => {
      let mut it = reader.samples::<f32>();
      while let Some(s) = it.next() {
        let v = s.map_err(|e| format!("wav read sample failed: {e}"))?;
        let out = (v * gain).clamp(-1.0, 1.0);
        let i = (out * 32767.0).round() as i16;
        writer.write_sample(i).map_err(|e| format!("wav write sample failed: {e}"))?;
      }
    }
    hound::SampleFormat::Int => {
      if in_spec.bits_per_sample <= 16 {
        let mut it = reader.samples::<i16>();
        while let Some(s) = it.next() {
          let v = s.map_err(|e| format!("wav read sample failed: {e}"))? as i32;
          let out = ((v as f32) * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
          writer.write_sample(out).map_err(|e| format!("wav write sample failed: {e}"))?;
        }
      } else if in_spec.bits_per_sample <= 32 {
        let mut it = reader.samples::<i32>();
        let max_val: f32 = ((1i64 << (in_spec.bits_per_sample - 1)) - 1) as f32;
        while let Some(s) = it.next() {
          let v = s.map_err(|e| format!("wav read sample failed: {e}"))? as f32;
          let norm = (v / max_val) * gain;
          let i = (norm.clamp(-1.0, 1.0) * 32767.0).round() as i16;
          writer.write_sample(i).map_err(|e| format!("wav write sample failed: {e}"))?;
        }
      } else {
        return Err("unsupported bit depth".into());
      }
    }
  }

  writer.finalize().map_err(|e| format!("wav finalize failed: {e}"))?;
  Ok(())
}

// ---------------------------
// SSE parsing helpers (generic)
// ---------------------------

pub fn find_sse_event_boundary(buf: &[u8]) -> Option<usize> {
  for i in 0..buf.len().saturating_sub(1) {
    if buf[i] == b'\n' && buf[i + 1] == b'\n' { return Some(i + 2); }
    if i + 3 < buf.len() && buf[i] == b'\r' && buf[i + 1] == b'\n' && buf[i + 2] == b'\r' && buf[i + 3] == b'\n' {
      return Some(i + 4);
    }
  }
  None
}

pub fn consume_leading_newlines(buf: &mut Vec<u8>) -> usize {
  let mut n = 0;
  while n < buf.len() && (buf[n] == b'\n' || buf[n] == b'\r') { n += 1; }
  if n > 0 { let _ = buf.drain(..n); }
  n
}

pub fn extract_sse_data(ev_bytes: &[u8]) -> Option<String> {
  let text = String::from_utf8_lossy(ev_bytes);
  let mut data: Option<String> = None;
  for line in text.lines() {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix("data:") { data = Some(rest.trim_start().to_string()); }
  }
  data
}

// ---------------------------
// Temp WAV cleanup (generic)
// ---------------------------

pub fn delete_temp_wav(path: String) -> Result<bool, String> {
  let file_path = PathBuf::from(&path);
  if !file_path.exists() { return Ok(false); }
  let temp_dir = std::env::temp_dir();
  let temp_canon = std::fs::canonicalize(&temp_dir).unwrap_or(temp_dir.clone());
  let file_canon = std::fs::canonicalize(&file_path).map_err(|e| format!("canonicalize failed: {e}"))?;
  if !file_canon.starts_with(&temp_canon) { return Err("Refusing to delete non-temp file".into()); }
  let fname = file_canon.file_name().and_then(|s| s.to_str()).ok_or_else(|| "Invalid file name".to_string())?;
  if !(fname.starts_with("aidc_tts_") && fname.ends_with(".wav")) { return Err("Refusing to delete unexpected file".into()); }
  match fs::remove_file(&file_canon) { Ok(_) => Ok(true), Err(e) => { if e.kind() == std::io::ErrorKind::NotFound { Ok(false) } else { Err(format!("remove failed: {e}")) } } }
}

pub fn cleanup_stale_tts_wavs(max_age_minutes: Option<u64>) -> Result<u32, String> {
  let age_min = max_age_minutes.unwrap_or(240);
  let cutoff = SystemTime::now().checked_sub(Duration::from_secs(age_min.saturating_mul(60))).ok_or_else(|| "Invalid cutoff time".to_string())?;
  let temp_dir = std::env::temp_dir();
  let mut removed: u32 = 0;
  let it = match fs::read_dir(&temp_dir) { Ok(i) => i, Err(_) => return Ok(0) };
  for ent in it {
    if let Ok(de) = ent {
      let p = de.path();
      if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
        if name.starts_with("aidc_tts_") && name.to_ascii_lowercase().ends_with(".wav") {
          if let Ok(md) = de.metadata() { if let Ok(modified) = md.modified() { if modified < cutoff { let _ = fs::remove_file(&p).map(|_| { removed = removed.saturating_add(1); }); } } }
        }
      }
    }
  }
  Ok(removed)
}
