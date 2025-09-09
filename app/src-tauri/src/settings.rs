// Settings helpers and OpenAI model listing

pub fn get_api_key_from_settings_or_env() -> Result<String, String> {
  crate::config::get_api_key_from_settings_or_env()
}

pub fn get_model_from_settings_or_env() -> String {
  crate::config::get_model_from_settings_or_env()
}

pub fn get_temperature_from_settings_or_env() -> Option<f32> {
  crate::config::get_temperature_from_settings_or_env()
}

#[tauri::command]
pub async fn list_openai_models() -> Result<Vec<String>, String> {
  let key = get_api_key_from_settings_or_env()?;
  let client = reqwest::Client::new();
  let resp = client
    .get("https://api.openai.com/v1/models")
    .bearer_auth(key)
    .send()
    .await
    .map_err(|e| format!("request failed: {e}"))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    return Err(format!("OpenAI error: {status} {body_text}"));
  }

  let v: serde_json::Value = resp.json().await.map_err(|e| format!("json error: {e}"))?;
  let mut ids: Vec<String> = v.get("data")
    .and_then(|d| d.as_array())
    .map(|arr| arr.iter()
      .filter_map(|m| m.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()))
      .filter(|id| id.starts_with("gpt-") || id.contains("gpt-4") || id.contains("gpt-4o"))
      .collect())
    .unwrap_or_else(|| Vec::new());
  ids.sort();
  ids.dedup();
  Ok(ids)
}
