# Golos usability improvements — October 2026

Golos remains a private macOS developer preview. Transcription, audio, transcripts,
organization and preferences remain local. No account is required. The audio limit
is still 30 minutes. Pricing is undecided; this work introduces no payment flow.
Live transcription and word-level alignment remain future work.

## Guided model setup

Onboarding and Settings offer **Download recommended model**: multilingual
Whisper Small, quantized GGML Q5_1, compatible with the existing whisper.cpp engine.
The download is 190,085,487 bytes; setup requires 200 MB free space in addition to
existing files. Model RAM requirements exceed its file size. Manual imports remain.

Optional speaker setup downloads both pyannote int8 segmentation and NeMo TitaNet
small embedding models (47,215,727 bytes combined; 120 MB free space required).
Speaker models are independent of the Whisper model. Manual pair import remains.

Only these compiled-in HTTPS upstream URLs can be fetched:
- [whisper.cpp model repository](https://huggingface.co/ggerganov/whisper.cpp)
- [sherpa-onnx segmentation release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-segmentation-models)
- [sherpa-onnx embedding release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models)

Pinned SHA-256 values and exact sizes are in `native/src-tauri/src/download.rs`.
Whisper's hash is its upstream LFS SHA-256. Speaker hashes were computed from the
official release artifacts on October 20, 2026. Updates to these pins require
review; there is no automatic acceptance of a changed upstream file.
Progress reports downloaded bytes, followed by model validation. Cancel and retry
are available; retry starts a fresh download. Cancellation during a native model
load takes effect after validation returns. An in-flight network read may take up
to its timeout to return. No partial file becomes active. Whisper publishes by
same-filesystem rename; speaker models publish as an immutable complete generation.
Errors/cancellation preserve existing models. No audio is sent with model requests.

After installation, disconnect the network: recording, playback, transcription,
speaker detection, editing and exports use local files and need no account.
Models are not included in project backups. Guided speaker installations retain the segmentation archive’s MIT license alongside the active model pair. Speaker weight licensing is documented
in `native/SPEAKER_DIARIZATION.md`; downloaded weights are not bundled for redistribution.

## Recording

Choose a source before recording:
- **Microphone** uses the existing native CPAL path and default input device.
- **Application / system audio** uses Apple's ScreenCaptureKit on macOS 13+ (the
  app itself requires macOS 13.3+). Choose all system audio or refresh and select a
  running application, such as a browser, Zoom or Teams.
- **Microphone + application audio** uses ScreenCaptureKit microphone output on
  macOS 15+. It is unavailable on earlier systems.

Golos's Swift helper is compiled with the app and embedded in its native executable.
It is materialized as `golos-audio-capture` in the local app-data directory.
Permissions may identify it as **Golos Audio Capture**. Allow it in **System Settings
→ Privacy & Security → Screen & System Audio Recording**. Mixed mode also needs
**Microphone**. After denial, change the setting, restart Golos and refresh sources.
macOS 13 may call the category **Screen Recording**. No screen video is registered,
recorded or saved. The required ScreenCaptureKit display filter does require an
available display. Protected content and applications that prevent audio capture
may not produce audio. An application that closes must be refreshed/reselected.

All sources offer Pause, Resume, Stop and save, and confirmed Discard. The 30-minute
limit applies to captured time, excluding pauses. Resumed sections form one file
without inserted pause silence. Stopped and saving states are separate. Failed
library saves retain the stopped source and allow retry. Closing remains guarded.

For system/mixed capture, AVAudioConverter converts each source's native format to
16 kHz mono. Host-clock sample timestamps place packets on one timeline with a
500 ms buffering window. A bounded two-second buffer rejects clock jumps; late
new samples stop capture rather than silently desynchronizing. Duplicate callbacks
for a source are counted once. Mixed inputs each have half gain, and output clips
safely to PCM16. Pause intervals are subtracted from the common host timeline.
Golos's process and helper audio are excluded to prevent playback feedback.
Use headphones: physical speaker sound can still be picked up by a microphone;
this implementation does not provide acoustic echo cancellation. No live transcript
is generated. Forced termination can still leave an incomplete pending recording.

This source-capture feature is macOS-only. Packaging jobs on other operating
systems do not establish Windows, Linux or mobile capture support.

## Library and editing

Rename and organize a recording from **Rename & organize**. Folders are local
folder names; tags are comma-separated labels. Filters use exact folder/tag names.
Status filters distinguish ready, transcribed, processing and retry-needed entries.
Search uses Unicode-aware word-prefix matching across titles and **saved transcript
text**. Multiple words must all match; speaker labels are not searched.

SQLite FTS5 maintains the index through insert/edit/rename/delete triggers.
Search is debounced and runs on a blocking worker, away from the webview thread.
Library queries return 100 summaries at a time; **Load more** fetches the next page.
Only the opened transcript is loaded in full. Search and navigation retain unsaved
drafts in memory, including drafts in other recordings. Open those recordings to
save/discard before closing. The app blocks normal closing while drafts remain.
This is navigation protection, not recovery after power loss or process termination.

Undo/redo retains up to 100 edit steps for the open recording. Find-and-replace is
literal and case sensitive; the existing find navigation also remains. Use
Command/Ctrl+Enter to split at the cursor, or the paragraph's split-in-half action.
Split timestamps interpolate within the original paragraph bounds; they are valid
paragraph estimates, not word alignment. Merge with next requires matching speaker
labels and retains the outer timestamp bounds. Paragraph playback highlighting is
preserved.

