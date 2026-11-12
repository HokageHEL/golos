# Offline-first, cross-platform delivery plan

Planning baseline: 2026-10-16. This plan updates the priorities in
`NATIVE_ROADMAP.md`; that file retains the historical preview verification log.
Unchecked items here are proposed work, not implemented functionality.

## Product contract

- Target macOS, Windows, Linux, iOS/iPadOS, and Android. A Tauri shell is not
  evidence that a platform's import, inference, permissions, or packaging works.
- Import, record, transcribe, edit, search, play back, and export locally without
  an account or internet. Network failure must never block local work.
- Distinguish offline use after model installation from first launch on an
  air-gapped device. Keep manual model import and provide a documented offline
  installer/model-transfer route; downloading on first launch is not sufficient.
- Sync is an explicit opt-in convenience, not the source of truth or an inference
  service. Start with transcript/metadata sync; audio transfer is a separate,
  consented feature. Do not upload models, raw audio, logs, or transcripts silently.
- The legacy Next.js/cloud app has been retired; keep the native app independent
  of accounts and cloud infrastructure. Store purchases, summaries, and an engine rewrite are not prerequisites
  for this product's first cross-platform beta.

## What exists, and what does not

Verified by source inspection, not newly executed device tests:

- `native/` contains the React/Vite UI and Tauri/Rust application. The legacy
  Next.js frontend and its service configuration have been removed.
- `native/src-tauri/src/import.rs` decodes common audio formats (including Ogg
  Vorbis and mono/stereo Opus), downmixes and resamples to local WAV, with
  cleanup and a 30-minute/512-MiB import cap.
- `engine.rs` uses whisper-rs CPU inference, progress and cancellation;
  `model.rs` validates the GGML header and accepts the standard Whisper family
  (Tiny through Large V3 Turbo, including quantized variants) with no app-imposed
  file-size ceiling.
- `library.rs` persists recordings and transcript segments in SQLite and marks
  interrupted jobs failed, allowing a manual transcription retry. There is no
  durable retry queue. The UI supports explicit save/discard and TXT/SRT
  exports. This is a working preview foundation, not a new scaffold.
- Storage has a versioned SQLite migration path: schema version 1 is the existing
  preview `notes` table, older libraries are adopted after validation with a
  verified pre-upgrade snapshot, and newer or malformed schemas are refused
  without modifying the file. It still has no edit revisions, outbox, tombstones,
  device identity, durable job state, or native synchronization implementation.
- Inference reads the complete normalized audio into memory. Model setup supports
  manual offline import and explicit pinned Tiny/Base/Small downloads. Copy/download
  staging has progress, cancellation, free-space and validation checks; published
  generations retain the previous working model. Local playback, seeking, and paragraph highlighting are implemented.
  Optional speaker labeling uses a separate manually imported ONNX model pair.
  In-transcript search includes unsaved edits and occurrence navigation. Recording,
  library-wide search, and native DOCX export are absent.
- Bundle configuration currently targets a macOS app. Mobile icons and a mobile
  entry point do not establish working Android/iOS projects or device validation.
- No native sync transport or account integration exists. Any future service
  needs a separate local-first replication protocol; do not sync SQLite files
  or reuse historical cloud job records as replicated notes.

## Architecture decisions

1. **Keep the existing engine first.** Use multilingual quantized Small as the
   desktop quality candidate, Tiny/Base as lower-resource candidates, and
   Medium/Large/Large V3 Turbo (including quantized) as opt-in quality candidates.
   Select defaults from measured English/Ukrainian quality, peak memory, latency,
   battery, and thermal behavior; do not promise Small on every phone, and do not
   promise large-model performance, RAM, or speed. Benchmark alternative engines
   only if an explicit device/language target fails.
2. **Rust owns local durability.** Keep SQLite, model/audio files, inference and
   job scheduling behind typed IPC. Extract narrow storage/job interfaces as
   needed; do not introduce a large engine abstraction before a second engine.
3. **Version data before networking.** Add migrations, stable record/segment IDs,
   revisions, device IDs, modification metadata, tombstones, and an outbox.
   User-content mutations and their outbox events commit in one SQLite transaction.
   Keep transient inference state local rather than replicating `processing`.
4. **Sync operations, not database files.** Use authenticated, account-scoped push
   and incremental pull with server-issued cursors, idempotent operation IDs,
   expected/base revisions, and durable acknowledgments. Client timestamps are
   display metadata, not conflict ordering. Commit pulled data and cursor together.
