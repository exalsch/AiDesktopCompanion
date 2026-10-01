// Audio file transcription, optionally with speaker labels.
//
// Dictation hands the backend a few seconds of microphone audio. A file can
// be an hour-long meeting, which changes three things: it is decoded straight
// from disk into 16 kHz mono without ever holding the source samples, it is
// cut into chunks of a few minutes (Parakeet TDT cannot take more than about
// 8-10 minutes in one call) and it runs on a blocking thread with progress and
// its own cancel flag. Speaker labels come from one Sortformer pass over the
// whole file, matched to the transcript by time overlap.
// See specs/STT_FileTranscription_design.md.

// Without local engines only the command stubs are reachable.
#![cfg_attr(not(feature = "local-stt"), allow(dead_code))]

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

/// Returned when the user stopped the job. The frontend matches on this text
/// to stay quiet rather than report it as a failure.
pub const CANCELLED_MESSAGE: &str = "Transcription cancelled";

const SAMPLE_RATE: usize = 16_000;
/// Chunks aim for this length. Well under TDT's limit, short enough that
/// progress moves and a Stop lands within a few seconds of model time.
const CHUNK_TARGET_S: usize = 240;
/// How far back from the target a cut may move to find a quiet spot.
const CUT_SEARCH_S: usize = 20;

/// A file job's own cancel, separate from dictation's: stopping a dictation
/// must not kill a long file job, nor the other way round.
static CANCEL: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Piece of transcript with times in seconds.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimedText {
  pub start: f32,
  pub end: f32,
  pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FileSegment {
  pub start: f32,
  pub end: f32,
  /// Zero-based, numbered in order of first appearance. `None` when speakers
  /// were not requested or nobody overlapped this stretch.
  pub speaker: Option<usize>,
  pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FileTranscript {
  pub file_name: String,
  pub duration_s: f32,
  pub engine: String,
  pub diarized: bool,
  pub speaker_count: usize,
  pub segments: Vec<FileSegment>,
}

#[derive(Clone, Debug, Serialize)]
struct Progress {
  /// "decoding" | "model" | "speaker-model" | "transcribing" | "speakers"
  stage: &'static str,
  chunk: usize,
  chunks: usize,
  duration_s: f32,
}

fn emit_progress(app: &tauri::AppHandle, stage: &'static str, chunk: usize, chunks: usize, duration_s: f32) {
  use tauri::Emitter;
  let _ = app.emit_to("main", "stt-file:progress", Progress { stage, chunk, chunks, duration_s });
}

fn cancelled() -> bool {
  CANCEL.load(Ordering::SeqCst)
}

fn check_cancel() -> Result<(), String> {
  if cancelled() { Err(CANCELLED_MESSAGE.to_string()) } else { Ok(()) }
}

/// Abort callback for whisper.cpp, polled between decoding steps.
#[cfg(feature = "local-stt")]
unsafe extern "C" fn whisper_abort_callback(_user_data: *mut std::ffi::c_void) -> bool {
  cancelled()
}

struct RunningGuard;
impl Drop for RunningGuard {
  fn drop(&mut self) {
    RUNNING.store(false, Ordering::SeqCst);
  }
}

// ---- Decoding ----

/// Linear-interpolation resampler that takes its input in pieces, so a long
/// file never has to sit in memory at its source rate.
pub(crate) struct StreamResampler {
  /// Source samples per output sample.
  step: f64,
  /// Source position of the next output sample.
  t: f64,
  /// Source samples consumed so far.
  seen: u64,
  last: f32,
}

impl StreamResampler {
  pub(crate) fn new(src_rate: u32, dst_rate: u32) -> Self {
    Self { step: src_rate as f64 / dst_rate as f64, t: 0.0, seen: 0, last: 0.0 }
  }

  pub(crate) fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
    if input.is_empty() { return; }
    let end = self.seen + input.len() as u64;
    // Interpolating at t needs samples floor(t) and floor(t)+1. The loop stops
    // once the second one is beyond this block, so floor(t) is never further
    // back than the last sample of the previous block, which `last` keeps.
    loop {
      let i0 = self.t.floor() as u64;
      if i0 + 1 >= end { break; }
      let a = if i0 < self.seen { self.last } else { input[(i0 - self.seen) as usize] };
      let b = input[(i0 + 1 - self.seen) as usize];
      let frac = (self.t - i0 as f64) as f32;
      out.push(a + (b - a) * frac);
      self.t += self.step;
    }
    self.last = input[input.len() - 1];
    self.seen = end;
  }

  /// Emits the samples that fall on the very last source sample.
  pub(crate) fn finish(&mut self, out: &mut Vec<f32>) {
    while self.seen > 0 && self.t <= (self.seen - 1) as f64 {
      out.push(self.last);
      self.t += self.step;
    }
  }
}

/// Decodes an audio file to 16 kHz mono f32, packet by packet.
fn decode_file_to_16k_mono(path: &std::path::Path) -> Result<Vec<f32>, String> {
  use symphonia::core::codecs::audio::AudioDecoderOptions;
  use symphonia::core::formats::probe::Hint;
  use symphonia::core::formats::{FormatOptions, TrackType};
  use symphonia::core::io::MediaSourceStream;
  use symphonia::core::meta::MetadataOptions;

  let file = std::fs::File::open(path).map_err(|e| format!("Could not open the file: {e}"))?;
  let mss = MediaSourceStream::new(Box::new(file), Default::default());
  let mut hint = Hint::new();
  if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
    hint.with_extension(ext);
  }
  let mut format = symphonia::default::get_probe()
    .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
    .map_err(|e| format!("Unsupported or unreadable audio file: {e}"))?;
  let track = format
    .default_track(TrackType::Audio)
    .ok_or_else(|| "The file has no audio track.".to_string())?;
  let track_id = track.id;
  let audio_params = track
    .codec_params
    .as_ref()
    .and_then(|p| p.audio())
    .ok_or_else(|| "The audio track has no codec parameters.".to_string())?
    .clone();
  let mut decoder = symphonia::default::get_codecs()
    .make_audio_decoder(&audio_params, &AudioDecoderOptions::default())
    .map_err(|e| format!("No decoder for this audio format: {e}"))?;

  let mut out: Vec<f32> = Vec::new();
  let mut resampler: Option<StreamResampler> = None;
  let mut frame: Vec<f32> = Vec::new();
  let mut mono: Vec<f32> = Vec::new();
  let mut packets: u64 = 0;

  while let Ok(Some(packet)) = format.next_packet() {
    if packet.track_id != track_id { continue; }
    // Decoding an hour of audio takes a while; let a Stop through.
    packets += 1;
    if packets % 256 == 0 { check_cancel()?; }
    let Ok(buf) = decoder.decode(&packet) else { continue };
    let spec = buf.spec();
    let channels = spec.channels().count().max(1);
    let rate = spec.rate();
    buf.copy_to_vec_interleaved(&mut frame);

    mono.clear();
    if channels == 1 {
      mono.extend_from_slice(&frame);
    } else {
      mono.extend(frame.chunks_exact(channels).map(|c| c.iter().sum::<f32>() / channels as f32));
    }
    // The first packet fixes the rate. Files that change rate midway are
    // rare enough to accept a pitch shift on the remainder.
    let rs = resampler.get_or_insert_with(|| StreamResampler::new(rate, SAMPLE_RATE as u32));
    rs.push(&mono, &mut out);
  }
  if let Some(rs) = resampler.as_mut() {
    rs.finish(&mut out);
  }
  if out.is_empty() {
    return Err("The file decoded to no audio.".into());
  }
  Ok(out)
}

