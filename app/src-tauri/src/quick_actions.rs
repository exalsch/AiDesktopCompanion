use std::{thread, time::Duration};
use std::sync::Mutex;
use once_cell::sync::Lazy;

use arboard::Clipboard;
use enigo::{Enigo, Key, KeyboardControllable};
use tauri::{Emitter, Manager, PhysicalPosition};
#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;

// Store the last foreground window handle (Windows) as a raw isize so we can
// briefly return focus to it to capture selection without hiding the QA window.
#[cfg(target_os = "windows")]
static LAST_FOREGROUND: Lazy<Mutex<Option<isize>>> = Lazy::new(|| Mutex::new(None));

// UI actions and quick insertions

#[tauri::command]
pub fn prompt_action(app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
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
    if let Some(prev) = previous_text { let _ = clipboard.set_text(prev); }
  }

  // Bring main window to front and emit event with selection details
  if let Some(win) = app.get_webview_window("main") { let _ = win.show(); let _ = win.set_focus(); }
  let payload = serde_json::json!({ "text": selection });
  let _ = app.emit("prompt:new-conversation", payload);
  Ok("ok".to_string())
}

/// Called before showing the Quick Actions popup. Stores the current foreground
/// native window so we can refocus it during selection capture without hiding
/// the QA window.
#[tauri::command]
pub fn prepare_quick_actions() -> Result<(), String> {
  #[cfg(target_os = "windows")]
  unsafe {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let h = GetForegroundWindow();
    let mut guard = LAST_FOREGROUND.lock().map_err(|_| "lock poisoned".to_string())?;
    *guard = Some(h.0 as isize);
  }
  Ok(())
}

/// Refocus the previously active native window (if available) and copy the current
/// selection using Ctrl+C, then restore focus to the Quick Actions window. Returns
/// the copied text. When safe_mode is true, this just returns the current clipboard.
#[tauri::command]
pub fn focus_prev_then_copy_selection(app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
  let safe = safe_mode.unwrap_or(false);
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };

  if !safe {
    #[cfg(target_os = "windows")]
    unsafe {
      use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};
      if let Ok(guard) = LAST_FOREGROUND.lock() {
        if let Some(hraw) = *guard {
          let hwnd = HWND(hraw as *mut c_void);
          // Best-effort: restore if minimized then bring to foreground
          let _ = ShowWindow(hwnd, SW_RESTORE);
          let _ = SetForegroundWindow(hwnd);
          thread::sleep(Duration::from_millis(80));
        }
      }
    }

    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('c'));
    enigo.key_up(Key::Control);
    thread::sleep(Duration::from_millis(140));
  }

  let selection = clipboard.get_text().unwrap_or_default();

  if !safe {
    if let Some(prev) = previous_text { let _ = clipboard.set_text(prev); }
  }

  // Restore focus to quick-actions so the user sees the preview update
  if let Some(qa) = app.get_webview_window("quick-actions") {
    let _ = qa.show();
    let _ = qa.set_focus();
  }

  Ok(selection)
}

/// Set clipboard text directly. Used by Quick Actions result preview 'Copy' action.
#[tauri::command]
pub fn copy_text_to_clipboard(text: String) -> Result<(), String> {
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let _ = clipboard.set_text(text);
  Ok(())
}

#[tauri::command]
pub fn open_prompt_with_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
  if let Some(win) = app.get_webview_window("main") { let _ = win.show(); let _ = win.set_focus(); }
  let preview: String = text.chars().take(200).collect();
  let payload = serde_json::json!({ "selection": text, "preview": preview, "length": preview.chars().count() });
  let _ = app.emit("prompt:open", payload);
  Ok(())
}

#[tauri::command]
pub fn insert_prompt_text(app: tauri::AppHandle, text: String) -> Result<(), String> {
  let payload = serde_json::json!({ "text": text });
  if let Some(win) = app.get_webview_window("main") { let _ = win.emit("prompt:insert", payload); }
  else { let _ = app.emit("prompt:insert", serde_json::json!({ "text": text })); }
  Ok(())
}

