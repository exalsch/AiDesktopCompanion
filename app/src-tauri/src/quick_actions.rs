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
static LAST_SELECTED_TEXT: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new(String::new()));

pub fn last_selected_text() -> String {
  LAST_SELECTED_TEXT
    .lock()
    .map(|g| g.clone())
    .unwrap_or_default()
}

#[cfg(target_os = "windows")]
pub fn last_foreground_handle_raw() -> Option<isize> {
  LAST_FOREGROUND.lock().ok().and_then(|g| *g)
}

#[cfg(not(target_os = "windows"))]
pub fn last_foreground_handle_raw() -> Option<isize> {
  None
}

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
  if let Ok(mut guard) = LAST_SELECTED_TEXT.lock() {
    guard.clear();
  }
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
  if let Ok(mut guard) = LAST_SELECTED_TEXT.lock() {
    *guard = selection.clone();
  }

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

/// Return the work area (taskbar-excluded) of the monitor under `probe`, in
/// physical pixels as `(left, top, right, bottom)`. Falls back to the whole
/// virtual screen if the monitor query fails. So edge detection is relative to
/// the monitor the cursor/caret is actually on, not the whole multi-monitor desktop.
#[cfg(target_os = "windows")]
unsafe fn work_area_for_point(probe: windows::Win32::Foundation::POINT) -> (i32, i32, i32, i32) {
  use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST,
  };
  use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
  };
  let hmon = MonitorFromPoint(probe, MONITOR_DEFAULTTONEAREST);
  let mut mi = MONITORINFO {
    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
    ..std::mem::zeroed()
  };
  if GetMonitorInfoW(hmon, &mut mi).as_bool() {
    (mi.rcWork.left, mi.rcWork.top, mi.rcWork.right, mi.rcWork.bottom)
  } else {
    let sx = GetSystemMetrics(SM_XVIRTUALSCREEN);
    let sy = GetSystemMetrics(SM_YVIRTUALSCREEN);
    let sw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
    let sh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
    (sx, sy, sx + sw, sy + sh)
  }
}

