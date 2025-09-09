#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // Initialize TTS streaming server on startup
  tauri::async_runtime::spawn(async {
    if let Err(e) = init_tts_streaming_server().await {
      eprintln!("Failed to initialize TTS streaming server: {}", e);
    }
  });

  tauri::Builder::default()
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_dialog::init())
    .on_window_event(|window, event| {
      if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        // Close-to-tray: prevent app exit and hide the main window
        if window.label() == "main" {
          api.prevent_close();
          let _ = window.hide();
        }
      }
    })
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      // System tray: build a minimal menu and icon
      // Menu items: Show (shows and focuses main window) and Exit (quits app)
      let show_item = MenuItemBuilder::with_id("show", "Show").build(app)?;
      let exit_item = MenuItemBuilder::with_id("exit", "Exit").build(app)?;
      let tray_menu = MenuBuilder::new(app)
        .items(&[&show_item, &exit_item])
        .build()?;

      let mut tray_builder = TrayIconBuilder::new()
        .menu(&tray_menu)
        .tooltip("AiDesktopCompanion")
        .on_tray_icon_event(|tray, event| {
          if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
          } = event
          {
            let app = tray.app_handle();
            if let Some(window) = app.get_webview_window("main") {
              let _ = window.unminimize();
              let _ = window.show();
              let _ = window.set_focus();
            }
          }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
          "show" => {
            if let Some(window) = app.get_webview_window("main") {
              let _ = window.unminimize();
              let _ = window.show();
              let _ = window.set_focus();
            }
          }
          "exit" => {
            app.exit(0);
          }
          _ => {}
        });

      if let Some(icon) = app.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
      }
      let _tray = tray_builder.build(app)?;
      // Ensure default quick_prompts.json exists on first run to avoid errors when loading quick prompts
      if let Some(p) = quick_prompts_config_path() {
        if !p.exists() {
          let _ = generate_default_quick_prompts();
        }
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      prompt_action,
      position_quick_actions,
      tts_selection,
      tts_open_with_selection,
      open_tts_with_text,
      tts_start,
      tts_stop,
      tts_list_voices,
      tts_synthesize_wav,
      tts_openai_synthesize_wav,
      tts_openai_synthesize_file,
      tts_openai_stream_start,
      tts_openai_stream_stop,
      tts_openai_responses_stream_start,
      tts_create_stream_session,
      tts_stop_stream_session,
      tts_stream_session_count,
      tts_stream_cleanup_idle,
      stt_transcribe,
      chat_complete,
      insert_text_into_focused_app,
      insert_prompt_text,
      open_prompt_with_text,
      run_quick_prompt,
      generate_default_quick_prompts,
      get_quick_prompts,
      save_quick_prompts,
      get_settings,
      save_settings,
      list_openai_models,
      load_conversation_state,
      save_conversation_state,
      clear_conversations,
      copy_file_to_path,
      tts_delete_temp_wav,
      cleanup_stale_tts_wavs,
      get_virtual_screen_bounds,
      size_overlay_to_virtual_screen,
      capture_region,
      mcp_connect,
      mcp_disconnect,
      mcp_list_tools,
      mcp_call_tool,
      mcp_list_resources,
      mcp_read_resource,
      mcp_list_prompts,
      mcp_get_prompt,
      mcp_ping
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

use std::{thread, time::Duration};
use std::fs;
use std::time::SystemTime;
use std::path::PathBuf;
use tauri::Manager; // bring get_webview_window into scope
use tauri::Emitter; // bring emit into scope
use tauri::PhysicalPosition; // for window positioning
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use serde::Deserialize;
use std::io::{Write, Cursor};
use std::process::{Command, Stdio};
use once_cell::sync::Lazy;
use tokio::sync::oneshot;
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::Mutex as AsyncMutex;

mod tts_streaming_server;
mod config;
mod quick_prompts;
mod mcp;
use tts_streaming_server::TtsStreamingServer;
use rmcp::{
  service::{RoleClient, DynService, RunningService},
};
use rmcp::model::CallToolRequestParam;
use base64::Engine; // for .encode on base64 engines
// Audio decoding (fallback for non-WAV responses)
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use arboard::Clipboard;
use enigo::{Enigo, Key, KeyboardControllable};

// Decode arbitrary audio bytes (e.g., WAV/MP3/AAC) and write a 16-bit PCM WAV.
// If the buffer is already WAV, we try hound first; otherwise, we fall back to Symphonia.
fn write_pcm16_wav_from_any(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
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
// Settings helpers and commands
// ---------------------------

// settings_config_path wrapper removed; use config::settings_config_path() directly where needed

/// Start streaming using OpenAI Responses API with SSE, emitting the same tts:stream:* events.
#[tauri::command]
async fn tts_openai_responses_stream_start(app: tauri::AppHandle, text: String, voice: Option<String>, model: Option<String>, format: Option<String>) -> Result<u64, String> {
  let key = get_api_key_from_settings_or_env()?;
  let fmt = format.unwrap_or_else(|| "opus".to_string());
  let req_model = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  // Responses API requires a compatible model for audio output; override common TTS-only models
  let m = if req_model.contains("tts") { "gpt-4o-realtime-preview".to_string() } else { req_model };
  let v = voice.unwrap_or_else(|| "alloy".to_string());

  let body = serde_json::json!({
    "model": m,
    "modalities": ["text", "audio"],
    "audio": { "voice": v, "format": fmt },
    "input": text,
    "stream": true
  });

  let (tx, mut rx) = oneshot::channel::<()>();
  let id = STREAM_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
  {
    let mut map = STREAM_STOPPERS.lock().unwrap();
    map.insert(id, tx);
  }

  let app2 = app.clone();
  tauri::async_runtime::spawn(async move {
    let client = reqwest::Client::new();
    let resp_res = client
      .post("https://api.openai.com/v1/responses")
      .bearer_auth(key)
      .header("Accept", "text/event-stream")
      .json(&body)
      .send()
      .await;

    let emit_err = |msg: String| {
      let _ = app2.emit("tts:stream:error", serde_json::json!({ "id": id, "message": msg }));
    };

    let resp = match resp_res {
      Ok(r) => r,
      Err(e) => { emit_err(format!("request failed: {e}"));
        let mut map = STREAM_STOPPERS.lock().unwrap(); map.remove(&id); return; }
    };

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      emit_err(format!("OpenAI error: {status} {body_text}"));
      let mut map = STREAM_STOPPERS.lock().unwrap(); map.remove(&id);
      return;
    }

    // Decide MIME for frontend MSE based on requested fmt
    let mime = match fmt.as_str() {
      "mp3" => "audio/mpeg",
      "wav" => "audio/wav",
      // Prefer Opus; if WebM is required later we can switch here
      _ => "audio/ogg; codecs=opus",
    };
    let _ = app2.emit("tts:stream:start", serde_json::json!({ "id": id, "mime": mime }));

    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    loop {
      tokio::select! {
        _ = &mut rx => {
          let _ = app2.emit("tts:stream:cancelled", serde_json::json!({ "id": id }));
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
                    if data_json.trim() == "[DONE]" { let _ = app2.emit("tts:stream:end", serde_json::json!({ "id": id })); break; }
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data_json) {
                      let typ = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                      if typ == "response.output_audio.delta" {
                        // New audio delta chunk
                        let b64 = val.get("delta").and_then(|v| v.as_str())
                          .or_else(|| val.get("audio").and_then(|v| v.as_str()))
                          .unwrap_or("");
                        if !b64.is_empty() {
                          let _ = app2.emit("tts:stream:chunk", serde_json::json!({ "id": id, "data": b64 }));
                        }
                      } else if typ == "response.completed" {
                        let _ = app2.emit("tts:stream:end", serde_json::json!({ "id": id }));
                        break;
                      }
                    }
                  }
                } else { break; }
              }
            }
            Some(Err(e)) => { emit_err(format!("stream error: {e}")); break; }
            None => { let _ = app2.emit("tts:stream:end", serde_json::json!({ "id": id })); break; }
          }
        }
      }
    }

    let mut map = STREAM_STOPPERS.lock().unwrap();
    map.remove(&id);
  });

  Ok(id)
}

