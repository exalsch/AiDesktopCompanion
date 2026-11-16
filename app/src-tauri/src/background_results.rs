// Results of voice work that finished in the background.
//
// A voice tool call that runs past its time limit goes to the background
// (background_tools.rs) so the call can carry on. When it finishes, the result
// comes through here: it is kept in a pending list and emitted to the main
// window (`background:job-finished`). A live voice session takes it into the
// conversation and acknowledges it; when nothing acknowledges it within a few
// seconds, a desktop notification says it is done and the result waits for the
// next session, which collects it on connect.

use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::Emitter;

/// How many unacknowledged results are kept. Results nobody picks up are not
/// worth keeping forever.
const MAX_PENDING: usize = 20;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundResult {
  /// Who did the work, as the voice session names it: the voice tool whose
  /// call went to the background.
  pub agent: String,
  pub run_id: String,
  pub task: String,
  /// completed | failed | cancelled | lost
  pub status: String,
  pub output: String,
  pub error: String,
  pub speak_result: bool,
}

/// Finished jobs no voice session has taken yet.
static PENDING: Lazy<Mutex<Vec<BackgroundResult>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Collapse whitespace and cut to `max` characters, marking the cut.
pub(crate) fn clip(s: &str, max: usize) -> String {
  let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
  if s.chars().count() <= max { return s; }
  let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
  out.push_str("...");
  out
}

pub fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
  use tauri_plugin_notification::NotificationExt;
  if let Err(e) = app.notification().builder().title(title).body(clip(body, 240)).show() {
    log::warn!("background results: notification failed: {e}");
  }
}

/// Add `result` to `pending`, replacing an older entry for the same run and
/// dropping the oldest beyond `MAX_PENDING`.
fn push_pending(pending: &mut Vec<BackgroundResult>, result: BackgroundResult) {
  pending.retain(|o| o.run_id != result.run_id);
  pending.push(result);
  let len = pending.len();
  if len > MAX_PENDING { pending.drain(..len - MAX_PENDING); }
}

pub(crate) fn deliver(app: &tauri::AppHandle, result: BackgroundResult) {
  if let Ok(mut p) = PENDING.lock() {
    push_pending(&mut p, result.clone());
  }
  let _ = app.emit_to("main", "background:job-finished", result.clone());
  // A live session acknowledges within moments. Nothing did: say it here.
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    tokio::time::sleep(Duration::from_secs(4)).await;
    let still_pending = PENDING.lock().map(|p| p.iter().any(|o| o.run_id == result.run_id)).unwrap_or(false);
    if still_pending {
      let title = if result.status == "completed" { format!("{} finished", result.agent) } else { format!("{} did not finish", result.agent) };
      let body = if result.status == "completed" {
        format!("{}: {}", clip(&result.task, 60), result.output)
      } else {
        format!("{} ({}): {}", clip(&result.task, 60), result.status, result.error)
      };
      notify(&app, &title, &body);
    }
  });
}

/// Results waiting for a voice session. Not removed until acknowledged, so a
/// session that drops before it spoke them still finds them next time.
#[tauri::command]
pub fn background_pending_results() -> Vec<BackgroundResult> {
  PENDING.lock().map(|p| p.clone()).unwrap_or_default()
}

/// A voice session has the result in its conversation.
#[tauri::command]
pub fn background_ack_result(run_id: String) {
  if let Ok(mut p) = PENDING.lock() { p.retain(|o| o.run_id != run_id); }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn result(id: &str) -> BackgroundResult {
    BackgroundResult {
      agent: "ask_claude".into(),
      run_id: id.into(),
      task: "t".into(),
      status: "completed".into(),
      output: "o".into(),
      error: String::new(),
      speak_result: true,
    }
  }

  #[test]
  fn clips_and_collapses_whitespace() {
    assert_eq!(clip("a  b\n c", 10), "a b c");
    assert_eq!(clip("abcdefghijk", 8), "abcde...");
  }

  #[test]
  fn pending_list_replaces_same_run_and_stays_bounded() {
    let mut p = Vec::new();
    push_pending(&mut p, result("a"));
    let mut newer = result("a");
    newer.output = "newer".into();
    push_pending(&mut p, newer);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].output, "newer");
    for i in 0..30 { push_pending(&mut p, result(&format!("r{i}"))); }
    assert_eq!(p.len(), MAX_PENDING);
    assert_eq!(p.last().unwrap().run_id, "r29");
  }

  #[test]
  fn serializes_camel_case_for_the_webview() {
    let v = serde_json::to_value(result("x")).unwrap();
    assert_eq!(v["runId"], "x");
    assert_eq!(v["speakResult"], true);
  }
}
