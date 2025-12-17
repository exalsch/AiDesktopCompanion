use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[cfg(feature = "local-stt")]
use once_cell::sync::Lazy;
#[cfg(feature = "local-stt")]
use reqwest;
#[cfg(feature = "local-stt")]
use serde_json::json;
#[cfg(feature = "local-stt")]
use tauri::Emitter;

#[cfg(feature = "local-stt")]
static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::new());

#[cfg(feature = "local-stt")]
static MODEL_TARBALL_URL: &str = "https://github.com/jason-ni/parakeet-rs/releases/download/v0.1.0/parakeet-tdt-0.6b-v2-onnx.tar.gz";

fn models_dir() -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("models");
      p.push("parakeet");
      p.push("parakeet-tdt-0.6b-v2");
      return Some(p);
    }
    None
  }
  #[cfg(not(target_os = "windows"))]
  {
    if let Ok(home) = std::env::var("HOME") {
      let mut p = PathBuf::from(home);
      p.push(".cache");
      p.push("AiDesktopCompanion");
      p.push("models");
      p.push("parakeet");
      p.push("parakeet-tdt-0.6b-v2");
      return Some(p);
    }
    None
  }
}

fn file_name_from_url(url: &str) -> String {
  url.split('/').last().filter(|s| !s.is_empty()).unwrap_or("model.bin").to_string()
}

#[cfg(feature = "local-stt")]
async fn download_file_with_progress(app: Option<&tauri::AppHandle>, url: &str, path: &PathBuf, event_name: &str) -> Result<(), String> {
  let mut tmp = path.clone();
  let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("download");
  tmp.set_file_name(format!("{}.part", file_name));

  let resp = CLIENT.get(url).send().await.map_err(|e| format!("download failed: {e}"))?;
  if !resp.status().is_success() {
    return Err(format!("download error: {}", resp.status()));
  }

  let total = resp.content_length().unwrap_or(0);

  let mut stream = resp.bytes_stream();
  let mut f = fs::File::create(&tmp).map_err(|e| format!("write tmp failed: {e}"))?;
  use futures_util::StreamExt;
  let mut received: u64 = 0;
  while let Some(chunk) = stream.next().await {
    let bytes = chunk.map_err(|e| format!("download chunk failed: {e}"))?;
    f.write_all(&bytes).map_err(|e| format!("write failed: {e}"))?;
    received += bytes.len() as u64;
    if let Some(app) = app {
      let _ = app.emit(
        event_name,
        json!({"kind":"progress","file":file_name,"received":received,"total":total}),
      );
    }
  }
  drop(f);
  fs::rename(&tmp, path).map_err(|e| format!("rename model failed: {e}"))?;

  if let Some(app) = app {
    let _ = app.emit(
      event_name,
      json!({"kind":"done","file":file_name,"path":path.to_string_lossy().to_string()}),
    );
  }
  Ok(())
}

#[cfg(feature = "local-stt")]
fn find_model_root(dir: &PathBuf) -> Option<PathBuf> {
  let cpu_required = [
    "encoder.int8.onnx",
    "decoder.int8.onnx",
    "joint.pred.int8.onnx",
    "joint.enc.int8.onnx",
    "joint.joint_net.int8.onnx",
    "nemo128.onnx",
    "tokens.txt",
  ];
  let cuda_required = [
    "encoder.fp32.onnx",
    "decoder.onnx",
    "joint.pred.onnx",
    "joint.enc.onnx",
    "joint.joint_net.onnx",
    "nemo128.onnx",
    "tokens.txt",
  ];

  let all_present = |p: &PathBuf, required: &[&str]| required.iter().all(|f| p.join(f).exists());
  if all_present(dir, &cpu_required) || all_present(dir, &cuda_required) {
    return Some(dir.clone());
  }

  if let Ok(entries) = fs::read_dir(dir) {
    for entry in entries.flatten() {
      let p = entry.path();
      if p.is_dir() && (all_present(&p, &cpu_required) || all_present(&p, &cuda_required)) {
        return Some(p);
      }
    }
  }
  None
}

