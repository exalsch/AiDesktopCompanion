#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
      stt_transcribe,
      open_prompt_with_text,
      run_quick_prompt,
      generate_default_quick_prompts,
      get_quick_prompts,
      save_quick_prompts,
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

use arboard::Clipboard;
use enigo::{Enigo, Key, KeyboardControllable};

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
fn tts_selection(_app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
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

  speak_windows(&selection)?;
  Ok("ok".into())
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

// Transcribe audio bytes using OpenAI Whisper API (expects WEBM/Opus by default).
// Returns the transcribed text on success.
#[tauri::command]
async fn stt_transcribe(audio: Vec<u8>, mime: String) -> Result<String, String> {
  let key = std::env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;

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

  // Call OpenAI Chat Completions
  let key = std::env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set".to_string())?;
  let model = std::env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());

  let body = serde_json::json!({
    "model": model,
    "messages": [
      { "role": "system", "content": system_content },
      { "role": "user", "content": user_content }
    ]
  });

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
