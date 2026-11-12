// The desktop agent: the voice session's way to get multi-step work done on
// the desktop without carrying every tool itself.
//
// A realtime voice model chooses badly from ~70 tools (specs/ScreenPresence_design.md,
// section 5), so a call gets the screen tools and a few single-step actions
// directly. Anything that needs several steps - "switch Outlook to month view
// and open next Tuesday" - goes to this tool, which runs the chat model the
// user configured (or `assistant_realtime.desktop_agent_model`) through the
// same MCP tool loop as the Prompt chat, with every connected tool. It runs
// quietly (nothing lands in the chat view) and returns its answer plus the
// steps it took, so the voice model can say what was done.
//
// It is an ordinary voice tool for the background path: if it takes longer
// than background_tools::BACKGROUND_AFTER the call carries on and the result
// is delivered when it lands.

use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;

use crate::mcp::ClientMap;

pub const TOOL_NAME: &str = "desktop_agent";
const MAX_ROUNDS: u8 = 12;

pub struct AgentConfig {
  pub enabled: bool,
  /// Empty: the Prompt chat model.
  pub model: String,
}

pub fn config_from(settings: &serde_json::Value) -> AgentConfig {
  let ar = settings.get("assistant_realtime");
  AgentConfig {
    enabled: ar.and_then(|a| a.get("desktop_agent")).and_then(|b| b.as_bool()).unwrap_or(true),
    model: ar
      .and_then(|a| a.get("desktop_agent_model"))
      .and_then(|m| m.as_str())
      .map(str::trim)
      .unwrap_or("")
      .to_string(),
  }
}

pub fn config_from_settings() -> AgentConfig {
  config_from(&crate::config::load_settings_json())
}

pub fn tool_definition() -> serde_json::Value {
  serde_json::json!({
    "type": "function",
    "name": TOOL_NAME,
    "description": "Hand a task on the user's Windows desktop that needs several steps to an agent that has every tool: mouse, keyboard, windows, UI Automation, apps, files. Use it for things like 'switch Outlook to month view and open next Tuesday' or 'find the latest invoice PDF and open it'. For one click, one field or one shortcut, use the direct tools instead. Pass a complete, standalone task that includes what you read from the screen (window, element names). Say in a few words that you are on it; it can take a while.",
    "parameters": {
      "type": "object",
      "properties": {
        "task": { "type": "string", "description": "The complete task as a standalone instruction, with every detail from the conversation and the screen. The agent cannot hear the call." }
      },
      "required": ["task"],
      "additionalProperties": false
    }
  })
}

/// What the agent is told besides the task.
pub fn system_prompt(screen_context: Option<&str>) -> String {
  let mut s = String::from(
    "You are a desktop agent on the user's Windows PC, working for a voice assistant that hands you one task at a time. \
     Carry the task out with your tools. Prefer UI Automation: find the element (find_ui_element, read_ui_tree, \
     get_pointer_context), then invoke_ui_element or set_ui_value with its element_ref; use mouse coordinates only when \
     no element is found. Check the result of each step before the next one. \
     Send, post or submit only when the task says so in those words; a task to draft, write, prepare or type a message \
     ends with the text in the box, unsent. \
     Before anything that sends, deletes, submits, pays or posts, ask with request_user_confirmation and continue only \
     when its decision is \"approved\"; on \"denied\" or \"timeout\" stop and say so. \
     Do not open or change anything the task does not need. \
     If you cannot find what you need after three or four tries, stop and say what you found instead. \
     End with one or two plain sentences that say what you did, or what stopped you. No markdown, no lists.",
  );
  if let Some(ctx) = screen_context.map(str::trim).filter(|c| !c.is_empty()) {
    s.push_str("\n\nWhat the user had on screen when they asked (text only):\n");
    s.push_str(ctx);
  }
  s
}

/// The tool result the voice session gets: the agent's answer and its steps,
/// in the MCP result shape background_tools reads.
pub fn result_value(answer: &str, steps: &[String]) -> serde_json::Value {
  let mut text = answer.trim().to_string();
  if text.is_empty() { text = "(The agent returned no text.)".into(); }
  if !steps.is_empty() {
    text.push_str("\nSteps: ");
    text.push_str(&steps.join("; "));
  }
  serde_json::json!({ "content": [{ "type": "text", "text": text }] })
}

/// Run the task. Err is a failure to run at all (no model, no key); a task
/// the agent could not finish comes back as its own answer.
pub async fn run(
  app: tauri::AppHandle,
  clients: &'static AsyncMutex<ClientMap>,
  task: String,
  screen_context: Option<String>,
) -> Result<serde_json::Value, String> {
  let task = task.trim().to_string();
  if task.is_empty() { return Err("task is empty".into()); }
  let cfg = config_from_settings();
  let model = if cfg.model.is_empty() { crate::settings::get_model_from_settings_or_env() } else { cfg.model };
  let endpoint = crate::llm_provider::chat_endpoint_for_model(&model)?;
  let messages = vec![
    crate::chat::ChatMessage { role: "system".into(), content: crate::chat::ChatContent::Text(system_prompt(screen_context.as_deref())) },
    crate::chat::ChatMessage { role: "user".into(), content: crate::chat::ChatContent::Text(task) },
  ];
  let trace = Arc::new(Mutex::new(Vec::new()));
  let opts = crate::chat::LoopOptions { trace: Some(trace.clone()), max_rounds: MAX_ROUNDS };
  log::info!("desktop agent: started with {model}");
  let answer = crate::chat::chat_complete_with_mcp_opts(app, messages, endpoint, model, None, clients, opts).await?;
  let steps = trace.lock().map(|t| t.clone()).unwrap_or_default();
  log::info!("desktop agent: done after {} tool calls", steps.len());
  Ok(result_value(&answer, &steps))
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn enabled_by_default_and_model_optional() {
    let c = config_from(&json!({}));
    assert!(c.enabled);
    assert!(c.model.is_empty());
    let c = config_from(&json!({ "assistant_realtime": { "desktop_agent": false, "desktop_agent_model": " gpt-5.1 " } }));
    assert!(!c.enabled);
    assert_eq!(c.model, "gpt-5.1");
  }

  #[test]
  fn prompt_asks_before_outward_actions_and_carries_context() {
    let p = system_prompt(Some("Foreground: olk \"Calendar\""));
    assert!(p.contains("request_user_confirmation"));
    // A draft stays unsent (call 25, 12.11.2026: "draft it in Teams" was sent).
    assert!(p.contains("unsent"));
    assert!(p.contains("\"approved\""));
    assert!(p.contains("Foreground: olk"));
    assert!(!system_prompt(None).contains("had on screen"));
  }

  #[test]
  fn result_lists_the_steps() {
    let v = result_value("Switched to month view.", &["find_ui_element {} -> ok".into(), "invoke_ui_element {} -> ok".into()]);
    let text = v["content"][0]["text"].as_str().unwrap();
    assert!(text.starts_with("Switched to month view."));
    assert!(text.contains("Steps: find_ui_element {} -> ok; invoke_ui_element {} -> ok"));
    assert_eq!(tool_definition()["name"], TOOL_NAME);
  }
}