// ---- Chunking ----

/// Splits `len` samples into chunks of about `target` samples. Each cut moves
/// back by up to `search` samples to the middle of the quietest `window`, so
/// a cut rarely lands inside a word. A remainder shorter than a quarter of the
/// target is folded into the last chunk rather than transcribed on its own.
fn plan_chunks(pcm: &[f32], target: usize, search: usize, window: usize) -> Vec<(usize, usize)> {
  let len = pcm.len();
  let mut chunks = Vec::new();
  let mut start = 0usize;
  while len - start > target + target / 4 {
    let hi = start + target;
    let lo = hi.saturating_sub(search).max(start + window);
    let hop = (window / 5).max(1);
    let mut best_cut = hi;
    let mut best_energy = f32::INFINITY;
    let mut w = lo;
    while w + window <= hi {
      let e: f32 = pcm[w..w + window].iter().map(|s| s * s).sum();
      if e < best_energy {
        best_energy = e;
        best_cut = w + window / 2;
      }
      w += hop;
    }
    chunks.push((start, best_cut));
    start = best_cut;
  }
  chunks.push((start, len));
  chunks
}

/// Joins sub-word tokens (a word start carries a leading space, as the TDT
/// decoder emits them) into sentences with times. A sentence ends on a token
/// ending in `.`, `?` or `!` that is followed by a new word or nothing, so
/// the dot in "3.5" or the first one in "z.B." does not split it. Each sentence starts at its first
/// token with text, so a stray leading space token does not stretch it back.
pub(crate) fn sentences_from_tokens(tokens: &[TimedText]) -> Vec<TimedText> {
  let mut out = Vec::new();
  let mut text = String::new();
  let mut start: Option<f32> = None;
  let mut end = 0.0f32;
  for (i, tok) in tokens.iter().enumerate() {
    text.push_str(&tok.text);
    if !tok.text.trim().is_empty() {
      start.get_or_insert(tok.start);
      end = tok.end;
    }
    let ends_on_terminator = tok.text.trim_end().ends_with(['.', '?', '!']);
    let next_starts_word = tokens.get(i + 1).map_or(true, |n| n.text.starts_with(' '));
    if ends_on_terminator && next_starts_word {
      let t = text.trim();
      if let (Some(s), false) = (start, t.is_empty()) {
        out.push(TimedText { start: s, end, text: t.to_string() });
      }
      text.clear();
      start = None;
    }
  }
  let t = text.trim();
  if let (Some(s), false) = (start, t.is_empty()) {
    out.push(TimedText { start: s, end, text: t.to_string() });
  }
  out
}

