use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use tauri::Emitter;

#[cfg(target_os = "windows")]
use std::ffi::c_void;
#[cfg(target_os = "windows")]
use windows::core::PWSTR;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
  OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

static COMMAND_RUNNING: AtomicBool = AtomicBool::new(false);

const LOG_FILE_NAME: &str = "command-hook.log";
const DEFAULT_TIMEOUT_SECS: u64 = 120;
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

fn app_base_dir() -> Result<PathBuf, String> {
  #[cfg(target_os = "windows")]
  {
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    let mut p = PathBuf::from(appdata);
    p.push("AiDesktopCompanion");
    Ok(p)
  }

  #[cfg(not(target_os = "windows"))]
  {
    let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    let mut p = PathBuf::from(home);
    p.push(".config");
    p.push("AiDesktopCompanion");
    Ok(p)
  }
}

fn hooks_dir_path() -> Result<PathBuf, String> {
  let mut p = app_base_dir()?;
  p.push("hooks");
  Ok(p)
}

fn logs_dir_path() -> Result<PathBuf, String> {
  let mut p = app_base_dir()?;
  p.push("logs");
  Ok(p)
}

fn ensure_hooks_dir() -> Result<PathBuf, String> {
  let p = hooks_dir_path()?;
  fs::create_dir_all(&p).map_err(|e| format!("Failed to create hooks directory: {e}"))?;
  Ok(p)
}

fn ensure_logs_dir() -> Result<PathBuf, String> {
  let p = logs_dir_path()?;
  fs::create_dir_all(&p).map_err(|e| format!("Failed to create logs directory: {e}"))?;
  Ok(p)
}

fn log_file_path() -> Result<PathBuf, String> {
  let mut p = ensure_logs_dir()?;
  p.push(LOG_FILE_NAME);
  Ok(p)
}

fn append_log_line(line: &str) {
  let path = match log_file_path() {
    Ok(p) => p,
    Err(_) => return,
  };

  if let Ok(meta) = fs::metadata(&path) {
    if meta.len() > MAX_LOG_BYTES {
      let _ = fs::remove_file(&path);
    }
  }

  let header = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
  let full = format!("[{header}] {line}\n");
  if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
    let _ = f.write_all(full.as_bytes());
  }
}

fn supported_script_extensions() -> &'static [&'static str] {
  #[cfg(target_os = "windows")]
  {
    &["cmd", "bat", "ps1", "exe"]
  }

  #[cfg(not(target_os = "windows"))]
  {
    &[""]
  }
}

fn has_supported_extension(path: &Path) -> bool {
  let ext = path
    .extension()
    .and_then(|x| x.to_str())
    .unwrap_or("")
    .trim()
    .to_lowercase();
  supported_script_extensions().iter().any(|e| *e == ext)
}

fn sanitize_script_name(filename: &str) -> Result<String, String> {
  let trimmed = filename.trim();
  if trimmed.is_empty() {
    return Err("No command script configured".to_string());
  }
  if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
    return Err("Invalid command script filename".to_string());
  }
  Ok(trimmed.to_string())
}

fn resolve_script_path_from_name(filename: &str) -> Result<PathBuf, String> {
  let safe_name = sanitize_script_name(filename)?;
  let mut p = hooks_dir_path()?;
  p.push(safe_name);
  if !p.exists() || !p.is_file() {
    return Err("Configured command script does not exist".to_string());
  }
  if !has_supported_extension(&p) {
    return Err("Configured command script has unsupported extension".to_string());
  }
  Ok(p)
}

fn command_timeout_secs() -> u64 {
  let settings = crate::config::load_settings_json();
  let raw = settings
    .get("command_hook_timeout_secs")
    .and_then(|x| x.as_u64())
    .unwrap_or(DEFAULT_TIMEOUT_SECS);
  raw.clamp(5, 3600)
}

fn command_enabled() -> bool {
  let settings = crate::config::load_settings_json();
  settings
    .get("command_enabled")
    .and_then(|x| x.as_bool())
    .unwrap_or(false)
}

fn configured_script_name() -> String {
  let settings = crate::config::load_settings_json();
  settings
    .get("command_active_script")
    .and_then(|x| x.as_str())
    .unwrap_or("")
    .trim()
    .to_string()
}