// Helpers to parse SSE lines from a raw byte buffer
fn find_sse_event_boundary(buf: &[u8]) -> Option<usize> {
  // SSE events are separated by two newlines (\n\n). Handle \r\n as well.
  for i in 0..buf.len().saturating_sub(1) {
    if buf[i] == b'\n' && buf[i+1] == b'\n' { return Some(i+2); }
    if i+3 < buf.len() && buf[i] == b'\r' && buf[i+1] == b'\n' && buf[i+2] == b'\r' && buf[i+3] == b'\n' { return Some(i+4); }
  }
  None
}

fn consume_leading_newlines(buf: &mut Vec<u8>) -> usize {
  let mut n = 0;
  while n < buf.len() && (buf[n] == b'\n' || buf[n] == b'\r') { n += 1; }
  if n > 0 { let _ = buf.drain(..n); }
  n
}

fn extract_sse_data(ev_bytes: &[u8]) -> Option<String> {
  // Find the last 'data: ' line and return its content
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

/// Initialize TTS streaming server on app startup
async fn init_tts_streaming_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let server = TtsStreamingServer::new().await?;
  let mut server_guard = TTS_STREAMING_SERVER.lock().unwrap();
  *server_guard = Some(server);
  Ok(())
}

/// Create a new TTS streaming session and return the stream URL
#[tauri::command]
async fn tts_create_stream_session(text: String, voice: Option<String>, model: Option<String>, format: Option<String>, instructions: Option<String>) -> Result<String, String> {
  let api_key = get_api_key_from_settings_or_env()?;
  let voice = voice.unwrap_or_else(|| "alloy".to_string());
  let model = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let format = format.unwrap_or_else(|| "mp3".to_string()); // Default to MP3 for best compatibility
  
  // Ensure streaming server is initialized
  {
    let need_init = {
      let server_guard = TTS_STREAMING_SERVER.lock().unwrap();
      server_guard.is_none()
    };
    if need_init {
      init_tts_streaming_server().await.map_err(|e| format!("failed to init streaming server: {}", e))?;
    }
  }
  
  let server_guard = TTS_STREAMING_SERVER.lock().unwrap();
  let server = server_guard.as_ref().unwrap();
  
  let session_id = server.create_session(text, voice, model, format, api_key, instructions);
  let stream_url = server.get_stream_url(&session_id);
  
  Ok(stream_url)
}

/// Stop a TTS streaming session
#[tauri::command]
fn tts_stop_stream_session(session_id: String) -> Result<bool, String> {
  let server_guard = TTS_STREAMING_SERVER.lock().unwrap();
  if let Some(server) = server_guard.as_ref() {
    Ok(server.stop_session(&session_id))
  } else {
    Err("TTS streaming server not available".to_string())
  }
}

/// QA: get active TTS streaming sessions count
#[tauri::command]
fn tts_stream_session_count() -> Result<usize, String> {
  let server_guard = TTS_STREAMING_SERVER.lock().unwrap();
  if let Some(server) = server_guard.as_ref() {
    Ok(server.count_sessions())
  } else {
    Ok(0)
  }
}

/// QA: cleanup idle TTS sessions older than ttl_seconds (that have not started)
#[tauri::command]
fn tts_stream_cleanup_idle(ttl_seconds: u64) -> Result<usize, String> {
  let server_guard = TTS_STREAMING_SERVER.lock().unwrap();
  if let Some(server) = server_guard.as_ref() {
    let removed = server.cleanup_idle(std::time::Duration::from_secs(ttl_seconds));
    Ok(removed)
  } else {
    Ok(0)
  }
}

// Path for persisted conversation state (single-thread for now)
// conversation_state_path and persist_conversations_enabled wrappers removed; use config:: directly

// ---------------------------
// Conversation persistence commands
// ---------------------------
#[tauri::command]
fn load_conversation_state() -> Result<serde_json::Value, String> { config::load_conversation_state() }

#[tauri::command]
fn save_conversation_state(state: serde_json::Value) -> Result<String, String> { config::save_conversation_state(state) }

#[tauri::command]
fn clear_conversations() -> Result<String, String> { config::clear_conversations() }

// ---------------------------
// MCP Tools — rmcp integration
// ... (rest of the code remains the same)

static MCP_CLIENTS: Lazy<AsyncMutex<std::collections::HashMap<String, Arc<RunningService<RoleClient, Box<dyn DynService<RoleClient>>>>>>> = Lazy::new(|| {
  AsyncMutex::new(std::collections::HashMap::new())
});

// resolve_windows_program moved to mcp.rs

#[tauri::command]
async fn mcp_connect(
  app: tauri::AppHandle,
  server_id: String,
  command: String,
  args: Vec<String>,
  cwd: Option<String>,
  env: Option<serde_json::Value>,
  transport: Option<String>,
) -> Result<String, String> {
  mcp::connect(&app, &MCP_CLIENTS, server_id, command, args, cwd, env, transport).await
}

#[tauri::command]
async fn mcp_disconnect(app: tauri::AppHandle, server_id: String) -> Result<String, String> {
  mcp::disconnect(&app, &MCP_CLIENTS, server_id).await
}

#[tauri::command]
async fn mcp_list_tools(server_id: String) -> Result<serde_json::Value, String> {
  mcp::list_tools(&MCP_CLIENTS, &server_id).await
}

#[tauri::command]
async fn mcp_call_tool(server_id: String, name: String, args: serde_json::Value) -> Result<serde_json::Value, String> {
  mcp::call_tool(&MCP_CLIENTS, &server_id, &name, args).await
}

#[tauri::command]
async fn mcp_list_resources(server_id: String) -> Result<serde_json::Value, String> {
  mcp::list_resources(&MCP_CLIENTS, &server_id).await
}

