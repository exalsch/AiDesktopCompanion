# STT: Audio File Transcription with Speaker Diarization

Status: phase 1 (local) implemented, phase 2 (cloud) planned.

## Goal

Turn an audio file on disk (a meeting recording, a voice memo, a podcast) into a
transcript with timestamps. Optionally label who is speaking ("Speaker 1",
"Speaker 2"), with names the user can fill in afterwards.

Dictation stays as it is. This is a second view in the STT section: the sidebar
gets an **Audio Files** sub-item under STT, in the same way Prompt has History.

## Phases

1. **Local (this change).** Whisper or Parakeet V3 for the words, NVIDIA
   Sortformer v2.1 for the speakers. Everything runs on the machine.
2. **Cloud (later).** OpenAI `gpt-4o-transcribe-diarize` with the
   `diarized_json` response format, behind the existing cloud STT key and base
   URL. Uploads are capped at 25 MB, so long files need re-encoding or chunking
   first. Check the current API docs before building it; nothing in this
   document was verified against them.

## Why these pieces

- **Decoding** reuses `symphonia`, already a dependency with mp3, aac/m4a, flac,
  ogg/vorbis and wav enabled. Opus and wma are not supported by symphonia and
  fail with a clear error.
- **Diarization** comes from `parakeet-rs`, the crate already pinned at 0.2.6
  for Parakeet. Its `sortformer` cargo feature has no extra dependencies and
  runs on the same `ort`, so turning it on cannot cause a version fight.
  Sortformer handles **at most 4 speakers**. A fifth voice is folded into one
  of the four.
- The speaker model (`diar_streaming_sortformer_4spk-v2.1.onnx`, about 490 MB,
  CC-BY-4.0 by NVIDIA) is downloaded on first use from
  `huggingface.co/altunenes/parakeet-rs` into
  `%APPDATA%\AiDesktopCompanion\models\parakeet\sortformer\`.

## Pipeline (`stt_file.rs`)

1. **Decode** the file straight from disk to 16 kHz mono f32. Unlike the
   dictation decoder, this one downmixes and resamples packet by packet, so an
   hour of 48 kHz stereo never sits in memory as interleaved source samples
   (1h at 16 kHz mono is about 230 MB; the source would be about 1,4 GB).
2. **Chunk** into pieces of about 4 minutes. Parakeet TDT has a sequence
   length limit around 8-10 minutes, so it cannot take a long file in one
   call. Each cut lands in the quietest half second within the last 20 seconds
   before the target, so a cut rarely splits a word. Whisper uses the same
   chunks; that keeps progress and cancel granular for both engines.
3. **Transcribe** each chunk with segment timestamps (Parakeet: sentence mode;
   Whisper: its own segments) and shift them by the chunk's start time.
4. **Diarize** (when asked) the whole file in one Sortformer pass. It streams
   internally in ~10 s chunks with a speaker cache, so speaker IDs stay stable
   across the file. Each transcript segment takes the speaker it overlaps
   most. A segment with no overlap stays unlabelled.
5. **Return** the segments. The frontend merges consecutive segments of the
   same speaker into turns for display and export.

Model loading, decoding, transcription and diarization run on a blocking
thread. Progress goes out as `stt-file:progress` events. Cancel has its own
flag, separate from dictation, so stopping a dictation does not kill a file job
and the other way round. Only one file job runs at a time.

## Known limits

- Whisper segments can be several seconds long and occasionally straddle a
  speaker change; the segment then goes to whoever spoke most of it. Parakeet
  sentences are shorter, so Parakeet gives the cleaner speaker split.
- The Sortformer pass cannot be interrupted midway; a cancel takes effect when
  it returns.
- While a Parakeet file job runs, a Parakeet dictation waits for the current
  chunk to finish (they share one loaded model).
- Whisper uses half the CPU cores for a file job (dictation uses all but one),
  so a long job does not make the machine unusable.
- Parakeet sentences are built from raw tokens in `sentences_from_tokens`,
  not with the crate's `Sentences` mode. That mode drops the space in front of
  a number ("um12 Uhr") and swallows a repeated word.
- Parakeet sometimes hears a short "Ha ha ha." in a silent gap. Sortformer finds no
  speaker there, so it shows up as "Unknown". It is kept rather than filtered,
  because dropping text by guesswork is worse than showing a stray word.
- No AI cleanup pass on file transcripts. Long transcripts would need chunked
  prompting and the cleanup prompt is tuned for dictation.

## UI (`SttFilePanel.vue`)

- Pick a file (dialog filter: mp3, m4a, mp4, aac, wav, flac, ogg, oga).
- Engine: Parakeet V3 or Whisper (the Whisper model configured in Settings).
  Defaults to whichever local model Settings has selected.
- **Identify speakers** checkbox. When the speaker model is missing, a
  download button with progress appears next to it.
- Progress bar with the stage and chunk count, and a Stop button.
- Result: turns with `[mm:ss]` timestamps and speaker labels. Speaker labels
  can be renamed inline; renames apply everywhere at once.
- Copy, **Use as prompt**, and export as `.txt`, `.md` or `.srt`.

Job state lives in a module-level composable (`useSttFileJob`), so leaving the
view while a long file runs does not lose the result.

## Testing

`cargo test --features local-stt --lib stt_file` runs the unit tests
(resampler, chunk planner, sentence grouping, speaker assignment). A real run
over a file is an ignored test:

```powershell
$env:STT_FILE_SAMPLE="C:\path\to\dialog.wav"
cargo test --features local-stt --lib stt_file::tests::sample -- --ignored --nocapture
```

It runs Parakeet only. `STT_FILE_WHISPER=1` adds the Whisper model from
Settings, but never in a debug build with a large model: unoptimised
whisper.cpp on the CPU ran for over 15 minutes on one minute of audio and
pinned every core. In a debug build the Parakeet run on 68 seconds of
two-voice German took about 1 minute with CUDA (3 on the CPU) and got every sentence's speaker right.

## Backend commands

| Command | Purpose |
|---|---|
| `stt_file_transcribe(path, engine, diarize)` | Runs the pipeline, returns `FileTranscript` |
| `stt_file_cancel()` | Stops the running file job |
| `stt_diarizer_status()` | Whether the speaker model is on disk, and its path |
| `stt_prefetch_diarizer_model()` | Downloads it, emits `stt-diarizer-download` |
| `stt_file_save_text(path, contents)` | Writes an export chosen in the save dialog |
