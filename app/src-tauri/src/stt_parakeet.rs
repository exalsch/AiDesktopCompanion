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

/// One file of a model bundle. `sha256` is checked after the download and
/// before the file is moved into place, so a truncated or swapped file never
/// reaches ONNX Runtime. Every URL names a fixed revision, not `main`, or an
/// upstream re-upload would break the hash check for everyone.
#[cfg(feature = "local-stt")]
struct ModelFile {
  name: &'static str,
  url: &'static str,
  sha256: Option<&'static str>,
}

/// A Parakeet TDT model that the pinned parakeet-rs 0.2.6 `ParakeetTDT` loads.
/// Only fine-tunes of parakeet-tdt-0.6b-v3 belong here: same architecture,
/// same vocabulary, same file names, ONNX opset 22 or lower. New architectures (Unified, Nemotron)
/// need parakeet-rs 0.4 and with it the CUDA 13 ONNX Runtime; see Cargo.toml.
#[cfg(feature = "local-stt")]
struct ParakeetModel {
  /// Value of the `stt_local_model` setting and name of the model folder.
  id: &'static str,
  files: &'static [ModelFile],
  /// Files the loader needs. Licence and notice files are fetched but not
  /// required, so a model counts as downloaded without them.
  required: &'static [&'static str],
}

/// The vocabulary is byte-identical across V3 and both fine-tunes.
#[cfg(feature = "local-stt")]
const VOCAB_V3_SHA256: &str = "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d";

#[cfg(feature = "local-stt")]
const DEFAULT_MODEL_ID: &str = "parakeet-tdt-0.6b-v3";