#[tauri::command]
async fn mcp_read_resource(server_id: String, uri: String) -> Result<serde_json::Value, String> {
  mcp::read_resource(&MCP_CLIENTS, &server_id, &uri).await
}

#[tauri::command]
async fn mcp_list_prompts(server_id: String) -> Result<serde_json::Value, String> {
  mcp::list_prompts(&MCP_CLIENTS, &server_id).await
}

#[tauri::command]
async fn mcp_get_prompt(server_id: String, name: String, arguments: Option<serde_json::Value>) -> Result<serde_json::Value, String> {
  mcp::get_prompt(&MCP_CLIENTS, &server_id, &name, arguments).await
}

#[tauri::command]
async fn mcp_ping(server_id: String) -> Result<String, String> {
  mcp::ping(&MCP_CLIENTS, &server_id).await
}

fn load_settings_json() -> serde_json::Value { config::load_settings_json() }

// get_disabled_tools_map local helper removed; use config::get_disabled_tools_map()

fn get_api_key_from_settings_or_env() -> Result<String, String> { config::get_api_key_from_settings_or_env() }

fn get_model_from_settings_or_env() -> String { config::get_model_from_settings_or_env() }

fn get_temperature_from_settings_or_env() -> Option<f32> { config::get_temperature_from_settings_or_env() }

#[tauri::command]
fn get_settings() -> Result<serde_json::Value, String> {
  config::get_settings()
}

#[tauri::command]
fn save_settings(map: serde_json::Value) -> Result<String, String> {
  config::save_settings(map)
}

#[tauri::command]
async fn list_openai_models() -> Result<Vec<String>, String> {
  let key = get_api_key_from_settings_or_env()?;
  let client = reqwest::Client::new();
  let resp = client
    .get("https://api.openai.com/v1/models")
    .bearer_auth(key)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    return Err(format!("OpenAI error: {status} {body_text}"));
  }
  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  let mut ids: Vec<String> = v.get("data")
    .and_then(|d| d.as_array())
    .map(|arr| arr.iter()
      .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
      .filter(|id| id.starts_with("gpt-") || id.contains("gpt-4") || id.contains("gpt-4o"))
      .collect())
    .unwrap_or_else(|| Vec::new());
  ids.sort();
  ids.dedup();
  Ok(ids)
}

#[tauri::command]
fn generate_default_quick_prompts() -> Result<String, String> {
  quick_prompts::generate_default_quick_prompts()
}

#[tauri::command]
fn get_quick_prompts() -> Result<serde_json::Value, String> {
  quick_prompts::get_quick_prompts()
}

#[tauri::command]
fn save_quick_prompts(map: serde_json::Value) -> Result<String, String> {
  quick_prompts::save_quick_prompts(map)
}

#[tauri::command]
fn prompt_action(app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
  let safe = safe_mode.unwrap_or(false);

  // Prepare clipboard access
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;

  // Save current clipboard text (best-effort) when aggressive mode
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };

  // Simulate Ctrl+C to copy current selection (aggressive mode)
  if !safe {
    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('c'));
    enigo.key_up(Key::Control);
    // Allow some time for clipboard to update
    thread::sleep(Duration::from_millis(120));
  }

  // Read selection text (fallback to empty string)
  let selection = clipboard.get_text().unwrap_or_default();

  // Restore clipboard (best-effort) if we changed it
  if !safe {
    if let Some(prev) = previous_text {
      let _ = clipboard.set_text(prev);
    }
  }

  // Bring main window to front and emit event with selection details
  if let Some(win) = app.get_webview_window("main") {
    let _ = win.show();
    let _ = win.set_focus();
  }
  // Emit a direct-insert + new-conversation event (no preview UI)
  let payload = serde_json::json!({
    "text": selection,
  });
  let _ = app.emit("prompt:new-conversation", payload);
  Ok("ok".to_string())
}

// Positions the 'quick-actions' window near the current cursor position (Windows-first).
// On non-Windows platforms this is a no-op for now.
#[tauri::command]
fn position_quick_actions(app: tauri::AppHandle) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    unsafe {
      let mut pt = POINT { x: 0, y: 0 };
      if let Err(e) = GetCursorPos(&mut pt) {
        return Err(format!("GetCursorPos failed: {e}"));
      }

      // Small offset so we don't overlap the cursor
      let x = pt.x + 12;
      let y = pt.y + 12;

      if let Some(win) = app.get_webview_window("quick-actions") {
        let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
      }
      Ok(())
    }
  }
  #[cfg(not(target_os = "windows"))]
  {
    Ok(())
  }
}

// payload_length removed (unused)

// Return the Windows virtual desktop bounds (spanning all monitors).
// x/y can be negative if a monitor is to the left/top of the primary.
#[tauri::command]
fn get_virtual_screen_bounds() -> Result<serde_json::Value, String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
    use windows::Win32::UI::WindowsAndMessaging::{SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

    unsafe {
      let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
      let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
      let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
      let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
      if w <= 0 || h <= 0 {
        return Err("GetSystemMetrics returned invalid virtual screen size".into());
      }
      return Ok(serde_json::json!({
        "x": x,
        "y": y,
        "width": w,
        "height": h,
      }));
    }
  }
  #[cfg(not(target_os = "windows"))]
  {
    Err("get_virtual_screen_bounds not implemented on this platform".into())
  }
}

// Size and position the 'capture-overlay' window to span the full virtual desktop using physical coordinates.
#[tauri::command]
fn size_overlay_to_virtual_screen(app: tauri::AppHandle) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics;
    use windows::Win32::UI::WindowsAndMessaging::{SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

    unsafe {
      let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
      let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
      let w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
      let h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
      if w <= 0 || h <= 0 { return Err("GetSystemMetrics returned invalid virtual screen size".into()); }
      if let Some(win) = app.get_webview_window("capture-overlay") {
        let _ = win.set_fullscreen(false);
        let _ = win.set_decorations(false);
        let _ = win.set_always_on_top(true);
        let _ = win.set_resizable(true);
        // Position first, then size, to avoid intermediate clamping by the window manager
        let _ = win.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
        let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize { width: w as u32, height: h as u32 }));
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.set_resizable(false);
        Ok(())
      } else {
        Err("capture-overlay window not found".into())
      }
    }
  }
  #[cfg(not(target_os = "windows"))]
  {
    Err("size_overlay_to_virtual_screen not implemented on this platform".into())
  }
}

// Open the main window TTS panel with provided text and optional autoplay.
#[tauri::command]
fn open_tts_with_text(app: tauri::AppHandle, text: String, autoplay: Option<bool>) -> Result<(), String> {
  if let Some(win) = app.get_webview_window("main") {
    let _ = win.show();
    let _ = win.set_focus();
  }
  let payload = serde_json::json!({
    "text": text,
    "autoplay": autoplay.unwrap_or(false),
  });
  let _ = app.emit("tts:open", payload);
  Ok(())
}

