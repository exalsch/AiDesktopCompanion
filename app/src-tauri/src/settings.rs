// Settings helpers and model listing

pub fn get_api_key_from_settings_or_env() -> Result<String, String> {
  crate::config::get_api_key_from_settings_or_env()
}

pub fn get_model_from_settings_or_env() -> String {
  crate::config::get_model_from_settings_or_env()
}

pub fn get_temperature_from_settings_or_env() -> Option<f32> {
  crate::config::get_temperature_from_settings_or_env()
}

/// GET an OpenAI-style `/models` listing and return the raw ids.
async fn fetch_model_ids(url: &str, key: &str, label: &str) -> Result<Vec<String>, String> {
  let client = reqwest::Client::builder()
    .timeout(std::time::Duration::from_secs(15))
    .connect_timeout(std::time::Duration::from_secs(10))
    .build()
    .unwrap_or_else(|_| reqwest::Client::new());
  let resp = client
    .get(url)
    .bearer_auth(key)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    return Err(format!("{label} error: {status} {body_text}"));
  }

  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  Ok(v.get("data")
    .and_then(|d| d.as_array())
    .map(|arr| arr.iter()
      .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
      .collect())
    .unwrap_or_default())
}

#[tauri::command]
pub async fn list_openai_models() -> Result<Vec<String>, String> {
  let key = get_api_key_from_settings_or_env()?;
  let mut ids: Vec<String> = fetch_model_ids("https://api.openai.com/v1/models", &key, "OpenAI")
    .await?
    .into_iter()
    .filter(|id| id.starts_with("gpt-") || id.contains("gpt-4") || id.contains("gpt-4o"))
    .collect();
  ids.sort();
  ids.dedup();
  Ok(ids)
}

async fn list_gemini_models() -> Result<Vec<String>, String> {
  let key = crate::config::get_gemini_api_key_from_settings_or_env()?;
  let url = format!("{}/models", crate::llm_provider::GEMINI_OPENAI_BASE_URL);
  let ids = fetch_model_ids(&url, &key, "Gemini").await?;
  Ok(crate::llm_provider::filter_gemini_chat_models(&ids))
}

/// Models for the chat-style pickers (chat, quick prompts, STT cleanup):
/// OpenAI's and Gemini's, from whichever providers have a key.
///
/// One provider failing does not hide the other's models. Only when neither
/// answers is it an error, and then both reasons are reported.
#[tauri::command]
pub async fn list_chat_models() -> Result<Vec<String>, String> {
  let (openai, gemini) = tokio::join!(list_openai_models(), list_gemini_models());
  match (openai, gemini) {
    (Err(a), Err(b)) => Err(format!("{a}; {b}")),
    (a, b) => {
      let mut ids = a.unwrap_or_default();
      ids.extend(b.unwrap_or_default());
      Ok(ids)
    }
  }
}
