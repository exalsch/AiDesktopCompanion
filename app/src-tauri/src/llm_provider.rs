//! Which API a chat model is served by.
//!
//! Chat, quick prompts and STT post-processing all speak the OpenAI
//! chat-completions protocol. Google serves Gemini over an OpenAI-compatible
//! endpoint as well, so supporting it is a matter of picking the right base
//! URL and key: the request and response bodies stay the same.
//!
//! The provider follows from the model id. Any model whose id starts with
//! `gemini-` goes to Google, everything else to OpenAI. That keeps the
//! existing per-feature model settings (`openai_chat_model`,
//! `quick_prompt_model`, `stt_post_process_model`) as the only switch, so one
//! feature can run on Gemini while another stays on OpenAI.
//!
//! Speech (TTS, cloud STT) and Assistant Mode (Realtime) are OpenAI-only APIs
//! and do not go through here.

pub const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
pub const GEMINI_OPENAI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/openai";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
  OpenAi,
  Gemini,
}

impl Provider {
  /// Name used in error messages, so a failure says which service rejected it.
  pub fn label(self) -> &'static str {
    match self {
      Provider::OpenAi => "OpenAI",
      Provider::Gemini => "Gemini",
    }
  }
}

pub fn provider_for_model(model: &str) -> Provider {
  let m = model.trim().to_ascii_lowercase();
  let m = m.strip_prefix("models/").unwrap_or(&m);
  if m.starts_with("gemini-") { Provider::Gemini } else { Provider::OpenAi }
}

/// Where to send a chat completion for `model`, and with which key.
pub struct ChatEndpoint {
  pub provider: Provider,
  pub url: String,
  pub key: String,
}

pub fn chat_endpoint_for_model(model: &str) -> Result<ChatEndpoint, String> {
  let provider = provider_for_model(model);
  match provider {
    Provider::OpenAi => Ok(ChatEndpoint {
      provider,
      url: OPENAI_CHAT_COMPLETIONS_URL.to_string(),
      key: crate::config::get_api_key_from_settings_or_env()?,
    }),
    Provider::Gemini => Ok(ChatEndpoint {
      provider,
      url: format!("{GEMINI_OPENAI_BASE_URL}/chat/completions"),
      key: crate::config::get_gemini_api_key_from_settings_or_env()?,
    }),
  }
}

/// Gemini models usable for text chat, from the compat `/models` listing.
///
/// The listing returns ids as `models/<id>` and mixes in embedding, speech,
/// image, video and live models that the chat-completions endpoint rejects;
/// those are dropped so the model pickers only offer what can actually work.
pub fn filter_gemini_chat_models(ids: &[String]) -> Vec<String> {
  const NOT_CHAT: [&str; 7] = ["embedding", "tts", "image", "live", "audio", "transcribe", "computer-use"];
  let mut out: Vec<String> = ids
    .iter()
    .map(|id| id.strip_prefix("models/").unwrap_or(id).to_string())
    .filter(|id| id.starts_with("gemini-"))
    .filter(|id| !NOT_CHAT.iter().any(|bad| id.contains(bad)))
    .collect();
  out.sort();
  out.dedup();
  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn routes_by_model_prefix() {
    assert_eq!(provider_for_model("gemini-2.5-flash"), Provider::Gemini);
    assert_eq!(provider_for_model(" Gemini-3.5-Flash "), Provider::Gemini);
    assert_eq!(provider_for_model("models/gemini-pro-latest"), Provider::Gemini);
    assert_eq!(provider_for_model("gpt-4o-mini"), Provider::OpenAi);
    assert_eq!(provider_for_model(""), Provider::OpenAi);
  }

  #[test]
  fn keeps_only_chat_capable_gemini_models() {
    let ids: Vec<String> = [
      "models/gemini-2.5-flash",
      "models/gemini-2.5-flash-preview-tts",
      "models/gemini-embedding-001",
      "models/gemini-3-pro-image",
      "models/gemini-3.8-live",
      "models/gemini-2.5-flash-native-audio-latest",
      "models/gemma-4-31b-it",
      "models/veo-3.1-generate-preview",
      "models/gemini-flash-latest",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(filter_gemini_chat_models(&ids), vec!["gemini-2.5-flash", "gemini-flash-latest"]);
  }
}
