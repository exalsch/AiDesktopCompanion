use std::fs;
use std::path::PathBuf;
use std::{thread, time::Duration};

use arboard::Clipboard;
use tauri::{Manager, Emitter};

use crate::config::{get_api_key_from_settings_or_env, get_model_from_settings_or_env, get_temperature_from_settings_or_env};

pub fn quick_prompts_config_path() -> Option<PathBuf> {
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

/// Block until the modifier keys are physically released, or `max_wait` elapses.
///
/// A global shortcut fires on key-down, so its modifiers are usually still held
/// when the handler runs. Sending a synthetic Ctrl+A at that moment would reach
/// the target application as Ctrl+Alt+A (or whatever the shortcut was), which
/// most apps ignore or bind to something else entirely.
#[cfg(target_os = "windows")]
fn wait_for_modifiers_released(max_wait: Duration) {
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
  };
  let keys = [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN];
  let started = std::time::Instant::now();
  loop {
    let any_down = unsafe {
      keys
        .iter()
        .any(|k| (GetAsyncKeyState(k.0 as i32) as u16 & 0x8000) != 0)
    };
    if !any_down || started.elapsed() >= max_wait {
      break;
    }
    thread::sleep(Duration::from_millis(20));
  }
  // Let the target application process the key-up events before we synthesize.
  thread::sleep(Duration::from_millis(40));
}

#[cfg(not(target_os = "windows"))]
fn wait_for_modifiers_released(_max_wait: Duration) {
  thread::sleep(Duration::from_millis(60));
}

/// Capture the current selection of the focused application through the
/// clipboard, restoring the previous clipboard content afterwards.
///
/// * `safe` - skip the synthetic Ctrl+C and clipboard restore, and just read
///   whatever is already on the clipboard.
/// * `select_all` - send Ctrl+A first, so the whole document is captured
///   instead of the current selection.
fn capture_selection(safe: bool, select_all: bool) -> Result<String, String> {
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };

  if !safe {
    if select_all {
      // The hotkey that triggered this is likely still held down.
      wait_for_modifiers_released(Duration::from_millis(900));
      crate::utils::send_ctrl_key('a')?;
      thread::sleep(Duration::from_millis(80));
    }
    crate::utils::send_ctrl_key('c')?;
    thread::sleep(Duration::from_millis(120));
  }

  let selection = clipboard.get_text().unwrap_or_default();

  if !safe {
    if let Some(prev) = previous_text {
      let _ = clipboard.set_text(prev);
    }
  }

  Ok(selection)
}

/// Build the system/user messages for a quick prompt and run one chat
/// completion, returning the assistant text.
///
/// System prompt composition: the dedicated `quick_prompt_system_prompt`
/// setting wins over the global `system_prompt`; the quick prompt template is
/// appended to whichever is used.
async fn complete_quick_prompt(app: &tauri::AppHandle, index: u8, selection: &str) -> Result<String, String> {
  let template = load_quick_prompt_template_with_notify(Some(app), index);
  let settings = crate::config::load_settings_json();
  let base = {
    let qp = settings
      .get("quick_prompt_system_prompt")
      .and_then(|x| x.as_str())
      .unwrap_or("")
      .trim();
    if qp.is_empty() {
      settings
        .get("system_prompt")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
    } else {
      qp.to_string()
    }
  };
  let system_content = if base.is_empty() { template.clone() } else { format!("{base}\n\n{template}") };

  let key = get_api_key_from_settings_or_env()?;
  // Prefer dedicated quick_prompt_model; fallback to global chat model
  let model = {
    let s = settings
      .get("quick_prompt_model")
      .and_then(|x| x.as_str())
      .unwrap_or("")
      .trim()
      .to_string();
    if s.is_empty() { get_model_from_settings_or_env() } else { s }
  };
  let temp = get_temperature_from_settings_or_env();

  let mut body = serde_json::json!({
    "model": model,
    "messages": [
      { "role": "system", "content": system_content },
      { "role": "user", "content": selection }
    ]
  });
  if let Some(t) = temp {
    if let serde_json::Value::Object(ref mut m) = body {
      m.insert("temperature".to_string(), serde_json::json!(t));
    }
  }

  let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(120))
    .connect_timeout(std::time::Duration::from_secs(10))
    .build()
    .unwrap_or_else(|_| reqwest::Client::new());
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
  let text = v
    .get("choices")
    .and_then(|c| c.get(0))
    .and_then(|c| c.get("message"))
    .and_then(|m| m.get("content"))
    .and_then(|t| t.as_str())
    .unwrap_or("")
    .to_string();

  Ok(if text.trim().is_empty() { "No response received.".to_string() } else { text })
}