// ---- Speakers ----

/// Gives each transcript segment the speaker it overlaps most, then renumbers
/// speakers in order of first appearance so "Speaker 1" is whoever talks
/// first. Returns the segments and the number of speakers that got any text.
fn assign_speakers(segments: Vec<TimedText>, turns: &[(f32, f32, usize)]) -> (Vec<FileSegment>, usize) {
  let mut order: Vec<usize> = Vec::new();
  let out = segments
    .into_iter()
    .map(|seg| {
      let mut overlap_by_speaker: Vec<(usize, f32)> = Vec::new();
      for &(s, e, spk) in turns {
        let o = (seg.end.min(e) - seg.start.max(s)).max(0.0);
        if o <= 0.0 { continue; }
        match overlap_by_speaker.iter_mut().find(|(k, _)| *k == spk) {
          Some((_, total)) => *total += o,
          None => overlap_by_speaker.push((spk, o)),
        }
      }
      let raw = overlap_by_speaker
        .into_iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(k, _)| k);
      let speaker = raw.map(|k| match order.iter().position(|&x| x == k) {
        Some(i) => i,
        None => {
          order.push(k);
          order.len() - 1
        }
      });
      FileSegment { start: seg.start, end: seg.end, speaker, text: seg.text }
    })
    .collect();
  (out, order.len())
}

// ---- Commands ----

#[tauri::command]
pub async fn stt_file_transcribe(app: tauri::AppHandle, path: String, engine: String, diarize: bool) -> Result<FileTranscript, String> {
  if RUNNING.swap(true, Ordering::SeqCst) {
    return Err("A file is already being transcribed.".into());
  }
  let _guard = RunningGuard;
  CANCEL.store(false, Ordering::SeqCst);
  let started = std::time::Instant::now();
  let out = run(app, path, engine, diarize).await;
  match &out {
    Ok(t) => println!(
      "[stt-file] done engine={} duration_s={:.0} segments={} speakers={} took_s={:.0}",
      t.engine,
      t.duration_s,
      t.segments.len(),
      t.speaker_count,
      started.elapsed().as_secs_f32()
    ),
    Err(e) => println!("[stt-file] failed: {e}"),
  }
  out
}

