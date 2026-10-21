# Automatic local AI rules

Open **Folders & automation → Automatic AI rules**. Nothing runs by default:
there are no rules, and new rules start disabled. Configure a trusted local,
cloud-disabled Ollama service and save its installed model tag, port and context.
Then create a named rule, choose its task and sources, enable it, and explicitly
save the rule list. Opening settings and saving configuration do not contact
Ollama. See [local AI setup](LOCAL_AI.md) for installation and offline boundaries.

Tasks are summary, action points, cleanup, translation and custom prompts.
Cleanup preserves the source language. Translation requires an explicit target
and keeps paragraph times/speakers. Custom prompts generate separate free-form
text; they do not create timestamp-aligned subtitles. Source choices are:

- **Single recording:** the individual **Transcribe offline** command.
- **Manual batch:** a queue transcription whose recording did not originate in
  a watched folder.
- **Watched folders:** queue transcriptions of recordings imported by a watched
  folder, including when the user starts that queue manually.

Rules run only after future successful transcriptions. Saving edits, importing a
project, creating a translated recording, or enabling a rule never backfills the
existing library. A recording made with any supported capture source follows the
same workflow after the saved recording is transcribed. The existing 30-minute
limit, offline recording/transcription, local storage, no-account use and undecided
pricing remain unchanged.

## Separate results and captured settings

Each queued task captures its source paragraphs/checksum, rule kind/prompt/target,
and local model **settings** (tag, loopback port and context). Changing settings
later does not silently change pending tasks. This does not freeze the actual
model weights outside Golos: the trusted local service owns those weights.
Golos validates installed weights/capabilities before generation and pins their
digest throughout each multi-chunk run, using the same adapter as manual AI.

Rule/source/model configuration identities prevent duplicate scheduling. Renaming
a rule does not create a new semantic identity; changing its task, prompt or target
does. A changed or damaged captured source is skipped before generation. A source
edited before the task starts is skipped with a recovery message. Disabling or
deleting and saving a rule skips its queued tasks. Other edits to enabled rules
apply to future scheduling; pending tasks keep their old captured rule.

Automatic work writes separate saved AI results, never the transcript or audio.
Review results under **recording → Local AI & translation → Saved results**.
Automatic provenance includes the rule name and survives portable project backups,
including recovery of interrupted results. Use the existing manual Apply-as-draft
or Create-separate-recording controls deliberately; unsaved transcript drafts are
protected. Applying text is never automatic. Original transcript exports remain
independent; automatic AI results are not automatically exported.

## Pause, failure and recovery

**Cancel automatic AI and pause** uses global automation pause. It cancels the
running background operation and leaves queued work waiting. It also pauses
watched-folder work and local exports. Resume using **Resume automation** above.
A generic cancellation during background AI also pauses automation so the next
queued task does not immediately start. Recording and other foreground work use
the existing operation guard; no background generation runs concurrently with them.

Retry is explicit. It uses the task's captured source/rule/model settings, creates
a fresh result, and preserves previous results. The rule must still be enabled
and the saved source unchanged. If the source changed, generate manually from the
current saved transcript instead. Skip affects one queued task; Remove affects
only task metadata and never deletes saved results, transcripts or audio.

Service/model errors produce failed tasks/results. They do not rerun Whisper or
silently retry AI. Restart makes interrupted AI failed/retryable, preserves its
provenance, and retains queued tasks. A global pause survives restart. The scheduler
alternates opportunities for AI and watched-folder imports/transcription; exports
keep their existing priority. Only one background operation runs at a time.

Successful transcript commits include scheduling in a savepoint. If optional AI
scheduling fails, that savepoint rolls back and the successful transcript still
commits. The UI reports that AI needs attention; use manual local AI on the saved
transcript to recover. No successful transcription is lost due to an AI setup or
queue problem.

## Bounds, retention and migration

Up to ten rules can be saved. Names use at most 100 UTF-8 bytes, target languages
80, prompts 4,000, and source snapshots 2 MB. Context/error/output bounds match
manual AI. The pending/running queue is limited to 1,000 tasks or 64 MB of source
snapshots plus prompts, whichever comes first. Exceeding it reports a scheduling
warning and preserves the transcript; it does not omit part of a task.

The task list shows at most 100 rows, prioritizing running work and the oldest
queued tasks. The **Skip all queued automatic tasks** control also skips queued
work outside this list. Pause automation first if a background task is running;
the control requires the shared operation guard. It changes only queued task
metadata and preserves saved results, transcripts, and rules. Enabled rules
still apply to future transcriptions. Pending/running metadata is retained. The newest 100 finished task
records are retained; older finished metadata is pruned, while their saved AI
results remain in the paged recording history. Saved results/snapshots still add
disk usage and can be explicitly deleted there.