const EMPTY_SELECTION_HINT: &str = "No selection. Type your input or paste it here.";

/// Shared body for the two "capture, run, paste back" flows.
async fn run_quick_prompt_inner(app: tauri::AppHandle, index: u8, safe_mode: Option<bool>, select_all: bool) -> Result<(), String> {
  if index < 1 || index > 9 {
    return Err("Quick prompt index must be 1-9".into());
  }
  let safe = safe_mode.unwrap_or(false);
  let label = if select_all {
    format!("Quick Prompt {index} (whole document)")
  } else {
    format!("Quick Prompt {index}")
  };

  crate::busy::start(&app, &label);
  let outcome = async {
    let selection = capture_selection(safe, select_all)?;

    // If empty selection, open main window with a friendly message.
    if selection.trim().is_empty() {
      let _ = crate::quick_actions::open_prompt_with_text(app.clone(), EMPTY_SELECTION_HINT.to_string());
      return Ok(());
    }

    let out = complete_quick_prompt(&app, index, &selection).await?;
    // Insert result into the active application: set clipboard -> Ctrl+V -> restore clipboard
    crate::quick_actions::insert_text_into_focused_app(out, Some(safe))
  }
  .await;
  crate::busy::finish(&app, outcome.clone());
  outcome
}

/// Runs a predefined quick prompt (1-9) on the current selection and pastes the AI result
/// back into the focused application.
/// Uses aggressive copy-restore by default unless safe_mode is true.
#[tauri::command]
pub async fn run_quick_prompt(app: tauri::AppHandle, index: u8, safe_mode: Option<bool>) -> Result<(), String> {
  run_quick_prompt_inner(app, index, safe_mode, false).await
}

/// Selects the whole document in the focused application (Ctrl+A), then runs the
/// configured quick prompt on it and pastes the result back over the selection.
///
/// This backs the dedicated "select all + quick prompt" global hotkey, so a whole
/// document can be rewritten without opening the popup first.
#[tauri::command]
pub async fn run_quick_prompt_select_all(app: tauri::AppHandle, index: u8, safe_mode: Option<bool>) -> Result<(), String> {
  run_quick_prompt_inner(app, index, safe_mode, true).await
}

/// Runs a predefined quick prompt (1-9) on the current selection and RETURNS the AI result text
/// without inserting it into the focused application. Use this for inline preview flows in the
/// Quick Actions popup. Uses the same selection capture and system prompt composition as
/// `run_quick_prompt`.
#[tauri::command]
pub async fn run_quick_prompt_result(app: tauri::AppHandle, index: u8, safe_mode: Option<bool>) -> Result<String, String> {
  if index < 1 || index > 9 {
    return Err("Quick prompt index must be 1-9".into());
  }
  let selection = capture_selection(safe_mode.unwrap_or(false), false)?;
  if selection.trim().is_empty() {
    return Ok(EMPTY_SELECTION_HINT.to_string());
  }
  complete_quick_prompt(&app, index, &selection).await
}

/// Same as `run_quick_prompt_result` but uses the provided selection string directly,
/// avoiding clipboard operations and window focus changes. This is intended for
/// inline preview flows when the frontend has already captured the selection.
#[tauri::command]
pub async fn run_quick_prompt_with_selection(app: tauri::AppHandle, index: u8, selection: String) -> Result<String, String> {
  if index < 1 || index > 9 {
    return Err("Quick prompt index must be 1-9".into());
  }
  if selection.trim().is_empty() {
    return Ok(EMPTY_SELECTION_HINT.to_string());
  }
  complete_quick_prompt(&app, index, &selection).await
}

pub fn quick_prompt_template(index: u8) -> &'static str {
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

pub fn load_quick_prompt_template_with_notify(app: Option<&tauri::AppHandle>, index: u8) -> String {
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

#[allow(dead_code)]
pub fn load_quick_prompt_template(index: u8) -> String {
  load_quick_prompt_template_with_notify(None, index)
}

// capture/file/screen commands moved to quick_actions.rs

#[tauri::command]
pub fn generate_default_quick_prompts() -> Result<String, String> {
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
pub fn get_quick_prompts() -> Result<serde_json::Value, String> {
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
          _ => {}
        }
      }
    }
  }

  Ok(serde_json::Value::Object(obj))
}

#[tauri::command]
pub fn save_quick_prompts(map: serde_json::Value) -> Result<String, String> {
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
  let tmp_path = path.with_extension("json.tmp");
  fs::write(&tmp_path, &pretty).map_err(|e| format!("Write config failed: {e}"))?;
  #[cfg(target_os = "windows")]
  { if path.exists() { let _ = fs::remove_file(&path); } }
  fs::rename(&tmp_path, &path).map_err(|e| format!("Rename config failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
}
