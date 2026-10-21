//! Validated, persisted configuration for the opt-in global dictation feature.
//!
//! Everything here is pure policy plus a small JSON row. Native shortcut
//! registration, recording and insertion stay in `dictation_runtime`; this
//! module never talks to AppKit or the microphone. Defaults keep the whole
//! feature off, including local AI processing.
use crate::dictation::ShortcutMode;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SHORTCUT_LIMIT: usize = 100;
pub const PROMPT_LIMIT: usize = 4_000;
pub const BUNDLE_LIMIT: usize = 255;
pub const OVERRIDE_LIMIT: usize = 100;
pub const DEFAULT_PROCESS_PROMPT: &str =
    "Fix punctuation, capitalization and obvious filler words. Keep the original language and meaning. Return only the corrected text.";
pub const PROCESS_KINDS: &[&str] = &["cleanup", "custom"];

#[cfg(target_os = "macos")]
pub fn default_shortcut() -> String {
    crate::dictation::shortcut::DEFAULT_SHORTCUT.into()
}
#[cfg(not(target_os = "macos"))]
pub fn default_shortcut() -> String {
    String::new()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AppOverride {
    pub bundle_id: String,
    pub ai_enabled: Option<bool>,
    pub ai_kind: Option<String>,
    pub ai_prompt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub shortcut: String,
    pub mode: ShortcutMode,
    pub language: String,
    pub show_overlay: bool,
    pub overlay_always_on_top: bool,
    pub restore_clipboard: bool,
    pub ai_enabled: bool,
    pub ai_kind: String,
    pub ai_prompt: String,
    pub app_overrides: Vec<AppOverride>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: default_shortcut(),
            mode: ShortcutMode::Toggle,
            language: "auto".into(),
            show_overlay: true,
            overlay_always_on_top: true,
            restore_clipboard: true,
            ai_enabled: false,
            ai_kind: "cleanup".into(),
            ai_prompt: DEFAULT_PROCESS_PROMPT.into(),
            app_overrides: vec![],
        }
    }
}

impl Settings {
    /// Bounded, non-secret validation. The shortcut is only fully parsed where
    /// the native plugin exists; other targets still reject malformed text so a
    /// later platform port cannot inherit an unbounded value.
    pub fn validate(&self) -> Result<(), String> {
        if self.shortcut.len() > SHORTCUT_LIMIT || self.shortcut.contains('\0') {
            return Err("Use a shortcut of up to 100 bytes, such as Command+Shift+Space.".into());
        }
        if self.language.len() > 16 || self.language.contains('\0') || self.language.is_empty() {
            return Err("Choose a supported dictation language.".into());
        }
        if !PROCESS_KINDS.contains(&self.ai_kind.as_str()) {
            return Err("Choose cleanup or a custom local prompt for dictation processing.".into());
        }
        if self.ai_prompt.len() > PROMPT_LIMIT || self.ai_prompt.contains('\0') {
            return Err("Keep the dictation prompt under 4,000 bytes.".into());
        }
        if self.app_overrides.len() > OVERRIDE_LIMIT {
            return Err("Keep at most 100 per-app dictation overrides.".into());
        }
        let mut seen = std::collections::HashSet::new();
        for item in &self.app_overrides {
            if item.bundle_id.trim().is_empty()
                || item.bundle_id.len() > BUNDLE_LIMIT
                || item.bundle_id.contains('\0')
                || !seen.insert(item.bundle_id.as_str())
            {
                return Err(
                    "Each app override needs a unique bundle ID of up to 255 bytes.".into(),
                );
            }
            if let Some(kind) = &item.ai_kind {
                if !PROCESS_KINDS.contains(&kind.as_str()) {
                    return Err(
                        "Per-app processing must be cleanup or a custom local prompt.".into()
                    );
                }
            }
            if let Some(prompt) = &item.ai_prompt {
                if prompt.len() > PROMPT_LIMIT || prompt.contains('\0') {
                    return Err("Keep each per-app prompt under 4,000 bytes.".into());
                }
            }
        }
        if self.enabled {
            #[cfg(target_os = "macos")]
            {
                crate::dictation::shortcut::validate(&self.shortcut)?;
            }
            #[cfg(not(target_os = "macos"))]
            {
                return Err("Global dictation shortcuts are available only on macOS.".into());
            }
        }
        Ok(())
    }
}

/// The effective local-processing policy after applying an exact bundle-ID
/// override. No fuzzy matching and no network/cloud tags: the prompt is data,
/// never executable text or a URL.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessingPolicy {
    pub ai_enabled: bool,
    pub ai_kind: String,
    pub ai_prompt: String,
}

