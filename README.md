# Golos

Private, offline audio transcription built with **Tauri 2**, React, Rust, SQLite,
and whisper.cpp. Audio and transcripts stay on your device. The current native
app is a **macOS developer preview**; multiplatform support is planned.

## What works today

- Import WAV, MP3, M4A/MP4 audio (AAC-LC or ALAC), raw AAC-LC, FLAC,
  Ogg/Vorbis or Opus, and AIFF, up to 30 minutes / 512 MiB per file.
- Transcribe locally on the CPU with an imported Whisper GGML model,
  including Tiny, Base, Small, Medium, Large, and Large V3 Turbo.
- Auto-detect the spoken language, or select English or Ukrainian.
- Keep a local SQLite library, edit timestamped transcripts, and export TXT or SRT.
- Play local audio, seek from timestamps, and follow highlighted transcript paragraphs.
- Detect speakers offline with optional imported ONNX models and edit speaker labels.
- Cancel transcription, retry interrupted jobs, and delete local library entries.

No account, backend, API keys, Docker, or paid transcription service is needed
for the native app. Models are imported separately. Local storage is not encrypted
by the app; see [storage and privacy](native/README.md#storage-and-privacy).

## Start developing

The current setup targets macOS. Install Node.js **22.12+**, npm, stable Rust,
Apple command-line tools (`xcode-select --install`), and CMake.

From the repository root:

```sh
npm ci
npm run dev
```

`npm run dev` (also available as `npm run native:app`) launches the Tauri
desktop app with the React/Vite development server.
`npm run native:dev` runs only the browser UI preview; native file access,
transcription, and saving require the Tauri app.

To transcribe, import a compatible whisper.cpp GGML `.bin` model, then import an
audio file and choose **Transcribe offline**. The current default recommendation
is multilingual `ggml-small-q5_1.bin` (about 190 MB); `.en` models support English
only. See the [native guide](native/README.md#first-transcription) for model sources,
codec limitations, storage details, and real-inference test instructions.

## Multiplatform direction

Tauri provides the application shell, React/Vite the interface, and Rust the local
audio, inference, and storage layer. The development plan keeps this core shared
while adapting file access, recording, lifecycle behavior, and distribution for
each platform.

- **macOS:** current development preview; desktop beta and signed distribution next.
- **Windows:** planned desktop build, packaging, and device validation.
- **Linux:** planned evaluation after Windows, including packaging and audio testing.
- **iOS/iPadOS and Android:** later milestones requiring mobile file import,
  recording, responsive UI, and memory, thermal, and lifecycle validation.

The default bundle configuration produces a macOS `.app`. CI packaging targets
macOS, Windows, and Linux; builds do not establish real-device support. Mobile
feasibility probes and an optional iOS simulator build are documented separately
in the [mobile feasibility report](docs/MOBILE_FEASIBILITY.md). Store releases
and full platform/device validation remain outstanding.

## Development plan

The [offline-first delivery plan](docs/OFFLINE_FIRST_PLAN.md) defines the current
priorities: durable local data, desktop validation, mobile feasibility, and optional
device sync before store monetization. The core transcription workflow must remain
usable offline without an account.

Next work includes bounded-memory inference beyond the 30-minute cap, verified
model downloads, device benchmarks and acceleration evaluation, microphone
recording, search, DOCX export, Ukrainian UI, and accessibility. Playback and
optional offline speaker detection are already implemented; see the
[speaker guide](native/SPEAKER_DIARIZATION.md) for setup and validation limits.

See the [native roadmap](docs/NATIVE_ROADMAP.md) for detailed release criteria and
verification history. Local summaries and cross-store purchase portability remain
future work.

## Checks and builds

```sh
npm run build
npm run typecheck
npm test
npm run native:test
cargo clippy --manifest-path native/src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path native/src-tauri/Cargo.toml -- --check
npm run lint
```

Build a local macOS app bundle with `npm run native:bundle`, or add `-- --debug`
for a debug bundle. Distribution signing and store preparation remain roadmap work.
The [native guide](native/README.md#verification) covers optional tests using real
models and audio fixtures.

## Repository map

- [`native/`](native/README.md): Tauri app, React/Vite UI, and native setup guide.
- `native/src-tauri/src/`: Rust audio decoding, Whisper inference, SQLite library,
  and native commands.
- `native/src/export/` and `native/src/transcript/`: local transcript helpers and tests.
- [`docs/NATIVE_ROADMAP.md`](docs/NATIVE_ROADMAP.md): native development and release plan.
- `scripts/create-audio-fixtures.py`: optional codec test fixture generator.

The repository contains only the native app. The former cloud-backed web app and
its service configuration have been removed; earlier versions remain in Git history.
