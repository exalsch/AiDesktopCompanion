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
      use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
      if let Ok(guard) = LAST_FOREGROUND.lock() {
        if let Some(hraw) = *guard {
          let hwnd = HWND(hraw as *mut c_void);
          // Only SetForegroundWindow — no ShowWindow(SW_RESTORE) to avoid resizing maximized windows
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

/// Refocus the previously stored foreground window (from prepare_quick_actions).
/// Used to restore focus to the correct app before pasting STT results.
#[tauri::command]
pub fn refocus_previous_app() -> Result<(), String> {
  #[cfg(target_os = "windows")]
  unsafe {
    use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;
    if let Ok(guard) = LAST_FOREGROUND.lock() {
      if let Some(hraw) = *guard {
        let hwnd = HWND(hraw as *mut c_void);
        // Only SetForegroundWindow — no ShowWindow(SW_RESTORE) to avoid resizing maximized windows
        let _ = SetForegroundWindow(hwnd);
      }
    }
  }
  Ok(())
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

// Window positioning near cursor (caret-first, mouse fallback, screen-edge clamping)
#[tauri::command]
pub fn position_quick_actions(app: tauri::AppHandle) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
      GetCursorPos, GetGUIThreadInfo, GetSystemMetrics, GetForegroundWindow,
      GetWindowThreadProcessId, GUITHREADINFO, SM_CXVIRTUALSCREEN,
      SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };

    // Wrap everything in catch_unwind so a panic never kills the app
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        let mut use_caret = false;

        // Try caret position from the foreground window's GUI thread
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
          let tid = GetWindowThreadProcessId(fg, None);
          if tid != 0 {
            let mut gui = GUITHREADINFO {
              cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
              ..std::mem::zeroed()
            };
            if GetGUIThreadInfo(tid, &mut gui).is_ok() {
              let caret = gui.rcCaret;
              if caret.right > caret.left && caret.bottom > caret.top && !gui.hwndCaret.0.is_null() {
                let mut caret_pt = POINT { x: caret.left, y: caret.bottom };
                use windows::Win32::Graphics::Gdi::ClientToScreen;
                if ClientToScreen(gui.hwndCaret, &mut caret_pt).as_bool() {
                  pt = caret_pt;
                  use_caret = true;
                }
              }
            }
          }
        }

        // Fallback: mouse cursor + small offset
        if !use_caret {
          let _ = GetCursorPos(&mut pt);
          pt.x += 12;
          pt.y += 12;
        }

        // Clamp to virtual screen bounds
        let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let screen_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let screen_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let popup_w = 380;
        let popup_h = 95;

        let x = pt.x.max(screen_x).min(screen_x + screen_w - popup_w);
        let y = pt.y.max(screen_y).min(screen_y + screen_h - popup_h);

        (x, y)
      }
    }));

    let (x, y) = match result {
      Ok(pos) => pos,
      Err(_) => {
        // Panic fallback: center-ish on primary monitor
        log::warn!("position_quick_actions: panic caught, using fallback position");
        (200, 200)
      }
    };

    if let Some(win) = app.get_webview_window("quick-actions") {
      let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
    }
    Ok(())
  }
  #[cfg(not(target_os = "windows"))]
  { Ok(()) }
}

/// Clamp the quick-actions window to screen bounds after a resize.
/// Reads the current window position and size, then adjusts position if any part is off-screen.
#[tauri::command]
pub fn clamp_quick_actions_to_screen(app: tauri::AppHandle) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::UI::WindowsAndMessaging::{
      GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
      SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    };
    if let Some(win) = app.get_webview_window("quick-actions") {
      let pos = win.outer_position().map_err(|e| format!("{e}"))?;
      let size = win.outer_size().map_err(|e| format!("{e}"))?;
      unsafe {
        let screen_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let screen_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let screen_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let screen_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        let w = size.width as i32;
        let h = size.height as i32;
        let mut x = pos.x;
        let mut y = pos.y;
        let mut changed = false;
        if x + w > screen_x + screen_w { x = screen_x + screen_w - w; changed = true; }
        if y + h > screen_y + screen_h { y = screen_y + screen_h - h; changed = true; }
        if x < screen_x { x = screen_x; changed = true; }
        if y < screen_y { y = screen_y; changed = true; }
        if changed {
          let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
        }
      }
    }
    Ok(())
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

/// Dump debug text to a log file in the app config directory.
/// Returns the full path of the written file.
#[tauri::command]
pub fn dump_key_log(text: String) -> Result<String, String> {
  let dir = if let Ok(appdata) = std::env::var("APPDATA") {
    let mut p = std::path::PathBuf::from(appdata);
    p.push("AiDesktopCompanion");
    p
  } else {
    return Err("APPDATA not set".into());
  };
  let _ = std::fs::create_dir_all(&dir);
  let path = dir.join("qa_key_log.txt");
  // Append with timestamp header
  let header = format!("\n===== {} =====\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"));
  let content = format!("{}{}\n", header, text);
  use std::io::Write;
  let mut f = std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&path)
    .map_err(|e| format!("open failed: {e}"))?;
  f.write_all(content.as_bytes()).map_err(|e| format!("write failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
}

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