5. **Preserve conflicting work.** Reject stale writes and retain both versions
   for explicit resolution; never silently overwrite edits. Define edit/delete,
   duplicate delivery, pagination, tombstone retention, stale-device resync, and
   sign-out/account-switch behavior before implementing a network worker.
   Start with whole-note revision conflicts and explicit conflict copies, not
   automatic per-segment merging or a CRDT. Version the wire format and define
   how older clients handle unsupported schemas without discarding local work.
6. **Privacy is a release gate.** Decide server-readable versus end-to-end
   encrypted content before finalizing the wire schema. For this private product,
   prefer E2EE, subject to a reviewed key provisioning, device enrollment,
   recovery, revocation, and metadata-leakage design. Do not label TLS-only sync
   E2EE. Keep initial sync disabled until the chosen threat model is implemented.
7. **Separate replicated content from cached audio.** A synced transcript without
   local audio remains editable/exportable; playback/retranscription must explain
   that audio is unavailable. If audio sync is later enabled, use immutable,
   integrity-checked blobs, resumable transfers, quotas and explicit download policy.

## Delivery sequence and acceptance gates

### P0 — Reproducible baseline and platform risk spikes

- Establish clean-checkout setup and CI for native TypeScript, frontend helper
  and workflow tests, Rust tests, formatting and Clippy. Keep real-inference tests explicit and
  use public fixtures plus pinned models; no personal audio in CI.
- Add macOS/Windows/Linux compile and packaging jobs, with per-platform bundles
  rather than the current macOS-only target. Verify clean installs on actual OSes.
- In parallel, initialize Android/iOS projects and run minimal import + inference
  spikes. Record minimum OS/toolchain versions and verify the actual Whisper,
  decoder and resampler dependencies compile on each mobile target. Validate
  Android document URIs and iOS document-provider access instead
  of assuming every picker result is an ordinary filesystem path.
- Create a reference-device matrix and reproducible EN/UK speech corpus with
  consent/license metadata. Record model artifact/hash, quality metric, real-time
  factor, peak RSS, cancellation latency and device/OS/build settings.

**Done when:** clean-checkout commands are reproducible; desktop CI results are
recorded; each mobile spike has device evidence or a concrete blocker; no platform
is advertised solely because it compiles. Historical test claims remain clearly
separate from fresh results.

### P1 — Durable local data and jobs

- Introduce tested versioned SQLite migrations without losing existing preview
  libraries. Add durable job state, revision metadata and transactional mutation
  helpers. Define the outbox contract now; keep network transport disabled.
  Back up before upgrades, test preview-schema and interrupted-migration fixtures,
  reject unsupported newer schemas safely, and document restoration.
- Make import/model installation/deletion recoverable across DB/file failures,
  disk-full conditions and process termination. Reconcile orphan/missing files.
  Specify staged import names, durable install/delete markers and startup repair;
  a filesystem rename and SQLite commit are not one atomic transaction.
- Add operation IDs and defined queued/running/completed/cancelled/failed states;
  cancellation must not become an unexplained permanent failure. Preserve the
  current serialized inference safety while adding a persistent job queue.
- Add bounded-memory chunked inference and checkpoint/recovery only after
  timestamp offsets, overlap deduplication, progress and cancellation are tested.
  Compare with full-pass results for EN/UK auto-detection, long silences and
  boundary speech; memory improvements must not silently regress accuracy.
  Retain the present duration cap until the longer-recording gate passes.

**P1 progress — package B, versioned migrations (implemented; Rust and JavaScript
checks pass on 2026-10-17, no device runs).**
`schema version 1` is the existing preview `notes` table. A library whose
`user_version` is 0 is adopted in place when its `notes` table matches the preview
schema exactly — the same seven columns with the same types, nullability and
defaults, and no hidden or generated column; an empty database with no `notes`
table is created fresh at version 1; an unsupported newer `user_version`, a `notes`
object that is not a table, and a `notes` table that does not match (including a
same-seven-column variant that drops a `NOT NULL` or the `segments` default) are all
refused before any mutation, so a refusal leaves the file unchanged (no journal-mode
change, no snapshot, no interrupted-job recovery). Before upgrading an existing
version-0 library, Golos reserves a snapshot name with `create_new`, so an earlier
backup can never be reused or destroyed, then writes one consistent snapshot with
`VACUUM INTO` — a single database that already includes content still in the `-wal`
file — verifies it with an integrity check and a row count, and flushes it to disk;
a snapshot that fails to write is removed. Only then do the schema and
`user_version` commit in one transaction, interrupted jobs become retryable, and a
normal startup of a current library neither migrates nor snapshots again.
`native/src-tauri/src/migrations.rs` carries tests for fresh create/reopen, a
preview fixture with transcript, title and timestamps, snapshot independence with
WAL-only content, the newer-schema refusal, malformed and near-miss schema refusals,
snapshot reservation that never reuses or rewrites an existing backup, cleanup of a
failed snapshot, and a failed schema transaction that rolls the version and table
back through the production helper; `native/README.md` documents snapshot locations
and manual restoration with the app closed. Not included in this package: durable
job state and operation IDs, revisions/outbox, and filesystem import/model recovery
(packages C, D, E, G). On 2026-10-17 the Rust and CMake toolchain was installed in
this checkout and the Rust checks were run here: `cargo fmt --check` and
`cargo clippy --locked --manifest-path native/src-tauri/Cargo.toml -- -D warnings`
are clean, and `cargo test --locked` passes with 24 passed / 0 failed / 2 ignored
(14 of them the migration tests). The two `#[ignore]`d real-inference/codec tests
were not run because their fixtures are absent, and no device or bundle result is
claimed. The earlier toolchain-free SQLite replay, which is non-substitute context
rather than the test result, is recorded in `native/README.md`.

