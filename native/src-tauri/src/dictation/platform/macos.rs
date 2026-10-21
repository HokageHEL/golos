//! macOS AppKit/Accessibility adapter.
//!
//! All AppKit and AX objects live in a main-thread-only thread-local lease
//! store. `MacPlatform` itself is `Send + Sync`; it never carries a retained
//! AppKit object across threads. Calls marshal a closure onto the Tauri main
//! thread and wait with a bounded timeout. AX messaging uses a small timeout on
//! every element, so a hung target cannot block the app indefinitely.

use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::NonNull,
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use objc2::rc::{autoreleasepool, Retained};
use objc2::{runtime::ProtocolObject, MainThreadMarker};
use objc2_app_kit::{
    NSPasteboard, NSPasteboardAccessBehavior, NSPasteboardItem, NSPasteboardTypeString,
    NSPasteboardWriting, NSRunningApplication, NSWorkspace,
};
use objc2_application_services::{
    kAXTrustedCheckOptionPrompt, AXError, AXIsProcessTrusted, AXIsProcessTrustedWithOptions,
    AXUIElement, AXValue, AXValueType,
};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFEqual, CFRange, CFRetained, CFString, CFType,
};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};
use tauri::AppHandle;

use core_graphics::event::{CGEvent, CGEventFlags, KeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use super::contract::{
    ClipboardLimits, ClipboardSnapshot, ClipboardTransaction, LedgerReservation, RequestLedger,
    SnapshotBuilder, SnapshotError, SnapshotType,
};
use crate::dictation::{
    insertion_refusal, CapturedTarget, ClipboardOutcome, FocusEvidence, InsertOutcome,
    InsertRefusal, PermissionKind, Permissions, Platform, RestoreDecision, TargetApp,
};

const MAIN_THREAD_TIMEOUT: Duration = Duration::from_secs(3);
const INSERT_TIMEOUT: Duration = Duration::from_secs(12);
const RESTORE_DELAY: Duration = Duration::from_millis(500);
const MAX_TRANSCRIPT_BYTES: usize = 4 * 1024 * 1024;
const MAX_REQUEST_ID_BYTES: usize = 256;
const MAX_PENDING_JOBS: usize = 8;
const MAX_LEASES: usize = 16;
const AX_MESSAGING_TIMEOUT_SECONDS: f32 = 0.5;
const ACCESSIBILITY_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// Public, non-prompting check for the PostEvent TCC service.
    fn CGPreflightPostEventAccess() -> bool;
    /// Public explicit request; only called from `request_permission`.
    fn CGRequestPostEventAccess() -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SelectionRange {
    location: i64,
    length: i64,
}

#[derive(Clone)]
struct NativeLease {
    app: Retained<NSRunningApplication>,
    app_info: TargetApp,
    element: Option<CFRetained<AXUIElement>>,
    selection: Option<SelectionRange>,
    secure: bool,
    editable: bool,
}

thread_local! {
    /// Retained AppKit/AX objects never cross a thread boundary. This map is
    /// touched only by main-thread dispatches (and by tests that never access
    /// it). The token is the only cross-thread identity.
    static MAIN_LEASES: std::cell::RefCell<HashMap<String, NativeLease>> =
        std::cell::RefCell::new(HashMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchState {
    Queued,
    Committed,
    Cancelled,
}

#[derive(Debug)]
struct JobControl {
    target_token: String,
    state: Mutex<DispatchState>,
}

impl JobControl {
    fn new(target_token: &str) -> Self {
        Self {
            target_token: target_token.to_string(),
            state: Mutex::new(DispatchState::Queued),
        }
    }

    fn is_cancelled(&self) -> bool {
        *lock(&self.state) == DispatchState::Cancelled
    }

    fn cancel_before_dispatch(&self) -> bool {
        let mut state = lock(&self.state);
        match *state {
            DispatchState::Queued => {
                *state = DispatchState::Cancelled;
                true
            }
            DispatchState::Committed => false,
            DispatchState::Cancelled => true,
        }
    }

    /// Commits the single event post. Cancellation wins when it acquired the
    /// state lock first; otherwise the paste is already considered dispatched
    /// and a later timeout can only report a lost response.
    fn commit_dispatch(&self) -> bool {
        let mut state = lock(&self.state);
        if *state == DispatchState::Cancelled {
            false
        } else {
            *state = DispatchState::Committed;
            true
        }
    }
}

struct RestoreJob {
    snapshot: ClipboardSnapshot,
    written_count: i64,
}

pub struct MacPlatform {
    app: AppHandle,
    limits: ClipboardLimits,
    requests: Arc<Mutex<RequestLedger>>,
    jobs: Arc<Mutex<HashMap<String, Arc<JobControl>>>>,
    restore_tx: Sender<RestoreJob>,
    restore_worker_available: bool,
}

impl MacPlatform {
    pub fn new(app: AppHandle) -> Self {
        let (restore_tx, restore_rx) = mpsc::channel::<RestoreJob>();
        let restore_worker_available = spawn_restore_worker(app.clone(), restore_rx);
        Self {
            app,
            limits: ClipboardLimits::default(),
            requests: Arc::new(Mutex::new(RequestLedger::new(
                super::contract::DEFAULT_REQUEST_HISTORY,
            ))),
            jobs: Arc::new(Mutex::new(HashMap::new())),
            restore_tx,
            restore_worker_available,
        }
    }

    fn cancel_jobs_for(&self, target_token: &str) {
        let jobs = lock(&self.jobs);
        for job in jobs.values() {
            if job.target_token == target_token {
                job.cancel_before_dispatch();
            }
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn spawn_restore_worker(app: AppHandle, jobs: Receiver<RestoreJob>) -> bool {
    thread::Builder::new()
        .name("golos-clipboard-restore".into())
        .spawn(move || {
            while let Ok(job) = jobs.recv() {
                // Never sleep on the AppKit main thread. A later user copy
                // changes the pasteboard count and wins over this restore.
                thread::sleep(RESTORE_DELAY);
                let app = app.clone();
                let _ = app.run_on_main_thread(move || {
                    autoreleasepool(|_| {
                        let pasteboard = NSPasteboard::generalPasteboard();
                        if pasteboard.changeCount() as i64 != job.written_count {
                            return;
                        }
                        let _ = restore_pasteboard(&pasteboard, &job.snapshot);
                    });
                });
            }
        })
        .map(|_| true)
        .unwrap_or(false)
}

/// Run a closure on the AppKit main thread with a bounded wait. The closure
/// must not capture retained AppKit objects. Calling this from the main thread
/// runs inline to avoid a self-deadlock.
fn run_on_main<T, F>(app: &AppHandle, timeout: Duration, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    if MainThreadMarker::new().is_some() {
        return Ok(f());
    }
    let (tx, rx) = mpsc::channel();
    app.run_on_main_thread(move || {
        let result = catch_unwind(AssertUnwindSafe(f)).ok();
        let _ = tx.send(result);
    })
    .map_err(|error| format!("Could not reach the macOS main thread: {error}"))?;
    match rx.recv_timeout(timeout) {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err("The macOS main-thread task failed unexpectedly.".into()),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("The macOS main thread did not respond in time.".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("The macOS main-thread task was cancelled.".into())
        }
    }
}

fn dispatch_detached(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
    if MainThreadMarker::new().is_some() {
        f();
    } else {
        let _ = app.run_on_main_thread(f);
    }
}

fn app_is_self(pid: i32) -> bool {
    pid == std::process::id() as i32
}

fn app_info(app: &NSRunningApplication) -> TargetApp {
    let bundle_id = app
        .bundleIdentifier()
        .map(|value| value.to_string())
        .unwrap_or_default();
    let name = app
        .localizedName()
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| bundle_id.clone());
    let launch_time_ms = app
        .launchDate()
        .map(|date| (date.timeIntervalSince1970() * 1000.0) as i64)
        .unwrap_or(0);
    TargetApp {
        pid: app.processIdentifier(),
        bundle_id,
        name,
        launch_time_ms,
    }
}

fn current_post_events_allowed() -> bool {
    // SAFETY: Public CoreGraphics query with no arguments or stored state.
    unsafe { CGPreflightPostEventAccess() }
}

fn current_accessibility_allowed() -> bool {
    // SAFETY: Public Accessibility query with no arguments.
    unsafe { AXIsProcessTrusted() }
}

fn current_permissions() -> Permissions {
    Permissions {
        accessibility: current_accessibility_allowed(),
        post_events: current_post_events_allowed(),
    }
}

fn ax_attribute(name: &'static str) -> CFRetained<CFString> {
    CFString::from_static_str(name)
}

fn copy_attribute(
    element: &AXUIElement,
    name: &'static str,
) -> Result<CFRetained<CFType>, AXError> {
    let attribute = ax_attribute(name);
    let mut value: *const CFType = std::ptr::null();
    let pointer = NonNull::new(&mut value as *mut *const CFType)
        .expect("stack pointer for AX attribute result is non-null");
    // SAFETY: `element` is a live AX object and `pointer` points at valid
    // storage for the returned CFType reference.
    let error = unsafe { element.copy_attribute_value(&attribute, pointer) };
    if error != AXError::Success {
        return Err(error);
    }
    if value.is_null() {
        return Err(AXError::NoValue);
    }
    // SAFETY: AX copy functions return a +1 CFType reference.
    Ok(unsafe { CFRetained::from_raw(NonNull::new_unchecked(value.cast_mut())) })
}

fn copy_element(
    element: &AXUIElement,
    name: &'static str,
) -> Result<CFRetained<AXUIElement>, AXError> {
    let value = copy_attribute(element, name)?;
    value
        .downcast::<AXUIElement>()
        .map_err(|_| AXError::IllegalArgument)
}

fn attribute_string(element: &AXUIElement, name: &'static str) -> Option<String> {
    let value = copy_attribute(element, name).ok()?;
    let string = value.downcast::<CFString>().ok()?;
    Some(string.to_string())
}

fn secure_role(role: &str, subrole: &str) -> bool {
    role == "AXSecureTextField" || subrole == "AXSecureTextField"
}

fn editable_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField"
    )
}

fn read_selection(element: &AXUIElement) -> Option<SelectionRange> {
    let value = copy_attribute(element, "AXSelectedTextRange").ok()?;
    let value = value.downcast::<AXValue>().ok()?;
    // SAFETY: The value is a live AXValue object.
    if unsafe { value.r#type() } != AXValueType::CFRange {
        return None;
    }
    let mut range = CFRange {
        location: 0,
        length: 0,
    };
    let pointer = NonNull::new((&mut range as *mut CFRange).cast::<std::ffi::c_void>())?;
    // SAFETY: `range` is a valid CFRange and the AXValue type was checked.
    if !unsafe { value.value(AXValueType::CFRange, pointer) } {
        return None;
    }
    Some(SelectionRange {
        location: range.location as i64,
        length: range.length as i64,
    })
}

fn read_role(element: &AXUIElement) -> (String, String) {
    let role = attribute_string(element, "AXRole").unwrap_or_default();
    let subrole = attribute_string(element, "AXSubrole").unwrap_or_default();
    (role, subrole)
}

fn bounded_element(element: &AXUIElement) {
    // SAFETY: Public AX API; the timeout is local to this element object.
    let _ = unsafe { element.set_messaging_timeout(AX_MESSAGING_TIMEOUT_SECONDS) };
}

fn capture_on_main(token: &str) -> Result<CapturedTarget, String> {
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace
        .frontmostApplication()
        .ok_or_else(|| "No frontmost application could be captured.".to_string())?;
    let app_info = app_info(&app);
    let self_target = app_is_self(app_info.pid);
    let permissions = current_permissions();

    let mut element = None;
    let mut selection = None;
    let mut secure = false;
    let mut editable = false;

    if permissions.accessibility {
        let application = unsafe { AXUIElement::new_application(app_info.pid) };
        bounded_element(&application);
        if let Ok(focused) = copy_element(&application, "AXFocusedUIElement") {
            bounded_element(&focused);
            let (role, subrole) = read_role(&focused);
            secure = secure_role(&role, &subrole);
            if !secure {
                editable = editable_role(&role);
                if editable {
                    // AXSelectedTextRange is documented read-only and is the
                    // only range evidence used; the field's AXValue text is
                    // never read.
                    selection = read_selection(&focused);
                    if selection.is_none() {
                        editable = false;
                    }
                }
            }
            element = Some(focused);
        }
    }

    let focus = FocusEvidence {
        accessibility: permissions.accessibility,
        post_events: permissions.post_events,
        target_alive: !app.isTerminated(),
        self_target,
        secure,
        editable,
        same_application: true,
        same_element: true,
        same_selection: true,
    };
    let refusal = insertion_refusal(focus);
    let target = CapturedTarget {
        token: token.to_string(),
        app: app_info.clone(),
        insertion_available: refusal.is_none(),
        refusal,
    };

    let element_for_store = element;
    MAIN_LEASES.with(|leases| {
        let mut leases = leases.borrow_mut();
        if leases.len() >= MAX_LEASES {
            if let Some(oldest) = leases.keys().next().cloned() {
                leases.remove(&oldest);
            }
        }
        leases.insert(
            token.to_string(),
            NativeLease {
                app,
                app_info,
                element: element_for_store,
                selection,
                secure,
                editable,
            },
        );
    });
    Ok(target)
}

fn lease_for(token: &str) -> Option<NativeLease> {
    MAIN_LEASES.with(|leases| leases.borrow().get(token).cloned())
}

fn remove_lease(token: &str) {
    MAIN_LEASES.with(|leases| {
        leases.borrow_mut().remove(token);
    });
}

fn app_still_same(lease: &NativeLease) -> bool {
    let info = &lease.app_info;
    if lease.app.isTerminated() {
        return false;
    }
    let Some(current) = NSRunningApplication::runningApplicationWithProcessIdentifier(info.pid)
    else {
        return false;
    };
    if current.isTerminated() {
        return false;
    }
    let current_info = app_info(&current);
    current_info.bundle_id == info.bundle_id
        && current_info.launch_time_ms == info.launch_time_ms
        && current_info.pid == info.pid
}

/// Revalidate the exact retained application, focused AX element and selection
/// range. There is no silent retargeting: any mismatch is a refusal.
fn revalidate(lease: &NativeLease) -> Result<(), InsertRefusal> {
    if !current_accessibility_allowed() {
        return Err(InsertRefusal::AccessibilityDenied);
    }
    if !current_post_events_allowed() {
        return Err(InsertRefusal::PostEventDenied);
    }
    if !app_still_same(lease) {
        return Err(InsertRefusal::TargetUnavailable);
    }
    if lease.secure {
        return Err(InsertRefusal::SecureField);
    }
    if !lease.editable {
        return Err(InsertRefusal::NoEditableFocus);
    }
    let Some(captured) = &lease.element else {
        return Err(InsertRefusal::FocusChanged);
    };
    let application = unsafe { AXUIElement::new_application(lease.app_info.pid) };
    bounded_element(&application);
    let focused = copy_element(&application, "AXFocusedUIElement")
        .map_err(|_| InsertRefusal::FocusChanged)?;
    bounded_element(&focused);
    if !CFEqual(Some(captured.as_ref()), Some(focused.as_ref())) {
        return Err(InsertRefusal::FocusChanged);
    }
    let (role, subrole) = read_role(&focused);
    if secure_role(&role, &subrole) {
        return Err(InsertRefusal::SecureField);
    }
    if !editable_role(&role) {
        return Err(InsertRefusal::NoEditableFocus);
    }
    let current_selection = read_selection(&focused).ok_or(InsertRefusal::SelectionChanged)?;
    if Some(current_selection) != lease.selection {
        return Err(InsertRefusal::SelectionChanged);
    }
    Ok(())
}

fn refusal(reason: InsertRefusal, message: &str) -> InsertOutcome {
    InsertOutcome::Refused {
        reason,
        message: message.to_string(),
    }
}

fn failed(clipboard: ClipboardOutcome, message: String) -> InsertOutcome {
    InsertOutcome::Failed { clipboard, message }
}

fn snapshot_pasteboard(
    pasteboard: &NSPasteboard,
    limits: ClipboardLimits,
) -> Result<ClipboardSnapshot, SnapshotError> {
    if pasteboard.accessBehavior() == NSPasteboardAccessBehavior::AlwaysDeny {
        return Err(SnapshotError::Denied);
    }
    let mut builder = SnapshotBuilder::new(pasteboard.changeCount() as i64, limits);
    let mut total_bytes = 0usize;
    if let Some(items) = pasteboard.pasteboardItems() {
        for item in items.iter() {
            if builder.item_count() >= limits.max_items {
                return Err(SnapshotError::TooManyItems);
            }
            let types = item.types();
            if types.len() > limits.max_types_per_item {
                return Err(SnapshotError::TooManyTypes);
            }
            let mut copied = Vec::with_capacity(types.len());
            for kind in types.iter() {
                let data = item
                    .dataForType(&kind)
                    .ok_or(SnapshotError::PromisedDataUnavailable)?;
                total_bytes = total_bytes
                    .checked_add(data.len())
                    .ok_or(SnapshotError::TooLarge)?;
                if total_bytes > limits.max_bytes {
                    return Err(SnapshotError::TooLarge);
                }
                copied.push(SnapshotType {
                    kind: kind.to_string(),
                    data: data.to_vec(),
                });
            }
            builder.add_item(copied)?;
        }
    }
    Ok(builder.finish())
}

fn write_transcript(pasteboard: &NSPasteboard, text: &str) -> Result<i64, String> {
    let _ = pasteboard.clearContents();
    let string = NSString::from_str(text);
    // SAFETY: `NSPasteboardTypeString` is a static AppKit string constant.
    let kind = unsafe { NSPasteboardTypeString };
    if !pasteboard.setString_forType(&string, kind) {
        return Err("The clipboard rejected the transcript.".into());
    }
    Ok(pasteboard.changeCount() as i64)
}

fn restore_pasteboard(
    pasteboard: &NSPasteboard,
    snapshot: &ClipboardSnapshot,
) -> Result<(), String> {
    if !snapshot.complete {
        return Err("A lossy clipboard snapshot cannot be restored.".into());
    }
    let mut writers: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
        Vec::with_capacity(snapshot.items.len());
    for item in &snapshot.items {
        let pasteboard_item = NSPasteboardItem::new();
        for value in &item.types {
            let data = NSData::with_bytes(&value.data);
            let kind = NSString::from_str(&value.kind);
            if !pasteboard_item.setData_forType(&data, &kind) {
                return Err("The clipboard rejected a restored format.".into());
            }
        }
        writers.push(ProtocolObject::from_retained(pasteboard_item));
    }
    let _ = pasteboard.clearContents();
    if writers.is_empty() {
        return Ok(());
    }
    let array = NSArray::from_retained_slice(&writers);
    if !pasteboard.writeObjects(&array) {
        return Err("The clipboard rejected the restored contents.".into());
    }
    Ok(())
}

fn restore_after_failure(
    pasteboard: &NSPasteboard,
    transaction: &ClipboardTransaction,
) -> ClipboardOutcome {
    match transaction.restore_decision(pasteboard.changeCount() as i64) {
        RestoreDecision::KeepNewerClipboard => ClipboardOutcome::KeptNewerContent,
        RestoreDecision::KeepTranscript => ClipboardOutcome::TranscriptRetained,
        RestoreDecision::Restore => {
            let Some(snapshot) = transaction.restore_snapshot() else {
                return ClipboardOutcome::TranscriptRetained;
            };
            if restore_pasteboard(pasteboard, snapshot).is_ok() {
                ClipboardOutcome::Restored
            } else {
                ClipboardOutcome::RestoreFailed
            }
        }
    }
}

fn post_cmd_v(pid: i32) -> Result<(), String> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| "Could not create a keyboard event source.".to_string())?;
    let down = CGEvent::new_keyboard_event(source.clone(), KeyCode::ANSI_V, true)
        .map_err(|_| "Could not create the paste key event.".to_string())?;
    let up = CGEvent::new_keyboard_event(source, KeyCode::ANSI_V, false)
        .map_err(|_| "Could not create the paste key event.".to_string())?;
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    // Public API: post only to the verified process. There is no HID/global
    // fallback and no acknowledgement that the target consumed the paste.
    down.post_to_pid(pid);
    up.post_to_pid(pid);
    Ok(())
}

/// Runs entirely on the AppKit main thread. It revalidates before the clipboard
/// write and again before the event post, consumes the request before posting,
/// and never retries.
struct InsertRequest {
    token: String,
    request_id: String,
    text: String,
    restore_previous: bool,
    limits: ClipboardLimits,
    requests: Arc<Mutex<RequestLedger>>,
    job: Arc<JobControl>,
    restore_tx: Sender<RestoreJob>,
    restore_worker_available: bool,
}

fn insert_on_main(request: InsertRequest) -> Result<InsertOutcome, String> {
    let InsertRequest {
        token,
        request_id,
        text,
        restore_previous,
        limits,
        requests,
        job,
        restore_tx,
        restore_worker_available,
    } = request;
    let Some(lease) = lease_for(&token) else {
        return Ok(refusal(
            InsertRefusal::TargetUnavailable,
            "The captured target has expired. Capture it again before inserting.",
        ));
    };
    if job.is_cancelled() {
        return Err("Insertion was cancelled before clipboard access.".into());
    }
    if let Err(reason) = revalidate(&lease) {
        return Ok(refusal(reason, refusal_message(reason)));
    }
    if restore_previous && !restore_worker_available {
        return Ok(refusal(
            InsertRefusal::ClipboardUnavailable,
            "Previous-clipboard restoration is unavailable. Use Copy instead.",
        ));
    }

    let pasteboard = NSPasteboard::generalPasteboard();
    let original_count = pasteboard.changeCount() as i64;
    let snapshot = if restore_previous {
        match snapshot_pasteboard(&pasteboard, limits) {
            Ok(snapshot) => Some(snapshot),
            Err(_) => {
                return Ok(refusal(
                    InsertRefusal::ClipboardUnavailable,
                    "The clipboard contains data that cannot be copied completely, so insertion was refused. Use Copy instead.",
                ));
            }
        }
    } else {
        None
    };

    let mut transaction = ClipboardTransaction::prepare(original_count, restore_previous, snapshot);

    // Revalidate after the potentially slow snapshot and immediately before
    // the pasteboard write.
    if job.is_cancelled() {
        return Err("Insertion was cancelled before the clipboard write.".into());
    }
    if let Err(reason) = revalidate(&lease) {
        return Ok(refusal(reason, refusal_message(reason)));
    }
    if let Err(reason) = transaction.authorize_write(pasteboard.changeCount() as i64) {
        return Ok(refusal(reason, refusal_message(reason)));
    }

    let written_count = match write_transcript(&pasteboard, &text) {
        Ok(count) => count,
        Err(error) => {
            // The pasteboard was already cleared. Nothing was pasted, so the
            // captured snapshot can be put back immediately instead of
            // leaving the user without their clipboard.
            let clipboard = match transaction.restore_snapshot() {
                Some(snapshot) => match restore_pasteboard(&pasteboard, snapshot) {
                    Ok(()) => ClipboardOutcome::Restored,
                    Err(_) => ClipboardOutcome::RestoreFailed,
                },
                None => ClipboardOutcome::RestoreFailed,
            };
            return Ok(failed(
                clipboard,
                format!("{error} The previous clipboard could not be kept."),
            ));
        }
    };
    transaction.note_written(written_count);

    // Revalidate after our write and immediately before posting. A changed
    // caret, element or application refuses the paste and restores only while
    // our write still owns the clipboard.
    if job.is_cancelled() {
        let clipboard = restore_after_failure(&pasteboard, &transaction);
        return Err(format!(
            "Insertion was cancelled before paste ({clipboard:?})."
        ));
    }
    if let Err(reason) = revalidate(&lease) {
        let clipboard = restore_after_failure(&pasteboard, &transaction);
        return Ok(failed(
            clipboard,
            format!(
                "The target changed after the clipboard write ({}). Nothing was pasted.",
                refusal_message(reason)
            ),
        ));
    }
    if let Err(reason) = transaction.authorize_paste(pasteboard.changeCount() as i64) {
        let clipboard = restore_after_failure(&pasteboard, &transaction);
        return Ok(failed(clipboard, refusal_message(reason).to_string()));
    }

    // Commit before the first event. If this response is lost, the request ID
    // stays consumed and can never dispatch another paste.
    if !job.commit_dispatch() {
        let clipboard = restore_after_failure(&pasteboard, &transaction);
        return Err(format!(
            "Insertion was cancelled before paste ({clipboard:?})."
        ));
    }
    if let Err(reason) = transaction.consume_dispatch() {
        return Ok(refusal(reason, refusal_message(reason)));
    }
    debug_assert!(matches!(
        transaction.restore_decision(pasteboard.changeCount() as i64),
        RestoreDecision::Restore
            | RestoreDecision::KeepNewerClipboard
            | RestoreDecision::KeepTranscript
    ));
    lock(&requests).mark_dispatched(&request_id);

    if let Err(error) = post_cmd_v(lease.app_info.pid) {
        let clipboard = restore_after_failure(&pasteboard, &transaction);
        return Ok(failed(clipboard, error));
    }

    let clipboard = if !restore_previous {
        ClipboardOutcome::TranscriptRetained
    } else if let Some(snapshot) = transaction.restore_snapshot().cloned() {
        if restore_tx
            .send(RestoreJob {
                snapshot,
                written_count,
            })
            .is_err()
        {
            ClipboardOutcome::RestoreFailed
        } else {
            // The transcript is still on the clipboard at response time. A
            // delayed worker restores the snapshot only if the count is ours.
            ClipboardOutcome::TranscriptRetained
        }
    } else {
        ClipboardOutcome::TranscriptRetained
    };

    Ok(InsertOutcome::PasteRequested {
        clipboard,
        message: if restore_previous {
            "Paste requested; verify the target app. The previous clipboard is restored only if it is still ours.".into()
        } else {
            "Paste requested; verify the target app. The transcript is retained on the clipboard."
                .into()
        },
    })
}

fn refusal_message(reason: InsertRefusal) -> &'static str {
    match reason {
        InsertRefusal::AccessibilityDenied => {
            "Accessibility access is required to verify the edit target."
        }
        InsertRefusal::PostEventDenied => {
            "Permission to post keyboard events is required for insertion."
        }
        InsertRefusal::TargetUnavailable => "The captured application is no longer available.",
        InsertRefusal::SelfTarget => "Golos cannot insert into its own windows.",
        InsertRefusal::SecureField => "Insertion is refused in a secure text field.",
        InsertRefusal::NoEditableFocus => {
            "The captured target no longer has an editable focused field."
        }
        InsertRefusal::ApplicationChanged => {
            "The target application changed. Capture the target again."
        }
        InsertRefusal::FocusChanged => "The focused field changed. Capture the target again.",
        InsertRefusal::SelectionChanged => {
            "The caret or selection changed. Capture the target again."
        }
        InsertRefusal::ClipboardUnavailable => {
            "The clipboard cannot be copied completely for safe restoration."
        }
        InsertRefusal::ClipboardChanged => {
            "The clipboard changed during insertion. Nothing was pasted."
        }
        InsertRefusal::AlreadyDispatched => {
            "This insertion request was already dispatched and will not be retried."
        }
    }
}

