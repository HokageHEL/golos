# Golos native and multiplatform development plan

For the current offline-first, all-platform direction and optional device sync,
see [the delivery plan](OFFLINE_FIRST_PLAN.md). It supersedes the sequencing below:
cross-platform validation and durable local data now precede store monetization.
The completed preview work and verification history below remain applicable.

See [product opportunities](PRODUCT_OPPORTUNITIES.md) for the earlier competitor
review; the current delivery plan above remains authoritative.

## Product direction

Private transcription on the user's device. Free download with a useful trial
and a non-consumable, one-time Pro unlock. Pricing and free-tier limits are not
final; the developer preview has no purchase gates. No ads in the first release.
Cloud services, if introduced, must have separately sustainable pricing.

## Milestone 1: macOS development preview

- [x] Native app in `native/`; retire the legacy cloud-backed web app and tooling.
- [x] Tauri/React shell with native file dialogs and constrained IPC commands.
- [x] Local SQLite history and copied audio, with deletion and crash recovery.
- [x] User-imported Whisper GGML model; real CPU inference through whisper-rs.
- [x] PCM WAV import, progress, cancellation, English/Ukrainian/auto language choice.
- [x] Editable timestamped transcript, local save, TXT/SRT export.
- [x] Native storage/audio tests and explicit opt-in inference smoke test.

## Milestone 2: usable desktop beta

- [x] Common audio formats (M4A/AAC-LC/ALAC, MP3, FLAC, Ogg/Vorbis or Opus, AIFF,
      WAV), stereo/multichannel downmixing and multiple sample rates.
- [x] Streamed import decoding, anti-alias resampling, partial-file cleanup, and
      GGML header validation across the standard Whisper family (quantized
      multilingual Small recommended; Tiny/Base/Small were the original subset).
- [ ] Robust long-recording handling and bounded-memory inference beyond the
      current 30-minute cap (inference still loads normalized f32 audio).
- [ ] Curated model downloads with pinned hashes, progress, cancellation,
      free-space checks, model metadata, and removal.
- [ ] Device benchmarks and acceleration evaluation (Metal first on Apple silicon).
- [x] Local playback, seeking, and transcript highlighting.
- [x] Transcript search, including unsaved edits, with occurrence navigation.
- [x] Persistent Light/Dark/System themes and model controls in Settings.
- [ ] Microphone recording, library-wide search, and DOCX export.
- [ ] Ukrainian UI, accessibility, keyboard operation, and native lifecycle QA.
- [ ] Complete the desktop platform rollout and acceptance checks below.

## Multiplatform desktop rollout

The current implementation and bundle target are macOS-only. Reuse the Tauri 2
shell, React/Vite interface, Rust audio/inference code, and SQLite library where
possible; isolate platform-specific file access, permissions, and lifecycle work.
These are planned acceptance criteria, not claims of current support.

1. **macOS baseline:** document minimum OS/hardware requirements; verify import,
   model replacement, transcription/cancellation, edit/save/export, deletion,
   restart recovery, and unsaved-edit protection in a packaged build.
2. **Windows beta:** configure a Windows installer, resolve native build/toolchain
   requirements, and run the same workflow on real Windows hardware. Check Unicode
   paths, file permissions, model memory use, and clean install/upgrade/uninstall.
3. **Linux evaluation:** select supported distributions and package formats, verify
   system dependencies and desktop file dialogs, then run the same workflow and
   resource checks before declaring support.
4. **Repeatable verification:** validate the added platform CI for the frontend build, Rust tests,
   formatting, Clippy, and packaging. Keep real-model smoke tests opt-in with local
   fixtures; record manual device results and known limitations for each release.

Desktop beta exit criteria: documented supported targets, reproducible packages,
passing automated checks, and recorded end-to-end validation on every advertised
platform. Microphone features also require device and permission testing.

## Milestone 3: paid Mac App Store release

- [ ] Finalize product identifier, free limits, Pro features, price, and upgrade policy.
- [ ] StoreKit permanent unlock, purchase restore/verification, refund/revocation handling.
- [ ] Preserve offline Pro access without requiring online validation per transcription.
- [ ] Apple signing, provisioning, sandbox file access and distribution entitlements.
- [ ] Test the whole workflow in the sandboxed store build, not just a dev bundle.
- [ ] Privacy policy, support site, store disclosures, screenshots, onboarding, TestFlight.
- [ ] Dependency/license review and release-specific security checks.

