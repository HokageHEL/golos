# Native dictation adapter: implementation status and device-validation matrix

Status: **macOS adapter implemented and unit-tested; no device validation was
performed.** Nothing in this document is evidence that dictation insertion works
on a real Mac. The automated checks below exercise a portable decision seam and
an isolated, named AppKit pasteboard. Focus behaviour, TCC consent, native
shortcut delivery, microphone capture, paste delivery into another application
and clipboard restoration timing all still require a human-run device session.

Scope of the native adapter implemented in `native/src-tauri/src/dictation/platform`:

- `platform.rs` selects the adapter. macOS uses the real implementation; every
  other target compiles the same public type with explicit errors, so a
  non-macOS build never claims an insertion capability.
- `platform/contract.rs` is the portable decision seam: snapshot limits,
  complete/lossy snapshot tracking, write/paste/restore authorization,
  single-use dispatch consumption and the bounded request ledger.
- `platform/macos.rs` is the AppKit/Accessibility/CoreGraphics implementation.

## What the automated checks actually prove

| Check | What it exercises | What it does **not** prove |
| --- | --- | --- |
| `cargo test --lib dictation::` (portable seam) | Snapshot completeness, memory caps, refusal before a write, single-use dispatch consumption, restore decision when a foreign copy won | Anything about AppKit, AX, TCC or the pasteboard server |
| `cargo test --lib dictation::platform::macos::tests::` (real AppKit, isolated named pasteboard `com.golos.dictation.test.*`) | That a rich item with a binary type round-trips, that multiple items survive, that an empty pasteboard snapshots as complete, that `changeCount` advances, that a foreign write wins over a delayed restore, that an oversized clipboard fails closed | That the general pasteboard behaves the same under the user's privacy settings, that a target app consumed a paste, that a restore landed in time |
| `cargo clippy --all-targets -- -D warnings` | No warnings in the macOS configuration | – |
| Non-macOS configuration simulation (module selected manually, then reverted) | That the unsupported adapter and the portable seam compile and lint without warnings, which is what Linux/Windows CI does | – |

Test hygiene: the tests only ever touch named pasteboards owned by this test
suite. They never read or write the user's general pasteboard, never request an
Accessibility or PostEvent grant, never read another application's UI, and
never start recording.

## Adapter behaviour that is implemented but still unverified on a device

- Target capture keeps the retained `NSRunningApplication` (bundle id, pid and
  launch time), the focused `AXUIElement` and the `AXSelectedTextRange`. The
  field's `AXValue` text is never read, and no AX attribute is written.
- Insertion revalidates the same application, the same element and the same
  selection range twice: once immediately before the clipboard write and once
  immediately before the event post. A changed caret, element or application is
  refused; there is no silent retarget.
- Secure fields, Golos itself and non-editable focus are refused. A secure
  field is rejected before any selection or clipboard data is read.
- Permission status is read without prompting. Accessibility and PostEvent are
  requested separately and only on an explicit request; recovery opens Privacy
  & Security.
- The clipboard snapshot covers every item and every type, including binary
  data, under explicit size caps. An incomplete, promised, denied or oversized
  snapshot refuses the insertion before the general pasteboard is changed.
  Restoration-disabled insertion is a separate explicit setting that retains
  the transcript on the clipboard.
- The paste is a `CGEventPostToPid` Command-V aimed at the verified pid. There
  is no HID/global fallback, and the adapter reports `PasteRequested`, never
  `Inserted`, because the public API cannot acknowledge consumption.
- The request ID is reserved before any main-thread work and consumed before
  the first event post, so a repeated or retried ID can never dispatch a
  second paste. A response lost after dispatch is not retried.
- A 500 ms worker thread, never the main thread, restores the snapshot and
  only while the pasteboard change count is still the adapter's own write. A
  concurrent user copy always wins, including after a failed paste.

## Device validation matrix (all rows are still open)

Record the actual OS build, hardware, app version and outcome for every row.
A CI pass closes none of them.