/// The adapter is safe to share between threads without any `unsafe impl Send`:
/// the retained AppKit/AX objects stay in the main-thread lease store.
struct AssertSendSync<T: Send + Sync>(std::marker::PhantomData<T>);
const _: AssertSendSync<MacPlatform> = AssertSendSync(std::marker::PhantomData);

impl Platform for MacPlatform {
    fn permissions(&self) -> Result<Permissions, String> {
        Ok(current_permissions())
    }

    fn request_permission(&self, kind: PermissionKind) -> Result<Permissions, String> {
        match kind {
            PermissionKind::Accessibility => {
                let key = unsafe { kAXTrustedCheckOptionPrompt };
                let key_cf: &CFType = key.as_ref();
                let value = CFBoolean::new(true);
                let value_cf: &CFType = value.as_ref();
                let dictionary =
                    CFDictionary::<CFType, CFType>::from_slices(&[key_cf], &[value_cf]);
                // SAFETY: The dictionary uses the documented boolean option.
                let _ = unsafe { AXIsProcessTrustedWithOptions(Some(dictionary.as_ref())) };
            }
            PermissionKind::PostEvents => {
                // SAFETY: Public explicit request for the PostEvent TCC service.
                let _ = unsafe { CGRequestPostEventAccess() };
            }
        }
        Ok(current_permissions())
    }