#[cfg(feature = "local-stt")]
static MODELS: &[ParakeetModel] = &[
  // NVIDIA Parakeet TDT 0.6B v3, int8 ONNX export by istupakov. About 670 MB.
  ParakeetModel {
    id: "parakeet-tdt-0.6b-v3",
    files: &[
      ModelFile {
        name: "encoder-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx?download=true",
        sha256: Some("6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09"),
      },
      ModelFile {
        name: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx?download=true",
        sha256: Some("eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70"),
      },
      ModelFile {
        name: "nemo128.onnx",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx?download=true",
        sha256: Some("a9fde1486ebfcc08f328d75ad4610c67835fea58c73ba57e3209a6f6cf019e9f"),
      },
      ModelFile {
        name: "vocab.txt",
        url: "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt?download=true",
        sha256: Some(VOCAB_V3_SHA256),
      },
    ],
    // nemo128.onnx stays required: existing installs have it and dropping it
    // would change nothing for them, while the loader computes the mel
    // spectrogram itself and never opens it.
    required: &["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "nemo128.onnx", "vocab.txt"],
  },
  // Moondream Parakeet Ultra: V3 post-trained further, CC-BY-4.0. Int8 ONNX
  // export by Olicorne (same layout as the V3 export above), about 670 MB.
  // Not the parakeet-rs author's export: that one is ONNX opset 23, and the
  // ONNX Runtime 1.22 pinned for CUDA 12 stops at opset 22.
  ParakeetModel {
    id: "parakeet-ultra",
    files: &[
      ModelFile {
        name: "encoder-model.int8.onnx",
        url: "https://huggingface.co/Olicorne/parakeet-tdt-0.6b-v3-ultra-onnx/resolve/b27f93c2238aa35e138996c005a62f1e8430f801/int8/encoder-model.int8.onnx?download=true",
        sha256: Some("714047d70be4b612dce02e0ab2f9e0d6552f78adc421f408d154070796bc2dcf"),
      },
      ModelFile {
        name: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/Olicorne/parakeet-tdt-0.6b-v3-ultra-onnx/resolve/b27f93c2238aa35e138996c005a62f1e8430f801/int8/decoder_joint-model.int8.onnx?download=true",
        sha256: Some("667ffde2242a560923cba6bc7f69ef03d736f8a36ca9588f2e6f9fa818cf2fc1"),
      },
      ModelFile {
        name: "vocab.txt",
        url: "https://huggingface.co/Olicorne/parakeet-tdt-0.6b-v3-ultra-onnx/resolve/b27f93c2238aa35e138996c005a62f1e8430f801/vocab.txt?download=true",
        sha256: Some(VOCAB_V3_SHA256),
      },
    ],
    required: &["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "vocab.txt"],
  },
  // Oruk AI Orukeet v0.1.0: V3 fine-tune with Gabor kernels in the encoder,
  // CC-BY-SA-4.0. Official int8 export, about 670 MB, same files as V3.
  ParakeetModel {
    id: "parakeet-orukeet-v0.1.0",
    files: &[
      ModelFile {
        name: "encoder-model.int8.onnx",
        url: "https://huggingface.co/oruk/orukeet/resolve/eac739d754bb171287930e6e63386f5b88f8179e/onnx/combined-v0.1.0-int8/encoder-model.int8.onnx?download=true",
        sha256: Some("7b55f2a504a20a8e462899f5befd45f4a1784948d76ed0127902d9cf39405487"),
      },
      ModelFile {
        name: "decoder_joint-model.int8.onnx",
        url: "https://huggingface.co/oruk/orukeet/resolve/eac739d754bb171287930e6e63386f5b88f8179e/onnx/combined-v0.1.0-int8/decoder_joint-model.int8.onnx?download=true",
        sha256: Some("95d3b1f53f9aadc5ef58e63664a3681a2184ee228b5010e1ef975a1c4ea8318a"),
      },
      ModelFile {
        name: "vocab.txt",
        url: "https://huggingface.co/oruk/orukeet/resolve/eac739d754bb171287930e6e63386f5b88f8179e/onnx/combined-v0.1.0-int8/vocab.txt?download=true",
        sha256: Some(VOCAB_V3_SHA256),
      },
      // CC-BY-SA asks for the licence and attribution to travel with the
      // weights.
      ModelFile {
        name: "LICENSE-WEIGHTS",
        url: "https://huggingface.co/oruk/orukeet/resolve/eac739d754bb171287930e6e63386f5b88f8179e/onnx/combined-v0.1.0-int8/LICENSE-WEIGHTS?download=true",
        sha256: Some("23ee78c8bae49cf08ea2f0c84945c66b987ebe4520881fb51b3dad4fb43d07c2"),
      },
      ModelFile {
        name: "NOTICE.md",
        url: "https://huggingface.co/oruk/orukeet/resolve/eac739d754bb171287930e6e63386f5b88f8179e/onnx/combined-v0.1.0-int8/NOTICE.md?download=true",
        sha256: Some("0a04b1a496cc7af09fbacd03425d49cae64d741bbe6e45d6adca3e550960e1e3"),
      },
    ],
    required: &["encoder-model.int8.onnx", "decoder_joint-model.int8.onnx", "vocab.txt"],
  },
];

/// The model a stored setting names. Anything unknown - an empty value, the
/// retired English-only V2, a Whisper setting when file transcription asks
/// for Parakeet - falls through to V3 rather than failing, so a profile that
/// never opens the settings page keeps working.
#[cfg(feature = "local-stt")]
fn resolve_model(local_model: &str) -> &'static ParakeetModel {
  let t = local_model.trim().to_lowercase();
  MODELS
    .iter()
    .find(|m| m.id == t)
    .unwrap_or_else(|| MODELS.iter().find(|m| m.id == DEFAULT_MODEL_ID).expect("default Parakeet model is listed"))
}

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
async fn download_file_with_progress(
  app: Option<&tauri::AppHandle>,
  url: &str,
  path: &PathBuf,
  event_name: &str,
  sha256: Option<&str>,
) -> Result<(), String> {
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
  use sha2::Digest;
  let mut hasher = sha2::Sha256::new();
  let mut received: u64 = 0;
  while let Some(chunk) = stream.next().await {
    let bytes = chunk.map_err(|e| format!("download chunk failed: {e}"))?;
    f.write_all(&bytes).map_err(|e| format!("write failed: {e}"))?;
    hasher.update(&bytes);
    received += bytes.len() as u64;
    if let Some(app) = app {
      let _ = app.emit(
        event_name,
        json!({"kind":"progress","file":file_name,"received":received,"total":total}),
      );
    }
  }
  drop(f);
  if let Some(expected) = sha256 {
    let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !actual.eq_ignore_ascii_case(expected) {
      let _ = fs::remove_file(&tmp);
      return Err(format!(
        "download of {file_name} is corrupt (SHA-256 {actual}, expected {expected}). Try the download again."
      ));
    }
  }
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
async fn ensure_model_files(app: Option<&tauri::AppHandle>, model: &ParakeetModel) -> Result<PathBuf, String> {
  let dir = models_dir(model.id).ok_or_else(|| "Unsupported platform for model path".to_string())?;
  if !dir.exists() {
    fs::create_dir_all(&dir).map_err(|e| format!("create model dir failed: {e}"))?;
  }

  for file in model.files {
    let path = dir.join(file.name);
    if path.exists() {
      continue;
    }
    // A missing licence file must not block a transcription that would
    // otherwise work offline, so only required files fail the call.
    let res = download_file_with_progress(app, file.url, &path, "stt-parakeet-model-download", file.sha256).await;
    if let Err(e) = res {
      if model.required.contains(&file.name) {
        return Err(e);
      }
      println!("[stt] parakeet: optional file {} not downloaded: {e}", file.name);
    }
  }

  if model.required.iter().all(|f| dir.join(f).exists()) {
    return Ok(dir);
  }
  Err(format!("Parakeet model '{}' download finished but required files were not found.", model.id))
}

#[cfg(feature = "local-stt")]
pub fn local_model_status(local_model: String, _has_cuda: bool) -> Result<(bool, String, Vec<String>), String> {
  let model = resolve_model(&local_model);
  let dir = models_dir(model.id).ok_or_else(|| "Unsupported platform for model path".to_string())?;
  let missing: Vec<String> = model
    .required
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
pub async fn prefetch_model_with_progress(app: tauri::AppHandle, local_model: String) -> Result<String, String> {
  let root = ensure_model_files(Some(&app), resolve_model(&local_model)).await?;
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
    let model = resolve_model(&local_model);
    if model.id != local_model.trim().to_lowercase() {
      println!("[stt] parakeet: unknown or retired model '{local_model}', using {}", model.id);
    }
    let model_dir = ensure_model_files(None, model).await?;
    let pcm = crate::stt_whisper::decode_to_f32_mono_16k(&audio, &mime)?;

    let res = with_tdt(&model_dir, has_cuda, |asr| {
      asr
        .transcribe_samples(pcm, 16000, 1, None)
        .map_err(|e| format!("parakeet transcribe failed: {e}"))
    })?;
    Ok(res.text.trim().to_string())
  }
}

/// Runs `f` on the cached TDT model, loading it first when nothing is cached
/// or the cached one was built for another directory or execution provider.
/// Holds the cache lock for the whole call, so a dictation waits for a file
/// chunk in progress rather than loading a second copy of the model.
#[cfg(feature = "local-stt")]
fn with_tdt<R>(
  model_dir: &std::path::Path,
  has_cuda: bool,
  f: impl FnOnce(&mut parakeet_rs_alt::ParakeetTDT) -> Result<R, String>,
) -> Result<R, String> {
  let model_dir_key = model_dir.to_string_lossy().to_string();
  let mut cache = PARKEET_TDT_CACHE
    .lock()
    .map_err(|_| "parakeet cache lock poisoned".to_string())?;

  let needs_init = match cache.as_ref() {
    Some(c) => c.has_cuda != has_cuda || c.model_dir != model_dir_key,
    None => true,
  };

  if needs_init {
    let asr = parakeet_rs_alt::ParakeetTDT::from_pretrained(model_dir, Some(execution_config(has_cuda)))
      .map_err(|e| format!("parakeet init failed: {e}"))?;

    *cache = Some(ParakeetTdtCache {
      has_cuda,
      model_dir: model_dir_key,
      asr,
    });
  }

  let c = cache.as_mut().ok_or_else(|| "parakeet cache init failed".to_string())?;
  f(&mut c.asr)
}

#[cfg(feature = "local-stt")]
fn execution_config(has_cuda: bool) -> parakeet_rs_alt::ExecutionConfig {
  if has_cuda {
    parakeet_rs_alt::ExecutionConfig::new().with_execution_provider(parakeet_rs_alt::ExecutionProvider::Cuda)
  } else {
    parakeet_rs_alt::ExecutionConfig::new().with_execution_provider(parakeet_rs_alt::ExecutionProvider::Cpu)
  }
}

/// Makes sure the Parakeet model selected in Settings is on disk (downloading
/// it when missing) and returns its directory. File transcription calls this
/// before going onto a blocking thread, where the download could not run.
/// With Whisper selected in Settings, this is V3.
#[cfg(feature = "local-stt")]
pub async fn ensure_model_dir() -> Result<PathBuf, String> {
  let lm = crate::config::get_stt_local_model_from_settings_or_env();
  ensure_model_files(None, resolve_model(&lm)).await
}

/// Transcribes 16 kHz mono samples into sentences with start and end times in
/// seconds, relative to the start of `pcm`. Blocking: call it from a blocking
/// thread. Feed it a few minutes at most; TDT has a sequence length limit
/// around 8-10 minutes.
#[cfg(feature = "local-stt")]
pub fn transcribe_sentences_blocking(
  model_dir: &std::path::Path,
  has_cuda: bool,
  pcm: Vec<f32>,
) -> Result<Vec<crate::stt_file::TimedText>, String> {
  use parakeet_rs_alt::Transcriber;
  // Raw tokens, grouped here rather than by the crate's `Sentences` mode:
  // that one drops the separate space token in front of a number ("um12
  // Uhr") and swallows a repeated word ("die die").
  let res = with_tdt(model_dir, has_cuda, |asr| {
    asr
      .transcribe_samples(pcm, 16000, 1, Some(parakeet_rs_alt::TimestampMode::Tokens))
      .map_err(|e| format!("parakeet transcribe failed: {e}"))
  })?;
  let tokens: Vec<crate::stt_file::TimedText> = res
    .tokens
    .into_iter()
    .map(|t| crate::stt_file::TimedText { start: t.start, end: t.end, text: t.text })
    .collect();
  Ok(crate::stt_file::sentences_from_tokens(&tokens))
}

// ---- Speaker diarization (NVIDIA Sortformer v2.1, up to 4 speakers) ----

#[cfg(feature = "local-stt")]
const DIARIZER_FILE: &str = "diar_streaming_sortformer_4spk-v2.1.onnx";
#[cfg(feature = "local-stt")]
const DIARIZER_URL: &str =
  "https://huggingface.co/altunenes/parakeet-rs/resolve/main/diar_streaming_sortformer_4spk-v2.1.onnx?download=true";

/// The real file is about 490 MB. Anything far smaller is a failed or
/// truncated download and must not be handed to ONNX Runtime.
#[cfg(feature = "local-stt")]
const DIARIZER_MIN_BYTES: u64 = 100 * 1024 * 1024;

#[cfg(feature = "local-stt")]
fn diarizer_path() -> Result<PathBuf, String> {
  let dir = models_dir("sortformer").ok_or_else(|| "Unsupported platform for model path".to_string())?;
  Ok(dir.join(DIARIZER_FILE))
}

#[cfg(feature = "local-stt")]
fn diarizer_present(path: &std::path::Path) -> bool {
  fs::metadata(path).map(|m| m.len() >= DIARIZER_MIN_BYTES).unwrap_or(false)
}

/// Whether the speaker model is on disk, and where it lives (or would live).
#[cfg(feature = "local-stt")]
pub fn diarizer_status() -> Result<(bool, String), String> {
  let path = diarizer_path()?;
  Ok((diarizer_present(&path), path.to_string_lossy().to_string()))
}

#[cfg(not(feature = "local-stt"))]
pub fn diarizer_status() -> Result<(bool, String), String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

/// Downloads the speaker model when it is missing. Progress goes out as
/// `stt-diarizer-download` events in the same shape as the TDT download.
#[cfg(feature = "local-stt")]
pub async fn ensure_diarizer(app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
  let path = diarizer_path()?;
  if diarizer_present(&path) {
    return Ok(path);
  }
  if let Some(dir) = path.parent() {
    fs::create_dir_all(dir).map_err(|e| format!("create model dir failed: {e}"))?;
  }
  download_file_with_progress(app, DIARIZER_URL, &path, "stt-diarizer-download", None).await?;
  if !diarizer_present(&path) {
    return Err("Speaker model download finished but the file is incomplete.".into());
  }
  Ok(path)
}

#[cfg(not(feature = "local-stt"))]
pub async fn ensure_diarizer(_app: Option<&tauri::AppHandle>) -> Result<PathBuf, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

/// Labels who speaks when across the whole recording, as `(start, end,
/// speaker)` in seconds with zero-based speaker numbers. One pass over the
/// full file: Sortformer streams internally with a speaker cache, so the
/// numbers stay stable from start to end. Blocking and not interruptible.
#[cfg(feature = "local-stt")]
pub fn diarize_blocking(model_path: &std::path::Path, has_cuda: bool, pcm: Vec<f32>) -> Result<Vec<(f32, f32, usize)>, String> {
  use parakeet_rs_alt::sortformer::{DiarizationConfig, Sortformer};
  let mut sf = Sortformer::with_config(model_path, Some(execution_config(has_cuda)), DiarizationConfig::callhome())
    .map_err(|e| format!("speaker model init failed: {e}"))?;
  let segs = sf.diarize(pcm, 16000, 1).map_err(|e| format!("speaker diarization failed: {e}"))?;
  Ok(segs.into_iter().map(|s| (s.start, s.end, s.speaker_id)).collect())
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

#[cfg(all(test, feature = "local-stt"))]
mod tests {
  use super::*;

  #[test]
  fn resolve_model_maps_unknown_ids_to_v3() {
    assert_eq!(resolve_model("parakeet-ultra").id, "parakeet-ultra");
    assert_eq!(resolve_model(" Parakeet-Orukeet-v0.1.0 ").id, "parakeet-orukeet-v0.1.0");
    assert_eq!(resolve_model("parakeet-tdt-0.6b-v3").id, "parakeet-tdt-0.6b-v3");
    for retired in ["", "parakeet-tdt-0.6b-v2", "whisper", "parakeet-unified-en-0.6b"] {
      assert_eq!(resolve_model(retired).id, DEFAULT_MODEL_ID, "{retired}");
    }
  }

  #[test]
  fn model_table_is_consistent() {
    for m in MODELS {
      for r in m.required {
        assert!(m.files.iter().any(|f| f.name == *r), "{}: required {r} has no download", m.id);
      }
      for f in m.files {
        assert!(!f.url.contains("/resolve/main/"), "{}: {} is not pinned to a revision", m.id, f.name);
        assert!(f.sha256.map(|h| h.len() == 64).unwrap_or(true), "{}: bad hash for {}", m.id, f.name);
      }
    }
  }

  /// Downloads each model and transcribes a real recording. Opt-in, because
  /// the downloads are large (Ultra is about 2.5 GB):
  ///   $env:PARAKEET_SAMPLE="C:\path\to\speech.wav"
  ///   $env:PARAKEET_MODELS="parakeet-ultra,parakeet-orukeet-v0.1.0"   # default: all
  ///   cargo test --features local-stt --lib stt_parakeet::tests::models_transcribe -- --ignored --nocapture
  #[tokio::test]
  #[ignore]
  async fn models_transcribe_sample() {
    use parakeet_rs_alt::Transcriber;
    let Ok(sample) = std::env::var("PARAKEET_SAMPLE") else { return };
    let audio = fs::read(&sample).unwrap();
    let pcm = crate::stt_whisper::decode_to_f32_mono_16k(&audio, "audio/wav").unwrap();
    let wanted = std::env::var("PARAKEET_MODELS").unwrap_or_default();
    let has_cuda = std::env::var("PARAKEET_CUDA").is_ok();
    for m in MODELS {
      if !wanted.is_empty() && !wanted.split(',').any(|w| w.trim() == m.id) {
        continue;
      }
      let dir = ensure_model_files(None, m).await.unwrap();
      let t0 = std::time::Instant::now();
      let mut asr = parakeet_rs_alt::ParakeetTDT::from_pretrained(&dir, Some(execution_config(has_cuda))).unwrap();
      let load = t0.elapsed().as_secs_f32();
      // The first run pays for ONNX Runtime's lazy setup; the second is the
      // speed a dictation sees once the model is cached.
      let t1 = std::time::Instant::now();
      let _ = asr.transcribe_samples(pcm.clone(), 16000, 1, None).unwrap();
      let first = t1.elapsed().as_secs_f32();
      let t2 = std::time::Instant::now();
      let res = asr.transcribe_samples(pcm.clone(), 16000, 1, None).unwrap();
      let warm = t2.elapsed().as_secs_f32();
      println!("--- {} (load {load:.1}s, first {first:.1}s, warm {warm:.1}s): {}", m.id, res.text.trim());
      assert!(!res.text.trim().is_empty(), "{} produced no text", m.id);
    }
  }
}
