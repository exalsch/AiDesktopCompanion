use crate::utils::ps_escape_single_quoted;
#[cfg(target_os = "windows")]
use std::io::Write;
#[cfg(target_os = "windows")]
use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use once_cell::sync::Lazy;
#[cfg(target_os = "windows")]
use std::sync::Mutex;

#[cfg(target_os = "windows")]
static TTS_CHILD: Lazy<Mutex<Option<std::process::Child>>> = Lazy::new(|| Mutex::new(None));

#[cfg(target_os = "windows")]
pub fn local_tts_start(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<(), String> {
  if let Ok(mut guard) = TTS_CHILD.lock() {
    if let Some(mut c) = guard.take() { let _ = c.kill(); let _ = c.wait(); }
  }
  let v = voice.unwrap_or_default();
  let v_escaped = ps_escape_single_quoted(&v);
  let r = rate.unwrap_or(-2).clamp(-10, 10);
  let vol = volume.unwrap_or(100).min(100);
  let ps = format!(
    r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
try {{
  $s.Volume = {vol};
  $s.Rate = {r};
  if ('{voice}' -ne '') {{ try {{ $s.SelectVoice('{voice}'); }} catch {{}} }}
  [void]$s.Speak([Console]::In.ReadToEnd());
}} finally {{ $s.Dispose(); }}
"#,
    vol = vol, r = r, voice = v_escaped,
  );
  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if let Some(stdin) = child.stdin.as_mut() { stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?; }
  drop(child.stdin.take());
  if let Ok(mut guard) = TTS_CHILD.lock() { *guard = Some(child); }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_start(_text: String, _voice: Option<String>, _rate: Option<i32>, _volume: Option<u8>) -> Result<(), String> {
  Err("TTS not implemented on this platform".into())
}

#[cfg(target_os = "windows")]
pub fn local_tts_stop() -> Result<(), String> {
  if let Ok(mut guard) = TTS_CHILD.lock() {
    if let Some(mut c) = guard.take() {
      let _ = c.kill(); let _ = c.wait();
    }
  }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_stop() -> Result<(), String> { Err("TTS not implemented on this platform".into()) }

#[cfg(target_os = "windows")]
pub fn local_tts_list_voices() -> Result<Vec<String>, String> {
  let ps = r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
$names = $s.GetInstalledVoices() | ForEach-Object { $_.VoiceInfo.Name };
$s.Dispose();
$names | ForEach-Object { $_ }
"#;
  let out = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", ps])
    .output()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if !out.status.success() { return Err(format!("powershell exited with status: {}", out.status)); }
  let s = String::from_utf8_lossy(&out.stdout);
  let mut names: Vec<String> = s.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).map(|l| l.to_string()).collect();
  names.dedup();
  Ok(names)
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_list_voices() -> Result<Vec<String>, String> { Ok(vec![]) }

#[allow(dead_code)]
#[cfg(target_os = "windows")]
pub fn local_speak_blocking(text: String, voice: String, rate: i32, vol: u8) -> Result<(), String> {
  let v_escaped = ps_escape_single_quoted(&voice);
  let ps = format!(
    r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
try {{
  $s.Volume = {vol};
  $s.Rate = {rate};
  if ('{voice}' -ne '') {{ try {{ $s.SelectVoice('{voice}'); }} catch {{}} }}
  [void]$s.Speak([Console]::In.ReadToEnd());
}} finally {{ $s.Dispose(); }}
"#,
    vol = vol,
    rate = rate.clamp(-10, 10),
    voice = v_escaped,
  );
  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if let Some(stdin) = child.stdin.as_mut() { stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?; }
  drop(child.stdin.take());
  let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
  if !status.success() { return Err(format!("powershell exited with status: {status}")); }
  Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn local_speak_blocking(_text: String, _voice: String, _rate: i32, _vol: u8) -> Result<(), String> {
  Err("TTS not implemented on this platform".into())
}

#[cfg(target_os = "windows")]
pub fn local_tts_synthesize_wav(text: String, voice: Option<String>, rate: Option<i32>, volume: Option<u8>) -> Result<String, String> {
  let v = voice.unwrap_or_default();
  let v_escaped = ps_escape_single_quoted(&v);
  let r = rate.unwrap_or(-2).clamp(-10, 10);
  let vol = volume.unwrap_or(100).min(100);
  let file_name = format!("aidc_tts_{}.wav", chrono::Local::now().format("%Y%m%d_%H%M%S"));
  let mut path = std::env::temp_dir();
  path.push(file_name);
  let target = path.to_string_lossy().to_string();
  let ps = format!(
    r#"
Add-Type -AssemblyName System.Speech;
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer;
try {{
  $s.Volume = {vol};
  $s.Rate = {r};
  if ('{voice}' -ne '') {{ try {{ $s.SelectVoice('{voice}'); }} catch {{}} }}
  $s.SetOutputToWaveFile('{target}');
  [void]$s.Speak([Console]::In.ReadToEnd());
  $s.SetOutputToDefaultAudioDevice();
}} finally {{ $s.Dispose(); }}
"#,
    vol = vol, r = r, voice = v_escaped, target = target.replace('\\', "\\\\"),
  );
  let mut child = Command::new("powershell.exe")
    .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| format!("launch powershell failed: {e}"))?;
  if let Some(stdin) = child.stdin.as_mut() { stdin.write_all(text.as_bytes()).map_err(|e| format!("stdin write failed: {e}"))?; }
  drop(child.stdin.take());
  let status = child.wait().map_err(|e| format!("powershell wait failed: {e}"))?;
  if !status.success() { return Err(format!("powershell exited with status: {status}")); }
  Ok(target)
}

#[cfg(not(target_os = "windows"))]
pub fn local_tts_synthesize_wav(_text: String, _voice: Option<String>, _rate: Option<i32>, _volume: Option<u8>) -> Result<String, String> {
  Err("TTS not implemented on this platform".into())
}
