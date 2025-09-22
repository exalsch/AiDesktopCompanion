// TTS module facade: re-export OpenAI, generic utils, and Windows-native TTS helpers
// This file replaces the legacy tts.rs implementation to avoid duplication.
#![allow(unused_imports)]

pub use crate::tts_utils::{
  write_pcm16_wav_from_any,
  apply_wav_gain_and_rate,
  find_sse_event_boundary,
  consume_leading_newlines,
  extract_sse_data,
  delete_temp_wav,
  cleanup_stale_tts_wavs,
};

pub use crate::tts_openai::{
  openai_synthesize_file,
  openai_synthesize_wav,
  openai_stream_start,
  openai_stream_stop,
  spawn_speech_stream,
  responses_stream_start,
  spawn_responses_stream,
  ensure_streaming_server,
  create_stream_session,
  stop_stream_session,
  stream_session_count,
  stream_cleanup_idle,
};

pub use crate::tts_win_native::{
  local_tts_start,
  local_tts_stop,
  local_tts_list_voices,
  local_speak_blocking,
  local_tts_synthesize_wav,
};
