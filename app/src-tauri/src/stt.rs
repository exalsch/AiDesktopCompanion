use reqwest;
use once_cell::sync::Lazy;

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
  reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(60))
    .connect_timeout(std::time::Duration::from_secs(10))
    .build()
    .unwrap_or_else(|_| reqwest::Client::new())
});

fn build_transcriptions_url(base_url: &str) -> String {
  let b = base_url.trim().trim_end_matches('/');
  if b.ends_with("/v1") {
    format!("{}/audio/transcriptions", b)
  } else {
    format!("{}/v1/audio/transcriptions", b)
  }
}

/// Transcribe audio bytes using OpenAI Whisper API (expects WEBM/Opus by default).
/// Returns the transcribed text on success.
pub async fn transcribe(key: Option<String>, base_url: String, model: String, audio: Vec<u8>, mime: String) -> Result<String, String> {
  if audio.is_empty() { return Err("Audio data is empty".into()); }
  // Build multipart form: model + file
  let file_name = if mime.contains("webm") { "audio.webm" } else { "audio.bin" };
  let part = reqwest::multipart::Part::bytes(audio)
    .file_name(file_name.to_string())
    .mime_str(&mime)
    .map_err(|e| format!("mime error: {e}"))?;

  let mut form = reqwest::multipart::Form::new()
    .text("model", model)
    .part("file", part);

  // `prompt` is the API's own vocabulary hint: a sample of the expected text
  // that biases the decoder toward these spellings. Sent only when there is
  // something to say, so an empty setting does not add a field to every call.
  let vocabulary = crate::config::get_stt_vocabulary_hint();
  if !vocabulary.is_empty() {
    form = form.text("prompt", vocabulary);
  }

  let client = &*CLIENT;
  let url = build_transcriptions_url(&base_url);
  let req = client
    .post(url)
    .multipart(form);
  let req = if let Some(k) = key {
    if k.trim().is_empty() { req } else { req.bearer_auth(k) }
  } else {
    req
  };
  let resp = req
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(format!("STT error: {status} {body}"));
  }

  let body = resp.bytes().await.map_err(|e| format!("read body error: {e}"))?;
  if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&body) {
    let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
    // Return the extracted text — even if empty. The old fallback would return raw JSON
    // (e.g. {"text":"","usage":...}) which the frontend would paste as-is.
    return Ok(text);
  }
  // Only fall back to raw body if JSON parsing fails entirely (non-JSON response)
  let text = String::from_utf8_lossy(&body).to_string();
  Ok(text)
}

/// Inline audio in a Gemini request counts toward its 20 MB request limit, and
/// base64 grows it by a third. Past this the request would be rejected anyway;
/// failing here says why. About ten minutes of the 16 kHz mono WAV the
/// frontend records.
const GEMINI_MAX_AUDIO_BYTES: usize = 14 * 1024 * 1024;

/// Transcribe with a Gemini model.
///
/// Gemini's OpenAI-compatible endpoint has no `/audio/transcriptions` (it
/// answers 404), but its chat completions accept audio as an `input_audio`
/// part, so transcription is a chat request with an instruction. The
/// instruction sits in the user turn rather than a system message because
/// some audio models reject system instructions outright.
pub async fn transcribe_gemini(model: String, audio: Vec<u8>, mime: String) -> Result<String, String> {
  if audio.is_empty() { return Err("Audio data is empty".into()); }
  let endpoint = crate::llm_provider::chat_endpoint_for_model(&model)?;
  let m = mime.to_ascii_lowercase();
  let format = if m.contains("wav") {
    "wav"
  } else if m.contains("mpeg") || m.contains("mp3") {
    "mp3"
  } else {
    return Err(format!("Gemini transcription needs WAV or MP3 audio, got {mime}"));
  };
  if audio.len() > GEMINI_MAX_AUDIO_BYTES {
    return Err(format!(
      "Recording is too long for Gemini transcription ({} MB, limit about {} MB). Use a shorter recording or a local engine.",
      audio.len() / (1024 * 1024),
      GEMINI_MAX_AUDIO_BYTES / (1024 * 1024)
    ));
  }

  let mut instruction = "Transcribe this audio verbatim, in the language that is spoken. Return only the transcript: no commentary, no labels, no timestamps. If there is no speech, return nothing.".to_string();
  let vocabulary = crate::config::get_stt_vocabulary_hint();
  if !vocabulary.is_empty() {
    instruction.push_str(&format!(" Names and terms that may occur, spelled correctly: {vocabulary}. Use these spellings only where the audio actually says them."));
  }

  let b64 = {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(&audio)
  };
  let body = serde_json::json!({
    "model": model,
    "temperature": 0,
    "messages": [{
      "role": "user",
      "content": [
        { "type": "input_audio", "input_audio": { "data": b64, "format": format } },
        { "type": "text", "text": instruction }
      ]
    }]
  });

  let resp = CLIENT
    .post(&endpoint.url)
    .bearer_auth(&endpoint.key)
    .json(&body)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;
  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(format!("STT error (Gemini): {status} {body}"));
  }
  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  Ok(v
    .pointer("/choices/0/message/content")
    .and_then(|t| t.as_str())
    .unwrap_or("")
    .trim()
    .to_string())
}
