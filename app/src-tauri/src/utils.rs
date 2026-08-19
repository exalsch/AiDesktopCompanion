// Cross-platform small utilities

#[cfg(target_os = "windows")]
pub fn ps_escape_single_quoted(s: &str) -> String {
  // In PowerShell single-quoted strings, escape ' by doubling it
  s.replace('\'', "''")
}

#[cfg(not(target_os = "windows"))]
pub fn ps_escape_single_quoted(s: &str) -> String { s.to_string() }

use std::path::PathBuf;
use std::fs;
#[cfg(target_os = "windows")]
use tauri::Emitter;

// Utility: Copy a file to destination (used by Save As flow)
pub fn copy_file_to_path(src: String, dest: String, overwrite: Option<bool>) -> Result<String, String> {
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

// Play a WAV file synchronously using PowerShell SoundPlayer on Windows
#[cfg(target_os = "windows")]
pub fn play_wav_blocking_windows(app: &tauri::AppHandle, wav_path: &str) -> Result<(), String> {
  use std::process::Command;
  // Sanity checks
  match fs::metadata(&wav_path) {
    Ok(meta) => {
      if meta.len() < 44 { // smaller than typical WAV header
        let msg = format!("synthesized WAV too small: {} bytes at {}", meta.len(), &wav_path);
        let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
        return Err(msg);
      }
    }
    Err(e) => {
      let msg = format!("synthesized WAV not found: {} ({})", &wav_path, e);
      let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
      return Err(msg);
    }
  }
  let wav_escaped = ps_escape_single_quoted(&wav_path);
  let ps = format!(
    r#"$p = New-Object System.Media.SoundPlayer '{path}'; $p.PlaySync();"#,
    path = wav_escaped
  );
  let out = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .output()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if !out.status.success() {
    let stderr_s = String::from_utf8_lossy(&out.stderr);
    let msg = if stderr_s.trim().is_empty() {
      format!("audio play failed: {}", out.status)
    } else {
      format!("audio play failed: {}\n{}", out.status, stderr_s)
    };
    let _ = app.emit("tts:error", serde_json::json!({ "message": msg }));
    return Err(msg);
  }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn play_wav_blocking_windows(_app: &tauri::AppHandle, _wav_path: &str) -> Result<(), String> {
  Err("WAV playback not implemented on this platform".into())
}

// ---------------------------
// Keyboard input simulation
// ---------------------------

/// Synthesize `Ctrl`+`ch` into whatever application currently has focus.
///
/// Every selection capture and paste-back in the app goes through this, so the
/// enigo details live in exactly one place. Errors are returned rather than
/// ignored: if input simulation is unavailable the calling flow would silently
/// read a stale clipboard and act on the wrong text.
pub fn send_ctrl_key(ch: char) -> Result<(), String> {
  send_chord(enigo::Key::Unicode(ch), false, &format!("Ctrl+{ch}"))
}

/// Send Ctrl+Shift+Home, extending the selection from the caret back to the
/// start of the document.
///
/// This is the alternative the select-all hotkey offers to Ctrl+A: in a chat
/// box or comment field it grabs exactly the text the user just typed, without
/// swallowing the conversation history that Ctrl+A would also select.
pub fn send_ctrl_shift_home() -> Result<(), String> {
  send_chord(enigo::Key::Home, true, "Ctrl+Shift+Home")
}

/// Click `key` while Control - and optionally Shift - are held.
///
/// The modifiers are always released, even when the key itself fails, so a
/// failed simulation never leaves the user with a stuck Ctrl or Shift.
fn send_chord(key: enigo::Key, shift: bool, label: &str) -> Result<(), String> {
  use enigo::{Direction, Enigo, Key, Keyboard, Settings};
  let mut enigo = Enigo::new(&Settings::default())
    .map_err(|e| format!("input simulation unavailable: {e}"))?;
  enigo
    .key(Key::Control, Direction::Press)
    .map_err(|e| format!("ctrl press failed: {e}"))?;
  if shift {
    if let Err(e) = enigo.key(Key::Shift, Direction::Press) {
      let _ = enigo.key(Key::Control, Direction::Release);
      return Err(format!("shift press failed: {e}"));
    }
  }
  let click = enigo
    .key(key, Direction::Click)
    .map_err(|e| format!("{label} failed: {e}"));
  let shift_release = if shift {
    enigo
      .key(Key::Shift, Direction::Release)
      .map_err(|e| format!("shift release failed: {e}"))
  } else {
    Ok(())
  };
  let ctrl_release = enigo
    .key(Key::Control, Direction::Release)
    .map_err(|e| format!("ctrl release failed: {e}"));
  click.and(shift_release).and(ctrl_release)
}
