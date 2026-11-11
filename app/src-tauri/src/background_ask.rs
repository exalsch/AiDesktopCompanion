// Claude Code questions from a voice call run in the background.
//
// The companion bridge (MCP server "Rob", C:\Rob\companion-bridge) answers
// `ask_claude` synchronously for up to `defaultTimeoutSeconds` (300) before it
// hands back a ticket. In a voice call that kept the tool call open for
// minutes: on 11.11.2026 a "do we need to prepare" question took 216 s, the
// user hung up after 201 s and the answer was lost. The 300 s exist because
// voice models did not collect tickets with `claude_result`, so lowering it
// would lose answers the other way.
//
// So a voice session's `ask_claude` is answered here at once ("started") and
// the real call runs in a task: it asks with the bridge's maximum wait, then
// collects the ticket itself if one comes back. The answer goes out through
// hermes::deliver, the same way as a Hermes result: injected into a live call
// and spoken, else a desktop notification and kept for the next call.
//
// Chat is not affected; it calls the bridge directly through chat.rs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use tokio::sync::Mutex as AsyncMutex;

use crate::mcp::ClientMap;

pub const ASK_TOOL: &str = "ask_claude";
pub const RESULT_TOOL: &str = "claude_result";
pub const AGENT: &str = "Claude Code";

/// What the bridge's ask waits at most before handing back a ticket
/// (its `maxTimeoutSeconds`). The task then polls.
const ASK_WAIT_SECONDS: u64 = 900;
const POLL_EVERY: Duration = Duration::from_secs(15);
const POLL_FOR: Duration = Duration::from_secs(15 * 60);

/// The question in flight, if any: the bridge answers one question per lane
/// at a time, and a second ask would only come back "busy".
static IN_FLIGHT: Lazy<Mutex<Option<(String, Instant)>>> = Lazy::new(|| Mutex::new(None));
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Whether a voice call's `ask_claude` goes to the background (every server
/// that offers a tool of that name: the bridge is registered under any id).
pub fn handles(tool_name: &str) -> bool {
  tool_name == ASK_TOOL
}

/// The ask_claude description a voice model gets: the bridge's own text plus
/// how it behaves in a call. claude_result is not offered in a call at all.
pub fn voice_description(original: &str) -> String {
  format!(
    "{original} IN THIS VOICE CALL it runs in the background: it returns at once, \
     you say in a few words that Claude Code is on it and carry on, and the answer \
     arrives later as a [Claude Code] message. Never call it twice for the same question."
  )
}

/// What the bridge said, as text: the joined text blocks of an MCP result.
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

/// The ticket in a bridge reply that means "still working", or None for a
/// final answer. Matches mcp-server.js: "...call claude_result with ticket
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

/// Start the question in the background. The returned JSON is the tool
/// output the voice model sees.
pub async fn start(
  app: &tauri::AppHandle,
  clients: &'static AsyncMutex<ClientMap>,
  server_id: String,
  args: serde_json::Value,
) -> serde_json::Value {
  let question = args.get("question").and_then(|q| q.as_str()).unwrap_or("").trim().to_string();
  if question.is_empty() {
    return serde_json::json!({ "error": "question is empty" });
  }
  {
    let mut slot = match IN_FLIGHT.lock() { Ok(s) => s, Err(_) => return serde_json::json!({ "error": "internal lock poisoned" }) };
    if let Some((q, since)) = slot.as_ref() {
      return serde_json::json!({
        "error": format!(
          "Claude Code is still working on the earlier question ({} s so far): \"{}\". Its answer will arrive in this conversation as a [Claude Code] message. Ask again after that.",
          since.elapsed().as_secs(), crate::hermes::clip(q, 120)
        )
      });
    }
    *slot = Some((question.clone(), Instant::now()));
  }
  let id = format!("cc-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed));
  let mut call_args = args.clone();
  if let Some(o) = call_args.as_object_mut() {
    o.insert("timeout_seconds".into(), serde_json::json!(ASK_WAIT_SECONDS));
  }
  log::info!("background ask {id}: started");
  let app2 = app.clone();
  let id2 = id.clone();
  tauri::async_runtime::spawn(async move {
    crate::busy::start(&app2, &format!("Claude Code: {}", crate::hermes::clip(&question, 60)));
    let (status, output, error) = run(clients, &server_id, call_args).await;
    if let Ok(mut slot) = IN_FLIGHT.lock() { *slot = None; }
    log::info!("background ask {id2}: {status}");
    if status == "completed" {
      crate::busy::finish(&app2, Ok(()));
    } else {
      crate::busy::finish(&app2, Err(format!("Claude Code: {}", crate::hermes::clip(&error, 120))));
    }
    crate::hermes::deliver(&app2, crate::hermes::RunOutcome {
      agent: AGENT.into(), run_id: id2, task: question, status, output, error, speak_result: true,
    });
  });
  serde_json::json!({
    "status": "started",
    "job_id": id,
    "note": "Claude Code is working on it in the background. Tell the user in a few words that Claude Code is on it, then carry on. Do not guess the answer and do not call ask_claude again for this question: the answer is added to the conversation as a [Claude Code] message when it is ready."
  })
}

/// Ask, then collect a handed-back ticket. Returns (status, output, error).
async fn run(clients: &AsyncMutex<ClientMap>, server_id: &str, args: serde_json::Value) -> (String, String, String) {
  let first = match crate::mcp::call_tool(clients, server_id, ASK_TOOL, args).await {
    Ok(res) => result_text(&res),
    Err(e) => return ("failed".into(), String::new(), format!("could not reach Claude Code: {e}")),
  };
  let (mut text, mut is_error) = first;
  let deadline = Instant::now() + POLL_FOR;
  while let Some(ticket) = pending_ticket(&text) {
    if Instant::now() > deadline {
      return ("lost".into(), String::new(), format!("no answer after {} minutes (ticket {ticket})", POLL_FOR.as_secs() / 60));
    }
    tokio::time::sleep(POLL_EVERY).await;
    match crate::mcp::call_tool(clients, server_id, RESULT_TOOL, serde_json::json!({ "ticket": ticket })).await {
      Ok(res) => { let r = result_text(&res); text = r.0; is_error = r.1; }
      Err(e) => return ("failed".into(), String::new(), format!("lost the connection to Claude Code: {e}")),
    }
  }
  if is_error {
    ("failed".into(), String::new(), text)
  } else {
    ("completed".into(), text, String::new())
  }
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
  fn only_ask_claude_goes_to_the_background() {
    assert!(handles("ask_claude"));
    assert!(!handles("claude_status"));
    assert!(voice_description("Ask Claude Code.").contains("IN THIS VOICE CALL"));
  }
}