// Window positioning near cursor (caret-first, mouse fallback). The popup is
// placed below+right of the anchor by default, but flips above and/or to the
// left when there isn't room on that side, then is clamped into the monitor
// work area so it stays fully visible even when the cursor is in an edge zone.
#[tauri::command]
pub fn position_quick_actions(app: tauri::AppHandle) -> Result<(), String> {
  #[cfg(target_os = "windows")]
  {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{
      GetCursorPos, GetGUIThreadInfo, GetForegroundWindow, GetWindowThreadProcessId,
      GUITHREADINFO,
    };

    // Use the popup's actual size (physical px) so flip decisions are accurate;
    // fall back to the configured defaults if the window can't be measured yet.
    let (popup_w, popup_h) = app
      .get_webview_window("quick-actions")
      .and_then(|w| w.outer_size().ok())
      .map(|s| (s.width as i32, s.height as i32))
      .filter(|(w, h)| *w > 0 && *h > 0)
      .unwrap_or((380, 95));

    // Wrap everything in catch_unwind so a panic never kills the app
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
      unsafe {
        // Candidate edges for the popup, all in screen (physical) coordinates:
        //   right_x  = popup left edge when placed to the RIGHT of the anchor
        //   left_x   = popup left edge when placed to the LEFT of the anchor
        //   below_y  = popup top edge when placed BELOW the anchor
        //   above_y  = popup top edge when placed ABOVE the anchor
        let mut right_x = 0;
        let mut left_x = 0;
        let mut below_y = 0;
        let mut above_y = 0;
        let mut probe = POINT { x: 0, y: 0 };
        let mut use_caret = false;

        // Try caret position from the foreground window's GUI thread. The caret
        // rect gives us both its top and bottom, so we can flip above/below cleanly.
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
                let mut top_left = POINT { x: caret.left, y: caret.top };
                let mut bottom_left = POINT { x: caret.left, y: caret.bottom };
                let ok_top = ClientToScreen(gui.hwndCaret, &mut top_left).as_bool();
                let ok_bottom = ClientToScreen(gui.hwndCaret, &mut bottom_left).as_bool();
                if ok_top && ok_bottom {
                  let v_gap = 6;
                  right_x = bottom_left.x;
                  left_x = bottom_left.x - popup_w;
                  below_y = bottom_left.y + v_gap;
                  above_y = top_left.y - v_gap - popup_h;
                  probe = bottom_left;
                  use_caret = true;
                }
              }
            }
          }
        }

        // Fallback: mouse cursor with a small gap on every side.
        if !use_caret {
          let mut pt = POINT { x: 0, y: 0 };
          let _ = GetCursorPos(&mut pt);
          let gap = 12;
          right_x = pt.x + gap;
          left_x = pt.x - gap - popup_w;
          below_y = pt.y + gap;
          above_y = pt.y - gap - popup_h;
          probe = pt;
        }

        let (wa_left, wa_top, wa_right, wa_bottom) = work_area_for_point(probe);

        // Vertical: prefer below; flip above only when below overflows and above fits.
        let mut y = below_y;
        if below_y + popup_h > wa_bottom && above_y >= wa_top {
          y = above_y;
        }
        // Horizontal: prefer right; flip left only when right overflows and left fits.
        let mut x = right_x;
        if right_x + popup_w > wa_right && left_x >= wa_left {
          x = left_x;
        }

        // Final safety clamp so the popup is always fully on-screen near any edge.
        let x = x.max(wa_left).min(wa_right - popup_w);
        let y = y.max(wa_top).min(wa_bottom - popup_h);
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
    use windows::Win32::Foundation::POINT;
    if let Some(win) = app.get_webview_window("quick-actions") {
      let pos = win.outer_position().map_err(|e| format!("{e}"))?;
      let size = win.outer_size().map_err(|e| format!("{e}"))?;
      unsafe {
        let w = size.width as i32;
        let h = size.height as i32;
        // Clamp against the work area of the monitor the popup currently sits on
        // (using its center as the probe), matching position_quick_actions.
        let probe = POINT { x: pos.x + w / 2, y: pos.y + h / 2 };
        let (wa_left, wa_top, wa_right, wa_bottom) = work_area_for_point(probe);
        let mut x = pos.x;
        let mut y = pos.y;
        let mut changed = false;
        if x + w > wa_right { x = wa_right - w; changed = true; }
        if y + h > wa_bottom { y = wa_bottom - h; changed = true; }
        if x < wa_left { x = wa_left; changed = true; }
        if y < wa_top { y = wa_top; changed = true; }
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
  let dir = {
    #[cfg(target_os = "windows")]
    {
      std::env::var("APPDATA").ok().map(|a| {
        let mut p = std::path::PathBuf::from(a);
        p.push("AiDesktopCompanion");
        p
      })
    }
    #[cfg(not(target_os = "windows"))]
    {
      std::env::var("HOME").ok().map(|h| {
        let mut p = std::path::PathBuf::from(h);
        p.push(".config");
        p.push("AiDesktopCompanion");
        p
      })
    }
  }.ok_or_else(|| "Config directory not available".to_string())?;
  let _ = std::fs::create_dir_all(&dir);
  let path = dir.join("qa_key_log.txt");
  // Truncate if file exceeds 1MB to prevent unbounded growth
  if let Ok(meta) = std::fs::metadata(&path) {
    if meta.len() > 1_048_576 {
      let _ = std::fs::remove_file(&path);
    }
  }
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

  // Clipboard + Enigo + sleep are blocking — run on a dedicated thread to avoid starving the async runtime
  let selection = tokio::task::spawn_blocking(move || -> Result<String, String> {
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

    Ok(selection)
  }).await.map_err(|e| format!("spawn_blocking failed: {e}"))??;

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
    // local_speak_blocking is blocking — run on dedicated thread
    #[cfg(target_os = "windows")]
    {
      let voice = settings.get("tts_voice_local").and_then(|x| x.as_str()).unwrap_or("").to_string();
      tokio::task::spawn_blocking(move || {
        crate::tts::local_speak_blocking(selection, voice, rate, vol)
      }).await.map_err(|e| format!("spawn_blocking failed: {e}"))??;
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
