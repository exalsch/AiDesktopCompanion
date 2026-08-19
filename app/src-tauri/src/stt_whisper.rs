use std::fs;
use std::io::Write;
use std::path::PathBuf;

use once_cell::sync::Lazy;
use reqwest;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use tauri::Emitter;
#[cfg(feature = "local-stt")]
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

static DEFAULT_MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin";

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
  reqwest::Client::builder()
    .connect_timeout(std::time::Duration::from_secs(30))
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
});

pub(crate) fn models_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("models");
      p.push("whisper");
      return Some(p);
    }
    None
  }
  #[cfg(not(target_os = "windows"))]
  {
    if let Ok(home) = std::env::var("HOME") {
      let mut p = PathBuf::from(home);
      p.push(".cache");
      p.push("AiDesktopCompanion");
      p.push("models");
      p.push("whisper");
      return Some(p);
    }
    None
  }
}

pub(crate) fn file_name_from_url(url: &str) -> String {
  let u = url.split('?').next().unwrap_or(url);
  u.split('/').last().filter(|s| !s.is_empty()).unwrap_or("ggml-base.bin").to_string()
}

pub fn local_model_status(url: String) -> Result<(bool, String, Vec<String>), String> {
  let dir = models_dir().ok_or_else(|| "Unsupported platform for model path".to_string())?;
  let file_name = file_name_from_url(&url);
  let path = dir.join(&file_name);
  let p = path.to_string_lossy().to_string();
  if !path.exists() {
    return Ok((false, p, vec![file_name]));
  }
  if let Ok(md) = fs::metadata(&path) {
    if md.len() > 10 * 1024 * 1024 {
      return Ok((true, p, Vec::new()));
    }
  }
  Ok((false, p, vec![file_name]))
}

async fn ensure_model_file() -> Result<PathBuf, String> {
  let dir = models_dir().ok_or_else(|| "Unsupported platform for model path".to_string())?;
  if !dir.exists() { fs::create_dir_all(&dir).map_err(|e| format!("create model dir failed: {e}"))?; }
  // Determine model URL from settings, env, or default.
  let url = {
    let v = crate::config::load_settings_json();
    if let Some(s) = v.get("stt_whisper_model_url").and_then(|x| x.as_str()) {
      if !s.trim().is_empty() { s.trim().to_string() } else { std::env::var("AIDC_WHISPER_MODEL_URL").unwrap_or_else(|_| DEFAULT_MODEL_URL.to_string()) }
    } else {
      std::env::var("AIDC_WHISPER_MODEL_URL").unwrap_or_else(|_| DEFAULT_MODEL_URL.to_string())
    }
  };
  let file_name = file_name_from_url(&url);
  let mut path = dir.clone();
  path.push(&file_name);
  if path.exists() {
    // Basic sanity: file size > 10MB
    if let Ok(md) = fs::metadata(&path) {
      if md.len() > 10 * 1024 * 1024 { return Ok(path); }
    }
  }
  // Download into temp then rename
  let mut tmp = dir.clone();
  tmp.push(format!("{}.part", file_name));
  let resp = CLIENT.get(&url).send().await.map_err(|e| format!("download failed: {e}"))?;
  if !resp.status().is_success() { return Err(format!("download error: {}", resp.status())); }
  let bytes = resp.bytes().await.map_err(|e| format!("download bytes failed: {e}"))?;
  if bytes.len() < 10 * 1024 * 1024 { return Err("downloaded model too small".into()); }
  let mut f = fs::File::create(&tmp).map_err(|e| format!("write tmp failed: {e}"))?;
  f.write_all(&bytes).map_err(|e| format!("write tmp failed: {e}"))?;
  drop(f);
  #[cfg(target_os = "windows")]
  { if path.exists() { let _ = fs::remove_file(&path); } }
  fs::rename(&tmp, &path).map_err(|e| format!("rename model failed: {e}"))?;
  Ok(path)
}

// Prefetch helper with progress events. Emits `stt-model-download` events with JSON: { kind: "progress", received, total } and { kind: "done", path }
pub async fn prefetch_model_with_progress(app: tauri::AppHandle, url_opt: Option<String>) -> Result<String, String> {
  let dir = models_dir().ok_or_else(|| "Unsupported platform for model path".to_string())?;
  if !dir.exists() { fs::create_dir_all(&dir).map_err(|e| format!("create model dir failed: {e}"))?; }
  let url = url_opt
    .and_then(|s| if s.trim().is_empty() { None } else { Some(s.trim().to_string()) })
    .or_else(|| {
      let v = crate::config::load_settings_json();
      v.get("stt_whisper_model_url").and_then(|x| x.as_str()).map(|s| s.to_string())
    })
    .unwrap_or_else(|| std::env::var("AIDC_WHISPER_MODEL_URL").unwrap_or_else(|_| DEFAULT_MODEL_URL.to_string()));

  let file_name = file_name_from_url(&url);
  let mut path = dir.clone();
  path.push(&file_name);
  if path.exists() {
    if let Ok(md) = fs::metadata(&path) { if md.len() > 10 * 1024 * 1024 { return Ok(path.to_string_lossy().to_string()); } }
  }
  let mut tmp = dir.clone();
  tmp.push(format!("{}.part", file_name));

  let resp = CLIENT.get(&url).send().await.map_err(|e| format!("download failed: {e}"))?;
  if !resp.status().is_success() { return Err(format!("download error: {}", resp.status())); }
  let total = resp.content_length().unwrap_or(0);
  let mut stream = resp.bytes_stream();
  let mut f = fs::File::create(&tmp).map_err(|e| format!("write tmp failed: {e}"))?;
  let mut received: u64 = 0;
  use futures_util::StreamExt;
  while let Some(chunk) = stream.next().await {
    let bytes = chunk.map_err(|e| format!("download chunk failed: {e}"))?;
    f.write_all(&bytes).map_err(|e| format!("write failed: {e}"))?;
    received += bytes.len() as u64;
    let _ = app.emit("stt-model-download", serde_json::json!({"kind":"progress","received":received,"total":total}));
  }
  drop(f);
  #[cfg(target_os = "windows")]
  { if path.exists() { let _ = fs::remove_file(&path); } }
  fs::rename(&tmp, &path).map_err(|e| format!("rename model failed: {e}"))?;
  let p = path.to_string_lossy().to_string();
  let _ = app.emit("stt-model-download", serde_json::json!({"kind":"done","path":p}));
  Ok(p)
}

