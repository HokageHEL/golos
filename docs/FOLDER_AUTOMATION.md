# Offline watched folders and local export automation

Open **Folders & automation** in the sidebar. Golos watches only folders selected
through its native folder picker. No arbitrary source/output paths are accepted
from the webview. The app must be open; this is not a separate background service.
No cloud transcription, external upload, account or payment flow is involved.
The existing 30-minute / 512-MiB per-input limits remain.

## Watching local recordings

Choose the source language and optionally enable **Transcribe automatically**,
then select **Add watched folder**. By default, files already in that folder are
ignored. **Include files already in the folder** is an explicit opt-in. New or
changed files wait until at least five seconds have elapsed without a size or
modification-time change. The worker polls while idle; inference or another native
operation can delay detection. Stability is a heuristic, not a promise that an
external writer has finished. A subsequent metadata change triggers observation
again; failed stable files can also use **Retry failed imports**.

Up to ten folders, with at most 10,000 directory entries each, can be watched.
Observation is nonrecursive. Symlinks, subfolders and unsupported file extensions
are skipped. Unicode paths/names are supported. Replacing/moving a selected folder,
missing access or unavailable external volumes produces a recoverable folder/file
error. Reselect the directory or restore Files and Folders access in macOS System
Settings. Actual signed/sandboxed permission behavior still needs native QA.

Each stable input is copied to an owned local snapshot while computing SHA-256.
Size, modification time and canonical location are checked before/after copying;
normalization reads the snapshot, so an external change cannot substitute a
second version midway through decoding. Snapshot copying requires free space for
the source plus 60 MB of normalized audio. Source files are never changed/deleted.
The new audio, note, queue job and deduplication marker commit as one logical
import; a failed database transaction removes only newly created audio.

Content hashes prevent reimporting identical audio within the same watched folder,
including after restart or after a file is renamed. Deleting its library recording
does not silently reimport it. Different content becomes a new recording, preserving
previous work. Changes that preserve *both* size and modification time are not
noticed by the metadata poll. Deduplication is per folder, not global across all
selected directories.

Without automatic transcription, imported files wait in the batch queue for review
and **Start / resume queue**. With it enabled, only jobs originating from enabled,
automatic watched folders are picked up by the background worker. An installed
model is required; other pending manual jobs do not start automatically. Failed,
cancelled or interrupted jobs need explicit retry. Watch-folder jobs can resume
automatically after restart only when the saved automatic setting authorizes it.

**Pause automation** persists across restart, requests cancellation of current
background work and prevents further automatic imports/inference/exports.
Foreground manual work remains available. Pausing/removing a folder stops its
future automatic work and cancels active background work. Removing a folder keeps
its source files and all library recordings. Removing/readding a folder resets
that folder's observation history; explicitly including old files can therefore
create fresh recordings. There is no automatic deletion of source folders/files.

## Exporting locally

Select **Choose export folder**, check formats, and **Save export settings**.
TXT, SRT, VTT, Markdown, standalone HTML, CSV, structured JSON and DOCX are supported.
**Export automatically after transcription** applies to future successful manual,
batch and watched-folder transcriptions. **Export completed queue results** also
schedules saved completed batch results using the saved settings. Paused automation
keeps these tasks pending until resumed. Saved transcript data is used; unsaved
drafts are never exported automatically.

Files include corrected speaker labels and paragraph timestamps where applicable.
HTML escapes text and loads no remote content; CSV neutralizes spreadsheet formula
prefixes. Subtitle exporters remove internal blank cue terminators. Word-level
alignment, native PDF and the other output/layout gaps remain in the parity matrix.
Markdown can be written directly to a user-selected local notes/Obsidian vault;
no plugin, HTTP endpoint or external upload is involved.

Output names contain a safe title and recording ID. Existing destinations are
never overwritten, including files with matching titles/IDs. Name collisions get
numeric suffixes. Pending writes publish only after completion. The database tracks
completed formats by content revision and destination; retry skips successful
formats and resumes failed ones. A changed saved transcript creates new output
files on re-export, preserving earlier files. Removing or changing a completed
external export does not automatically rewrite it. Export failures are separate
from transcription success and use **Retry failed exports**; audio is not
transcribed again. Background status and errors are visible in Golos.