// Capture current selection text and open the TTS panel, optionally starting playback.
#[tauri::command]
fn tts_open_with_selection(app: tauri::AppHandle, safe_mode: Option<bool>, autoplay: Option<bool>) -> Result<(), String> {
  let safe = safe_mode.unwrap_or(false);

  // Capture selection text (copy-restore pattern like prompt_action)
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };

  if !safe {
    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('c'));
    enigo.key_up(Key::Control);
    thread::sleep(Duration::from_millis(120));
  }

  let selection = clipboard.get_text().unwrap_or_default();

  if !safe {
    if let Some(prev) = previous_text {
      let _ = clipboard.set_text(prev);
    }
  }

  if selection.trim().is_empty() {
    let _ = app.emit("tts:error", serde_json::json!({ "message": "No text selected" }));
    return Err("No text selected".into());
  }

  open_tts_with_text(app, selection, autoplay)
}

#[tauri::command]
async fn tts_selection(app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
  let safe = safe_mode.unwrap_or(false);

  // Capture selection text similar to prompt_action
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };

  if !safe {
    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('c'));
    enigo.key_up(Key::Control);
    thread::sleep(Duration::from_millis(120));
  }

  let selection = clipboard.get_text().unwrap_or_default();

  if !safe {
    if let Some(prev) = previous_text {
      let _ = clipboard.set_text(prev);
    }
  }

  if selection.trim().is_empty() {
    let _ = app.emit("tts:error", serde_json::json!({
      "message": "No text selected"
    }));
    return Err("No text selected".into());
  }

  // Read user TTS settings
  let settings = load_settings_json();
  let engine = settings
    .get("tts_engine").and_then(|x| x.as_str()).unwrap_or("local");
  let rate = settings
    .get("tts_rate").and_then(|x| x.as_i64()).unwrap_or(-2).clamp(-10, 10) as i32;
  let vol = settings
    .get("tts_volume").and_then(|x| x.as_i64()).unwrap_or(100).clamp(0, 100) as u8;

  if engine == "openai" {
    // Use OpenAI TTS per settings, then play the WAV
    let voice = settings
      .get("tts_openai_voice").and_then(|x| x.as_str()).unwrap_or("alloy").to_string();
    let model = settings
      .get("tts_openai_model").and_then(|x| x.as_str()).unwrap_or("gpt-4o-mini-tts").to_string();

    let wav = tts_openai_synthesize_wav(selection.clone(), Some(voice), Some(model), Some(rate), Some(vol)).await?;

    // Play WAV synchronously via PowerShell SoundPlayer
    #[cfg(target_os = "windows")]
    {
      use std::process::Command;
      // Best-effort sanity check before playback
      match fs::metadata(&wav) {
        Ok(meta) => {
          if meta.len() < 44 { // smaller than typical WAV header
            let msg = format!("synthesized WAV too small: {} bytes at {}", meta.len(), &wav);
            let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
            return Err(msg);
          }
        }
        Err(e) => {
          let msg = format!("synthesized WAV not found: {} ({})", &wav, e);
          let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
          return Err(msg);
        }
      }
      // Use single-quoted PS string and escape any single quotes in the path
      let wav_escaped = ps_escape_single_quoted(&wav);
      let ps = format!(
        r#"$p = New-Object System.Media.SoundPlayer '{path}'; $p.PlaySync();"#,
        path = wav_escaped
      );
      let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .output()
        .map_err(|e| format!("launch powershell failed: {e}"))?;
      if !out.status.success() {
        let stderr_s = String::from_utf8_lossy(&out.stderr);
        let msg = if stderr_s.trim().is_empty() {
          format!("audio play failed: {}", out.status)
        } else {
          format!("audio play failed: {}\n{}", out.status, stderr_s)
        };
        let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
        return Err(msg);
      }
    }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (selection);
      let msg = "OpenAI TTS playback not implemented on this platform".to_string();
      let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
      return Err(msg);
    }
    Ok("ok".into())
  } else {
    // Local Windows TTS with configured voice/rate/volume
    #[cfg(target_os = "windows")]
    {
      use std::io::Write;
      use std::process::{Command, Stdio};
      let voice = settings
        .get("tts_voice_local").and_then(|x| x.as_str()).unwrap_or("").to_string();
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
        rate = rate,
        voice = v_escaped,
      );

      let mut child = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("launch powershell failed: {e}"))?;

      if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(selection.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?;
      }
      let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
      if !status.success() {
        let msg = format!("powershell exited with status: {status}");
        let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
        return Err(msg);
      }
      Ok("ok".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (selection);
      let msg = "TTS not implemented on this platform".to_string();
      let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
      Err(msg)
    }
  }
}

// ---------------------------
// TTS controls (Windows-first): start/stop/list voices/synthesize to WAV
// ---------------------------

#[cfg(target_os = "windows")]
static TTS_CHILD: Lazy<Mutex<Option<std::process::Child>>> = Lazy::new(|| Mutex::new(None));

// Streaming state (OpenAI TTS chunked download)
static STREAM_COUNTER: AtomicU64 = AtomicU64::new(0);
static STREAM_STOPPERS: Lazy<Mutex<std::collections::HashMap<u64, oneshot::Sender<()>>>> = Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

// TTS Streaming Server (local HTTP proxy)
static TTS_STREAMING_SERVER: Lazy<Mutex<Option<TtsStreamingServer>>> = Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "windows")]
fn ps_escape_single_quoted(s: &str) -> String {
  // In PowerShell single-quoted strings, escape ' by doubling it
  s.replace('\'', "''")
}

#[tauri::command]
fn tts_start(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    // Stop any existing TTS first
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
      vol = vol,
      r = r,
      voice = v_escaped,
    );

    let mut child = Command::new("powershell.exe")
      .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
      .stdin(Stdio::piped())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .map_err(|e| format!("launch powershell failed: {e}"))?;

    if let Some(stdin) = child.stdin.as_mut() {
      stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?;
    }
    // Close stdin to signal completion
    drop(child.stdin.take());

    if let Ok(mut guard) = TTS_CHILD.lock() { *guard = Some(child); }
    return Ok(());
  }
  #[cfg(not(target_os = "windows"))]
  {
    let _ = (text, voice, rate, volume);
    Err("TTS not implemented on this platform".into())
  }
}

#[tauri::command]
fn tts_stop() -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(mut guard) = TTS_CHILD.lock() {
      if let Some(mut c) = guard.take() {
        let _ = c.kill();
        let _ = c.wait();
      }
    }
    Ok(())
  }
  #[cfg(not(target_os = "windows"))]
  {
    Err("TTS not implemented on this platform".into())
  }
}

#[tauri::command]
fn tts_list_voices() -> Result<Vec<String>, String> {
  #[cfg(target_os = "windows")]
  {
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
    if !out.status.success() {
      return Err(format!("powershell exited with status: {}", out.status));
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut names: Vec<String> = s.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| l.to_string()).collect();
    names.dedup();
    Ok(names)
  }
  #[cfg(not(target_os = "windows"))]
  {
    Ok(vec![])
  }
}

