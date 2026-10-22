# Native global recording and offline dictation implementation contract

Status: **integrated implementation; device validation remains open**. The Rust
runtime, settings, overlay, review UI and native macOS platform adapter are wired
together. This does not establish native shortcut, capture, permission, target
focus, paste or clipboard-restoration behavior on a device. The parity matrix
and device matrix remain open.

## What this build contains

- `dictation.rs` — the pure state, target, clipboard and latch contract.
- `dictation_settings.rs` — the opt-in settings, their bounded validation, the
  per-app processing overrides and the interrupted-job storage. Schema 8 adds
  both tables; the migration preserves every existing note, transcript, local
  AI result and automation row.
- `dictation_shortcut.rs` — the shortcut registration lifecycle. A new
  validated shortcut is registered before the old one is removed, a failed
  candidate leaves the working shortcut in place, and a startup failure stays
  visible in Settings instead of pretending the feature is armed.
- `dictation_runtime.rs` — the Rust-owned runtime. It owns the single recording
  session, the serialized pipeline and the explicit review step. The main
  window and the dictation share one recording owner, so neither can stop or
  discard the other's capture.
- `dictation_overlay.rs` — the overlay presentation through the public
  `orderFrontRegardless` path, so the overlay never becomes the key window.
- `dictation_commands.rs` — the command surface. Every command that can block
  (target capture, microphone, save, transcription, local model, native paste)
  runs on a worker thread, never on the webview thread.
- `Overlay.tsx`, `DictationPanel.tsx`, `DictationSettings.tsx` — the button-only
  overlay, the focusable review in the main window, and the grouped settings
  dialog.

## Contract amendments

- The runtime accepts an injected `Arc<dyn dictation::Platform>`. While no
  adapter is supplied, the runtime records, transcribes and copies, reports
  `insertionSupported: false`, and refuses every insert with a clear message.
  It never pretends a native capability.
- The native adapter is called from worker threads. `capture_target` and
  `insert` must not run on the main thread; the commands already guarantee it.
- The adapter reports `ClipboardOutcome::TranscriptRetained` while a clipboard
  restoration is still pending. The interface reports the outcome exactly as
  the adapter saw it and never claims that the previous clipboard content was
  restored before it was.
- Global dictation uses the microphone source only. The main window keeps the
  system and mixed capture sources. Parity for other sources is not claimed.
- A restored job is review-only: it keeps the saved text, never a target lease,
  and can never insert.

## Product invariants

- macOS only; retain the existing 13.3 application floor and actual capture source
  version gates. No Windows, Linux or mobile support claim follows from compiling
  the cross-platform shortcut plugin.
- Reuse the existing recording session, source clock handling, pause/resume,
  30-minute captured-audio cap, normalization, library save and Whisper engine.
  No live transcription. Pauses never add silence or consume captured duration.
- Rust owns one recording session and one serialized operation. The overlay must
  not start another microphone, run transcription in a webview, or lose a save
  retry when hidden. Discard always needs an explicit user action; short hotkey
  taps must never silently discard captured audio.
- Transcribe only after a successful save. Keep original audio/transcript and
  store any optional local AI output separately, using the existing loopback-only
  adapter and model validation. AI and global shortcuts are opt-in.
- The main window's selection and unsaved draft cache survive overlay activity.
  New dictation recordings enter the library without automatically selecting them
  or replacing the editor's draft. A shared recording-state event must distinguish
  recording, paused, stopped, saving and save-failed states across both windows.

## Platform adapter boundary

`dictation::Platform` is a blocking adapter for permissions, target leases and
explicit insertion. Native implementations marshal AppKit work onto the main
thread and bound AX messaging timeouts. No Accessibility or clipboard access is
performed on launch just to populate Settings. Microphone recording and offline
transcription remain useful when insertion permissions are denied.

A target token references retained native application and AX element objects.
Do not accept a webview-supplied PID as an insertion target. Keep process identity
(including launch identity), original focused element, and original selection.
Re-verify all three immediately before clipboard mutation and again before
posting the paste. Refuse a changed element or selection even inside the same
application. Never restore a caret/selection silently, read a secure field value,
paste into Golos, or use whole-field AXValue replacement.

