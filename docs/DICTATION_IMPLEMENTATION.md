# Native global recording and offline dictation implementation contract

Status: **implementation in progress, not a supported feature claim**. The pure
Rust state/safety contract in `dictation.rs` does not register a shortcut, show an
overlay, capture a target, access the clipboard or insert text. Native adapters,
orchestration, settings, UI and real-device verification must follow. The parity
matrix remains open.

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