Exports, find-and-replace and speaker detection live in sections that open in the
normal document flow; see `docs/UI_LAYOUT.md`. Closing a section keeps its
contents, so a search query, a replacement text and a speaker count survive
reopening it. A collapsed section keeps its state in the summary line: the
applied filters and the number of matches stay visible. Escape inside a field
belongs to that field, so clearing the search query does not close the section. Recording, importing, playback, editing and saving stay visible.

## Exports and portable backups

TXT and SRT remain. VTT exports escape markup and use valid cue timestamps. DOCX
exports include the title, paragraph timestamps, speaker labels and Unicode text.
**Export audio** saves the library's normalized WAV. **Export project backup**
writes a `.golos` ZIP containing `project.json` (versioned manifest) and `audio.wav`.
It contains saved title, transcript, speaker labels, date, folder and tags.

Imports enforce member names/count, compressed and expanded size limits, version,
metadata limits, transcript timestamp bounds, WAV format and actual audio duration.
Audio is read and validated before publication. Every import gets a fresh UUID;
existing entries and files are never overwritten. Failure removes only the new
import's files and rolls back its database transaction. Project backups are per
recording; repeat for other recordings. They do not contain model weights, unsaved
drafts or appearance preferences. Backups are ordinary unencrypted local files.

Schema version 2 adds organization and the full-text index while keeping the
existing notes schema. Preview and version-1 libraries receive a verified
`library.pre-v2-*.sqlite3` snapshot before migration. Migration changes are
transactional and preserve titles, dates, text, timestamps and speaker labels.
Snapshots contain the database, not audio; portable projects include the audio.
Newer schema versions are refused without mutation.

## Validation and release gates

Automated frontend checks include text editing/history, split/merge, VTT escaping,
draft retention across search and navigation, saving restored drafts, source
selection, pause/resume controls and discard confirmation. They also cover
keyboard handling inside sections, focus returning to the control that opened a
dialog, the state a collapsed section shows, and the page regions that stay
mounted while their data loads. These tests mock native commands and **do not
exercise recording hardware or OS permissions**.

Rust tests cover streamed download hashes/sizes/cancellation/partial cleanup,
preserving the working model after validation failure/cancellation, speaker-pair
replacement, sample-based microphone pause/limit behavior, timestamp mixing and
duplicate callbacks, clock/source rejection, FTS search and pagination, migration
snapshots/rollback, project round trips/invalid inputs, DOCX packaging, and the
existing codecs, playback authorization and save retries.

Opt-in checks:
```sh
cargo test --manifest-path native/src-tauri/Cargo.toml downloads_pinned_upstream_artifacts -- --ignored
GOLOS_TEST_MODEL=/path/ggml-small-q5_1.bin GOLOS_TEST_AUDIO=/path/jfk.wav \
  cargo test --manifest-path native/src-tauri/Cargo.toml transcribes_real_audio_offline -- --ignored
```

The developer-only browser fixture is `/tests/ui.html` on `npm run native:dev`.
It uses Tauri's mock IPC and is not a production build entry. Thirteen real
Chromium browser tests exercise navigation/draft retention, editing/export
controls, rename/organization at the minimum window size, model-download error
recovery, mixed-source recording controls, and the workspace action sections and
their geometry at 760×560 and 1180×780. Rendered screenshots were inspected.
This caught and fixed sidebar overflow from long source names; model errors are
also kept in view. These browser checks still use mocked native commands.

Run them with `npx playwright install chromium` followed by `npm run test:ui`.
The test runner starts a separate local Vite server on a per-checkout port,
derived from the checkout path so parallel worktrees never share a port, and
writes screenshots/results to the operating system's temporary directory under
`golos-usability-browser/`. Set `GOLOS_UI_PORT` to run a checkout on its own
loopback port; an explicitly requested port is never reused, so a suite cannot
silently run against another checkout's dev server. CI installs Chromium and runs
the same suite. T3's collaborative preview timed out, so this verification used
standalone headless Chromium instead. Microphone, system audio, native dialogs and
inference are not exercised by browser tests.

Still required on physical macOS devices before release:
- Fresh allow/deny/recovery prompts for both microphone and system audio, including
  helper permission attribution across upgrades and signed distribution.
- Record/save/play/transcribe each source with the network disconnected after setup.
- Browser, Zoom and Teams capture, app exit, display changes and protected-content behavior.
- 44.1/48/96 kHz devices, Bluetooth and USB microphones, source-clock drift, clipping,
  headphones, echo and disconnection behavior.
- Pause/resume repeatedly, stop/save while paused, failed-save retry, and a full
  30-minute run excluding substantial pauses.
- Actual Word/Pages/LibreOffice rendering and VTT playback in external consumers.
- Native WebKit and keyboard/accessibility QA beyond the mocked Chromium flows, large-library benchmarks and signed packaging.

Actual microphone/system-audio capture has not been claimed as tested here.

### Results recorded in this checkout on October 20, 2026

- Native frontend build, both TypeScript checks, ESLint, Rust formatting and
  Clippy across all targets with warnings denied passed.
- Frontend: 28 unit/DOM tests passed, including four mounted UI interaction regressions; four Chromium browser tests also passed with mocked native IPC and screenshot inspection.
- Default Rust suite: 72 passed, four opt-in checks skipped.
- Separately executed: real upstream download/hash verification for all three
  artifacts and native speaker-model validation; real Whisper Small CPU inference
  on the public JFK sample, including inference cancellation. Both opt-in tests passed.
- An unsigned arm64 macOS debug Golos.app was built (about 78 MiB).
- Native helper capability probing reports system and mixed support on the build
  host. This is capability detection, not actual sound capture.
- Real speaker diarization on a recording and external codec integration tests
  were not rerun. Microphone/system-audio hardware and OS permission flows were
  not exercised. The physical-device, native WebKit/accessibility and external-consumer gates above remain open.
