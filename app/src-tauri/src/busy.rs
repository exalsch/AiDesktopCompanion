// Floating "busy" indicator window.
//
// Several flows (quick prompts triggered by hotkey, TTS on selection, STT
// transcription) run entirely in the background: the Quick Actions popup is
// already hidden and the main window may never be shown. When the OpenAI call
// is slow — or times out after two minutes — the user has no way of telling
// whether the app is still working. This module drives a small always-on-top
// pill window that reports the current background operation and, on failure,
// the error that would otherwise be swallowed.
//
// The window itself is declared statically in `tauri.conf.json` (label
// `busy-indicator`) so it never has to be created on the hot path.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{Emitter, Manager};

pub const BUSY_WINDOW_LABEL: &str = "busy-indicator";

#[derive(Clone, Debug, Serialize)]
pub struct BusyState {
  /// "idle" | "running" | "error"
  pub state: String,
  /// Short description of the running operation, e.g. "Quick Prompt 3".
  pub label: String,
  /// Error text when `state == "error"`, empty otherwise.
  pub detail: String,
  /// Wall-clock epoch milliseconds when the operation started, so the window
  /// can render an elapsed-time counter even if it mounts late.
  pub started_ms: i64,
}

impl Default for BusyState {
  fn default() -> Self {
    Self { state: "idle".into(), label: String::new(), detail: String::new(), started_ms: 0 }
  }
}

static BUSY_STATE: Lazy<Mutex<BusyState>> = Lazy::new(|| Mutex::new(BusyState::default()));
/// Number of operations currently running. The window stays visible until the
/// last one finishes so overlapping actions don't hide each other's indicator.
static BUSY_COUNT: Lazy<Mutex<u32>> = Lazy::new(|| Mutex::new(0));
/// Whether the currently running batch of operations should stay invisible
/// (indicator disabled in settings, or the main window is already in front).
static SUPPRESSED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

fn is_suppressed() -> bool {
  SUPPRESSED.lock().map(|s| *s).unwrap_or(false)
}

fn now_ms() -> i64 {
  chrono::Utc::now().timestamp_millis()
}

fn indicator_enabled() -> bool {
  crate::config::load_settings_json()
    .get("show_busy_indicator")
    .and_then(|x| x.as_bool())
    .unwrap_or(true)
}

/// The pill exists for operations the user cannot otherwise see. When the main
/// window is right there in front of them it already renders its own busy and
/// error state, so a second floating indicator is just noise.
fn should_suppress(app: &tauri::AppHandle) -> bool {
  if !indicator_enabled() {
    return true;
  }
  match app.get_webview_window("main") {
    Some(w) => w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false),
    None => false,
  }
}

fn publish(app: &tauri::AppHandle, state: &BusyState) {
  let _ = app.emit_to(BUSY_WINDOW_LABEL, "busy:state", state.clone());
}

/// Place the pill in the bottom-right corner of the work area of the monitor
/// under the cursor, so it follows the user across a multi-monitor desktop.
#[cfg(target_os = "windows")]
fn position_window(win: &tauri::WebviewWindow) {
  use windows::Win32::Foundation::POINT;
  use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

  let size = match win.outer_size() {
    Ok(s) => s,
    Err(_) => return,
  };
  unsafe {
    let mut pt = POINT { x: 0, y: 0 };
    if GetCursorPos(&mut pt).is_err() {
      return;
    }
    let (_l, _t, right, bottom) = crate::quick_actions::work_area_for_point(pt);
    const MARGIN: i32 = 16;
    let x = right - size.width as i32 - MARGIN;
    let y = bottom - size.height as i32 - MARGIN;
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
  }
}

#[cfg(not(target_os = "windows"))]
fn position_window(_win: &tauri::WebviewWindow) {}

