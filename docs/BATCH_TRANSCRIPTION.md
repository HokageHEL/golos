# Durable offline batch transcription

Golos can import up to 100 local audio files in one native file picker and keep
up to 1,000 pending jobs in its SQLite library. Each input still has the existing
30-minute / 512-MiB source limit. Recording, storage and inference remain local;
no account, cloud service or upload is involved. Pricing remains undecided.

Open **Batch transcription** in the sidebar, choose the audio language and select
**Add files to queue**. Each successfully decoded file becomes a separate ready
recording with a queued job. Unsupported/corrupt/oversized files are reported
individually; other files continue. Source files are never modified. Cancelled
imports retain already completed imports and remove the current partial file.
A stopped native file picker can also be cancelled using its own OS controls.

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
instruction, while pending jobs stay queued. Processing does not restart silently;
open the queue and choose Start / resume. Partial decoding is repeated from the
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

Mounted frontend tests cover per-file reports, selected languages, native-event
updates, cancellation/close protection, retries/removal and missing-model guidance.
The Chromium workflow checks batch controls, per-file failure reporting, removal
without deleting the recording, retained queue entries and minimum-window layout.
These UI checks mock native IPC, native file picking and inference. They do not
establish real native file-dialog or full end-to-end batch inference behavior.

The real-model offline inference/cancellation check ran in the preceding language
milestone. Native picker permission errors, signed builds, disk-full/force-termination
recovery, large queues and long-run resource behavior still need packaged macOS QA.
Drag/drop, automatic multi-format batch export, watch folders and batch speaker
options remain subsequent parity work; see [the full matrix](MACWHISPER_PARITY.md).

Validation in this checkout on October 20, 2026: 80 Rust tests passed (four opt-in
checks skipped), 38 frontend unit/DOM tests passed, and six Chromium flows passed.
Production frontend build, both TypeScript checks, ESLint, Rust formatting and
all-target Clippy with warnings denied passed. An unsigned arm64 macOS debug app
built at about 79.1 MiB. The rendered batch dialog was inspected at 760 × 560;
collaborative-preview snapshots timed out twice, so the existing Playwright
workflow supplied the screenshot. Native hardware/permission claims are unchanged.