    fn open_permission_settings(&self, _kind: PermissionKind) -> Result<(), String> {
        let settings_url = ACCESSIBILITY_SETTINGS_URL.to_string();
        run_on_main(&self.app, MAIN_THREAD_TIMEOUT, move || {
            autoreleasepool(|_| {
                let settings_url = NSString::from_str(&settings_url);
                let Some(url) = NSURL::URLWithString(&settings_url) else {
                    return Err("Could not build the Privacy & Security settings URL.".into());
                };
                let workspace = NSWorkspace::sharedWorkspace();
                if workspace.openURL(&url) {
                    Ok(())
                } else {
                    Err("macOS could not open Privacy & Security settings.".into())
                }
            })
        })?
    }

    fn capture_target(&self) -> Result<CapturedTarget, String> {
        let token = uuid::Uuid::new_v4().to_string();
        let token_for_main = token.clone();
        let result = run_on_main(&self.app, MAIN_THREAD_TIMEOUT, move || {
            autoreleasepool(|_| capture_on_main(&token_for_main))
        })?;
        if result.is_err() {
            // A timed-out dispatch can still run later; make sure a stale token
            // cannot be used.
            let token = token.clone();
            dispatch_detached(&self.app, move || remove_lease(&token));
        }
        result
    }

    fn release_target(&self, token: &str) {
        self.cancel_jobs_for(token);
        let token = token.to_string();
        dispatch_detached(&self.app, move || remove_lease(&token));
    }

