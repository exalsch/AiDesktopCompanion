use std::fs;
use std::path::PathBuf;
use std::collections::{HashMap, HashSet};

// ---------------------------
// Settings helpers and commands
// ---------------------------

pub fn settings_config_path() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("settings.json");
      return Some(p);
    }
    None
  }
  #[cfg(not(target_os = "windows"))]
  {
    if let Ok(home) = std::env::var("HOME") {
      let mut p = PathBuf::from(home);
      p.push(".config");
      p.push("AiDesktopCompanion");
      p.push("settings.json");
      return Some(p);
    }
    None
  }
}

// Build a map of server_id -> set of disabled tool names from persisted settings
pub fn get_disabled_tools_map() -> HashMap<String, HashSet<String>> {
  let mut out: HashMap<String, HashSet<String>> = HashMap::new();
  let v = load_settings_json();
  if let Some(arr) = v.get("mcp_servers").and_then(|x| x.as_array()) {
    for s in arr.iter() {
      let server_id = s.get("id").and_then(|x| x.as_str()).unwrap_or("").trim();
      if server_id.is_empty() { continue; }
      if let Some(dis) = s.get("disabled_tools").and_then(|x| x.as_array()) {
        let mut set: HashSet<String> = HashSet::new();
        for t in dis.iter() {
          if let Some(name) = t.as_str() {
            let n = name.trim();
            if !n.is_empty() { set.insert(n.to_string()); }
          }
        }
        if !set.is_empty() { out.insert(server_id.to_string(), set); }
      }
    }
  }
  out
}

pub fn load_settings_json() -> serde_json::Value {
  if let Some(path) = settings_config_path() {
    if let Ok(text) = fs::read_to_string(&path) {
      if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
        if v.is_object() { return v; }
      }
    }
  }
  serde_json::json!({})
}

pub fn get_api_key_from_settings_or_env() -> Result<String, String> {
  let v = load_settings_json();
  if let Some(s) = v.get("openai_api_key").and_then(|x| x.as_str()) {
    if !s.trim().is_empty() { return Ok(s.to_string()); }
  }
  std::env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY not set in settings or environment".to_string())
}

pub fn get_model_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("openai_chat_model").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return t.to_string(); }
  }
  std::env::var("OPENAI_CHAT_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string())
}

pub fn get_temperature_from_settings_or_env() -> Option<f32> {
  let v = load_settings_json();
  v.get("temperature").and_then(|x| x.as_f64()).map(|f| f as f32)
}

// Speech-To-Text engine selection: "openai" (default) or "local"
pub fn get_stt_engine_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_engine").and_then(|x| x.as_str()) {
    let t = s.trim().to_lowercase();
    if t == "local" || t == "openai" { return t; }
  }
  std::env::var("AIDC_STT_ENGINE").ok().map(|s| s.to_lowercase()).filter(|t| t == "local" || t == "openai").unwrap_or_else(|| "openai".to_string())
}

pub fn get_stt_local_model_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_local_model").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return t.to_string(); }
  }
  std::env::var("AIDC_STT_LOCAL_MODEL").unwrap_or_else(|_| "whisper".to_string())
}

pub fn get_stt_parakeet_has_cuda_from_settings_or_env() -> bool {
  let v = load_settings_json();
  if let Some(b) = v.get("stt_parakeet_has_cuda").and_then(|x| x.as_bool()) {
    return b;
  }
  std::env::var("AIDC_STT_PARAKEET_HAS_CUDA")
    .ok()
    .map(|s| {
      let t = s.trim().to_lowercase();
      t == "1" || t == "true" || t == "yes" || t == "y" || t == "on"
    })
    .unwrap_or(false)
}

pub fn get_stt_cloud_base_url_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_cloud_base_url").and_then(|x| x.as_str()) {
    let t = s.trim().trim_end_matches('/');
    if !t.is_empty() { return t.to_string(); }
  }
  std::env::var("AIDC_STT_CLOUD_BASE_URL")
    .ok()
    .map(|s| s.trim().trim_end_matches('/').to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(|| "https://api.openai.com".to_string())
}

pub fn get_stt_cloud_model_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_cloud_model").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return t.to_string(); }
  }
  std::env::var("AIDC_STT_CLOUD_MODEL").unwrap_or_else(|_| "whisper-1".to_string())
}

pub fn get_stt_cloud_api_key_from_settings_or_env() -> Option<String> {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_cloud_api_key").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return Some(t.to_string()); }
  }
  std::env::var("AIDC_STT_CLOUD_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn get_settings() -> Result<serde_json::Value, String> {
  let v = load_settings_json();
  Ok(v)
}

