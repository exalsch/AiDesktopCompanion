use std::time::Duration;

use base64::Engine;
use futures_util::StreamExt;
use once_cell::sync::Lazy as OnceLazy;
use once_cell::sync::Lazy as GlobalLazy;
use serde_json;
use tauri::Emitter;
use tokio::sync::oneshot;
use crate::tts_utils::{
  write_pcm16_wav_from_any,
  find_sse_event_boundary,
  consume_leading_newlines,
  extract_sse_data,
};

use std::collections::HashMap;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::tts_streaming_server::TtsStreamingServer;

// Audio decode helpers moved to tts_utils

// ---------------------------
// TTS Streaming Server state and helpers
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
// OpenAI direct streaming (speech) and Responses SSE
// ---------------------------

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
        _ = &mut rx => { let _ = app.emit("tts:stream:cancelled", serde_json::json!({ "id": id })); break; }
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

    let mime = match fmt.as_str() {
      "mp3" => "audio/mpeg",
      "wav" => "audio/wav",
      _ => "audio/ogg; codecs=opus",
    };
    let _ = app.emit("tts:stream:start", serde_json::json!({ "id": id, "mime": mime }));

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
      tokio::select! {
        _ = &mut rx => { let _ = app.emit("tts:stream:cancelled", serde_json::json!({ "id": id })); break; }
        next = stream.next() => {
          match next {
            Some(Ok(chunk)) => {
              buf.extend_from_slice(&chunk);
              loop {
                if let Some(pos) = find_sse_event_boundary(&buf) {
                  let ev_bytes = buf.drain(..pos).collect::<Vec<u8>>();
                  let _ = consume_leading_newlines(&mut buf);
                  if let Some(data_json) = extract_sse_data(&ev_bytes) {
                    if data_json.trim() == "[DONE]" { let _ = app.emit("tts:stream:end", serde_json::json!({ "id": id })); break; }
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_json) {
                      let typ = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                      if typ == "response.output_audio.delta" {
                        let b64 = val.get("delta").and_then(|v| v.as_str())
                          .or_else(|| val.get("audio").and_then(|v| v.as_str()))
                          .unwrap_or("");
                        if !b64.is_empty() { let _ = app.emit("tts:stream:chunk", serde_json::json!({ "id": id, "data": b64 })); }
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

// SSE helpers moved to tts_utils

// ---------------------------
// OpenAI synth helpers (file and wav)
// ---------------------------

pub async fn openai_synthesize_file(
  key: String,
  text: String,
  voice: Option<String>,
  model: Option<String>,
  format: Option<String>,
  rate: Option<i32>,
  volume: Option<u8>,
  instructions: Option<String>,
) -> Result<String, String> {
  let fmt_in = format.unwrap_or_else(|| "wav".to_string());
  let (accept, body_format) = match fmt_in.as_str() { "mp3" => ("audio/mpeg", "mp3"), "opus" => ("audio/ogg", "opus"), _ => ("audio/wav", "wav") };
  let m = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let v = voice.unwrap_or_else(|| "alloy".to_string());
  // Build JSON body; include instructions if provided & non-empty
  let mut body_obj = serde_json::Map::new();
  body_obj.insert("model".to_string(), serde_json::Value::String(m));
  body_obj.insert("input".to_string(), serde_json::Value::String(text));
  body_obj.insert("voice".to_string(), serde_json::Value::String(v));
  body_obj.insert("format".to_string(), serde_json::Value::String(body_format.to_string()));
  if let Some(instr) = instructions {
    if !instr.trim().is_empty() {
      body_obj.insert("instructions".to_string(), serde_json::Value::String(instr));
    }
  }
  let body = serde_json::Value::Object(body_obj);
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
  openai_synthesize_file(key, text, voice, model, Some("wav".to_string()), rate, volume, None).await
}

// Temp file cleanup moved to tts_utils
