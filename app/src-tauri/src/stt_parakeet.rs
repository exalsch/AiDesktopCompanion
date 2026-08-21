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
static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
  reqwest::Client::builder()
    .connect_timeout(std::time::Duration::from_secs(30))
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
});

#[cfg(feature = "local-stt")]
struct ParakeetTdtCache {
  has_cuda: bool,
  model_dir: String,
  asr: parakeet_rs_alt::ParakeetTDT,
}

#[cfg(feature = "local-stt")]
static PARKEET_TDT_CACHE: Lazy<Mutex<Option<ParakeetTdtCache>>> = Lazy::new(|| Mutex::new(None));

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
  #[cfg(target_os = "windows")]
  { if path.exists() { let _ = fs::remove_file(path); } }
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

/// The only Parakeet model still supported.
#[cfg(feature = "local-stt")]
const PARAKEET_MODEL_ID: &str = "parakeet-tdt-0.6b-v3";

/// Whether a stored setting names a model that no longer exists.
///
/// V2 was English-only and has been removed. Rather than failing, those profiles
/// fall through to V3 - the alternative strands anyone who never opens the
/// settings page.
#[cfg(feature = "local-stt")]
fn is_retired_model(local_model: &str) -> bool {
  let t = local_model.trim().to_lowercase();
  !t.is_empty() && !t.contains("v3")
}

#[cfg(feature = "local-stt")]
pub fn local_model_status(_local_model: String, _has_cuda: bool) -> Result<(bool, String, Vec<String>), String> {
  let dir = models_dir(PARAKEET_MODEL_ID).ok_or_else(|| "Unsupported platform for model path".to_string())?;
  let required = [
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
  ];
  let missing: Vec<String> = required
    .iter()
    .filter(|f| !dir.join(f).exists())
    .map(|f| f.to_string())
    .collect();
  Ok((missing.is_empty(), dir.to_string_lossy().to_string(), missing))
}

#[cfg(not(feature = "local-stt"))]
pub fn local_model_status(_local_model: String, _has_cuda: bool) -> Result<(bool, String, Vec<String>), String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

#[cfg(feature = "local-stt")]
pub async fn prefetch_model_with_progress(app: tauri::AppHandle, _local_model: String) -> Result<String, String> {
  let root = ensure_model_files_v3(Some(&app)).await?;
  Ok(root.to_string_lossy().to_string())
}

#[cfg(not(feature = "local-stt"))]
pub async fn prefetch_model_with_progress(_app: tauri::AppHandle, _local_model: String) -> Result<String, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

#[cfg(feature = "local-stt")]
pub async fn transcribe_local(audio: Vec<u8>, mime: String, has_cuda: bool, local_model: String) -> Result<String, String> {
  {
    use parakeet_rs_alt::Transcriber;
    if is_retired_model(&local_model) {
      println!("[stt] parakeet: '{local_model}' has been retired, using {PARAKEET_MODEL_ID}");
    }
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
    Ok(res.text.trim().to_string())
  }
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
