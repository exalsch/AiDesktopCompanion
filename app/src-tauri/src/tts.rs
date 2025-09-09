use std::io::Cursor;

// Audio decoding (fallback for non-WAV responses)
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use futures_util::StreamExt;
use tauri::Emitter;
use serde_json;
use base64::Engine;
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};
#[cfg(target_os = "windows")]
use std::io::Write;
#[cfg(target_os = "windows")]
use once_cell::sync::Lazy;
#[cfg(target_os = "windows")]
use std::sync::Mutex;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, Duration};
use crate::utils::ps_escape_single_quoted;
use crate::tts_streaming_server::TtsStreamingServer;
use once_cell::sync::Lazy as OnceLazy;
use std::sync::Mutex as StdMutex;

// No submodule API; use top-level public functions directly from other modules.

/// Decode arbitrary audio bytes (e.g., WAV/MP3/AAC) and write a 16-bit PCM WAV.
/// If the buffer is already WAV, we try hound first; otherwise, we fall back to Symphonia.
pub fn write_pcm16_wav_from_any(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
  // Try WAV path first
  if apply_wav_gain_and_rate(bytes, target_path, rate, volume).is_ok() {
    return Ok(());
  }

  // Fallback: generic decode using Symphonia
  // Own the data so the media source can be 'static.
  let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
  let hint = Hint::new();
  let probed = symphonia::default::get_probe()
    .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
    .map_err(|e| format!("audio probe failed: {e}"))?;

  let mut format = probed.format;
  let track = format.default_track().ok_or_else(|| "no default track".to_string())?;
  // Copy required info to avoid holding an immutable borrow of `format` during the loop.
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
    let packet = match format.next_packet() {
      Ok(p) => p,
      Err(_) => break,
    };
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
          _ => {
            // Other formats not explicitly handled are ignored.
          }
        }
      }
      Err(_e) => { /* skip bad packet */ }
    }
  }

  if pcm.is_empty() {
    return Err("decode produced no samples".into());
  }

  // Apply rate and volume, then write WAV
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

// ---------------------------
// TTS Streaming Server: shared state and helpers
// ---------------------------

static TTS_STREAMING_SERVER: OnceLazy<StdMutex<Option<TtsStreamingServer>>> = OnceLazy::new(|| StdMutex::new(None));

pub async fn ensure_streaming_server() -> Result<(), String> {
  let need_init = {
    let guard = TTS_STREAMING_SERVER.lock().map_err(|_| "Mutex poisoned")?;
    guard.is_none()
  };
  if need_init {
    let server = TtsStreamingServer::new().await.map_err(|e| format!("init streaming server failed: {}", e))?;
    let mut guard = TTS_STREAMING_SERVER.lock().map_err(|_| "Mutex poisoned")?;
    *guard = Some(server);
  }
  Ok(())
}

pub async fn create_stream_session(text: String, voice: Option<String>, model: Option<String>, format: Option<String>, instructions: Option<String>, api_key: String) -> Result<String, String> {
  ensure_streaming_server().await?;
  let guard = TTS_STREAMING_SERVER.lock().map_err(|_| "Mutex poisoned")?;
  let server = guard.as_ref().ok_or_else(|| "TTS streaming server not available".to_string())?;
  let voice = voice.unwrap_or_else(|| "alloy".to_string());
  let model = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let format = format.unwrap_or_else(|| "mp3".to_string());
  let session_id = server.create_session(text, voice, model, format, api_key, instructions);
  let url = server.get_stream_url(&session_id);
  Ok(url)
}

pub fn stop_stream_session(session_id: String) -> Result<bool, String> {
  let guard = TTS_STREAMING_SERVER.lock().map_err(|_| "Mutex poisoned")?;
  if let Some(server) = guard.as_ref() { Ok(server.stop_session(&session_id)) } else { Err("TTS streaming server not available".into()) }
}

pub fn stream_session_count() -> Result<usize, String> {
  let guard = TTS_STREAMING_SERVER.lock().map_err(|_| "Mutex poisoned")?;
  if let Some(server) = guard.as_ref() { Ok(server.count_sessions()) } else { Ok(0) }
}

pub fn stream_cleanup_idle(ttl_seconds: u64) -> Result<usize, String> {
  let guard = TTS_STREAMING_SERVER.lock().map_err(|_| "Mutex poisoned")?;
  if let Some(server) = guard.as_ref() { Ok(server.cleanup_idle(Duration::from_secs(ttl_seconds))) } else { Ok(0) }
}