#[cfg(feature = "local-stt")]
async fn run(app: tauri::AppHandle, path: String, engine: String, diarize: bool) -> Result<FileTranscript, String> {
  let path = std::path::PathBuf::from(path);
  let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("audio").to_string();
  let use_parakeet = match engine.trim().to_lowercase().as_str() {
    "parakeet" => true,
    "whisper" => false,
    other => return Err(format!("Unknown engine '{other}'. Use 'parakeet' or 'whisper'.")),
  };
  let has_cuda = crate::config::get_stt_parakeet_has_cuda_from_settings_or_env();
  println!("[stt-file] start file={file_name} engine={engine} diarize={diarize} cuda={has_cuda}");

  emit_progress(&app, "decoding", 0, 0, 0.0);
  let decode_path = path.clone();
  let pcm = tokio::task::spawn_blocking(move || decode_file_to_16k_mono(&decode_path))
    .await
    .map_err(|e| format!("decode task failed: {e}"))??;
  let duration_s = pcm.len() as f32 / SAMPLE_RATE as f32;
  check_cancel()?;

  // Downloads cannot run on the blocking thread, so models are fetched first.
  emit_progress(&app, "model", 0, 0, duration_s);
  let model_path = if use_parakeet {
    crate::stt_parakeet::ensure_model_dir().await?
  } else {
    crate::stt_whisper::ensure_model_file().await?
  };
  check_cancel()?;
  let diarizer_path = if diarize {
    emit_progress(&app, "speaker-model", 0, 0, duration_s);
    Some(crate::stt_parakeet::ensure_diarizer(Some(&app)).await?)
  } else {
    None
  };
  check_cancel()?;

  let app2 = app.clone();
  let (segments, speaker_count) = tokio::task::spawn_blocking(move || {
    transcribe_pcm_blocking(
      pcm,
      use_parakeet,
      &model_path,
      diarizer_path.as_deref(),
      has_cuda,
      CHUNK_TARGET_S,
      &|stage, chunk, chunks| emit_progress(&app2, stage, chunk, chunks, duration_s),
    )
  })
  .await
  .map_err(|e| format!("transcription task failed: {e}"))??;

  Ok(FileTranscript {
    file_name,
    duration_s,
    engine: if use_parakeet { "parakeet".into() } else { "whisper".into() },
    diarized: diarize,
    speaker_count,
    segments,
  })
}

