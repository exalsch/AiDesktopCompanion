// Screen layer: the shared windows-commander instance.
//
// windows-commander is the MCP server that sees and marks up the screen
// (annotations, activity timeline, pointer context). Its tracker and its
// overlays are per process, so every client has to talk to one long-running
// instance - the "shared instance" started with `--http` - or each would see
// its own timeline and draw its own arrows.
//
// This app is the one that is always running in the user's session, so it
// owns that instance:
//
// - At startup (main window process, once) it asks the exe `--status` and,
//   when nothing runs, starts `--http` detached and without a console.
// - It never stops the instance on exit: Claude Code, Hermes and anything else
//   may be connected to it. Settings offers an explicit "Stop shared instance".
// - The windows-commander MCP entry connects over HTTP with the bearer token
//   read from the discovery record at connect time (`mcp.rs`), so the token
//   never lands in settings.json.
//
// It also turns the instance's context tools into the short text item the
// voice session gets at the start of each turn (`format_context_item`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex as AsyncMutex;

use crate::mcp::ClientMap;

/// Value of an MCP server entry's `auth` field that means "read url and token
/// from the windows-commander discovery record".
pub const WC_AUTH: &str = "windows-commander";
/// Conventional id of the windows-commander entry in `mcp_servers`.
pub const WC_SERVER_ID: &str = "windows-commander";
const EXE_NAME: &str = "WindowsCommander.McpServer.exe";

/// Hard cap on the context item. It goes into every voice turn, so it is
/// billed on every turn; a few lines are enough to say what the user looks at.
pub const CONTEXT_MAX_CHARS: usize = 600;

// ---------------------------------------------------------------------------
// Discovery record
// ---------------------------------------------------------------------------

/// `%LOCALAPPDATA%\WindowsCommander\http.json`, written by `--http`.
///
/// `Debug` is implemented by hand so the token can never reach a log line.
#[derive(Clone, Deserialize, PartialEq)]
pub struct DiscoveryRecord {
  pub url: String,
  #[serde(default)]
  pub port: Option<u16>,
  #[serde(default)]
  pub pid: Option<u32>,
  #[serde(default)]
  pub token: String,
  #[serde(default)]
  pub version: Option<String>,
  #[serde(default)]
  pub running: bool,
}

impl std::fmt::Debug for DiscoveryRecord {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("DiscoveryRecord")
      .field("url", &self.url)
      .field("port", &self.port)
      .field("pid", &self.pid)
      .field("token", &if self.token.is_empty() { "<none>" } else { "<redacted>" })
      .field("version", &self.version)
      .field("running", &self.running)
      .finish()
  }
}

pub fn parse_discovery(text: &str) -> Result<DiscoveryRecord, String> {
  // PowerShell-written JSON may start with a BOM.
  let text = text.trim_start_matches('\u{feff}').trim();
  let rec: DiscoveryRecord = serde_json::from_str(text).map_err(|e| format!("discovery record is not valid JSON: {e}"))?;
  if rec.url.trim().is_empty() {
    return Err("discovery record has no url".into());
  }
  Ok(rec)
}

pub fn discovery_path() -> Option<PathBuf> {
  // The same override windows-commander itself honours (its tests use it).
  if let Ok(p) = std::env::var("WINDOWS_COMMANDER_DISCOVERY_FILE") {
    if !p.trim().is_empty() { return Some(PathBuf::from(p)); }
  }
  let base = std::env::var("LOCALAPPDATA").ok()?;
  Some(Path::new(&base).join("WindowsCommander").join("http.json"))
}

