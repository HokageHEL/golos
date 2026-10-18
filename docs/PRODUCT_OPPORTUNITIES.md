# Golos product opportunities

Research date: 2026-10-18. Recommendations for the offline native app, following
the direction in [NATIVE_ROADMAP.md](NATIVE_ROADMAP.md).

Implementation update: audio-linked playback and transcript search from this
proposal are now implemented. See [native usage](../native/README.md) and the
[roadmap verification record](NATIVE_ROADMAP.md). The baseline below records
the app as inspected before those additions.

## Recommendation

Build a private workspace for turning interviews and lectures into reviewed,
usable text. Prioritize listening, correcting, finding, and exporting before
adding a general AI assistant. Make Ukrainian/English workflows a focus, with
measured accuracy rather than a claim that language support alone is unique.

This positioning is a hypothesis to validate with users. The comparison below
uses public first-party documentation, not hands-on competitor testing. Features
can vary by platform, edition, model, and configuration. Proposed differentiators
are opportunities, not claims that no competing product offers them.

## Baseline inspected for this review

Verified against `native/src/App.tsx`, `native/src/api.ts`, the Rust commands,
and `native/README.md`:

- Local file import, normalization, SQLite library, deletion, and recovery of
  interrupted jobs.
- CPU Whisper transcription using a manually imported Tiny/Base/Small model.
- Auto/English/Ukrainian language selection, progress, and cancellation.
- Timestamped paragraph editing with explicit save/discard and TXT/SRT export.
- No account or transcription service required.

Native limits include 30 minutes / 512 MiB per source, one operation at a time,
an English-only interface, no audio player, search, recording, or speaker separation.
Native storage is not encrypted by the app. The existing web app has speaker
labels, Ukrainian UI, and DOCX export; those are not native features yet.

## Similar apps and what they establish

### MacWhisper: a mature transcription workflow

Its official support documentation covers [batch transcription](https://docs.macwhisper.com/article/19-batch-transcription),
[automatic meeting recording](https://docs.macwhisper.com/article/30-record-meetings),
and [speaker recognition](https://docs.macwhisper.com/article/32-automatic-speaker-recognition-in-macwhisper).
These are reference workflows for repeat users. Golos currently lacks all three
in native. Evaluate speaker recognition's engine and hardware requirements before
assuming feature parity or fully offline behavior.

### Buzz: offline capability is already widely available

The [official repository](https://github.com/chidiwilliams/buzz#features) lists
microphone transcription, speaker identification, GPU backends, search, playback
and speed controls, keyboard shortcuts, watch folders, and TXT/SRT/VTT export.
Its main-branch documentation may describe work ahead of a packaged release.
For Golos, the implication is that offline processing alone is insufficient
positioning; the correction workflow and setup experience need to be excellent.

### Aiko: simplicity is a serious competitor

[Aiko's developer page](https://sindresorhus.com/aiko) describes on-device
transcription, Ukrainian among its supported languages, and Shortcuts integration.
Its FAQ explicitly says in-app text editing is not planned; batch processing is
available through Shortcuts rather than built in. Golos already has an editing
foundation and could develop that advantage into a complete review workflow.

### Superwhisper: dictation is a separate product direction

The [official product page](https://superwhisper.com/) advertises offline support,
custom vocabulary, formatting modes, keyboard shortcuts, and inserting text into
other apps. It also offers online models; the landing page does not establish
that every advertised feature operates offline. System-wide dictation would
require substantial new work in Golos and should follow validation that our
users want it more than better recording review.

## Missing essentials, in recommended order

1. **Audio-linked transcript review.** Play/pause, seek by paragraph timestamp,
   current-paragraph highlighting, speed control, and keyboard navigation. Keep
   editing usable while listening. This closes the loop between generated text
   and checking what was actually said. Initial synchronization should be at
   segment level; current timestamps do not establish word-level alignment.
2. **Simple model setup.** Recommend a supported multilingual model and offer a
   download with size, progress, cancellation, free-space checks, and pinned hash
   verification. Retain manual import. After setup, transcription must work
   without network access. Keep working models intact if replacement fails.
3. **Find and organize.** Search within a transcript and across saved recordings,
   rename recordings, and filter by status. Add tags only if a growing library
   makes them useful. Larger libraries should use indexed, paginated queries
   rather than loading every transcript into the sidebar.
4. **Longer recordings and a persistent queue.** Bounded-memory inference,
   checkpoints, restart recovery, and serial batch processing. Validate timestamp
   continuity and duplicate/missing text at chunk boundaries before raising the
   30-minute cap. Benchmark acceleration separately rather than promising speed.
5. **Recording and useful output.** Microphone recording first, then DOCX/VTT and
   copy transcript. System audio and automatic meeting detection come later
   because they add platform-specific permissions and lifecycle work.
6. **Ukrainian interface and accessibility.** Translate the native workflow,
   provide keyboard access and focus states, and test with VoiceOver. Existing
   web translations are a starting point, not a complete native translation.

## Opportunities beyond feature parity

### Review queue for questionable passages

Let users mark a passage as needing review, replay it, and resolve it. Later,
evaluate machine-generated review suggestions for silence, repetitive output,
or uncertain recognition. Do not present raw Whisper scores as calibrated
probabilities of correctness. Start with manual flags so the workflow is useful
without another model.

### Bookmarks and quote collections

Select transcript passages, attach notes, and export a collection of quotes with
recording titles and timestamps. Each quote should jump back to its audio.
This turns transcription into a practical interview/research tool. Preserve
whether a quote comes from edited text and avoid implying it is authenticated.

### Ukrainian/English correction tools

Benchmark consented recordings containing Ukrainian names, English technical
terms, and language switching. Explore per-recording vocabulary hints and
explicit, previewable replacements. Preserve the original transcript alongside
edits. Do not promise reliable mixed-language recognition or automatic learning
until tested; custom vocabulary already exists elsewhere.

### Portable local projects

Export/import a project containing audio, transcript, bookmarks, and edit history
so users can back up or move work without an account. Offer a transcript-only
option and show exactly what the archive contains. Encryption would be a
separate feature requiring key recovery and archive-format design.

### Source-linked local summaries, later

If users request summaries, make each summary point link to supporting transcript
segments, keep generated text separate from the transcript, and permit explicit
“not found” results. Evaluate model memory, accuracy, and device performance
independently of Whisper. This should follow a useful review workflow.

## Suggested next milestone

Deliver audio-linked review plus transcript search before adding another model.
Acceptance criteria:

- Open a saved recording and play its local normalized audio without a network.
- Select a timestamp and seek to that segment; reflect playback position in text.
- Support play/pause, speed adjustment, and keyboard seeking without intercepting
  normal textarea typing shortcuts.
- Find text, navigate matches, and keep unsaved edits intact during searching
  and playback; show a useful empty result.
- Release playback resources when switching/deleting a recording and handle
  missing audio with a recoverable error.
- Check the complete workflow in Tauri, including local media access, rather
  than treating a browser preview as evidence of native behavior.

Then tackle model onboarding and long-recording reliability. Validate the next
larger investment with interview/lecture users: compare time spent correcting a
recording, finding a quote, and reaching the first successful transcript.
Use observed sessions or opt-in feedback; analytics are not required.

Keep store signing, sandbox testing, purchase restore, and release QA on the
existing roadmap. New features do not remove those release requirements.