#[tauri::command]
fn tts_synthesize_wav(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  #[cfg(target_os = "windows")]
  {
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
      vol = vol,
      r = r,
      voice = v_escaped,
      target = target.replace('\\', "\\\\"),
    );

    let mut child = Command::new("powershell.exe")
      .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
      .stdin(Stdio::piped())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .map_err(|e| format!("launch powershell failed: {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
      stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?;
    }
    drop(child.stdin.take());
    let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
    if !status.success() { return Err(format!("powershell exited with status: {status}")); }
    Ok(target)
  }
  #[cfg(not(target_os = "windows"))]
  {
    let _ = (text, voice, rate, volume);
    Err("TTS not implemented on this platform".into())
  }
}

/// Back-compat wrapper: synthesize WAV via OpenAI and return a temp file path.
#[tauri::command]
async fn tts_openai_synthesize_wav(text: String, voice: Option<String>, model: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  tts_openai_synthesize_file(text, voice, model, Some("wav".to_string()), rate, volume).await
}

/// Synthesize speech via OpenAI and return a temp file path. Supports wav/mp3/opus.
#[tauri::command]
async fn tts_openai_synthesize_file(text: String, voice: Option<String>, model: Option<String>, format: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  let key = get_api_key_from_settings_or_env()?;
  let fmt_in = format.unwrap_or_else(|| "wav".to_string());
  let (accept, body_format) = match fmt_in.as_str() {
    "mp3" => ("audio/mpeg", "mp3"),
    "opus" => ("audio/ogg", "opus"), // prefer OGG Opus container for broader player support
    _ => ("audio/wav", "wav"),
  };

  let m = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let v = voice.unwrap_or_else(|| "alloy".to_string());

  let body = serde_json::json!({
    "model": m,
    "input": text,
    "voice": v,
    "format": body_format,
  });

  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/audio/speech")
    .bearer_auth(key)
    .header("Accept", accept)
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    return Err(format!("OpenAI error: {status} {body_text}"));
  }

  // Decide extension from Content-Type header for robustness
  let ct_hdr = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
  let ext = if ct_hdr.contains("wav") { "wav" }
    else if ct_hdr.contains("mpeg") || ct_hdr.contains("mp3") { "mp3" }
    else if ct_hdr.contains("ogg") { "ogg" }
    else if ct_hdr.contains("opus") { "opus" }
    else if fmt_in == "mp3" { "mp3" }
    else if fmt_in == "opus" { "opus" } else { "wav" };

  let file_name = format!("aidc_tts_{}_openai.{}", chrono::Local::now().format("%Y%m%d_%H%M%S"), ext);
  let mut path = std::env::temp_dir();
  path.push(file_name);
  let target = path.to_string_lossy().to_string();

  let bytes = resp.bytes().await.map_err(|e| format!("bytes error: {e}"))?;

  if ext == "wav" {
    let r = rate.unwrap_or(0).clamp(-10, 10);
    let vol = volume.unwrap_or(100).min(100);
    write_pcm16_wav_from_any(&bytes, &target, r, vol)?;
  } else {
    std::fs::write(&target, &bytes).map_err(|e| format!("write failed: {e}"))?;
  }

  Ok(target)
}

/// Start a chunked download stream from OpenAI audio/speech and emit chunks to the frontend.
/// NOTE: This streams raw container bytes (e.g., MP3 or OGG/Opus). Frontend must handle playback.
#[tauri::command]
async fn tts_openai_stream_start(app: tauri::AppHandle, text: String, voice: Option<String>, model: Option<String>, format: Option<String>) -> Result<u64, String> {
  let key = get_api_key_from_settings_or_env()?;
  let fmt = format.unwrap_or_else(|| "opus".to_string());
  let (accept, body_format, mime) = match fmt.as_str() {
    "mp3" => ("audio/mpeg", "mp3", "audio/mpeg"),
    // Ogg Opus: include codecs in MIME for MSE
    _ => ("audio/ogg", "opus", "audio/ogg; codecs=opus"),
  };
  let m = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let v = voice.unwrap_or_else(|| "alloy".to_string());

  let body = serde_json::json!({
    "model": m,
    "input": text,
    "voice": v,
    "format": body_format,
  });

  // Create a cancellation channel and stream id
  let (tx, mut rx) = oneshot::channel::<()>();
  let id = STREAM_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
  {
    let mut map = STREAM_STOPPERS.lock().unwrap();
    map.insert(id, tx);
  }

  let app2 = app.clone();
  tauri::async_runtime::spawn(async move {
    let client = reqwest::Client::new();
    let resp_res = client
      .post("https://api.openai.com/v1/audio/speech")
      .bearer_auth(key)
      .header("Accept", accept)
      .json(&body)
      .send()
      .await;

    let emit_err = |msg: String| {
      let _ = app2.emit("tts:stream:error", serde_json::json!({ "id": id, "message": msg }));
    };

    let resp = match resp_res {
      Ok(r) => r,
      Err(e) => { emit_err(format!("request failed: {e}"));
        let mut map = STREAM_STOPPERS.lock().unwrap(); map.remove(&id); return; }
    };

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      emit_err(format!("OpenAI error: {status} {body_text}"));
      let mut map = STREAM_STOPPERS.lock().unwrap(); map.remove(&id);
      return;
    }

    // Notify start with mime
    let _ = app2.emit("tts:stream:start", serde_json::json!({ "id": id, "mime": mime }));

    let mut stream = resp.bytes_stream();
    loop {
      tokio::select! {
        _ = &mut rx => { // cancelled
          let _ = app2.emit("tts:stream:cancelled", serde_json::json!({ "id": id }));
          break;
        }
        next = stream.next() => {
          match next {
            Some(Ok(chunk)) => {
              let b64 = base64::engine::general_purpose::STANDARD.encode(&chunk);
              let _ = app2.emit("tts:stream:chunk", serde_json::json!({ "id": id, "data": b64 }));
            }
            Some(Err(e)) => {
              emit_err(format!("stream error: {e}"));
              break;
            }
            None => {
              let _ = app2.emit("tts:stream:end", serde_json::json!({ "id": id }));
              break;
            }
          }
        }
      }
    }

    let mut map = STREAM_STOPPERS.lock().unwrap();
    map.remove(&id);
  });

  Ok(id)
}

#[tauri::command]
fn tts_openai_stream_stop(id: u64) -> Result<bool, String> {
  let tx = {
    let mut map = STREAM_STOPPERS.lock().unwrap();
    map.remove(&id)
  };
  if let Some(tx) = tx {
    let _ = tx.send(());
    Ok(true)
  } else {
    Ok(false)
  }
}

