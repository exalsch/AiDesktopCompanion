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

/// One step of a keystroke-mode insertion: a run of literal text, or a
/// structural key that has no Unicode keystroke of its own.
///
/// Synthetic Unicode input has no way to express a line break - sending U+000A
/// as a character event is silently dropped by most applications, so a
/// multi-line result would arrive as a single run-on line. Newlines and tabs
/// therefore have to be lifted out of the text and sent as real key presses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypedSegment {
  Text(String),
  Newline,
  Tab,
}

/// Split `text` into the segments `type_text` sends, folding `\r\n` and a lone
/// `\r` into a single newline so Windows-style input does not press Return
/// twice per line.
pub fn split_typed_segments(text: &str) -> Vec<TypedSegment> {
  let mut segments: Vec<TypedSegment> = Vec::new();
  let mut buf = String::new();
  let mut chars = text.chars().peekable();
  while let Some(ch) = chars.next() {
    match ch {
      '\r' | '\n' => {
        // Swallow the '\n' of a "\r\n" pair so it counts as one line break.
        if ch == '\r' && chars.peek() == Some(&'\n') {
          chars.next();
        }
        if !buf.is_empty() {
          segments.push(TypedSegment::Text(std::mem::take(&mut buf)));
        }
        segments.push(TypedSegment::Newline);
      }
      '\t' => {
        if !buf.is_empty() {
          segments.push(TypedSegment::Text(std::mem::take(&mut buf)));
        }
        segments.push(TypedSegment::Tab);
      }
      _ => buf.push(ch),
    }
  }
  if !buf.is_empty() {
    segments.push(TypedSegment::Text(buf));
  }
  segments
}

/// Type `text` into whatever window currently has focus by simulating key
/// presses, leaving the clipboard untouched.
///
/// The alternative to the clipboard/Ctrl+V insertion path: it costs the user
/// nothing in clipboard contents or clipboard history, but every newline is a
/// real Return press, which submits the message in chat-style inputs. That
/// trade-off is why clipboard insertion stays the default.
pub fn type_text(text: &str) -> Result<(), String> {
  use enigo::{Direction, Enigo, Key, Keyboard, Settings};
  let segments = split_typed_segments(text);
  if segments.is_empty() {
    return Ok(());
  }
  let mut enigo = Enigo::new(&Settings::default())
    .map_err(|e| format!("input simulation unavailable: {e}"))?;
  for segment in segments {
    match segment {
      TypedSegment::Text(s) => enigo
        .text(&s)
        .map_err(|e| format!("typing text failed: {e}"))?,
      TypedSegment::Newline => enigo
        .key(Key::Return, Direction::Click)
        .map_err(|e| format!("Return failed: {e}"))?,
      TypedSegment::Tab => enigo
        .key(Key::Tab, Direction::Click)
        .map_err(|e| format!("Tab failed: {e}"))?,
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::{split_typed_segments, TypedSegment};

  fn text(s: &str) -> TypedSegment {
    TypedSegment::Text(s.to_string())
  }

  #[test]
  fn plain_text_is_one_segment() {
    assert_eq!(split_typed_segments("hello world"), vec![text("hello world")]);
  }

  #[test]
  fn empty_text_produces_no_segments() {
    assert!(split_typed_segments("").is_empty());
  }

  #[test]
  fn newlines_become_return_presses() {
    assert_eq!(
      split_typed_segments("a\nb"),
      vec![text("a"), TypedSegment::Newline, text("b")]
    );
  }

  #[test]
  fn crlf_counts_as_a_single_newline() {
    assert_eq!(
      split_typed_segments("a\r\nb"),
      vec![text("a"), TypedSegment::Newline, text("b")]
    );
  }

  #[test]
  fn lone_carriage_return_counts_as_a_newline() {
    assert_eq!(
      split_typed_segments("a\rb"),
      vec![text("a"), TypedSegment::Newline, text("b")]
    );
  }

  #[test]
  fn blank_lines_are_preserved() {
    assert_eq!(
      split_typed_segments("a\n\nb"),
      vec![
        text("a"),
        TypedSegment::Newline,
        TypedSegment::Newline,
        text("b")
      ]
    );
  }

  #[test]
  fn leading_and_trailing_newlines_are_preserved() {
    assert_eq!(
      split_typed_segments("\na\n"),
      vec![TypedSegment::Newline, text("a"), TypedSegment::Newline]
    );
  }

  #[test]
  fn tabs_become_tab_presses() {
    assert_eq!(
      split_typed_segments("a\tb"),
      vec![text("a"), TypedSegment::Tab, text("b")]
    );
  }

  #[test]
  fn non_ascii_text_survives_intact() {
    assert_eq!(
      split_typed_segments("naive - \u{65e5}\u{672c}\u{8a9e} \u{1f642}"),
      vec![text("naive - \u{65e5}\u{672c}\u{8a9e} \u{1f642}")]
    );
  }
}
