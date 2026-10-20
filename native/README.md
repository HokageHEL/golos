# Golos native preview

An offline desktop transcription app, developed first for macOS. Tauri hosts a
React/Vite UI; Rust owns file access, a SQLite library, and Whisper inference.
The repository root provides the shared npm scripts and frontend tooling.

## Prerequisites

- Node 22.12+ and npm.
- Stable Rust installed through [rustup](https://rustup.rs/).
- CMake and a C/C++ toolchain: whisper.cpp is compiled from source, and
  `whisper-rs` generates its bindings with `bindgen`, which needs `libclang`.
- macOS: Apple command-line tools (`xcode-select --install`). Full Xcode and an
  Apple Developer membership are needed later for iOS/store builds.
- Windows: the "Desktop development with C++" Visual Studio Build Tools workload
  (it includes CMake), LLVM/`libclang` (set `LIBCLANG_PATH` if it is not on
  `PATH`), and the WebView2 Evergreen Runtime (preinstalled on Windows 10 1803+).
- Linux (Debian/Ubuntu): `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev
  librsvg2-dev libxdo-dev libssl-dev libclang-dev libasound2-dev build-essential cmake patchelf`.

From the repository root:

```sh
npm ci
node scripts/check-native-prerequisites.mjs
npm run native:app
```

`check-native-prerequisites.mjs` uses only the Node standard library, runs
read-only version probes, and never downloads anything or touches audio/models.
It exits non-zero when a required tool is missing and prints platform-specific
install hints, so local setup and CI share the same definition of "ready".

`native:app` opens the actual Tauri app. `npm run native:dev` only opens a browser
preview: native actions are intentionally disabled there. No cloud backend, paid transcription service, accounts, or environment secrets
are needed for native use.

## Record directly in Golos

1. Choose **Record audio** (or **Record your first conversation** in an empty library).
2. Allow microphone access when the operating system asks. Golos uses the default
   system input device; choose another microphone in your system sound settings
   before recording. No transcription model is needed to record.
3. Watch the recorded-audio timer, then choose **Stop and save**. Capture stops
   automatically at 30 minutes; choose **Stop and save** to keep it in the library.
4. The saved entry opens with local playback and **Transcribe offline**. Import a
   Whisper model in Settings if needed. Audio and its eventual transcript share
   the same library entry. Failed or cancelled transcription keeps the saved audio.

**Discard…** asks before deleting an unsaved capture. Closing the window is blocked
while recording or saving. If library saving fails, the stopped capture stays
available for another **Stop and save** attempt; free disk space and retry, or
explicitly discard it. A disconnected microphone or overloaded capture queue
stops capture and reports that interruption when the captured audio is saved.
No system/meeting audio capture, pause/resume, or input-device picker is included.

On macOS, enable Golos under **System Settings → Privacy & Security → Microphone**
if access was denied, then retry (restart Golos if the system requires it). The
bundle includes `NSMicrophoneUsageDescription`. A development launch may be
attributed to its launcher; use the bundled `.app` when validating permissions.
Windows microphone access is controlled by its system privacy settings as well.

Capture uses CPAL on a dedicated native worker and a bounded audio queue. Input
at 8–96 kHz is downmixed to mono and written incrementally as 16-bit PCM; this
keeps a 30-minute source below the import size limit. On save, the existing
anti-aliased importer converts it to the library's 16 kHz WAV format and the
source is removed. Unsaved capture sources are `microphone-*.pending.wav` in
Golos's data directory. Forced termination/power loss during capture has no
recovery workflow yet and may leave an incomplete source; this is not a backup.

Verification includes synthetic capture/downmix/queue overload/limit/import
checks, discard cleanup, and failed-save retry into a playable library entry.
Browser QA with mocked native commands checks the controls; it does not validate
microphone hardware or OS permission prompts. Physical-device permission,
disconnection, and long-duration recording checks remain release gates.

## First transcription

1. Obtain a compatible whisper.cpp GGML `.bin` model from the
   [upstream model repository](https://huggingface.co/ggerganov/whisper.cpp).
   Recommended: multilingual **`ggml-small-q5_1.bin`** for English/Ukrainian,
   approximately 190 MB on disk. Tiny and Base work for faster/lighter tests.
   The standard Whisper family is accepted — Medium, Large, and Large V3 Turbo
   (for example `ggml-large-v3-turbo-q5_0.bin`), including quantized variants.
   Golos imposes no file-size ceiling of its own; the file is checked by its GGML
   header and loaded once before it replaces a working model. `.en` models support
   English only. Model working memory exceeds the file size, even with
   quantization, so larger models need more RAM.
2. Open **Settings → Transcription model** and choose **Import model**. Golos copies and loads the model before activating it.
   Importing/replacing a model can take time; the previous model is retained if
   validation fails. Automatic downloads and disk-space estimates are follow-up work.
3. Import **WAV, MP3, M4A/MP4 (AAC-LC or ALAC), raw AAC-LC, FLAC, Ogg (Vorbis
   or Opus), or AIFF**, up to **30 minutes / 512 MiB**. Mono/stereo and up to
   8 channels, 8–192 kHz rates, and supported integer/float PCM formats are
   converted automatically. No external converter or FFmpeg installation is
   needed. Ogg Opus decodes at its native 48 kHz and honors the stream's
   pre-skip, end trim, and output gain; in the committed regression fixtures the
   imported duration matches the source exactly, and Opus is limited to plain
   mono/stereo (channel mapping family 0). MP4 imports its first supported audio
   track, not video. HE-AAC, DRM, video-only files, chained Ogg streams,
   mapped/multichannel Opus, and mid-file rate/layout changes are not supported.
4. Choose auto-detection, English, or Ukrainian, then **Transcribe offline**.
5. Edit paragraphs, save locally, and export TXT or SRT through a native save dialog.

Listen to imported audio in the app with play/pause, seeking and speed controls.
Transcript paragraphs highlight as they play; click a timestamp to seek and play.
Use the ten-second skip buttons, or focus the player and use Space to play/pause
and Left/Right to skip. These shortcuts do not intercept typing in editors.
Find text with **Find in transcript**, then use the arrows or Enter / Shift+Enter
to select occurrences. Search includes unsaved edits. Missing audio has a retry
action and does not prevent transcript editing or export.
Right-click a library entry for a confirmed delete action (Shift+F10 also works).
For optional offline speaker labels, import two ONNX models from **Settings →
Offline speaker detection**. Detection runs
after new transcriptions by default; existing transcripts use **Detect speakers**. Review/correct labels, then save. See
[model setup and limitations](SPEAKER_DIARIZATION.md). Speaker detection cannot
stop mid-pass; cancellation discards the result after the current pass returns.

Inference currently uses the CPU for a portable baseline. No summaries, cloud sync, ads, or payment integration is included yet.
An imported English-only model cannot produce Ukrainian transcription.

## Appearance

Open **Settings → Appearance** to choose Light, Dark, or System. The choice is
saved on this device and applied at startup; System follows changes to the
device's color scheme. Settings also contains transcription and speaker model
import/replacement. Appearance works in the browser preview; native model and
recording actions require the desktop app.

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

`library.sqlite3` records its schema version in SQLite's `user_version` field.
Version 1 is the original preview `notes` table, so an existing library is adopted
in place instead of being recreated. A database written by a newer build is refused
rather than altered, and a `notes` table that does not match the supported schema is
refused instead of being rewritten; in both cases Golos stops at startup without
changing the file, and the message names the reason.

Only one desktop instance is allowed, and only one import/transcription/deletion
operation runs at a time. Interrupted transcription becomes retryable on startup.
Cancellation does not retain partial transcripts. Saving/discarding edits is
required before switching recordings or closing the window.

### Library backups and manual restoration

Before an existing library is upgraded for the first time, Golos writes one
consistent snapshot next to it, named after the schema version it is being taken
for:

```text
~/Library/Application Support/com.golos.transcription/library.pre-v1-<millis>.sqlite3   # macOS
%APPDATA%\com.golos.transcription\library.pre-v1-<millis>.sqlite3                       # Windows
~/.local/share/com.golos.transcription/library.pre-v1-<millis>.sqlite3                  # Linux
```

The snapshot is a single database that already includes content still sitting in the
`-wal` file, and it is checked (integrity plus row count) before the upgrade is
allowed to proceed. An existing file is never overwritten: if a name is taken, a
numeric suffix is used. Snapshots are written only while upgrading an older library,
not on ordinary startups, and Golos never deletes them — remove old files yourself
once you trust the upgrade. The snapshot contains transcripts and metadata, not
recorded audio, because audio lives in `audio/` and is not part of the database. The
app prints the snapshot path to stderr when it takes one.

Manual restoration, with Golos fully closed (quit it, and make sure no Golos process
remains — only one instance may run):

1. Optionally copy the current `library.sqlite3`, `library.sqlite3-wal` and
   `library.sqlite3-shm` aside as an extra precaution.
2. Copy the `library.pre-v1-*.sqlite3` snapshot over `library.sqlite3`, then delete
   `library.sqlite3-wal` and `library.sqlite3-shm` if they still exist. A stale
   `-wal` beside a replaced database can corrupt it; do not leave one behind.
3. Reopen Golos. It treats the restored file as an older library, takes a new
   snapshot, and upgrades it again.

If startup refuses the library, the file was not modified: either install a Golos
build that supports that schema version, or restore a snapshot as above. For support,
the stored version can be read without Golos with
`sqlite3 <path-to-library.sqlite3> "PRAGMA user_version;"`.

## Verification

```sh
node scripts/check-native-prerequisites.mjs
npm run native:build
npm run native:test
cargo clippy --manifest-path native/src-tauri/Cargo.toml -- -D warnings
cargo fmt --manifest-path native/src-tauri/Cargo.toml -- --check
npm run lint
npm run typecheck
npm test
```

The default Rust tests cover PCM decoding, stereo/float imports at common rates,
downmixing, anti-alias filtering, exact output duration, invalid/oversized audio
files, Whisper model-header validation (family, English-only and Large V3 Turbo
detection, plus sparse fixtures beyond the old 512 MiB ceiling), timestamp
validation, SQLite persistence, interrupted-job recovery,
and the library migrations: fresh creation and reopen; a preview (`user_version` 0)
fixture whose title, timestamps and transcript survive; the pre-upgrade snapshot
(including content that is only in the `-wal` file) opened as an independent
database; a newer schema refused with no content, version or journal-mode change;
malformed and near-miss `notes` tables refused without modification; snapshot names
reserved so an existing backup is never reused or overwritten; removal of a
snapshot that failed to be written; and a failing schema transaction that rolls the
version and table back through the production migration helper. An opt-in
integration test runs real Whisper inference with local fixtures, without network
access:

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

The Ogg Opus/Vorbis regression tests do not need any of that: they decode small
tone fixtures committed under `native/src-tauri/tests/fixtures/`, so a plain
`cargo test` covers the Opus path with no external binaries and no network. The
same fixtures can be rebuilt with opus-tools and vorbis-tools (again, test
tooling only); `native/src-tauri/tests/fixtures/README.md` records their exact
commands, tool versions and synthetic provenance:

```sh
python3 scripts/create-audio-fixtures.py --synthetic native/src-tauri/tests/fixtures
```

## Packaging and continuous integration

The checked-in bundle configuration targets macOS only
(`"bundle": { "targets": ["app"] }` in `native/src-tauri/tauri.conf.json`). Every
other platform must override the bundle set, otherwise `tauri build` tries to
produce a macOS-only `app` target and fails:

```sh
npm run native:bundle -- --ci --no-sign --bundles app     # macOS .app
npm run native:bundle -- --ci --no-sign --bundles nsis    # Windows installer
npm run native:bundle -- --ci --no-sign --bundles deb     # Debian/Ubuntu package
```

`--ci` skips prompts and `--no-sign` produces unsigned artifacts, so no
certificates or cloud secrets are required. Other targets (`dmg`, `msi`, `rpm`,
`appimage`) exist but are not built by default. MSI relies on WiX, which Tauri
downloads automatically (3.14) when it is not already installed.

Two GitHub Actions workflows run read-only (`permissions: contents: read`),
without cloud credentials, and never download or execute a Whisper model:

- `.github/workflows/ci.yml` — on `ubuntu-24.04`: repository lint, web
  typecheck, the existing web test suite, the native TypeScript check and build,
  then Rust `fmt --check`, Clippy with `-D warnings`, and the offline Rust tests.
  The opt-in `transcribes_real_audio_offline` test stays `#[ignore]`d.
- `.github/workflows/native-desktop.yml` — a macOS/Windows/Linux packaging
  matrix that overrides `--bundles` per OS and uploads the unsigned installers as
  artifacts. It does not cross-compile or pin an architecture: each artifact
  matches the host architecture of the runner that produced it (for example
  `macos-15`), which the workflow does not assume to be either arm64 or x64.

Both run `scripts/check-native-prerequisites.mjs` before building.

### Local evidence in this environment

The JavaScript checks, YAML parsing and Rust/native checks were all validated in
this shared checkout. The Rust toolchain was missing here at first, which is kept
below because it is why the earliest migration review could only offer
toolchain-free evidence:

- Historically absent: `cargo`, `rustc`, `rustup` and `cmake` were not on `PATH`,
  so local `cargo fmt`, `cargo clippy`, the Rust tests and any desktop bundle were
  **not** executed, and `node scripts/check-native-prerequisites.mjs` exited
  non-zero for exactly those missing tools. That was the intended report, not a
  workflow error.
- Toolchain-free evidence for the migration SQL (not a substitute for the Rust
  tests, now superseded by them): the statements were exercised against the SQLite
  C library (3.53.2 through Python, and the 3.54.0 command-line shell) — fresh
  creation and a second idempotent startup; a preview fixture with a row that
  existed only in the `-wal` file, where the snapshot kept that row while the
  upgraded database reported `user_version` 1 and the interrupted row became
  retryable; a refuse path for `user_version` 2 that left the version, the `delete`
  journal mode and the row data unchanged; and a two-column `notes` table refused
  against the seven expected columns. The replay also pinned down three behaviours
  the implementation relies on: the SQLite library accepts an existing *empty* file
  as a `VACUUM INTO` target and refuses a file that already has content (the
  `sqlite3` command-line shell adds its own existence check and would fail this
  test), a view named `notes` does **not** make `CREATE TABLE IF NOT EXISTS notes`
  fail while an index named `notes` does (the rollback test uses the index), and
  `PRAGMA table_xinfo` reports a generated column that `PRAGMA table_info` hides
  (the near-miss fixtures rely on it).

On 2026-10-17 the Rust and CMake toolchain was installed in this checkout and the
native checks were run here for real, on the same tree that carries
`native/src-tauri/src/migrations.rs`. The counts below were refreshed the same day
after the Ogg Opus import support and the larger-Whisper-model header validation
landed, so they describe the current tree:

- Present: Node 22.22.3 with npm 10.9.8; Rust/Cargo 1.99.1 through rustup 1.29.1,
  with the `clippy` and `rustfmt` components; CMake 4.4.4; and Apple clang 21.0.0
  with `libclang.dylib` from the Xcode Command Line Tools.
  `node scripts/check-native-prerequisites.mjs` now reports "All required
  prerequisites present" and exits 0.
- `cargo fmt --manifest-path native/src-tauri/Cargo.toml -- --check` is clean. It
  first reported two formatting-only diffs in `migrations.rs` (the hidden-column
  `format!` and the `sync_backup` `map_err` closures); `cargo fmt` was applied and
  the check is now clean. Behavior did not change.
- `cargo clippy --locked --manifest-path native/src-tauri/Cargo.toml -- -D warnings`
  exits 0 with no warnings, and the same command with `--all-targets` (which also
  lints the test code) also exits 0.
- `cargo test --locked --manifest-path native/src-tauri/Cargo.toml` passes:
  **39 passed; 0 failed; 2 ignored** — `audio` 2, `import` 17 executed (1 ignored),
  `library` 2, `migrations` 14, `model` 4. Both `#[ignore]`d integration tests
  (real inference and real codecs) need local fixtures and were not run. The 39
  include the committed Ogg Opus/Vorbis fixtures and the model-header tests added
  with the larger-model support (family, English-only and Large V3 Turbo labels,
  plus sparse fixtures beyond the old 512 MiB ceiling).
- `npm run lint`, `npm run typecheck`, `npm run native:build` and `npm test` (22
  files, 112 tests) all pass on the same date.
- Not executed here: desktop bundle builds, Windows/Linux builds, device runs and
  real inference. The opt-in `transcribes_real_audio_offline` and
  `decodes_real_compressed_formats` tests stayed `#[ignore]`d because their
  fixtures are absent, so no inference or codec result is claimed, and no CI result
  is recorded yet.

## Distribution

`npm run native:bundle` builds a macOS app bundle targeting macOS 13.3 or newer.
The explicit deployment target covers Whisper’s C++ filesystem and newer
Accelerate interfaces; Tauri’s default macOS 10.13 target cannot build this engine.
For a faster local test bundle,
use `npm run native:bundle -- --debug`. The identifier is provisional and must be
confirmed before store registration. The generated icon is a placeholder.

Windows MSVC builds use the full static CRT through the root `.cargo/config.toml`.
This matches sherpa-onnx's `/MT` archives and propagates to Whisper's CMake build.
`build.windows.staticVCRuntime` is false because that Tauri option adds a hybrid
VC/UCRT link override; Rust's `+crt-static` already selects the complete static
runtime. Do not override Windows `RUSTFLAGS` to select a dynamic CRT while using
these prebuilt libraries.

This preview is **not App Store ready**. Signing, sandbox entitlements, StoreKit,
restore/refund handling, privacy metadata, accessibility/device testing, and
submission assets remain. The desktop app architecture is portable, but Windows,
Linux, Android, and iOS have not been built or validated. Mobile audio/background
work needs platform-specific implementation.

See the [main README](../README.md#multiplatform-direction) for platform status
and the [development roadmap](../docs/NATIVE_ROADMAP.md) for planned milestones
and release criteria.
