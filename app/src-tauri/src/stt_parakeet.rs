use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[cfg(feature = "local-stt")]
use once_cell::sync::Lazy;
#[cfg(feature = "local-stt")]
use std::sync::Mutex;
#[cfg(feature = "local-stt")]
use reqwest;
#[cfg(feature = "local-stt")]
use serde_json::json;
#[cfg(feature = "local-stt")]
use tauri::Emitter;

#[cfg(feature = "local-stt")]
static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::new());

#[cfg(feature = "local-stt")]
struct ParakeetAsrCache {
  has_cuda: bool,
  model_dir: String,
  asr: parakeet_rs_jason::asr::ParakeetASR,
}

#[cfg(feature = "local-stt")]
static PARKEET_ASR_CACHE: Lazy<Mutex<Option<ParakeetAsrCache>>> = Lazy::new(|| Mutex::new(None));

#[cfg(feature = "local-stt")]
struct ParakeetTdtCache {
  has_cuda: bool,
  model_dir: String,
  asr: parakeet_rs_alt::ParakeetTDT,
}

#[cfg(feature = "local-stt")]
static PARKEET_TDT_CACHE: Lazy<Mutex<Option<ParakeetTdtCache>>> = Lazy::new(|| Mutex::new(None));

#[cfg(feature = "local-stt")]
static MODEL_TARBALL_URL: &str = "https://github.com/jason-ni/parakeet-rs/releases/download/v0.1.0/parakeet-tdt-0.6b-v2-onnx.tar.gz";

#[cfg(feature = "local-stt")]
static MODEL_V3_FILES: [(&str, &str); 4] = [
  (
    "encoder-model.int8.onnx",
    "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/encoder-model.int8.onnx?download=true",
  ),
  (
    "decoder_joint-model.int8.onnx",
    "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/decoder_joint-model.int8.onnx?download=true",
  ),
  (
    "nemo128.onnx",
    "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/nemo128.onnx?download=true",
  ),
  (
    "vocab.txt",
    "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main/vocab.txt?download=true",
  ),
];

