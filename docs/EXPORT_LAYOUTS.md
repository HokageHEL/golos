# Export layouts, timestamp offsets and subtitle wrapping

Golos keeps every export local. This milestone adds explicit layout controls to
the main transcript editor without changing saved transcripts, drafts, canonical
structured data, audio exports or project backups.

## Where the options are

Open **Export → Export options** above playback in a recording. The
controls are off by default and are not persisted; restarting Golos restores the
historical output. Export buttons remain disabled while the draft has unsaved
changes, so these options cannot be used to bypass draft protection.

## Readable formats: TXT, Markdown, HTML and DOCX

When **Customize export layout** is enabled:

- **Include timestamps** shows the block start and end times. Turning it off
  leaves only readable text and optional speaker labels.
- **Include speaker labels** uses the current corrected labels (`speaker_1`
  becomes `Speaker 1`). Turning it off keeps the same paragraphs but hides names.
- **Paragraph grouping** keeps one saved segment per block. **Adjacent same
  speaker** combines directly adjacent segments only when both have the same
  non-null speaker. A speaker change, an unassigned segment or the end of the
  transcript closes the group. Merged blocks use the first start and latest end,
  so overlapping speech is not lost. Original segment text, order and metadata
  are never modified.

DOCX uses these same three options through the existing `export_saved` command.
The manual DOCX button sends a validated options object; a DOCX export without
custom options, automatic folder export and AI-result DOCX export keep the
historical writer and defaults.

## Subtitles: SRT and VTT

Subtitle options are separate from readable-format layout:

- **Timestamp offset (milliseconds)** is added to every cue uniformly. A
  negative offset can trim leading silence. Offsets must be whole finite
  milliseconds. A result with a negative time, a non-finite/unsafe integer, a
  time beyond `99:59:59.999`, or a cue with no positive duration is rejected
  with an error. Golos does not clamp, repair or invent times.
- **Line width** wraps text inside a cue at 32, 42 or 50 grapheme clusters. The
  default is **No wrapping**. Wrapping is Unicode-aware: combining marks, emoji
  and emoji ZWJ sequences are not split, and overlong unbroken tokens are
  chunked at grapheme boundaries. It never adds cue breaks or word-level
  timestamps.

SRT and VTT remain one cue per non-empty saved segment. Uniform offsets preserve
the saved order, and overlapping cues are kept rather than merged, clamped or
silently removed. Empty paragraphs are omitted instead of producing invalid
empty cues. Invalid saved order or zero-duration cues produce an error rather
than fabricated timing. No format in this milestone performs word alignment.

## What does not change

- **CSV and JSON** retain canonical timestamps, speaker IDs, text and segment
  order. Layout options are ignored for these formats so they stay predictable
  for spreadsheets, scripts and re-import.
- **Saved audio (WAV)** and **`.golos` project backups** are unaffected; layout
  options are rejected for those commands and backups keep their canonical
  transcript.
- **Automatic folder exports**, batch export settings and **AI-result exports**
  keep their existing defaults. They are not silently switched to the editor's
  transient options.
- The original saved transcript, draft cache, segment timestamps and speaker
  labels are read-only inputs. Export rendering works on copies.

## Validation and limits

Covered by frontend unit tests, mounted DOM tests and Chromium flows with mocked
native IPC:

- timestamp/speaker inclusion and adjacent-speaker grouping boundaries;
- negative, fractional, non-finite, unsafe and overflowing offset rejection;
- one-cue-per-segment output, empty-cue omission, preserved overlap and
  rejection of out-of-order or zero-duration cues;
- Unicode and long-token wrapping with no invented cue times;
- original segment objects left unchanged;
- manual DOCX options and unchanged default DOCX output in Rust tests;
- editor DOM/browser flows that pass the options through `export_transcript`
  and `export_saved`.

Not claimed by this milestone:

- opening the DOCX in Microsoft Word, Pages or LibreOffice;
- subtitle consumer/player validation on external devices;
- native OS save-dialog, disk-full or slow-volume testing;
- word-level alignment, PDF, DOTE or Avid timecode export;
- persistence of layout preferences across launches.

## Local validation — October 20, 2026

Run in an isolated worktree based on `9f75a09`:

- `npm run lint` — passed with zero warnings.
- `npm run typecheck` and `npm run native:check` — passed.
- `npm test` — 76 tests in 14 files passed.
- `npm run native:build` — production frontend built.
- `npm run test:ui` — 12 Chromium flows passed against mocked native IPC,
  including the new export-options flow.
- `cargo fmt --manifest-path native/src-tauri/Cargo.toml -- --check` — passed.
- `cargo clippy --locked --manifest-path native/src-tauri/Cargo.toml --all-targets -- -D warnings`
  — passed.
- `cargo test --locked --manifest-path native/src-tauri/Cargo.toml` — 130
  passed, 8 opt-in tests ignored, 0 failed.

The browser and DOM flows do not open OS save dialogs, write native files, run
inference or validate external Word/Pages/subtitle consumers.

Integration validation on October 22, 2026: options now live inside the shared
**Export** disclosure above playback and use the same in-flow disclosure controls.
All 86 frontend tests and 15 mocked-native Chromium flows passed, including a
760 × 560 no-overflow/player separation assertion and option retention across
closing/reopening Export. Both TypeScript checks and ESLint passed. Rust had
145 passing tests and eight opt-in skips; formatting and all-target Clippy with
warnings denied passed. The unsigned arm64 macOS debug app built (87.45 MiB).
These checks do not close external document/subtitle consumer or native device gates.

After merging main's library benchmark, Rust validation remained green with 145
passing tests and nine opt-in skips. A follow-up regression covers empty speech
with a nonempty speaker label: custom SRT/VTT exports omit the paragraph before
adding speaker text, avoiding label-only cues. All 87 frontend tests, the targeted
mocked export browser flow, lint/native TypeScript and a fresh macOS debug bundle
passed after this fix. The full 15-flow browser suite passed before that narrow
serializer correction; the targeted export flow passed again afterwards.