    fn insert(
        &self,
        target: &str,
        request_id: &str,
        text: &str,
        restore_previous_clipboard: bool,
    ) -> Result<InsertOutcome, String> {
        if target.is_empty() {
            return Ok(refusal(
                InsertRefusal::TargetUnavailable,
                "Capture a target before inserting.",
            ));
        }
        if request_id.is_empty() || request_id.len() > MAX_REQUEST_ID_BYTES {
            return Err("Insertion requests need a bounded, non-empty request ID.".into());
        }
        if text.is_empty() {
            return Ok(refusal(
                InsertRefusal::NoEditableFocus,
                "There is no transcript text to paste.",
            ));
        }
        if text.len() > MAX_TRANSCRIPT_BYTES {
            return Err("The transcript is too large for a single insertion request.".into());
        }
        match lock(&self.requests).reserve(request_id) {
            LedgerReservation::Duplicate => {
                return Ok(refusal(
                    InsertRefusal::AlreadyDispatched,
                    "This insertion request was already used and will not be retried.",
                ));
            }
            LedgerReservation::Reserved => {}
        }

        let job = Arc::new(JobControl::new(target));
        {
            let mut jobs = lock(&self.jobs);
            if jobs.len() >= MAX_PENDING_JOBS {
                return Err("Too many insertion requests are already pending.".into());
            }
            jobs.insert(request_id.to_string(), job.clone());
        }

        let app = self.app.clone();
        let requests = self.requests.clone();
        let restore_tx = self.restore_tx.clone();
        let restore_worker_available = self.restore_worker_available;
        let token = target.to_string();
        let request_id_owned = request_id.to_string();
        let text = text.to_string();
        let limits = self.limits;
        let job_for_main = job.clone();
        let (tx, rx) = mpsc::channel();
        let queued = app.run_on_main_thread(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                insert_on_main(InsertRequest {
                    token,
                    request_id: request_id_owned,
                    text,
                    restore_previous: restore_previous_clipboard,
                    limits,
                    requests,
                    job: job_for_main,
                    restore_tx,
                    restore_worker_available,
                })
            }))
            .unwrap_or_else(|_| Err("The native insertion task failed unexpectedly.".into()));
            let _ = tx.send(result);
        });
        if let Err(error) = queued {
            lock(&self.jobs).remove(request_id);
            return Err(format!("Could not schedule native insertion: {error}"));
        }

        match rx.recv_timeout(INSERT_TIMEOUT) {
            Ok(result) => {
                lock(&self.jobs).remove(request_id);
                result
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let cancelled_before_dispatch = job.cancel_before_dispatch();
                lock(&self.jobs).remove(request_id);
                if cancelled_before_dispatch {
                    Err("Insertion timed out before any paste was dispatched. The request ID is consumed; use a new request.".into())
                } else {
                    Err("Insertion timed out after the paste was dispatched. It will not be retried; verify the target app.".into())
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let cancelled_before_dispatch = job.cancel_before_dispatch();
                lock(&self.jobs).remove(request_id);
                if cancelled_before_dispatch {
                    Err(
                        "The native insertion task ended before dispatch. Use a new request ID."
                            .into(),
                    )
                } else {
                    Err("The paste was dispatched but its response was lost. It will not be retried; verify the target app.".into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique isolated pasteboard. The user's general pasteboard is never
    /// read or written by these tests.
    fn named_pasteboard(name: &str) -> Retained<NSPasteboard> {
        let name = NSString::from_str(name);
        NSPasteboard::pasteboardWithName(&name)
    }

    fn write_one_item(pasteboard: &NSPasteboard, types: &[(&str, &[u8])]) {
        let item = NSPasteboardItem::new();
        for (kind, bytes) in types {
            let data = NSData::with_bytes(bytes);
            let kind = NSString::from_str(kind);
            assert!(item.setData_forType(&data, &kind));
        }
        let writers: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> =
            vec![ProtocolObject::from_retained(item)];
        let array = NSArray::from_retained_slice(&writers);
        let _ = pasteboard.clearContents();
        assert!(pasteboard.writeObjects(&array));
    }

    fn write_two_items(pasteboard: &NSPasteboard) {
        let mut writers: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = Vec::new();
        for (kind, bytes) in [
            ("public.utf8-plain-text", b"one".as_slice()),
            ("com.example.binary", &[9u8, 8, 7][..]),
        ] {
            let item = NSPasteboardItem::new();
            let data = NSData::with_bytes(bytes);
            let kind = NSString::from_str(kind);
            assert!(item.setData_forType(&data, &kind));
            writers.push(ProtocolObject::from_retained(item));
        }
        let array = NSArray::from_retained_slice(&writers);
        let _ = pasteboard.clearContents();
        assert!(pasteboard.writeObjects(&array));
    }

    #[test]
    fn isolated_pasteboard_keeps_rich_binary_types_and_counters() {
        autoreleasepool(|_| {
            let pasteboard = named_pasteboard("com.golos.dictation.test.rich");
            write_one_item(
                &pasteboard,
                &[
                    ("public.utf8-plain-text", b"hello"),
                    ("public.tiff", &[0, 1, 2, 255]),
                ],
            );
            let before = pasteboard.changeCount() as i64;
            let snapshot = snapshot_pasteboard(&pasteboard, ClipboardLimits::default()).unwrap();
            assert_eq!(snapshot.items.len(), 1);
            assert_eq!(snapshot.items[0].types.len(), 2);
            assert_eq!(snapshot.items[0].types[0].data, b"hello".to_vec());
            assert_eq!(snapshot.items[0].types[1].data, vec![0, 1, 2, 255]);
            assert!(snapshot.complete);

            let written = write_transcript(&pasteboard, "transcript").unwrap();
            assert_ne!(written, before);
            assert_eq!(
                pasteboard
                    .stringForType(unsafe { NSPasteboardTypeString })
                    .unwrap()
                    .to_string(),
                "transcript"
            );
            // The transcript replaced the clipboard, so the captured count no
            // longer matches and a restore is only allowed for our own write.
            let mut transaction =
                ClipboardTransaction::prepare(before, true, Some(snapshot.clone()));
            transaction.note_written(written);
            assert_eq!(transaction.authorize_paste(written), Ok(()));
            assert_eq!(
                transaction.restore_decision(written),
                RestoreDecision::Restore
            );
            restore_pasteboard(&pasteboard, &snapshot).unwrap();
            let restored = pasteboard.pasteboardItems().unwrap();
            assert_eq!(restored.len(), 1);
            assert_eq!(restored.iter().next().unwrap().types().len(), 2);
        });
    }

    #[test]
    fn isolated_pasteboard_keeps_multiple_items_and_empty_state() {
        autoreleasepool(|_| {
            let pasteboard = named_pasteboard("com.golos.dictation.test.multiple");
            let _ = pasteboard.clearContents();
            let snapshot = snapshot_pasteboard(&pasteboard, ClipboardLimits::default()).unwrap();
            assert!(snapshot.complete);
            assert!(snapshot.items.is_empty());

            write_two_items(&pasteboard);
            let snapshot = snapshot_pasteboard(&pasteboard, ClipboardLimits::default()).unwrap();
            assert_eq!(snapshot.items.len(), 2);
            assert_eq!(snapshot.items[0].types.len(), 1);
            assert_eq!(snapshot.items[1].types[0].data, vec![9, 8, 7]);
            restore_pasteboard(&pasteboard, &snapshot).unwrap();
            assert_eq!(pasteboard.pasteboardItems().unwrap().len(), 2);
        });
    }

    #[test]
    fn isolated_pasteboard_foreign_copy_wins_over_restore() {
        autoreleasepool(|_| {
            let pasteboard = named_pasteboard("com.golos.dictation.test.foreign");
            write_one_item(&pasteboard, &[("public.utf8-plain-text", b"original")]);
            let before = pasteboard.changeCount() as i64;
            let snapshot = snapshot_pasteboard(&pasteboard, ClipboardLimits::default()).unwrap();
            let written = write_transcript(&pasteboard, "transcript").unwrap();
            let mut transaction = ClipboardTransaction::prepare(before, true, Some(snapshot));
            transaction.note_written(written);

            // A user copy between our write and the delayed restore wins.
            let _ = pasteboard.clearContents();
            assert!(
                pasteboard.setString_forType(&NSString::from_str("foreign"), unsafe {
                    NSPasteboardTypeString
                })
            );
            let foreign = pasteboard.changeCount() as i64;
            assert_ne!(foreign, written);
            assert_eq!(
                transaction.restore_decision(foreign),
                RestoreDecision::KeepNewerClipboard
            );
            // The restore worker skips because the count is no longer ours.
            assert_ne!(pasteboard.changeCount() as i64, written);
        });
    }

    #[test]
    fn isolated_pasteboard_memory_cap_fails_closed_before_write() {
        autoreleasepool(|_| {
            let pasteboard = named_pasteboard("com.golos.dictation.test.cap");
            write_one_item(&pasteboard, &[("com.example.binary", &[0; 64])]);
            let result = snapshot_pasteboard(
                &pasteboard,
                ClipboardLimits {
                    max_bytes: 8,
                    max_items: 4,
                    max_types_per_item: 4,
                },
            );
            assert_eq!(result, Err(SnapshotError::TooLarge));
            // Without a complete snapshot, a restore-enabled insertion refuses
            // before the clipboard is touched.
            let transaction =
                ClipboardTransaction::prepare(pasteboard.changeCount() as i64, true, None);
            assert_eq!(
                transaction.authorize_write(pasteboard.changeCount() as i64),
                Err(InsertRefusal::ClipboardUnavailable)
            );
        });
    }
}