Schema **7** adds rule/task tables transactionally, with the existing verified
pre-upgrade SQLite backup. Tests migrate version 5 and both development-era
version 6 task-table shapes without changing transcripts, manual AI results/settings,
watched folders, export tasks, or batch jobs. Rebuilding preserves task order and
computes the same semantic rule hashes used for scheduling. Completed results
reconcile to completed tasks after interruption; deleting a result clears its
task link. Rules
and queued jobs are device configuration and are not imported from project files;
portable projects include generated results and provenance, without triggering
new automatic work on import. Older project formats remain importable.

## Validation and release gates

Default Rust tests cover opt-in validation/scopes, settings snapshots, semantic
deduplication, source checksums, queue budgets and scheduling rollback, source
changes/disabled rules, cancellation/failure/retry/restart, provenance, retention,
bulk-skip timestamp consistency and preservation of newer completed task history,
original-text preservation, schema-5/6 migrations, finished-result crash recovery,
and deleted-result link cleanup. DOM/browser checks use mocked
native IPC and cover opt-in saving, UTF-8 limits, required translation targets,
cleanup behavior, polling without draft clobbering, normalization, hidden prompt
removal, error recovery, task controls and global pause. Native bridge/device tests
remain separate from those browser checks.

A separately executed component integration used real multilingual Whisper Small
on the public JFK fixture, queued automatic cleanup/summary/Ukrainian translation,
ran them through local Ollama 0.40.6 with cloud disabled and Qwen2.5:1.5b, checked
paragraph metadata/original preservation, and exported/imported all three results.
It exercises native queue/generation/storage/project components; it does **not**
launch the Tauri scheduler or exercise OS permissions, actual microphone/system
capture, native dialogs or event delivery. It is functionality evidence, not a
translation/summary accuracy evaluation. Small-model hallucinations were already
observed in the manual AI milestone; generated text requires review.

Rerun with public local fixtures and a trusted cloud-disabled service:

```sh
GOLOS_TEST_MODEL=/path/to/model.bin GOLOS_TEST_AUDIO=/path/to/public-jfk.wav \
GOLOS_TEST_OLLAMA_PORT=11434 GOLOS_TEST_OLLAMA_MODEL=qwen2.5:1.5b \
  cargo test --manifest-path native/src-tauri/Cargo.toml \
  real_whisper_to_automatic_ai_and_backup -- --ignored --nocapture
```

Outstanding gates include native scheduler/event/close flows, service termination
and sleep/wake, long-library/storage pressure and low-disk behavior, multilingual
quality, VoiceOver/native WebKit, external document consumers, and real microphone,
application/system capture and permission recovery. Full offline MacWhisper parity
is still tracked in [the parity matrix](MACWHISPER_PARITY.md).

Milestone validation on October 20, 2026: **133 Rust tests passed**, eight opt-in
tests skipped by default; the real component integration above passed separately.
**63 frontend tests** and **11 Chromium flows** passed. ESLint, both TypeScript
checks, Rust formatting/all-target Clippy with warnings denied, production frontend
build and an unsigned arm64 macOS debug app build passed (86.97 MiB). The T3
collaborative preview also exercised rule creation, explicit translation target,
saving and normalization using the mocked-native fixture. This is browser UI
evidence, not native scheduler, microphone or system-audio hardware evidence.

On October 21, the 133 Rust tests, 63 frontend tests and 11 mocked-native Chromium
flows passed again after restart recovery and the bulk-skip retention fix. Earlier
packaged-app validation used a distinct application identifier and a temporary
library containing only public fixtures: manual Whisper model replacement returned
to Ready, and an explicitly enabled single-recording summary rule and loopback
model settings were saved through the native UI. The full native scheduler flow
did not complete: the audio chooser left the public WAV fixture disabled, and
subsequent UI automation lost its app connection. This is an unresolved native
validation finding, not proof of a decoder or permission failure. Foregrounding
the model picker resolved an earlier disabled-button observation; no picker or
dialog-layout workaround was retained. Real microphone/system capture, permission
recovery and native import-to-automatic-result behavior remain unvalidated.

The October 21 unsigned arm64 macOS debug build passed (87.07 MiB). In a fresh,
isolated packaged app, real guided Whisper Small download cancellation, retry,
pinned-checksum installation and cancellation with a working model installed
passed. A separate filesystem check confirmed the 190,085,487-byte installed
model matches the compiled-in SHA-256 and no pending model files remained. The
native rule/model controls saved successfully. Audio selection again remained
disabled; a pointer-based follow-up failed in the UI tool with
`noWindowsAvailable` while the app process remained alive. No import or scheduler
success is claimed from this attempt. A loopback AI stub was prepared, but no
generation request reached it; it provides no real-inference evidence.

Subsequent native watched-folder validation found and fixed a retained database
mutex during candidate selection. After the fix, the packaged Tauri scheduler
imported public audio, ran real Whisper Small, persisted the expected AI-service
absence failure, and completed a separate automatic result for a second fixture
through a labelled loopback stub. This closes that scheduler happy path and the
service-absence persistence check; it does not establish real AI inference, UI
event delivery, cancellation/fairness or hardware behavior. See the precise
[native regression evidence](FOLDER_AUTOMATION.md#native-scheduler-regression-october-21).