// ---------------------------
// OpenAI direct streaming (speech and responses): shared state and helpers
// ---------------------------
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use once_cell::sync::Lazy as GlobalLazy;
use tokio::sync::oneshot;

static STREAM_COUNTER: GlobalLazy<AtomicU64> = GlobalLazy::new(|| AtomicU64::new(0));
static STREAM_STOPPERS: GlobalLazy<StdMutex<HashMap<u64, oneshot::Sender<()>>>> = GlobalLazy::new(|| StdMutex::new(HashMap::new()));

pub fn openai_stream_start(
  app: tauri::AppHandle,
  key: String,
  text: String,
  voice: Option<String>,
  model: Option<String>,
  format: Option<String>,
) -> Result<u64, String> {
  let fmt = format.unwrap_or_else(|| "opus".to_string());
  let (accept, body_format, mime): (&'static str, &'static str, &'static str) = match fmt.as_str() {
    "mp3" => ("audio/mpeg", "mp3", "audio/mpeg"),
    _ => ("audio/ogg", "opus", "audio/ogg; codecs=opus"),
  };
  let m = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let v = voice.unwrap_or_else(|| "alloy".to_string());
  let body = serde_json::json!({ "model": m, "input": text, "voice": v, "format": body_format });

  let (tx, rx) = oneshot::channel::<()>();
  let id = STREAM_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
  {
    let mut map = STREAM_STOPPERS.lock().map_err(|_| "Mutex poisoned")?;
    map.insert(id, tx);
  }
  spawn_speech_stream(app, key, body, accept, mime, id, rx, move |rid| {
    if let Ok(mut map) = STREAM_STOPPERS.lock() { map.remove(&rid); }
  });
  Ok(id)
}

pub fn openai_stream_stop(id: u64) -> Result<bool, String> {
  let tx = {
    let mut map = STREAM_STOPPERS.lock().map_err(|_| "Mutex poisoned")?;
    map.remove(&id)
  };
  if let Some(tx) = tx { let _ = tx.send(()); Ok(true) } else { Ok(false) }
}

pub fn responses_stream_start(
  app: tauri::AppHandle,
  key: String,
  text: String,
  voice: Option<String>,
  model: Option<String>,
  format: Option<String>,
) -> Result<u64, String> {
  let fmt = format.unwrap_or_else(|| "opus".to_string());
  let req_model = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let m = if req_model.contains("tts") { "gpt-4o-realtime-preview".to_string() } else { req_model };
  let v = voice.unwrap_or_else(|| "alloy".to_string());
  let body = serde_json::json!({
    "model": m,
    "modalities": ["text", "audio"],
    "audio": { "voice": v, "format": fmt },
    "input": text,
    "stream": true
  });
  let (tx, rx) = oneshot::channel::<()>();
  let id = STREAM_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
  {
    let mut map = STREAM_STOPPERS.lock().map_err(|_| "Mutex poisoned")?;
    map.insert(id, tx);
  }
  spawn_responses_stream(app, key, body, fmt, id, rx, move |rid| {
    if let Ok(mut map) = STREAM_STOPPERS.lock() { map.remove(&rid); }
  });
  Ok(id)
}

// ---------------------------
// Local TTS helpers
// ---------------------------