#[cfg(target_os = "windows")]
fn active_app_name_from_last_foreground() -> String {
  let hraw = match crate::quick_actions::last_foreground_handle_raw() {
    Some(v) => v,
    None => return String::new(),
  };

  unsafe {
    let hwnd = HWND(hraw as *mut c_void);
    if hwnd.0.is_null() {
      return String::new();
    }

    let mut pid: u32 = 0;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
      return String::new();
    }

    let process: HANDLE = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
      Ok(h) => h,
      Err(_) => return String::new(),
    };

    let mut size: u32 = 32768;
    let mut buf = vec![0u16; size as usize];
    let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_FORMAT(0), PWSTR(buf.as_mut_ptr()), &mut size).is_ok();
    let _ = CloseHandle(process);

    if !ok || size == 0 {
      return String::new();
    }

    let full = String::from_utf16_lossy(&buf[..size as usize]);
    Path::new(&full)
      .file_name()
      .and_then(|x| x.to_str())
      .unwrap_or("")
      .to_string()
  }
}

#[cfg(not(target_os = "windows"))]
fn active_app_name_from_last_foreground() -> String {
  String::new()
}

fn collect_context_env(transcript: &str, selected_text: Option<String>) -> Vec<(String, String)> {
  let selected = selected_text.unwrap_or_else(crate::quick_actions::last_selected_text);
  let active_app = active_app_name_from_last_foreground();
  let clipboard = Clipboard::new()
    .ok()
    .and_then(|mut c| c.get_text().ok())
    .unwrap_or_default();

  vec![
    ("AIDC_TRANSCRIPT".to_string(), transcript.to_string()),
    ("AIDC_ACTIVE_APP".to_string(), active_app),
    ("AIDC_CLIPBOARD".to_string(), clipboard),
    ("AIDC_SELECTED_TEXT".to_string(), selected),
  ]
}

fn build_command_for_script(script_path: &Path) -> Result<Command, String> {
  let ext = script_path
    .extension()
    .and_then(|x| x.to_str())
    .unwrap_or("")
    .to_lowercase();

  #[cfg(target_os = "windows")]
  {
    let cmd = if ext == "ps1" {
      let mut c = Command::new("powershell.exe");
      c.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
      ]);
      c.arg(script_path);
      c
    } else if ext == "cmd" || ext == "bat" {
      let mut c = Command::new("cmd.exe");
      c.arg("/c");
      c.arg(script_path);
      c
    } else if ext == "exe" {
      Command::new(script_path)
    } else {
      return Err("Unsupported command script extension".to_string());
    };
    Ok(cmd)
  }

  #[cfg(not(target_os = "windows"))]
  {
    let _ = ext;
    Ok(Command::new(script_path))
  }
}

fn emit_command_state(app: &tauri::AppHandle, state: &str) {
  let _ = app.emit("command:state", serde_json::json!({ "state": state }));
}

fn emit_command_error(app: &tauri::AppHandle, message: &str) {
  let _ = app.emit("command:error", serde_json::json!({ "message": message }));
}

fn clear_running_and_emit_idle(app: &tauri::AppHandle) {
  COMMAND_RUNNING.store(false, Ordering::SeqCst);
  emit_command_state(app, "idle");
}

#[tauri::command]
pub fn command_is_running() -> Result<bool, String> {
  Ok(COMMAND_RUNNING.load(Ordering::SeqCst))
}

#[tauri::command]
pub fn list_command_scripts() -> Result<Vec<String>, String> {
  let dir = ensure_hooks_dir()?;
  let mut out: Vec<String> = Vec::new();

  let entries = fs::read_dir(&dir).map_err(|e| format!("Failed to list hooks directory: {e}"))?;
  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    let name = match path.file_name().and_then(|x| x.to_str()) {
      Some(v) => v,
      None => continue,
    };
    if name.starts_with('.') {
      continue;
    }
    if has_supported_extension(&path) {
      out.push(name.to_string());
    }
  }

  out.sort_unstable();
  Ok(out)
}

#[tauri::command]
pub fn create_default_command_script() -> Result<String, String> {
  let hooks_dir = ensure_hooks_dir()?;
  let file_name = "command.ps1";
  let path = hooks_dir.join(file_name);

  if !path.exists() {
    let template = r#"# AiDesktopCompanion - Command Mode hook (default template)
# Receives transcribed utterance on stdin. Edit this file to route voice
# commands to your own tools, scripts, terminal sessions, or AI agents.

$ErrorActionPreference = 'Continue'
$log = "$env:APPDATA\AiDesktopCompanion\logs\command-hook.log"
$transcript = [Console]::In.ReadToEnd().Trim()
Add-Content -Path $log -Value "[$(Get-Date -Format o)] in=$transcript"
Add-Content -Path $log -Value "  active=$env:AIDC_ACTIVE_APP clip=($($env:AIDC_CLIPBOARD.Length) chars) sel=($($env:AIDC_SELECTED_TEXT.Length) chars)"

# Example router block (optional): utterances starting with "Rob"
if ($transcript -match '^(?i)Rob\b[\s,\.;:!?-]*(.+)$') {
    $msg = $Matches[1].Trim()
    # Add your routing logic here
}

exit 0
"#;
    fs::write(&path, template).map_err(|e| format!("Failed to write default command script: {e}"))?;
  }

  Ok(file_name.to_string())
}

