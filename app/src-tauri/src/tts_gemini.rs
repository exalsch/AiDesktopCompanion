//! Gemini text-to-speech.
//!
//! Google's OpenAI-compatible endpoint has no `/audio/speech`, so this talks to
//! the native `generateContent` API with an audio response modality. The reply
//! is base64 audio inline in JSON: raw 16-bit little-endian PCM on the older
//! models (`audio/L16;codec=pcm;rate=24000`), a complete WAV on newer ones
//! (`audio/wav`). Both end up as a WAV in the temp dir, run through the same
//! rate/volume step as OpenAI's output so the sliders mean the same thing.
//!
//! No streaming: the whole clip arrives in one response.

use base64::Engine;

use crate::tts_utils::write_pcm16_wav_from_any;

pub const DEFAULT_GEMINI_TTS_MODEL: &str = "gemini-3.8-flash-tts";
pub const DEFAULT_GEMINI_TTS_VOICE: &str = "Kore";

/// The prompt actually sent.
///
/// Without a tone it is the bare text. The 3.8 models speak any instruction in
/// front of it ("Say: Hi." comes out as "Say hi"). With a tone the style goes
/// inline: a separate "Style: ..." line made `gemini-3.8-flash-lite-tts` return
/// silence.
fn build_prompt(text: &str, tone: Option<&str>) -> String {
  match tone.map(str::trim).filter(|t| !t.is_empty()) {
    Some(t) => format!("Say in this style ({t}): {text}"),
    None => text.to_string(),
  }
}

/// Retry prompt for a model that refused bare text ("Model tried to generate
/// text, but it should only be used for TTS"). `gemini-2.5-flash-preview-tts`
/// does that for a short "Hi." and reads this form without speaking the "Say".
fn build_retry_prompt(text: &str) -> String {
  format!("Say: {text}")
}

/// Sample rate from a mime type such as `audio/L16;codec=pcm;rate=24000`.
fn pcm_rate_from_mime(mime: &str) -> u32 {
  mime
    .split(';')
    .filter_map(|p| p.trim().strip_prefix("rate="))
    .find_map(|r| r.trim().parse::<u32>().ok())
    .unwrap_or(24000)
}

/// Wrap mono 16-bit little-endian PCM in a WAV container.
fn wav_from_pcm16le(pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>, String> {
  let mut out = std::io::Cursor::new(Vec::new());
  {
    let mut w = hound::WavWriter::new(&mut out, hound::WavSpec {
      channels: 1,
      sample_rate,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    }).map_err(|e| format!("wav writer create failed: {e}"))?;
    for chunk in pcm.chunks_exact(2) {
      w.write_sample(i16::from_le_bytes([chunk[0], chunk[1]])).map_err(|e| format!("wav write sample failed: {e}"))?;
    }
    w.finalize().map_err(|e| format!("wav finalize failed: {e}"))?;
  }
  Ok(out.into_inner())
}

async fn request_audio(key: &str, model: &str, voice: &str, prompt: &str) -> Result<(String, Vec<u8>), String> {
  let url = format!("https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent");
  let body = serde_json::json!({
    "contents": [{ "parts": [{ "text": prompt }] }],
    "generationConfig": {
      "responseModalities": ["AUDIO"],
      "speechConfig": { "voiceConfig": { "prebuiltVoiceConfig": { "voiceName": voice } } }
    }
  });
  let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(120))
    .connect_timeout(std::time::Duration::from_secs(10))
    .build()
    .unwrap_or_else(|_| reqwest::Client::new());
  let resp = client
    .post(url)
    .header("x-goog-api-key", key)
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;
  if !resp.status().is_success() {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    return Err(format!("Gemini error: {status} {body_text}"));
  }
  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  let inline = v
    .pointer("/candidates/0/content/parts")
    .and_then(|p| p.as_array())
    .and_then(|parts| parts.iter().find_map(|p| p.get("inlineData")))
    .ok_or_else(|| "Gemini returned no audio".to_string())?;
  let mime = inline.get("mimeType").and_then(|x| x.as_str()).unwrap_or("").to_string();
  let data = inline.get("data").and_then(|x| x.as_str()).unwrap_or("");
  let bytes = base64::engine::general_purpose::STANDARD
    .decode(data)
    .map_err(|e| format!("audio decode failed: {e}"))?;
  if bytes.is_empty() { return Err("Gemini returned empty audio".into()); }
  Ok((mime, bytes))
}

/// Synthesize `text` and return the path of a WAV in the temp dir.
pub async fn gemini_synthesize_wav(
  key: String,
  text: String,
  voice: Option<String>,
  model: Option<String>,
  rate: Option<i32>,
  volume: Option<u8>,
  instructions: Option<String>,
) -> Result<String, String> {
  let text = text.trim().to_string();
  if text.is_empty() { return Err("Text is empty".into()); }
  let model = model.map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).unwrap_or_else(|| DEFAULT_GEMINI_TTS_MODEL.to_string());
  let voice = voice.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| DEFAULT_GEMINI_TTS_VOICE.to_string());
  let prompt = build_prompt(&text, instructions.as_deref());

  let (mime, bytes) = match request_audio(&key, &model, &voice, &prompt).await {
    Err(e) if e.contains("tried to generate text") => {
      request_audio(&key, &model, &voice, &build_retry_prompt(&text)).await?
    }
    other => other?,
  };
  let wav = if mime.to_ascii_lowercase().contains("wav") {
    bytes
  } else {
    wav_from_pcm16le(&bytes, pcm_rate_from_mime(&mime))?
  };

  let file_name = format!("aidc_tts_{}_gemini.wav", chrono::Local::now().format("%Y%m%d_%H%M%S"));
  let mut path = std::env::temp_dir();
  path.push(file_name);
  let target = path.to_string_lossy().to_string();
  let r = rate.unwrap_or(0).clamp(-10, 10);
  let vol = volume.unwrap_or(100).min(100);
  if let Err(e) = write_pcm16_wav_from_any(&wav, &target, r, vol) {
    let _ = std::fs::remove_file(&target);
    return Err(e);
  }
  Ok(target)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_rate_from_mime() {
    assert_eq!(pcm_rate_from_mime("audio/L16;codec=pcm;rate=24000"), 24000);
    assert_eq!(pcm_rate_from_mime("audio/l16; rate=16000; channels=1"), 16000);
    assert_eq!(pcm_rate_from_mime("audio/L16"), 24000);
  }

  #[test]
  fn wraps_pcm_as_wav() {
    let pcm: Vec<u8> = [0i16, 1000, -1000, i16::MAX].iter().flat_map(|s| s.to_le_bytes()).collect();
    let wav = wav_from_pcm16le(&pcm, 24000).unwrap();
    let mut r = hound::WavReader::new(std::io::Cursor::new(wav)).unwrap();
    assert_eq!(r.spec().sample_rate, 24000);
    assert_eq!(r.spec().channels, 1);
    let samples: Vec<i16> = r.samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples, vec![0, 1000, -1000, i16::MAX]);
  }

  #[test]
  fn tone_goes_inline() {
    assert_eq!(build_prompt("Hi.", Some("cheerful")), "Say in this style (cheerful): Hi.");
    assert_eq!(build_prompt("Hi.", Some("  ")), "Hi.");
    assert_eq!(build_prompt("Hi.", None), "Hi.");
  }
}