The adapter reports `PasteRequested`, not `Inserted`: a successful public event
post has no acknowledgement that the target consumed the paste. The UI must say
“Paste requested; verify the target app.” A dispatched request is single-use and
must not be automatically retried after a timeout, repeated hotkey or double click.
A timeout must invalidate a queued main-thread request before it can mutate the
clipboard or post events. Mark dispatch as consumed before posting; losing the
response must never make the same request eligible again. Dismissal must cancel
undispatched work, release its native target lease and prevent a late callback
from pasting after the review is gone. Never sleep on the AppKit main thread while
waiting to restore the clipboard.

## Clipboard contract

Insertion with restoration enabled requires a complete snapshot of every item
and format, including binary rich content. Denied access, promised data returning
nil, or an exceeded memory cap refuses insertion before changing the clipboard.
Offer explicit Copy as an alternative; Copy intentionally replaces the clipboard.
Restoration-disabled insertion must be a separate explicit preference, with clear
wording that it retains transcript text on the clipboard.

Check the pasteboard change count before writing and before pasting. Restore the
snapshot only while the clipboard still belongs to our write. A concurrent copy
always wins, including after a refused or failed paste. Never restore a partial
snapshot or retry restoration in a loop. Use an isolated named pasteboard for
adapter tests rather than reading the user's general clipboard.

A delayed clipboard restore cannot prove the target consumed a paste: public APIs
provide no consumption acknowledgement. Keep this residual timing limitation
visible in device evidence. General clipboard access has additional privacy gates
on recent macOS releases; never infer permission from Accessibility alone.

## Shortcut and overlay contract

Use the supported Tauri global-shortcut plugin from Rust, without adding broad
JavaScript shortcut-registration permissions. A handler only enqueues work; it
must not run AX, database or recording work on the main thread. Register a new
validated shortcut before removing the previous one, and retain the working
shortcut when a change conflicts or fails. Report startup registration failures.

Support toggle and hold-to-talk; require a real key and a Command/Control/Option
modifier, reject common reserved shortcuts, and ignore repeated key-down edges.
Fn/modifier-only shortcuts are not promised. The pure latch never inserts or
replaces a pending review, and cannot stop a recording owned by another workflow.

A normal Tauri `show()` can take focus. The implementation must use an explicitly
non-activating macOS presentation path and verify it on-device. Tauri creates an
NSWindow; simply applying NSPanel-only style bits is not a supported solution.
The recording overlay is button-only; editable preview lives in a focusable review
window. Preserve the Dock/main-window behavior. Overlay close hides it while the
Rust job continues; main close keeps existing recording/save protection.

## Orchestration and UI contract

The runtime exposes status, configuration, start, pause, stop/save, retry, preview,
insert and dismiss commands. Settings include an enabled shortcut, toggle/hold
mode, source language, overlay preference, clipboard preference, optional local
processing, and app-specific prompt overrides keyed by bundle ID. Configuration
uses the same operation guard as recording and model setup. Persist settings and
interrupted job state transactionally with a tested schema migration after the
automatic-AI migration; startup never resumes an insertion or trusts an old target
lease.

Start captures the frontmost target before showing the overlay. Stop/save failure
retains the capture for retry and prevents transcription/insertion. Transcription
failure retains the saved recording for retry. Original and processed text are
shown separately, with explicit review and insertion; no automatic paste follows
recording, permission approval, model completion or reopening the app. Dismiss
keeps saved recordings/results and releases native target/clipboard leases.

Permission recovery offers explicit request, Open Settings and Check again, with
copy-only fallback. Changes in permissions never trigger a deferred insertion.
Raw recording/transcription remains available without an account or network.

## Integration notes

- On macOS the native adapter is injected where the runtime is created:
  `DictationRuntime::new(app.handle().clone(), Some(Arc::new(MacPlatform::new(app.handle().clone()))))`.
  Other targets receive no adapter and report insertion as unavailable; no
  Windows, Linux or mobile dictation support is claimed.
- The overlay window is created lazily with the label `overlay` and the URL
  `index.html?overlay=1`. Its capability file allows the core
  default set plus `core:window:allow-hide`; it grants no shortcut
  registration and no clipboard permission.
- The dictation settings live in their own dialog component
  (`DictationSettings.tsx`) and are opened from **More tools → Global dictation…**.
  The dialog uses the shared fixed header/footer shell and in-flow grouped sections. Moving that entry into the shared settings dialog is a
  navigation change only: the component takes `open`, `status`, `onStatus`,
  `onError`, `nativeAvailable` and `onClose`.