#[tauri::command]
pub fn insert_text_into_focused_app(text: String, safe_mode: Option<bool>) -> Result<(), String> {
  let safe = safe_mode.unwrap_or(false);
  let mut clipboard = Clipboard::new().map_err(|e| format!("clipboard init failed: {e}"))?;
  let previous_text = if !safe { clipboard.get_text().ok() } else { None };
  let _ = clipboard.set_text(text);
  {
    let mut enigo = Enigo::new();
    enigo.key_down(Key::Control);
    enigo.key_click(Key::Layout('v'));
    enigo.key_up(Key::Control);
  }
  thread::sleep(Duration::from_millis(120));
  if !safe { if let Some(prev) = previous_text { let _ = clipboard.set_text(prev); } }
  Ok(())
}

// Window positioning near cursor
#[tauri::command]
pub fn position_quick_actions(app: tauri::AppHandle) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    unsafe {
      let mut pt = POINT { x: 0, y: 0 };
      if let Err(e) = GetCursorPos(&mut pt) { return Err(format!("GetCursorPos failed: {e}")); }
      let x = pt.x + 12; let y = pt.y + 12;
      if let Some(win) = app.get_webview_window("quick-actions") {
        let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
      }
      Ok(())
    }
  }
  #[cfg(not(target_os = "windows"))]
  { Ok(()) }
}

// File util passthrough
#[tauri::command]
pub fn copy_file_to_path(src: String, dest: String, overwrite: Option<bool>) -> Result<String, String> {
  crate::utils::copy_file_to_path(src, dest, overwrite)
}

// Screen capture helpers passthrough
#[tauri::command]
pub fn get_virtual_screen_bounds() -> Result<serde_json::Value, String> {
  crate::capture::get_virtual_screen_bounds()
}

#[tauri::command]
pub fn size_overlay_to_virtual_screen(app: tauri::AppHandle) -> Result<(), String> {
  crate::capture::size_overlay_to_virtual_screen(app)
}

#[tauri::command]
pub fn capture_region(app: tauri::AppHandle, x: i32, y: i32, width: i32, height: i32) -> Result<String, String> {
  crate::capture::capture_region(app, x, y, width, height)
}

// TTS selection flow (moved from lib.rs)
#[tauri::command]
pub async fn tts_selection(app: tauri::AppHandle, safe_mode: Option<bool>) -> Result<String, String> {
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
    if let Some(prev) = previous_text { let _ = clipboard.set_text(prev); }
  }

  if selection.trim().is_empty() {
    let _ = app.emit("tts:error", serde_json::json!({ "message": "No text selected" }));
    return Err("No text selected".into());
  }

  // Read user TTS settings
  let settings = crate::config::load_settings_json();
  let engine = settings.get("tts_engine").and_then(|x| x.as_str()).unwrap_or("local");
  let rate = settings.get("tts_rate").and_then(|x| x.as_i64()).unwrap_or(-2).clamp(-10, 10) as i32;
  let vol = settings.get("tts_volume").and_then(|x| x.as_i64()).unwrap_or(100).clamp(0, 100) as u8;

  if engine == "openai" {
    let voice = settings.get("tts_openai_voice").and_then(|x| x.as_str()).unwrap_or("alloy").to_string();
    let model = settings.get("tts_openai_model").and_then(|x| x.as_str()).unwrap_or("gpt-4o-mini-tts").to_string();
    let wav = crate::tts_openai_synthesize_wav(selection.clone(), Some(voice), Some(model), Some(rate), Some(vol)).await?;
    #[cfg(target_os = "windows")]
    { crate::utils::play_wav_blocking_windows(&app, &wav)?; }
    #[cfg(not(target_os = "windows"))]
    {
      let _ = (selection);
      let msg = "OpenAI TTS playback not implemented on this platform".to_string();
      let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
      return Err(msg);
    }
    Ok("ok".into())
  } else {
    #[cfg(target_os = "windows")]
    {
      let voice = settings.get("tts_voice_local").and_then(|x| x.as_str()).unwrap_or("").to_string();
      crate::tts::local_speak_blocking(selection, voice, rate, vol)?;
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
