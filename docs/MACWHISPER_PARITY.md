# MacWhisper offline functionality parity

Audit: October 20, 2026. Reference: **MacWhisper direct-download Pro**, not just
its more limited App Store edition. Sources below are first-party documentation;
this is a documented-capability comparison, not hands-on competitor testing or a
claim that every configuration behaves identically.

The user selected offline-first local equivalents and excluded cloud services
and external uploads. Preserve the 30-minute per-file/captured-audio limit,
local storage, account-free use, and undecided pricing. Internet model setup is
optional; manual transfer remains available. Long recordings and live
transcription while recording remain outside the existing product contract.

**Full parity has not been achieved.** “Implemented” means code exists; device,
accuracy, accessibility, signing and release validation are separate gates.

| Capability | Golos status | Acceptance needed for parity |
| --- | --- | --- |
| Local audio import, normalization, offline Whisper | Implemented | Continue real-model and codec regression tests; retain 30-minute/512-MiB source cap. |
| Multiple local engines / acceleration | Gap | Evaluate equivalent accuracy/latency, local Parakeet/Apple engines or measured Whisper acceleration; no speed claim without device benchmarks. |
| Model download / manual model management | Partial | Verified Small + optional speaker downloads exist; add curated choices, removal, per-model selection and resource guidance. |
| Multilingual transcription | Implemented language picker | Enumerate bundled engine languages; reject incompatible English-only/Cantonese selections. Accuracy per language remains unmeasured. |
| Mixed languages in one recording | Partial | Auto detection is not proof of reliable code switching; validate multilingual samples and expose engine limitations. |
| Speech translation to English | Implemented; language/accuracy gates open | Separate saved Whisper translation; multilingual non-Turbo model required. Real English-fixture task smoke passed; non-English quality/hardware review remains. |
| Translate transcript/subtitles to other languages | Implemented manual local workflow; quality gates open | Optional local Ollama, explicit target language, bounded chunks, separate result with native paragraph metadata. Review translation quality; see [local AI](LOCAL_AI.md). |
| Microphone recording, pause/resume, stop/save/discard | Implemented; hardware unvalidated | Permissions, interruptions, save failure and 30-minute device tests. |
| Application/system recording and microphone mixing | Implemented; hardware unvalidated | macOS 13.3+ system / 15+ mixed; verify common clocks, denied access, source loss, drift, headphones and actual Zoom/Teams/browser capture. |
| Automatic meeting detection and recording prompts | Gap | Opt-in supported native detection, per-app controls, start/end transitions, no recording without visible indication; false-positive and permission testing. |
| System-wide hotkey / floating Global recording | Integrated microphone-only implementation; device gates open | Rust-owned opt-in toggle/hold shortcut, nonactivating button overlay, pause/save/retry and 30-minute cap. Main recorder retains system/mixed sources. Native shortcut, focus, capture, permissions and source parity remain; see [dictation contract](DICTATION_IMPLEMENTATION.md). |
| Dictation into another app | Integrated macOS adapter; device gates open | Offline post-save transcription, editable review and explicit one-shot process-targeted paste, with separate Accessibility/event permissions, fail-closed target validation and owned clipboard restoration. Real target apps/TCC/paste delivery and restore timing remain unvalidated; see [device matrix](DICTATION_DEVICE_MATRIX.md). |
| Dictation prompts, app-specific modes, grammar improvement | Integrated opt-in local cleanup/custom processing and per-app prompt overrides; quality/device gates open | Original audio/transcript and separate local AI results are retained; explicit preview/insert, no automatic paste. Native target mapping, model accuracy and end-to-end integration require validation. |
| Batch files and serial transcription queue | Implemented core workflow; partial parity | Native multi-select, per-file outcomes, cancellation, retry and restart recovery exist with stale-job protection. Multi-format local auto-export is implemented in the automation milestone. Local drag/drop is implemented through Tauri's native window event with an explicit name-only preview and a one-shot Rust grant; batch speaker options remain. Validate real Finder/WebKit drops, OS file-access prompts and packaged macOS behavior on a device. See [batch workflow](BATCH_TRANSCRIPTION.md). |
| Watch folders | Implemented; native permission/device gates open | Explicit picker grants, new-file opt-in, stable metadata checks, hashed snapshots, durable deduplication, pause/remove, per-file retry and automatic/queued modes. Nonrecursive, app-open polling; OS notifications and sandbox relaunch QA remain. See [folder automation](FOLDER_AUTOMATION.md). |
| Multi-track podcast transcription | Gap | Named per-speaker inputs, common offsets, overlapping speech preserved, source playback; validate different lengths/sample rates. |
| Local speaker detection / rename / paragraph assignments | Implemented | Real multi-language diarization accuracy remains unmeasured. |
| Speaker merge, saved names, colors and shortcuts | Partial | Renaming to same label can merge; add explicit merge, reusable name suggestions/colors and keyboard assignment. |
| Library history, rename, folders/tags, title/text search | Implemented | Paginated FTS queries validated with 10,000 synthetic recordings / 80,000 paragraphs; [database benchmark](LIBRARY_SCALE.md). Native UI, cold-cache and larger-library validation remain. |
| Playback, speeds, timestamp seeking, paragraph highlight | Implemented | Native playback/keyboard/accessibility device QA. Word alignment remains explicitly deferred. |
| Editor undo/redo, find/replace, split/merge | Implemented | Keep timestamp validity, draft protection and speakers. More export layout customization is still needed. |
| Favorites/bookmarks / review flags | Gap | Persist paragraph bookmarks and navigable filtered view without losing edits. |
| TXT / SRT / VTT / DOCX / saved WAV / portable projects | Implemented | External Word/Pages/LibreOffice and subtitle consumer validation remains. |
| Markdown / HTML / CSV / JSON / copy transcript | Implemented in current milestone | Escaped local exports with exact paragraph timestamps, labels and Unicode; JSON does not invent word timings. HTML is printable, not a native PDF exporter. |
| PDF / DOTE / Avid timecode export | Gap | Real formats with documented schema/timebase; native PDF must render correctly without network. |
| Export layouts / timestamp offset / line wrapping | Implemented for manual transcript/DOCX exports; external-app/device QA open | TXT/Markdown/HTML/DOCX can show or hide timestamps and speaker labels and combine adjacent same-speaker segments. Manual SRT/VTT exports accept a validated millisecond offset and Unicode-aware line width. Cues stay one per saved segment, ordered, non-empty and overlap-preserving, with no invented word times or cue splitting. Defaults, automatic/AI exports, CSV/JSON and backups are unchanged. See [export layouts](EXPORT_LAYOUTS.md). |
| Local video import and inline video/subtitles | Partial | MP4 audio decoding exists; MOV format support and retained video playback with synchronized subtitle overlay still needed. |
| Local summaries / action points / prompts / transcript chat | Implemented manual workflow; partial parity | Local Ollama only, explicit setup, cancellation, separate saved results and captured sources, chunked long inputs, explicit budget errors. Small-model hallucinations observed; groundedness evaluation and other local provider support remain. |
| Automatic AI cleanup / summaries after transcription | Implemented core workflow; native scheduler/quality gates open | Opt-in scoped rules, separate results, durable snapshots/deduplication, pause/retry/recovery and provenance. Includes explicit-target translation and custom prompts; no auto-overwrite or auto-export of AI results. See [automatic AI](AUTOMATIC_AI.md). |
| Local CLI / automation | Gap | Stable documented commands, serial access to library, structured output, model/language options, nonzero failures and no overwrite by default. |
| Local Obsidian / filesystem auto-export | Implemented | User-selected local destination, multi-format automatic/batch exports, safe collision handling, content-revision tracking and independent retry. Markdown can target a local vault; no HTTP/upload integration. Native slow-volume/disk-full/consumer QA remains. |
| Managed deployment | Gap | Offline model provisioning and documented managed preferences for applicable local features. Licensing/pricing remains undecided. |
| YouTube / remote media URL import | Excluded | Requires online fetching; import an already-downloaded local file instead. |
| Cloud transcription, online AI, external webhooks/services | Excluded | User explicitly excluded cloud services and external uploads. No external transcript/audio requests; optional AI sends text only to the trusted loopback service. |
| iOS app / mobile features | Separate platform work | Existing task does not establish mobile implementation or device support. |