| # | Scenario | Expected behaviour | Status |
| --- | --- | --- | --- |
| 1 | TextEdit, empty document, caret at 0 | Capture names TextEdit; insertion pastes at the caret; UI says "Paste requested; verify the target app." | Not validated |
| 2 | TextEdit, caret in the middle of existing text | Insertion replaces the selection only; surrounding text is unchanged | Not validated |
| 3 | Notes (rich text) | Insertion inserts plain text without destroying note formatting | Not validated |
| 4 | Safari address bar and a page text field | Insertion targets the field that was captured, not the address bar | Not validated |
| 5 | Chrome text field | Same as Safari | Not validated |
| 6 | Slack message composer | Insertion reaches the composer, not the message list | Not validated |
| 7 | VS Code editor | Insertion reaches the editor, not the side bar or a panel | Not validated |
| 8 | Terminal | Insertion reaches the shell line; no unsafe `AXValue` replacement is used | Not validated |
| 9 | Mail compose body | Insertion reaches the body, not the headers | Not validated |
| 10 | Pages document | Insertion reaches the text box | Not validated |
| 11 | Secure field (Keychain, Safari password field, 1Password) | Refused with `SecureField`; no value read, no clipboard touched | Not validated |
| 12 | Focus moved to another app between capture and insert | Refused with `ApplicationChanged`/`FocusChanged`; nothing pasted | Not validated |
| 13 | Caret moved inside the same field | Refused with `SelectionChanged`; nothing pasted | Not validated |
| 14 | Focus moved to another field in the same app | Refused with `FocusChanged`; nothing pasted | Not validated |
| 15 | Target app quits, or the pid is reused by a new launch | Refused with `TargetUnavailable`; the launch identity is compared, not only the pid | Not validated |
| 16 | Accessibility denied | Status shows denied without a prompt; recording/transcription still work; insertion refused | Not validated |
| 17 | PostEvent denied | Same, with the separate PostEvent request path | Not validated |
| 18 | Permission revoked while the review window is open | The deferred insertion is not triggered; the user must confirm again | Not validated |
| 19 | Rich clipboard (image + text + RTF) | Complete snapshot, restore returns every type; a promised type fails closed | Not validated |
| 20 | Clipboard with a promised/lazy provider | Insertion refused with `ClipboardUnavailable` before the write | Not validated |
| 21 | User copies something between the write and the restore | The user's copy survives; the adapter reports the newer content | Not validated |
| 22 | User copies something between capture and insert | Insertion refused with `ClipboardChanged`; the clipboard is not modified | Not validated |
| 23 | Slow target app that reads the paste late | The 500 ms restore may land before the app reads the paste; this residual timing limitation is visible in the UI | Not validated |
| 24 | Full-screen app, Spaces, Stage Manager | Capture and paste still reach the captured pid | Not validated |
| 25 | Multiple monitors with different scaling | Capture and paste are unaffected by the display layout | Not validated |
| 26 | Sleep/wake between capture and insert | The lease is invalidated; insertion is refused until the target is captured again | Not validated |
| 27 | 30-minute capture stop and the resulting transcript | Insertion of a long transcript is a single request within the 4 MiB per-request limit | Not validated |
| 28 | Save failure or transcription failure | No insertion is offered; the target lease is released on dismiss | Not validated |
| 29 | Double click on Insert, or the hotkey pressed twice | The second request is refused with `AlreadyDispatched`; only one paste is dispatched | Not validated |
| 30 | Review window dismissed while the insertion is queued | The queued job is invalidated; no late paste occurs | Not validated |

## Known limitations and residual risks

1. **No paste-consumption acknowledgement.** The public event API cannot report
   that the target read the clipboard. The adapter therefore reports
   `PasteRequested`, and the UI must ask the user to verify the target app.
2. **Restore timing.** The restore worker waits 500 ms. A slow target may read
   the clipboard after the restore, and a slow user copy may win. Public APIs
   offer no way to prove consumption.
3. **Background focus.** The adapter revalidates the target application's
   `AXFocusedUIElement`. Some applications clear their own focus while they
   are not the key application, which the adapter treats as `FocusChanged`
   (fail closed). Whether the review window can stay open while the target
   keeps a verifiable focus is a device question the parent must answer before
   promising a one-key insert flow.
4. **Role allowlist.** Insertion is only offered for `AXTextField`,
   `AXTextArea`, `AXComboBox` and `AXSearchField` roles, with `AXSecureTextField`
   (role or subrole) always refused. An application using a custom role is
   refused with `NoEditableFocus`.