pub fn save_settings(map: serde_json::Value) -> Result<String, String> {
  let path = settings_config_path().ok_or_else(|| "Unsupported platform for config path".to_string())?;
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
  }
  // Merge with existing settings. Only update known keys present in `map`.
  let current = load_settings_json();
  let mut obj = current.as_object().cloned().unwrap_or_default();

  // Existing keys
  if let Some(k) = map.get("openai_api_key").and_then(|x| x.as_str()) { obj.insert("openai_api_key".to_string(), serde_json::Value::String(k.to_string())); }
  if let Some(m) = map.get("openai_chat_model").and_then(|x| x.as_str()) { obj.insert("openai_chat_model".to_string(), serde_json::Value::String(m.to_string())); }
  // Dedicated model for Quick Actions quick prompts (optional; empty string means fallback to global)
  if let Some(qpm) = map.get("quick_prompt_model").and_then(|x| x.as_str()) { obj.insert("quick_prompt_model".to_string(), serde_json::Value::String(qpm.to_string())); }
  if let Some(t) = map.get("temperature").and_then(|x| x.as_f64()) { obj.insert("temperature".to_string(), serde_json::Value::Number(serde_json::Number::from_f64(t).unwrap_or_else(|| serde_json::Number::from_f64(1.0).unwrap()))); }
  if let Some(p) = map.get("persist_conversations").and_then(|x| x.as_bool()) { obj.insert("persist_conversations".to_string(), serde_json::Value::Bool(p)); }
  // Persist UI style selection
  if let Some(ui) = map.get("ui_style").and_then(|x| x.as_str()) { obj.insert("ui_style".to_string(), serde_json::Value::String(ui.to_string())); }
  // Persist chat display preference
  if let Some(hide) = map.get("hide_tool_calls_in_chat").and_then(|x| x.as_bool()) { obj.insert("hide_tool_calls_in_chat".to_string(), serde_json::Value::Bool(hide)); }
  // Persist global hotkey
  if let Some(hk) = map.get("global_hotkey").and_then(|x| x.as_str()) { obj.insert("global_hotkey".to_string(), serde_json::Value::String(hk.to_string())); }
  // Persist global system prompt
  if let Some(sp) = map.get("system_prompt").and_then(|x| x.as_str()) { obj.insert("system_prompt".to_string(), serde_json::Value::String(sp.to_string())); }
  // Persist Quick Prompts specific system prompt
  if let Some(qpsp) = map.get("quick_prompt_system_prompt").and_then(|x| x.as_str()) { obj.insert("quick_prompt_system_prompt".to_string(), serde_json::Value::String(qpsp.to_string())); }
  // Persist Quick Actions preview toggle for quick prompts
  if let Some(flag) = map.get("show_quick_prompt_result_in_popup").and_then(|x| x.as_bool()) { obj.insert("show_quick_prompt_result_in_popup".to_string(), serde_json::Value::Bool(flag)); }
  // Remove deprecated global MCP auto_connect flag if present
  obj.remove("auto_connect");
  // Pass-through for MCP servers configuration when provided
  if let Some(ms) = map.get("mcp_servers") { obj.insert("mcp_servers".to_string(), ms.clone()); }

  // Persist Assistant Mode realtime settings when provided
  if let Some(ar) = map.get("assistant_realtime") { obj.insert("assistant_realtime".to_string(), ar.clone()); }

  // New TTS preference keys
  if let Some(e) = map.get("tts_engine").and_then(|x| x.as_str()) { obj.insert("tts_engine".to_string(), serde_json::Value::String(e.to_string())); }
  if let Some(r) = map.get("tts_rate").and_then(|x| x.as_i64()) { obj.insert("tts_rate".to_string(), serde_json::Value::Number((r as i64).into())); }
  if let Some(v) = map.get("tts_volume").and_then(|x| x.as_i64()) { obj.insert("tts_volume".to_string(), serde_json::Value::Number((v as i64).into())); }
  if let Some(vl) = map.get("tts_voice_local").and_then(|x| x.as_str()) { obj.insert("tts_voice_local".to_string(), serde_json::Value::String(vl.to_string())); }
  if let Some(ov) = map.get("tts_openai_voice").and_then(|x| x.as_str()) { obj.insert("tts_openai_voice".to_string(), serde_json::Value::String(ov.to_string())); }
  if let Some(om) = map.get("tts_openai_model").and_then(|x| x.as_str()) { obj.insert("tts_openai_model".to_string(), serde_json::Value::String(om.to_string())); }
  if let Some(of) = map.get("tts_openai_format").and_then(|x| x.as_str()) { obj.insert("tts_openai_format".to_string(), serde_json::Value::String(of.to_string())); }
  if let Some(os) = map.get("tts_openai_streaming").and_then(|x| x.as_bool()) { obj.insert("tts_openai_streaming".to_string(), serde_json::Value::Bool(os)); }

  // New STT preference keys
  if let Some(se) = map.get("stt_engine").and_then(|x| x.as_str()) { obj.insert("stt_engine".to_string(), serde_json::Value::String(se.to_string())); }
  if let Some(lm) = map.get("stt_local_model").and_then(|x| x.as_str()) { obj.insert("stt_local_model".to_string(), serde_json::Value::String(lm.to_string())); }
  if let Some(b) = map.get("stt_parakeet_has_cuda").and_then(|x| x.as_bool()) { obj.insert("stt_parakeet_has_cuda".to_string(), serde_json::Value::Bool(b)); }
  if let Some(bu) = map.get("stt_cloud_base_url").and_then(|x| x.as_str()) { obj.insert("stt_cloud_base_url".to_string(), serde_json::Value::String(bu.to_string())); }
  if let Some(sm) = map.get("stt_cloud_model").and_then(|x| x.as_str()) { obj.insert("stt_cloud_model".to_string(), serde_json::Value::String(sm.to_string())); }
  if let Some(sk) = map.get("stt_cloud_api_key").and_then(|x| x.as_str()) { obj.insert("stt_cloud_api_key".to_string(), serde_json::Value::String(sk.to_string())); }
  // Whisper (local STT) model selection
  if let Some(u) = map.get("stt_whisper_model_url").and_then(|x| x.as_str()) { obj.insert("stt_whisper_model_url".to_string(), serde_json::Value::String(u.to_string())); }
  if let Some(preset) = map.get("stt_whisper_model_preset").and_then(|x| x.as_str()) { obj.insert("stt_whisper_model_preset".to_string(), serde_json::Value::String(preset.to_string())); }

  // Remove deprecated local STT model selector keys if present
  obj.remove("stt_local_base_url");

  let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(obj)).map_err(|e| format!("Serialize settings failed: {e}"))?;
  fs::write(&path, pretty).map_err(|e| format!("Write settings failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
}

// ---------------------------
// Conversation persistence
// ---------------------------

pub fn conversation_state_path() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("conversations.json");
      return Some(p);
    }
    None
  }
  #[cfg(not(target_os = "windows"))]
  {
    if let Ok(home) = std::env::var("HOME") {
      let mut p = PathBuf::from(home);
      p.push(".config");
      p.push("AiDesktopCompanion");
      p.push("conversations.json");
      return Some(p);
    }
    None
  }
}

