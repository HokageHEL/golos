//! Shared safety and state contracts for the macOS dictation implementation.
//! This module alone does not register a shortcut, capture audio or insert text.
use serde::{Deserialize, Serialize};

pub mod platform;
#[cfg(target_os = "macos")]
pub mod shortcut;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Idle,
    Starting,
    Recording,
    Paused,
    Stopped,
    Saving,
    SaveFailed,
    Transcribing,
    Processing,
    Preview,
    Inserting,
    PasteRequested,
    Failed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutMode {
    Toggle,
    Hold,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEdge {
    Pressed,
    Released,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutAction {
    None,
    Start,
    StopAndSave,
    ShowReview,
    Busy,
}
/// One latch per registered shortcut. OS key repeat must never stop a capture or
/// dispatch a second paste. The runtime resets the latch when changing shortcuts.
#[derive(Debug, Default)]
pub struct ShortcutLatch {
    pressed: bool,
}
impl ShortcutLatch {
    pub fn edge(
        &mut self,
        edge: KeyEdge,
        mode: ShortcutMode,
        phase: Phase,
        another_operation: bool,
    ) -> ShortcutAction {
        match edge {
            KeyEdge::Pressed if self.pressed => ShortcutAction::None,
            KeyEdge::Released if !self.pressed => ShortcutAction::None,
            KeyEdge::Released => {
                self.pressed = false;
                if mode == ShortcutMode::Hold
                    && !another_operation
                    && matches!(
                        phase,
                        Phase::Starting | Phase::Recording | Phase::Paused | Phase::Stopped
                    )
                {
                    ShortcutAction::StopAndSave
                } else {
                    ShortcutAction::None
                }
            }
            KeyEdge::Pressed => {
                self.pressed = true;
                if another_operation {
                    return ShortcutAction::Busy;
                }
                match phase {
                    Phase::Idle => ShortcutAction::Start,
                    Phase::Recording | Phase::Paused | Phase::Stopped
                        if mode == ShortcutMode::Toggle =>
                    {
                        ShortcutAction::StopAndSave
                    }
                    Phase::Preview | Phase::PasteRequested => ShortcutAction::ShowReview,
                    _ => ShortcutAction::Busy,
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetApp {
    pub pid: i32,
    pub bundle_id: String,
    pub name: String,
    pub launch_time_ms: i64,
}
/// Target tokens refer to native-owned retained app/AX objects, never a PID
/// supplied by the webview. They expire on restart or explicit target dismissal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturedTarget {
    pub token: String,
    pub app: TargetApp,
    pub insertion_available: bool,
    pub refusal: Option<InsertRefusal>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InsertRefusal {
    AccessibilityDenied,
    PostEventDenied,
    TargetUnavailable,
    SelfTarget,
    SecureField,
    NoEditableFocus,
    ApplicationChanged,
    FocusChanged,
    SelectionChanged,
    ClipboardUnavailable,
    ClipboardChanged,
    AlreadyDispatched,
}
/// Native code compares the retained NSRunningApplication and AXUIElement, not
/// just the PID/name/role. A new caret/selection requires a new explicit target.
/// Never read AXValue or snapshot clipboard data in a secure field.
#[derive(Debug, Clone, Copy)]
pub struct FocusEvidence {
    pub accessibility: bool,
    pub post_events: bool,
    pub target_alive: bool,
    pub self_target: bool,
    pub secure: bool,
    pub editable: bool,
    pub same_application: bool,
    pub same_element: bool,
    pub same_selection: bool,
}
pub fn insertion_refusal(focus: FocusEvidence) -> Option<InsertRefusal> {
    if !focus.accessibility {
        Some(InsertRefusal::AccessibilityDenied)
    } else if !focus.post_events {
        Some(InsertRefusal::PostEventDenied)
    } else if !focus.target_alive {
        Some(InsertRefusal::TargetUnavailable)
    } else if focus.self_target {
        Some(InsertRefusal::SelfTarget)
    } else if focus.secure {
        Some(InsertRefusal::SecureField)
    } else if !focus.editable {
        Some(InsertRefusal::NoEditableFocus)
    } else if !focus.same_application {
        Some(InsertRefusal::ApplicationChanged)
    } else if !focus.same_element {
        Some(InsertRefusal::FocusChanged)
    } else if !focus.same_selection {
        Some(InsertRefusal::SelectionChanged)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreDecision {
    Restore,
    KeepNewerClipboard,
    KeepTranscript,
}
/// Pure ownership checks used around the native pasteboard transaction. A
/// snapshot must include every item/type, including binary rich content. If a
/// promised type cannot be read or the configured memory cap is exceeded,
/// insertion is refused before touching the general pasteboard. Explicit Copy
/// remains a separate user action that intentionally replaces clipboard data.
#[derive(Debug, Clone, Copy)]
pub struct ClipboardLease {
    pub original_count: i64,
    pub written_count: i64,
    pub complete_snapshot: bool,
    pub restore_previous: bool,
}
impl ClipboardLease {
    pub fn before_write(&self, current_count: i64) -> Result<(), InsertRefusal> {
        if !self.complete_snapshot && self.restore_previous {
            Err(InsertRefusal::ClipboardUnavailable)
        } else if current_count != self.original_count {
            Err(InsertRefusal::ClipboardChanged)
        } else {
            Ok(())
        }
    }
    pub fn before_paste(&self, current_count: i64) -> Result<(), InsertRefusal> {
        if current_count == self.written_count {
            Ok(())
        } else {
            Err(InsertRefusal::ClipboardChanged)
        }
    }
    pub fn restore(&self, current_count: i64) -> RestoreDecision {
        if current_count != self.written_count {
            RestoreDecision::KeepNewerClipboard
        } else if self.restore_previous && self.complete_snapshot {
            RestoreDecision::Restore
        } else {
            RestoreDecision::KeepTranscript
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ClipboardOutcome {
    Unchanged,
    Restored,
    KeptNewerContent,
    TranscriptRetained,
    RestoreFailed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum InsertOutcome {
    /// Public event APIs provide no target-consumption acknowledgement. The UI
    /// must say "Paste requested; verify the target app", never "Inserted".
    PasteRequested {
        clipboard: ClipboardOutcome,
        message: String,
    },
    Refused {
        reason: InsertRefusal,
        message: String,
    },
    Failed {
        clipboard: ClipboardOutcome,
        message: String,
    },
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permissions {
    pub accessibility: bool,
    pub post_events: bool,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionKind {
    Accessibility,
    PostEvents,
}
/// Blocking native adapter. Implementations dispatch AppKit work to the main
/// thread, bound AX timeouts, and never require a grant to record/transcribe.
/// Permissions are only prompted by explicit request_permission calls.
pub trait Platform: Send + Sync {
    fn permissions(&self) -> Result<Permissions, String>;
    fn request_permission(&self, kind: PermissionKind) -> Result<Permissions, String>;
    fn open_permission_settings(&self, kind: PermissionKind) -> Result<(), String>;
    fn capture_target(&self) -> Result<CapturedTarget, String>;
    fn release_target(&self, token: &str);
    /// Called only after explicit preview confirmation. Revalidate the captured
    /// app/element/selection immediately before writing and before posting to PID.
    /// Repeated request_id values must never dispatch another paste.
    fn insert(
        &self,
        target: &str,
        request_id: &str,
        text: &str,
        restore_previous_clipboard: bool,
    ) -> Result<InsertOutcome, String>;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn good_focus() -> FocusEvidence {
        FocusEvidence {
            accessibility: true,
            post_events: true,
            target_alive: true,
            self_target: false,
            secure: false,
            editable: true,
            same_application: true,
            same_element: true,
            same_selection: true,
        }
    }
    #[test]
    fn toggle_shortcut_ignores_repeat_and_cannot_operate_someone_elses_capture() {
        let mut latch = ShortcutLatch::default();
        assert_eq!(
            latch.edge(KeyEdge::Pressed, ShortcutMode::Toggle, Phase::Idle, false),
            ShortcutAction::Start
        );
        assert_eq!(
            latch.edge(
                KeyEdge::Pressed,
                ShortcutMode::Toggle,
                Phase::Recording,
                false
            ),
            ShortcutAction::None
        );
        assert_eq!(
            latch.edge(
                KeyEdge::Released,
                ShortcutMode::Toggle,
                Phase::Recording,
                false
            ),
            ShortcutAction::None
        );
        assert_eq!(
            latch.edge(KeyEdge::Pressed, ShortcutMode::Toggle, Phase::Paused, false),
            ShortcutAction::StopAndSave
        );
        latch.edge(
            KeyEdge::Released,
            ShortcutMode::Toggle,
            Phase::Saving,
            false,
        );
        assert_eq!(
            latch.edge(
                KeyEdge::Pressed,
                ShortcutMode::Toggle,
                Phase::Recording,
                true
            ),
            ShortcutAction::Busy
        );
        assert_eq!(
            latch.edge(
                KeyEdge::Released,
                ShortcutMode::Hold,
                Phase::Recording,
                true
            ),
            ShortcutAction::None
        );
    }
    #[test]
    fn hold_shortcut_stops_once_even_when_capture_start_is_still_pending() {
        let mut latch = ShortcutLatch::default();
        assert_eq!(
            latch.edge(KeyEdge::Released, ShortcutMode::Hold, Phase::Idle, false),
            ShortcutAction::None
        );
        assert_eq!(
            latch.edge(KeyEdge::Pressed, ShortcutMode::Hold, Phase::Idle, false),
            ShortcutAction::Start
        );
        assert_eq!(
            latch.edge(
                KeyEdge::Released,
                ShortcutMode::Hold,
                Phase::Starting,
                false
            ),
            ShortcutAction::StopAndSave
        );
        assert_eq!(
            latch.edge(
                KeyEdge::Released,
                ShortcutMode::Hold,
                Phase::Starting,
                false
            ),
            ShortcutAction::None
        );
        // A short tap still saves; there is no automatic discard of user audio.
    }
    #[test]
    fn shortcut_cannot_insert_discard_or_replace_a_pending_review() {
        for mode in [ShortcutMode::Toggle, ShortcutMode::Hold] {
            for phase in [
                Phase::SaveFailed,
                Phase::Saving,
                Phase::Transcribing,
                Phase::Processing,
                Phase::Inserting,
                Phase::Failed,
            ] {
                let mut latch = ShortcutLatch::default();
                assert_eq!(
                    latch.edge(KeyEdge::Pressed, mode, phase, false),
                    ShortcutAction::Busy
                );
                assert_eq!(
                    latch.edge(KeyEdge::Released, mode, phase, false),
                    ShortcutAction::None
                );
            }
            for phase in [Phase::Preview, Phase::PasteRequested] {
                let mut latch = ShortcutLatch::default();
                assert_eq!(
                    latch.edge(KeyEdge::Pressed, mode, phase, false),
                    ShortcutAction::ShowReview
                );
                assert_eq!(
                    latch.edge(KeyEdge::Released, mode, phase, false),
                    ShortcutAction::None
                );
            }
        }
    }
    #[test]
    fn insertion_requires_permissions_and_the_original_nonsecure_editable_target() {
        assert_eq!(insertion_refusal(good_focus()), None);
        for (focus, reason) in [
            (
                FocusEvidence {
                    accessibility: false,
                    ..good_focus()
                },
                InsertRefusal::AccessibilityDenied,
            ),
            (
                FocusEvidence {
                    post_events: false,
                    ..good_focus()
                },
                InsertRefusal::PostEventDenied,
            ),
            (
                FocusEvidence {
                    target_alive: false,
                    ..good_focus()
                },
                InsertRefusal::TargetUnavailable,
            ),
            (
                FocusEvidence {
                    self_target: true,
                    ..good_focus()
                },
                InsertRefusal::SelfTarget,
            ),
            (
                FocusEvidence {
                    secure: true,
                    ..good_focus()
                },
                InsertRefusal::SecureField,
            ),
            (
                FocusEvidence {
                    editable: false,
                    ..good_focus()
                },
                InsertRefusal::NoEditableFocus,
            ),
            (
                FocusEvidence {
                    same_application: false,
                    ..good_focus()
                },
                InsertRefusal::ApplicationChanged,
            ),
            (
                FocusEvidence {
                    same_element: false,
                    ..good_focus()
                },
                InsertRefusal::FocusChanged,
            ),
            (
                FocusEvidence {
                    same_selection: false,
                    ..good_focus()
                },
                InsertRefusal::SelectionChanged,
            ),
        ] {
            assert_eq!(insertion_refusal(focus), Some(reason));
        }
    }
    #[test]
    fn clipboard_snapshot_and_restore_never_overwrite_a_concurrent_copy() {
        let lease = ClipboardLease {
            original_count: 10,
            written_count: 11,
            complete_snapshot: true,
            restore_previous: true,
        };
        assert_eq!(lease.before_write(10), Ok(()));
        assert_eq!(lease.before_write(11), Err(InsertRefusal::ClipboardChanged));
        assert_eq!(lease.before_paste(11), Ok(()));
        assert_eq!(lease.before_paste(12), Err(InsertRefusal::ClipboardChanged));
        assert_eq!(lease.restore(11), RestoreDecision::Restore);
        assert_eq!(lease.restore(12), RestoreDecision::KeepNewerClipboard);
        // A foreign write followed by a later foreign write remains foreign.
        assert_eq!(lease.restore(13), RestoreDecision::KeepNewerClipboard);
        let lossy = ClipboardLease {
            complete_snapshot: false,
            ..lease
        };
        assert_eq!(
            lossy.before_write(10),
            Err(InsertRefusal::ClipboardUnavailable)
        );
        assert_eq!(lossy.restore(11), RestoreDecision::KeepTranscript);
        let explicit_no_restore = ClipboardLease {
            restore_previous: false,
            ..lossy
        };
        assert_eq!(explicit_no_restore.before_write(10), Ok(()));
        assert_eq!(
            explicit_no_restore.restore(11),
            RestoreDecision::KeepTranscript
        );
        assert_eq!(
            explicit_no_restore.restore(12),
            RestoreDecision::KeepNewerClipboard
        );
    }
}