#[cfg(target_os = "windows")]
static TTS_CHILD: Lazy<Mutex<Option<std::process::Child>>> = Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "windows")]
pub fn local_tts_start(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<(), String> {
  if let Ok(mut guard) = TTS_CHILD.lock() {
    if let Some(mut c) = guard.take() { let _ = c.kill(); let _ = c.wait(); }
  }
  let v = voice.unwrap_or_default();
  let v_escaped = ps_escape_single_quoted(&v);
  let r = rate.unwrap_or(-2).clamp(-10, 10);
  let vol = volume.unwrap_or(100).min(100);
  let ps = format!(
    r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
try {{
  $s.Volume = {vol};
  $s.Rate = {r};
  if ('{voice}' -ne '') {{ try {{ $s.SelectVoice('{voice}'); }} catch {{}} }}
  [void]$s.Speak([Console]::In.ReadToEnd());
}} finally {{ $s.Dispose(); }}
"#,
    vol = vol, r = r, voice = v_escaped,
  );
  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if let Some(stdin) = child.stdin.as_mut() { stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?; }
  drop(child.stdin.take());
  if let Ok(mut guard) = TTS_CHILD.lock() { *guard = Some(child); }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_start(_text: String, _voice: Option<String>, _rate: Option<i32>, _volume: Option<u8>) -> Result<(), String> {
  Err("TTS not implemented on this platform".into())
}

#[cfg(target_os = "windows")]
pub fn local_tts_stop() -> Result<(), String> {
  if let Ok(mut guard) = TTS_CHILD.lock() {
    if let Some(mut c) = guard.take() {
      let _ = c.kill(); let _ = c.wait();
    }
  }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_stop() -> Result<(), String> { Err("TTS not implemented on this platform".into()) }

#[cfg(target_os = "windows")]
pub fn local_tts_list_voices() -> Result<Vec<String>, String> {
  let ps = r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
$names = $s.GetInstalledVoices() | ForEach-Object { $_.VoiceInfo.Name };
$s.Dispose();
$names | ForEach-Object { $_ }
"#;
  let out = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", ps])
    .output()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if !out.status.success() { return Err(format!("powershell exited with status: {}", out.status)); }
  let s = String::from_utf8_lossy(&out.stdout);
  let mut names: Vec<String> = s.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| l.to_string()).collect();
  names.dedup();
  Ok(names)
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_list_voices() -> Result<Vec<String>, String> { Ok(vec![]) }

// Speak text synchronously using System.Speech (blocking) with explicit voice/rate/vol
#[cfg(target_os = "windows")]
pub fn local_speak_blocking(text: String, voice: String, rate: i32, vol: u8) -> Result<(), String> {
  let v_escaped = ps_escape_single_quoted(&voice);
  let ps = format!(
    r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
try {{
  $s.Volume = {vol};
  $s.Rate = {rate};
  if ('{voice}' -ne '') {{ try {{ $s.SelectVoice('{voice}'); }} catch {{}} }}
  [void]$s.Speak([Console]::In.ReadToEnd());
}} finally {{ $s.Dispose(); }}
"#,
    vol = vol,
    rate = rate.clamp(-10, 10),
    voice = v_escaped,
  );
  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if let Some(stdin) = child.stdin.as_mut() { stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?; }
  drop(child.stdin.take());
  let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
  if !status.success() { return Err(format!("powershell exited with status: {status}")); }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn local_speak_blocking(_text: String, _voice: String, _rate: i32, _vol: u8) -> Result<(), String> {
  Err("TTS not implemented on this platform".into())
}

#[cfg(target_os = "windows")]
pub fn local_tts_synthesize_wav(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  let v = voice.unwrap_or_default();
  let v_escaped = ps_escape_single_quoted(&v);
  let r = rate.unwrap_or(-2).clamp(-10, 10);
  let vol = volume.unwrap_or(100).min(100);
  let file_name = format!("aidc_tts_{}.wav", chrono::Local::now().format("%Y%m%d_%H%M%S"));
  let mut path = std::env::temp_dir();
  path.push(file_name);
  let target = path.to_string_lossy().to_string();
  let ps = format!(
    r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
try {{
  $s.Volume = {vol};
  $s.Rate = {r};
  if ('{voice}' -ne '') {{ try {{ $s.SelectVoice('{voice}'); }} catch {{}} }}
  $s.SetOutputToWaveFile('{target}');
  [void]$s.Speak([Console]::In.ReadToEnd());
  $s.SetOutputToDefaultAudioDevice();
}} finally {{ $s.Dispose(); }}
"#,
    vol = vol, r = r, voice = v_escaped, target = target.replace('\\', "\\\\"),
  );
  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if let Some(stdin) = child.stdin.as_mut() { stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?; }
  drop(child.stdin.take());
  let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
  if !status.success() { return Err(format!("powershell exited with status: {status}")); }
  Ok(target)
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_synthesize_wav(_text: String, _voice: Option<String>, _rate: Option<i32>, _volume: Option<u8>) -> Result<String, String> {
  Err("TTS not implemented on this platform".into())
}

// ---------------------------
// OpenAI TTS synth helpers
// ---------------------------

pub async fn openai_synthesize_file(
  key: String,
  text: String,
  voice: Option<String>,
  model: Option<String>,
  format: Option<String>,
  rate: Option<i32>,
  volume: Option<u8>,
) -> Result<String, String> {
  let fmt_in = format.unwrap_or_else(|| "wav".to_string());
  let (accept, body_format) = match fmt_in.as_str() { "mp3" => ("audio/mpeg", "mp3"), "opus" => ("audio/ogg", "opus"), _ => ("audio/wav", "wav") };
  let m = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let v = voice.unwrap_or_else(|| "alloy".to_string());
  let body = serde_json::json!({ "model": m, "input": text, "voice": v, "format": body_format });
  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/audio/speech")
    .bearer_auth(key)
    .header("Accept", accept)
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;
  if !resp.status().is_success() { let status = resp.status(); let body_text = resp.text().await.unwrap_or_default(); return Err(format!("OpenAI error: {status} {body_text}")); }
  let ct_hdr = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
  let ext = if ct_hdr.contains("wav") { "wav" } else if ct_hdr.contains("mpeg") || ct_hdr.contains("mp3") { "mp3" } else if ct_hdr.contains("ogg") { "ogg" } else if ct_hdr.contains("opus") { "opus" } else if fmt_in == "mp3" { "mp3" } else if fmt_in == "opus" { "opus" } else { "wav" };
  let file_name = format!("aidc_tts_{}_openai.{}", chrono::Local::now().format("%Y%m%d_%H%M%S"), ext);
  let mut path = std::env::temp_dir(); path.push(file_name); let target = path.to_string_lossy().to_string();
  let bytes = resp.bytes().await.map_err(|e| format!("bytes error: {e}"))?;
  if ext == "wav" { let r = rate.unwrap_or(0).clamp(-10, 10); let vol = volume.unwrap_or(100).min(100); write_pcm16_wav_from_any(&bytes, &target, r, vol)?; } else { std::fs::write(&target, &bytes).map_err(|e| format!("write failed: {e}"))?; }
  Ok(target)
}

pub async fn openai_synthesize_wav(key: String, text: String, voice: Option<String>, model: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  openai_synthesize_file(key, text, voice, model, Some("wav".to_string()), rate, volume).await
}

/// Spawn an async task to stream audio chunks from OpenAI audio/speech endpoint and emit tts:stream:* events.
pub fn spawn_speech_stream(
  app: tauri::AppHandle,
  key: String,
  body: serde_json::Value,
  accept: &'static str,
  mime: &'static str,
  id: u64,
  mut rx: tokio::sync::oneshot::Receiver<()>,
  on_remove: impl FnOnce(u64) + Send + 'static,
) {
  tauri::async_runtime::spawn(async move {
    let client = reqwest::Client::new();
    let resp_res = client
      .post("https://api.openai.com/v1/audio/speech")
      .bearer_auth(key)
      .header("Accept", accept)
      .json(&body)
      .send()
      .await;

    let app2 = app.clone();
    let emit_err = |msg: String| { let _ = app2.emit("tts:stream:error", serde_json::json!({ "id": id, "message": msg })); };

    let resp = match resp_res {
      Ok(r) => r,
      Err(e) => { emit_err(format!("request failed: {e}")); on_remove(id); return; }
    };

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      emit_err(format!("OpenAI error: {status} {body_text}"));
      on_remove(id);
      return;
    }

    let _ = app.emit("tts:stream:start", serde_json::json!({ "id": id, "mime": mime }));

    let mut stream = resp.bytes_stream();
    loop {
      tokio::select! {
        _ = &mut rx => {
          let _ = app.emit("tts:stream:cancelled", serde_json::json!({ "id": id }));
          break;
        }
        next = stream.next() => {
          match next {
            Some(Ok(chunk)) => {
              let b64 = base64::engine::general_purpose::STANDARD.encode(&chunk);
              let _ = app.emit("tts:stream:chunk", serde_json::json!({ "id": id, "data": b64 }));
            }
            Some(Err(e)) => { emit_err(format!("stream error: {e}")); break; }
            None => { let _ = app.emit("tts:stream:end", serde_json::json!({ "id": id })); break; }
          }
        }
      }
    }

    on_remove(id);
  });
}

// ---------------------------
// Temp WAV cleanup (OpenAI TTS)
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

/// Spawn an async task that calls OpenAI Responses SSE endpoint for audio and emits tts:stream:* events.
/// The caller is responsible for generating a unique id and providing a cancellation receiver.
pub fn spawn_responses_stream(
  app: tauri::AppHandle,
  key: String,
  body: serde_json::Value,
  fmt: String,
  id: u64,
  mut rx: tokio::sync::oneshot::Receiver<()>,
  on_remove: impl FnOnce(u64) + Send + 'static,
) {
  tauri::async_runtime::spawn(async move {
    let client = reqwest::Client::new();
    let resp_res = client
      .post("https://api.openai.com/v1/responses")
      .bearer_auth(key)
      .header("Accept", "text/event-stream")
      .json(&body)
      .send()
      .await;

    let app2 = app.clone();
    let emit_err = |msg: String| {
      let _ = app2.emit("tts:stream:error", serde_json::json!({ "id": id, "message": msg }));
    };

    let resp = match resp_res {
      Ok(r) => r,
      Err(e) => { emit_err(format!("request failed: {e}")); on_remove(id); return; }
    };

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      emit_err(format!("OpenAI error: {status} {body_text}"));
      on_remove(id);
      return;
    }

    // Decide MIME for frontend MSE based on requested fmt
    let mime = match fmt.as_str() {
      "mp3" => "audio/mpeg",
      "wav" => "audio/wav",
      // Prefer Opus; if WebM is required later we can switch here
      _ => "audio/ogg; codecs=opus",
    };
    let _ = app.emit("tts:stream:start", serde_json::json!({ "id": id, "mime": mime }));

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
      tokio::select! {
        _ = &mut rx => {
          let _ = app.emit("tts:stream:cancelled", serde_json::json!({ "id": id }));
          break;
        }
        next = stream.next() => {
          match next {
            Some(Ok(chunk)) => {
              buf.extend_from_slice(&chunk);
              // Process complete SSE events separated by double newlines
              loop {
                if let Some(pos) = find_sse_event_boundary(&buf) {
                  let ev_bytes = buf.drain(..pos).collect::<Vec<u8>>();
                  // Remove potential trailing newlines
                  let _ = consume_leading_newlines(&mut buf);
                  if let Some(data_json) = extract_sse_data(&ev_bytes) {
                    if data_json.trim() == "[DONE]" { let _ = app.emit("tts:stream:end", serde_json::json!({ "id": id })); break; }
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_json) {
                      let typ = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                      if typ == "response.output_audio.delta" {
                        // New audio delta chunk
                        let b64 = val.get("delta").and_then(|v| v.as_str())
                          .or_else(|| val.get("audio").and_then(|v| v.as_str()))
                          .unwrap_or("");
                        if !b64.is_empty() {
                          let _ = app.emit("tts:stream:chunk", serde_json::json!({ "id": id, "data": b64 }));
                        }
                      } else if typ == "response.completed" {
                        let _ = app.emit("tts:stream:end", serde_json::json!({ "id": id }));
                        break;
                      }
                    }
                  }
                } else { break; }
              }
            }
            Some(Err(e)) => { emit_err(format!("stream error: {e}")); break; }
            None => { let _ = app.emit("tts:stream:end", serde_json::json!({ "id": id })); break; }
          }
        }
      }
    }

    on_remove(id);
  });
}

// --- SSE parsing helpers (used by Responses streaming) ---

/// Find the boundary position between SSE events separated by two newlines.
pub fn find_sse_event_boundary(buf: &[u8]) -> Option<usize> {
  // SSE events are separated by two newlines (\n\n). Handle \r\n as well.
  for i in 0..buf.len().saturating_sub(1) {
    if buf[i] == b'\n' && buf[i + 1] == b'\n' {
      return Some(i + 2);
    }
    if i + 3 < buf.len()
      && buf[i] == b'\r'
      && buf[i + 1] == b'\n'
      && buf[i + 2] == b'\r'
      && buf[i + 3] == b'\n'
    {
      return Some(i + 4);
    }
  }
  None
}

/// Consume leading newlines from the buffer and return the count consumed.
pub fn consume_leading_newlines(buf: &mut Vec<u8>) -> usize {
  let mut n = 0;
  while n < buf.len() && (buf[n] == b'\n' || buf[n] == b'\r') {
    n += 1;
  }
  if n > 0 {
    let _ = buf.drain(..n);
  }
  n
}

/// Extract the last data line (prefix "data:") from an SSE event bytes block.
pub fn extract_sse_data(ev_bytes: &[u8]) -> Option<String> {
  let text = String::from_utf8_lossy(ev_bytes);
  let mut data: Option<String> = None;
  for line in text.lines() {
    let line = line.trim_start();
    if let Some(rest) = line.strip_prefix("data:") {
      data = Some(rest.trim_start().to_string());
    }
  }
  data
}

/// Apply simple gain (volume) and playback rate (by adjusting the sample rate header) to a WAV buffer.
/// Note: rate adjustment will change pitch (no time-stretch). If processing fails, the caller should fall back.
pub fn apply_wav_gain_and_rate(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
  let mut reader = hound::WavReader::new(Cursor::new(bytes))
    .map_err(|e| format!("wav decode failed: {e}"))?;
  let in_spec = reader.spec();

  // Target: always 16-bit PCM for System.Media.SoundPlayer compatibility
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