**P1 progress — package C, audio operation recovery (2026-11-12).**
Imports now stage and flush normalized WAVs in owned operation directories before
committing the note; deletion moves audio aside before committing row removal.
Startup completes or rolls back these operations according to the row, without a
schema change or an arbitrary orphan-file sweep. Deterministic reopen/boundary,
write-failure and database-failure tests pass. Physical power loss and actual
Windows/Linux filesystem behavior remain unverified. See [local recovery](LOCAL_RECOVERY.md).
Durable job queues, revisions/outbox and bounded-memory inference remain open.

**Done when:** migration, crash/relaunch, disk-full, retry, duplicate operation,
delete recovery and cancellation tests pass; old libraries open unchanged in
content; a documented long-recording fixture stays within measured memory limits
without duplicated/missing chunk-boundary speech.

### P2 — Usable offline desktop beta

- The initial model catalog, pinned downloads, staged install, free-space checks,
  progress and cancellation are implemented, alongside manual import/offline transfer.
  Models are loaded before a unique generation is published; current and previous
  generated copies are retained. Add removal/rollback controls and exercise
  replacement on Windows and termination at install stages on actual platforms.
- Playback, timestamp navigation, transcript highlighting, and in-transcript
  search are implemented. Add microphone recording with native permission
  handling, library-wide search and optional DOCX export.
- Add responsive layout, English/Ukrainian UI, keyboard/screen-reader workflows,
  explicit job/error states and onboarding that accurately describes model setup.
- Evaluate acceleration behind a CPU fallback; adopt only measured improvements.

**Done when:** each desktop target completes install → model setup → record/import
→ transcribe → edit → relaunch → search/playback → export/delete with networking
disabled; EN/UK quality and cancellation satisfy written device-specific budgets.

### P3 — Optional transcript synchronization

- Write a small sync specification and privacy decision before transport work.
  Specify conflict UX, account attachment of pre-login local notes, deletion
  retention, lost-device recovery and logout/account separation.
- Implement the outbox/revision protocol first against a deterministic fake
  server. Test two devices, repeated/reordered delivery, crash between commit and
  acknowledgment, expired cursors, concurrent edit/delete and clock skew.
- Add separate sync schema/API with account isolation and cross-account tests;
  Supabase is a candidate, not an assumption that its existing jobs schema works.
  A client must never contain the service-role key.
- Add native sign-in, OS-backed credential storage, encryption/key lifecycle
  according to the approved threat model, backoff/jitter and foreground/reconnect
  scheduling. Handle authentication expiry without interrupting local work.
- Add opt-in settings, sync status, conflict resolution, pause, device/account
  controls and meaningful errors. Restrict any CSP/network capability changes to
  the selected endpoints. Do not place provider/admin credentials in the webview.

**Done when:** device A creates and edits offline, reconnects and synchronizes to
device B; concurrent changes preserve both versions; deletions do not resurrect;
offline edits survive auth expiry, app restart and partial requests; account B
cannot read A's data. Sign-out never silently erases or uploads local recordings.

### P4 — Mobile completion and release hardening

- Build on P0's spikes: mobile import/export adapters, recording permissions,
  touch/safe-area layout, secure storage, lifecycle/interruption handling and
  measured per-device model defaults. Do not assume indefinite background compute.
- Validate iOS/iPadOS and Android in airplane mode on physical devices, including
  low-memory/thermal stress, phone-call interruption and force-stop/relaunch.
- Ship signed/notarized or store-packaged artifacts as appropriate, with upgrade
  migration tests, dependency/model-license review, privacy disclosures and
  support/recovery documentation. Validate packaging on all target platforms.

