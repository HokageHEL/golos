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
| Speech translation to English | Gap | Opt-in supported Whisper translation, reject incompatible models; retain source transcript/audio. |
| Translate transcript/subtitles to other languages | Gap | Local text model, explicit target language and separate translated result retaining timestamps/speakers. |
| Microphone recording, pause/resume, stop/save/discard | Implemented; hardware unvalidated | Permissions, interruptions, save failure and 30-minute device tests. |
| Application/system recording and microphone mixing | Implemented; hardware unvalidated | macOS 13.3+ system / 15+ mixed; verify common clocks, denied access, source loss, drift, headphones and actual Zoom/Teams/browser capture. |
| Automatic meeting detection and recording prompts | Gap | Opt-in supported native detection, per-app controls, start/end transitions, no recording without visible indication; false-positive and permission testing. |
| System-wide hotkey / floating Global recording | Gap | Registered configurable shortcut, overlay, always-on-top preference and focus/permission tests. |
| Dictation into another app | Gap | Offline post-record transcription and explicit insertion with Accessibility permission; preserve previous clipboard/focus, no unintended insertion. |
| Dictation prompts, app-specific modes, grammar improvement | Gap | Local processing, configurable prompts and app mapping; preview/cancel and accuracy checks. |
| Batch files and serial transcription queue | Gap | Native multi-select/drag-drop, per-file outcomes, cancellation, retry and restart recovery, no edited transcript overwrite; individual cap retained. |
| Watch folders | Gap | Explicit folder grants; stable-file detection, durable deduplication, pause/remove, per-file failures, safe output naming; never delete input. |
| Multi-track podcast transcription | Gap | Named per-speaker inputs, common offsets, overlapping speech preserved, source playback; validate different lengths/sample rates. |
| Local speaker detection / rename / paragraph assignments | Implemented | Real multi-language diarization accuracy remains unmeasured. |
| Speaker merge, saved names, colors and shortcuts | Partial | Renaming to same label can merge; add explicit merge, reusable name suggestions/colors and keyboard assignment. |
| Library history, rename, folders/tags, title/text search | Implemented | Paginated FTS queries exist; add representative large-library benchmarks. |
| Playback, speeds, timestamp seeking, paragraph highlight | Implemented | Native playback/keyboard/accessibility device QA. Word alignment remains explicitly deferred. |
| Editor undo/redo, find/replace, split/merge | Implemented | Keep timestamp validity, draft protection and speakers. More export layout customization is still needed. |
| Favorites/bookmarks / review flags | Gap | Persist paragraph bookmarks and navigable filtered view without losing edits. |
| TXT / SRT / VTT / DOCX / saved WAV / portable projects | Implemented | External Word/Pages/LibreOffice and subtitle consumer validation remains. |
| Markdown / HTML / CSV / JSON / copy transcript | Implemented in current milestone | Escaped local exports with exact paragraph timestamps, labels and Unicode; JSON does not invent word timings. HTML is printable, not a native PDF exporter. |
| PDF / DOTE / Avid timecode export | Gap | Real formats with documented schema/timebase; native PDF must render correctly without network. |
| Export layouts / timestamp offset / line wrapping | Gap | Explicit options, nonnegative ordered cues, speaker grouping and width tests; do not imply word timing. |
| Local video import and inline video/subtitles | Partial | MP4 audio decoding exists; MOV format support and retained video playback with synchronized subtitle overlay still needed. |
| Local summaries / action points / prompts / transcript chat | Gap | Local model only, explicit setup/resource needs, cancellation, separate saved output, long-context errors and groundedness evaluation. |
| Local CLI / automation | Gap | Stable documented commands, serial access to library, structured output, model/language options, nonzero failures and no overwrite by default. |
| Local Obsidian / filesystem auto-export | Gap | User-selected local destination, safe filenames, collision protection and multi-format batch export; no webhook uploads. |
| Managed deployment | Gap | Offline model provisioning and documented managed preferences for applicable local features. Licensing/pricing remains undecided. |
| YouTube / remote media URL import | Excluded | Requires online fetching; import an already-downloaded local file instead. |
| Cloud transcription, online AI, external webhooks/services | Excluded | User explicitly excluded cloud services and external uploads. No network transcript/audio requests. |
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
