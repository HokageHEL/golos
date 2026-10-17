# Golos native pivot

For the current offline-first, all-platform direction and optional device sync,
see [the delivery plan](OFFLINE_FIRST_PLAN.md). It supersedes the sequencing below:
cross-platform validation and durable local data now precede store monetization.
The completed preview work and verification history below remain applicable.

## Product direction

Private transcription on the user's device. Free download with a useful trial
and a non-consumable, one-time Pro unlock. Pricing and free-tier limits are not
final; the developer preview has no purchase gates. No ads in the first release.
Cloud services, if introduced, must have separately sustainable pricing.

## Milestone 1: macOS development preview

- [x] New `codex/tauri-local-transcription` branch; preserve the existing web app.
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
      Small/Base/Tiny model limits with quantized multilingual Small recommended.
- [ ] Robust long-recording handling and bounded-memory inference beyond the
      current 30-minute cap (inference still loads normalized f32 audio).
- [ ] Curated model downloads with pinned hashes, progress, cancellation,
      free-space checks, model metadata, and removal.
- [ ] Device benchmarks and acceleration evaluation (Metal first on Apple silicon).
- [ ] Microphone recording, playback, search, and DOCX export.
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
