# Golos

Private, offline audio transcription built with **Tauri 2**, React, Rust, SQLite,
and whisper.cpp. Audio and transcripts stay on your device. The current native
app is a **macOS developer preview**; multiplatform support is planned.

## What works today

- Import WAV, MP3, M4A/MP4 audio (AAC-LC or ALAC), raw AAC-LC, FLAC,
  Ogg/Vorbis, and AIFF, up to 30 minutes / 512 MiB per file.
- Transcribe locally on the CPU with an imported Whisper Tiny, Base, or Small model.
- Auto-detect the spoken language, or select English or Ukrainian.
- Keep a local SQLite library, edit timestamped transcripts, and export TXT or SRT.
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
npm run native:app
```

This launches the Tauri desktop app with the React/Vite development server.
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

Only macOS has been built and validated so far. The current bundle configuration
produces a macOS `.app`; other targets and store releases are not ready yet.

## Development plan

1. **Desktop reliability:** bounded-memory transcription beyond the current
   30-minute cap, verified model downloads, device benchmarks, and acceleration
   evaluation, starting with Metal on Apple silicon.
2. **Desktop usability:** microphone recording, playback, search, DOCX export,
   Ukrainian UI, accessibility, and keyboard support.
3. **Multiplatform desktop beta:** Windows build and installer, then Linux
   evaluation, with repeatable per-platform checks and import/edit/export testing.
4. **macOS release:** finalize product identity and free/Pro scope, implement a
   one-time purchase and restore flow, and complete signing, sandboxing, privacy
   metadata, and store validation. The preview currently has no purchase gates.
5. **Mobile:** initialize and validate iOS/iPadOS and Android targets, adapt native
   integrations and UI, and measure real-device performance before release.

See the [development roadmap](docs/NATIVE_ROADMAP.md) for detailed milestones,
release criteria, and previous verification results. Speaker separation, local
summaries, and cross-store purchase portability remain future work.

## Checks and builds

```sh
npm run native:build
npm run native:test
cargo clippy --manifest-path native/src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path native/src-tauri/Cargo.toml -- --check
npm run lint
```

Build a local macOS app bundle with `npm run native:bundle`, or add `-- --debug`
for a debug bundle. Distribution signing and store preparation remain roadmap work.
The [native guide](native/README.md#verification) covers optional tests using real
models and audio fixtures. The existing web app has separate verification steps.

## Repository map

- [`native/`](native/README.md): Tauri app, React/Vite UI, and native setup guide.
- `native/src-tauri/src/`: Rust audio decoding, Whisper inference, SQLite library,
  and native commands.
- `lib/export/` and `lib/transcript/`: transcript helpers shared with the web app.
- [`docs/NATIVE_ROADMAP.md`](docs/NATIVE_ROADMAP.md): native development and release plan.
- `app/`, `components/`, and the remaining `lib/`: existing Next.js web app.

The cloud-backed web app is preserved during migration. Its setup, service
credentials, billing, and deployment instructions are in the
[legacy web guide](docs/WEB_APP.md); they are not part of native setup.