pub fn resolve_processing(settings: &Settings, bundle_id: Option<&str>) -> ProcessingPolicy {
    let mut policy = ProcessingPolicy {
        ai_enabled: settings.ai_enabled,
        ai_kind: settings.ai_kind.clone(),
        ai_prompt: settings.ai_prompt.clone(),
    };
    let Some(bundle_id) = bundle_id else {
        return policy;
    };
    let Some(item) = settings
        .app_overrides
        .iter()
        .find(|item| item.bundle_id == bundle_id)
    else {
        return policy;
    };
    if let Some(enabled) = item.ai_enabled {
        policy.ai_enabled = enabled;
    }
    if let Some(kind) = &item.ai_kind {
        policy.ai_kind = kind.clone();
    }
    if let Some(prompt) = &item.ai_prompt {
        policy.ai_prompt = prompt.clone();
    }
    policy
}

/// Read the single settings row. Missing or blank JSON is the opt-in defaults;
/// oversized or malformed JSON is a visible error instead of silently enabling
/// or disabling a feature.
pub fn load(db: &Connection) -> Result<Settings, String> {
    let json: String = db
        .query_row("SELECT json FROM dictation_settings WHERE id=1", [], |row| {
            row.get(0)
        })
        .map_err(|e| format!("Could not read dictation settings: {e}"))?;
    if json.len() > 100_000 {
        return Err("Saved dictation settings are oversized. Reset them in Settings.".into());
    }
    if json.trim().is_empty() || json.trim() == "{}" {
        return Ok(Settings::default());
    }
    let settings: Settings = serde_json::from_str(&json)
        .map_err(|e| format!("Saved dictation settings are damaged: {e}"))?;
    settings.validate()?;
    Ok(settings)
}

pub fn save(db: &Connection, settings: &Settings) -> Result<(), String> {
    settings.validate()?;
    let json = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    db.execute(
        "INSERT INTO dictation_settings(id,json,updated_at) VALUES(1,?1,?2)
         ON CONFLICT(id) DO UPDATE SET json=excluded.json,updated_at=excluded.updated_at",
        params![json, now],
    )
    .map_err(|e| format!("Could not save dictation settings: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_leave_every_dictation_feature_off() {
        let settings = Settings::default();
        assert!(!settings.enabled);
        assert!(!settings.ai_enabled);
        assert!(settings.restore_clipboard);
        assert_eq!(settings.mode, ShortcutMode::Toggle);
        assert!(settings.show_overlay);
        assert!(settings.app_overrides.is_empty());
        settings.validate().unwrap();
    }

    #[test]
    fn byte_bounds_and_duplicate_overrides_are_rejected() {
        let mut settings = Settings {
            language: "x".repeat(17),
            ..Settings::default()
        };
        assert!(settings.validate().is_err());
        settings.language = "auto".into();
        settings.ai_prompt = "x".repeat(PROMPT_LIMIT + 1);
        assert!(settings.validate().is_err());
        settings.ai_prompt = "Fix it.".into();
        settings.ai_kind = "cloud-summary".into();
        assert!(settings.validate().is_err());
        settings.ai_kind = "cleanup".into();
        settings.app_overrides = vec![
            AppOverride {
                bundle_id: "com.example.One".into(),
                ai_enabled: Some(true),
                ai_kind: None,
                ai_prompt: None,
            },
            AppOverride {
                bundle_id: "com.example.One".into(),
                ai_enabled: None,
                ai_kind: None,
                ai_prompt: None,
            },
        ];
        assert!(settings.validate().is_err());
        settings.app_overrides[1].bundle_id = "com.example.Two".into();
        settings.validate().unwrap();
    }

    #[test]
    fn app_override_resolves_only_by_exact_bundle_id_and_keeps_global_defaults() {
        let mut settings = Settings {
            ai_prompt: "Global prompt".into(),
            ..Settings::default()
        };
        settings.app_overrides = vec![AppOverride {
            bundle_id: "com.example.Mail".into(),
            ai_enabled: Some(true),
            ai_kind: Some("custom".into()),
            ai_prompt: Some("Turn this into bullet points.".into()),
        }];
        assert_eq!(
            resolve_processing(&settings, Some("com.example.Mail")),
            ProcessingPolicy {
                ai_enabled: true,
                ai_kind: "custom".into(),
                ai_prompt: "Turn this into bullet points.".into(),
            }
        );
        assert_eq!(
            resolve_processing(&settings, Some("com.example.mail")),
            ProcessingPolicy {
                ai_enabled: false,
                ai_kind: "cleanup".into(),
                ai_prompt: "Global prompt".into(),
            }
        );
        assert!(!resolve_processing(&settings, None).ai_enabled);
        // A URL-looking prompt is still plain bounded data, never executed.
        settings.app_overrides[0].ai_prompt = Some("https://example.com/upload".into());
        settings.validate().unwrap();
        assert_eq!(
            resolve_processing(&settings, Some("com.example.Mail")).ai_prompt,
            "https://example.com/upload"
        );
    }

    #[test]
    fn round_trip_and_blank_row_use_defaults() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(crate::dictation_settings::SCHEMA).unwrap();
        assert_eq!(load(&db).unwrap(), Settings::default());
        let settings = Settings {
            enabled: true,
            ..Settings::default()
        };
        #[cfg(target_os = "macos")]
        {
            save(&db, &settings).unwrap();
            assert_eq!(load(&db).unwrap(), settings);
        }
        #[cfg(not(target_os = "macos"))]
        {
            assert!(save(&db, &settings).is_err());
        }
    }
}