fn models_dir(model_id: &str) -> Option<PathBuf> {
  #[cfg(target_os = "windows")]
  {
    if let Ok(appdata) = std::env::var("APPDATA") {
      let mut p = PathBuf::from(appdata);
      p.push("AiDesktopCompanion");
      p.push("models");
      p.push("parakeet");
      p.push(model_id);
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
      p.push(model_id);
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
fn ensure_cuda_fallback_files(model_dir: &PathBuf) -> Result<(), String> {
  let pairs: [(&str, &str); 5] = [
    ("encoder.fp32.onnx", "encoder.int8.onnx"),
    ("decoder.onnx", "decoder.int8.onnx"),
    ("joint.pred.onnx", "joint.pred.int8.onnx"),
    ("joint.enc.onnx", "joint.enc.int8.onnx"),
    ("joint.joint_net.onnx", "joint.joint_net.int8.onnx"),
  ];

  for (dst, src) in pairs {
    let dst_path = model_dir.join(dst);
    if dst_path.exists() {
      continue;
    }
    let src_path = model_dir.join(src);
    if !src_path.exists() {
      continue;
    }

    if fs::hard_link(&src_path, &dst_path).is_ok() {
      continue;
    }
    fs::copy(&src_path, &dst_path)
      .map_err(|e| format!("Failed to create {dst} from {src}: {e}"))?;
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
  let dir = models_dir("parakeet-tdt-0.6b-v2").ok_or_else(|| "Unsupported platform for model path".to_string())?;
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
async fn ensure_model_files_v3(app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
  let dir = models_dir("parakeet-tdt-0.6b-v3").ok_or_else(|| "Unsupported platform for model path".to_string())?;
  if !dir.exists() {
    fs::create_dir_all(&dir).map_err(|e| format!("create model dir failed: {e}"))?;
  }

  let required = [
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
  ];
  let all_present = required.iter().all(|f| dir.join(f).exists());
  if all_present {
    return Ok(dir);
  }

  for (file, url) in MODEL_V3_FILES {
    let path = dir.join(file);
    if path.exists() {
      continue;
    }
    download_file_with_progress(app, url, &path, "stt-parakeet-model-download").await?;
  }

  let all_present = required.iter().all(|f| dir.join(f).exists());
  if all_present {
    return Ok(dir);
  }

  Err("Parakeet V3 model download finished but required files were not found.".into())
}

#[cfg(feature = "local-stt")]
fn is_parakeet_v3_local_model(local_model: &str) -> bool {
  let t = local_model.trim().to_lowercase();
  t.contains("v3") || t.contains("0.6b-v3")
}

#[cfg(feature = "local-stt")]
pub fn local_model_status(local_model: String, has_cuda: bool) -> Result<(bool, String, Vec<String>), String> {
  if is_parakeet_v3_local_model(&local_model) {
    let dir = models_dir("parakeet-tdt-0.6b-v3").ok_or_else(|| "Unsupported platform for model path".to_string())?;
    let required = [
      "encoder-model.int8.onnx",
      "decoder_joint-model.int8.onnx",
      "nemo128.onnx",
      "vocab.txt",
    ];
    let mut missing: Vec<String> = Vec::new();
    for f in required {
      if !dir.join(f).exists() {
        missing.push(f.to_string());
      }
    }
    return Ok((missing.is_empty(), dir.to_string_lossy().to_string(), missing));
  }

  let base_dir = models_dir("parakeet-tdt-0.6b-v2").ok_or_else(|| "Unsupported platform for model path".to_string())?;
  let root = find_model_root(&base_dir).unwrap_or(base_dir);

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

  let all_present = |required: &[&str]| required.iter().all(|f| root.join(f).exists());
  let cpu_ok = all_present(&cpu_required);
  let cuda_ok = all_present(&cuda_required) || cpu_ok;

  let downloaded = if has_cuda { cuda_ok } else { cpu_ok || all_present(&cuda_required) };
  if downloaded {
    return Ok((true, root.to_string_lossy().to_string(), Vec::new()));
  }

  let required_for_missing = if has_cuda { &cuda_required[..] } else { &cpu_required[..] };
  let mut missing: Vec<String> = Vec::new();
  for f in required_for_missing {
    if !root.join(f).exists() {
      missing.push((*f).to_string());
    }
  }

  Ok((false, root.to_string_lossy().to_string(), missing))
}

#[cfg(not(feature = "local-stt"))]
pub fn local_model_status(_local_model: String, _has_cuda: bool) -> Result<(bool, String, Vec<String>), String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
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
pub async fn prefetch_model_with_progress(app: tauri::AppHandle, local_model: String) -> Result<String, String> {
  let root = if is_parakeet_v3_local_model(&local_model) {
    ensure_model_files_v3(Some(&app)).await?
  } else {
    ensure_model_files(Some(&app)).await?
  };
  Ok(root.to_string_lossy().to_string())
}

#[cfg(not(feature = "local-stt"))]
pub async fn prefetch_model_with_progress(_app: tauri::AppHandle, _local_model: String) -> Result<String, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

#[cfg(feature = "local-stt")]
pub async fn transcribe_local(audio: Vec<u8>, mime: String, has_cuda: bool, local_model: String) -> Result<String, String> {
  if is_parakeet_v3_local_model(&local_model) {
    use parakeet_rs_alt::Transcriber;
    let model_dir = ensure_model_files_v3(None).await?;
    let pcm = crate::stt_whisper::decode_to_f32_mono_16k(&audio, &mime)?;

    let model_dir_key = model_dir.to_string_lossy().to_string();
    let mut cache = PARKEET_TDT_CACHE
      .lock()
      .map_err(|_| "parakeet v3 cache lock poisoned".to_string())?;

    let needs_init = match cache.as_ref() {
      Some(c) => c.has_cuda != has_cuda || c.model_dir != model_dir_key,
      None => true,
    };

    if needs_init {
      let exec = if has_cuda {
        parakeet_rs_alt::ExecutionConfig::new().with_execution_provider(parakeet_rs_alt::ExecutionProvider::Cuda)
      } else {
        parakeet_rs_alt::ExecutionConfig::new().with_execution_provider(parakeet_rs_alt::ExecutionProvider::Cpu)
      };

      let asr = parakeet_rs_alt::ParakeetTDT::from_pretrained(&model_dir, Some(exec))
        .map_err(|e| format!("parakeet v3 init failed: {e}"))?;

      *cache = Some(ParakeetTdtCache {
        has_cuda,
        model_dir: model_dir_key.clone(),
        asr,
      });
    }

    let asr = cache.as_mut().ok_or_else(|| "parakeet v3 cache init failed".to_string())?;
    let res = asr
      .asr
      .transcribe_samples(pcm, 16000, 1, None)
      .map_err(|e| format!("parakeet v3 transcribe failed: {e}"))?;
    return Ok(res.text.trim().to_string());
  }

  let model_dir = ensure_model_files(None).await?;
  if has_cuda {
    ensure_cuda_fallback_files(&model_dir)?;
  }
  validate_model_files_for_mode(&model_dir, has_cuda)?;

  let pcm = crate::stt_whisper::decode_to_f32_mono_16k(&audio, &mime)?;

  let model_dir_key = model_dir.to_string_lossy().to_string();
  let mut cache = PARKEET_ASR_CACHE
    .lock()
    .map_err(|_| "parakeet cache lock poisoned".to_string())?;

  let needs_init = match cache.as_ref() {
    Some(c) => c.has_cuda != has_cuda || c.model_dir != model_dir_key,
    None => true,
  };

  if needs_init {
    let asr = parakeet_rs_jason::asr::ParakeetASR::new(
      &model_dir.to_string_lossy(),
      true,
      has_cuda,
    ).map_err(|e| {
      let msg = format!("{e}");
      if has_cuda {
        format!("parakeet init failed (CUDA enabled): {msg}")
      } else {
        format!("parakeet init failed: {msg}")
      }
    })?;

    *cache = Some(ParakeetAsrCache {
      has_cuda,
      model_dir: model_dir_key.clone(),
      asr,
    });
  }

  let asr = cache.as_mut().ok_or_else(|| "parakeet cache init failed".to_string())?;
  let res = asr.asr.infer_buffer(&pcm).map_err(|e| format!("parakeet transcribe failed: {e}"))?;
  Ok(res.to_text().trim().to_string())
}

#[cfg(not(feature = "local-stt"))]
pub async fn transcribe_local(_audio: Vec<u8>, _mime: String, _has_cuda: bool, _local_model: String) -> Result<String, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

#[cfg(feature = "local-stt")]
pub fn check_cuda_available() -> Result<(), String> {
  use ort::execution_providers::cuda::CUDAExecutionProvider;
  use ort::execution_providers::ExecutionProvider;
  use ort::session::Session;

  let mut builder = Session::builder().map_err(|e| format!("ONNX Runtime init failed: {e}"))?;
  CUDAExecutionProvider::default().register(&mut builder).map_err(|e| {
    let msg = format!("{e}");
    if msg.to_lowercase().contains("cudnn") {
      format!(
        "CUDA is not available: {msg}. ONNX Runtime's CUDA provider loaded, but a required NVIDIA dependency is missing (e.g. cuDNN: cudnn64_9.dll). Install the matching cuDNN for your CUDA version and ensure its 'bin' folder is on PATH (or place the DLLs next to the executable), then retry."
      )
    } else {
      format!(
        "CUDA is not available: {msg}. Install NVIDIA driver + CUDA runtime (cudart/cublas) and cuDNN, ensure DLLs are on PATH, or disable CUDA."
      )
    }
  })?;
  Ok(())
}

#[cfg(not(feature = "local-stt"))]
pub fn check_cuda_available() -> Result<(), String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}
