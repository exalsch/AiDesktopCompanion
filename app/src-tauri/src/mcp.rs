use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex as AsyncMutex;
use rmcp::service::{RunningService, RoleClient, DynService};
use rmcp::service::ServiceExt;
use rmcp::transport::{TokioChildProcess, streamable_http_client::StreamableHttpClientTransport};
use tokio::process::Command as TokioCommand;
use tauri::Emitter;

#[cfg(target_os = "windows")]
pub fn resolve_windows_program(prog: &str, cwd: Option<&str>) -> Option<String> {
  if prog.contains('\\') || prog.contains('/') || Path::new(prog).extension().is_some() { return None; }
  let pathext: Vec<String> = std::env::var("PATHEXT")
    .ok()
    .map(|v| v.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect())
    .unwrap_or_else(|| vec![".COM".into(), ".EXE".into(), ".BAT".into(), ".CMD".into()]);
  let mut candidate_dirs: Vec<PathBuf> = Vec::new();
  if let Some(d) = cwd {
    let mut p = PathBuf::from(d);
    p.push("node_modules");
    p.push(".bin");
    candidate_dirs.push(p);
  }
  if let Some(path_var) = std::env::var_os("PATH") {
    for p in std::env::split_paths(&path_var) { candidate_dirs.push(p); }
  }
  for dir in candidate_dirs {
    for ext in &pathext {
      let candidate = dir.join(format!("{}{}", prog, ext));
      if candidate.is_file() { return Some(candidate.to_string_lossy().to_string()); }
    }
  }
  None
}

pub async fn connect(
  app: &tauri::AppHandle,
  clients: &AsyncMutex<ClientMap>,
  server_id: String,
  command: String,
  args: Vec<String>,
  cwd: Option<String>,
  env: Option<serde_json::Value>,
  transport: Option<String>,
) -> Result<String, String> {
  // fast path: already connected
  {
    let map = clients.lock().await;
    if map.contains_key(&server_id) {
      return Ok("already connected".into());
    }
  }

  let transport_kind = transport.unwrap_or_else(|| "stdio".to_string());
  if transport_kind == "http" {
    let uri = command.trim().to_string();
    if uri.is_empty() { return Err("HTTP transport requires a non-empty URI in 'command'".into()); }
    let http_transport = StreamableHttpClientTransport::<reqwest::Client>::from_uri(uri);
    let service = ().into_dyn().serve(http_transport).await.map_err(|e| {
      let msg = format!("serve failed: {e}");
      let _ = app.emit("mcp:error", serde_json::json!({ "serverId": server_id, "message": msg }));
      msg
    })?;
    let service = Arc::new(service);
    {
      let mut map = clients.lock().await;
      map.insert(server_id.clone(), service.clone());
    }
    let _ = app.emit("mcp:connected", serde_json::json!({ "serverId": server_id }));
    return Ok("connected".into());
  }

  // Default: stdio child process
  #[cfg(target_os = "windows")]
  let program_to_run: String = resolve_windows_program(&command, cwd.as_deref()).unwrap_or_else(|| command.clone());
  #[cfg(not(target_os = "windows"))]
  let program_to_run: String = command.clone();

  let mut cmd = TokioCommand::new(&program_to_run);
  cmd.args(args.iter());
  if let Some(dir) = cwd.as_ref() { cmd.current_dir(dir); }
  if let Some(envv) = env.as_ref() {
    if let Some(obj) = envv.as_object() {
      for (k, v) in obj.iter() { if let Some(s) = v.as_str() { cmd.env(k, s); } }
    }
  }
  let child_transport = TokioChildProcess::new(cmd).map_err(|e| format!("spawn failed: {e}"))?;
  let service = ().into_dyn().serve(child_transport).await.map_err(|e| {
    let msg = format!("serve failed: {e}");
    let _ = app.emit("mcp:error", serde_json::json!({ "serverId": server_id, "message": msg }));
    msg
  })?;
  let service = Arc::new(service);
  {
    let mut map = clients.lock().await;
    map.insert(server_id.clone(), service.clone());
  }
  let _ = app.emit("mcp:connected", serde_json::json!({ "serverId": server_id }));
  Ok("connected".into())
}