#[cfg(feature = "local-stt")]
fn extract_tar_gz(archive_path: &PathBuf, dest_dir: &PathBuf) -> Result<(), String> {
  let f = fs::File::open(archive_path).map_err(|e| format!("open archive failed: {e}"))?;
  let gz = flate2::read::GzDecoder::new(f);
  let mut ar = tar::Archive::new(gz);
  ar.unpack(dest_dir).map_err(|e| format!("extract failed: {e}"))?;
  Ok(())
}

#[cfg(feature = "local-stt")]
async fn ensure_model_files(app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
  let dir = models_dir().ok_or_else(|| "Unsupported platform for model path".to_string())?;
  if !dir.exists() {
    fs::create_dir_all(&dir).map_err(|e| format!("create model dir failed: {e}"))?;
  }

  if let Some(root) = find_model_root(&dir) {
    return Ok(root);
  }

  let tar_name = file_name_from_url(MODEL_TARBALL_URL);
  let mut tar_path = dir.clone();
  tar_path.push(&tar_name);

  download_file_with_progress(app, MODEL_TARBALL_URL, &tar_path, "stt-parakeet-model-download").await?;
  extract_tar_gz(&tar_path, &dir)?;

  if let Some(root) = find_model_root(&dir) {
    return Ok(root);
  }

  Err("Parakeet model extraction finished but required files were not found.".into())
}

#[cfg(feature = "local-stt")]
fn validate_model_files_for_mode(model_dir: &PathBuf, has_cuda: bool) -> Result<(), String> {
  let required: [&str; 7] = if has_cuda {
    [
      "encoder.fp32.onnx",
      "decoder.onnx",
      "joint.pred.onnx",
      "joint.enc.onnx",
      "joint.joint_net.onnx",
      "nemo128.onnx",
      "tokens.txt",
    ]
  } else {
    [
      "encoder.int8.onnx",
      "decoder.int8.onnx",
      "joint.pred.int8.onnx",
      "joint.enc.int8.onnx",
      "joint.joint_net.int8.onnx",
      "nemo128.onnx",
      "tokens.txt",
    ]
  };

  let mut missing: Vec<String> = Vec::new();
  for f in required {
    let p = model_dir.join(f);
    if !p.exists() {
      missing.push(f.to_string());
    }
  }
  if !missing.is_empty() {
    return Err(format!(
      "Parakeet model files missing for {} mode: {}",
      if has_cuda { "CUDA" } else { "CPU" },
      missing.join(", ")
    ));
  }
  Ok(())
}

#[cfg(feature = "local-stt")]
pub async fn prefetch_model_with_progress(app: tauri::AppHandle) -> Result<String, String> {
  let root = ensure_model_files(Some(&app)).await?;
  Ok(root.to_string_lossy().to_string())
}

#[cfg(not(feature = "local-stt"))]
pub async fn prefetch_model_with_progress(_app: tauri::AppHandle) -> Result<String, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

#[cfg(feature = "local-stt")]
pub async fn transcribe_local(audio: Vec<u8>, mime: String, has_cuda: bool) -> Result<String, String> {
  let model_dir = ensure_model_files(None).await?;
  validate_model_files_for_mode(&model_dir, has_cuda)?;

  let pcm = crate::stt_whisper::decode_to_f32_mono_16k(&audio, &mime)?;

  let mut asr = parakeet_rs_jason::asr::ParakeetASR::new(
    &model_dir.to_string_lossy(),
    true,
    has_cuda,
  ).map_err(|e| format!("parakeet init failed: {e}"))?;

  let res = asr.infer_buffer(&pcm).map_err(|e| format!("parakeet transcribe failed: {e}"))?;
  Ok(res.to_text().trim().to_string())
}

#[cfg(not(feature = "local-stt"))]
pub async fn transcribe_local(_audio: Vec<u8>, _mime: String, _has_cuda: bool) -> Result<String, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}
