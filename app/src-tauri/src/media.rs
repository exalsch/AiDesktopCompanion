//! Pause whatever is playing while the user is talking, and put it back after.
//!
//! Dictating over music is a losing fight: the microphone hears both, and the
//! transcript pays for it. This pauses the current media session for the length
//! of a recording, then resumes it.
//!
//! It drives the same system media transport controls that the keyboard's
//! play/pause key does, which is what Spotify, browsers, VLC and the rest
//! register with. Deliberately *not* the media key itself: that is a toggle with
//! no way to ask what state it is in, so pressing it when nothing was playing
//! starts music instead of stopping it. Asking the session manager first means
//! we only ever pause something already playing, and only resume something we
//! paused.
//!
//! Requests nest. Assistant Mode's push-to-talk can fire while an STT recording
//! is still open, and the second release must not resume playback while the
//! first is still recording, so a counter holds the pause until the last one
//! lets go.

use std::sync::Mutex;

use once_cell::sync::Lazy;

/// How many callers currently want playback held. Playback resumes when this
/// returns to zero, not on the first release.
static HOLD_COUNT: Lazy<Mutex<u32>> = Lazy::new(|| Mutex::new(0));
/// Whether this app is the one that paused. Without it, resuming would start
/// music the user had deliberately stopped themselves.
static WE_PAUSED: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));

#[cfg(target_os = "windows")]
mod imp {
  use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
  };

  /// AsyncStatus::Completed. The enum is not re-exported under
  /// `windows::Foundation` and naming it means pinning windows-future in
  /// lockstep with the `windows` crate, so the ABI value is used directly.
  const ASYNC_COMPLETED: i32 = 1;
  const ASYNC_STARTED: i32 = 0;

  /// Block until a WinRT async operation finishes.
  ///
  /// A macro rather than a generic function so the operation's type never has
  /// to be named. These settle in microseconds - the session manager is a local
  /// RPC hop, not I/O - and the bound stops a wedged media app hanging a
  /// recording.
  macro_rules! wait_for {
    ($op:expr, $what:expr) => {{
      let op = $op;
      let start = std::time::Instant::now();
      loop {
        let status = op.Status().map_err(|e| format!("{}: status failed: {e}", $what))?.0;
        if status == ASYNC_COMPLETED {
          break op.GetResults().map_err(|e| format!("{}: {e}", $what))?;
        }
        if status != ASYNC_STARTED {
          return Err(format!("{}: ended with status {status}", $what));
        }
        if start.elapsed() > std::time::Duration::from_millis(1500) {
          return Err(format!("{}: media session did not respond in time", $what));
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
      }
    }};
  }

  /// Pause the current media session if something is actually playing.
  ///
  /// Returns whether it paused anything. Absence of a session is normal - most
  /// of the time nothing is playing - so it is not an error.
  pub fn pause_if_playing() -> Result<bool, String> {
    let manager = wait_for!(
      GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|e| format!("media session manager unavailable: {e}"))?,
      "media session manager"
    );

    let session = match manager.GetCurrentSession() {
      Ok(s) => s,
      // No app currently owns the transport controls.
      Err(_) => return Ok(false),
    };

    let status = session
      .GetPlaybackInfo()
      .and_then(|info| info.PlaybackStatus())
      .map_err(|e| format!("could not read playback status: {e}"))?;

    if status != GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing {
      return Ok(false);
    }

    let paused = wait_for!(
      session.TryPauseAsync().map_err(|e| format!("pause request failed: {e}"))?,
      "pause"
    );
    Ok(paused)
  }

  /// Resume the current media session.
  pub fn resume() -> Result<bool, String> {
    let manager = wait_for!(
      GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|e| format!("media session manager unavailable: {e}"))?,
      "media session manager"
    );

    let session = match manager.GetCurrentSession() {
      Ok(s) => s,
      // The player closed while we held the pause; nothing to put back.
      Err(_) => return Ok(false),
    };

    let played = wait_for!(
      session.TryPlayAsync().map_err(|e| format!("resume request failed: {e}"))?,
      "resume"
    );
    Ok(played)
  }
}

