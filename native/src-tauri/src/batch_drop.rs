//! Bounded, single-use grants for files the user drops on the native window.
//!
//! The webview never receives a filesystem path. Dropping files only creates a
//! short-lived grant in Rust; the frontend sees sanitized file names and later
//! asks to import the grant by its random id. A grant is consumed exactly once,
//! so a stale or replayed id can never import another selection.
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

/// The same per-import bound as the native picker. The durable queue itself can
/// hold 1,000 pending jobs.
pub const MAX_FILES: usize = 100;
/// Long enough to choose a language, short enough that an abandoned selection
/// does not linger for the rest of the session.
pub const GRANT_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_NAME_CHARS: usize = 255;

/// Sanitized preview sent to the webview: a random grant id and file names only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropPreview {
    pub id: String,
    pub names: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DropRejected {
    pub message: String,
}

struct Grant {
    id: String,
    paths: Vec<PathBuf>,
    expires_at: Instant,
}

/// Holds at most one unconsumed drop. A new idle drop replaces the previous
/// grant; consuming, discarding or expiring it removes it.
#[derive(Default)]
pub struct DropStore {
    grant: Mutex<Option<Grant>>,
}

impl DropStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn replace(&self, paths: Vec<PathBuf>) -> Result<DropPreview, String> {
        self.replace_at(paths, Instant::now())
    }

    fn replace_at(&self, paths: Vec<PathBuf>, now: Instant) -> Result<DropPreview, String> {
        if paths.is_empty() {
            return Err("Drop one or more local audio files.".into());
        }
        if paths.len() > MAX_FILES {
            return Err(format!(
                "Drop up to {MAX_FILES} files at a time. The queue can hold 1,000 pending recordings."
            ));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let preview = DropPreview {
            id: id.clone(),
            names: paths.iter().map(|path| display_name(path)).collect(),
        };
        *self
            .grant
            .lock()
            .map_err(|_| "Dropped files are unavailable. Restart Golos.".to_string())? =
            Some(Grant {
                id,
                paths,
                expires_at: now + GRANT_TTL,
            });
        Ok(preview)
    }

    /// Consume the matching grant. A stale id leaves the current grant intact.
    pub fn take(&self, id: &str) -> Result<Vec<PathBuf>, String> {
        self.take_at(id, Instant::now())
    }

    fn take_at(&self, id: &str, now: Instant) -> Result<Vec<PathBuf>, String> {
        let mut slot = self
            .grant
            .lock()
            .map_err(|_| "Dropped files are unavailable. Restart Golos.".to_string())?;
        if matches!(slot.as_ref(), Some(grant) if grant.expires_at <= now) {
            *slot = None;
            return Err("The dropped-file selection expired. Drop the files again.".into());
        }
        let Some(grant) = slot.as_ref() else {
            return Err("No dropped files are ready. Drop files again.".into());
        };
        if grant.id != id {
            return Err(
                "This dropped-file selection is no longer current. Drop the files again.".into(),
            );
        }
        let grant = slot.take().expect("checked drop grant");
        Ok(grant.paths)
    }

    /// Invalidate a selection by id. Returns whether a matching grant existed.
    pub fn discard(&self, id: &str) -> bool {
        let Ok(mut slot) = self.grant.lock() else {
            return false;
        };
        if matches!(slot.as_ref(), Some(grant) if grant.id == id) {
            *slot = None;
            true
        } else {
            false
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.grant.lock().unwrap().is_none()
    }
}

/// Emit only a file name, never the directory. Control characters are replaced
/// so a hostile file name cannot forge lines or terminal output in the UI.
pub fn display_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Unnamed file".into());
    name.chars()
        .map(|ch| if ch.is_control() { '�' } else { ch })
        .take(MAX_NAME_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|index| PathBuf::from(format!("/private/drop/{index}.wav")))
            .collect()
    }

    #[test]
    fn grants_are_sanitized_single_use_and_leave_the_current_selection_on_a_stale_id() {
        let store = DropStore::new();
        let preview = store
            .replace(vec![
                PathBuf::from("/private/Interviews/One.wav"),
                PathBuf::from("/private/Two.mp3"),
            ])
            .unwrap();
        assert_eq!(preview.names, ["One.wav", "Two.mp3"]);
        assert!(preview.names.iter().all(|name| !name.contains("/private")));

        // A stale or replayed id must not consume the current grant.
        assert!(store
            .take("stale-id")
            .unwrap_err()
            .contains("no longer current"));
        assert!(!store.is_empty());
        assert_eq!(store.take(&preview.id).unwrap().len(), 2);
        assert!(store.is_empty());
        assert!(store
            .take(&preview.id)
            .unwrap_err()
            .contains("No dropped files"));
    }

    #[test]
    fn expired_grants_are_cleared_and_discard_is_id_scoped() {
        let store = DropStore::new();
        let now = Instant::now();
        let expired = store.replace_at(paths(1), now).unwrap();
        assert!(store
            .take_at(&expired.id, now + GRANT_TTL + Duration::from_secs(1))
            .unwrap_err()
            .contains("expired"));
        assert!(store.is_empty());

        let live = store.replace_at(paths(1), now).unwrap();
        assert!(!store.discard("different-id"));
        assert!(!store.is_empty());
        assert!(store.discard(&live.id));
        assert!(store.is_empty());
        assert!(!store.discard(&live.id));
    }

    #[test]
    fn selection_size_is_bounded_and_invalid_drops_keep_the_previous_grant() {
        let store = DropStore::new();
        let now = Instant::now();
        let preview = store.replace_at(paths(MAX_FILES), now).unwrap();
        assert_eq!(preview.names.len(), MAX_FILES);
        assert!(store
            .replace_at(paths(MAX_FILES + 1), now)
            .unwrap_err()
            .contains("100"));
        assert!(store.replace_at(vec![], now).is_err());
        // Failed replacements never destroy the unconsumed selection.
        assert!(!store.is_empty());
        assert_eq!(store.take_at(&preview.id, now).unwrap().len(), MAX_FILES);
    }

    #[test]
    fn control_characters_in_names_are_replaced_and_names_are_bounded() {
        let store = DropStore::new();
        let long = format!("file\n\u{1b}{}", "a".repeat(300));
        let preview = store
            .replace(vec![PathBuf::from(format!("/tmp/{long}"))])
            .unwrap();
        let name = &preview.names[0];
        assert_eq!(name.chars().count(), MAX_NAME_CHARS);
        assert!(name.starts_with("file��"));
        assert!(!name.chars().any(char::is_control));
    }
}
