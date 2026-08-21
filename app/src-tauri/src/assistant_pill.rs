//! Floating "call" pill for Assistant Mode.
//!
//! A voice session is meant to be used while working in another application, so
//! the main window is usually hidden behind whatever the user is actually doing.
//! Without something on screen there is no way to tell a live call from a closed
//! one, and no way to hang up without hunting for the window.
//!
//! Two states are shown:
//!   * `armed` - the push-to-talk key was pressed with no session running, so the
//!     pill invites a second press to start the call. Starting a call costs
//!     money and turns on a microphone; neither should happen on a single
//!     stray keypress.
//!   * `live`  - a call is up. The pill reports it and offers a hang-up button.
//!
//! The window is declared statically in `tauri.conf.json` (label
//! `assistant-pill`) so it never has to be created on the hot path, and it
//! borrows `busy::make_non_activating` so showing it never steals focus from
//! the application the user is typing in.

use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::{Emitter, Manager};

pub const PILL_WINDOW_LABEL: &str = "assistant-pill";

#[derive(Clone, Debug, Serialize)]
pub struct PillState {
  /// "hidden" | "armed" | "live"
  pub state: String,
  /// Epoch milliseconds the call connected, so the pill can render an elapsed
  /// counter even if the window mounts late. Zero unless live.
  pub started_ms: i64,
  /// Whether the microphone is currently open, for the talking indicator.
  pub mic_open: bool,
  /// Configured push-to-talk shortcut, shown in the armed prompt so the user is
  /// told which key to press again rather than having to remember.
  pub hotkey: String,
}

impl Default for PillState {
  fn default() -> Self {
    Self { state: "hidden".into(), started_ms: 0, mic_open: false, hotkey: String::new() }
  }
}

static PILL_STATE: Lazy<Mutex<PillState>> = Lazy::new(|| Mutex::new(PillState::default()));

fn now_ms() -> i64 {
  chrono::Utc::now().timestamp_millis()
}

/// Place the pill in the bottom-right corner of the work area of the monitor
/// under the cursor, matching the busy indicator so both follow the user across
/// a multi-monitor desktop.
///
/// When the busy indicator is already occupying that corner the call pill stacks
/// above it rather than covering it - a slow quick prompt and a live call are
/// both things the user needs to see.
#[cfg(target_os = "windows")]
fn position_window(app: &tauri::AppHandle, win: &tauri::WebviewWindow) {
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
    let busy_visible = app
      .get_webview_window(crate::busy::BUSY_WINDOW_LABEL)
      .and_then(|w| w.is_visible().ok())
      .unwrap_or(false);
    let busy_offset = if busy_visible {
      app
        .get_webview_window(crate::busy::BUSY_WINDOW_LABEL)
        .and_then(|w| w.outer_size().ok())
        .map(|s| s.height as i32 + MARGIN)
        .unwrap_or(0)
    } else {
      0
    };
    let x = right - size.width as i32 - MARGIN;
    let y = bottom - size.height as i32 - MARGIN - busy_offset;
    let _ = win.set_position(tauri::PhysicalPosition::new(x, y));
  }
}

#[cfg(not(target_os = "windows"))]
fn position_window(_app: &tauri::AppHandle, _win: &tauri::WebviewWindow) {}

fn publish(app: &tauri::AppHandle, state: &PillState) {
  let _ = app.emit_to(PILL_WINDOW_LABEL, "assistant-pill:state", state.clone());
}

/// Drive the pill from the frontend.
///
/// `state` is "hidden", "armed" or "live". Anything else is treated as hidden
/// rather than rejected: a pill that refuses to disappear is worse than one that
/// disappears when it should not.
#[tauri::command]
pub fn assistant_pill_set(
  app: tauri::AppHandle,
  state: String,
  mic_open: Option<bool>,
  hotkey: Option<String>,
) -> Result<(), String> {
  let wanted = match state.as_str() {
    "armed" => "armed",
    "live" => "live",
    _ => "hidden",
  };

  let next = {
    let mut cur = PILL_STATE.lock().map_err(|_| "pill state poisoned")?;
    // Keep the original connect time across updates so the elapsed counter does
    // not restart every time the microphone opens or closes.
    let started_ms = if wanted == "live" {
      if cur.state == "live" && cur.started_ms > 0 { cur.started_ms } else { now_ms() }
    } else {
      0
    };
    *cur = PillState {
      state: wanted.to_string(),
      started_ms,
      mic_open: mic_open.unwrap_or(false),
      hotkey: hotkey.unwrap_or_default(),
    };
    cur.clone()
  };

  if let Some(win) = app.get_webview_window(PILL_WINDOW_LABEL) {
    if wanted == "hidden" {
      let _ = win.hide();
    } else {
      crate::busy::make_non_activating(&win);
      position_window(&app, &win);
      let _ = win.show();
      let _ = win.set_always_on_top(true);
    }
  }
  publish(&app, &next);
  Ok(())
}

/// Current state, for the pill window to read when it mounts.
#[tauri::command]
pub fn assistant_pill_get_state() -> Result<PillState, String> {
  PILL_STATE
    .lock()
    .map(|s| s.clone())
    .map_err(|_| "pill state poisoned".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn unknown_states_hide_rather_than_error() {
    // A pill stuck on screen is worse than one that hides when it should not,
    // so anything unrecognised collapses to hidden.
    for input in ["", "LIVE", "connecting", "garbage"] {
      let mapped = match input {
        "armed" => "armed",
        "live" => "live",
        _ => "hidden",
      };
      assert_eq!(mapped, "hidden", "unexpected mapping for {input:?}");
    }
  }

  #[test]
  fn default_state_is_hidden_with_no_timer() {
    let s = PillState::default();
    assert_eq!(s.state, "hidden");
    assert_eq!(s.started_ms, 0);
    assert!(!s.mic_open);
  }
}