pub fn read_discovery() -> Result<DiscoveryRecord, String> {
  let path = discovery_path().ok_or_else(|| "LOCALAPPDATA is not set".to_string())?;
  let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
  parse_discovery(&text)
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

pub fn default_exe_path() -> String {
  match std::env::var("LOCALAPPDATA") {
    Ok(base) => Path::new(&base).join("WindowsCommander").join("server").join(EXE_NAME).to_string_lossy().to_string(),
    Err(_) => EXE_NAME.to_string(),
  }
}

pub fn exe_path_from(settings: &serde_json::Value) -> String {
  settings
    .get("screen_layer_exe_path")
    .and_then(|x| x.as_str())
    .map(|s| s.trim().trim_matches('"').to_string())
    .filter(|s| !s.is_empty())
    .unwrap_or_else(default_exe_path)
}

pub fn exe_path_from_settings() -> String {
  exe_path_from(&crate::config::load_settings_json())
}

/// Whether the app starts the shared instance. On unless switched off.
pub fn autostart_from_settings() -> bool {
  crate::config::load_settings_json()
    .get("screen_layer_autostart")
    .and_then(|x| x.as_bool())
    .unwrap_or(true)
}

/// Whether Assistant Mode sends the screen context item (and passes it to
/// Hermes). Lives in `assistant_realtime`; on unless switched off.
pub fn screen_context_enabled() -> bool {
  crate::config::load_settings_json()
    .get("assistant_realtime")
    .and_then(|ar| ar.get("screen_context"))
    .and_then(|x| x.as_bool())
    .unwrap_or(true)
}

/// Id of the MCP entry that is windows-commander: the one with
/// `auth: "windows-commander"`, else one called `windows-commander`.
pub fn wc_server_id() -> String {
  let v = crate::config::load_settings_json();
  if let Some(arr) = v.get("mcp_servers").and_then(|x| x.as_array()) {
    for s in arr {
      if s.get("auth").and_then(|x| x.as_str()) == Some(WC_AUTH) {
        if let Some(id) = s.get("id").and_then(|x| x.as_str()).filter(|s| !s.trim().is_empty()) {
          return id.to_string();
        }
      }
    }
  }
  WC_SERVER_ID.to_string()
}

// ---------------------------------------------------------------------------
// Settings migration
// ---------------------------------------------------------------------------

fn is_truthy(v: Option<&serde_json::Value>) -> bool {
  match v {
    Some(serde_json::Value::String(s)) => matches!(s.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
    Some(serde_json::Value::Bool(b)) => *b,
    Some(serde_json::Value::Number(n)) => n.as_i64().map(|i| i != 0).unwrap_or(false),
    _ => false,
  }
}

/// Move a stdio windows-commander entry onto the shared instance.
///
/// The entry becomes `transport: "http"` with `auth: "windows-commander"`, so
/// `mcp.rs` reads url and token from the discovery record when it connects. The
/// stdio side survives as `fallback_command`, pointed at the current exe
/// rather than the old publish folder: when the shared instance is not
/// reachable, that exe still bridges to it later or runs standalone. Env stays
/// as it was (the fallback uses it); `WINDOWS_COMMANDER_UNATTENDED` also
/// becomes the header that does the same over HTTP.
///
/// Runs once: `screen_layer_migrated` is set afterwards, so a user who moves
/// the entry back to stdio by hand keeps that choice. Returns whether
/// anything changed.
pub fn migrate_windows_commander_entry(settings: &mut serde_json::Value, exe: &str) -> bool {
  let Some(obj) = settings.as_object_mut() else { return false };
  if obj.get("screen_layer_migrated").and_then(|x| x.as_bool()) == Some(true) {
    return false;
  }
  let mut changed = false;
  if let Some(servers) = obj.get_mut("mcp_servers").and_then(|x| x.as_array_mut()) {
    for s in servers.iter_mut() {
      let Some(entry) = s.as_object_mut() else { continue };
      if entry.get("auth").and_then(|x| x.as_str()).map(|a| !a.is_empty()).unwrap_or(false) { continue; }
      let transport = entry.get("transport").and_then(|x| x.as_str()).unwrap_or("stdio");
      if transport == "http" || transport == "sse" { continue; }
      let id = entry.get("id").and_then(|x| x.as_str()).unwrap_or("");
      let command = entry.get("command").and_then(|x| x.as_str()).unwrap_or("");
      let is_wc = id.eq_ignore_ascii_case(WC_SERVER_ID)
        || command.to_ascii_lowercase().ends_with(&EXE_NAME.to_ascii_lowercase());
      if !is_wc { continue; }

      let unattended = is_truthy(entry.get("env").and_then(|e| e.get("WINDOWS_COMMANDER_UNATTENDED")));
      let mut headers = entry
        .get("headers")
        .and_then(|h| h.as_object().cloned())
        .unwrap_or_default();
      if unattended && !headers.keys().any(|k| k.eq_ignore_ascii_case("Windows-Commander-Unattended")) {
        headers.insert("Windows-Commander-Unattended".into(), serde_json::json!("1"));
      }
      entry.insert("transport".into(), serde_json::json!("http"));
      entry.insert("auth".into(), serde_json::json!(WC_AUTH));
      entry.insert("command".into(), serde_json::json!(""));
      entry.insert("fallback_command".into(), serde_json::json!(exe));
      entry.insert("headers".into(), serde_json::Value::Object(headers));
      changed = true;
    }
  }
  if changed {
    obj.insert("screen_layer_migrated".into(), serde_json::json!(true));
  }
  changed
}

/// Apply `migrate_windows_commander_entry` to settings.json. Called once at
/// startup, before the webview loads settings.
pub fn migrate_settings_file() {
  let mut v = crate::config::load_settings_json();
  let exe = exe_path_from(&v);
  if migrate_windows_commander_entry(&mut v, &exe) {
    match crate::config::write_settings_json(&v) {
      Ok(_) => log::info!("screen layer: moved the windows-commander MCP entry to the shared instance (HTTP, stdio fallback {exe})"),
      Err(e) => log::warn!("screen layer: settings migration not saved: {e}"),
    }
  }
}

// ---------------------------------------------------------------------------
// Process control
// ---------------------------------------------------------------------------

#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(target_os = "windows")]
const DETACHED_PROCESS: u32 = 0x0000_0008;
#[cfg(target_os = "windows")]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
#[cfg(target_os = "windows")]
const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

/// Run the exe with `args`, hidden, and return (exit code, stdout+stderr).
/// Bounded: a `--status` that hangs must not hold startup or a settings click.
fn run_exe(exe: &str, args: &[&str], timeout: Duration) -> Result<(i32, String), String> {
  let mut cmd = std::process::Command::new(exe);
  cmd.args(args)
    .stdin(std::process::Stdio::null())
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped());
  #[cfg(target_os = "windows")]
  {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
  }
  let mut child = cmd.spawn().map_err(|e| format!("cannot run {exe}: {e}"))?;
  let started = std::time::Instant::now();
  loop {
    match child.try_wait() {
      Ok(Some(status)) => {
        let mut out = String::new();
        if let Some(mut so) = child.stdout.take() { let _ = std::io::Read::read_to_string(&mut so, &mut out); }
        if let Some(mut se) = child.stderr.take() { let _ = std::io::Read::read_to_string(&mut se, &mut out); }
        return Ok((status.code().unwrap_or(-1), out.trim().to_string()));
      }
      Ok(None) if started.elapsed() > timeout => {
        let _ = child.kill();
        return Err(format!("{exe} {} did not finish within {}s", args.join(" "), timeout.as_secs()));
      }
      Ok(None) => std::thread::sleep(Duration::from_millis(50)),
      Err(e) => return Err(format!("waiting for {exe} failed: {e}")),
    }
  }
}

/// `--status`: exit 0 when the shared instance runs, 1 when not.
pub fn is_running(exe: &str) -> Result<bool, String> {
  run_exe(exe, &["--status"], Duration::from_secs(10)).map(|(code, _)| code == 0)
}

/// Start `--http` so it outlives this app: no console, its own process group,
/// and out of any job object the app runs in (a dev launcher's job would kill
/// it with the app otherwise). Breakaway is refused when the job does not
/// allow it, so that is tried first and dropped on failure.
fn spawn_detached(exe: &str) -> Result<(), String> {
  let build = |flags: u32| {
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--http")
      .stdin(std::process::Stdio::null())
      .stdout(std::process::Stdio::null())
      .stderr(std::process::Stdio::null());
    #[cfg(target_os = "windows")]
    {
      use std::os::windows::process::CommandExt;
      cmd.creation_flags(flags);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = flags;
    cmd
  };
  #[cfg(target_os = "windows")]
  let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
  #[cfg(not(target_os = "windows"))]
  let base = 0u32;
  #[cfg(target_os = "windows")]
  let first = build(base | CREATE_BREAKAWAY_FROM_JOB).spawn();
  #[cfg(not(target_os = "windows"))]
  let first = build(base).spawn();
  match first {
    Ok(_) => Ok(()),
    Err(_) => build(base).spawn().map(|_| ()).map_err(|e| format!("cannot start {exe} --http: {e}")),
  }
}

/// Start the shared instance unless it runs, then wait until `--status` says
/// it is up (a few seconds at most).
pub fn start_shared_instance(exe: &str) -> Result<String, String> {
  if !Path::new(exe).is_file() {
    return Err(format!("windows-commander not found at {exe}"));
  }
  if is_running(exe)? {
    return Ok("already running".into());
  }
  spawn_detached(exe)?;
  for _ in 0..30 {
    std::thread::sleep(Duration::from_millis(300));
    if is_running(exe).unwrap_or(false) {
      return Ok("started".into());
    }
  }
  Err("started windows-commander --http, but it did not report running within 9s".into())
}

/// Set once the startup check has finished, either way. A windows-commander
/// connect waits for it, so the app's own MCP client does not race the
/// instance it is about to start and end up on a standalone process with its
/// own, separate timeline.
static STARTUP_DONE: AtomicBool = AtomicBool::new(false);

pub async fn wait_for_startup(max: Duration) {
  let started = std::time::Instant::now();
  while !STARTUP_DONE.load(Ordering::SeqCst) && started.elapsed() < max {
    tokio::time::sleep(Duration::from_millis(200)).await;
  }
}

/// Startup hook (main process, once): migrate the MCP entry, then make sure
/// the shared instance runs. On a worker thread so the window is not held up.
pub fn on_app_start() {
  migrate_settings_file();
  std::thread::spawn(|| {
    if autostart_from_settings() {
      let exe = exe_path_from_settings();
      match start_shared_instance(&exe) {
        Ok(msg) => log::info!("screen layer: shared windows-commander instance {msg}"),
        Err(e) => log::warn!("screen layer: {e}"),
      }
    }
    STARTUP_DONE.store(true, Ordering::SeqCst);
  });
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenLayerStatus {
  pub autostart: bool,
  pub exe_path: String,
  pub exe_found: bool,
  pub running: bool,
  pub version: Option<String>,
  pub url: Option<String>,
  pub pid: Option<u32>,
  /// What went wrong, or empty.
  pub detail: String,
}

fn current_status() -> ScreenLayerStatus {
  let exe = exe_path_from_settings();
  let exe_found = Path::new(&exe).is_file();
  let mut st = ScreenLayerStatus {
    autostart: autostart_from_settings(),
    exe_path: exe.clone(),
    exe_found,
    running: false,
    version: None,
    url: None,
    pid: None,
    detail: String::new(),
  };
  let rec = read_discovery();
  if exe_found {
    match is_running(&exe) {
      Ok(r) => st.running = r,
      Err(e) => st.detail = e,
    }
  } else {
    // Without the exe only the record can tell; it says `running: false`
    // after a clean stop but cannot notice a crash.
    st.running = rec.as_ref().map(|r| r.running).unwrap_or(false);
    st.detail = format!("windows-commander not found at {exe}");
  }
  if st.running {
    if let Ok(r) = rec {
      st.version = r.version;
      st.url = Some(r.url);
      st.pid = r.pid;
    }
  }
  st
}

#[tauri::command]
pub async fn screen_layer_status() -> Result<ScreenLayerStatus, String> {
  tauri::async_runtime::spawn_blocking(current_status).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn screen_layer_start() -> Result<ScreenLayerStatus, String> {
  tauri::async_runtime::spawn_blocking(|| {
    start_shared_instance(&exe_path_from_settings())?;
    Ok(current_status())
  })
  .await
  .map_err(|e| e.to_string())?
}

/// Stop the shared instance. Every client of it loses the connection, which
/// is why this is only ever a button and never part of app exit.
#[tauri::command]
pub async fn screen_layer_stop() -> Result<ScreenLayerStatus, String> {
  tauri::async_runtime::spawn_blocking(|| {
    let exe = exe_path_from_settings();
    if !Path::new(&exe).is_file() {
      return Err(format!("windows-commander not found at {exe}"));
    }
    let (code, out) = run_exe(&exe, &["--stop"], Duration::from_secs(15))?;
    // 1 = was not running, which is the state the user asked for.
    if code != 0 && code != 1 {
      return Err(format!("--stop failed (exit {code}): {out}"));
    }
    Ok(current_status())
  })
  .await
  .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------------------
// Screen context for the voice session
// ---------------------------------------------------------------------------

/// The JSON a windows-commander tool returned: its first text block, parsed.
pub fn tool_result_json(result: &serde_json::Value) -> Option<serde_json::Value> {
  let items = result.get("content")?.as_array()?;
  for it in items {
    if it.get("type").and_then(|t| t.as_str()) != Some("text") { continue; }
    let text = it.get("text").and_then(|t| t.as_str())?;
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
      return Some(v);
    }
  }
  None
}

fn clip(s: &str, max: usize) -> String {
  let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
  if s.chars().count() <= max { return s; }
  let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
  out.push_str("...");
  out
}

fn str_field<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
  v.get(key).and_then(|x| x.as_str()).map(|s| s.trim()).filter(|s| !s.is_empty())
}

fn describe_window(entry: &serde_json::Value, title_max: usize) -> String {
  let process = str_field(entry, "processName").unwrap_or("unknown app");
  if entry.get("blocked").and_then(|b| b.as_bool()) == Some(true) {
    return format!("{process} (private window, details withheld)");
  }
  match str_field(entry, "title") {
    Some(t) => format!("{process} \"{}\"", clip(t, title_max)),
    None => process.to_string(),
  }
}

fn describe_element(el: &serde_json::Value) -> Option<String> {
  let ctype = str_field(el, "controlType").unwrap_or("element");
  let name = str_field(el, "name");
  let mut out = match name {
    Some(n) => format!("{ctype} \"{}\"", clip(n, 60)),
    None => ctype.to_string(),
  };
  if name.is_none() && str_field(el, "controlType").is_none() { return None; }
  if let Some(r) = str_field(el, "elementRef").filter(|r| r.len() <= 48) {
    out.push_str(&format!(" [element_ref {r}]"));
  }
  Some(out)
}

/// The context item, from `get_current_context` and `get_pointer_context`
/// (both already parsed out of their tool results).
///
/// A few labelled lines, never more than `CONTEXT_MAX_CHARS`: the recent
/// history is shortened first, then the whole text is cut. Element values and
/// window handles are left out on purpose - names say what the user looks at,
/// values are what they typed.
pub fn format_context_item(current: &serde_json::Value, pointer: Option<&serde_json::Value>) -> String {
  let mut head: Vec<String> = vec!["[Screen context: what the user has in front of them right now. Not said by the user; do not read it out.]".into()];
  let now = current.get("current").unwrap_or(&serde_json::Value::Null);
  let now_seq = now.get("sequence").and_then(|s| s.as_i64());
  if now.is_object() {
    head.push(format!("Foreground: {}", describe_window(now, 90)));
    let blocked = now.get("blocked").and_then(|b| b.as_bool()) == Some(true);
    if !blocked {
      if let Some(url) = str_field(now, "url") { head.push(format!("URL: {}", clip(url, 120))); }
      if let Some(fe) = now.get("focusedElement").and_then(describe_element) { head.push(format!("Focused: {fe}")); }
    }
  }
  if let Some(p) = pointer {
    let window = p.get("window");
    let blocked = p.get("blocked").and_then(|b| b.as_bool()) == Some(true);
    let line = if blocked {
      let process = window.and_then(|w| str_field(w, "processName")).unwrap_or("an app");
      Some(format!("Under the cursor: {process} (private window, details withheld)"))
    } else if let Some(el) = p.get("element").and_then(describe_element) {
      match window.and_then(|w| str_field(w, "processName")) {
        Some(proc_) => Some(format!("Under the cursor: {el} in {proc_}")),
        None => Some(format!("Under the cursor: {el}")),
      }
    } else {
      window.map(|w| format!("Under the cursor: window {}", describe_window(w, 60)))
    };
    if let Some(l) = line { head.push(l); }
  }

  let recent: Vec<String> = current
    .get("recent")
    .and_then(|r| r.as_array())
    .map(|arr| {
      arr.iter()
        .filter(|e| e.get("sequence").and_then(|s| s.as_i64()) != now_seq || now_seq.is_none())
        .take(5)
        .map(|e| {
          let w = describe_window(e, 40);
          match e.get("dwellMs").and_then(|d| d.as_i64()) {
            Some(ms) if ms >= 1000 => format!("{w} ({}s)", ms / 1000),
            _ => w,
          }
        })
        .collect()
    })
    .unwrap_or_default();

  let base = head.join("\n");
  let mut out = base.clone();
  // Add as many recent entries as fit; the newest are the useful ones.
  for n in (1..=recent.len()).rev() {
    let candidate = format!("{base}\nBefore that: {}", recent[..n].join("; "));
    if candidate.chars().count() <= CONTEXT_MAX_CHARS {
      out = candidate;
      break;
    }
  }
  clip_lines(&out, CONTEXT_MAX_CHARS)
}

/// Cut to `max` chars, keeping line breaks (`clip` would fold them).
fn clip_lines(s: &str, max: usize) -> String {
  if s.chars().count() <= max { return s.to_string(); }
  let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
  out.push_str("...");
  out
}

/// The last item sent to the voice session, so an unchanged screen is not
/// sent again on the next turn.
static LAST_CONTEXT: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

async fn call_json(clients: &AsyncMutex<ClientMap>, server: &str, tool: &str, args: serde_json::Value) -> Option<serde_json::Value> {
  match tokio::time::timeout(Duration::from_secs(3), crate::mcp::call_tool(clients, server, tool, args)).await {
    Ok(Ok(res)) => tool_result_json(&res),
    Ok(Err(e)) => { log::debug!("screen context: {tool} failed: {e}"); None }
    Err(_) => { log::debug!("screen context: {tool} timed out"); None }
  }
}

/// Fresh context text, or None when windows-commander is not connected or
/// screen context is switched off.
pub async fn current_context_text(clients: &AsyncMutex<ClientMap>) -> Option<String> {
  if !screen_context_enabled() { return None; }
  let server = wc_server_id();
  {
    let map = clients.lock().await;
    if !map.contains_key(&server) { return None; }
  }
  let (current, pointer) = tokio::join!(
    call_json(clients, &server, "get_current_context", serde_json::json!({})),
    call_json(clients, &server, "get_pointer_context", serde_json::json!({ "include_image": false })),
  );
  let current = current?;
  Some(format_context_item(&current, pointer.as_ref()))
}

/// Context item for a new voice turn, or None when there is nothing new.
/// `reset` forgets the previous one (a new session has not seen it).
pub async fn turn_context(clients: &AsyncMutex<ClientMap>, reset: bool) -> Option<String> {
  if reset {
    if let Ok(mut last) = LAST_CONTEXT.lock() { *last = None; }
  }
  let text = current_context_text(clients).await?;
  let mut last = LAST_CONTEXT.lock().ok()?;
  if last.as_deref() == Some(text.as_str()) {
    return None;
  }
  *last = Some(text.clone());
  Some(text)
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn parses_discovery_record_and_hides_token() {
    let text = "\u{feff}{\"url\":\"http://127.0.0.1:47821/mcp\",\"port\":47821,\"pid\":63680,\"token\":\"s3cret\",\"version\":\"0.1.1\",\"startedAt\":\"x\",\"instanceId\":\"y\",\"running\":true,\"stoppedAt\":null}";
    let rec = parse_discovery(text).unwrap();
    assert_eq!(rec.url, "http://127.0.0.1:47821/mcp");
    assert_eq!(rec.port, Some(47821));
    assert_eq!(rec.pid, Some(63680));
    assert_eq!(rec.version.as_deref(), Some("0.1.1"));
    assert!(rec.running);
    assert_eq!(rec.token, "s3cret");
    assert!(!format!("{rec:?}").contains("s3cret"));
  }

  #[test]
  fn discovery_record_after_stop_and_garbage() {
    let rec = parse_discovery(r#"{"url":"http://127.0.0.1:47821/mcp","running":false,"token":"t"}"#).unwrap();
    assert!(!rec.running);
    assert!(parse_discovery("not json").is_err());
    assert!(parse_discovery(r#"{"url":"  "}"#).is_err());
  }

  fn legacy_settings() -> serde_json::Value {
    json!({
      "openai_api_key": "k",
      "mcp_servers": [
        { "id": "CoTiRe", "transport": "stdio", "command": "C:\\cotire-mcp-stdio.exe", "args": [], "env": {} },
        { "id": "windows-commander", "transport": "stdio",
          "command": "C:\\Programming\\MCP_Servers\\windows-commander-mcp\\publish\\WindowsCommander.McpServer.exe",
          "args": [], "cwd": "", "env": { "WINDOWS_COMMANDER_UNATTENDED": "1" }, "auto_connect": true, "disabled_tools": ["kill_process"] }
      ]
    })
  }

  #[test]
  fn migrates_the_stdio_entry_to_the_shared_instance() {
    let mut v = legacy_settings();
    assert!(migrate_windows_commander_entry(&mut v, "C:\\new\\WindowsCommander.McpServer.exe"));
    let wc = &v["mcp_servers"][1];
    assert_eq!(wc["transport"], "http");
    assert_eq!(wc["auth"], WC_AUTH);
    assert_eq!(wc["command"], "");
    assert_eq!(wc["fallback_command"], "C:\\new\\WindowsCommander.McpServer.exe");
    assert_eq!(wc["headers"]["Windows-Commander-Unattended"], "1");
    // Everything else is kept.
    assert_eq!(wc["env"]["WINDOWS_COMMANDER_UNATTENDED"], "1");
    assert_eq!(wc["auto_connect"], true);
    assert_eq!(wc["disabled_tools"][0], "kill_process");
    // Other servers are untouched, no token is written anywhere.
    assert_eq!(v["mcp_servers"][0]["transport"], "stdio");
    assert!(v["mcp_servers"][0].get("auth").is_none());
    assert!(!v.to_string().to_lowercase().contains("bearer"));
    assert_eq!(v["screen_layer_migrated"], true);
  }

  #[test]
  fn migration_runs_once_and_matches_by_exe_name() {
    let mut v = legacy_settings();
    assert!(migrate_windows_commander_entry(&mut v, "x.exe"));
    // The user moves it back to stdio: left alone from now on.
    v["mcp_servers"][1]["transport"] = json!("stdio");
    v["mcp_servers"][1]["auth"] = json!("");
    assert!(!migrate_windows_commander_entry(&mut v, "x.exe"));
    assert_eq!(v["mcp_servers"][1]["transport"], "stdio");

    let mut renamed = json!({ "mcp_servers": [
      { "id": "screen", "transport": "stdio", "command": "D:\\tools\\windowscommander.mcpserver.exe", "env": {} }
    ]});
    assert!(migrate_windows_commander_entry(&mut renamed, "x.exe"));
    assert_eq!(renamed["mcp_servers"][0]["auth"], WC_AUTH);
    // No unattended env, no header.
    assert!(renamed["mcp_servers"][0]["headers"].as_object().unwrap().is_empty());

    let mut none = json!({ "mcp_servers": [{ "id": "Rob", "transport": "stdio", "command": "node.exe" }] });
    assert!(!migrate_windows_commander_entry(&mut none, "x.exe"));
    assert!(none.get("screen_layer_migrated").is_none());
  }

  fn sample_current() -> serde_json::Value {
    json!({
      "current": { "sequence": 121, "processName": "msedge", "title": "Pull request #41 - GitHub",
                   "url": "https://github.com/exalsch/AiDesktopCompanion/pull/41",
                   "focusedElement": { "name": "Merge pull request", "controlType": "Button", "automationId": "" } },
      "currentForMs": 5000,
      "recent": [
        { "sequence": 121, "processName": "msedge", "title": "Pull request #41 - GitHub" },
        { "sequence": 120, "processName": "olk", "title": "FW: CoTiRe - status update", "dwellMs": 4385 },
        { "sequence": 119, "processName": "KeePassXC", "blocked": true, "dwellMs": 2000 },
        { "sequence": 118, "processName": "explorer", "title": "Desktop - File Explorer", "dwellMs": 300 }
      ],
      "cursor": 121, "trackerRunning": true
    })
  }

  #[test]
  fn formats_a_compact_context_item() {
    let pointer = json!({
      "x": 10, "y": 20,
      "window": { "processName": "msedge", "title": "Pull request #41 - GitHub" },
      "element": { "elementRef": "e:4f2a", "name": "Files changed", "controlType": "TabItem", "value": "SECRET TEXT" }
    });
    let text = format_context_item(&sample_current(), Some(&pointer));
    assert!(text.contains("Foreground: msedge \"Pull request #41 - GitHub\""), "{text}");
    assert!(text.contains("URL: https://github.com/exalsch/AiDesktopCompanion/pull/41"));
    assert!(text.contains("Focused: Button \"Merge pull request\""));
    assert!(text.contains("Under the cursor: TabItem \"Files changed\" [element_ref e:4f2a] in msedge"));
    assert!(text.contains("olk \"FW: CoTiRe - status update\" (4s)"));
    assert!(text.contains("KeePassXC (private window, details withheld)"));
    // The current entry is not repeated in the history; values never appear.
    assert_eq!(text.matches("Pull request #41").count(), 1);
    assert!(!text.contains("SECRET"));
    assert!(text.chars().count() <= CONTEXT_MAX_CHARS);
  }

  #[test]
  fn blocked_foreground_and_pointer_reveal_nothing() {
    let current = json!({ "current": { "sequence": 5, "processName": "KeePassXC", "blocked": true }, "recent": [] });
    let pointer = json!({ "x": 1, "y": 1, "blocked": true, "window": { "processName": "KeePassXC" } });
    let text = format_context_item(&current, Some(&pointer));
    assert!(text.contains("Foreground: KeePassXC (private window, details withheld)"));
    assert!(text.contains("Under the cursor: KeePassXC (private window"));
    assert!(!text.contains("Before that"));
  }

  #[test]
  fn long_context_is_capped() {
    let long = "x".repeat(400);
    let mut current = sample_current();
    current["current"]["title"] = json!(long.clone());
    current["current"]["url"] = json!(format!("https://example.com/{long}"));
    let recent: Vec<_> = (0..10).map(|i| json!({ "sequence": i, "processName": "app", "title": long.clone(), "dwellMs": 9000 })).collect();
    current["recent"] = json!(recent);
    let text = format_context_item(&current, None);
    assert!(text.chars().count() <= CONTEXT_MAX_CHARS, "{} chars", text.chars().count());
    assert!(text.contains("Foreground: msedge"));
  }

  #[test]
  fn reads_json_out_of_a_tool_result() {
    let res = json!({ "content": [{ "type": "text", "text": "{\"x\":919}" }] });
    assert_eq!(tool_result_json(&res).unwrap()["x"], 919);
    assert!(tool_result_json(&json!({ "content": [{ "type": "text", "text": "nope" }] })).is_none());
  }
}
