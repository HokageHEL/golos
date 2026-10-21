//! Shortcut registration lifecycle, kept separate from the state machine so the
//! new-before-old and failure-retains-old rules are testable without a desktop.
use crate::dictation::shortcut::ValidatedShortcut;
#[cfg(target_os = "macos")]
use tauri::{AppHandle, Manager};
#[cfg(target_os = "macos")]
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutEvent};

pub trait Registrar: Send + Sync {
    fn register(&self, shortcut: &ValidatedShortcut) -> Result<(), String>;
    fn unregister(&self, shortcut: &ValidatedShortcut) -> Result<(), String>;
}

/// The currently working shortcut plus the last visible failure. A failed
/// candidate never replaces the working registration.
#[derive(Debug, Default)]
pub struct Registration {
    current: Option<ValidatedShortcut>,
    last_error: Option<String>,
    startup_failed: bool,
}

impl Registration {
    pub fn current(&self) -> Option<&ValidatedShortcut> {
        self.current.as_ref()
    }
    pub fn error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
    pub fn startup_failed(&self) -> bool {
        self.startup_failed
    }

    /// Register the candidate first. Only after the OS accepted it is the old
    /// shortcut removed, so a conflict leaves the user with a working hotkey.
    pub fn apply(
        &mut self,
        candidate: ValidatedShortcut,
        registrar: &dyn Registrar,
    ) -> Result<(), String> {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.shortcut.id() == candidate.shortcut.id())
        {
            self.last_error = None;
            return Ok(());
        }
        if let Err(error) = registrar.register(&candidate) {
            self.startup_failed = false;
            self.last_error = Some(error.clone());
            return Err(error);
        }
        if let Some(previous) = self.current.replace(candidate) {
            // Rollback is best-effort: the new shortcut already works, and the
            // plugin treats a stale extra registration as a conflict to report.
            if let Err(error) = registrar.unregister(&previous) {
                self.last_error = Some(format!(
                    "The new shortcut is active, but the previous one could not be removed: {error}"
                ));
            }
        }
        Ok(())
    }

    /// Startup registration keeps failures visible instead of pretending the
    /// feature is armed.
    pub fn start(
        &mut self,
        candidate: ValidatedShortcut,
        registrar: &dyn Registrar,
    ) -> Result<(), String> {
        match self.apply(candidate, registrar) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.startup_failed = true;
                self.last_error = Some(format!(
                    "Global dictation shortcut could not be registered at startup: {error}"
                ));
                Err(self.last_error.clone().unwrap_or(error))
            }
        }
    }

    pub fn clear(&mut self, registrar: &dyn Registrar) {
        if let Some(current) = self.current.take() {
            if let Err(error) = registrar.unregister(&current) {
                self.last_error =
                    Some(format!("The dictation shortcut could not be removed: {error}"));
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub struct PluginRegistrar {
    app: AppHandle,
}

#[cfg(target_os = "macos")]
impl PluginRegistrar {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[cfg(target_os = "macos")]
impl Registrar for PluginRegistrar {
    fn register(&self, shortcut: &ValidatedShortcut) -> Result<(), String> {
        // The plugin owns the handler installed in `lib::run`; this call only
        // registers the combination, which the OS requires on the main thread.
        self.app
            .global_shortcut()
            .register(shortcut.shortcut)
            .map_err(|error| format!("{error}"))
    }
    fn unregister(&self, shortcut: &ValidatedShortcut) -> Result<(), String> {
        self.app
            .global_shortcut()
            .unregister(shortcut.shortcut)
            .map_err(|error| format!("{error}"))
    }
}

/// Plugin handler for every registered shortcut. It only validates the edge,
/// runs the latch and enqueues work on a worker; it deliberately performs no AX,
/// database or recording access on the main thread.
#[cfg(target_os = "macos")]
pub fn plugin_handler(app: &AppHandle, event: &ShortcutEvent) {
    use crate::dictation::KeyEdge;
    let Some(state) = app.try_state::<crate::AppState>() else {
        return;
    };
    state
        .dictation
        .shortcut_edge(match event.state {
            tauri_plugin_global_shortcut::ShortcutState::Pressed => KeyEdge::Pressed,
            tauri_plugin_global_shortcut::ShortcutState::Released => KeyEdge::Released,
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Fake {
        calls: Mutex<Vec<String>>,
        fail_register: Mutex<Option<String>>,
    }
    impl Registrar for Fake {
        fn register(&self, shortcut: &ValidatedShortcut) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("register:{}", shortcut.canonical));
            if let Some(error) = self.fail_register.lock().unwrap().take() {
                return Err(error);
            }
            Ok(())
        }
        fn unregister(&self, shortcut: &ValidatedShortcut) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("unregister:{}", shortcut.canonical));
            Ok(())
        }
    }
    #[cfg(target_os = "macos")]
    fn validated(value: &str) -> ValidatedShortcut {
        crate::dictation::shortcut::validate(value).unwrap()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn new_shortcut_is_registered_before_the_old_one_is_removed() {
        let fake = Fake::default();
        let mut registration = Registration::default();
        registration
            .apply(validated("Cmd+Shift+Space"), &fake)
            .unwrap();
        registration
            .apply(validated("Ctrl+Option+F5"), &fake)
            .unwrap();
        let calls = fake.calls.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                "register:shift+super+Space",
                "register:control+alt+F5",
                "unregister:shift+super+Space",
            ]
        );
        assert_eq!(
            registration.current().unwrap().display,
            "⌃⌥F5".to_string()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn failed_candidate_keeps_the_previous_shortcut_and_reports_the_failure() {
        let fake = Fake::default();
        let mut registration = Registration::default();
        registration
            .start(validated("Cmd+Shift+Space"), &fake)
            .unwrap();
        *fake.fail_register.lock().unwrap() = Some("already in use".into());
        let error = registration
            .apply(validated("Cmd+Shift+D"), &fake)
            .unwrap_err();
        assert!(error.contains("already in use"));
        assert_eq!(
            registration.current().unwrap().shortcut.id(),
            validated("Cmd+Shift+Space").shortcut.id()
        );
        assert!(!fake
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|call| call.starts_with("unregister")));
        assert!(registration.error().unwrap().contains("already in use"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn startup_failure_is_visible_and_clearing_unregisters_once() {
        let fake = Fake::default();
        *fake.fail_register.lock().unwrap() = Some("conflict".into());
        let mut registration = Registration::default();
        assert!(registration
            .start(validated("Cmd+Shift+Space"), &fake)
            .is_err());
        assert!(registration.startup_failed());
        assert!(registration.current().is_none());
        registration.clear(&fake);
        *fake.fail_register.lock().unwrap() = None;
        registration
            .start(validated("Cmd+Shift+Space"), &fake)
            .unwrap();
        registration.clear(&fake);
        assert!(registration.current().is_none());
        let calls = fake.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 3);
    }
}
