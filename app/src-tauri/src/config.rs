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
    if !s.trim().is_empty() { return Ok(s.trim().to_string()); }
  }
  std::env::var("OPENAI_API_KEY")
    .map(|s| s.trim().to_string())
    .map_err(|_| "OPENAI_API_KEY not set in settings or environment".to_string())
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

pub fn get_start_in_tray_from_settings() -> bool {
  let v = load_settings_json();
  v.get("start_in_tray").and_then(|x| x.as_bool()).unwrap_or(false)
}

/// Whether stdio MCP servers should keep their console window.
///
/// Off by default: the app talks to those processes over pipes and the user
/// never types into them, so the window is noise. Worth turning on when a
/// server dies at startup and its error only appears there.
pub fn mcp_show_console() -> bool {
  load_settings_json()
    .get("mcp_show_console")
    .and_then(|x| x.as_bool())
    .unwrap_or(false)
}

/// Normalize a `select_all_capture_mode` value to one of the three supported
/// modes, defaulting to `"ctrl_shift_home"`.
pub fn normalize_select_all_capture_mode(value: &str) -> &'static str {
  match value.trim() {
    "none" => "none",
    "ctrl_a" => "ctrl_a",
    _ => "ctrl_shift_home",
  }
}

/// How much text the select-all hotkey selects before copying:
/// `"ctrl_shift_home"` (everything from the caret back to the start, the
/// default), `"ctrl_a"` (whole document) or `"none"` (use the existing
/// selection).
pub fn get_select_all_capture_mode() -> String {
  let v = load_settings_json();
  let raw = v
    .get("select_all_capture_mode")
    .and_then(|x| x.as_str())
    .unwrap_or("");
  normalize_select_all_capture_mode(raw).to_string()
}

/// Longest paste delay we will honour. Past a couple of seconds the user is
/// staring at a frozen popup and would rather see the paste fail.
pub const MAX_PASTE_DELAY_MS: u64 = 2000;

/// Default wait between sending the paste combo and restoring the clipboard.
/// Long enough for the target application to have read the clipboard; short
/// enough not to be noticeable.
pub const DEFAULT_PASTE_DELAY_MS: u64 = 120;

/// Normalize an `insert_mode` value to one of the five supported modes,
/// defaulting to `"ctrl_v"`.
///
/// `"clipboard"` is accepted as an alias for `"ctrl_v"`: it was the value of
/// the two-way toggle this setting replaced.
pub fn normalize_insert_mode(value: &str) -> &'static str {
  match value.trim() {
    "ctrl_shift_v" => "ctrl_shift_v",
    "shift_insert" => "shift_insert",
    "keystrokes" => "keystrokes",
    "none" => "none",
    _ => "ctrl_v",
  }
}

/// How results are put back into the focused application.
///
/// `"ctrl_v"` (the default), `"ctrl_shift_v"` and `"shift_insert"` all go
/// through the clipboard and differ only in the combo they send - the latter
/// two are what terminals and consoles answer to. `"keystrokes"` types the
/// text and never opens the clipboard. `"none"` inserts nothing, which is only
/// useful together with `clipboard_handling = "copy_to_clipboard"`.
pub fn get_insert_mode() -> String {
  let v = load_settings_json();
  let raw = v.get("insert_mode").and_then(|x| x.as_str()).unwrap_or("");
  normalize_insert_mode(raw).to_string()
}

/// Normalize a `clipboard_handling` value, defaulting to `"dont_modify"`.
pub fn normalize_clipboard_handling(value: &str) -> &'static str {
  match value.trim() {
    "copy_to_clipboard" => "copy_to_clipboard",
    _ => "dont_modify",
  }
}

/// What the clipboard looks like once an insertion has finished:
/// `"dont_modify"` (the default) puts back whatever was there before, and
/// `"copy_to_clipboard"` deliberately leaves the inserted text on it.
pub fn get_clipboard_handling() -> String {
  let v = load_settings_json();
  let raw = v
    .get("clipboard_handling")
    .and_then(|x| x.as_str())
    .unwrap_or("");
  normalize_clipboard_handling(raw).to_string()
}

