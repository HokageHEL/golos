# Golos native pivot

## Product direction

Private transcription on the user's device. Free download with a useful trial
and a non-consumable, one-time Pro unlock. Pricing and free-tier limits are not
final; the developer preview has no purchase gates. No ads in the first release.
Cloud services, if introduced, must have separately sustainable pricing.

See [product opportunities](PRODUCT_OPPORTUNITIES.md) for the 2026-10-18
competitor review and proposed feature priorities. Those proposals are not
release commitments; audio-linked review and search are the recommended next slice.

## Milestone 1: macOS development preview

- [x] New `codex/tauri-local-transcription` branch; preserve the existing web app.
- [x] Tauri/React shell with native file dialogs and constrained IPC commands.
- [x] Local SQLite history and copied audio, with deletion and crash recovery.
- [x] User-imported Whisper GGML model; real CPU inference through whisper-rs.
- [x] PCM WAV import, progress, cancellation, English/Ukrainian/auto language choice.
- [x] Editable timestamped transcript, local save, TXT/SRT export.
- [x] Native storage/audio tests and explicit opt-in inference smoke test.

## Milestone 2: usable desktop beta

- [x] Common audio formats (M4A/AAC-LC/ALAC, MP3, FLAC, Ogg/Vorbis, AIFF, WAV),
      stereo/multichannel downmixing and multiple sample rates.
- [x] Streamed import decoding, anti-alias resampling, partial-file cleanup, and
      Small/Base/Tiny model limits with quantized multilingual Small recommended.
- [ ] Robust long-recording handling and bounded-memory inference beyond the
      current 30-minute cap (inference still loads normalized f32 audio).
- [ ] Curated model downloads with pinned hashes, progress, cancellation,
      free-space checks, model metadata, and removal.
- [ ] Device benchmarks and acceleration evaluation (Metal first on Apple silicon).
- [x] Local audio playback, timestamp seeking, speed/skip controls, current-segment
      highlighting, and search within the editable transcript.
- [ ] Microphone recording, library-wide search, and DOCX export.
- [ ] Ukrainian UI, accessibility, keyboard operation, and native lifecycle QA.
- [ ] Windows build/packaging and real-device tests; then Linux evaluation.

## Milestone 3: paid Mac App Store release

- [ ] Finalize product identifier, free limits, Pro features, price, and upgrade policy.
- [ ] StoreKit permanent unlock, purchase restore/verification, refund/revocation handling.
- [ ] Preserve offline Pro access without requiring online validation per transcription.
- [ ] Apple signing, provisioning, sandbox file access and distribution entitlements.
- [ ] Test the whole workflow in the sandboxed store build, not just a dev bundle.
- [ ] Privacy policy, support site, store disclosures, screenshots, onboarding, TestFlight.
- [ ] Dependency/license review and release-specific security checks.

## Later: mobile and advanced features

- [ ] iOS/iPadOS and Android builds with mobile-native file import/recording.
- [ ] Memory, thermal, battery, interruption, and background-execution tests.
- [ ] Separate on-device diarization pipeline and accuracy evaluation.
- [ ] Optional local summaries, with independent model/hardware requirements.
- [ ] Cross-store purchase entitlement system if one purchase covers all platforms.

No public promise of Large V3 Turbo performance, unlimited-length recordings,
speaker identification, or all-platform purchase portability until validated.

## Transcript review — 2026-10-18

- Added local playback of normalized audio with a constrained Tauri asset scope,
  segment timestamp seeking, playback speed, ten-second skips, and highlighting.
- Transcript search matches English/Ukrainian text and literal punctuation,
  selects individual occurrences, and includes unsaved edits.
- Playback is released when leaving a recording. Missing audio leaves editing and
  exports available and provides an explicit retry action.
- Native frontend build, repository lint, web/native TypeScript checks, 116
  JavaScript tests, 10 native tests, Rust Clippy, and formatting checks passed.
  The two opt-in codec/inference tests were not rerun for this change.
- An unsigned macOS debug bundle was tested with synthetic WAV audio and an
  isolated QA library, including timestamp playback, speed selection, match
  navigation, live edits, and recovery after missing audio was restored. This does not replace
  accessibility review or testing in a signed, sandboxed release.

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
