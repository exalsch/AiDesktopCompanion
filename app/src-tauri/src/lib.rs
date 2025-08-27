#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
    .plugin(tauri_plugin_dialog::init())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }

      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      prompt_action,
      position_quick_actions,
      tts_selection,
      tts_start,
      tts_stop,
      tts_list_voices,
      tts_synthesize_wav,
      tts_openai_synthesize_wav,
      stt_transcribe,
      chat_complete,
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
      capture_region
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

use std::{thread, time::Duration};
use std::fs;
use std::path::PathBuf;
use tauri::Manager; // bring get_webview_window into scope
use tauri::Emitter; // bring emit into scope
use tauri::PhysicalPosition; // for window positioning
use serde::Deserialize;
use std::io::{Write, Cursor};
use std::process::{Command, Stdio};
use once_cell::sync::Lazy;
use std::sync::Mutex;

use arboard::Clipboard;
use enigo::{Enigo, Key, KeyboardControllable};

// ---------------------------
// Settings helpers and commands
// ---------------------------

fn settings_config_path() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("settings.json");
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
      p.push("settings.json");
      return Some(p);
    }
    None
  }
}

// Path for persisted conversation state (single-thread for now)
fn conversation_state_path() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("conversations.json");
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
      p.push("conversations.json");
      return Some(p);
    }
    None
  }
}

fn persist_conversations_enabled() -> bool {
  let v = load_settings_json();
  v.get("persist_conversations").and_then(|x| x.as_bool()).unwrap_or(false)
}

// ---------------------------
// Conversation persistence commands
// ---------------------------
#[tauri::command]
fn load_conversation_state() -> Result<serde_json::Value, String> {
  if !persist_conversations_enabled() {
    // Respect privacy: do not read/write when disabled
    return Ok(serde_json::json!({}));
  }
  if let Some(path) = conversation_state_path() {
    match fs::read_to_string(&path) {
      Ok(text) => {
        match serde_json::from_str::<serde_json::Value>(&text) {
          Ok(v) => Ok(v),
          Err(e) => Err(format!("Invalid JSON in conversations.json: {e}")),
        }
      }
      Err(_) => Ok(serde_json::json!({})), // not found -> empty
    }
  } else {
    Err("Unsupported platform for config path".into())
  }
}

#[tauri::command]
fn save_conversation_state(state: serde_json::Value) -> Result<String, String> {
  if !persist_conversations_enabled() {
    // If disabled, proactively delete any existing file
    if let Some(path) = conversation_state_path() {
      let _ = fs::remove_file(path);
    }
    return Ok("persistence disabled".into());
  }
  let path = conversation_state_path().ok_or_else(|| "Unsupported platform for config path".to_string())?;
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
  }
  let pretty = serde_json::to_string_pretty(&state).map_err(|e| format!("Serialize conversation failed: {e}"))?;
  fs::write(&path, pretty).map_err(|e| format!("Write conversations failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn clear_conversations() -> Result<String, String> {
  if let Some(path) = conversation_state_path() {
    if path.exists() {
      fs::remove_file(&path).map_err(|e| format!("Remove conversations failed: {e}"))?;
    }
    Ok(path.to_string_lossy().to_string())
  } else {
    Err("Unsupported platform for config path".into())
  }
}

fn load_settings_json() -> serde_json::Value {
  if let Some(path) = settings_config_path() {
    if let Ok(text) = fs::read_to_string(&path) {
      if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if v.is_object() { return v; }
      }
    }
  }
  serde_json::json!({})
}

fn get_api_key_from_settings_or_env() -> Result<String, String> {
  let v = load_settings_json();
  if let Some(s) = v.get("openai_api_key").and_then(|x| x.as_str()) {
    if !s.trim().is_empty() { return Ok(s.to_string()); }
  }
  std::env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())
}

fn get_model_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("openai_chat_model").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return t.to_string(); }
  }
  std::env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string())
}

fn get_temperature_from_settings_or_env() -> Option<f32> {
  let v = load_settings_json();
  v.get("temperature").and_then(|x| x.as_f64()).map(|f| f as f32)
}

