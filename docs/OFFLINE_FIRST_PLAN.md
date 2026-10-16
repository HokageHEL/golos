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
- Keep existing Next.js/cloud billing and transcription flows independent. Store
  purchases, diarization, summaries, and an engine rewrite are not prerequisites
  for this product's first cross-platform beta.

## What exists, and what does not

Verified by source inspection, not newly executed device tests:

- `native/` contains a React/Vite UI and Tauri/Rust application, independent of
  the legacy Next.js frontend.
- `native/src-tauri/src/import.rs` decodes common audio formats, downmixes and
  resamples to local WAV, with cleanup and a 30-minute/512-MiB import cap.
- `engine.rs` uses whisper-rs CPU inference, progress and cancellation;
  `model.rs` accepts Tiny/Base/Small GGML models, including quantized variants.
- `library.rs` persists recordings and transcript segments in SQLite and marks
  interrupted jobs failed, allowing a manual transcription retry. There is no
  durable retry queue. The UI supports explicit save/discard and TXT/SRT
  exports. This is a working preview foundation, not a new scaffold.
- Storage has no schema version/migration system, edit revisions, outbox,
  tombstones, device identity, or native synchronization implementation.
- Inference reads the complete normalized audio into memory. Model setup is
  manual. Recording, playback, native search, and native DOCX export are absent.
- Bundle configuration currently targets a macOS app. Mobile icons and a mobile
  entry point do not establish working Android/iOS projects or device validation.
- Existing Supabase tables describe cloud transcription jobs and billing, not a
  local-first replication protocol. Reuse authentication infrastructure only
  after a native security review; do not sync local SQLite files or reuse job
  records as replicated notes.

## Architecture decisions

1. **Keep the existing engine first.** Use multilingual quantized Small as the
   desktop quality candidate and Tiny/Base as lower-resource candidates. Select
   defaults from measured English/Ukrainian quality, peak memory, latency, battery,
   and thermal behavior; do not promise Small on every phone. Benchmark alternative
   engines only if an explicit device/language target fails.
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

- Establish clean-checkout setup and CI for web/native TypeScript, existing web
  tests, Rust tests, formatting and Clippy. Keep real-inference tests explicit and
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

**Done when:** migration, crash/relaunch, disk-full, retry, duplicate operation,
delete recovery and cancellation tests pass; old libraries open unchanged in
content; a documented long-recording fixture stays within measured memory limits
without duplicated/missing chunk-boundary speech.

### P2 — Usable offline desktop beta

- Add model catalog metadata, pinned hashes, staged/atomic install, free-space
  checks, progress, cancellation and removal. Keep manual import/offline transfer.
  Do not remove the active working model before its replacement is validated.
  Exercise existing-model replacement on Windows and termination at every install
  stage, including startup rollback/repair, instead of assuming rename portability.
- Add playback, microphone recording with native permission handling, library
  search and optional DOCX export by reusing pure existing transcript utilities.
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

Start implementation with **A + B**, while **F** exposes mobile risks early.
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
No application behavior was changed. Dependencies are not installed in this
checkout and `cargo` is not on PATH, so application builds, native tests and device
benchmarks have not been rerun. Prior verification recorded in `NATIVE_ROADMAP.md`
is historical evidence, not a substitute for P0 clean-checkout validation.
