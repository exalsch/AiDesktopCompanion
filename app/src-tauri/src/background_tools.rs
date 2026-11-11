// Slow tool calls in a voice call go to the background.
//
// A realtime voice session waits for every tool result before it can speak
// again, so one slow MCP tool holds the whole call. On 11.11.2026 a Claude Code
// question (through the companion bridge, which waits up to 300 s before it
// hands back a ticket) took 216 s; the user hung up after 201 s and the answer
// was lost.
//
// So every MCP tool call of a voice session runs as a task. If it finishes
// within BACKGROUND_AFTER the result goes back as usual, in the same turn. If
// not, the model gets `{status: "started", ...}` at once and the call carries
// on; the result is delivered through hermes::deliver when it lands, the same
// way as a Hermes result: injected into a live call and spoken, else a
// desktop notification and kept for the next call. Nothing here knows any
// particular server.
//
// One adapter is specific: a reply that says "still working ... ticket X"
// (the companion bridge's hand-back) is collected with the same server's
// `claude_result` tool, if it has one, so the user never has to. The voice
// session is not offered `claude_result` for that reason.
//
// Chat is not affected; it calls tools through chat.rs.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;

use crate::mcp::ClientMap;

/// How long a tool call may take before the call is handed to the background.
/// Screen tools answer in well under a second (the WebView2 lookup in
/// voice_tools.rs takes up to 5 s), so they always come back in the turn.
pub const BACKGROUND_AFTER: Duration = Duration::from_secs(8);

/// The companion bridge's ticket collector; see the module comment.
pub const TICKET_RESULT_TOOL: &str = "claude_result";
const POLL_EVERY: Duration = Duration::from_secs(15);
const POLL_FOR: Duration = Duration::from_secs(15 * 60);

/// (server, tool) pairs with a call in the background, and what it was for.
/// One at a time per tool: a second call while one runs would only compete
/// with it (the bridge, for one, answers one question per lane).
static IN_FLIGHT: Lazy<Mutex<Vec<(String, String, String, Instant)>>> = Lazy::new(|| Mutex::new(Vec::new()));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// What the call was for, in a few words: the usual request argument, else
/// the arguments as JSON.
pub fn request_summary(args: &serde_json::Value) -> String {
  for key in ["question", "task", "query", "prompt", "text", "message"] {
    if let Some(s) = args.get(key).and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
      return crate::hermes::clip(s, 300);
    }
  }
  crate::hermes::clip(&args.to_string(), 300)
}

/// What a tool said, as text: the joined text blocks of an MCP result.
fn result_text(res: &serde_json::Value) -> (String, bool) {
  let text = res
    .get("content")
    .and_then(|c| c.as_array())
    .map(|arr| {
      arr.iter()
        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
    })
    .unwrap_or_default();
  let is_error = res.get("isError").and_then(|b| b.as_bool()).unwrap_or(false);
  (text, is_error)
}

/// The ticket in a reply that means "still working", or None for a final
/// answer. Matches the companion bridge: "...call claude_result with ticket
/// abc123 ..." and "Still working, 108 seconds so far. Ticket abc123 is still
/// open."
pub fn pending_ticket(text: &str) -> Option<String> {
  let lower = text.to_lowercase();
  if !(lower.contains("still working") || lower.contains("still open")) { return None; }
  let words: Vec<&str> = text.split_whitespace().collect();
  for (i, w) in words.iter().enumerate() {
    if w.eq_ignore_ascii_case("ticket") {
      if let Some(next) = words.get(i + 1) {
        let t: String = next.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
        if !t.is_empty() { return Some(t); }
      }
    }
  }
  None
}

/// The tool output the model gets when its call goes to the background.
pub fn started_output(tool: &str, job_id: &str) -> serde_json::Value {
  serde_json::json!({
    "status": "started",
    "job_id": job_id,
    "note": format!(
      "{tool} takes longer and continues in the background. Tell the user in a few words that it is running, then carry on. \
       Do not guess its result and do not call {tool} again for this request: the result is added to the conversation \
       as a [{tool}] message when it is ready."
    ),
  })
}

/// Already running for this tool: the error the model gets instead of a second call.
pub fn busy_output(tool: &str, request: &str, since: Instant) -> serde_json::Value {
  serde_json::json!({
    "error": format!(
      "{tool} is still working on an earlier request ({} s so far): \"{}\". Its result will arrive in this conversation as a [{tool}] message. Try again after that.",
      since.elapsed().as_secs(), crate::hermes::clip(request, 120)
    )
  })
}

/// What a call came to: the MCP result, or the tool output that says it went
/// to the background (or that one is already running).
pub enum Outcome {
  Done(Result<serde_json::Value, String>),
  Background(serde_json::Value),
}