/// Persisted shape of one dictation job. No target token, no clipboard
/// content and no lease is stored: an interrupted job can only be reviewed or
/// copied after a restart.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobRow {
    pub id: String,
    pub note_id: Option<String>,
    pub phase: String,
    pub original_text: String,
    pub processed_text: Option<String>,
    pub model: String,
    pub prompt: String,
    pub target_bundle_id: String,
    pub target_name: String,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub fn save_job(db: &Connection, row: &JobRow) -> Result<(), String> {
    db.execute(
        "INSERT INTO dictation_jobs(id,note_id,phase,original_text,processed_text,model,prompt,target_bundle_id,target_name,error,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(id) DO UPDATE SET note_id=excluded.note_id,phase=excluded.phase,original_text=excluded.original_text,
           processed_text=excluded.processed_text,model=excluded.model,prompt=excluded.prompt,target_bundle_id=excluded.target_bundle_id,
           target_name=excluded.target_name,error=excluded.error,updated_at=excluded.updated_at",
        params![
            row.id,
            row.note_id,
            row.phase,
            row.original_text,
            row.processed_text,
            row.model,
            row.prompt,
            row.target_bundle_id,
            row.target_name,
            row.error,
            row.created_at,
            row.updated_at,
        ],
    )
    .map_err(|e| format!("Could not save dictation job state: {e}"))?;
    Ok(())
}

pub fn delete_job(db: &Connection, id: &str) -> Result<(), String> {
    db.execute("DELETE FROM dictation_jobs WHERE id=?1", [id])
        .map_err(|e| format!("Could not clear dictation job state: {e}"))?;
    Ok(())
}

pub fn list_jobs(db: &Connection) -> Result<Vec<JobRow>, String> {
    let mut statement = db
        .prepare(
            "SELECT id,note_id,phase,original_text,processed_text,model,prompt,target_bundle_id,target_name,error,created_at,updated_at
             FROM dictation_jobs ORDER BY created_at DESC, id DESC LIMIT 20",
        )
        .map_err(|e| format!("Could not read dictation job state: {e}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok(JobRow {
                id: row.get(0)?,
                note_id: row.get(1)?,
                phase: row.get(2)?,
                original_text: row.get(3)?,
                processed_text: row.get(4)?,
                model: row.get(5)?,
                prompt: row.get(6)?,
                target_bundle_id: row.get(7)?,
                target_name: row.get(8)?,
                error: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })
        .map_err(|e| format!("Could not read dictation job state: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Could not read dictation job state: {e}"))?;
    Ok(rows)
}

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS dictation_settings(
 id INTEGER PRIMARY KEY CHECK(id=1),
 json TEXT NOT NULL DEFAULT '{}',
 updated_at INTEGER NOT NULL DEFAULT 0);
INSERT OR IGNORE INTO dictation_settings(id) VALUES(1);
CREATE TABLE IF NOT EXISTS dictation_jobs(
 id TEXT PRIMARY KEY,
 note_id TEXT,
 phase TEXT NOT NULL,
 original_text TEXT NOT NULL DEFAULT '',
 processed_text TEXT,
 model TEXT NOT NULL DEFAULT '',
 prompt TEXT NOT NULL DEFAULT '',
 target_bundle_id TEXT NOT NULL DEFAULT '',
 target_name TEXT NOT NULL DEFAULT '',
 error TEXT,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL);
";