/// Apply `WS_EX_NOACTIVATE` so showing the pill never steals focus from the
/// application the user is typing in — the whole point of this window is to be
/// informative without interrupting.
#[cfg(target_os = "windows")]
pub fn make_non_activating(win: &tauri::WebviewWindow) {
  use windows::Win32::Foundation::HWND;
  use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
  };
  if let Ok(handle) = win.hwnd() {
    unsafe {
      let hwnd = HWND(handle.0 as _);
      let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
      let next = cur | (WS_EX_NOACTIVATE.0 as isize) | (WS_EX_TOOLWINDOW.0 as isize);
      SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
    }
  }
}

#[cfg(not(target_os = "windows"))]
pub fn make_non_activating(_win: &tauri::WebviewWindow) {}

/// Mark the start of a background operation and show the indicator.
pub fn start(app: &tauri::AppHandle, label: &str) {
  let first = {
    let mut count = BUSY_COUNT.lock().unwrap();
    let was_idle = *count == 0;
    *count = count.saturating_add(1);
    was_idle
  };
  if first {
    if let Ok(mut s) = SUPPRESSED.lock() {
      *s = should_suppress(app);
    }
  }
  if is_suppressed() {
    return;
  }
  let state = BusyState {
    state: "running".into(),
    label: label.to_string(),
    detail: String::new(),
    started_ms: now_ms(),
  };
  if let Ok(mut cur) = BUSY_STATE.lock() {
    *cur = state.clone();
  }
  if let Some(win) = app.get_webview_window(BUSY_WINDOW_LABEL) {
    make_non_activating(&win);
    position_window(&win);
    let _ = win.show();
    let _ = win.set_always_on_top(true);
  }
  publish(app, &state);
}

/// Mark the end of a background operation.
///
/// On success the indicator hides as soon as the last operation completes. On
/// failure it switches to an error state and stays visible; the window hides
/// itself again via `busy_hide` after a few seconds, or when clicked.
pub fn finish(app: &tauri::AppHandle, result: Result<(), String>) {
  let remaining = {
    let mut count = BUSY_COUNT.lock().unwrap();
    *count = count.saturating_sub(1);
    *count
  };
  match result {
    Ok(()) => {
      if remaining == 0 {
        if let Ok(mut cur) = BUSY_STATE.lock() {
          *cur = BusyState::default();
        }
        publish(app, &BusyState::default());
        if let Some(win) = app.get_webview_window(BUSY_WINDOW_LABEL) {
          let _ = win.hide();
        }
      }
    }
    Err(err) => {
      if is_suppressed() {
        return;
      }
      let state = BusyState {
        state: "error".into(),
        label: BUSY_STATE.lock().map(|s| s.label.clone()).unwrap_or_default(),
        detail: err,
        started_ms: now_ms(),
      };
      if let Ok(mut cur) = BUSY_STATE.lock() {
        *cur = state.clone();
      }
      if let Some(win) = app.get_webview_window(BUSY_WINDOW_LABEL) {
        make_non_activating(&win);
        position_window(&win);
        let _ = win.show();
        let _ = win.set_always_on_top(true);
      }
      publish(app, &state);
    }
  }
}

/// Convenience wrapper: run `fut` with the indicator showing, reporting any
/// error through the indicator while still returning it to the caller.
pub async fn with_indicator<T, F>(app: &tauri::AppHandle, label: &str, fut: F) -> Result<T, String>
where
  F: std::future::Future<Output = Result<T, String>>,
{
  start(app, label);
  let result = fut.await;
  match &result {
    Ok(_) => finish(app, Ok(())),
    Err(e) => finish(app, Err(e.clone())),
  }
  result
}

/// Current state, so the indicator window can render correctly even when its
/// webview finishes loading after the operation already started.
#[tauri::command]
pub fn busy_get_state() -> Result<BusyState, String> {
  Ok(BUSY_STATE.lock().map(|s| s.clone()).unwrap_or_default())
}

/// Hide the indicator (used by the window itself to dismiss an error).
#[tauri::command]
pub fn busy_hide(app: tauri::AppHandle) -> Result<(), String> {
  if let Ok(mut cur) = BUSY_STATE.lock() {
    *cur = BusyState::default();
  }
  if let Some(win) = app.get_webview_window(BUSY_WINDOW_LABEL) {
    let _ = win.hide();
  }
  Ok(())
}
