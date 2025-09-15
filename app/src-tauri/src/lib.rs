#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
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
      if let Some(p) = quick_prompts::quick_prompts_config_path() {
        if !p.exists() {
          let _ = quick_prompts::generate_default_quick_prompts();
        }
      }
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      quick_actions::prompt_action,
      quick_actions::position_quick_actions,
      quick_actions::tts_selection,
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
      quick_actions::insert_text_into_focused_app,
      quick_actions::insert_prompt_text,
      quick_actions::open_prompt_with_text,
      quick_actions::prepare_quick_actions,
      quick_actions::focus_prev_then_copy_selection,
      quick_prompts::run_quick_prompt,
      quick_prompts::run_quick_prompt_result,
      quick_prompts::run_quick_prompt_with_selection,
      quick_prompts::generate_default_quick_prompts,
      quick_prompts::get_quick_prompts,
      quick_prompts::save_quick_prompts,
      get_settings,
      save_settings,
      settings::list_openai_models,
      load_conversation_state,
      save_conversation_state,
      clear_conversations,
      quick_actions::copy_file_to_path,
      tts_delete_temp_wav,
      cleanup_stale_tts_wavs,
      quick_actions::get_virtual_screen_bounds,
      quick_actions::size_overlay_to_virtual_screen,
      quick_actions::capture_region,
      quick_actions::copy_text_to_clipboard,
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

use tauri::Manager; // bring get_webview_window into scope
use tauri::Emitter; // bring emit into scope
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use arboard::Clipboard;
use enigo::{Enigo, Key, KeyboardControllable};

pub mod tts_streaming_server;
mod utils;
mod config;
mod quick_prompts;
mod mcp;
mod tts_openai;
mod tts_win_native;
mod tts_utils;
pub mod tts_mod;
pub use tts_mod as tts;
mod stt;
mod capture;
mod chat;
mod settings;
mod quick_actions;

use rmcp::{
  service::{RoleClient, DynService, RunningService},
};
// Audio decoding (fallback for non-WAV responses)

// write_pcm16_wav_from_any wrapper removed; use tts::write_pcm16_wav_from_any directly from helpers as needed

// ---------------------------
// Settings helpers and commands
// ---------------------------

// settings_config_path wrapper removed; use config::settings_config_path() directly where needed

/// Start streaming using OpenAI Responses API with SSE, emitting tts:stream:* events.
#[tauri::command]
async fn tts_openai_responses_stream_start(app: tauri::AppHandle, text: String, voice: Option<String>, model: Option<String>, format: Option<String>) -> Result<u64, String> {
  let key = settings::get_api_key_from_settings_or_env()?;
  tts_openai::responses_stream_start(app, key, text, voice, model, format)
}

// Helpers to parse SSE lines from a raw byte buffer (moved to tts module)

/// Create a new TTS streaming session and return the stream URL
#[tauri::command]
async fn tts_create_stream_session(text: String, voice: Option<String>, model: Option<String>, format: Option<String>, instructions: Option<String>) -> Result<String, String> {
  let api_key = settings::get_api_key_from_settings_or_env()?;
  tts_openai::create_stream_session(text, voice, model, format, instructions, api_key).await
}

/// Stop a TTS streaming session
#[tauri::command]
fn tts_stop_stream_session(session_id: String) -> Result<bool, String> {
  tts_openai::stop_stream_session(session_id)
}

/// QA: get active TTS streaming sessions count
#[tauri::command]
fn tts_stream_session_count() -> Result<usize, String> {
  tts_openai::stream_session_count()
}

/// QA: cleanup idle TTS sessions older than ttl_seconds (that have not started)
#[tauri::command]
fn tts_stream_cleanup_idle(ttl_seconds: u64) -> Result<usize, String> {
  tts_openai::stream_cleanup_idle(ttl_seconds)
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

// get_disabled_tools_map local helper removed; use config::get_disabled_tools_map()

// settings helpers moved to settings.rs

#[tauri::command]
fn get_settings() -> Result<serde_json::Value, String> {
  config::get_settings()
}

#[tauri::command]
fn save_settings(map: serde_json::Value) -> Result<String, String> {
  config::save_settings(map)
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

// tts_selection moved to quick_actions

// TTS Streaming state moved to tts module

#[tauri::command]
fn tts_start(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<(), String> {
  tts_win_native::local_tts_start(text, voice, rate, volume)
}

#[tauri::command]
fn tts_stop() -> Result<(), String> { 
  tts_win_native::local_tts_stop() 
}

#[tauri::command]
fn tts_list_voices() -> Result<Vec<String>, String> { 
  tts_win_native::local_tts_list_voices() 
}

#[tauri::command]
fn tts_synthesize_wav(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  tts_win_native::local_tts_synthesize_wav(text, voice, rate, volume)
}

/// Back-compat wrapper: synthesize WAV via OpenAI and return a temp file path.
#[tauri::command]
async fn tts_openai_synthesize_wav(text: String, voice: Option<String>, model: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  let key = settings::get_api_key_from_settings_or_env()?;
  tts_openai::openai_synthesize_wav(key, text, voice, model, rate, volume).await
}

/// Synthesize speech via OpenAI and return a temp file path. Supports wav/mp3/opus.
#[tauri::command]
async fn tts_openai_synthesize_file(text: String, voice: Option<String>, model: Option<String>, format: Option<String>, rate: Option<i32>, volume: Option<u8>, instructions: Option<String>) -> Result<String, String> {
  let key = settings::get_api_key_from_settings_or_env()?;
  tts_openai::openai_synthesize_file(key, text, voice, model, format, rate, volume, instructions).await
}

/// Start a chunked download stream from OpenAI audio/speech and emit chunks to the frontend.
/// NOTE: This streams raw container bytes (e.g., MP3 or OGG/Opus). Frontend must handle playback.
#[tauri::command]
async fn tts_openai_stream_start(app: tauri::AppHandle, text: String, voice: Option<String>, model: Option<String>, format: Option<String>) -> Result<u64, String> {
  let key = settings::get_api_key_from_settings_or_env()?;
  tts_openai::openai_stream_start(app, key, text, voice, model, format)
}

#[tauri::command]
fn tts_openai_stream_stop(id: u64) -> Result<bool, String> {
  tts_openai::openai_stream_stop(id)
}

// Transcribe audio bytes using OpenAI Whisper API (expects WEBM/Opus by default).
// Returns the transcribed text on success.
#[tauri::command]
async fn stt_transcribe(audio: Vec<u8>, mime: String) -> Result<String, String> {
  let key = settings::get_api_key_from_settings_or_env()?;
  stt::transcribe(key, audio, mime).await
}

// ---------------------------
// Temp WAV cleanup (OpenAI TTS)
// ---------------------------
#[tauri::command]
fn tts_delete_temp_wav(path: String) -> Result<bool, String> {
  tts::delete_temp_wav(path)
}

#[tauri::command]
fn cleanup_stale_tts_wavs(max_age_minutes: Option<u64>) -> Result<u32, String> {
  tts::cleanup_stale_tts_wavs(max_age_minutes)
}

#[tauri::command]
async fn chat_complete(app: tauri::AppHandle, messages: Vec<chat::ChatMessage>) -> Result<String, String> {
  let key = settings::get_api_key_from_settings_or_env()?;
  let model = settings::get_model_from_settings_or_env();
  let temp = settings::get_temperature_from_settings_or_env();
  chat::chat_complete_with_mcp(app, messages, key, model, temp, &MCP_CLIENTS).await
}