// Apply simple gain (volume) and playback rate (by adjusting the sample rate header) to a WAV buffer.
// Note: rate adjustment will change pitch (no time-stretch). If processing fails, the caller should fall back.
fn apply_wav_gain_and_rate(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
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

// Transcribe audio bytes using OpenAI Whisper API (expects WEBM/Opus by default).
// Returns the transcribed text on success.
#[tauri::command]
async fn stt_transcribe(audio: Vec<u8>, mime: String) -> Result<String, String> {
  let key = get_api_key_from_settings_or_env()?;

  // Build multipart form: model + file
  let file_name = if mime.contains("webm") { "audio.webm" } else { "audio.bin" };
  let part = reqwest::multipart::Part::bytes(audio)
    .file_name(file_name.to_string())
    .mime_str(&mime)
    .map_err(|e| format!("mime error: {e}"))?;

  let form = reqwest::multipart::Form::new()
    .text("model", "whisper-1")
    .part("file", part);

  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/audio/transcriptions")
    .bearer_auth(key)
    .multipart(form)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(format!("OpenAI error: {status} {body}"));
  }

  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
  Ok(text)
}

// ---------------------------
// Temp WAV cleanup (OpenAI TTS)
// ---------------------------
#[tauri::command]
fn tts_delete_temp_wav(path: String) -> Result<bool, String> {
  let file_path = PathBuf::from(&path);
  // Ensure file exists early; if not, return Ok(false)
  if !file_path.exists() { return Ok(false); }

  // Only allow deletion inside the system temp directory and matching our prefix/suffix
  let temp_dir = std::env::temp_dir();
  let temp_canon = std::fs::canonicalize(&temp_dir).unwrap_or(temp_dir.clone());
  let file_canon = std::fs::canonicalize(&file_path).map_err(|e| format!("canonicalize failed: {e}"))?;

  if !file_canon.starts_with(&temp_canon) {
    return Err("Refusing to delete non-temp file".into());
  }

  let fname = file_canon.file_name().and_then(|s| s.to_str()).ok_or_else(|| "Invalid file name".to_string())?;
  if !(fname.starts_with("aidc_tts_") && fname.ends_with(".wav")) {
    return Err("Refusing to delete unexpected file".into());
  }

  match fs::remove_file(&file_canon) {
    Ok(_) => Ok(true),
    Err(e) => {
      if e.kind() == std::io::ErrorKind::NotFound { Ok(false) } else { Err(format!("remove failed: {e}")) }
    }
  }
}

#[tauri::command]
fn cleanup_stale_tts_wavs(max_age_minutes: Option<u64>) -> Result<u32, String> {
  let age_min = max_age_minutes.unwrap_or(240);
  let cutoff = SystemTime::now()
    .checked_sub(Duration::from_secs(age_min.saturating_mul(60)))
    .ok_or_else(|| "Invalid cutoff time".to_string())?;

  let temp_dir = std::env::temp_dir();
  let mut removed: u32 = 0;
  let it = match fs::read_dir(&temp_dir) { Ok(i) => i, Err(_) => return Ok(0) };
  for ent in it {
    if let Ok(de) = ent {
      let p = de.path();
      if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
        if name.starts_with("aidc_tts_") && name.to_ascii_lowercase().ends_with(".wav") {
          if let Ok(md) = de.metadata() {
            if let Ok(modified) = md.modified() {
              if modified < cutoff {
                let _ = fs::remove_file(&p).map(|_| { removed = removed.saturating_add(1); });
              }
            }
          }
        }
      }
    }
  }
  Ok(removed)
}

// ---------------------------
// Utility: Copy a file to destination (used by Save As flow)
// ---------------------------
#[tauri::command]
fn copy_file_to_path(src: String, dest: String, overwrite: Option<bool>) -> Result<String, String> {
  let overwrite = overwrite.unwrap_or(true);
  let dest_path = PathBuf::from(&dest);
  if let Some(dir) = dest_path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create destination dir: {e}"))?;
  }
  if dest_path.exists() && !overwrite {
    return Err("Destination file already exists".into());
  }
  fs::copy(&src, &dest_path).map_err(|e| format!("Copy failed: {e}"))?;
  Ok(dest_path.to_string_lossy().to_string())
}