- The main window never selects a dictated recording and never replaces the
  editor draft. The review panel is separate from the library list, and the
  dictated note is added to the library as its own entry.

## Validation still required

Pure state/permission/clipboard decision tests are necessary but do not validate
AppKit focus behavior, TCC, native shortcuts, microphone capture, paste delivery or
clipboard restoration. Require mocked runtime and frontend flows, real local
Whisper/AI component integration, full frontend/Rust checks and a macOS app build.

Device evidence must cover TextEdit, Notes, Safari/Chrome editing fields, Slack,
VS Code, Terminal, Mail and Pages; secure fields must refuse insertion. Cover
changed focus/caret, target quit/PID reuse, denied permissions, rich/promise
clipboard data, concurrent copy, slow paste, full-screen/Spaces/Stage Manager,
multiple monitors, save failure, sleep/wake and the 30-minute stop. Record actual
OS/device/application versions and outcomes; passing CI closes none of these gates.

## Primary API references

- [Tauri global shortcuts](https://v2.tauri.app/plugin/global-shortcut/)
- [Tauri window API](https://docs.rs/tauri/latest/tauri/window/struct.Window.html)
- [Apple NSPanel](https://developer.apple.com/documentation/appkit/nspanel)
- [Apple Accessibility element API](https://developer.apple.com/documentation/applicationservices/axuielement)
- [Apple frontmost application](https://developer.apple.com/documentation/appkit/nsworkspace/frontmostapplication)
- [Apple pasteboard change count](https://developer.apple.com/documentation/appkit/nspasteboard/changecount)
- [Apple pasteboard access behavior](https://developer.apple.com/documentation/appkit/nspasteboard/accessbehavior-swift.enum)
- [Apple process-targeted event posting](https://developer.apple.com/documentation/coregraphics/cgevent/posttopid(_:))
- [Apple App Sandbox restrictions](https://developer.apple.com/library/archive/documentation/Security/Conceptual/AppSandboxDesignGuide/DesigningYourSandbox/DesigningYourSandbox.html)

Research also inspected the installed macOS SDK headers. In particular,
AXSelectedText is documented non-writable; do not build the default insertion
path around undocumented success in an individual app. Read-only application
activity is at most a meeting prompt hint, not proof of a meeting and never
permission to start recording automatically.


## Integration checks — October 22, 2026

The runtime and native adapter were integrated with the current submenu, batch
drop and export-layout branches. The shared recording owner, one-shot drop store
and test fixtures coexist; window-close protection applies to the main window,
while hiding/closing the button-only overlay leaves its Rust job alive.

Integration fixes add separate explicit requests for Accessibility and keyboard
event access, stable per-app override inputs while bundle IDs are edited, shared
theme colors and fixed settings header/footer controls. No request runs just by
opening settings. The new DOM regression checks both permission command kinds
and input identity/focus; a mocked browser flow types an app ID character by
character and checks dialog containment at 760 × 560.

Rust: 184 tests passed, nine opt-in checks skipped, formatting and all-target
Clippy with warnings denied passed. These include isolated named-pasteboard
AppKit tests, which do not read or write the user's general clipboard. The full
19-flow browser suite passed with mocked native IPC before adding the final
permission/input regression assertions. Real microphone, global shortcut/TCC,
target app insertion, clipboard restore timing, 30-minute capture and Whisper
end-to-end behavior remain unverified for this combined dictation build.

The integrated macOS debug app built successfully (91.19 MiB). After the native
build finished, all 108 frontend tests passed with one worker, and the targeted
minimum-window dictation-settings flow passed with the final input/permission
assertions. Two earlier runs under concurrent compilation hit existing async-test
time limits; no timeout or assertion was weakened. Lint and both TypeScript checks
passed. This is compilation/mocked-test evidence, not device validation.

Integration follow-up still needed: the global recording surface does not yet
show captured elapsed time or react to an engine-driven stop before the user
presses Stop. The engine still enforces the audio cap; global timer/stopped-state
visibility must be completed and tested before claiming the workflow complete.

After integrating the signed-offset export fix, the combined branch passed 110
frontend tests, all 20 mocked browser flows, 185 Rust tests (nine opt-in skips),
lint, both TypeScript checks, formatting and all-target Clippy. This merge adds
no recording or paste behavior; the timer/engine-stop visibility follow-up and
native hardware gates above remain open.