#[tauri::command]
fn get_settings() -> Result<serde_json::Value, String> {
  let v = load_settings_json();
  Ok(v)
}

#[tauri::command]
fn save_settings(map: serde_json::Value) -> Result<String, String> {
  let path = settings_config_path().ok_or_else(|| "Unsupported platform for config path".to_string())?;
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
  }
  // Merge with existing settings. Only update known keys present in `map`.
  let mut current = load_settings_json();
  let mut obj = current.as_object().cloned().unwrap_or_default();

  // Existing keys
  if let Some(k) = map.get("openai_api_key").and_then(|x| x.as_str()) { obj.insert("openai_api_key".to_string(), serde_json::Value::String(k.to_string())); }
  if let Some(m) = map.get("openai_chat_model").and_then(|x| x.as_str()) { obj.insert("openai_chat_model".to_string(), serde_json::Value::String(m.to_string())); }
  if let Some(t) = map.get("temperature").and_then(|x| x.as_f64()) { obj.insert("temperature".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(t).unwrap_or_else(|| serde_json::Number::from_f64(1.0).unwrap()))); }
  if let Some(p) = map.get("persist_conversations").and_then(|x| x.as_bool()) { obj.insert("persist_conversations".to_string(), serde_json::Value::Bool(p)); }

  // New TTS preference keys
  if let Some(e) = map.get("tts_engine").and_then(|x| x.as_str()) { obj.insert("tts_engine".to_string(), serde_json::Value::String(e.to_string())); }
  if let Some(r) = map.get("tts_rate").and_then(|x| x.as_i64()) { obj.insert("tts_rate".to_string(), serde_json::Value::Number((r as i64).into())); }
  if let Some(v) = map.get("tts_volume").and_then(|x| x.as_i64()) { obj.insert("tts_volume".to_string(), serde_json::Value::Number((v as i64).into())); }
  if let Some(vl) = map.get("tts_voice_local").and_then(|x| x.as_str()) { obj.insert("tts_voice_local".to_string(), serde_json::Value::String(vl.to_string())); }
  if let Some(ov) = map.get("tts_openai_voice").and_then(|x| x.as_str()) { obj.insert("tts_openai_voice".to_string(), serde_json::Value::String(ov.to_string())); }
  if let Some(om) = map.get("tts_openai_model").and_then(|x| x.as_str()) { obj.insert("tts_openai_model".to_string(), serde_json::Value::String(om.to_string())); }

  let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(obj)).map_err(|e| format!("Serialize settings failed: {e}"))?;
  fs::write(&path, pretty).map_err(|e| format!("Write settings failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
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
  let path = quick_prompts_config_path().ok_or_else(|| "Unsupported platform for config path".to_string())?;
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
  }

  let defaults = serde_json::json!({
    "1": quick_prompt_template(1),
    "2": quick_prompt_template(2),
    "3": quick_prompt_template(3),
    "4": quick_prompt_template(4),
    "5": quick_prompt_template(5),
    "6": quick_prompt_template(6),
    "7": quick_prompt_template(7),
    "8": quick_prompt_template(8),
    "9": quick_prompt_template(9)
  });

  let pretty = serde_json::to_string_pretty(&defaults).map_err(|e| format!("Serialize defaults failed: {e}"))?;
  fs::write(&path, pretty).map_err(|e| format!("Write config failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
fn get_quick_prompts() -> Result<serde_json::Value, String> {
  // Return an object with keys "1".."9". Fill missing/invalid entries with defaults.
  let mut obj = serde_json::Map::new();
  for i in 1..=9u8 { obj.insert(i.to_string(), serde_json::Value::String(quick_prompt_template(i).to_string())); }

  if let Some(path) = quick_prompts_config_path() {
    if let Ok(text) = fs::read_to_string(&path) {
      if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        match v {
          serde_json::Value::Array(arr) => {
            for i in 1..=9u8 {
              if let Some(s) = arr.get((i as usize) - 1).and_then(|x| x.as_str()) {
                obj.insert(i.to_string(), serde_json::Value::String(s.to_string()));
              }
            }
          }
          serde_json::Value::Object(map_in) => {
            for i in 1..=9u8 {
              let k = i.to_string();
              if let Some(s) = map_in.get(&k).and_then(|x| x.as_str()) {
                obj.insert(k, serde_json::Value::String(s.to_string()));
              }
            }
          }
          _ => { /* keep defaults */ }
        }
      }
    }
  }

  Ok(serde_json::Value::Object(obj))
}

#[tauri::command]
fn save_quick_prompts(map: serde_json::Value) -> Result<String, String> {
  // Accept either array or object; normalize to object of 1..9 with strings.
  let mut obj = serde_json::Map::new();
  for i in 1..=9u8 {
    let k = i.to_string();
    let v = match &map {
      serde_json::Value::Array(arr) => arr.get((i as usize) - 1).and_then(|x| x.as_str()).unwrap_or(quick_prompt_template(i)),
      serde_json::Value::Object(m) => m.get(&k).and_then(|x| x.as_str()).unwrap_or(quick_prompt_template(i)),
      _ => quick_prompt_template(i),
    };
    let trimmed = v.trim();
    let final_v = if trimmed.is_empty() { quick_prompt_template(i) } else { trimmed };
    obj.insert(k, serde_json::Value::String(final_v.to_string()));
  }

  let path = quick_prompts_config_path().ok_or_else(|| "Unsupported platform for config path".to_string())?;
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
  }

  let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(obj))
    .map_err(|e| format!("Serialize prompts failed: {e}"))?;
  fs::write(&path, pretty).map_err(|e| format!("Write config failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
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
  let preview: String = selection.chars().take(200).collect();
  let payload = serde_json::json!({
    "selection": selection,
    "preview": preview,
    "length": payload_length(&preview),
  });
  let _ = app.emit("prompt:open", payload);
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

fn payload_length(s: &str) -> usize {
  s.chars().count()
}

#[tauri::command]
async fn tts_selection(_app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
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
      if !out.status.success() { return Err(format!("audio play failed: {}", out.status)); }
    }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (selection);
      return Err("OpenAI TTS playback not implemented on this platform".into());
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
      if !status.success() { return Err(format!("powershell exited with status: {status}")); }
      Ok("ok".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (selection);
      Err("TTS not implemented on this platform".into())
    }
  }
}

#[cfg(target_os = "windows")]
fn speak_windows(text: &str) -> Result<(), String> {
  use std::io::Write;
  use std::process::{Command, Stdio};

  // Use .NET's System.Speech via PowerShell to perform TTS.
  // Rate is set slightly slower than default for clarity.
  let ps = r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
$s.Rate = -2; $s.Volume = 100;
[void]$s.Speak([Console]::In.ReadToEnd());
"#;

  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;

  if let Some(stdin) = child.stdin.as_mut() {
    stdin
      .write_all(text.as_bytes())
      .map_err(|e| format!("powershell stdin write failed: {e}"))?;
  }

  let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
  if !status.success() {
    return Err(format!("powershell exited with status: {status}"));
  }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
fn speak_windows(_text: &str) -> Result<(), String> {
  Err("TTS not implemented on this platform".into())
}

// ---------------------------
// TTS controls (Windows-first): start/stop/list voices/synthesize to WAV
// ---------------------------

#[cfg(target_os = "windows")]
static TTS_CHILD: Lazy<Mutex<Option<std::process::Child>>> = Lazy::new(|| Mutex::new(None));

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

#[tauri::command]
async fn tts_openai_synthesize_wav(text: String, voice: Option<String>, model: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  // Uses OpenAI's TTS endpoint to synthesize speech as WAV and returns the file path
  let key = get_api_key_from_settings_or_env()?;
  let m = model.unwrap_or_else(|| "gpt-4o-mini-tts".to_string());
  let v = voice.unwrap_or_else(|| "alloy".to_string());

  let file_name = format!("aidc_tts_{}_openai.wav", chrono::Local::now().format("%Y%m%d_%H%M%S"));
  let mut path = std::env::temp_dir();
  path.push(file_name);
  let target = path.to_string_lossy().to_string();

  let body = serde_json::json!({
    "model": m,
    "input": text,
    "voice": v,
    "format": "wav"
  });

  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/audio/speech")
    .bearer_auth(key)
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    return Err(format!("OpenAI error: {status} {body_text}"));
  }

  let bytes = resp.bytes().await.map_err(|e| format!("bytes error: {e}"))?;
  let r = rate.unwrap_or(0).clamp(-10, 10);
  let vol = volume.unwrap_or(100).min(100);

  if r != 0 || vol != 100 {
    match apply_wav_gain_and_rate(&bytes, &target, r, vol) {
      Ok(()) => {}
      Err(_e) => {
        // Fallback to writing original bytes if processing fails
        fs::write(&target, &bytes).map_err(|e| format!("write wav failed: {e}"))?;
      }
    }
  } else {
    fs::write(&target, &bytes).map_err(|e| format!("write wav failed: {e}"))?;
  }
  Ok(target)
}

// Apply simple gain (volume) and playback rate (by adjusting the sample rate header) to a WAV buffer.
// Note: rate adjustment will change pitch (no time-stretch). If processing fails, the caller should fall back.
fn apply_wav_gain_and_rate(bytes: &[u8], target_path: &str, rate: i32, volume: u8) -> Result<(), String> {
  let mut reader = hound::WavReader::new(Cursor::new(bytes))
    .map_err(|e| format!("wav decode failed: {e}"))?;
  let mut spec = reader.spec();

  let gain: f32 = (volume as f32 / 100.0).max(0.0);
  let r = rate.clamp(-10, 10);
  if r != 0 {
    let factor = (2f32).powf(r as f32 / 10.0);
    let new_rate = ((spec.sample_rate as f32) * factor).round() as u32;
    // Clamp to a sane range to avoid extreme sample rates
    let new_rate = new_rate.clamp(8000, 192000);
    spec.sample_rate = new_rate;
  }

  let mut writer = hound::WavWriter::create(target_path, spec.clone())
    .map_err(|e| format!("wav writer create failed: {e}"))?;

  match spec.sample_format {
    hound::SampleFormat::Float => {
      // 32-bit float common
      let mut it = reader.samples::<f32>();
      while let Some(s) = it.next() {
        let v = s.map_err(|e| format!("wav read sample failed: {e}"))?;
        let out = (v * gain).clamp(-1.0, 1.0);
        writer.write_sample(out).map_err(|e| format!("wav write sample failed: {e}"))?;
      }
    }
    hound::SampleFormat::Int => {
      if spec.bits_per_sample <= 16 {
        let mut it = reader.samples::<i16>();
        while let Some(s) = it.next() {
          let v = s.map_err(|e| format!("wav read sample failed: {e}"))?;
          let out = ((v as f32) * gain).clamp(i16::MIN as f32, i16::MAX as f32) as i16;
          writer.write_sample(out).map_err(|e| format!("wav write sample failed: {e}"))?;
        }
      } else if spec.bits_per_sample <= 32 {
        let mut it = reader.samples::<i32>();
        // Compute clamp range from bits_per_sample
        let max_val: i64 = (1i64 << (spec.bits_per_sample - 1)) - 1;
        let min_val: i64 = -1i64 << (spec.bits_per_sample - 1);
        while let Some(s) = it.next() {
          let v = s.map_err(|e| format!("wav read sample failed: {e}"))? as i64;
          let out = ((v as f32) * gain).round() as i64;
          let clamped = out.clamp(min_val, max_val) as i32;
          writer.write_sample(clamped).map_err(|e| format!("wav write sample failed: {e}"))?;
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
// The frontend builds the message list; we simply forward to OpenAI.
#[derive(Debug, Deserialize)]
struct ChatMessage {
  role: String,
  content: String,
}

#[tauri::command]
async fn chat_complete(messages: Vec<ChatMessage>) -> Result<String, String> {
  let key = get_api_key_from_settings_or_env()?;
  let model = get_model_from_settings_or_env();
  let temp = get_temperature_from_settings_or_env();

  // Normalize roles to allowed set; default to user
  let norm_msgs: Vec<serde_json::Value> = messages
    .into_iter()
    .map(|m| {
      let r = match m.role.to_ascii_lowercase().as_str() {
        "system" | "assistant" | "user" => m.role.to_ascii_lowercase(),
        _ => "user".to_string(),
      };
      serde_json::json!({ "role": r, "content": m.content })
    })
    .collect();

  let mut body = serde_json::json!({
    "model": model,
    "messages": norm_msgs,
  });
  if let Some(t) = temp { if let serde_json::Value::Object(ref mut m) = body { m.insert("temperature".to_string(), serde_json::json!(t)); } }

  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/chat/completions")
    .bearer_auth(key)
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
  let text = v.get("choices")
    .and_then(|c| c.get(0))
    .and_then(|c| c.get("message"))
    .and_then(|m| m.get("content"))
    .and_then(|t| t.as_str())
    .unwrap_or("")
    .to_string();

  Ok(text)
}

// Open the main window prompt panel with provided text (used by STT flow).
#[tauri::command]
fn open_prompt_with_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
  if let Some(win) = app.get_webview_window("main") {
    let _ = win.show();
    let _ = win.set_focus();
  }
  let preview: String = text.chars().take(200).collect();
  let payload = serde_json::json!({
    "selection": text,
    "preview": preview,
    "length": payload_length(&preview),
  });
  let _ = app.emit("prompt:open", payload);
  Ok(())
}

// Runs a predefined quick prompt (1–9) on the current selection and opens the main window with the AI result.
// Uses aggressive copy-restore by default unless safe_mode is true.
#[tauri::command]
async fn run_quick_prompt(app: tauri::AppHandle, index: u8, safe_mode: Option<bool>) -> Result<(), String> {
  let safe = safe_mode.unwrap_or(false);

  // Capture selection text (duplication kept for clarity and simplicity; refactor later if needed)
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

  // If empty selection, open main window with a friendly message.
  if selection.trim().is_empty() {
    let _ = open_prompt_with_text(app, "No selection. Type your input or paste it here.".to_string());
    return Ok(());
  }

  // Build messages: system carries instruction + template; user is raw selection
  let template = load_quick_prompt_template_with_notify(Some(&app), index);
  let system_content = format!("Reply only with the result and nothing else. {template}");
  let user_content = selection.clone();

  // Call OpenAI Chat Completions (respect settings overrides)
  let key = get_api_key_from_settings_or_env()?;
  let model = get_model_from_settings_or_env();
  let temp = get_temperature_from_settings_or_env();

  let mut body = serde_json::json!({
    "model": model,
    "messages": [
      { "role": "system", "content": system_content },
      { "role": "user", "content": user_content }
    ]
  });
  if let Some(t) = temp { if let serde_json::Value::Object(ref mut m) = body { m.insert("temperature".to_string(), serde_json::json!(t)); } }

  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/chat/completions")
    .bearer_auth(key)
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
  let text = v.get("choices")
    .and_then(|c| c.get(0))
    .and_then(|c| c.get("message"))
    .and_then(|m| m.get("content"))
    .and_then(|t| t.as_str())
    .unwrap_or("")
    .to_string();

  let out = if text.trim().is_empty() { "No response received.".to_string() } else { text };

  // Insert result into the active application: set clipboard -> Ctrl+V -> restore clipboard
  let after_restore_before_paste = clipboard.get_text().ok();
  let _ = clipboard.set_text(out);
  {
    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('v'));
    enigo.key_up(Key::Control);
  }
  thread::sleep(Duration::from_millis(120));
  if let Some(prev) = after_restore_before_paste {
    let _ = clipboard.set_text(prev);
  }
  Ok(())
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
    let _ = overlay.set_always_on_top(false);
    let _ = overlay.set_fullscreen(false);
    let _ = overlay.unmaximize();
    let _ = overlay.hide();
    let _ = overlay.close();
  }
  std::thread::sleep(std::time::Duration::from_millis(30));
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