5. **Selection evidence.** A target that does not expose
   `AXSelectedTextRange` is refused, because the caret cannot be verified.
6. **Pasteboard privacy gates.** General-pasteboard access can be denied by the
   user or the system. The adapter fails closed in that case and never
   infers pasteboard permission from the Accessibility grant.
7. **No HID fallback.** If a process-targeted event is not delivered, the
   adapter does not fall back to a global synthetic keystroke, because that
   would type into whatever is focused. The result is a missing paste, not a
   misdirected one.
8. **Bounded request history.** The duplicate-request ledger keeps the most
   recent 4096 request IDs per process run. The parent must generate unique
   request IDs (UUIDs) and never reuse one.

## Integration notes for the parent (orchestration)

- `MacPlatform::new(AppHandle)` is the constructor, and the type is exported as
  `dictation::platform::MacPlatform`.
- `capture_target` and `insert` are blocking calls. When they are invoked from
  the AppKit main thread the adapter runs them inline (to avoid a
  self-deadlock), which blocks the UI for the duration of the AX round trips
  and the clipboard snapshot. Call them from an async command, a worker
  thread, or `std::thread::spawn`, never from a synchronous Tauri command that
  runs on the main thread.
- `insert` returns `Ok(InsertOutcome::Refused { .. })` for a safety refusal and
  `Err(String)` for cancellation, a queue timeout, or a native scheduling
  failure. Both mean "nothing was inserted"; only `PasteRequested` means a
  paste may have been delivered, and even then the UI must ask the user to
  verify the target app.
- A request ID is single use. A second call with the same ID is refused with
  `AlreadyDispatched`, including after a lost response. Use a fresh ID for
  every manual retry.
- `release_target` cancels queued work for that token and drops the retained
  native objects, so dismissal cannot be followed by a late paste.

## Contract amendments the parent may want to make

These are observations from implementing the adapter against the contracts in
`native/src-tauri/src/dictation.rs` (commit `ef2553b`). No parent-owned
contract file was modified by the adapter.

1. `ClipboardOutcome` has no value for "a restore is scheduled but has not run
   yet". The adapter returns `TranscriptRetained` at response time, which is
   accurate for the moment of the response but cannot describe the later
   restore. A `RestorePending` variant, or a separate restore event, would let
   the UI report the final state.
2. `InsertOutcome::Failed` carries no refusal reason. When the target changed
   after the clipboard write, the adapter has to put the reason in the message
   text. Adding `reason: Option<InsertRefusal>` to `Failed` would remove the
   need to parse messages.
3. There is no `InsertRefusal` variant for "the clipboard write itself
   failed". The adapter reports `Failed` with `ClipboardOutcome::RestoreFailed`
   and explains it in the message.
4. `Platform::insert` returns `Result<InsertOutcome, String>`. Cancellation,
   queue timeouts and native scheduling failures are returned as `Err`, which
   the parent must treat as "nothing was inserted" and must not retry
   automatically. A typed error enum would make that explicit.
5. `CapturedTarget` has no field for "captured while Accessibility was
   denied". The adapter keeps the app identity and reports
   `refusal: AccessibilityDenied` with `insertion_available: false`; no
   amendment is required, but the UI must not treat a captured target as
   insertable in that state.
6. `open_permission_settings` opens the Accessibility pane for both
   permission kinds, because the PostEvent TCC service is managed there. If
   the UI names the exact pane per kind, the two kinds need distinct
   destinations.

## Dependency changes made by the adapter

No new crate was added. The committed dependency set was extended with
features only:

- `core-graphics`: enabled the `elcapitan` feature, which gates the public
  `CGEventPostToPid` binding.
- `objc2-application-services`: enabled the `libc` feature, which gates
  `AXUIElementCreateApplication` and the pid-based AX element.
- `objc2-app-kit`: enabled the `libc` feature, which gates
  `NSRunningApplication.processIdentifier` and
  `runningApplicationWithProcessIdentifier`.

`CGPreflightPostEventAccess` and `CGRequestPostEventAccess` are public
CoreGraphics entry points and are declared directly against the system
framework instead of adding another crate.
