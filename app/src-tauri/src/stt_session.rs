// Cancellation and the "last transcript" record for speech-to-text.
//
// A local Whisper run on a large model can take a long time. Until now the
// only way out was to wait for it. `request_cancel` flips a flag that the
// Whisper decoder polls through its abort callback. It also wakes anything
// waiting in `cancelled()` so `stt_transcribe` can return straight away instead of
// sitting on a result nobody wants any more.
//
// Most transcriptions start from a hotkey with no app window open, so the
// result is also kept here and pushed to the main window. That is what lets
// the STT section show what was heard and what the AI pass turned it into.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::Emitter;
use tokio::sync::Notify;

/// Returned by `stt_transcribe` when the user stopped it. The frontend matches
/// on this text to stay quiet rather than report it as a failure.
pub const CANCELLED_MESSAGE: &str = "Transcription cancelled";

static CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);
static CANCEL_NOTIFY: Lazy<Notify> = Lazy::new(Notify::new);

/// Clear a cancel left over from an earlier run, so a stray click on a finished
/// transcription does not kill the next one.
pub fn reset_cancel() {
  CANCEL_REQUESTED.store(false, Ordering::SeqCst);
}

pub fn cancel_requested() -> bool {
  CANCEL_REQUESTED.load(Ordering::SeqCst)
}

pub fn request_cancel() {
  CANCEL_REQUESTED.store(true, Ordering::SeqCst);
  CANCEL_NOTIFY.notify_waiters();
}

/// Resolves once a cancel has been requested.
pub async fn cancelled() {
  loop {
    // Register interest before checking the flag, so a cancel that lands in
    // between is not missed.
    let notified = CANCEL_NOTIFY.notified();
    if cancel_requested() {
      return;
    }
    notified.await;
  }
}

/// Abort callback for whisper.cpp: polled between decoding steps, a `true`
/// stops the run and makes `full` return an error.
#[cfg(feature = "local-stt")]
pub unsafe extern "C" fn whisper_abort_callback(_user_data: *mut std::ffi::c_void) -> bool {
  cancel_requested()
}

#[derive(Clone, Debug, Serialize)]
pub struct LastTranscript {
  /// What the speech engine heard.
  pub original_text: String,
  /// What was handed on: the AI-corrected text when that pass ran, otherwise
  /// the same as `original_text`.
  pub final_text: String,
  pub post_process_applied: bool,
  pub post_process_error: Option<String>,
  /// Epoch milliseconds.
  pub at_ms: i64,
}

static LAST: Lazy<Mutex<Option<LastTranscript>>> = Lazy::new(|| Mutex::new(None));

pub fn record(app: &tauri::AppHandle, entry: LastTranscript) {
  if let Ok(mut last) = LAST.lock() {
    *last = Some(entry.clone());
  }
  let _ = app.emit_to("main", "stt:last-transcript", entry);
}

#[tauri::command]
pub fn stt_cancel() -> Result<(), String> {
  request_cancel();
  Ok(())
}

#[tauri::command]
pub fn stt_get_last_transcript() -> Result<Option<LastTranscript>, String> {
  Ok(LAST.lock().map(|l| l.clone()).unwrap_or(None))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn cancel_wakes_a_waiter_and_reset_clears_it() {
    reset_cancel();
    let waiter = tokio::spawn(cancelled());
    tokio::task::yield_now().await;
    request_cancel();
    waiter.await.unwrap();
    reset_cancel();
    assert!(!cancel_requested());
  }
}