pub(crate) fn decode_to_f32_mono_16k(audio: &[u8], _mime: &str) -> Result<Vec<f32>, String> {
  // Decode container using Symphonia to interleaved f32 and track sample rate/channels
  let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(audio.to_vec())), Default::default());
  let hint = Hint::new();
  let mut format = symphonia::default::get_probe()
    .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
    .map_err(|e| format!("audio probe failed: {e}"))?;

  let track = format
    .default_track(TrackType::Audio)
    .ok_or_else(|| "no default track".to_string())?;
  let track_id = track.id;
  let audio_params = track
    .codec_params
    .as_ref()
    .and_then(|p| p.audio())
    .ok_or_else(|| "track has no audio codec parameters".to_string())?
    .clone();
  let mut decoder = symphonia::default::get_codecs()
    .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
    .map_err(|e| format!("decoder init failed: {e}"))?;

  let mut src_rate: u32 = audio_params.sample_rate.unwrap_or(16000);
  let mut channels: usize = audio_params.channels.as_ref().map(|c| c.count()).unwrap_or(1);
  let mut pcm: Vec<f32> = Vec::new();
  // Reused across packets: copy_to_vec_interleaved resizes its destination
  // rather than appending, so it needs a scratch buffer of its own.
  let mut frame: Vec<f32> = Vec::new();

  // 0.6 hands back a generic buffer that converts to f32 itself, so the
  // per-sample-format match this used to carry is gone along with the
  // hand-rolled i16/i32/u8 scaling.
  while let Ok(Some(packet)) = format.next_packet() {
    if packet.track_id != track_id { continue; }
    if let Ok(buf) = decoder.decode(&packet) {
      let spec = buf.spec();
      src_rate = spec.rate();
      channels = spec.channels().count();
      buf.copy_to_vec_interleaved(&mut frame);
      pcm.extend_from_slice(&frame);
    }
  }

  if pcm.is_empty() { return Err("decode produced no samples".into()); }
  if channels == 0 { channels = 1; }

  // Downmix to mono
  let mut mono: Vec<f32> = Vec::with_capacity(pcm.len() / channels.max(1));
  if channels == 1 {
    mono = pcm;
  } else {
    let mut i = 0usize;
    while i + channels <= pcm.len() {
      let mut sum = 0.0f32;
      for c in 0..channels { sum += pcm[i + c]; }
      mono.push(sum / (channels as f32));
      i += channels;
    }
  }

  // Resample to 16k using simple linear interpolation
  let src_len = mono.len();
  if src_rate == 16000 || src_len == 0 {
    return Ok(mono);
  }
  let ratio = 16000.0f32 / (src_rate as f32);
  let out_len = ((src_len as f32) * ratio).round() as usize;
  let mut out = Vec::with_capacity(out_len);
  for n in 0..out_len {
    let t = (n as f32) / ratio;
    let i0 = t.floor() as usize;
    let i1 = (i0 + 1).min(src_len - 1);
    let frac = t - (i0 as f32);
    let s = mono[i0] * (1.0 - frac) + mono[i1] * frac;
    out.push(s);
  }
  Ok(out)
}

#[cfg(feature = "local-stt")]
pub async fn transcribe_local(audio: Vec<u8>, mime: String) -> Result<String, String> {
  let model_path = ensure_model_file().await?;
  // Safety: whisper-rs expects 16k mono f32 PCM samples in [-1,1]
  let pcm = decode_to_f32_mono_16k(&audio, &mime)?;

  let n_threads = std::cmp::max(1, num_cpus::get() as i32 - 1);

  let ctx = WhisperContext::new_with_params(
    model_path.to_string_lossy().as_ref(),
    WhisperContextParameters::default(),
  ).map_err(|e| format!("whisper init failed: {e}"))?;

  let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
  params.set_n_threads(n_threads);
  params.set_translate(false);
  // Auto language detection
  params.set_language(Some("auto"));
  // Silence noisy console output
  params.set_print_progress(false);
  params.set_print_special(false);
  params.set_print_realtime(false);

  let mut state = ctx.create_state().map_err(|e| format!("whisper state create failed: {e}"))?;
  state.full(params, &pcm).map_err(|e| format!("whisper full failed: {e}"))?;

  let num_segments = state.full_n_segments();
  let mut out = String::new();
  for i in 0..num_segments {
    if let Some(seg) = state.get_segment(i) {
      if let Ok(text) = seg.to_str() { out.push_str(text); }
    }
  }
  Ok(out.trim().to_string())
}

#[cfg(not(feature = "local-stt"))]
pub async fn transcribe_local(_audio: Vec<u8>, _mime: String) -> Result<String, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}