/// Clamp a paste delay to something a human would want to sit through.
pub fn normalize_paste_delay_ms(value: u64) -> u64 {
  value.min(MAX_PASTE_DELAY_MS)
}

/// Milliseconds to wait after sending the paste combo before restoring the
/// clipboard. Too short and the target application reads the restored contents
/// instead of the result, pasting the wrong text.
pub fn get_paste_delay_ms() -> u64 {
  let v = load_settings_json();
  match v.get("paste_delay_ms").and_then(|x| x.as_u64()) {
    Some(ms) => normalize_paste_delay_ms(ms),
    None => DEFAULT_PASTE_DELAY_MS,
  }
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
  std::env::var("AIDC_STT_CLOUD_MODEL").unwrap_or_else(|_| "gpt-transcribe".to_string())
}

pub fn get_stt_cloud_api_key_from_settings_or_env() -> Option<String> {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_cloud_api_key").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return Some(t.to_string()); }
  }
  std::env::var("AIDC_STT_CLOUD_API_KEY").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn get_stt_post_process_enabled_from_settings_or_env() -> bool {
  let v = load_settings_json();
  if let Some(b) = v.get("stt_post_process_enabled").and_then(|x| x.as_bool()) {
    return b;
  }
  std::env::var("AIDC_STT_POST_PROCESS_ENABLED")
    .ok()
    .map(|s| {
      let t = s.trim().to_lowercase();
      t == "1" || t == "true" || t == "yes" || t == "y" || t == "on"
    })
    .unwrap_or(false)
}

pub fn get_stt_post_process_model_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_post_process_model").and_then(|x| x.as_str()) {
    let t = s.trim();
    if !t.is_empty() { return t.to_string(); }
  }
  std::env::var("AIDC_STT_POST_PROCESS_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string())
}

/// Proper nouns and terms the recogniser keeps getting wrong, one per line.
///
/// Names outside the model's training distribution - foreign surnames, product
/// names - are where speech recognition drifts most, and the local Parakeet
/// engine has no decode-time hook to bias it. So the list is applied where it
/// can be: as a hint field for the engines that take one, and otherwise through
/// the post-processing pass.
pub fn get_stt_vocabulary_from_settings_or_env() -> String {
  let v = load_settings_json();
  if let Some(s) = v.get("stt_vocabulary").and_then(|x| x.as_str()) {
    if !s.trim().is_empty() {
      return s.trim().to_string();
    }
  }
  std::env::var("AIDC_STT_VOCABULARY")
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_default()
}

/// The vocabulary as a single comma-separated line.
///
/// What the OpenAI `prompt` field and whisper's `initial_prompt` both want: a
/// sample of the expected text, not a bulleted list.
pub fn get_stt_vocabulary_hint() -> String {
  let raw = get_stt_vocabulary_from_settings_or_env();
  if raw.is_empty() { return String::new(); }
  let terms: Vec<&str> = raw
    .lines()
    .map(|l| l.trim())
    .filter(|l| !l.is_empty())
    .collect();
  if terms.is_empty() { return String::new(); }
  terms.join(", ")
}

