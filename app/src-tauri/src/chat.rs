use serde::Deserialize;
use base64::Engine;
use std::fs;
use std::sync::Arc;
use rmcp::service::{RoleClient, DynService, RunningService};
use tokio::sync::Mutex as AsyncMutex;
use tauri::Emitter;

#[derive(Debug, Deserialize)]
pub struct ChatMessage {
  pub role: String,
  pub content: ChatContent,
}

pub async fn chat_complete_with_mcp(
  app: tauri::AppHandle,
  messages: Vec<ChatMessage>,
  key: String,
  model: String,
  temp: Option<f32>,
  mcp_clients: &AsyncMutex<std::collections::HashMap<String, Arc<RunningService<RoleClient, Box<dyn DynService<RoleClient>>>>>>,
) -> Result<String, String> {
  use crate::mcp;

  // Normalize incoming messages to OpenAI format
  let mut norm_msgs: Vec<serde_json::Value> = Vec::new();
  for m in messages.into_iter() {
    let r = match m.role.to_ascii_lowercase().as_str() { "system" | "assistant" | "user" => m.role.to_ascii_lowercase(), _ => "user".to_string() };
    let content_value = match m.content {
      ChatContent::Text(s) => serde_json::Value::String(s),
      ChatContent::Parts(parts) => {
        let mut out_parts: Vec<serde_json::Value> = Vec::new();
        for p in parts {
          match p {
            FrontendPart::InputText { text } => { out_parts.push(serde_json::json!({ "type": "text", "text": text })); }
            FrontendPart::InputImage { path, mime } => {
              let mime_final = mime.or_else(|| guess_mime_from_path_rs(&path).map(|s| s.to_string())).ok_or_else(|| format!("Missing/unknown image MIME for: {}", path))?;
              let bytes = fs::read(&path).map_err(|e| format!("Failed to read image '{}': {}", path, e))?;
              let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
              let url = format!("data:{};base64,{}", mime_final, b64);
              out_parts.push(serde_json::json!({ "type": "image_url", "image_url": { "url": url } }));
            }
          }
        }
        serde_json::Value::Array(out_parts)
      }
    };
    norm_msgs.push(serde_json::json!({ "role": r, "content": content_value }));
  }

  // Build tool definitions from connected MCP servers (via MCP module)
  let tools = {
    let map = mcp_clients.lock().await;
    mcp::build_openai_tools_from_mcp(&*map).await
  };

  let client = reqwest::Client::new();
  let sys_tool_guidance = serde_json::json!({
    "role": "system",
    "content": "You can use MCP tools. When you call a tool, ALWAYS provide all required parameters per its JSON Schema, with correct types. Do not call tools with empty arguments."
  });
  let mut msgs_for_oai: Vec<serde_json::Value> = Vec::new();
  msgs_for_oai.push(sys_tool_guidance);
  msgs_for_oai.extend(norm_msgs.clone());
  let mut final_text: Option<String> = None;

  for _ in 0..6u8 {
    let mut body = serde_json::json!({ "model": &model, "messages": msgs_for_oai });
    if let Some(t) = temp { if let serde_json::Value::Object(ref mut m) = body { m.insert("temperature".to_string(), serde_json::json!(t)); } }
    if !tools.is_empty() {
      if let serde_json::Value::Object(ref mut m) = body {
        m.insert("tools".to_string(), serde_json::Value::Array(tools.clone()));
        m.insert("tool_choice".to_string(), serde_json::Value::String("auto".to_string()));
        m.insert("parallel_tool_calls".to_string(), serde_json::Value::Bool(true));
      }
    }

    let resp = client
      .post("https://api.openai.com/v1/chat/completions")
      .bearer_auth(&key)
      .json(&body)
      .send()
      .await
      .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      return Err(format!("OpenAI error: {status} {body_text}"));
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
    let choice0 = v.get("choices").and_then(|c| c.get(0)).cloned().unwrap_or(serde_json::Value::Null);
    let msg = choice0.get("message").cloned().unwrap_or(serde_json::Value::Null);
    let tool_calls_opt = msg.get("tool_calls").and_then(|x| x.as_array()).cloned();
    let content_str_opt = msg.get("content").and_then(|t| t.as_str()).map(|s| s.to_string());

    if let Some(tool_calls) = tool_calls_opt {
      // Append assistant message with tool_calls to history
      let mut assistant_msg = serde_json::Map::new();
      assistant_msg.insert("role".to_string(), serde_json::Value::String("assistant".to_string()));
      if let Some(c) = content_str_opt.as_ref() { assistant_msg.insert("content".to_string(), serde_json::Value::String(c.clone())); }
      assistant_msg.insert("tool_calls".to_string(), serde_json::Value::Array(tool_calls.clone()));
      msgs_for_oai.push(serde_json::Value::Object(assistant_msg));

      // Dispatch each tool call sequentially and append tool results
      for tc in tool_calls.into_iter() {
        let id = tc.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let fname = tc.get("function").and_then(|f| f.get("name")).and_then(|x| x.as_str()).unwrap_or("").to_string();
        let fargs_str = tc.get("function").and_then(|f| f.get("arguments")).and_then(|x| x.as_str()).unwrap_or("{}");
        let mut fargs_val: serde_json::Value = serde_json::from_str(fargs_str).unwrap_or_else(|_| serde_json::json!({}));
        if !fargs_val.is_object() { fargs_val = serde_json::json!({}); }

        if let Some((server_id, tool_name)) = mcp::parse_mcp_fn_call_name(&fname) {
          let _ = app.emit("chat:tool-call", serde_json::json!({ "id": id, "function": fname, "serverId": server_id, "tool": tool_name, "args": fargs_val.clone() }));
          // Respect disabled tools from settings
          let disabled_map = crate::config::get_disabled_tools_map();
          let is_disabled = disabled_map.get(&server_id).map(|set| set.contains(&tool_name)).unwrap_or(false);
          let tool_result_text: String;
          if is_disabled {
            tool_result_text = serde_json::json!({ "serverId": server_id, "tool": tool_name, "error": "tool disabled by settings" }).to_string();
            let _ = app.emit("chat:tool-result", serde_json::json!({ "id": id, "function": fname, "serverId": server_id, "tool": tool_name, "ok": false, "error": "tool disabled by settings" }));
          } else {
            let svc_opt = {
              let map2 = mcp_clients.lock().await;
              map2.get(&server_id).cloned()
            };
            if let Some(svc) = svc_opt {
              let arg_map_opt = fargs_val.as_object().cloned();
              match svc.call_tool(rmcp::model::CallToolRequestParam { name: tool_name.clone().into(), arguments: arg_map_opt }).await {
                Ok(res) => {
                  tool_result_text = serde_json::to_string(&serde_json::json!({ "serverId": server_id, "tool": tool_name, "result": res })).unwrap_or_else(|_| "{}".to_string());
                  let _ = app.emit("chat:tool-result", serde_json::json!({ "id": id, "function": fname, "serverId": server_id, "tool": tool_name, "ok": true, "result": res }));
                }
                Err(e) => {
                  tool_result_text = serde_json::json!({ "serverId": server_id, "tool": tool_name, "error": format!("call_tool failed: {}", e) }).to_string();
                  let _ = app.emit("chat:tool-result", serde_json::json!({ "id": id, "function": fname, "serverId": server_id, "tool": tool_name, "ok": false, "error": format!("call_tool failed: {}", e) }));
                }
              }
            } else {
              tool_result_text = serde_json::json!({ "error": format!("MCP server not connected: {}", server_id) }).to_string();
              let _ = app.emit("chat:tool-result", serde_json::json!({ "id": id, "function": fname, "serverId": server_id, "tool": tool_name, "ok": false, "error": format!("MCP server not connected: {}", server_id) }));
            }
          }

          // Append tool result message
          msgs_for_oai.push(serde_json::json!({ "role": "tool", "tool_call_id": id, "content": tool_result_text }));
        } else {
          let tool_result_text = serde_json::json!({ "error": format!("Unsupported tool function: {}", fname) }).to_string();
          let _ = app.emit("chat:tool-result", serde_json::json!({ "id": id, "function": fname, "ok": false, "error": format!("Unsupported tool function: {}", fname) }));
          msgs_for_oai.push(serde_json::json!({ "role": "tool", "tool_call_id": id, "content": tool_result_text }));
        }
      }
      // Continue loop for next assistant turn
      continue;
    }

    final_text = Some(content_str_opt.unwrap_or_default());
    break;
  }

  Ok(final_text.unwrap_or_else(|| "".to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ChatContent {
  Text(String),
  Parts(Vec<FrontendPart>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FrontendPart {
  InputText { text: String },
  InputImage { path: String, mime: Option<String> },
}

pub fn guess_mime_from_path_rs(path: &str) -> Option<&'static str> {
  let p = path.to_ascii_lowercase();
  if p.ends_with(".png") { return Some("image/png"); }
  if p.ends_with(".jpg") || p.ends_with(".jpeg") { return Some("image/jpeg"); }
  if p.ends_with(".webp") { return Some("image/webp"); }
  if p.ends_with(".gif") { return Some("image/gif"); }
  if p.ends_with(".bmp") { return Some("image/bmp"); }
  if p.ends_with(".tif") || p.ends_with(".tiff") { return Some("image/tiff"); }
  None
}

#[allow(dead_code)]
pub async fn chat_complete(
  _app: tauri::AppHandle,
  messages: Vec<ChatMessage>,
  key: String,
  model: String,
  temp: Option<f32>,
  tools: Vec<serde_json::Value>,
) -> Result<String, String> {
  // Normalize incoming messages to OpenAI format
  let mut norm_msgs: Vec<serde_json::Value> = Vec::new();
  for m in messages.into_iter() {
    let r = match m.role.to_ascii_lowercase().as_str() {
      "system" | "assistant" | "user" => m.role.to_ascii_lowercase(),
      _ => "user".to_string(),
    };

    let content_value = match m.content {
      ChatContent::Text(s) => serde_json::Value::String(s),
      ChatContent::Parts(parts) => {
        let mut out_parts: Vec<serde_json::Value> = Vec::new();
        for p in parts {
          match p {
            FrontendPart::InputText { text } => {
              out_parts.push(serde_json::json!({ "type": "text", "text": text }));
            }
            FrontendPart::InputImage { path, mime } => {
              let mime_final = mime
                .or_else(|| guess_mime_from_path_rs(&path).map(|s| s.to_string()))
                .ok_or_else(|| format!("Missing/unknown image MIME for: {}", path))?;
              let bytes = fs::read(&path).map_err(|e| format!("Failed to read image '{}': {}", path, e))?;
              let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
              let url = format!("data:{};base64,{}", mime_final, b64);
              out_parts.push(serde_json::json!({ "type": "image_url", "image_url": { "url": url } }));
            }
          }
        }
        serde_json::Value::Array(out_parts)
      }
    };
    norm_msgs.push(serde_json::json!({ "role": r, "content": content_value }));
  }

  let client = reqwest::Client::new();
  // Prepend a short system directive to improve first-call argument completeness
  let sys_tool_guidance = serde_json::json!({
    "role": "system",
    "content": "You can use MCP tools. When you call a tool, ALWAYS provide all required parameters per its JSON Schema, with correct types. Do not call tools with empty arguments."
  });
  let mut msgs_for_oai: Vec<serde_json::Value> = Vec::new();
  msgs_for_oai.push(sys_tool_guidance);
  msgs_for_oai.extend(norm_msgs.clone());
  let mut final_text: Option<String> = None;

  // Iterate tool-calls up to a reasonable limit
  for _ in 0..6u8 {
    let mut body = serde_json::json!({
      "model": &model,
      "messages": msgs_for_oai,
    });
    if let Some(t) = temp {
      if let serde_json::Value::Object(ref mut m) = body {
        m.insert("temperature".to_string(), serde_json::json!(t));
      }
    }
    if !tools.is_empty() {
      if let serde_json::Value::Object(ref mut m) = body {
        m.insert("tools".to_string(), serde_json::Value::Array(tools.clone()));
        m.insert("tool_choice".to_string(), serde_json::Value::String("auto".to_string()));
        // Allow model to use multiple tool calls
        m.insert("parallel_tool_calls".to_string(), serde_json::Value::Bool(true));
      }
    }

    let resp = client
      .post("https://api.openai.com/v1/chat/completions")
      .bearer_auth(&key)
      .json(&body)
      .send()
      .await
      .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body_text = resp.text().await.unwrap_or_default();
      return Err(format!("OpenAI error: {status} {body_text}"));
    }

    let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
    let choice0 = v.get("choices").and_then(|c| c.get(0)).cloned().unwrap_or(serde_json::Value::Null);
    let msg = choice0.get("message").cloned().unwrap_or(serde_json::Value::Null);
    let tool_calls_opt = msg.get("tool_calls").and_then(|x| x.as_array()).cloned();
    let content_str_opt = msg.get("content").and_then(|t| t.as_str()).map(|s| s.to_string());

    if let Some(tool_calls) = tool_calls_opt {
      // Append assistant message with tool_calls to history
      let mut assistant_msg = serde_json::Map::new();
      assistant_msg.insert("role".to_string(), serde_json::Value::String("assistant".to_string()));
      if let Some(c) = content_str_opt.as_ref() {
        assistant_msg.insert("content".to_string(), serde_json::Value::String(c.clone()));
      }
      assistant_msg.insert("tool_calls".to_string(), serde_json::Value::Array(tool_calls.clone()));
      msgs_for_oai.push(serde_json::Value::Object(assistant_msg));

      // Return to allow the wrapper to dispatch tools; in this simplified move we break after first non-empty tool call set
      // The lib.rs wrapper will re-loop after adding tool results.
      // For now we mirror previous behavior by continuing the loop here (no-op since tool execution is in lib.rs)
      continue;
    }

    // No tool calls; return final assistant content
    final_text = Some(content_str_opt.unwrap_or_default());
    break;
  }

  Ok(final_text.unwrap_or_else(|| "".to_string()))
}
