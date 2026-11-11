// Hermes Agent: long jobs handed off from Assistant Mode.
//
// The voice model is good at talking and bad at anything that takes minutes.
// Hermes (the user's long-running agent, with memory, M365 and a browser)
// exposes an OpenAI-compatible API server with an async "runs" API. The voice
// session gets one function tool, `delegate_to_hermes`; calling it starts a
// run here in Rust and returns at once, and this module follows the run in the
// background:
//
//   POST /v1/runs                 -> { run_id, status }      (202)
//   GET  /v1/runs/{id}/events     -> SSE, `data: {event, run_id, ...}`
//   GET  /v1/runs/{id}            -> pollable status (fallback when SSE drops)
//
// Progress shows in the busy pill. When the run ends the result is emitted to
// the main window (`hermes:run-finished`); a live voice session picks it up
// and acknowledges it, otherwise it stays pending for the next session and a
// notification says it is done. Approval requests are never answered here:
// the user gets a notification with the text and decides in Hermes.
//
// Requests are made from Rust rather than the webview so the API key never
// enters the page and the CSP needs no new host.

use std::sync::Mutex;
use std::time::Duration;

use futures_util::StreamExt;
use once_cell::sync::Lazy;
use serde::Serialize;
use tauri::Emitter;

pub const TOOL_NAME: &str = "delegate_to_hermes";
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8642";
const MAX_RUN_TIME: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone)]
pub struct HermesConfig {
  pub enabled: bool,
  pub base_url: String,
  pub api_key: Option<String>,
}