#[cfg(not(target_os = "windows"))]
mod imp {
  pub fn pause_if_playing() -> Result<bool, String> { Ok(false) }
  pub fn resume() -> Result<bool, String> { Ok(false) }
}

/// Whether the caller's feature is allowed to pause playback.
///
/// The decision sits here rather than in the frontend so both callers read the
/// same settings keys, and so a caller cannot hold playback it never asked for.
fn enabled_for(reason: &str) -> bool {
  let key = match reason {
    "stt" => "pause_media_on_stt",
    "assistant" => "pause_media_on_assistant",
    _ => return false,
  };
  crate::config::load_settings_json()
    .get(key)
    .and_then(|x| x.as_bool())
    .unwrap_or(false)
}

/// Ask for playback to be held for the given feature ("stt" or "assistant").
///
/// Returns whether a hold was registered. Callers must only call
/// `media_release` when this returned true, or the counter drifts and playback
/// resumes while something is still recording.
///
/// Errors from the media session are swallowed into a successful hold on
/// purpose: failing to pause background music must never stop a recording.
#[tauri::command]
pub fn media_hold(reason: String) -> Result<bool, String> {
  if !enabled_for(&reason) {
    return Ok(false);
  }
  let first = {
    let mut count = HOLD_COUNT.lock().map_err(|_| "media hold count poisoned")?;
    let was_zero = *count == 0;
    *count = count.saturating_add(1);
    was_zero
  };
  if !first {
    // Already held by another recording; nothing more to do.
    return Ok(true);
  }

  let paused = imp::pause_if_playing().unwrap_or(false);
  *WE_PAUSED.lock().map_err(|_| "media pause flag poisoned")? = paused;
  Ok(true)
}

/// Release a hold, resuming playback once the last one is released.
#[tauri::command]
pub fn media_release() -> Result<bool, String> {
  let last = {
    let mut count = HOLD_COUNT.lock().map_err(|_| "media hold count poisoned")?;
    *count = count.saturating_sub(1);
    *count == 0
  };
  if !last {
    return Ok(false);
  }

  let mut flag = WE_PAUSED.lock().map_err(|_| "media pause flag poisoned")?;
  if !*flag {
    // We never paused anything, so there is nothing of ours to resume.
    return Ok(false);
  }
  *flag = false;
  drop(flag);
  Ok(imp::resume().unwrap_or(false))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn reset() {
    *HOLD_COUNT.lock().unwrap() = 0;
    *WE_PAUSED.lock().unwrap() = false;
  }

  #[test]
  fn releasing_more_than_held_cannot_underflow() {
    reset();
    // A stray release - a recording that errored before it started, say - must
    // not push the counter below zero and strand the next hold.
    let _ = media_release();
    let _ = media_release();
    assert_eq!(*HOLD_COUNT.lock().unwrap(), 0);
  }

  #[test]
  fn nested_holds_resume_only_on_the_last_release() {
    reset();
    // Simulate two overlapping recordings without touching the real session.
    *HOLD_COUNT.lock().unwrap() = 2;
    *WE_PAUSED.lock().unwrap() = true;

    let resumed_early = media_release().unwrap();
    assert!(!resumed_early, "first release must not resume playback");
    assert_eq!(*HOLD_COUNT.lock().unwrap(), 1);
    assert!(*WE_PAUSED.lock().unwrap(), "pause flag must survive until the last release");
  }

  #[test]
  fn a_hold_we_did_not_cause_does_not_resume() {
    reset();
    // Nothing was playing, so nothing was paused: releasing must leave the
    // user's own paused music alone rather than starting it.
    *HOLD_COUNT.lock().unwrap() = 1;
    *WE_PAUSED.lock().unwrap() = false;
    assert!(!media_release().unwrap());
  }
}