#[tauri::command]
pub fn open_command_hooks_folder() -> Result<(), String> {
  let hooks = ensure_hooks_dir()?;
  #[cfg(target_os = "windows")]
  {
    Command::new("explorer.exe")
      .arg(hooks)
      .spawn()
      .map_err(|e| format!("Failed to open hooks folder: {e}"))?;
    return Ok(());
  }

  #[cfg(not(target_os = "windows"))]
  {
    let _ = hooks;
    Err("Opening hooks folder is not implemented for this platform".to_string())
  }
}

#[tauri::command]
pub fn run_command_hook(
  app: tauri::AppHandle,
  transcript: String,
  selected_text: Option<String>,
) -> Result<(), String> {
  let trimmed = transcript.trim().to_string();
  if trimmed.is_empty() {
    append_log_line("command hook skipped: empty transcript");
    return Ok(());
  }

  if !command_enabled() {
    return Err("Command Mode is disabled in settings".to_string());
  }

  if COMMAND_RUNNING
    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
    .is_err()
  {
    emit_command_state(&app, "running");
    return Ok(());
  }

  let script_name = configured_script_name();
  let script_path = match resolve_script_path_from_name(&script_name) {
    Ok(p) => p,
    Err(_) => {
      let msg = "No command script configured - see Settings -> Command Mode";
      append_log_line(msg);
      emit_command_error(&app, msg);
      clear_running_and_emit_idle(&app);
      return Err(msg.to_string());
    }
  };

  let mut cmd = match build_command_for_script(&script_path) {
    Ok(c) => c,
    Err(e) => {
      append_log_line(&format!("command hook build failed: {e}"));
      emit_command_error(&app, &e);
      clear_running_and_emit_idle(&app);
      return Err(e);
    }
  };

  let log_path = log_file_path()?;
  let stdout_log = fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&log_path)
    .map_err(|e| format!("Failed to open command log file: {e}"))?;
  let stderr_log = fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(&log_path)
    .map_err(|e| format!("Failed to open command log file: {e}"))?;

  cmd.stdin(Stdio::piped())
    .stdout(Stdio::from(stdout_log))
    .stderr(Stdio::from(stderr_log));

  let context_env = collect_context_env(&trimmed, selected_text);
  for (k, v) in context_env {
    cmd.env(k, v);
  }

  let mut child = match cmd.spawn() {
    Ok(c) => c,
    Err(e) => {
      let msg = format!("Failed to start command script: {e}");
      append_log_line(&msg);
      emit_command_error(&app, &msg);
      clear_running_and_emit_idle(&app);
      return Err(msg);
    }
  };

  if let Some(stdin) = child.stdin.as_mut() {
    if let Err(e) = stdin.write_all(trimmed.as_bytes()) {
      append_log_line(&format!("command hook stdin write failed: {e}"));
    }
  }
  drop(child.stdin.take());

  emit_command_state(&app, "running");
  append_log_line(&format!(
    "command hook started: script={} timeout={}s",
    script_path.to_string_lossy(),
    command_timeout_secs()
  ));

  let app_for_watcher = app.clone();
  let timeout = command_timeout_secs();
  std::thread::spawn(move || {
    let start = Instant::now();
    let mut timed_out = false;
    let mut exit_code: Option<i32> = None;

    loop {
      match child.try_wait() {
        Ok(Some(status)) => {
          exit_code = status.code();
          break;
        }
        Ok(None) => {
          if start.elapsed() >= Duration::from_secs(timeout) {
            timed_out = true;
            let _ = child.kill();
            if let Ok(status) = child.wait() {
              exit_code = status.code();
            }
            break;
          }
          std::thread::sleep(Duration::from_millis(200));
        }
        Err(e) => {
          append_log_line(&format!("command hook watcher wait failed: {e}"));
          break;
        }
      }
    }

    if timed_out {
      append_log_line("command hook timeout reached; process killed");
    }

    append_log_line(&format!(
      "command hook finished: exit_code={} timed_out={}",
      exit_code
        .map(|x| x.to_string())
        .unwrap_or_else(|| "none".to_string()),
      timed_out
    ));

    clear_running_and_emit_idle(&app_for_watcher);
  });

  Ok(())
}
