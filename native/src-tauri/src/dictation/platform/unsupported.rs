//! Explicit unsupported-platform adapter.
//!
//! There is no fake target capture and no clipboard fallback here: the platform
//! trait methods all fail so orchestration can keep recording/transcribing
//! offline while telling the user insertion is unavailable on this OS.

use tauri::AppHandle;

use crate::dictation::{CapturedTarget, InsertOutcome, PermissionKind, Permissions, Platform};

const UNSUPPORTED: &str = "Native dictation insertion is only available on macOS.";

pub struct MacPlatform {
    _app: Option<AppHandle>,
}

impl MacPlatform {
    pub fn new(app: AppHandle) -> Self {
        Self { _app: Some(app) }
    }

    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self { _app: None }
    }
}

struct AssertSendSync<T: Send + Sync>(std::marker::PhantomData<T>);
const _: AssertSendSync<MacPlatform> = AssertSendSync(std::marker::PhantomData);

impl Platform for MacPlatform {
    fn permissions(&self) -> Result<Permissions, String> {
        Err(UNSUPPORTED.into())
    }

    fn request_permission(&self, _kind: PermissionKind) -> Result<Permissions, String> {
        Err(UNSUPPORTED.into())
    }

    fn open_permission_settings(&self, _kind: PermissionKind) -> Result<(), String> {
        Err(UNSUPPORTED.into())
    }

    fn capture_target(&self) -> Result<CapturedTarget, String> {
        Err(UNSUPPORTED.into())
    }

    fn release_target(&self, _token: &str) {}

    fn insert(
        &self,
        _target: &str,
        _request_id: &str,
        _text: &str,
        _restore_previous_clipboard: bool,
    ) -> Result<InsertOutcome, String> {
        Err(UNSUPPORTED.into())
    }
}