## Later: mobile and advanced features

- [ ] Initialize iOS/iPadOS and Android Tauri targets and validate native dependencies.
- [ ] Adapt the desktop interface for touch, small screens, and accessibility.
- [ ] Implement mobile-native file import, recording permissions, and export/share.
- [ ] Verify persistence, cancellation, and recovery across app suspension/restarts.
- [ ] Memory, thermal, battery, interruption, and background-execution tests.
- [x] Optional offline diarization pipeline with manually imported ONNX models.
- [ ] Broader diarization accuracy and device evaluation.
- [ ] Optional local summaries, with independent model/hardware requirements.
- [ ] Cross-store purchase entitlement system if one purchase covers all platforms.

Mobile release criteria: passing builds and import/transcribe/edit/export workflows
on physical devices, documented OS/device requirements and resource limits, and
platform-specific signing, store billing (if paid), privacy, and submission checks.
Choose mobile model limits from measurements; desktop limits are not a guarantee.

No public promise of Large V3 Turbo performance, unlimited-length recordings,
speaker identification, or all-platform purchase portability until validated.

## Preview verification — 2026-10-16

- Native frontend and existing Next.js production builds, repository lint, and
  web/native TypeScript checks passed.
- Existing web suite: 112 tests passed.
- Native suite: 4 storage/audio tests plus 1 real-inference test passed. The latter
  used the upstream tiny.en model and JFK sample, checked recognized words and
  timestamps, and exercised cancellation through the actual progress/abort callbacks.
- Rust Clippy passed with warnings denied.
- macOS development app bundle built and opened; the native library/setup commands
  initialized successfully. Browser empty-state layout inspected visually.
- Complete interactive import/edit/export testing in a sandboxed App Store build,
  Ukrainian accuracy benchmarks, and other OS/device validation remain outstanding.

The Whisper callback bridge intentionally uses explicit, borrowed FFI data:
whisper-rs 0.16's safe abort helper has a closure-pointer mismatch and the safe
progress helper leaks its allocation. The offline test caught the immediate abort;
the local bridge is covered by real transcription and mid-call cancellation tests.

## Expanded audio imports — 2026-10-16

- Native imports use Symphonia plus Rubato, with only the selected formats/codecs
  enabled. No FFmpeg executable or transcription model is shipped in the app.
- Actual MP3, AAC-LC/ALAC M4A, raw AAC, FLAC, Vorbis Ogg, AIFF, and 24-bit WAV
  fixtures were decoded to valid normalized audio. An Opus Ogg fixture was rejected.
- Stereo float WAVs at 8, 16, 22.05, 44.1, 48, 96 and 192 kHz preserved duration
  and speech-band amplitude; a 12 kHz test tone was suppressed during downsampling.
- 12 native tests passed, including real inference and cancellation using the
  quantized multilingual Small model. The existing 112 web tests also passed.
- Native UI build, web/native type checks, repository lint and Rust Clippy passed.
- The recommended Small Q5_1 model is about 190 MB (181 MiB), downloaded separately.
  Tiny/Base are retained for lower-resource devices; Medium/Large are rejected.
- The 30-minute cap remains. Import memory is bounded by packets/resampling blocks;
  inference still holds the normalized recording in memory, as documented above.
- Optimized macOS app bundle: **10.06 MiB**, excluding the separately imported model.
  This is an unsigned local development release, not an App Store distribution.

## Native-only repository — 2026-10-17

The legacy Next.js app, cloud integrations, database migrations, deployment
configuration, and web-only documentation/tests have been removed. Earlier web
verification records above are historical; they do not describe the current test
suite. TXT/SRT export helpers and their tests now live inside `native/src/`.

Root `dev`, `build`, and `typecheck` scripts target the native app. `npm test` runs
the retained frontend helper tests; `npm run native:test` runs the Rust suite.

## Ogg Opus imports — 2026-10-17

- The importer accepts Ogg Opus (`.ogg`, `.oga`, `.opus`) alongside Vorbis. Opus
  decodes at its native 48 kHz through the pure-Rust `rusty-opus` crate — no FFI,
  no bundled C library, and nothing installed or executed at runtime — and is
  resampled to 16 kHz by the existing Rubato path, so only the codec registry
  changed, not the streaming/resampling design. `rusty-opus` is a new dependency
  validated here only by the committed regression fixtures below, not by an
  external conformance run.