pub fn get_stt_post_process_prompt_from_settings_or_env() -> String {
  let default_prompt = "You are an STT post-processor. Rewrite the given transcript to improve readability only: fix punctuation, casing, spacing, and obvious recognition artifacts including repeating words. Preserve original meaning, language, and details while improving clarity. Return only the cleaned transcript text.".to_string();
  let v = load_settings_json();
  if let Some(s) = v.get("stt_post_process_prompt").and_then(|x| x.as_str()) {
    if !s.trim().is_empty() {
      return s.to_string();
    }
  }
  std::env::var("AIDC_STT_POST_PROCESS_PROMPT")
    .ok()
    .map(|s| s.trim().to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or(default_prompt)
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
  if let Some(b) = map.get("mcp_show_console").and_then(|x| x.as_bool()) { obj.insert("mcp_show_console".to_string(), serde_json::Value::Bool(b)); }
  if let Some(b) = map.get("pause_media_on_stt").and_then(|x| x.as_bool()) { obj.insert("pause_media_on_stt".to_string(), serde_json::Value::Bool(b)); }
  if let Some(b) = map.get("pause_media_on_assistant").and_then(|x| x.as_bool()) { obj.insert("pause_media_on_assistant".to_string(), serde_json::Value::Bool(b)); }
  if let Some(s) = map.get("start_in_tray").and_then(|x| x.as_bool()) { obj.insert("start_in_tray".to_string(), serde_json::Value::Bool(s)); }
  // Persist UI style selection
  if let Some(ui) = map.get("ui_style").and_then(|x| x.as_str()) { obj.insert("ui_style".to_string(), serde_json::Value::String(ui.to_string())); }
  // Persist chat display preference
  if let Some(hide) = map.get("hide_tool_calls_in_chat").and_then(|x| x.as_bool()) { obj.insert("hide_tool_calls_in_chat".to_string(), serde_json::Value::Bool(hide)); }
  // Persist global hotkey
  if let Some(hk) = map.get("global_hotkey").and_then(|x| x.as_str()) { obj.insert("global_hotkey".to_string(), serde_json::Value::String(hk.to_string())); }
  // Persist the dedicated "select all + run quick prompt" hotkey and the prompt it runs
  if let Some(hk) = map.get("select_all_hotkey").and_then(|x| x.as_str()) { obj.insert("select_all_hotkey".to_string(), serde_json::Value::String(hk.to_string())); }
  if let Some(hk) = map.get("push_to_talk_hotkey").and_then(|x| x.as_str()) { obj.insert("push_to_talk_hotkey".to_string(), serde_json::Value::String(hk.to_string())); }
  if let Some(idx) = map.get("select_all_quick_prompt").and_then(|x| x.as_u64()) {
    obj.insert("select_all_quick_prompt".to_string(), serde_json::Value::Number(serde_json::Number::from(idx.clamp(1, 9))));
  }
  if let Some(mode) = map.get("select_all_capture_mode").and_then(|x| x.as_str()) {
    let normalized = normalize_select_all_capture_mode(mode);
    obj.insert("select_all_capture_mode".to_string(), serde_json::Value::String(normalized.to_string()));
  }
  // Persist how results are inserted into the focused app
  if let Some(mode) = map.get("insert_mode").and_then(|x| x.as_str()) {
    let normalized = normalize_insert_mode(mode);
    obj.insert("insert_mode".to_string(), serde_json::Value::String(normalized.to_string()));
  }
  if let Some(mode) = map.get("clipboard_handling").and_then(|x| x.as_str()) {
    let normalized = normalize_clipboard_handling(mode);
    obj.insert("clipboard_handling".to_string(), serde_json::Value::String(normalized.to_string()));
  }
  if let Some(ms) = map.get("paste_delay_ms").and_then(|x| x.as_u64()) {
    let clamped = normalize_paste_delay_ms(ms);
    obj.insert("paste_delay_ms".to_string(), serde_json::Value::Number(serde_json::Number::from(clamped)));
  }
  // Persist the floating busy indicator toggle
  if let Some(flag) = map.get("show_busy_indicator").and_then(|x| x.as_bool()) { obj.insert("show_busy_indicator".to_string(), serde_json::Value::Bool(flag)); }
  // Persist global system prompt
  if let Some(sp) = map.get("system_prompt").and_then(|x| x.as_str()) { obj.insert("system_prompt".to_string(), serde_json::Value::String(sp.to_string())); }
  // Persist Quick Prompts specific system prompt
  if let Some(qpsp) = map.get("quick_prompt_system_prompt").and_then(|x| x.as_str()) { obj.insert("quick_prompt_system_prompt".to_string(), serde_json::Value::String(qpsp.to_string())); }
  // Persist Quick Actions preview toggle for quick prompts
  if let Some(flag) = map.get("show_quick_prompt_result_in_popup").and_then(|x| x.as_bool()) { obj.insert("show_quick_prompt_result_in_popup".to_string(), serde_json::Value::Bool(flag)); }
  // Remove deprecated global MCP auto_connect flag if present
  obj.remove("auto_connect");
  // Pass-through for MCP servers configuration when provided
  if let Some(ms) = map.get("mcp_servers") {
    if !ms.is_null() { obj.insert("mcp_servers".to_string(), ms.clone()); }
  }

  // Persist Assistant Mode realtime settings when provided
  if let Some(ar) = map.get("assistant_realtime") {
    if !ar.is_null() { obj.insert("assistant_realtime".to_string(), ar.clone()); }
  }

  // New TTS preference keys
  if let Some(e) = map.get("tts_engine").and_then(|x| x.as_str()) { obj.insert("tts_engine".to_string(), serde_json::Value::String(e.to_string())); }
  if let Some(r) = map.get("tts_rate").and_then(|x| x.as_i64()) { obj.insert("tts_rate".to_string(), serde_json::Value::Number((r as i64).into())); }
  if let Some(v) = map.get("tts_volume").and_then(|x| x.as_i64()) { obj.insert("tts_volume".to_string(), serde_json::Value::Number((v as i64).into())); }
  if let Some(vl) = map.get("tts_voice_local").and_then(|x| x.as_str()) { obj.insert("tts_voice_local".to_string(), serde_json::Value::String(vl.to_string())); }
  if let Some(ov) = map.get("tts_openai_voice").and_then(|x| x.as_str()) { obj.insert("tts_openai_voice".to_string(), serde_json::Value::String(ov.to_string())); }
  if let Some(om) = map.get("tts_openai_model").and_then(|x| x.as_str()) { obj.insert("tts_openai_model".to_string(), serde_json::Value::String(om.to_string())); }
  if let Some(of) = map.get("tts_openai_format").and_then(|x| x.as_str()) { obj.insert("tts_openai_format".to_string(), serde_json::Value::String(of.to_string())); }
  if let Some(os) = map.get("tts_openai_streaming").and_then(|x| x.as_bool()) { obj.insert("tts_openai_streaming".to_string(), serde_json::Value::Bool(os)); }
  if let Some(ti) = map.get("tts_openai_instructions").and_then(|x| x.as_str()) { obj.insert("tts_openai_instructions".to_string(), serde_json::Value::String(ti.to_string())); }

  // Tokenizer mode
  if let Some(tm) = map.get("tokenizer_mode").and_then(|x| x.as_str()) { obj.insert("tokenizer_mode".to_string(), serde_json::Value::String(tm.to_string())); }

  // New STT preference keys
  if let Some(se) = map.get("stt_engine").and_then(|x| x.as_str()) { obj.insert("stt_engine".to_string(), serde_json::Value::String(se.to_string())); }
  if let Some(lm) = map.get("stt_local_model").and_then(|x| x.as_str()) { obj.insert("stt_local_model".to_string(), serde_json::Value::String(lm.to_string())); }
  if let Some(b) = map.get("stt_parakeet_has_cuda").and_then(|x| x.as_bool()) { obj.insert("stt_parakeet_has_cuda".to_string(), serde_json::Value::Bool(b)); }
  if let Some(bu) = map.get("stt_cloud_base_url").and_then(|x| x.as_str()) { obj.insert("stt_cloud_base_url".to_string(), serde_json::Value::String(bu.to_string())); }
  if let Some(sm) = map.get("stt_cloud_model").and_then(|x| x.as_str()) { obj.insert("stt_cloud_model".to_string(), serde_json::Value::String(sm.to_string())); }
  if let Some(sk) = map.get("stt_cloud_api_key").and_then(|x| x.as_str()) { obj.insert("stt_cloud_api_key".to_string(), serde_json::Value::String(sk.to_string())); }
  if let Some(did) = map.get("stt_input_device_id").and_then(|x| x.as_str()) { obj.insert("stt_input_device_id".to_string(), serde_json::Value::String(did.to_string())); }
  if let Some(pp) = map.get("stt_post_process_enabled").and_then(|x| x.as_bool()) { obj.insert("stt_post_process_enabled".to_string(), serde_json::Value::Bool(pp)); }
  if let Some(pm) = map.get("stt_post_process_model").and_then(|x| x.as_str()) { obj.insert("stt_post_process_model".to_string(), serde_json::Value::String(pm.to_string())); }
  if let Some(ppp) = map.get("stt_post_process_prompt").and_then(|x| x.as_str()) { obj.insert("stt_post_process_prompt".to_string(), serde_json::Value::String(ppp.to_string())); }
  if let Some(vocab) = map.get("stt_vocabulary").and_then(|x| x.as_str()) { obj.insert("stt_vocabulary".to_string(), serde_json::Value::String(vocab.to_string())); }
  // Whisper (local STT) model selection
  if let Some(u) = map.get("stt_whisper_model_url").and_then(|x| x.as_str()) { obj.insert("stt_whisper_model_url".to_string(), serde_json::Value::String(u.to_string())); }
  if let Some(preset) = map.get("stt_whisper_model_preset").and_then(|x| x.as_str()) { obj.insert("stt_whisper_model_preset".to_string(), serde_json::Value::String(preset.to_string())); }

  // Command Mode settings
  if let Some(enabled) = map.get("command_enabled").and_then(|x| x.as_bool()) { obj.insert("command_enabled".to_string(), serde_json::Value::Bool(enabled)); }
  if let Some(script) = map.get("command_active_script").and_then(|x| x.as_str()) { obj.insert("command_active_script".to_string(), serde_json::Value::String(script.to_string())); }
  if let Some(timeout) = map.get("command_hook_timeout_secs").and_then(|x| x.as_u64()) {
    obj.insert("command_hook_timeout_secs".to_string(), serde_json::Value::Number(serde_json::Number::from(timeout.clamp(5, 3600))));
  }

  // Remove deprecated local STT model selector keys if present
  obj.remove("stt_local_base_url");

  let pretty = serde_json::to_string_pretty(&serde_json::Value::Object(obj)).map_err(|e| format!("Serialize settings failed: {e}"))?;
  let tmp_path = path.with_extension("json.tmp");
  fs::write(&tmp_path, &pretty).map_err(|e| format!("Write settings failed: {e}"))?;
  // On Windows, fs::rename fails if target exists — remove first
  #[cfg(target_os = "windows")]
  { if path.exists() { let _ = fs::remove_file(&path); } }
  fs::rename(&tmp_path, &path).map_err(|e| format!("Rename settings failed: {e}"))?;
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
  let tmp_path = path.with_extension("json.tmp");
  fs::write(&tmp_path, &pretty).map_err(|e| format!("Write conversations failed: {e}"))?;
  #[cfg(target_os = "windows")]
  { if path.exists() { let _ = fs::remove_file(&path); } }
  fs::rename(&tmp_path, &path).map_err(|e| format!("Rename conversations failed: {e}"))?;
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn insert_mode_accepts_every_supported_value() {
    for mode in ["ctrl_v", "ctrl_shift_v", "shift_insert", "keystrokes", "none"] {
      assert_eq!(normalize_insert_mode(mode), mode);
    }
  }

  #[test]
  fn insert_mode_tolerates_surrounding_whitespace() {
    assert_eq!(normalize_insert_mode("  keystrokes \n"), "keystrokes");
  }

  #[test]
  fn insert_mode_falls_back_to_ctrl_v() {
    assert_eq!(normalize_insert_mode(""), "ctrl_v");
    assert_eq!(normalize_insert_mode("nonsense"), "ctrl_v");
    assert_eq!(normalize_insert_mode("Keystrokes"), "ctrl_v");
  }

  #[test]
  fn insert_mode_maps_the_retired_clipboard_value_to_ctrl_v() {
    assert_eq!(normalize_insert_mode("clipboard"), "ctrl_v");
  }

  #[test]
  fn clipboard_handling_defaults_to_leaving_the_clipboard_alone() {
    assert_eq!(normalize_clipboard_handling("copy_to_clipboard"), "copy_to_clipboard");
    assert_eq!(normalize_clipboard_handling("dont_modify"), "dont_modify");
    assert_eq!(normalize_clipboard_handling(""), "dont_modify");
    assert_eq!(normalize_clipboard_handling("whatever"), "dont_modify");
  }

  #[test]
  fn paste_delay_is_clamped_to_the_ceiling() {
    assert_eq!(normalize_paste_delay_ms(0), 0);
    assert_eq!(normalize_paste_delay_ms(120), 120);
    assert_eq!(normalize_paste_delay_ms(MAX_PASTE_DELAY_MS), MAX_PASTE_DELAY_MS);
    assert_eq!(normalize_paste_delay_ms(60_000), MAX_PASTE_DELAY_MS);
  }
}