**Done when:** the same offline and two-device sync scenarios pass on the declared
support matrix. Publish actual supported devices/limits, not an all-hardware claim.

P0 mobile risk discovery runs early; full mobile delivery follows a stable shared
core. P3 protocol work can run alongside P2 once P1's persistence contract is stable.
Store payments and optional audio sync follow these gates, rather than blocking them.

## Small-model work packages

Use smaller-model agents for narrow, independently testable tasks. Keep shared
schema/protocol decisions and final integration/review with the coordinating agent.

| Package | Scope | Depends on | Required evidence |
| --- | --- | --- | --- |
| A | CI/bootstrap and desktop build matrix | Baseline | Clean-checkout logs/artifacts |
| B | SQLite migrations and migration tests | Storage design | Existing-library fixtures preserved |
| C | File/DB crash recovery | B | Fault-injection tests |
| D | Model catalog and safe installation | C | Hash/cancel/disk-full/replacement tests |
| E | Playback/recording and permission UX | Local job contract | Native workflow and refusal/retry tests |
| F | Android/iOS feasibility spikes | Baseline; parallel with A | Device logs and resource measurements |
| G | Revision/outbox/conflict simulator | B; approved sync spec | Deterministic two-device tests |
| H | Authenticated sync transport | G; privacy/key design | Isolation, reconnect and replay tests |
| I | Localization/accessibility/mobile layout | Stable IPC | Keyboard, screen-reader and touch checks |

The initial **A + B** work added desktop CI/packaging configuration and versioned
migrations; successful remote CI and real-device runs remain separate acceptance
evidence. Next, finish the current UI, then implement **C** and safe model setup
from **D**, while **F** continues to expose mobile risks.
Avoid simultaneous edits to shared IPC/schema files; each package must include
tests and a bounded review. Do not assign agents a vague “implement all sync” task.

## Delegated review outcome

Two read-only reviews completed with `gpt-5.6-luna`: native/platform feasibility
and local-first synchronization. Initial `claude-haiku-4-5` attempts were cancelled
after provider rate limiting. Verified risks incorporated above include:

- Import writes audio before inserting its database row (`lib.rs:93`); deletion
  removes audio before deleting the row (`lib.rs:233`). Process termination can
  bypass ordinary error cleanup, so P1 needs explicit recovery protocols.
- Edits currently replace transcript JSON without revisions (`lib.rs:212`), and
  startup only changes interrupted job status (`library.rs:25`). These are not
  sufficient conflict or durable-job contracts for device synchronization.
- Whole-file inference (`engine.rs:39`), staged model replacement (`lib.rs:140`),
  and shared native dependencies (`Cargo.toml:14`) require measured memory,
  replacement-failure and mobile-build gates rather than portability assumptions.

References in this section are under `native/src-tauri/` and reflect this audit
baseline. Reviews were checked against source: a separate retry IPC command is
not currently necessary because `transcribe` already accepts failed notes; no
claim is made that replacing an existing model necessarily fails on Windows.

## Validation status for this planning pass

This document is based on repository inspection and delegated read-only audits.
The planning pass itself changed no application behavior; the package-B progress
note under P1 records the later migration implementation and how far its evidence
goes. On 2026-10-17 in this shared checkout, after `npm ci`, `npm run lint`,
`npm run typecheck`, `npm run native:build` and `npm test` (22 files, 112 tests) all
pass, now together with Rust `cargo fmt --check`, `cargo clippy --locked
--manifest-path native/src-tauri/Cargo.toml -- -D warnings` and `cargo test
--locked --manifest-path native/src-tauri/Cargo.toml` (24 passed, 0 failed, 2
ignored) after the Rust and CMake toolchain was installed here. Device benchmarks,
real inference (its fixtures are absent) and desktop bundle builds still have not
been run, and no CI result is recorded. Prior verification recorded in
`NATIVE_ROADMAP.md` is historical evidence, not a substitute for P0 clean-checkout
validation.

### Current implementation verification — 2026-11-12

The UI/settings, audio recovery, and initial model setup packages now pass the
local build/typecheck/lint checks, 36 frontend tests, 75 Rust tests, formatting
and all-target Clippy. An unsigned macOS debug bundle built successfully. The
explicit Tiny download test verified a real pinned artifact and Whisper loading
in temporary storage; the four opt-in Rust tests remain excluded from the default
suite. This is not a new transcription-quality, interactive device, Windows/Linux,
mobile, signed-release or remote-CI result. The full evidence and remaining limits
are recorded in [the native roadmap](NATIVE_ROADMAP.md#settings-recovery-and-model-setup--2026-11-12).