/// Run `work` (the tool call, already set up by the caller) and wait for it
/// up to BACKGROUND_AFTER. Past that, follow it in the background and deliver
/// its result when it lands.
pub async fn call_or_background<F>(
  app: &tauri::AppHandle,
  clients: &'static AsyncMutex<ClientMap>,
  server_id: String,
  tool: String,
  args: &serde_json::Value,
  work: F,
) -> Outcome
where
  F: std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'static,
{
  let request = request_summary(args);
  if let Ok(list) = IN_FLIGHT.lock() {
    if let Some((_, _, req, since)) = list.iter().find(|(s, t, _, _)| *s == server_id && *t == tool) {
      return Outcome::Background(busy_output(&tool, req, *since));
    }
  }
  let mut handle = tokio::spawn(work);
  match tokio::time::timeout(BACKGROUND_AFTER, &mut handle).await {
    Ok(joined) => return Outcome::Done(joined.unwrap_or_else(|e| Err(format!("tool task failed: {e}")))),
    Err(_) => {}
  }
  let job_id = format!("bg-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
  if let Ok(mut list) = IN_FLIGHT.lock() {
    list.push((server_id.clone(), tool.clone(), request.clone(), Instant::now()));
  }
  log::info!("voice tool {tool} ({server_id}) moved to the background as {job_id}");
  let app2 = app.clone();
  let (tool2, job2) = (tool.clone(), job_id.clone());
  tauri::async_runtime::spawn(async move {
    crate::busy::start(&app2, &format!("{tool2}: {}", crate::hermes::clip(&request, 60)));
    let first = handle.await.unwrap_or_else(|e| Err(format!("tool task failed: {e}")));
    let (status, output, error) = finish(clients, &server_id, first).await;
    if let Ok(mut list) = IN_FLIGHT.lock() {
      list.retain(|(s, t, _, _)| !(*s == server_id && *t == tool2));
    }
    log::info!("voice tool {tool2} ({job2}): {status}");
    if status == "completed" {
      crate::busy::finish(&app2, Ok(()));
    } else {
      crate::busy::finish(&app2, Err(format!("{tool2}: {}", crate::hermes::clip(&error, 120))));
    }
    crate::hermes::deliver(&app2, crate::hermes::RunOutcome {
      agent: tool2, run_id: job2, task: request, status, output, error, speak_result: true,
    });
  });
  Outcome::Background(started_output(&tool, &job_id))
}

/// Turn the finished call into (status, output, error), collecting a
/// handed-back ticket first if the reply is one.
async fn finish(clients: &AsyncMutex<ClientMap>, server_id: &str, first: Result<serde_json::Value, String>) -> (String, String, String) {
  let (mut text, mut is_error) = match first {
    Ok(res) => result_text(&res),
    Err(e) => return ("failed".into(), String::new(), e),
  };
  let deadline = Instant::now() + POLL_FOR;
  while let Some(ticket) = pending_ticket(&text) {
    if Instant::now() > deadline {
      return ("lost".into(), String::new(), format!("no result after {} minutes (ticket {ticket})", POLL_FOR.as_secs() / 60));
    }
    tokio::time::sleep(POLL_EVERY).await;
    match crate::mcp::call_tool(clients, server_id, TICKET_RESULT_TOOL, serde_json::json!({ "ticket": ticket })).await {
      Ok(res) => { let r = result_text(&res); text = r.0; is_error = r.1; }
      // No collector on this server (or it went away): hand over what we have.
      Err(e) => return ("failed".into(), String::new(), format!("{text} (could not collect the result: {e})")),
    }
  }
  if is_error { ("failed".into(), String::new(), text) } else { ("completed".into(), text, String::new()) }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_tickets_from_both_bridge_replies() {
    assert_eq!(
      pending_ticket("Claude Code is still working on that after 300 seconds. Tell the user it is still going, then call claude_result with ticket a1c7eb in about fifteen seconds.").as_deref(),
      Some("a1c7eb")
    );
    assert_eq!(pending_ticket("Still working, 108 seconds so far. Ticket a1c7eb is still open.").as_deref(), Some("a1c7eb"));
    assert_eq!(pending_ticket("Nothing formal to prepare. The invite has no agenda."), None);
    // A final answer that happens to mention a ticket is still final.
    assert_eq!(pending_ticket("Your ticket CS00432401 was closed yesterday."), None);
  }

  #[test]
  fn joins_text_blocks_and_reads_the_error_flag() {
    let res = serde_json::json!({ "content": [{ "type": "text", "text": "a" }, { "type": "image" }, { "type": "text", "text": "b" }], "isError": true });
    assert_eq!(result_text(&res), ("a\nb".to_string(), true));
  }

  #[test]
  fn summarises_the_request() {
    assert_eq!(request_summary(&serde_json::json!({ "question": " Prepare? ", "lane": "rob" })), "Prepare?");
    assert_eq!(request_summary(&serde_json::json!({ "path": "C:/x" })), "{\"path\":\"C:/x\"}");
  }

  #[test]
  fn started_output_names_the_tool_and_the_message_prefix() {
    let v = started_output("ask_claude", "bg-1");
    assert_eq!(v["status"], "started");
    assert!(v["note"].as_str().unwrap().contains("[ask_claude] message"));
  }
}
