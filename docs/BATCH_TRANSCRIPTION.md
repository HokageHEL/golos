# Durable offline batch transcription

Golos can import up to 100 local audio files in one native file picker or
native window drop and keep up to 1,000 pending jobs in its SQLite library. Each input still has the existing
30-minute / 512-MiB source limit. Recording, storage and inference remain local;
no account, cloud service or upload is involved. Pricing remains undecided.

Open **More tools → Batch transcription** in the sidebar, choose the audio language and select
**Add files to queue**. Each successfully decoded file becomes a separate ready
recording with a queued job. Unsupported/corrupt/oversized files are reported
individually; other files continue. Source files are never modified. Cancelled
imports retain already completed imports and remove the current partial file.
A stopped native file picker can also be cancelled using its own OS controls.

Open the batch dialog first, then drop local audio files from Finder anywhere in
the Golos window. A drop is only a preview: the dialog lists sanitized file names
and nothing is read, imported or transcribed until you explicitly choose
**Add dropped files**. **Discard selection** clears the preview; dropping again
while idle replaces an unconsumed preview. Duplicate canonical paths inside one
selection are imported once, while an identical file dropped in a later
selection still creates a new recording. Drops are ignored with a visible hint
while another import, recording or transcription operation is running. Up to 100
files are accepted per drop or picker import, and the durable queue still holds
1,000 pending jobs.

You can also open a ready/failed library recording and choose **Add to batch queue**.
A model is needed only when starting the queue. Language/model incompatibilities
become actionable per-file errors at inference time. The batch language applies
to new imports; existing jobs retain the language chosen when they were queued.

**Start / resume queue** runs jobs serially with per-file transcription progress.
One failed recording does not prevent later jobs from running. Cancelling requests
an inference abort and keeps later items queued. The current item is marked
cancelled if it did not finish; **Retry** puts it back in the queue. A result that
already completed is retained if cancellation arrives just after inference.
**Remove … from queue** removes only the job, leaving its library recording and
original source intact. Speaker detection can be run on each finished transcript.
Automatic speaker detection in batch jobs is not implemented in this milestone.

The application guards normal closing while import/inference is active. After a
forced termination, previously running jobs are marked failed with a retry
instruction, while pending jobs stay queued. Manual batch processing does not restart silently;
open the queue and choose Start / resume. Explicitly opted-in automatic watched-folder
jobs can run after restart; see [folder automation](FOLDER_AUTOMATION.md). Partial decoding is repeated from the
beginning; there are no partial-inference checkpoints. Forced termination during
file publication can still leave an orphaned audio file; do not delete unknown
files automatically. Portable project backups remain the recommended backup route.

Batch jobs only accept untranscribed recordings. Completing a recording manually
before its job runs cancels the obsolete job and preserves the saved transcript.
Retries do not replace a completed or edited transcript. Deleting a recording also
removes its job. Removing jobs does not delete recordings. Queue state/results are
transactional; the transcription result and completed state commit together.
Audio normalization runs without holding the library mutex. Publication reserves
new names and uses a same-filesystem hard link, so failures do not overwrite audio.

Drag/drop uses Tauri's native `WindowEvent::DragDrop` on the main window, not the
browser `File.path` extension or a remote URL fetch. Rust captures the
user-authorized paths into one bounded, expiring, random-id grant and emits only
file names to the webview. `import_dropped_batch` consumes that grant exactly
once and never accepts arbitrary paths from JavaScript; clearing the selection,
expiry, restart or a busy drop invalidates it without consuming a selection on a
busy-claim error. Decoding still goes through the shared batch import helper, so
the picker and drop paths share normalization, per-file failure reporting,
cancellation, deduplication, source preservation and library/queue commits.

This implementation targets the macOS Finder/WebKit native event used by the
current app. Real Finder-to-WebKit drops, OS file-access prompts and packaged
sandbox behavior have **not** been validated on a physical device. No Windows,
Linux or mobile drag/drop support is claimed.

Schema version **3** adds a jobs table, pending-order index and cleanup trigger.
Existing preview/version-1/version-2 libraries receive a verified `library.pre-v3-*.sqlite3`
snapshot before the transactional migration. Version-2 organization and full-text
search objects are retained, without rebuilding or losing their content. Newer
versions are refused. Snapshots include the database, not audio.

## Validation

Rust regression tests cover actual WAV normalization/publication, cancelled import
cleanup, independent duplicate imports, failed queue insertion rolling back both
new note and file, serial job states, cancellation/retry, stale-job protection,
conflicting results, queue bounds, persistence across reopening SQLite, interrupted
recovery, deletion cleanup and the version-2 upgrade preserving search and metadata.
Drop-grant tests cover stale ids, expiry, single use, id-scoped clearing, busy
claims preserving the grant and the 100-file bound. Drop-import tests combine a
real WAV with a corrupt file, a folder, an unsupported file and a duplicate
selection; they assert partial success, unchanged sources, one queued job and
retained completed imports on cancellation.

Mounted frontend tests cover per-file reports, selected languages, native-event
updates, cancellation/close protection, retries/removal and missing-model guidance.
Native-drop DOM tests assert that a mocked `batch-drop` event only previews names,
sends no import request until **Add dropped files**, applies the selected
language, discards explicitly, keeps the current preview on an oversized or busy
rejection, replaces an older preview, and unsubscribes/invalidates on unmount.
The Chromium workflow checks batch controls, per-file failure reporting, removal
without deleting the recording, retained queue entries and minimum-window layout,
plus a preview/discard/add flow driven through an explicitly mocked native drop
event and mocked IPC. These UI checks do not establish real Finder, WebKit,
file-dialog, permission or end-to-end batch inference behavior.

The real-model offline inference/cancellation check ran in the preceding language
milestone. Native picker permission errors, signed builds, disk-full/force-termination
recovery, large queues and long-run resource behavior still need packaged macOS QA.
Watched folders and automatic multi-format local exports are implemented in the
[automation milestone](FOLDER_AUTOMATION.md). Actual Finder drag/drop on a packaged
macOS build and batch speaker options remain validation/parity work; see
[the full matrix](MACWHISPER_PARITY.md).

Validation in this isolated checkout on October 20, 2026: 133 Rust tests passed
(eight opt-in checks skipped), 67 frontend unit/DOM tests passed, and 12 Chromium
flows passed. Production frontend build, both TypeScript checks, ESLint, Rust
formatting and all-target Clippy with warnings denied passed. The Playwright
batch flow rendered the preview and imported through mocked IPC. A packaged macOS
app build and a real Finder/WebKit drag on a physical device were **not** run in
this checkout, so native permission, sandbox and platform claims are unchanged.

Integration validation on October 22, 2026: 73 frontend tests and 14 mocked-IPC
Chromium flows passed, along with ESLint and both TypeScript checks. The batch
dialog uses the shared fixed-header settings shell; a 100-file preview at
760 × 560 scrolls within its content area while the close control stays visible.
The unsigned macOS debug app bundle built successfully (87.41 MiB). This build
and browser layout check do not validate real Finder drops, OS permissions or
microphone/system-audio hardware.
Rust validation also passed: 141 tests, eight opt-in checks skipped, formatting
and all-target Clippy with warnings denied.
