# Local AI and translation

Open a recording and choose **Local AI & translation**. Manual summaries, action
points, transcript chat, custom prompts, cleanup and text translation use an
optional local Ollama instruct model. **Speech to English (Whisper)** uses the
installed multilingual, non-Turbo Whisper model instead; it does not need Ollama.
Core recording and transcription still work without either optional AI setup or
an account. Pricing remains undecided. Audio stays limited to 30 minutes.

## Setup and offline boundary

Install [Ollama](https://ollama.com/) separately. Quit its app/service before
starting a replacement service in Terminal:

```sh
OLLAMA_NO_CLOUD=1 ollama serve
```

In another Terminal, download a local instruct model once, for example:

```sh
ollama pull qwen2.5:1.5b
```

This starter is about 0.99 GB on disk; RAM requirements depend on context and
hardware. It is a small functionality-test model, not an accuracy recommendation.
Golos does not install Ollama, pull AI models, configure a login, or enable cloud
features. Follow [Ollama's local-only instructions](https://docs.ollama.com/faq)
when using its persistent service instead. Restart after changing its settings.
Keep that service running for AI generation. In Golos, expand setup, check the
local port, refresh installed models and choose a model. Generate also saves the
displayed settings. Nothing connects to Ollama merely by opening this dialog.

Golos only addresses literal HTTP `127.0.0.1` on the configured port; proxies and
redirects are disabled. Use an Ollama service you installed and trust, configured
with cloud disabled. Before **every** generation request Golos checks local GGUF
weight metadata, completion capability, context and installed digest. Remote/cloud
model tags, unavailable weights and models with baked-in chat histories are
refused. The model digest must stay the same throughout a multi-chunk task.
Listing installed GGUF models is not a guarantee of text-generation capability;
unsupported models fail preflight before source text is sent. No cloud endpoint,
API key, external transcript/audio upload or remote media downloader is provided.
A locally modified proxy cannot be authenticated merely by its loopback address.

Once software and weights are present, generation works without internet access.
Model-list checks have a 10-second deadline; generation has a 10-minute request
deadline and a Cancel control. Native Whisper remains independent of the service.

## Saved work and editing

- Save transcript edits before generating. Source paragraphs, times and speakers
  are captured with each result. Originals are never automatically overwritten.
- Cleanup preserves the source language. Translation requires an explicit target
  language. Structured results retain native source times/speakers, regardless
  of metadata invented by the model; missing, duplicate or empty fragments fail.
- Results show a paragraph preview. **Use as transcript draft** requires an
  unchanged saved source and no unsaved edits; it produces an undoable, unsaved
  editor draft. Save it explicitly to replace the saved transcript.
- **Create separate recording** copies the captured structured result and original
  audio into a fresh library entry, preserving folder/tags. Stale results can
  still be copied deliberately; their captured text/labels do not incorporate
  later source edits. Deleting either recording does not remove the other's audio.
- Speech translation derives new paragraphs from audio. Speaker names transfer by
  greatest time overlap, which is an estimate requiring review. English-only and
  Turbo Whisper models cannot perform this translation task.
- Chat stores completed questions/answers in a conversation. New chat starts a
  separate conversation. A changed source or history exceeding 8,000 UTF-8 bytes
  requires a new chat; previous answers remain saved. References like `[s1]` seek
  to captured source paragraphs and close the dialog to reveal playback.
- Retry creates a **new** result from the previous **captured** source, with current
  model settings. To use current saved edits, select a task and Generate locally.
  Cancellation/interruption retains completed chunks and earlier results. An
  interrupted run becomes failed/retryable on startup rather than auto-running.
- Result summaries load in pages of 100; all remain stored. Each result adds a
  source snapshot plus generated text to SQLite. Delete unwanted results to reclaim
  space; deleting a recording removes its associated results.

Manual cleanup remains available. Optional [automatic local AI rules](AUTOMATIC_AI.md) now add post-transcription cleanup, summaries, translation and custom prompts as separate results. Other local providers and richer prompt preset management remain parity work. It does not implement live transcription.

## Bounds, exports and backup compatibility

Input is limited to 2 MB, with a maximum of 128 source chunks. The chosen
8,192–32,768-token context is clamped to model capabilities. Conservative UTF-8
byte budgeting reserves template, prompt/history and output capacity; no paragraphs
are silently dropped. Multi-chunk summaries combine all partial outputs, with at
most eight combining passes. Output cutoffs, malformed structured responses and
uncompressible combinations fail explicitly. Choose a larger context/model or a
smaller project to recover; larger contexts need more RAM.

Completed free-form results export TXT, stable user-facing JSON and untimed DOCX.
Structured results also export SRT, VTT, Markdown, HTML and CSV with paragraph
metadata. JSON summaries omit internal identifiers and captured source snapshots.
Generated text is rendered as plain text, not executable HTML. Copy uses the
system clipboard only when requested.

Schema **5** introduced local settings/results; schema **6** adds optional automatic rules/tasks. Upgrades remain transactional, following the existing
verified pre-upgrade SQLite backup. Older library migrations remain tested. Newer
unknown schemas are refused. `.golos` version **2** includes AI results/snapshots;
version 1 remains importable, and projects without AI stay version 1. Old builds
refuse version 2 rather than silently discarding AI work. Maximum metadata is
64 MB, with 1,000 results / 60 MB of AI payload per project. Exceeding the limits
fails export rather than omitting results. Imports validate versioned canonical source hashes,
metadata, timestamps and sizes, remap result/conversation IDs and always create a
fresh recording. Imported in-progress work is failed/retryable and never executes
on import. Model weights, service configuration and endpoint settings are not
included in project archives.

## Validation and remaining gates

Default tests cover local/remote model checks, loopback HTTP errors/redirects,
fragmented Unicode streaming, incomplete output and cancellation while waiting
for headers/body; chunk coverage, structured fragment validation, source metadata,
chat budgets, stale apply protection, retries/recovery, separate audio copies,
version-4 migration and version-1/2 backup validation are covered too.

A real isolated Ollama **0.40.6**, cloud disabled on `127.0.0.1:11439`, and
`qwen2.5:1.5b` (digest
`65ec06548149b04c096a120e4a6da9d4017ea809c91734ea5631e89f96ddc57b`)
were exercised with a synthetic public test sentence for summary, actions, chat,
cleanup and Ukrainian translation. This validates execution/structure/storage,
**not semantic accuracy**: the small model invented details in a summary. Review
facts, names, citations and translations. No accuracy parity claim is made.
The real Whisper translation flag was exercised with multilingual Small and the
public English JFK sample; that is not non-English translation-quality evidence.

Opt-in reruns:

```sh
GOLOS_TEST_OLLAMA_PORT=11434 GOLOS_TEST_OLLAMA_MODEL=qwen2.5:1.5b \
  cargo test --manifest-path native/src-tauri/Cargo.toml real_local_ai_pipeline -- --ignored --nocapture
GOLOS_TEST_MODEL=/path/to/model.bin GOLOS_TEST_AUDIO=/path/to/public-jfk.wav \
  cargo test --manifest-path native/src-tauri/Cargo.toml real_whisper_translation -- --ignored
```

Browser/DOM tests mock native IPC. They validate controls, source-language choice,
result export, cancellation/retry, draft protection and undo, not OS dialogs,
permissions, microphone/system capture or native WebKit behavior. Hardware gates
remain: real Ukrainian/other-language translation with quality review, large-context
RAM/thermal/latency benchmarks, 30-minute inference, native service interruption,
sleep/wake, native clipboard/VoiceOver, disk-full/external-volume exports, and
Word/Pages/LibreOffice interoperability. Existing microphone/system-audio permission,
synchronization/drift and Zoom/Teams/browser capture validation is still outstanding.

API references: [chat](https://docs.ollama.com/api/chat),
[installed models](https://docs.ollama.com/api/tags),
[model inspection](https://docs.ollama.com/api/show).

Milestone checks on October 20, 2026: **112 Rust default tests**, seven opt-in
skips; the two real-model smoke tests above were separately run and passed.
**51 frontend tests** and **10 Chromium flows** passed. ESLint, both TypeScript
checks, Rust formatting/all-target Clippy with warnings denied and the production
frontend build passed. An unsigned arm64 macOS debug `Golos.app` built (86.37 MiB).
Two parallel DeepSeek reviews covered backend correctness and usability/parity.
Follow-up fixes include an 8K minimum context, verbose-answer compaction before
summary reduction, paged result history, a stable versioned source hash, explicit
speech-language choice, cancellation before inference starts and clear retry
semantics. Prompt/history limits remain upper bounds, not a guarantee they fit
all model templates; context errors explain how to recover before source is sent.
