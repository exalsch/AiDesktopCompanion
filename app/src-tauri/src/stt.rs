use reqwest;

/// Transcribe audio bytes using OpenAI Whisper API (expects WEBM/Opus by default).
/// Returns the transcribed text on success.
pub async fn transcribe(key: String, audio: Vec<u8>, mime: String) -> Result<String, String> {
  // Build multipart form: model + file
  let file_name = if mime.contains("webm") { "audio.webm" } else { "audio.bin" };
  let part = reqwest::multipart::Part::bytes(audio)
    .file_name(file_name.to_string())
    .mime_str(&mime)
    .map_err(|e| format!("mime error: {e}"))?;

  let form = reqwest::multipart::Form::new()
    .text("model", "whisper-1")
    .part("file", part);

  let client = reqwest::Client::new();
  let resp = client
    .post("https://api.openai.com/v1/audio/transcriptions")
    .bearer_auth(key)
    .multipart(form)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    return Err(format!("OpenAI error: {status} {body}"));
  }

  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  let text = v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string();
  Ok(text)
}
