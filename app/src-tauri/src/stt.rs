use reqwest;

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
  // Build multipart form: model + file
  let file_name = if mime.contains("webm") { "audio.webm" } else { "audio.bin" };
  let part = reqwest::multipart::Part::bytes(audio)
    .file_name(file_name.to_string())
    .mime_str(&mime)
    .map_err(|e| format!("mime error: {e}"))?;

  let form = reqwest::multipart::Form::new()
    .text("model", model)
    .part("file", part);

  let client = reqwest::Client::new();
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
    if !text.trim().is_empty() { return Ok(text); }
  }
  let text = String::from_utf8_lossy(&body).to_string();
  Ok(text)
}