/// Chunks, transcribes and (given a speaker model) diarizes decoded audio.
/// Blocking: runs on a blocking thread. `progress` gets the stage plus the
/// chunk index and count.
#[cfg(feature = "local-stt")]
fn transcribe_pcm_blocking(
  pcm: Vec<f32>,
  use_parakeet: bool,
  model_path: &std::path::Path,
  diarizer_path: Option<&std::path::Path>,
  has_cuda: bool,
  chunk_target_s: usize,
  progress: &dyn Fn(&'static str, usize, usize),
) -> Result<(Vec<FileSegment>, usize), String> {
  let plan = plan_chunks(&pcm, chunk_target_s * SAMPLE_RATE, CUT_SEARCH_S * SAMPLE_RATE, SAMPLE_RATE / 2);
  let n = plan.len();
  let mut timed: Vec<TimedText> = Vec::new();

  if use_parakeet {
    for (i, &(s, e)) in plan.iter().enumerate() {
      check_cancel()?;
      progress("transcribing", i, n);
      let offset = s as f32 / SAMPLE_RATE as f32;
      let segs = crate::stt_parakeet::transcribe_sentences_blocking(model_path, has_cuda, pcm[s..e].to_vec())?;
      timed.extend(segs.into_iter().map(|t| TimedText { start: t.start + offset, end: t.end + offset, ..t }));
    }
  } else {
    let slices: Vec<&[f32]> = plan.iter().map(|&(s, e)| &pcm[s..e]).collect();
    let per_chunk = crate::stt_whisper::transcribe_chunks_blocking(model_path, &slices, whisper_abort_callback, |i| {
      check_cancel()?;
      progress("transcribing", i, n);
      Ok(())
    })
    .map_err(|e| if cancelled() { CANCELLED_MESSAGE.to_string() } else { e })?;
    for (segs, &(s, _)) in per_chunk.into_iter().zip(plan.iter()) {
      let offset = s as f32 / SAMPLE_RATE as f32;
      timed.extend(segs.into_iter().map(|t| TimedText { start: t.start + offset, end: t.end + offset, ..t }));
    }
  }
  check_cancel()?;

  match diarizer_path {
    Some(dp) => {
      progress("speakers", n, n);
      let turns = crate::stt_parakeet::diarize_blocking(dp, has_cuda, pcm)?;
      check_cancel()?;
      Ok(assign_speakers(timed, &turns))
    }
    None => Ok((
      timed
        .into_iter()
        .map(|t| FileSegment { start: t.start, end: t.end, speaker: None, text: t.text })
        .collect(),
      0,
    )),
  }
}

#[cfg(not(feature = "local-stt"))]
async fn run(_app: tauri::AppHandle, _path: String, _engine: String, _diarize: bool) -> Result<FileTranscript, String> {
  Err("Local STT is not available: app built without 'local-stt' feature.".into())
}

#[tauri::command]
pub fn stt_file_cancel() -> Result<(), String> {
  CANCEL.store(true, Ordering::SeqCst);
  Ok(())
}

#[derive(Serialize)]
pub struct DiarizerStatus {
  downloaded: bool,
  path: String,
}

#[tauri::command]
pub fn stt_diarizer_status() -> Result<DiarizerStatus, String> {
  let (downloaded, path) = crate::stt_parakeet::diarizer_status()?;
  Ok(DiarizerStatus { downloaded, path })
}

#[tauri::command]
pub async fn stt_prefetch_diarizer_model(app: tauri::AppHandle) -> Result<String, String> {
  let p = crate::stt_parakeet::ensure_diarizer(Some(&app)).await?;
  Ok(p.to_string_lossy().to_string())
}

/// Writes an export to the path the user picked in the save dialog.
#[tauri::command]
pub fn stt_file_save_text(path: String, contents: String) -> Result<(), String> {
  std::fs::write(&path, contents).map_err(|e| format!("Could not save the file: {e}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn resample_all(input: &[f32], src: u32, dst: u32, piece: usize) -> Vec<f32> {
    let mut rs = StreamResampler::new(src, dst);
    let mut out = Vec::new();
    for p in input.chunks(piece) {
      rs.push(p, &mut out);
    }
    rs.finish(&mut out);
    out
  }

  #[test]
  fn resampler_output_does_not_depend_on_how_input_is_split() {
    let input: Vec<f32> = (0..4800).map(|i| (i as f32 * 0.01).sin()).collect();
    let whole = resample_all(&input, 48_000, 16_000, input.len());
    let pieces = resample_all(&input, 48_000, 16_000, 7);
    assert_eq!(whole, pieces);
    assert_eq!(whole.len(), 1600);
  }

  #[test]
  fn resampler_interpolates_a_ramp_when_upsampling() {
    let input: Vec<f32> = (0..100).map(|i| i as f32).collect();
    let out = resample_all(&input, 8_000, 16_000, 13);
    assert_eq!(out.len(), 199);
    assert!((out[1] - 0.5).abs() < 1e-6);
    assert!((out[51] - 25.5).abs() < 1e-6);
    assert_eq!(*out.last().unwrap(), 99.0);
  }

  #[test]
  fn resampler_at_same_rate_is_identity() {
    let input: Vec<f32> = (0..50).map(|i| i as f32).collect();
    assert_eq!(resample_all(&input, 16_000, 16_000, 9), input);
  }

  #[test]
  fn short_audio_is_one_chunk() {
    let pcm = vec![0.1f32; 1000];
    assert_eq!(plan_chunks(&pcm, 800, 100, 10), vec![(0, 1000)]);
  }

  #[test]
  fn chunks_cover_everything_and_cut_in_the_quiet_spot() {
    // Loud everywhere except a quiet gap at 900..940.
    let mut pcm = vec![0.5f32; 3000];
    for s in &mut pcm[900..940] { *s = 0.0; }
    let chunks = plan_chunks(&pcm, 1000, 200, 20);
    assert_eq!(chunks.first().unwrap().0, 0);
    assert_eq!(chunks.last().unwrap().1, 3000);
    for w in chunks.windows(2) {
      assert_eq!(w[0].1, w[1].0);
    }
    let first_cut = chunks[0].1;
    assert!((900..=940).contains(&first_cut), "cut at {first_cut}");
  }

  #[test]
  fn small_remainder_is_folded_into_the_last_chunk() {
    let pcm = vec![0.5f32; 1100];
    assert_eq!(plan_chunks(&pcm, 1000, 100, 10), vec![(0, 1100)]);
  }

  fn tt(start: f32, end: f32, text: &str) -> TimedText {
    TimedText { start, end, text: text.into() }
  }

  #[test]
  fn sentences_keep_the_space_before_numbers_and_repeated_words() {
    let toks = vec![
      tt(0.0, 0.2, " Wir"), tt(0.2, 0.4, " lie"), tt(0.4, 0.5, "gen"), tt(0.5, 0.6, " et"), tt(0.6, 0.7, "wa"),
      tt(0.7, 0.75, " "), tt(0.75, 0.8, "1"), tt(0.8, 0.85, "0"), tt(0.85, 0.9, " %"), tt(0.9, 1.0, "."),
      tt(1.5, 1.6, " die"), tt(1.6, 1.7, " die"), tt(1.7, 1.8, " Uhr"), tt(1.8, 1.9, "?"),
    ];
    let s = sentences_from_tokens(&toks);
    assert_eq!(s, vec![tt(0.0, 1.0, "Wir liegen etwa 10 %."), tt(1.5, 1.9, "die die Uhr?")]);
  }

  #[test]
  fn a_dot_inside_a_word_does_not_end_the_sentence() {
    let toks = vec![tt(0.0, 0.1, " z"), tt(0.1, 0.2, "."), tt(0.2, 0.3, "B"), tt(0.3, 0.4, "."), tt(0.4, 0.5, " 3"), tt(0.5, 0.6, "."), tt(0.6, 0.7, "5"), tt(0.7, 0.8, " Tage")];
    let s = sentences_from_tokens(&toks);
    assert_eq!(s, vec![tt(0.0, 0.4, "z.B."), tt(0.4, 0.8, "3.5 Tage")]);
  }

  #[test]
  fn speakers_follow_largest_overlap_and_are_numbered_by_first_appearance() {
    let segs = vec![tt(0.0, 2.0, "hello"), tt(2.0, 5.0, "hi there"), tt(5.0, 6.0, "ok"), tt(10.0, 11.0, "silence")];
    // Raw ids from the model: 3 speaks first, then 1.
    let turns = vec![(0.0, 2.2, 3), (2.2, 5.5, 1), (5.5, 6.0, 3)];
    let (out, count) = assign_speakers(segs, &turns);
    assert_eq!(count, 2);
    assert_eq!(out[0].speaker, Some(0));
    assert_eq!(out[1].speaker, Some(1));
    // 5.0..5.5 is speaker 1, 5.5..6.0 is speaker 3: a tie goes either way, so
    // only check it got someone.
    assert!(out[2].speaker.is_some());
    assert_eq!(out[3].speaker, None);
  }

  #[test]
  fn overlap_is_summed_per_speaker() {
    // Speaker 1 has two short turns that together outweigh speaker 2's one.
    let segs = vec![tt(0.0, 10.0, "long")];
    let turns = vec![(0.0, 3.0, 1), (3.0, 7.0, 2), (7.0, 10.0, 1)];
    let (out, _) = assign_speakers(segs, &turns);
    assert_eq!(out[0].speaker, Some(0));
    let turns = vec![(3.0, 7.0, 2), (0.0, 3.0, 1), (7.0, 10.0, 1)];
    let (out, _) = assign_speakers(vec![tt(0.0, 10.0, "long")], &turns);
    assert_eq!(out[0].speaker, Some(0));
  }

  /// End-to-end over a real file and the real models. Opt-in, because it
  /// downloads models on first run and takes a while:
  ///   $env:STT_FILE_SAMPLE="C:\path\to\dialog.wav"
  ///   cargo test --features local-stt --lib stt_file::tests::sample -- --ignored --nocapture
  /// Parakeet only by default. Set STT_FILE_WHISPER=1 to add the Whisper model
  /// from Settings, but not in a debug build with a large model: whisper.cpp
  /// unoptimised on the CPU takes many minutes and pins the machine.
  #[cfg(feature = "local-stt")]
  #[tokio::test]
  #[ignore]
  async fn sample_file_end_to_end() {
    let Ok(path) = std::env::var("STT_FILE_SAMPLE") else { return };
    let pcm = decode_file_to_16k_mono(std::path::Path::new(&path)).unwrap();
    println!("decoded {:.1}s", pcm.len() as f32 / SAMPLE_RATE as f32);
    let has_cuda = crate::config::get_stt_parakeet_has_cuda_from_settings_or_env();
    let diarizer = crate::stt_parakeet::ensure_diarizer(None).await.unwrap();
    let mut engines = vec![("parakeet", true, crate::stt_parakeet::ensure_model_dir().await.unwrap())];
    if std::env::var("STT_FILE_WHISPER").is_ok() {
      engines.push(("whisper", false, crate::stt_whisper::ensure_model_file().await.unwrap()));
    }
    for (name, use_parakeet, model) in engines {
      let t0 = std::time::Instant::now();
      // A short chunk target, so even a one-minute sample crosses chunk
      // boundaries and the time offsets get exercised.
      let (segs, speakers) =
        transcribe_pcm_blocking(pcm.clone(), use_parakeet, &model, Some(&diarizer), has_cuda, 20, &|_, _, _| {}).unwrap();
      println!("--- {name}: {} segments, {speakers} speakers, {:.1}s", segs.len(), t0.elapsed().as_secs_f32());
      for s in &segs {
        println!("[{:6.2} - {:6.2}] {:?}: {}", s.start, s.end, s.speaker, s.text);
      }
      assert!(!segs.is_empty());
      assert!(segs.windows(2).all(|w| w[0].start <= w[1].start + 0.5), "segments out of order");
    }
  }
}