pub fn persist_conversations_enabled() -> bool {
  let v = load_settings_json();
  v.get("persist_conversations").and_then(|x| x.as_bool()).unwrap_or(false)
}

pub fn load_conversation_state() -> Result<serde_json::Value, String> {
  if !persist_conversations_enabled() {
    return Ok(serde_json::json!({}));
  }
  if let Some(path) = conversation_state_path() {
    match fs::read_to_string(&path) {
      Ok(text) => {
        match serde_json::from_str::<serde_json::Value>(&text) {
          Ok(v) => Ok(v),
          Err(e) => Err(format!("Invalid JSON in conversations.json: {e}")),
        }
      }
      Err(_) => Ok(serde_json::json!({})),
    }
  } else {
    Err("Unsupported platform for config path".into())
  }
}

pub fn save_conversation_state(state: serde_json::Value) -> Result<String, String> {
  if !persist_conversations_enabled() {
    if let Some(path) = conversation_state_path() {
      let _ = fs::remove_file(path);
    }
    return Ok("persistence disabled".into());
  }
  let path = conversation_state_path().ok_or_else(|| "Unsupported platform for config path".to_string())?;
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("Failed to create config directory: {e}"))?;
  }
  let pretty = serde_json::to_string_pretty(&state).map_err(|e| format!("Serialize conversation failed: {e}"))?;
  fs::write(&path, pretty).map_err(|e| format!("Write conversations failed: {e}"))?;
  Ok(path.to_string_lossy().to_string())
}

pub fn clear_conversations() -> Result<String, String> {
  if let Some(path) = conversation_state_path() {
    if path.exists() {
      fs::remove_file(&path).map_err(|e| format!("Remove conversations failed: {e}"))?;
    }
    Ok(path.to_string_lossy().to_string())
  } else {
    Err("Unsupported platform for config path".into())
  }
}