## Delivery milestones

1. Language selection, safe structured/document exports, copy and evidence matrix.
2. Durable batch queue, stable watch folders, local multi-format export automation.
3. Local translation/AI workflows with explicit model setup and saved results.
4. Native global recording/dictation and opt-in meeting detection, then hardware QA.
5. Podcast/video workflows, richer speaker/bookmark/editor/export controls and CLI.
6. Model/acceleration evaluation, managed provisioning, full workflow/device audit.

Each milestone requires meaningful regression tests, frontend and Rust checks,
a macOS package, a local commit and reviewed GitHub checks before merging. A
passing compile or mocked UI test does not close a native permission/hardware gate.
The remaining rows must stay visible until implemented and validated; this matrix
must not be used to market “all MacWhisper features” prematurely.

## Sources

- [Official feature list](https://www.macwhisper.com/): transcription, output, video,
  app recording, dictation and automation categories.
- [Edition differences](https://docs.macwhisper.com/article/40-macwhisper-whisper-transcription-difference):
  direct-download dictation, meeting detection, CLI and managed deployment.
- [Batch workflow](https://docs.macwhisper.com/article/19-batch-transcription).
- [Watch folders](https://docs.macwhisper.com/article/35-automatically-transcribing-files-in-watch-folders).
- [Automatic meetings](https://docs.macwhisper.com/article/30-record-meetings).
- [Dictation](https://docs.macwhisper.com/article/14-how-to-use-the-dictation-feature)
  and [Global overlay](https://docs.macwhisper.com/article/16-global).
- [Speaker recognition](https://docs.macwhisper.com/article/32-automatic-speaker-recognition-in-macwhisper).
- [Local and remote AI providers](https://docs.macwhisper.com/article/58-setting-up-ai-providers-in-macwhisper).
- [Integrations](https://docs.macwhisper.com/article/53-integrating-macwhisper-with-other-services).
- [Command-line tool](https://docs.macwhisper.com/): follow the current CLI article
  from the official support index (export formats, layout, batch and model options).
- [Managed deployment](https://docs.macwhisper.com/article/27-deploying-macwhisper-with-mdm).

Do not copy competitor pricing or license restrictions into Golos. Engine/model
choice and a similar UI alone do not establish equivalent functionality or quality.

## Milestone 1 validation — October 20, 2026

- Rust: 73 default tests passed, four opt-in tests skipped; the real Whisper Small
  JFK inference/cancellation test was separately rerun and passed.
- Frontend unit/DOM tests cover escaped structured exports, CSV formula protection,
  draft copy without saving, format selection and expanded language selection.
- Five Chromium flows pass, including real browser clipboard interaction; native
  IPC remains mocked. Collaborative preview also rendered the editor and exercised
  JSON selection/export successfully.
- Native frontend production build, both TypeScript checks, ESLint, Rust formatting
  and all-target Clippy with warnings denied passed.
- Unsigned arm64 macOS debug app built, approximately 78.5 MiB. No new microphone,
  system-audio, OS-permission or native WebKit clipboard validation is claimed.

Milestone 1 merged in [PR #5](https://github.com/HokageHEL/golos/pull/5), with all four GitHub checks passing.

The durable batch milestone merged in [PR #6](https://github.com/HokageHEL/golos/pull/6), with all four GitHub checks passing. Folder automation and durable local multi-format exports merged in [PR #7](https://github.com/HokageHEL/golos/pull/7), with all four checks passing. Manual local AI and translation merged in [PR #8](https://github.com/HokageHEL/golos/pull/8), with all four checks passing; full parity remains open.

Manual local AI milestone validation: 112 Rust default tests (seven opt-in skips), 51 frontend tests, 10 mocked-native Chromium flows, separately executed real local Ollama and Whisper translation-task smoke tests, strict checks and an unsigned arm64 macOS app build. See [full evidence and limitations](LOCAL_AI.md). Full parity and native hardware validation remain open; automatic AI processing is documented in the following milestone.

Automatic local AI milestone validation: 133 Rust tests (eight opt-in skips), 63 frontend tests, 11 mocked-native Chromium flows, real Whisper-to-local-AI component integration with backup restore, collaborative preview rule creation, strict checks and an unsigned arm64 macOS app build. See [automatic AI evidence](AUTOMATIC_AI.md). Native scheduler/device and quality gates remain open.