pub fn config_from(settings: &serde_json::Value) -> HermesConfig {
  let ar = settings.get("assistant_realtime").cloned().unwrap_or_default();
  let enabled = ar.get("hermes_enabled").and_then(|x| x.as_bool()).unwrap_or(false);
  let base_url = ar
    .get("hermes_base_url")
    .and_then(|x| x.as_str())
    .map(|s| s.trim().trim_end_matches('/').to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
  let api_key = ar
    .get("hermes_api_key")
    .and_then(|x| x.as_str())
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .or_else(|| std::env::var("HERMES_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
  HermesConfig { enabled, base_url, api_key }
}

/// `assistant_realtime.hermes_api_key`, then `HERMES_API_KEY`.
pub fn config_from_settings_or_env() -> HermesConfig {
  config_from(&crate::config::load_settings_json())
}

fn http_client(timeout: Option<Duration>) -> reqwest::Client {
  let mut b = reqwest::Client::builder().connect_timeout(Duration::from_secs(5));
  if let Some(t) = timeout { b = b.timeout(t); }
  // Hermes listens on loopback; a system proxy must not get the request.
  if crate::mcp::is_loopback_url(&config_from_settings_or_env().base_url) { b = b.no_proxy(); }
  b.build().unwrap_or_else(|_| reqwest::Client::new())
}

/// The function tool, flat Realtime shape (Gemini converts it).
pub fn tool_definition() -> serde_json::Value {
  serde_json::json!({
    "type": "function",
    "name": TOOL_NAME,
    "description": "Hand a task to Hermes, the user's long-running agent (background jobs, memory, Microsoft 365, browser, research). Use it for anything that takes more than a few seconds or many steps. It returns immediately; Hermes works in the background and the result is added to this conversation when it is done. Tell the user you handed it over, then carry on.",
    "parameters": {
      "type": "object",
      "properties": {
        "task": { "type": "string", "description": "The complete task as a standalone instruction, with every detail from the conversation (names, dates, what the user already said). Hermes cannot hear the call." },
        "speak_result": { "type": "boolean", "description": "Whether to tell the user the result out loud as soon as it arrives. Default true." }
      },
      "required": ["task"],
      "additionalProperties": false
    }
  })
}

/// The run's `instructions`: how to answer, plus what the user is looking at.
/// Text only - Hermes never receives a screenshot from this app.
pub fn build_instructions(screen_context: Option<&str>) -> String {
  let mut s = String::from(
    "This task was handed over by voice from the user's desktop assistant (AiDesktopCompanion). \
     The user is not watching this run. Finish it without asking follow-up questions where you can. \
     End with a short plain-text result of two or three sentences that can be read aloud: no markdown, no tables, no long URLs or IDs.",
  );
  if let Some(ctx) = screen_context.map(str::trim).filter(|c| !c.is_empty()) {
    s.push_str("\n\nWhat the user had on screen when they asked (from windows-commander, text only):\n");
    s.push_str(ctx);
  }
  s
}

// ---------------------------------------------------------------------------
// Run events
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum RunEvent {
  ToolStarted(String),
  Approval(String),
  Terminal { status: String, output: String, error: String },
  Other,
}

pub fn is_terminal_status(s: &str) -> bool {
  matches!(s, "completed" | "failed" | "cancelled" | "interrupted")
}

fn approval_text(v: &serde_json::Value) -> String {
  let pick = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
  match (pick("description"), pick("command")) {
    (Some(d), Some(c)) if !d.contains(&c) => format!("{d}\n{c}"),
    (Some(d), _) => d,
    (None, Some(c)) => c,
    (None, None) => "Hermes is waiting for your approval.".into(),
  }
}

/// One `data:` payload from `/v1/runs/{id}/events`.
pub fn parse_run_event(v: &serde_json::Value) -> RunEvent {
  let name = v.get("event").and_then(|x| x.as_str()).unwrap_or("");
  let text = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
  match name {
    "tool.started" => RunEvent::ToolStarted(text("tool")),
    "approval.request" => RunEvent::Approval(approval_text(v)),
    _ if name.starts_with("run.") && is_terminal_status(&name[4..]) => RunEvent::Terminal {
      status: name[4..].to_string(),
      output: text("output"),
      error: text("error"),
    },
    _ => RunEvent::Other,
  }
}

/// The pollable status (`GET /v1/runs/{id}`) as an event: terminal, waiting
/// for approval, or still going.
pub fn parse_run_status(v: &serde_json::Value) -> RunEvent {
  let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("");
  if is_terminal_status(status) {
    return RunEvent::Terminal {
      status: status.to_string(),
      output: v.get("output").and_then(|x| x.as_str()).unwrap_or("").to_string(),
      error: v.get("error").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    };
  }
  if status == "waiting_for_approval" {
    return RunEvent::Approval(v.get("approval").map(approval_text).unwrap_or_else(|| approval_text(&serde_json::Value::Null)));
  }
  RunEvent::Other
}

/// Incremental SSE decoder: feed bytes, get the JSON of each complete
/// `data:` frame. Comments (`: keepalive`) and `id:`/`event:` lines are
/// skipped; a frame that is not JSON is dropped.
#[derive(Default)]
pub struct SseDecoder {
  buf: String,
}

impl SseDecoder {
  pub fn push(&mut self, chunk: &str) -> Vec<serde_json::Value> {
    self.buf.push_str(&chunk.replace("\r\n", "\n"));
    let mut out = Vec::new();
    while let Some(idx) = self.buf.find("\n\n") {
      let frame: String = self.buf[..idx].to_string();
      self.buf.drain(..idx + 2);
      let data: Vec<&str> = frame
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(|d| d.strip_prefix(' ').unwrap_or(d))
        .collect();
      if data.is_empty() { continue; }
      if let Ok(v) = serde_json::from_str::<serde_json::Value>(&data.join("\n")) {
        out.push(v);
      }
    }
    out
  }
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutcome {
  /// Who did the work, as the voice session names it: "Hermes" or
  /// "Claude Code" (background_ask.rs delivers through here too).
  pub agent: String,
  pub run_id: String,
  pub task: String,
  /// completed | failed | cancelled | interrupted | lost
  pub status: String,
  pub output: String,
  pub error: String,
  pub speak_result: bool,
}

/// Finished runs no voice session has taken yet.
static PENDING: Lazy<Mutex<Vec<RunOutcome>>> = Lazy::new(|| Mutex::new(Vec::new()));

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
    log::warn!("hermes: notification failed: {e}");
  }
}

pub(crate) fn deliver(app: &tauri::AppHandle, outcome: RunOutcome) {
  if let Ok(mut p) = PENDING.lock() {
    p.retain(|o| o.run_id != outcome.run_id);
    p.push(outcome.clone());
    // Bounded: results nobody picks up are not worth keeping forever.
    let len = p.len();
    if len > 20 { p.drain(..len - 20); }
  }
  let _ = app.emit_to("main", "hermes:run-finished", outcome.clone());
  // A live session acknowledges within moments. Nothing did: say it here.
  let app = app.clone();
  tauri::async_runtime::spawn(async move {
    tokio::time::sleep(Duration::from_secs(4)).await;
    let still_pending = PENDING.lock().map(|p| p.iter().any(|o| o.run_id == outcome.run_id)).unwrap_or(false);
    if still_pending {
      let title = if outcome.status == "completed" { format!("{} finished", outcome.agent) } else { format!("{} did not finish", outcome.agent) };
      let body = if outcome.status == "completed" {
        format!("{}: {}", clip(&outcome.task, 60), outcome.output)
      } else {
        format!("{} ({}): {}", clip(&outcome.task, 60), outcome.status, outcome.error)
      };
      notify(&app, &title, &body);
    }
  });
}

/// Results waiting for a voice session. Not removed until acknowledged, so a
/// session that drops before it spoke them still finds them next time.
#[tauri::command]
pub fn hermes_pending_results() -> Vec<RunOutcome> {
  PENDING.lock().map(|p| p.clone()).unwrap_or_default()
}

/// A voice session has the result in its conversation.
#[tauri::command]
pub fn hermes_ack_result(run_id: String) {
  if let Ok(mut p) = PENDING.lock() { p.retain(|o| o.run_id != run_id); }
}

// ---------------------------------------------------------------------------
// Starting and following runs
// ---------------------------------------------------------------------------

/// Start a run and follow it in the background. The returned JSON is the
/// tool output the voice model sees.
pub async fn delegate(app: &tauri::AppHandle, task: String, speak_result: bool, screen_context: Option<String>) -> serde_json::Value {
  let cfg = config_from_settings_or_env();
  if !cfg.enabled {
    return serde_json::json!({ "error": "Hermes is not enabled in the Assistant settings." });
  }
  let Some(key) = cfg.api_key.clone() else {
    return serde_json::json!({ "error": "No Hermes API key is configured (Assistant settings or HERMES_API_KEY)." });
  };
  let task = task.trim().to_string();
  if task.is_empty() {
    return serde_json::json!({ "error": "task is empty" });
  }
  let body = serde_json::json!({
    "input": task,
    "instructions": build_instructions(screen_context.as_deref()),
  });
  let resp = http_client(Some(Duration::from_secs(20)))
    .post(format!("{}/v1/runs", cfg.base_url))
    .bearer_auth(&key)
    .json(&body)
    .send()
    .await;
  let resp = match resp {
    Ok(r) => r,
    Err(e) => return serde_json::json!({ "error": format!("Hermes is not reachable at {}: {}", cfg.base_url, e.without_url()) }),
  };
  let status = resp.status();
  let v: serde_json::Value = resp.json().await.unwrap_or_default();
  if !status.is_success() {
    let msg = v.pointer("/error/message").and_then(|x| x.as_str()).unwrap_or("");
    return serde_json::json!({ "error": format!("Hermes refused the run: {status} {msg}") });
  }
  let Some(run_id) = v.get("run_id").and_then(|x| x.as_str()).map(str::to_string) else {
    return serde_json::json!({ "error": "Hermes accepted the run but returned no run_id" });
  };
  log::info!("hermes: started run {run_id}");
  let app2 = app.clone();
  let rid = run_id.clone();
  let task2 = task.clone();
  tauri::async_runtime::spawn(async move {
    follow_run(app2, cfg.base_url, key, rid, task2, speak_result).await;
  });
  serde_json::json!({
    "status": "started",
    "run_id": run_id,
    "note": "Hermes is working on it in the background. Tell the user in a few words that you handed it over and will report back. Do not wait for it and do not call this tool again for the same task; the result is added to the conversation when it is ready."
  })
}

struct Follow {
  app: tauri::AppHandle,
  task: String,
  approvals_seen: Vec<String>,
}

impl Follow {
  fn label(&self, suffix: &str) -> String {
    if suffix.is_empty() { format!("Hermes: {}", clip(&self.task, 40)) } else { format!("Hermes: {}", clip(suffix, 40)) }
  }

  /// Returns the terminal outcome when the event is one.
  fn handle(&mut self, ev: RunEvent) -> Option<(String, String, String)> {
    match ev {
      RunEvent::ToolStarted(tool) if !tool.is_empty() => {
        crate::busy::relabel(&self.app, &self.label(&format!("{} ({tool})", self.task)));
        None
      }
      RunEvent::Approval(text) => {
        if !self.approvals_seen.contains(&text) {
          self.approvals_seen.push(text.clone());
          crate::busy::relabel(&self.app, &self.label("waiting for your approval"));
          notify(&self.app, "Hermes needs your approval", &format!("{}\n{}", clip(&self.task, 60), text));
          let _ = self.app.emit_to("main", "hermes:approval", serde_json::json!({ "task": self.task, "text": text }));
        }
        None
      }
      RunEvent::Terminal { status, output, error } => Some((status, output, error)),
      _ => None,
    }
  }
}

async fn follow_sse(base: &str, key: &str, run_id: &str, f: &mut Follow) -> Result<Option<(String, String, String)>, String> {
  let resp = http_client(None)
    .get(format!("{base}/v1/runs/{run_id}/events"))
    .bearer_auth(key)
    .header("Accept", "text/event-stream")
    .send()
    .await
    .map_err(|e| e.without_url().to_string())?;
  if !resp.status().is_success() {
    return Err(format!("events: {}", resp.status()));
  }
  let mut stream = resp.bytes_stream();
  let mut dec = SseDecoder::default();
  let started = std::time::Instant::now();
  // Hermes sends a keepalive comment well inside this; silence this long
  // means the stream is dead, and polling takes over.
  while let Ok(next) = tokio::time::timeout(Duration::from_secs(90), stream.next()).await {
    let Some(chunk) = next else { break };
    let chunk = chunk.map_err(|e| e.without_url().to_string())?;
    for v in dec.push(&String::from_utf8_lossy(&chunk)) {
      if let Some(done) = f.handle(parse_run_event(&v)) { return Ok(Some(done)); }
    }
    if started.elapsed() > MAX_RUN_TIME { break; }
  }
  Ok(None)
}

async fn poll_status(base: &str, key: &str, run_id: &str, f: &mut Follow, deadline: std::time::Instant) -> Option<(String, String, String)> {
  let client = http_client(Some(Duration::from_secs(15)));
  let mut failures = 0u32;
  while std::time::Instant::now() < deadline {
    match client.get(format!("{base}/v1/runs/{run_id}")).bearer_auth(key).send().await {
      Ok(r) if r.status().is_success() => {
        failures = 0;
        if let Ok(v) = r.json::<serde_json::Value>().await {
          if let Some(done) = f.handle(parse_run_status(&v)) { return Some(done); }
        }
      }
      Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
        return Some(("lost".into(), String::new(), "Hermes no longer knows this run (gateway restarted?)".into()));
      }
      _ => {
        failures += 1;
        if failures >= 40 {
          return Some(("lost".into(), String::new(), "Hermes stopped answering while the run was going".into()));
        }
      }
    }
    tokio::time::sleep(Duration::from_secs(3)).await;
  }
  Some(("lost".into(), String::new(), "gave up following the run after two hours".into()))
}

async fn follow_run(app: tauri::AppHandle, base: String, key: String, run_id: String, task: String, speak_result: bool) {
  let mut f = Follow { app: app.clone(), task: task.clone(), approvals_seen: Vec::new() };
  crate::busy::start(&app, &f.label(""));
  let deadline = std::time::Instant::now() + MAX_RUN_TIME;
  let done = match follow_sse(&base, &key, &run_id, &mut f).await {
    Ok(Some(done)) => Some(done),
    Ok(None) => None,
    Err(e) => { log::info!("hermes: event stream for {run_id} unavailable ({e}); polling"); None }
  };
  let (status, output, error) = match done {
    Some(d) => d,
    None => poll_status(&base, &key, &run_id, &mut f, deadline).await.unwrap_or_default(),
  };
  log::info!("hermes: run {run_id} ended: {status}");
  if status == "completed" {
    crate::busy::finish(&app, Ok(()));
  } else {
    crate::busy::finish(&app, Err(format!("Hermes {status}: {}", clip(if error.is_empty() { &task } else { &error }, 120))));
  }
  deliver(&app, RunOutcome { agent: "Hermes".into(), run_id, task, status, output, error, speak_result });
}

// ---------------------------------------------------------------------------
// Settings: test connection
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HermesTestResult {
  pub base_url: String,
  pub health_ok: bool,
  pub version: Option<String>,
  pub has_key: bool,
  pub auth_ok: bool,
  pub detail: String,
}

/// `/health` (no auth), then `/v1/models` with the key, so "reachable" and
/// "the key works" are told apart.
#[tauri::command]
pub async fn hermes_test_connection() -> Result<HermesTestResult, String> {
  let cfg = config_from_settings_or_env();
  let client = http_client(Some(Duration::from_secs(6)));
  let mut out = HermesTestResult {
    base_url: cfg.base_url.clone(),
    health_ok: false,
    version: None,
    has_key: cfg.api_key.is_some(),
    auth_ok: false,
    detail: String::new(),
  };
  match client.get(format!("{}/health", cfg.base_url)).send().await {
    Ok(r) if r.status().is_success() => {
      out.health_ok = true;
      if let Ok(v) = r.json::<serde_json::Value>().await {
        out.version = v.get("version").and_then(|x| x.as_str()).map(str::to_string);
      }
    }
    Ok(r) => { out.detail = format!("/health answered {}", r.status()); return Ok(out); }
    Err(e) => { out.detail = format!("not reachable: {}", e.without_url()); return Ok(out); }
  }
  let Some(key) = cfg.api_key else {
    out.detail = "reachable, but no API key is set".into();
    return Ok(out);
  };
  match client.get(format!("{}/v1/models", cfg.base_url)).bearer_auth(&key).send().await {
    Ok(r) if r.status().is_success() => { out.auth_ok = true; out.detail = "connected".into(); }
    Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED || r.status() == reqwest::StatusCode::FORBIDDEN => {
      out.detail = "reachable, but the API key was rejected".into();
    }
    Ok(r) => out.detail = format!("/v1/models answered {}", r.status()),
    Err(e) => out.detail = format!("authenticated request failed: {}", e.without_url()),
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn config_reads_assistant_settings_with_defaults() {
    let cfg = config_from(&json!({ "assistant_realtime": { "hermes_enabled": true, "hermes_base_url": " http://127.0.0.1:9000/ ", "hermes_api_key": " k " } }));
    assert!(cfg.enabled);
    assert_eq!(cfg.base_url, "http://127.0.0.1:9000");
    assert_eq!(cfg.api_key.as_deref(), Some("k"));
    let empty = config_from(&json!({}));
    assert!(!empty.enabled);
    assert_eq!(empty.base_url, DEFAULT_BASE_URL);
  }

  #[test]
  fn parses_run_events() {
    assert_eq!(parse_run_event(&json!({ "event": "tool.started", "run_id": "r", "tool": "web_search", "preview": "x" })), RunEvent::ToolStarted("web_search".into()));
    assert_eq!(
      parse_run_event(&json!({ "event": "run.completed", "run_id": "r", "output": "Done: 3 mails.", "completed": true })),
      RunEvent::Terminal { status: "completed".into(), output: "Done: 3 mails.".into(), error: String::new() }
    );
    assert_eq!(
      parse_run_event(&json!({ "event": "run.failed", "error": "boom" })),
      RunEvent::Terminal { status: "failed".into(), output: String::new(), error: "boom".into() }
    );
    assert_eq!(parse_run_event(&json!({ "event": "message.delta", "delta": "he" })), RunEvent::Other);
    assert_eq!(parse_run_event(&json!({ "event": "run.queued" })), RunEvent::Other);
    match parse_run_event(&json!({ "event": "approval.request", "description": "Send mail to Paul", "command": "send_mail", "choices": ["once", "deny"] })) {
      RunEvent::Approval(t) => assert!(t.contains("Send mail to Paul") && t.contains("send_mail")),
      other => panic!("{other:?}"),
    }
  }

  #[test]
  fn parses_pollable_status() {
    assert_eq!(parse_run_status(&json!({ "object": "hermes.run", "status": "running" })), RunEvent::Other);
    assert!(matches!(parse_run_status(&json!({ "status": "waiting_for_approval", "approval": { "description": "Delete file" } })), RunEvent::Approval(t) if t == "Delete file"));
    assert_eq!(
      parse_run_status(&json!({ "status": "interrupted", "error": "Gateway shutdown interrupted the run." })),
      RunEvent::Terminal { status: "interrupted".into(), output: String::new(), error: "Gateway shutdown interrupted the run.".into() }
    );
  }

  #[test]
  fn decodes_sse_frames_across_chunks() {
    let mut d = SseDecoder::default();
    assert!(d.push(": open\n\n").is_empty());
    assert!(d.push("id: 0\ndata: {\"event\":\"tool.sta").is_empty());
    let got = d.push("rted\",\"tool\":\"t\"}\n\n: keepalive\r\n\r\nid: 1\r\ndata: {\"event\":\"run.completed\"}\r\n\r\n");
    assert_eq!(got.len(), 2);
    assert_eq!(got[0]["tool"], "t");
    assert_eq!(got[1]["event"], "run.completed");
    assert!(d.push("data: not json\n\n").is_empty());
  }

  #[test]
  fn instructions_carry_screen_context_text_only() {
    let with = build_instructions(Some("Foreground: olk \"Inbox\""));
    assert!(with.contains("Foreground: olk"));
    let without = build_instructions(Some("   "));
    assert!(!without.contains("on screen"));
  }

  #[test]
  fn tool_definition_is_flat_realtime_shape() {
    let t = tool_definition();
    assert_eq!(t["name"], TOOL_NAME);
    assert_eq!(t["parameters"]["required"][0], "task");
  }
}