- The adapter honors the OpusHead pre-skip, Symphonia's gapless end trim, and the
  signed Q7.8 dB OpusHead output gain (RFC 7845 §5.1). The committed 2.0 s stereo
  and 1.5 s mono Ogg Opus fixtures import as exactly 2.0 s and 1.5 s of
  non-silent audio, a fixture whose ID header advertises a non-48 kHz input rate
  imports identically, and a fixture relabelled with a known -6.02 dB gain
  decodes to about half the amplitude. Those exact durations are what the tests
  demonstrate; this is not a claim of sample-accuracy for arbitrary Opus files.
- Supported Opus is limited to channel mapping family 0 (plain mono or stereo).
  Family 1 — including its one- and two-channel cases, which carry a channel
  mapping table this single-stream adapter does not implement — family 255,
  channel counts other than 1 or 2, and identification-header versions above 15
  are refused. Chained Ogg streams, empty audio packets, and corrupted or
  truncated streams fail with a clear error and remove partial output instead of
  emitting invented audio.
- The default `cargo test` run now exercises real Ogg Opus and Vorbis decoding
  using small committed tone fixtures, so it needs no external binaries and no
  network; `scripts/create-audio-fixtures.py --synthetic` rebuilds them with
  opus-tools and vorbis-tools (test tooling only). Their provenance is documented
  in `native/src-tauri/tests/fixtures/README.md`.

## Larger Whisper models — 2026-10-17

- The app-imposed model ceiling is gone. `model.rs` previously accepted only
  Tiny/Base/Small GGML files between 1 MB and 512 MiB; it now accepts the standard
  Whisper family (Tiny, Base, Small, Medium, Large, and Large V3 Turbo), including
  quantized variants such as `ggml-large-v3-turbo-q5_0.bin`, with no upper file
  size. Magic, complete-header and recognized-architecture checks remain, and
  `import_model` still copies then loads the file once before replacing a working
  model, so an incompatible or corrupt file never overwrites a usable one. The
  CPU inference path and the 30-minute/512-MiB **audio** import caps are unchanged.
- The label is derived from the real GGML header, not the file name (the stored
  model is always `model.bin`): encoder depth names the family, and within the
  32-layer encoder a 4-layer decoder identifies large-v3-turbo while 32 decoder
  layers identify large (v3 when the vocabulary is 51,866). This matches
  whisper.cpp's own header parsing and the logged header of the upstream
  large-v3-turbo model. English-only detection (`n_vocab == 51,864`) is preserved.
- Tests in `model.rs` build representative Tiny/Medium/Large/Turbo headers and
  sparse fixtures larger than 512 MiB without allocating large buffers, and cover
  bad magic, a truncated header, and an unrecognized architecture. They assert
  header acceptance and labelling only; **no real Turbo inference or benchmark was
  run**, because no such model is present in this checkout. Large-model memory,
  latency and accuracy remain unmeasured, and the roadmap's "no public promise of
  Large V3 Turbo performance" still stands.

## Playback and speaker detection — 2026-10-17

- Local audio playback, seeking, speed controls, timestamp navigation, and
  paragraph highlighting are implemented.
- Optional offline sherpa-onnx diarization uses manually imported ONNX models.
  Labels are editable drafts and do not overwrite saved content until Save.
- A native inference smoke test on the upstream four-speaker Chinese sample
  passed with int8 pyannote segmentation and NeMo small embedding. This does not
  validate Ukrainian accuracy or mobile behavior. See
  [speaker setup and evidence](../native/SPEAKER_DIARIZATION.md).
- Cancellation waits for the current native inference pass. Search, recording,
  word-level alignment, and platform/device validation remain future work.

## Feature branch integration — 2026-10-19

- Merged appearance settings and transcript search into the current native-only
  app, preserving its compact editor, speaker detection, model support, migrations,
  and per-recording audio authorization. Both model import controls live in Settings.
- Extended the existing player with ten-second skips, focused-player keyboard
  controls, timestamp playback, half-speed playback, and missing-audio retry.
- Native frontend build, lint, both TypeScript checks, 18 frontend tests, 54
  native tests, Rust Clippy, and Rust formatting passed. Three opt-in native tests
  remain ignored without their model/audio fixtures.
- Browser integration QA used Tauri's IPC mocks and synthetic audio to check
  search selection in the compact editor, unsaved edits, retained speaker fields,
  timestamp playback, and dark-mode styling. This was not a new real-model or
  signed/sandboxed release validation.
