// Screen capture utilities for Windows (with stubs for other platforms)
// Exposes helpers used by Tauri commands in lib.rs
use tauri::{Manager, Emitter};

// Return the Windows virtual desktop bounds (spanning all monitors).
// x/y can be negative if a monitor is to the left/top of the primary.
pub fn get_virtual_screen_bounds() -> Result<serde_json::Value, String> {
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
pub fn size_overlay_to_virtual_screen(app: tauri::AppHandle) -> Result<(), String> {
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

// Capture a region of the screen and save to a temporary PNG. Returns the file path.
// On success also opens the main window and emits `image:capture` with { path }.
pub fn capture_region(app: tauri::AppHandle, x: i32, y: i32, width: i32, height: i32) -> Result<String, String> {
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