pub async fn disconnect(app: &tauri::AppHandle, clients: &AsyncMutex<ClientMap>, server_id: String) -> Result<String, String> {
  let svc = {
    let mut map = clients.lock().await;
    map.remove(&server_id)
  };
  let existed = svc.is_some();
  if let Some(svc) = svc { svc.cancellation_token().cancel(); }
  let _ = app.emit("mcp:disconnected", serde_json::json!({ "serverId": server_id, "existed": existed }));
  if existed { Ok("disconnected".into()) } else { Err("not connected".into()) }
}

pub type ClientMap = std::collections::HashMap<String, Arc<RunningService<RoleClient, Box<dyn DynService<RoleClient>>>>>;

pub async fn list_tools(clients: &AsyncMutex<ClientMap>, server_id: &str) -> Result<serde_json::Value, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  let res = svc.list_tools(Default::default()).await.map_err(|e| format!("list_tools failed: {e}"))?;
  serde_json::to_value(res).map_err(|e| format!("serialize failed: {e}"))
}

pub async fn list_resources(clients: &AsyncMutex<ClientMap>, server_id: &str) -> Result<serde_json::Value, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  let res = svc.list_resources(Default::default()).await.map_err(|e| format!("list_resources failed: {e}"))?;
  serde_json::to_value(res).map_err(|e| format!("serialize failed: {e}"))
}

pub async fn read_resource(clients: &AsyncMutex<ClientMap>, server_id: &str, uri: &str) -> Result<serde_json::Value, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  let res = svc
    .read_resource(rmcp::model::ReadResourceRequestParam { uri: uri.to_string().into() })
    .await
    .map_err(|e| format!("read_resource failed: {e}"))?;
  serde_json::to_value(res).map_err(|e| format!("serialize failed: {e}"))
}

pub async fn list_prompts(clients: &AsyncMutex<ClientMap>, server_id: &str) -> Result<serde_json::Value, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  let res = svc.list_prompts(Default::default()).await.map_err(|e| format!("list_prompts failed: {e}"))?;
  serde_json::to_value(res).map_err(|e| format!("serialize failed: {e}"))
}

pub async fn get_prompt(
  clients: &AsyncMutex<ClientMap>,
  server_id: &str,
  name: &str,
  arguments: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  let args_map = arguments.and_then(|v| v.as_object().cloned());
  let res = svc
    .get_prompt(rmcp::model::GetPromptRequestParam { name: name.to_string().into(), arguments: args_map })
    .await
    .map_err(|e| format!("get_prompt failed: {e}"))?;
  serde_json::to_value(res).map_err(|e| format!("serialize failed: {e}"))
}

pub async fn ping(clients: &AsyncMutex<ClientMap>, server_id: &str) -> Result<String, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  let _ = svc.list_tools(Default::default()).await.map_err(|e| format!("ping(list_tools) failed: {e}"))?;
  Ok("ok".into())
}

pub async fn call_tool(
  clients: &AsyncMutex<ClientMap>,
  server_id: &str,
  name: &str,
  args: serde_json::Value,
) -> Result<serde_json::Value, String> {
  let svc = {
    let map = clients.lock().await;
    map.get(server_id).cloned()
  }.ok_or_else(|| "not connected".to_string())?;
  // Respect disabled tools from settings
  let disabled_map = crate::config::get_disabled_tools_map();
  if disabled_map.get(server_id).map(|set| set.contains(name)).unwrap_or(false) {
    return Err("tool disabled by settings".into());
  }
  // Prepare arguments map if provided
  let arg_map_opt = if args.is_null() { None } else if let Some(obj) = args.as_object() { Some(obj.clone()) } else { return Err("call_tool args must be an object".into()) };
  let res = svc
    .call_tool(rmcp::model::CallToolRequestParam { name: name.to_string().into(), arguments: arg_map_opt })
    .await
    .map_err(|e| format!("call_tool failed: {e}"))?;
  serde_json::to_value(res).map_err(|e| format!("serialize failed: {e}"))
}

// --- Pure helpers used by MCP integrations ---

// --- Pure helpers used by MCP integrations ---

pub fn sanitize_fn_name(s: &str) -> String {
  let mut out = String::with_capacity(s.len());
  for ch in s.chars() {
    match ch {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => out.push(ch),
      _ => out.push('_'),
    }
  }
  out
}

