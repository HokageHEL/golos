# Golos native preview

An offline desktop transcription app, developed first for macOS. Tauri hosts a
React/Vite UI; Rust owns file access, a SQLite library, and Whisper inference.
The existing Next.js app remains at the repository root and runs independently.

## Prerequisites

- Node 22.12+ and npm.
- Stable Rust installed through [rustup](https://rustup.rs/).
- Apple command-line tools (`xcode-select --install`) and CMake for the C++ engine.
- Full Xcode and Apple Developer membership are needed later for iOS/store builds.

From the repository root:

```sh
npm ci
npm run native:app
```

`native:app` opens the actual Tauri app. `npm run native:dev` only opens a browser
preview: native actions are intentionally disabled there. No Supabase, AssemblyAI,
Polar, Redis, accounts, or environment secrets are needed for native use.

## First transcription

1. Obtain a compatible whisper.cpp GGML `.bin` model from the
   [upstream model repository](https://huggingface.co/ggerganov/whisper.cpp).
   Recommended: multilingual **`ggml-small-q5_1.bin`** for English/Ukrainian,
   approximately 190 MB on disk. Tiny and Base also work for faster/lighter tests.
   `.en` models support English only. Medium/Large models are rejected before
   loading; supported models are capped at 512 MiB. Model working memory exceeds
   file size, even with quantization.
2. Choose **Import model**. Golos copies and loads the model before activating it.
   Importing/replacing a model can take time; the previous model is retained if
   validation fails. Automatic downloads and disk-space estimates are follow-up work.
3. Import **WAV, MP3, M4A/MP4 (AAC-LC or ALAC), raw AAC-LC, FLAC, Ogg/Vorbis,
   or AIFF**, up to **30 minutes / 512 MiB**. Mono/stereo and up to 8 channels,
   8–192 kHz rates, and supported integer/float PCM formats are converted
   automatically. No external converter or FFmpeg installation is needed.
   MP4 imports its first supported audio track, not video. Opus, HE-AAC, DRM,
   video-only files, and mid-file rate/layout changes are not supported.
4. Choose auto-detection, English, or Ukrainian, then **Transcribe offline**.
5. Edit paragraphs, save locally, and export TXT or SRT through a native save dialog.

Inference currently uses the CPU for a portable baseline. No speaker separation,
recording, summaries, cloud sync, ads, or payment integration is included yet.
An imported English-only model cannot produce Ukrainian transcription.

## Storage and privacy

The app stores `library.sqlite3`, normalized WAV files under `audio/`, and `model.bin`
in Tauri's application data directory (normally
`~/Library/Application Support/com.golos.transcription` on macOS). This is local
storage, not application-level encrypted storage; normal OS permissions and disk
encryption apply. Exported files go where the user chooses.

Imports decode packet by packet, downmix to mono, and use a sinc resampler with
anti-alias filtering to write 16 kHz PCM directly to disk. It does not keep a
second copy of the original compressed/high-rate source. Failed imports remove
partial output. The normalized copy uses about 1.92 MB per audio minute;
transcription currently loads that normalized file into f32 samples (about
3.84 MB/minute, capped at 30 minutes) in addition to the model's working memory.
The model is released when transcription finishes. Release builds strip symbols
and optimize for size; no FFmpeg executable or model is bundled.

Deleting a library entry removes Golos's normalized audio and transcript; it does not
delete the original imported file. Recordings are never sent to a transcription
service. The app has no analytics, ad SDK, or remote model downloader. Development
tooling does access npm/crates registries, independently of audio processing.

Only one desktop instance is allowed, and only one import/transcription/deletion
operation runs at a time. Interrupted transcription becomes retryable on startup.
Cancellation does not retain partial transcripts. Saving/discarding edits is
required before switching recordings or closing the window.

## Verification

```sh
npm run native:build
npm run native:test
cargo clippy --manifest-path native/src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path native/src-tauri/Cargo.toml -- --check
npm run lint
npm run typecheck
npm test
```

The default Rust tests cover PCM decoding, stereo/float imports at common rates,
downmixing, anti-alias filtering, exact output duration, invalid/oversized files,
model limits, timestamp validation, SQLite persistence, and interrupted-job recovery.
An opt-in integration test
runs real Whisper inference with local fixtures, without network access:

```sh
GOLOS_TEST_MODEL=/absolute/path/ggml-small-q5_1.bin \
GOLOS_TEST_AUDIO=/absolute/path/jfk.wav \
cargo test --manifest-path native/src-tauri/Cargo.toml \
  transcribes_real_audio_offline -- --ignored
```

Use the public [`whisper.cpp` JFK sample](https://github.com/ggml-org/whisper.cpp/blob/master/samples/jfk.wav)
for that test. Models and recordings are not committed to the repository.

To test actual codec files, use FFmpeg as **test tooling only** to generate local
fixtures from the public sample, then run the opt-in format test:

```sh
python3 scripts/create-audio-fixtures.py /path/to/ffmpeg /path/to/jfk.wav /tmp/golos-formats
GOLOS_TEST_FORMATS=/tmp/golos-formats \
cargo test --manifest-path native/src-tauri/Cargo.toml decodes_real_compressed_formats -- --ignored
```

## Distribution

`npm run native:bundle` builds a macOS app bundle. For a faster local test bundle,
use `npm run native:bundle -- --debug`. The identifier is provisional and must be
confirmed before store registration. The generated icon is a placeholder.

This preview is **not App Store ready**. Signing, sandbox entitlements, StoreKit,
restore/refund handling, privacy metadata, accessibility/device testing, and
submission assets remain. The desktop app architecture is portable, but Windows,
Linux, Android, and iOS have not been built or validated. Mobile audio/background
work needs platform-specific implementation.