File publication and SQLite are not one filesystem transaction. A forced process
termination after publishing an output but before recording its completion may
leave an extra completed file; retry preserves it and can create another copy.
Recognized uncommitted input snapshots and normalized pending files are cleaned at
startup after the database opens successfully. Committed audio, unknown files and
symlinks are retained. A crash in the final audio-publication/DB-commit interval can
still leave an unreferenced audio file; it is not deleted automatically.

External file writes and inference run without holding the library database mutex;
folder observation reuses a prepared statement and indexed candidate lookup.
Existing recordings can be browsed, played and edited while background work runs.
Saving/mutating native content waits until the serialized operation finishes.
Normal window closing is guarded natively while work is active, and existing
unsaved-draft protection remains in the webview.

## Compatibility and validation

Schema **4** adds folder observations/deduplication, explicit local settings and
export task/completion tables. Existing preview, v1, v2 and v3 libraries receive a
verified `library.pre-v5-*.sqlite3` snapshot before transactional migration. Version-3
queue data and prior notes, organization and search are retained. No watched-folder
or output-folder access is granted by migration; automatic export defaults off.
Newer or damaged current schemas are refused before recovery writes.

Native tests perform real filesystem/WAV import, snapshot hashing, stable-file
observation, cancellation cleanup, durable deduplication/reopen, atomic marker
rollback, symlink exclusion, retry, source preservation, queue recovery and schema
migration. They write/read all eight output formats, check collision protection,
unchanged-result deduplication, changed revisions, safe HTML/CSV/subtitles and
missing-directory/cancelled/interrupted export recovery.

Mounted frontend tests cover new-file/automatic opt-ins, pause/removal, export
configuration, permission-error recovery and draft editing during native activity.
Seven Chromium flows include the automation controls, independent export retry
and screenshots at 760 × 560. Native IPC, pickers and inference are mocked in those
UI flows. Collaborative preview navigation timed out twice, so screenshots came
from the repository's Playwright workflow and were inspected.

Still required: packaged native picker/permission tests, signed sandbox/security-
scoped access across relaunch, removable/network volumes, file-copy interruptions,
slow/disk-full output, force-termination at publication boundaries, large-folder/
queue benchmarks, native WebKit/accessibility and full watch→infer→export device
runs. The existing microphone/system-audio hardware gates remain open. This work
does not establish new Windows/Linux/mobile device support or OS notifications;
queued-file notifications/status are currently in-app.

### Results in this checkout — October 20, 2026

- 94 default Rust tests passed; five opt-in checks skipped by default.
- Separately ran **watched_audio_exports_offline** with the local recommended
  Whisper Small Q5_1 model and public JFK sample: actual folder observation,
  snapshot/hash, normalized library import, native CPU inference, saved job result,
  and JSON/VTT/DOCX publication all passed without network calls. This executes
  production processing functions, not a native picker or scheduled-worker GUI run.
- 44 frontend unit/DOM tests and seven Chromium flows passed, including mocked
  folder/permission/export controls and draft editing during background activity.
- Frontend build, both TypeScript checks, ESLint, Rust formatting and all-target
  Clippy with warnings denied passed. Rendered minimum-window dialog and export
  screenshots were inspected.

Repeat the opt-in pipeline test with already-local public fixtures:

```sh
GOLOS_TEST_MODEL=/path/ggml-small-q5_1.bin GOLOS_TEST_AUDIO=/path/jfk.wav \
  cargo test --manifest-path native/src-tauri/Cargo.toml watched_audio_exports_offline -- --ignored
```

The final unsigned arm64 macOS debug `Golos.app` bundle built successfully at
approximately 80.4 MiB. This package build does not close the native permission,
volume-failure, accessibility, signed-distribution or hardware gates above.