pub fn parse_mcp_fn_call_name(name: &str) -> Option<(String, String)> {
  // Expected format: mcp__{serverId}__{toolName}
  if !name.starts_with("mcp__") { return None; }
  let rest = &name[5..];
  if let Some(idx) = rest.find("__") {
    let server = &rest[..idx];
    let tool = &rest[idx+2..];
    if !server.is_empty() && !tool.is_empty() {
      return Some((server.to_string(), tool.to_string()));
    }
  }
  None
}

pub fn summarize_input_schema(schema: &serde_json::Value) -> String {
  let props = schema.get("properties").and_then(|v| v.as_object());
  if props.is_none() { return String::new(); }
  let props = props.unwrap();
  let required: std::collections::HashSet<String> = schema
    .get("required")
    .and_then(|v| v.as_array())
    .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
    .unwrap_or_default();

  let mut parts: Vec<String> = Vec::new();
  for (name, def) in props.iter() {
    let ty = def.get("type").and_then(|v| v.as_str()).unwrap_or("any");
    let is_req = required.contains(name);
    let mut piece = format!("{}: {}{}", name, ty, if is_req { " (required)" } else { " (optional)" });
    if let Some(desc) = def.get("description").and_then(|v| v.as_str()) {
      if !desc.is_empty() {
        let trimmed = if desc.len() > 120 { &desc[..120] } else { desc };
        piece.push_str(&format!(" - {}", trimmed));
      }
    }
    parts.push(piece);
  }
  parts.join("; ")
}

/// Build OpenAI tool definitions from connected MCP servers (snapshot provided by caller)
pub async fn build_openai_tools_from_mcp(
  clients: &std::collections::HashMap<String, Arc<RunningService<RoleClient, Box<dyn DynService<RoleClient>>>>>
) -> Vec<serde_json::Value> {
  let mut out: Vec<serde_json::Value> = Vec::new();
  let disabled_map = crate::config::get_disabled_tools_map();
  for (server_id, svc) in clients.iter() {
    if let Ok(res) = svc.list_tools(Default::default()).await {
      if let Ok(v) = serde_json::to_value(&res) {
        if let Some(arr) = v.get("tools").and_then(|x| x.as_array()) {
          for t in arr {
            let name = t.get("name").and_then(|x| x.as_str()).unwrap_or("");
            if name.is_empty() { continue; }
            if let Some(set) = disabled_map.get(server_id) { if set.contains(name) { continue; } }
            let desc = t.get("description").and_then(|x| x.as_str()).unwrap_or("");
            let mut params = t
              .get("input_schema")
              .or_else(|| t.get("inputSchema"))
              .or_else(|| t.get("schema"))
              .cloned()
              .unwrap_or_else(|| serde_json::json!({
                "type": "object",
                "properties": {},
                "additionalProperties": true
              }));
            if !params.is_object() { params = serde_json::json!({ "type": "object", "properties": {}, "additionalProperties": true }); }
            if params.get("type").and_then(|x| x.as_str()).is_none() { if let Some(obj) = params.as_object_mut() { obj.insert("type".to_string(), serde_json::json!("object")); } }
            if params.get("properties").is_none() { if let Some(obj) = params.as_object_mut() { obj.insert("properties".to_string(), serde_json::json!({})); } }
            if params.get("additionalProperties").is_none() { if let Some(obj) = params.as_object_mut() { obj.insert("additionalProperties".to_string(), serde_json::json!(true)); } }
            let fn_name = sanitize_fn_name(&format!("mcp__{}__{}", server_id, name));
            let inputs_summary = summarize_input_schema(&params);
            let desc_aug = if desc.is_empty() {
              if inputs_summary.is_empty() { format!("MCP tool '{}' from server '{}'.", name, server_id) }
              else { format!("MCP tool '{}' from server '{}'. Inputs: {}", name, server_id, inputs_summary) }
            } else {
              if inputs_summary.is_empty() { format!("{} (server: {})", desc, server_id) }
              else { format!("{} (server: {}) Inputs: {}", desc, server_id, inputs_summary) }
            };
            out.push(serde_json::json!({
              "type": "function",
              "function": { "name": fn_name, "description": desc_aug, "parameters": params }
            }));
          }
        }
      }
    }
  }
  out
}