// Simple chat completion endpoint that takes prior conversation messages and returns a single assistant reply.
// Supports either plain string content or structured content parts for multimodal (Vision) prompts.
#[derive(Debug, Deserialize)]
struct ChatMessage {
  role: String,
  content: ChatContent,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ChatContent {
  Text(String),
  Parts(Vec<FrontendPart>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FrontendPart {
  InputText { text: String },
  InputImage { path: String, mime: Option<String> },
}

fn guess_mime_from_path_rs(path: &str) -> Option<&'static str> {
  let p = path.to_ascii_lowercase();
  if p.ends_with(".png") { return Some("image/png"); }
  if p.ends_with(".jpg") || p.ends_with(".jpeg") { return Some("image/jpeg"); }
  if p.ends_with(".webp") { return Some("image/webp"); }
  if p.ends_with(".gif") { return Some("image/gif"); }
  if p.ends_with(".bmp") { return Some("image/bmp"); }
  if p.ends_with(".tif") || p.ends_with(".tiff") { return Some("image/tiff"); }
  None
}

// MCP helper duplicates removed; use mcp::sanitize_fn_name, mcp::parse_mcp_fn_call_name, mcp::summarize_input_schema

#[tauri::command]
async fn chat_complete(app: tauri::AppHandle, messages: Vec<ChatMessage>) -> Result<String, String> {
  let key = get_api_key_from_settings_or_env()?;
  let model = get_model_from_settings_or_env();
  let temp = get_temperature_from_settings_or_env();
  let disabled_map = config::get_disabled_tools_map();

  // Normalize incoming messages to OpenAI format
  let mut norm_msgs: Vec<serde_json::Value> = Vec::new();
  for m in messages.into_iter() {
    let r = match m.role.to_ascii_lowercase().as_str() {
      "system" | "assistant" | "user" => m.role.to_ascii_lowercase(),
      _ => "user".to_string(),
    };

    let content_value = match m.content {
      ChatContent::Text(s) => serde_json::Value::String(s),
      ChatContent::Parts(parts) => {
        let mut out_parts: Vec<serde_json::Value> = Vec::new();
        for p in parts {
          match p {
            FrontendPart::InputText { text } => {
              out_parts.push(serde_json::json!({ "type": "text", "text": text }));
            }
            FrontendPart::InputImage { path, mime } => {
              let mime_final = mime.or_else(|| guess_mime_from_path_rs(&path).map(|s| s.to_string())).ok_or_else(|| format!("Missing/unknown image MIME for: {}", path))?;
              let bytes = std::fs::read(&path).map_err(|e| format!("Failed to read image '{}': {}", path, e))?;
              let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
              let url = format!("data:{};base64,{}", mime_final, b64);
              out_parts.push(serde_json::json!({ "type": "image_url", "image_url": { "url": url } }));
            }
          }
        }
        serde_json::Value::Array(out_parts)
      }
    };
    norm_msgs.push(serde_json::json!({ "role": r, "content": content_value }));
  }

  // Build tool definitions from connected MCP servers (via MCP module)
  let tools = {
    let map = MCP_CLIENTS.lock().await;
    mcp::build_openai_tools_from_mcp(&*map).await
  };

  let client = reqwest::Client::new();
  // Prepend a short system directive to improve first-call argument completeness
  let sys_tool_guidance = serde_json::json!({
    "role": "system",
    "content": "You can use MCP tools. When you call a tool, ALWAYS provide all required parameters per its JSON Schema, with correct types. Do not call tools with empty arguments."
  });
  let mut msgs_for_oai: Vec<serde_json::Value> = Vec::new();
  msgs_for_oai.push(sys_tool_guidance);
  msgs_for_oai.extend(norm_msgs.clone());
  let mut final_text: Option<String> = None;

  // Iterate tool-calls up to a reasonable limit
  for _ in 0..6u8 {
    let mut body = serde_json::json!({
      "model": &model,
      "messages": msgs_for_oai,
    });
    if let Some(t) = temp { if let serde_json::Value::Object(ref mut m) = body { m.insert("temperature".to_string(), serde_json::json!(t)); } }
    if !tools.is_empty() {
      if let serde_json::Value::Object(ref mut m) = body {
        m.insert("tools".to_string(), serde_json::Value::Array(tools.clone()));
        m.insert("tool_choice".to_string(), serde_json::Value::String("auto".to_string()));
        // Allow model to use multiple tool calls
        m.insert("parallel_tool_calls".to_string(), serde_json::Value::Bool(true));
      }
    }

    let resp = client
      .post("https://api.openai.com/v1/chat/completions")
      .bearer_auth(&key)
      .json(&body)
      .send()
      .await
      .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      return Err(format!("OpenAI error: {status} {body_text}"));
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
    let choice0 = v.get("choices").and_then(|c| c.get(0)).cloned().unwrap_or(serde_json::Value::Null);
    let msg = choice0.get("message").cloned().unwrap_or(serde_json::Value::Null);
    let tool_calls_opt = msg.get("tool_calls").and_then(|x| x.as_array()).cloned();
    let content_str_opt = msg.get("content").and_then(|t| t.as_str()).map(|s| s.to_string());

    if let Some(tool_calls) = tool_calls_opt {
      // Append assistant message with tool_calls to history
      let mut assistant_msg = serde_json::Map::new();
      assistant_msg.insert("role".to_string(), serde_json::Value::String("assistant".to_string()));
      if let Some(c) = content_str_opt.as_ref() { assistant_msg.insert("content".to_string(), serde_json::Value::String(c.clone())); }
      assistant_msg.insert("tool_calls".to_string(), serde_json::Value::Array(tool_calls.clone()));
      msgs_for_oai.push(serde_json::Value::Object(assistant_msg));

      // Dispatch each tool call sequentially and append tool results
      for tc in tool_calls.into_iter() {
        let id = tc.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let fname = tc.get("function").and_then(|f| f.get("name")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let fargs_str = tc.get("function").and_then(|f| f.get("arguments")).and_then(|x| x.as_str()).unwrap_or("{}");
        // Parse args JSON (best-effort)
        let mut fargs_val: serde_json::Value = serde_json::from_str(fargs_str).unwrap_or_else(|_| serde_json::json!({}));
        if !fargs_val.is_object() { fargs_val = serde_json::json!({}); }

        let tool_result_text: String;
        if let Some((server_id, tool_name)) = mcp::parse_mcp_fn_call_name(&fname) {
          // Emit tool-call event for UI visibility
          let _ = app.emit("chat:tool-call", serde_json::json!({
            "id": id,
            "function": fname,
            "serverId": server_id,
            "tool": tool_name,
            "args": fargs_val.clone()
          }));
          // Respect disabled tools from settings
          let is_disabled = disabled_map.get(&server_id).map(|set| set.contains(&tool_name)).unwrap_or(false);
          if is_disabled {
            tool_result_text = serde_json::json!({
              "serverId": server_id,
              "tool": tool_name,
              "error": "tool disabled by settings"
            }).to_string();
            let _ = app.emit("chat:tool-result", serde_json::json!({
              "id": id,
              "function": fname,
              "serverId": server_id,
              "tool": tool_name,
              "ok": false,
              "error": "tool disabled by settings"
            }));
          } else {
          // Call MCP tool directly
          let svc_opt = {
            let map2 = MCP_CLIENTS.lock().await;
            map2.get(&server_id).cloned()
          };
          if let Some(svc) = svc_opt {
            let arg_map_opt = fargs_val.as_object().cloned();
            match svc.call_tool(CallToolRequestParam { name: tool_name.clone().into(), arguments: arg_map_opt }).await {
              Ok(res) => {
                tool_result_text = serde_json::to_string(&serde_json::json!({ "serverId": server_id, "tool": tool_name, "result": res })).unwrap_or_else(|_| "{}".to_string());
                let _ = app.emit("chat:tool-result", serde_json::json!({
                  "id": id,
                  "function": fname,
                  "serverId": server_id,
                  "tool": tool_name,
                  "ok": true,
                  "result": res
                }));
              }
              Err(e) => {
                tool_result_text = serde_json::json!({
                  "serverId": server_id,
                  "tool": tool_name,
                  "error": format!("call_tool failed: {}", e)
                }).to_string();
                let _ = app.emit("chat:tool-result", serde_json::json!({
                  "id": id,
                  "function": fname,
                  "serverId": server_id,
                  "tool": tool_name,
                  "ok": false,
                  "error": format!("call_tool failed: {}", e)
                }));
              }
            }
          } else {
            tool_result_text = serde_json::json!({ "error": format!("MCP server not connected: {}", server_id) }).to_string();
            let _ = app.emit("chat:tool-result", serde_json::json!({
              "id": id,
              "function": fname,
              "serverId": server_id,
              "tool": tool_name,
              "ok": false,
              "error": format!("MCP server not connected: {}", server_id)
            }));
          }
          }
        } else {
          tool_result_text = serde_json::json!({ "error": format!("Unsupported tool function: {}", fname) }).to_string();
          let _ = app.emit("chat:tool-result", serde_json::json!({
            "id": id,
            "function": fname,
            "ok": false,
            "error": format!("Unsupported tool function: {}", fname)
          }));
        }

        // Append tool result message
        msgs_for_oai.push(serde_json::json!({
          "role": "tool",
          "tool_call_id": id,
          "content": tool_result_text
        }));
      }

      // Continue the loop for next assistant turn after tool results
      continue;
    }

    // No tool calls; return final assistant content
    final_text = Some(content_str_opt.unwrap_or_default());
    break;
  }

  Ok(final_text.unwrap_or_else(|| "".to_string()))
}

// Open the main window prompt panel with provided text (used by STT flow).
#[tauri::command]
fn open_prompt_with_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
  quick_prompts::open_prompt_with_text(app, text)
}

// Insert provided text directly into the prompt composer input (used by Quick Actions STT flow).
#[tauri::command]
fn insert_prompt_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
  quick_prompts::insert_prompt_text(app, text)
}

// Paste provided text into the currently focused application via clipboard + Ctrl+V.
// Restores the previous clipboard contents when aggressive mode is used (safe_mode=false).
#[tauri::command]
fn insert_text_into_focused_app(text: String, safe_mode: Option<bool>) -> Result<(), String> {
  let safe = safe_mode.unwrap_or(false);

  // Prepare clipboard and save previous contents when not in safe mode.
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };

  // Set clipboard to the text to paste.
  let _ = clipboard.set_text(text);

  // Simulate Ctrl+V to paste into the active application.
  {
    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('v'));
    enigo.key_up(Key::Control);
  }

  // Give time for the target app to read the clipboard.
  thread::sleep(Duration::from_millis(120));

  // Restore previous clipboard contents if we modified it.
  if !safe {
    if let Some(prev) = previous_text {
      let _ = clipboard.set_text(prev);
    }
  }

  Ok(())
}

// Runs a predefined quick prompt (1–9) on the current selection and opens the main window with the AI result.
// Uses aggressive copy-restore by default unless safe_mode is true.
#[tauri::command]
async fn run_quick_prompt(app: tauri::AppHandle, index: u8, safe_mode: Option<bool>) -> Result<(), String> {
  quick_prompts::run_quick_prompt(app, index, safe_mode).await
}

fn quick_prompt_template(index: u8) -> &'static str {
  match index {
    1 => "Summarize the following text in 3-5 bullet points.",
    2 => "Rewrite the following text to be clearer and more concise.",
    3 => "Translate the following text to English.",
    4 => "Explain the following text step-by-step for a beginner.",
    5 => "Extract key action items from the following text.",
    6 => "Generate a short email reply based on the following text.",
    7 => "List pros and cons of the following text.",
    8 => "Create a one-paragraph summary of the following text.",
    9 => "Convert the following text into a checklist.",
    _ => "Summarize the following text in a few bullet points.",
  }
}

// Load quick prompt template from configuration file with optional UI notification on failure.
// Windows: %APPDATA%\AiDesktopCompanion\quick_prompts.json
// Others:  ~/.config/AiDesktopCompanion/quick_prompts.json
// Accepted formats:
//   [ "template1", "template2", ... ]  (1-indexed; index-1)
//   { "1": "template1", "2": "template2", ... }
fn load_quick_prompt_template_with_notify(app: Option<&tauri::AppHandle>, index: u8) -> String {
  if let Some(path) = quick_prompts_config_path() {
    match fs::read_to_string(&path) {
      Ok(text) => {
        match serde_json::from_str::<serde_json::Value>(&text) {
          Ok(v) => {
            if let Some(arr) = v.as_array() {
              if let Some(s) = arr.get((index as usize).saturating_sub(1)).and_then(|x| x.as_str()) {
                return s.to_string();
              } else {
                // Missing or invalid entry - silently fallback without toast
              }
            } else if let Some(obj) = v.as_object() {
              let key = index.to_string();
              if let Some(s) = obj.get(&key).and_then(|x| x.as_str()) {
                return s.to_string();
              } else {
                // Missing or invalid entry - silently fallback without toast
              }
            } else {
              if let Some(app) = app {
                if let Some(win) = app.get_webview_window("main") { let _ = win.show(); let _ = win.set_focus(); }
                let _ = app.emit("settings:quick-prompts-error", serde_json::json!({
                  "message": "quick_prompts.json has invalid structure (expected array or object).",
                  "path": path.to_string_lossy()
                }));
              }
            }
          }
          Err(e) => {
            if let Some(app) = app {
              if let Some(win) = app.get_webview_window("main") { let _ = win.show(); let _ = win.set_focus(); }
              let _ = app.emit("settings:quick-prompts-error", serde_json::json!({
                "message": format!("Invalid JSON in quick_prompts.json: {e}"),
                "path": path.to_string_lossy()
              }));
            }
          }
        }
      }
      Err(e) => {
        if let Some(app) = app {
          if let Some(win) = app.get_webview_window("main") { let _ = win.show(); let _ = win.set_focus(); }
          let _ = app.emit("settings:quick-prompts-error", serde_json::json!({
            "message": format!("Failed to read quick_prompts.json: {e}"),
            "path": path.to_string_lossy()
          }));
        }
      }
    }
  }
  quick_prompt_template(index).to_string()
}

// Capture a region of the screen and save to a temporary PNG. Returns the file path.
// On success also opens the main window and emits `image:capture` with { path }.
#[tauri::command]
fn capture_region(app: tauri::AppHandle, x: i32, y: i32, width: i32, height: i32) -> Result<String, String> {
  if width <= 0 || height <= 0 { return Err("Invalid region size".into()); }
  // Proactively hide/close overlay before capture, to avoid it lingering
  if let Some(overlay) = app.get_webview_window("capture-overlay") {
    // Hiding is sufficient; avoid costly state changes and close after capture
    let _ = overlay.hide();
  }
  // Keep a tiny delay so the hide is applied before capture
  std::thread::sleep(std::time::Duration::from_millis(5));
  #[cfg(target_os = "windows")]
  {
    use screenshots::Screen;
    // Determine which screen contains the top-left point
    let screen = Screen::from_point(x, y).map_err(|e| format!("screen from_point failed: {e}"))?;
    let info = screen.display_info;
    let rel_x = x - info.x;
    let rel_y = y - info.y;
    let w = width as u32;
    let h = height as u32;
    let img = screen.capture_area(rel_x, rel_y, w, h).map_err(|e| format!("capture failed: {e}"))?;

    let file_name = format!("aidc_capture_{}.png", chrono::Local::now().format("%Y%m%d_%H%M%S"));
    let mut path = std::env::temp_dir();
    path.push(file_name);

    img.save(&path).map_err(|e| format!("image save failed: {e}"))?;

    // Open main window and emit event
    if let Some(win) = app.get_webview_window("main") { let _ = win.show(); let _ = win.set_focus(); }
    let payload = serde_json::json!({ "path": path.to_string_lossy() });
    let _ = app.emit("image:capture", payload);
    // Also attempt to close the overlay window by label for robustness
    if let Some(overlay) = app.get_webview_window("capture-overlay") {
      let _ = overlay.close();
    }
    return Ok(path.to_string_lossy().to_string());
  }
  #[cfg(not(target_os = "windows"))]
  {
    Err("Region capture not implemented on this platform".into())
  }
}

// Backwards-compatible wrapper without UI notifications
#[allow(dead_code)]
fn load_quick_prompt_template(index: u8) -> String {
  load_quick_prompt_template_with_notify(None, index)
}

fn quick_prompts_config_path() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("quick_prompts.json");
      return Some(p);
    }
    None
  }
  #[cfg(not(target_os = "windows"))]
  {
    if let Ok(home) = std::env::var("HOME") {
      let mut p = PathBuf::from(home);
      p.push(".config");
      p.push("AiDesktopCompanion");
      p.push("quick_prompts.json");
      return Some(p);
    }
    None
  }
}
