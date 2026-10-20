//! Versioned, recoverable SQLite migrations for the local recording library.
//!
//! Version 7 rebuilds automatic local AI tasks around the semantic rule hash and
//! clears task links whose result was deleted. Version 3 adds durable
//! transcription jobs. Version 2 adds organization metadata and a trigger-maintained FTS5 index.
//! Version 1 is the preview `notes` table. Preview builds created that table
//! without ever setting `user_version`, so an on-disk database whose
//! `user_version` is 0 and whose `notes` table matches the preview schema
//! exactly is adopted as version 1, while a database with `user_version` 0 and
//! no `notes` table is new.
//!
//! Order matters. Everything that can refuse a library (an unsupported newer
//! schema version, a malformed `notes` table, a damaged version-1 library) runs
//! before any mutation, so a refusal leaves the file untouched: no journal-mode
//! change, no interrupted-job recovery, no backup. Matching the preview schema
//! means matching it exactly, including each column's nullability, its default
//! expression and the absence of hidden or generated columns, because a variant
//! would accept writes the read path cannot handle. An existing version-0 library
//! is then snapshotted with `VACUUM INTO`, whose output name was reserved with
//! `create_new` so an earlier backup can never be reused or destroyed; the write
//! is one consistent database that already includes content still sitting in the
//! `-wal` file, and it is verified and flushed to disk before the schema and
//! `user_version` change commits in a single transaction. Interrupted recordings
//! are marked retryable only after that commit.

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// Schema version written by this build.
pub const SCHEMA_VERSION: i64 = 7;
/// Version reported by preview libraries that predate schema versioning.
pub const LEGACY_PREVIEW_VERSION: i64 = 0;
/// Message stored on `processing` rows that a shutdown interrupted.
pub const INTERRUPTED_ERROR: &str = "Processing was interrupted. Try again.";

/// One column of the preview `notes` table, exactly as the preview DDL declares
/// it.
///
/// The validator compares every field. Renames and retypes are refused, but so is
/// a same-seven-column variant that drops a `NOT NULL` or the `segments` default:
/// the write path (`library::insert`) omits `segments`, so such a library would
/// accept rows that the read path (`library::list`) can never read back. A
/// variant cannot be told apart from a corrupted library, so it is refused.
struct ExpectedColumn {
    name: &'static str,
    declared_type: &'static str,
    not_null: bool,
    default: Option<&'static str>,
    primary_key: bool,
}

const NOTES_SCHEMA: [ExpectedColumn; 7] = [
    ExpectedColumn {
        name: "id",
        declared_type: "TEXT",
        not_null: false,
        default: None,
        primary_key: true,
    },
    ExpectedColumn {
        name: "title",
        declared_type: "TEXT",
        not_null: true,
        default: None,
        primary_key: false,
    },
    ExpectedColumn {
        name: "created_at",
        declared_type: "INTEGER",
        not_null: true,
        default: None,
        primary_key: false,
    },
    ExpectedColumn {
        name: "duration_seconds",
        declared_type: "REAL",
        not_null: true,
        default: None,
        primary_key: false,
    },
    ExpectedColumn {
        name: "status",
        declared_type: "TEXT",
        not_null: true,
        default: None,
        primary_key: false,
    },
    ExpectedColumn {
        name: "segments",
        declared_type: "TEXT",
        not_null: true,
        default: Some("'[]'"),
        primary_key: false,
    },
    ExpectedColumn {
        name: "error",
        declared_type: "TEXT",
        not_null: false,
        default: None,
        primary_key: false,
    },
];

/// The preview table definition, unchanged so old and new libraries stay
/// compatible.
const CREATE_NOTES: &str = "CREATE TABLE IF NOT EXISTS notes (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL,
          duration_seconds REAL NOT NULL, status TEXT NOT NULL,
          segments TEXT NOT NULL DEFAULT '[]', error TEXT)";

const SEARCH_SCHEMA: &str = "
CREATE VIRTUAL TABLE notes_search USING fts5(note_id UNINDEXED,title,body,tokenize='unicode61');
INSERT INTO notes_search(note_id,title,body) SELECT id,title,COALESCE((SELECT group_concat(json_extract(value,'$.text'),' ') FROM json_each(segments)),'') FROM notes;
CREATE TRIGGER notes_search_insert AFTER INSERT ON notes BEGIN
 INSERT INTO notes_search(note_id,title,body) VALUES(new.id,new.title,COALESCE((SELECT group_concat(json_extract(value,'$.text'),' ') FROM json_each(new.segments)),''));
END;
CREATE TRIGGER notes_search_update AFTER UPDATE OF title,segments ON notes BEGIN
 DELETE FROM notes_search WHERE note_id=old.id;
 INSERT INTO notes_search(note_id,title,body) VALUES(new.id,new.title,COALESCE((SELECT group_concat(json_extract(value,'$.text'),' ') FROM json_each(new.segments)),''));
END;
CREATE TRIGGER notes_search_delete AFTER DELETE ON notes BEGIN
 DELETE FROM notes_search WHERE note_id=old.id;
 DELETE FROM organization WHERE note_id=old.id;
END;
";

const NOTES_OBJECT: &str = "SELECT type FROM sqlite_master WHERE name='notes'";
/// `table_xinfo` reports the same rows as `table_info` plus a `hidden` flag, so a
/// generated or hidden column cannot pass as one of the seven expected columns.
const NOTES_COLUMNS: &str = "PRAGMA table_xinfo(notes)";
const READ_VERSION: &str = "PRAGMA user_version";
const COUNT_NOTES: &str = "SELECT count(*) FROM notes";
const RECOVER_INTERRUPTED: &str =
    "UPDATE notes SET status='failed', error=?1 WHERE status='processing'";

/// Outcome of a successful, idempotent migration.
#[derive(Debug)]
pub struct Migration {
    /// Version read before the migration (0 for preview or new libraries).
    pub from_version: i64,
    /// Version in effect afterwards.
    pub to_version: i64,
    /// Consistent pre-upgrade snapshot, present only when an existing legacy
    /// library was upgraded.
    pub backup: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// No `notes` table and no schema version: nothing to preserve.
    Fresh,
    /// Existing preview library that predates `user_version`.
    LegacyPreview,
    /// Already at the current version: only interrupted-job recovery may run.
    Current,
}

/// Bring `db` (opened at `db_path`) to [`SCHEMA_VERSION`].
///
/// Safe to call on every startup: a library that is already current is not
/// migrated and gets no new snapshot.
pub fn migrate(db: &Connection, db_path: &Path) -> Result<Migration, String> {
    let from_version = read_version(db)?;
    let state = classify(db, db_path, from_version)?;
    // Snapshot an existing library before touching it. Every refusal above
    // returns before this point, so a rejected library never gains a backup.
    let backup = match state {
        State::LegacyPreview => Some(create_backup(db, db_path)?),
        State::Fresh | State::Current => None,
    };
    // WAL matches the preview's write pattern. It is set after validation and
    // before the transaction, because journal mode cannot change inside one.
    if let Err(error) = db.execute_batch("PRAGMA journal_mode=WAL;") {
        return Err(format!("Could not enable WAL journaling: {error}"));
    }
    if state != State::Current {
        apply_current_schema(db)?;
    }
    // Resuming a transcription would discard the text it already produced, so an
    // interrupted one becomes retryable. This runs only after the commit above.
    if let Err(error) = db.execute(RECOVER_INTERRUPTED, [INTERRUPTED_ERROR]) {
        return Err(format!("Could not recover interrupted recordings: {error}"));
    }
    crate::jobs::recover(db)?;
    crate::automation::recover(db)?;
    crate::ai::recover(db)?;
    crate::ai_automation::recover(db)?;
    Ok(Migration {
        from_version,
        to_version: SCHEMA_VERSION,
        backup,
    })
}

fn read_version(db: &Connection) -> Result<i64, String> {
    db.query_row(READ_VERSION, [], |row| row.get(0))
        .map_err(|e| format!("Could not read the library schema version: {e}"))
}

fn classify(db: &Connection, db_path: &Path, version: i64) -> Result<State, String> {
    let object: Option<String> = db
        .query_row(NOTES_OBJECT, [], |row| row.get(0))
        .optional()
        .map_err(|e| format!("Could not inspect the library schema: {e}"))?;
    match (version, object.as_deref()) {
        (SCHEMA_VERSION, Some("table")) => {
            validate_notes_schema(db, db_path)?;
            db.prepare("SELECT id,note_id,language,status,error,created_at FROM transcription_jobs LIMIT 0").map_err(|e|format!("The library batch queue is damaged. No changes were made: {e}"))?;
            crate::automation::settings(db).map_err(|e|format!("The local automation settings are damaged. No changes were made: {e}"))?;
            db.prepare("SELECT w.id,f.name,i.digest,e.formats,o.path FROM watch_folders w,watch_files f,watch_imports i,export_tasks e,export_files o LIMIT 0").map_err(|e|format!("The automation schema is damaged. No changes were made: {e}"))?;
            crate::ai::settings(db).map_err(|e|format!("Local AI settings are damaged. No changes were made: {e}"))?;
            db.prepare("SELECT id,note_id,kind,target,prompt,conversation,source_language,model,context_window,source,source_hash,status,content,segments,created_at,error,phase FROM ai_results LIMIT 0").map_err(|e|format!("Local AI results are damaged. No changes were made: {e}"))?;
            db.prepare("SELECT id,position,name,enabled,kind,target,prompt,sources FROM ai_automation_rules LIMIT 0").map_err(|e|format!("Automatic local AI rules are damaged. No changes were made: {e}"))?;
            db.prepare("SELECT id,note_id,rule_id,rule_name,rule_hash,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,error,result_id,created_at,updated_at FROM ai_automation_tasks LIMIT 0").map_err(|e|format!("Automatic local AI tasks are damaged. No changes were made: {e}"))?;
            Ok(State::Current)
        }
        (SCHEMA_VERSION, _) => Err(format!(
            "The library at {} reports schema version {SCHEMA_VERSION} but has no notes table. No changes were made; restore a pre-upgrade backup or contact support.",
            db_path.display()
        )),
        (LEGACY_PREVIEW_VERSION, None) => Ok(State::Fresh),
        (LEGACY_PREVIEW_VERSION | 1 | 2 | 3 | 4 | 5 | 6, Some("table")) => {
            validate_notes_schema(db, db_path)?;
            Ok(State::LegacyPreview)
        }
        (LEGACY_PREVIEW_VERSION, Some(kind)) => Err(format!(
            "The library at {} contains a {kind} named 'notes' instead of a table. No changes were made.",
            db_path.display()
        )),
        (other, _) => Err(format!(
            "The library at {} uses schema version {other}, but this build supports at most {SCHEMA_VERSION}. No changes were made. Update Golos, or restore the pre-upgrade backup, before opening this library.",
            db_path.display()
        )),
    }
}

/// Compare `notes` against the preview schema. Anything that does not match is
/// refused rather than altered, because a variant preview library cannot be told
/// apart from a corrupted one.
fn validate_notes_schema(db: &Connection, db_path: &Path) -> Result<(), String> {
    /// The parts of a `table_xinfo` row this validator compares.
    struct ActualColumn {
        name: String,
        declared_type: String,
        not_null: bool,
        default: Option<String>,
        primary_key: bool,
        hidden: bool,
    }

    let mut statement = db
        .prepare(NOTES_COLUMNS)
        .map_err(|e| format!("Could not read the notes table schema: {e}"))?;
    let columns = statement
        .query_map([], |row| {
            Ok(ActualColumn {
                name: row.get(1)?,
                declared_type: row.get(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                default: row.get(4)?,
                primary_key: row.get::<_, i64>(5)? != 0,
                hidden: row.get::<_, i64>(6)? != 0,
            })
        })
        .map_err(|e| format!("Could not read the notes table schema: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Could not read the notes table schema: {e}"))?;
    if columns.len() != NOTES_SCHEMA.len() {
        let detail = format!(
            "expected {} columns, found {}",
            NOTES_SCHEMA.len(),
            columns.len()
        );
        return Err(schema_mismatch(db_path, &detail));
    }
    for (index, expected) in NOTES_SCHEMA.iter().enumerate() {
        let column = &columns[index];
        let mismatch = if column.name.as_str() != expected.name {
            Some(format!(
                "column {} is named '{}', not '{}'",
                index + 1,
                column.name,
                expected.name
            ))
        } else if !column
            .declared_type
            .eq_ignore_ascii_case(expected.declared_type)
        {
            Some(format!(
                "column '{}' has type {}, not {}",
                expected.name, column.declared_type, expected.declared_type
            ))
        } else if column.not_null != expected.not_null {
            Some(format!(
                "column '{}' is {}, not {}",
                expected.name,
                nullability(column.not_null),
                nullability(expected.not_null)
            ))
        } else if column.default.as_deref().map(str::trim) != expected.default {
            Some(format!(
                "column '{}' has default {:?}, not {:?}",
                expected.name, column.default, expected.default
            ))
        } else if column.primary_key != expected.primary_key {
            Some(format!(
                "column '{}' has different primary-key membership",
                expected.name
            ))
        } else if column.hidden {
            Some(format!("column '{}' is hidden or generated", expected.name))
        } else {
            None
        };
        if let Some(detail) = mismatch {
            return Err(schema_mismatch(db_path, &detail));
        }
    }
    Ok(())
}

fn nullability(not_null: bool) -> &'static str {
    if not_null {
        "NOT NULL"
    } else {
        "nullable"
    }
}

fn schema_mismatch(db_path: &Path, detail: &str) -> String {
    format!(
        "The library at {} does not match the supported notes schema ({detail}). No changes were made.",
        db_path.display()
    )
}

/// Apply the schema and the schema version in one transaction so a failure
/// cannot leave a half-migrated library: if either statement fails, dropping the
/// transaction rolls the other one back.
fn apply_current_schema(db: &Connection) -> Result<(), String> {
    let from_version = read_version(db)?;
    let transaction = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(|e| format!("Could not start the library migration: {e}"))?;
    // Both statements are inside one transaction, so their order cannot change the
    // committed result. The version is recorded first so the tests below can show
    // that a failure after a write rolls that write back rather than leaving a
    // version-1 library without its table.
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|e| format!("Could not record the library schema version: {e}"))?;
    transaction
        .execute_batch(&format!("{CREATE_NOTES}; CREATE TABLE IF NOT EXISTS organization (note_id TEXT PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE, folder TEXT NOT NULL DEFAULT '', tags TEXT NOT NULL DEFAULT '[]'); CREATE INDEX IF NOT EXISTS notes_created ON notes(created_at DESC,id DESC);"))
        .map_err(|e| format!("Could not apply the library schema: {e}"))?;
    if from_version < 2 {
        transaction
            .execute_batch(SEARCH_SCHEMA)
            .map_err(|e| format!("Could not build the library search index: {e}"))?;
    }
    if from_version < 3 {
        transaction
            .execute_batch(crate::jobs::SCHEMA)
            .map_err(|e| format!("Could not build the batch queue: {e}"))?;
    }
    if from_version < 4 {
        transaction
            .execute_batch(crate::automation::SCHEMA)
            .map_err(|e| format!("Could not build local automation: {e}"))?;
    }
    if from_version < 5 {
        transaction
            .execute_batch(crate::ai::SCHEMA)
            .map_err(|e| format!("Could not build local AI storage: {e}"))?;
    }
    if from_version < 6 {
        transaction
            .execute_batch(crate::ai_automation::SCHEMA)
            .map_err(|e| format!("Could not build automatic local AI storage: {e}"))?;
    }
    if from_version < 7 {
        // Development builds stamped two different automatic-task tables as
        // version 6, so the table is rebuilt from its actual shape rather than
        // from the recorded version alone.
        crate::ai_automation::upgrade(&transaction)
            .map_err(|e| format!("Could not upgrade automatic local AI storage: {e}"))?;
    }
    transaction
        .commit()
        .map_err(|e| format!("Could not commit the library migration: {e}"))?;
    Ok(())
}

/// Write a consistent pre-upgrade snapshot next to the library and verify that it
/// is readable, intact and holds the same number of rows.
///
/// The name is reserved with `create_new` before anything is written, so this call
/// only ever removes a file it created. A pre-existing backup — or one another
/// process creates between reservation attempts — is skipped, never reused,
/// overwritten or deleted.
fn create_backup(db: &Connection, db_path: &Path) -> Result<PathBuf, String> {
    let target = reserve_backup(db_path, unix_millis())?;
    // From here the reserved empty file belongs to this call. Removing it on any
    // failure keeps an empty or partial file from surviving under a
    // restoration-ready name.
    let outcome = snapshot_into(db, db_path, &target)
        .and_then(|()| verify_backup(db, &target))
        .and_then(|()| sync_backup(&target));
    match outcome {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_file(&target);
            Err(error)
        }
    }
}

/// Fill the reserved, empty snapshot file with one consistent database.
fn snapshot_into(db: &Connection, db_path: &Path, target: &Path) -> Result<(), String> {
    let path = target
        .to_str()
        .ok_or_else(|| format!("The backup path {} is not valid UTF-8.", target.display()))?;
    // `VACUUM INTO` writes a single database that already includes the WAL. The
    // SQLite library accepts the empty file reserved above but refuses a file that
    // already has content, so a snapshot name is never reused.
    db.execute("VACUUM INTO ?1", [path]).map_err(|error| {
        format!(
            "Could not back up {} to {}: {error}",
            db_path.display(),
            target.display()
        )
    })?;
    Ok(())
}

/// Flush the finished snapshot, and best-effort its directory entry, before the
/// upgrade is allowed to proceed, so a reported success is backed by a durable
/// backup.
fn sync_backup(target: &Path) -> Result<(), String> {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(target)
        .map_err(|e| {
            format!(
                "Could not open the backup {} to flush it: {e}",
                target.display()
            )
        })?;
    file.sync_all().map_err(|e| {
        format!(
            "Could not flush the backup {} to disk: {e}",
            target.display()
        )
    })?;
    if let Some(directory) = target.parent() {
        // Directory fsync is not available on every platform (for example
        // Windows); the file flush above is the part that must succeed.
        let _ = fs::File::open(directory).and_then(|handle| handle.sync_all());
    }
    Ok(())
}

fn verify_backup(db: &Connection, backup: &Path) -> Result<(), String> {
    let expected: i64 = db
        .query_row(COUNT_NOTES, [], |row| row.get(0))
        .map_err(|e| format!("Could not count library rows: {e}"))?;
    let snapshot = Connection::open_with_flags(backup, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Could not open the backup {}: {e}", backup.display()))?;
    let integrity: String = snapshot
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|e| format!("Could not verify the backup {}: {e}", backup.display()))?;
    if integrity != "ok" {
        return Err(format!(
            "The backup {} is damaged: {integrity}",
            backup.display()
        ));
    }
    let actual: i64 = snapshot
        .query_row(COUNT_NOTES, [], |row| row.get(0))
        .map_err(|e| format!("Could not verify the backup {}: {e}", backup.display()))?;
    if actual != expected {
        return Err(format!(
            "The backup {} holds {actual} of {expected} rows.",
            backup.display()
        ));
    }
    Ok(())
}

/// Atomically reserve a free snapshot name next to the library and return it.
///
/// `create_new` succeeds only when the file did not already exist, so the path
/// returned here is always a file this call created. The reserved file is left
/// empty for `VACUUM INTO` to fill; a concurrent writer that wins the name first
/// is skipped rather than overwritten.
fn reserve_backup(db_path: &Path, millis: u128) -> Result<PathBuf, String> {
    let stem = match db_path.file_stem() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => String::from("library"),
    };
    let directory = db_path.parent().unwrap_or(Path::new("."));
    for attempt in 0..1_000u32 {
        let name = if attempt == 0 {
            format!("{stem}.pre-v{SCHEMA_VERSION}-{millis}.sqlite3")
        } else {
            format!("{stem}.pre-v{SCHEMA_VERSION}-{millis}-{attempt}.sqlite3")
        };
        let candidate = directory.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(_) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not reserve a backup name next to {}: {error}",
                    db_path.display()
                ));
            }
        }
    }
    Err(format!(
        "Could not find a free backup name next to {}",
        db_path.display()
    ))
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The exact DDL preview builds used, including its missing version marker.
    const LEGACY_DDL: &str = "CREATE TABLE notes (
          id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL,
          duration_seconds REAL NOT NULL, status TEXT NOT NULL,
          segments TEXT NOT NULL DEFAULT '[]', error TEXT)";

    const SEGMENTS: &str = r#"[{"startMs":0,"endMs":1200,"speaker":null,"text":"Привіт"}]"#;
    const SET_SEGMENTS: &str = "UPDATE notes SET segments=?1 WHERE id=?2";
    const STATUS_OF: &str = "SELECT status FROM notes WHERE id=?1";
    const TITLE_OF: &str = "SELECT title FROM notes WHERE id=?1";
    const SEGMENTS_OF: &str = "SELECT segments FROM notes WHERE id=?1";
    const NOTES_OBJECTS: &str = "SELECT count(*) FROM sqlite_master WHERE name='notes'";
    const NOTES_TABLES: &str =
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='notes'";
    const JOURNAL_MODE: &str = "PRAGMA journal_mode";
    const BACKUP_MARKER: &str = ".pre-v7-";
    /// Mirrors `library::insert`, which leaves `segments` at its `'[]'` default.
    const INSERT_ROW: &str = "INSERT INTO notes
        (id,title,created_at,duration_seconds,status) VALUES(?1,?2,?3,?4,?5)";

    struct TempLibrary {
        directory: PathBuf,
    }

    impl TempLibrary {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let process = std::process::id();
            let name = format!("golos-migration-{label}-{process}-{nanos}-{unique}");
            let directory = std::env::temp_dir().join(name);
            fs::create_dir_all(&directory).unwrap();
            Self { directory }
        }

        fn db_path(&self) -> PathBuf {
            self.path("library.sqlite3")
        }

        fn path(&self, name: &str) -> PathBuf {
            self.directory.join(name)
        }

        /// Open (and create) this fixture's library database.
        fn open(&self) -> Connection {
            Connection::open(self.db_path()).unwrap()
        }

        /// An unversioned preview library with the original `notes` table.
        fn open_legacy(&self) -> Connection {
            let db = self.open();
            db.execute_batch(LEGACY_DDL).unwrap();
            db
        }

        fn backups(&self) -> Vec<PathBuf> {
            let mut found: Vec<PathBuf> = fs::read_dir(&self.directory)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| is_backup(path.as_path()))
                .collect();
            found.sort();
            found
        }
    }

    impl Drop for TempLibrary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn is_backup(path: &Path) -> bool {
        path.file_name()
            .is_some_and(|name| name.to_string_lossy().contains(BACKUP_MARKER))
    }

    /// A library file name in the temporary directory for one upgrade test.
    fn migration_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("golos-{label}-{}.sqlite3", uuid::Uuid::new_v4()))
    }

    /// Valid saved local AI settings for automatic rules.
    fn local_ai_settings() -> crate::ai::Settings {
        crate::ai::Settings {
            port: 11434,
            model: "local:latest".into(),
            context_window: 8192,
        }
    }

    /// An enabled summary rule, scoped to one recording.
    fn summary_rule() -> crate::ai_automation::Rule {
        crate::ai_automation::Rule {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Summary".into(),
            enabled: true,
            kind: "summary".into(),
            target: String::new(),
            prompt: String::new(),
            sources: vec!["single".into()],
        }
    }

    /// An enabled custom rule, whose prompt is its semantic identity.
    fn custom_rule() -> crate::ai_automation::Rule {
        crate::ai_automation::Rule {
            kind: "custom".into(),
            prompt: "List decisions".into(),
            ..summary_rule()
        }
    }

    fn result_cleanup_triggers(db: &Connection) -> i64 {
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name='ai_automation_result_cleanup'",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// Recorded rule hashes, in a stable order.
    fn rule_hashes(db: &Connection) -> Vec<String> {
        db.prepare("SELECT rule_hash FROM ai_automation_tasks ORDER BY rule_id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    /// The development version-6 automatic-task table: no `rule_hash`, and a
    /// unique key that ignored rule semantics, so a changed rule could never
    /// schedule a second task for the same transcript.
    const DEVELOPMENT_V6_TASKS: &str = "CREATE TABLE ai_automation_tasks(
          id TEXT PRIMARY KEY,
          note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
          rule_id TEXT NOT NULL,
          rule_name TEXT NOT NULL,
          kind TEXT NOT NULL,
          target TEXT NOT NULL,
          prompt TEXT NOT NULL,
          source TEXT NOT NULL,
          source_language TEXT NOT NULL,
          source_hash TEXT NOT NULL,
          segments TEXT NOT NULL,
          model TEXT NOT NULL,
          port INTEGER NOT NULL,
          context_window INTEGER NOT NULL,
          status TEXT NOT NULL CHECK(status IN ('queued','running','done','failed','cancelled','skipped')),
          error TEXT,
          result_id TEXT,
          created_at INTEGER NOT NULL,
          updated_at INTEGER NOT NULL,
          UNIQUE(note_id,source_hash,rule_id,model,port,context_window))";

    /// The task columns both version-6 shapes declare, without `rule_hash`.
    const DEVELOPMENT_V6_COLUMNS: &str = "id,note_id,rule_id,rule_name,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,error,result_id,created_at,updated_at";

    /// Rewrite a migrated fixture as the development version-6 library: the task
    /// table without `rule_hash`, and no result-link cleanup trigger.
    fn downgrade_tasks_to_development_v6(db: &Connection) {
        db.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS ai_automation_note_cleanup;
             DROP TRIGGER IF EXISTS ai_automation_result_cleanup;
             DROP INDEX IF EXISTS ai_automation_tasks_pending;
             DROP INDEX IF EXISTS ai_automation_tasks_note;
             ALTER TABLE ai_automation_tasks RENAME TO ai_automation_tasks_scratch;
             {DEVELOPMENT_V6_TASKS};
             INSERT INTO ai_automation_tasks({DEVELOPMENT_V6_COLUMNS})
               SELECT {DEVELOPMENT_V6_COLUMNS} FROM ai_automation_tasks_scratch;
             DROP TABLE ai_automation_tasks_scratch;
             CREATE INDEX ai_automation_tasks_pending ON ai_automation_tasks(status,created_at,id);
             CREATE INDEX ai_automation_tasks_note ON ai_automation_tasks(note_id,created_at DESC);
             CREATE TRIGGER ai_automation_note_cleanup AFTER DELETE ON notes BEGIN DELETE FROM ai_automation_tasks WHERE note_id=old.id; END;
             PRAGMA user_version=6;"
        ))
        .unwrap();
    }

    /// Rewrite a migrated fixture as the later development version-6 library: the
    /// task table already carries `rule_hash` and the semantic unique key, but
    /// the result-link cleanup trigger does not exist yet.
    fn downgrade_tasks_to_current_v6(db: &Connection) {
        db.execute_batch(
            "DROP TRIGGER IF EXISTS ai_automation_result_cleanup; PRAGMA user_version=6;",
        )
        .unwrap();
    }

    /// One finished development version-6 task row, built from a real migrated
    /// row so its captured source snapshot and model settings stay genuine. The
    /// rule name mirrors the rule id: only the id and the rule fields decide
    /// scheduling.
    fn insert_v6_row(
        db: &Connection,
        id: &str,
        rule_id: &str,
        result_id: Option<&str>,
        from: &str,
    ) {
        db.execute(
            &format!(
                "INSERT INTO ai_automation_tasks({DEVELOPMENT_V6_COLUMNS})
                 SELECT ?1,'source',?2,?2,kind,target,prompt,'watch',source_language,source_hash,
                        segments,model,port,context_window,'done',NULL,?3,created_at,updated_at
                 FROM ai_automation_tasks WHERE id=?4"
            ),
            rusqlite::params![id, rule_id, result_id, from],
        )
        .unwrap();
    }

    fn add_row(db: &Connection, id: &str, title: &str, created: i64, duration: f64, status: &str) {
        db.execute(
            INSERT_ROW,
            rusqlite::params![id, title, created, duration, status],
        )
        .unwrap();
    }

    fn text_of(db: &Connection, sql: &str, id: &str) -> String {
        db.query_row(sql, [id], |row| row.get(0)).unwrap()
    }

    fn row_count(db: &Connection) -> i64 {
        db.query_row(COUNT_NOTES, [], |row| row.get(0)).unwrap()
    }

    fn journal_mode(db: &Connection) -> String {
        db.query_row(JOURNAL_MODE, [], |row| row.get(0)).unwrap()
    }

    fn notes_objects(db: &Connection) -> i64 {
        db.query_row(NOTES_OBJECTS, [], |row| row.get(0)).unwrap()
    }

    fn notes_tables(db: &Connection) -> i64 {
        db.query_row(NOTES_TABLES, [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn version_five_upgrade_adds_automatic_rules_and_preserves_every_existing_table() {
        let (db, request) = crate::ai::tests::fixture();
        let before = crate::library::get(&db, "source").unwrap();
        let work = crate::ai::create(&db, &request, "local:latest", 8192).unwrap();
        db.execute("UPDATE automation_settings SET paused=1 WHERE id=1", [])
            .unwrap();
        db.execute(
            "INSERT INTO watch_folders(id,path,enabled,auto_transcribe,language) VALUES('folder','/tmp/golos-v5-watch',1,1,'en')",
            [],
        )
        .unwrap();
        db.execute_batch("DROP TRIGGER ai_automation_note_cleanup;DROP TABLE ai_automation_tasks;DROP TABLE ai_automation_rules;PRAGMA user_version=5;").unwrap();
        let path = std::env::temp_dir().join(format!(
            "golos-v5-automation-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let migration = migrate(&db, &path).unwrap();
        assert_eq!(migration.from_version, 5);
        assert_eq!(migration.to_version, 7);
        let backup = migration.backup.expect("upgrade must take a snapshot");
        assert!(backup.is_file());
        assert_eq!(read_version(&db).unwrap(), 7);
        assert_eq!(
            crate::library::get(&db, "source").unwrap().segments[0].text,
            before.segments[0].text
        );
        assert_eq!(
            crate::ai::get(&db, &work.id).unwrap().source_hash,
            work.source_hash
        );
        assert!(crate::automation::settings(&db).unwrap().paused);
        assert_eq!(crate::automation::folders(&db).unwrap().len(), 1);
        assert!(crate::ai_automation::rules(&db).unwrap().is_empty());
        assert!(crate::ai_automation::tasks(&db).unwrap().is_empty());
        assert!(!crate::ai_automation::has_queued(&db).unwrap());
        // The fresh storage is usable and carries the result-link cleanup.
        assert_eq!(result_cleanup_triggers(&db), 1);
        crate::ai::configure(&db, &local_ai_settings()).unwrap();
        crate::ai_automation::configure(&db, vec![summary_rule()]).unwrap();
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            1
        );
        std::fs::remove_file(backup).unwrap();
    }

    #[test]
    fn development_version_six_without_rule_hash_is_rebuilt_with_semantic_hashes() {
        let (db, request) = crate::ai::tests::fixture();
        crate::ai::configure(&db, &local_ai_settings()).unwrap();
        let summary = summary_rule();
        crate::ai_automation::configure(&db, vec![summary.clone()]).unwrap();
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            1
        );
        let queued = crate::ai_automation::tasks(&db).unwrap().remove(0);
        assert_eq!(queued.status, "queued");
        // A finished task linked to a saved result, and one whose result was
        // deleted before the cleanup trigger existed.
        let live = crate::ai::create(&db, &request, "local:latest", 8192).unwrap();
        db.execute(
            "UPDATE ai_results SET status='done' WHERE id=?1",
            [&live.id],
        )
        .unwrap();
        downgrade_tasks_to_development_v6(&db);
        insert_v6_row(&db, "linked", "retired", Some(&live.id), &queued.id);
        insert_v6_row(
            &db,
            "dangling",
            "gone-rule",
            Some("deleted-result"),
            &queued.id,
        );

        let path = migration_path("v6-development");
        let migration = migrate(&db, &path).unwrap();
        assert_eq!(migration.from_version, 6);
        assert_eq!(migration.to_version, 7);
        let backup = migration
            .backup
            .expect("upgrading an existing library must take a snapshot");
        assert!(backup.is_file());
        assert_eq!(read_version(&db).unwrap(), 7);
        // The snapshot holds the pre-upgrade shape, hashless column and all.
        {
            let snapshot =
                Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            assert_eq!(read_version(&snapshot).unwrap(), 6);
            let hashless: i64 = snapshot
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('ai_automation_tasks') WHERE name='rule_hash'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(hashless, 0);
            assert_eq!(row_count(&snapshot), 1);
        }

        let tasks = crate::ai_automation::tasks(&db).unwrap();
        assert_eq!(tasks.len(), 3, "every captured task survives the rebuild");
        let kept = tasks.iter().find(|task| task.id == queued.id).unwrap();
        assert_eq!(kept.status, "queued");
        assert!(kept.result_id.is_none());
        let linked = tasks.iter().find(|task| task.id == "linked").unwrap();
        assert_eq!(linked.status, "done");
        assert_eq!(linked.result_id.as_deref(), Some(live.id.as_str()));
        let dangling = tasks.iter().find(|task| task.id == "dangling").unwrap();
        assert_eq!(dangling.status, "done");
        assert!(
            dangling.result_id.is_none(),
            "a link to a deleted result is cleared during the upgrade"
        );
        assert!(rule_hashes(&db).iter().all(|hash| !hash.is_empty()));
        // The rebuilt hash is exactly the one the scheduler computes, so the
        // unchanged rule is still recognised as already scheduled.
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            0
        );
        // Changing the rule's task now schedules new work: rule semantics are part
        // of the schedule identity, which the old unique key could not express.
        let mut changed = summary;
        changed.kind = "custom".into();
        changed.prompt = "List decisions".into();
        crate::ai_automation::configure(&db, vec![changed]).unwrap();
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            1
        );
        // The rest of the library is untouched.
        assert_eq!(crate::ai_automation::rules(&db).unwrap().len(), 1);
        assert_eq!(crate::ai::settings(&db).unwrap().model, "local:latest");
        assert!(crate::ai::get(&db, &live.id).is_ok());
        assert_eq!(
            crate::library::get(&db, "source").unwrap().segments[0].text,
            "We agreed to ship on Friday."
        );
        std::fs::remove_file(backup).unwrap();
    }

    #[test]
    fn current_version_six_tasks_keep_their_rows_and_gain_the_result_cleanup() {
        let (db, request) = crate::ai::tests::fixture();
        crate::ai::configure(&db, &local_ai_settings()).unwrap();
        crate::ai_automation::configure(&db, vec![summary_rule(), custom_rule()]).unwrap();
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            2
        );
        let live = crate::ai::create(&db, &request, "local:latest", 8192).unwrap();
        db.execute(
            "UPDATE ai_results SET status='done' WHERE id=?1",
            [&live.id],
        )
        .unwrap();
        let finished = crate::ai_automation::tasks(&db).unwrap().remove(0).id;
        db.execute(
            "UPDATE ai_automation_tasks SET status='done',result_id=?2 WHERE id=?1",
            rusqlite::params![&finished, &live.id],
        )
        .unwrap();
        let before = rule_hashes(&db);
        downgrade_tasks_to_current_v6(&db);
        assert_eq!(result_cleanup_triggers(&db), 0);

        let path = migration_path("v6-current");
        let migration = migrate(&db, &path).unwrap();
        let backup = migration
            .backup
            .expect("upgrading an existing library must take a snapshot");
        assert_eq!(read_version(&db).unwrap(), 7);
        assert_eq!(result_cleanup_triggers(&db), 1);
        assert_eq!(rule_hashes(&db), before, "recorded hashes are kept");
        assert_eq!(crate::ai_automation::tasks(&db).unwrap().len(), 2);
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            0
        );
        // The finished task keeps its link while the result exists, and loses it
        // when the result is deleted, without losing the task itself.
        let linked = |db: &Connection| {
            crate::ai_automation::tasks(db)
                .unwrap()
                .into_iter()
                .find(|task| task.id == finished)
                .unwrap()
        };
        assert_eq!(linked(&db).result_id.as_deref(), Some(live.id.as_str()));
        db.execute("DELETE FROM ai_results WHERE id=?1", [&live.id])
            .unwrap();
        let after = linked(&db);
        assert_eq!(after.status, "done");
        assert!(
            after.result_id.is_none(),
            "the deleted result releases the link"
        );
        std::fs::remove_file(backup).unwrap();
    }

    #[test]
    fn version_six_task_without_a_recorded_hash_is_normalised() {
        let (db, _) = crate::ai::tests::fixture();
        crate::ai::configure(&db, &local_ai_settings()).unwrap();
        crate::ai_automation::configure(&db, vec![summary_rule()]).unwrap();
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            1
        );
        // A task whose hash was never recorded: the current column with its
        // empty default, which cannot match any rule.
        db.execute("UPDATE ai_automation_tasks SET rule_hash=''", [])
            .unwrap();
        downgrade_tasks_to_current_v6(&db);

        let path = migration_path("v6-hashless");
        let migration = migrate(&db, &path).unwrap();
        assert_eq!(migration.from_version, 6);
        assert_eq!(migration.to_version, 7);
        let backup = migration
            .backup
            .expect("upgrading an existing library must take a snapshot");
        assert_eq!(crate::ai_automation::tasks(&db).unwrap().len(), 1);
        let hashes = rule_hashes(&db);
        assert_eq!(hashes.len(), 1);
        assert!(!hashes[0].is_empty());
        // The recorded hash is the scheduler's own, so the task still deduplicates.
        assert_eq!(
            crate::ai_automation::schedule_atomic(&db, "source", "single").unwrap(),
            0
        );
        std::fs::remove_file(backup).unwrap();
    }

    #[test]
    fn version_four_adds_ai_without_changing_automation_or_transcripts() {
        let (db, _) = crate::ai::tests::fixture();
        db.execute("UPDATE automation_settings SET paused=1", [])
            .unwrap();
        db.execute_batch("DROP TRIGGER ai_note_cleanup;DROP TABLE ai_results;DROP TABLE ai_settings;DROP TRIGGER ai_automation_note_cleanup;DROP TABLE ai_automation_tasks;DROP TABLE ai_automation_rules;PRAGMA user_version=4;").unwrap();
        let before = crate::library::get(&db, "source").unwrap();
        let path = std::env::temp_dir().join(format!(
            "golos-ai-migration-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let migration = migrate(&db, &path).unwrap();
        std::fs::remove_file(migration.backup.unwrap()).unwrap();
        assert_eq!(read_version(&db).unwrap(), 7);
        assert_eq!(
            db.query_row("SELECT paused FROM automation_settings", [], |row| row
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            crate::library::get(&db, "source").unwrap().segments[0].text,
            before.segments[0].text
        );
        assert!(crate::ai::settings(&db).unwrap().model.is_empty());
        assert!(crate::ai::list(&db, "source").unwrap().is_empty());
    }
    #[test]
    fn upgrades_version_one_with_metadata_and_preserves_transcript() {
        let fixture = TempLibrary::new("v1-metadata");
        let db = fixture.open_legacy();
        add_row(&db, "one", "Old title", 1, 3.0, "done");
        db.execute_batch("PRAGMA user_version=1").unwrap();
        let before = text_of(&db, "SELECT segments FROM notes WHERE id=?1", "one");
        let result = migrate(&db, &fixture.db_path()).unwrap();
        assert!(result.backup.is_some());
        assert_eq!(result.from_version, 1);
        assert_eq!(result.to_version, SCHEMA_VERSION);
        assert_eq!(
            text_of(&db, "SELECT segments FROM notes WHERE id=?1", "one"),
            before
        );
        db.execute(
            "INSERT INTO organization(note_id,folder,tags) VALUES('one','Work','[]')",
            [],
        )
        .unwrap();
        assert!(migrate(&db, &fixture.db_path()).unwrap().backup.is_none());
    }

    #[test]
    fn fresh_database_is_versioned_without_a_backup() {
        let fixture = TempLibrary::new("fresh");
        let path = fixture.db_path();
        {
            let db = fixture.open();
            let migration = migrate(&db, &path).unwrap();
            assert_eq!(migration.from_version, LEGACY_PREVIEW_VERSION);
            assert_eq!(migration.to_version, SCHEMA_VERSION);
            assert!(migration.backup.is_none());
            assert_eq!(read_version(&db).unwrap(), SCHEMA_VERSION);
            assert_eq!(notes_objects(&db), 1);
            add_row(&db, "one", "First", 1_700_000_000, 5.0, "ready");
        }
        assert!(fixture.backups().is_empty());

        // A second startup finds a current library: no backup, no duplicate work.
        let db = fixture.open();
        let migration = migrate(&db, &path).unwrap();
        assert_eq!(migration.from_version, SCHEMA_VERSION);
        assert_eq!(migration.to_version, SCHEMA_VERSION);
        assert!(migration.backup.is_none());
        assert!(fixture.backups().is_empty());
        assert_eq!(library::list(&db).unwrap().len(), 1);
    }

    #[test]
    fn legacy_preview_library_is_backed_up_and_preserved() {
        let fixture = TempLibrary::new("legacy");
        let path = fixture.db_path();
        let db = fixture.open_legacy();
        add_row(&db, "keep", "Interview", 1_700_000_001, 3.5, "done");
        db.execute(SET_SEGMENTS, rusqlite::params![SEGMENTS, "keep"])
            .unwrap();
        add_row(
            &db,
            "interrupted",
            "Meeting",
            1_700_000_002,
            2.0,
            "processing",
        );
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);

        let migration = migrate(&db, &path).unwrap();
        assert_eq!(migration.from_version, LEGACY_PREVIEW_VERSION);
        assert_eq!(migration.to_version, SCHEMA_VERSION);
        let backup = migration
            .backup
            .expect("upgrading an existing library must take a backup");
        assert!(backup.is_file());
        assert_eq!(read_version(&db).unwrap(), SCHEMA_VERSION);

        // Title, timestamps and transcript text survive the upgrade through the
        // same read path the UI uses.
        let notes = library::list(&db).unwrap();
        assert_eq!(notes.len(), 2);
        let kept = notes.iter().find(|note| note.id == "keep").unwrap();
        assert_eq!(kept.title, "Interview");
        assert_eq!(kept.created_at, 1_700_000_001);
        assert_eq!(kept.duration_seconds, 3.5);
        assert_eq!(kept.status, "done");
        assert_eq!(kept.segments.len(), 1);
        assert_eq!(kept.segments[0].start_ms, 0);
        assert_eq!(kept.segments[0].end_ms, 1200);
        assert_eq!(kept.segments[0].text, "Привіт");
        // Recovery happens only after a successful migration.
        let interrupted = notes.iter().find(|note| note.id == "interrupted").unwrap();
        assert_eq!(interrupted.status, "failed");
        assert_eq!(interrupted.error.as_deref(), Some(INTERRUPTED_ERROR));

        // The backup stands alone and holds the pre-upgrade state.
        {
            let snapshot =
                Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            assert_eq!(read_version(&snapshot).unwrap(), LEGACY_PREVIEW_VERSION);
            let integrity: String = snapshot
                .query_row("PRAGMA quick_check", [], |row| row.get(0))
                .unwrap();
            assert_eq!(integrity, "ok");
            assert_eq!(text_of(&snapshot, STATUS_OF, "interrupted"), "processing");
            assert_eq!(text_of(&snapshot, SEGMENTS_OF, "interrupted"), "[]");
            assert_eq!(text_of(&snapshot, SEGMENTS_OF, "keep"), SEGMENTS);
        }
        drop(db);

        // Normal startups after the upgrade must not recreate a backup.
        let db = fixture.open();
        let migration = migrate(&db, &path).unwrap();
        assert_eq!(migration.from_version, SCHEMA_VERSION);
        assert!(migration.backup.is_none());
        assert_eq!(fixture.backups(), vec![backup]);
    }

    #[test]
    fn upgrade_backup_includes_content_that_is_only_in_the_wal() {
        let fixture = TempLibrary::new("wal");
        let path = fixture.db_path();
        let db = fixture.open_legacy();
        // Keep the new row out of the main file: WAL with checkpointing off.
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .unwrap();
        add_row(&db, "wal-only", "Wal", 1_700_000_003, 1.5, "done");
        db.execute(SET_SEGMENTS, rusqlite::params![SEGMENTS, "wal-only"])
            .unwrap();
        let wal_size = fs::metadata(fixture.path("library.sqlite3-wal"))
            .expect("the WAL file should exist")
            .len();
        assert!(
            wal_size > 0,
            "the row should still live in the WAL when the backup is taken"
        );

        let migration = migrate(&db, &path).unwrap();
        let backup = migration.backup.expect("an upgrade must back up first");
        let snapshot =
            Connection::open_with_flags(&backup, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert_eq!(text_of(&snapshot, TITLE_OF, "wal-only"), "Wal");
        assert_eq!(text_of(&snapshot, SEGMENTS_OF, "wal-only"), SEGMENTS);
    }

    #[test]
    fn newer_schema_version_is_refused_without_mutation() {
        let fixture = TempLibrary::new("newer");
        let path = fixture.db_path();
        let db = fixture.open_legacy();
        add_row(
            &db,
            "interrupted",
            "Meeting",
            1_700_000_004,
            2.0,
            "processing",
        );
        // A library written by a future build, deliberately not in WAL mode so
        // the test can prove the journal mode was left alone.
        db.execute_batch("PRAGMA user_version = 8; PRAGMA journal_mode = DELETE;")
            .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("schema version 8"), "unexpected: {error}");
        assert!(
            error.contains("No changes were made"),
            "unexpected: {error}"
        );
        assert_eq!(read_version(&db).unwrap(), 8);
        assert_eq!(journal_mode(&db), "delete");
        assert_eq!(text_of(&db, STATUS_OF, "interrupted"), "processing");
        assert!(fixture.backups().is_empty());
        drop(db);

        // The refusal is persistent, not just a view of this connection.
        let reopened = fixture.open();
        assert_eq!(read_version(&reopened).unwrap(), 8);
        assert_eq!(journal_mode(&reopened), "delete");
        assert_eq!(row_count(&reopened), 1);
    }

    #[test]
    fn malformed_notes_table_is_rejected_without_changes() {
        let fixture = TempLibrary::new("malformed");
        let path = fixture.db_path();
        let db = fixture.open();
        db.execute_batch("CREATE TABLE notes (id TEXT PRIMARY KEY, title TEXT);")
            .unwrap();
        db.execute("INSERT INTO notes(id,title) VALUES('keep','Interview')", [])
            .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("notes schema"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);
        assert_eq!(journal_mode(&db), "delete");
        assert_eq!(text_of(&db, TITLE_OF, "keep"), "Interview");
        assert!(fixture.backups().is_empty());
    }

    #[test]
    fn non_table_notes_object_is_rejected() {
        let fixture = TempLibrary::new("view");
        let path = fixture.db_path();
        let db = fixture.open();
        db.execute_batch("CREATE VIEW notes AS SELECT 'keep' AS id;")
            .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("instead of a table"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);
        assert!(fixture.backups().is_empty());
    }

    #[test]
    fn current_version_without_a_notes_table_is_refused() {
        let fixture = TempLibrary::new("missing-table");
        let path = fixture.db_path();
        let db = fixture.open();
        db.pragma_update(None, "user_version", SCHEMA_VERSION)
            .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("no notes table"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), SCHEMA_VERSION);
        // A damaged current library must not be papered over with an empty table.
        assert_eq!(notes_objects(&db), 0);
        assert!(fixture.backups().is_empty());
    }

    #[test]
    fn failed_schema_application_rolls_back_the_version_and_table() {
        let db = Connection::open_in_memory().unwrap();
        // `CREATE TABLE IF NOT EXISTS notes` still fails when an *index* named
        // `notes` already exists, so this drives the real `apply_current_schema`
        // into its error arm after it has already written the version. Without the
        // production transaction, `user_version` would remain at 1 here.
        db.execute_batch(
            "CREATE TABLE other(id TEXT);
             CREATE INDEX notes ON other(id);
             INSERT INTO other VALUES('keep');",
        )
        .unwrap();

        let error = apply_current_schema(&db).unwrap_err();
        assert!(error.contains("library schema"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);
        assert_eq!(notes_tables(&db), 0);
        let kept: String = db
            .query_row("SELECT id FROM other", [], |row| row.get(0))
            .unwrap();
        assert_eq!(kept, "keep");

        // Removing the conflict lets the same connection migrate cleanly, proving
        // the failed attempt left no transaction open.
        db.execute_batch("DROP INDEX notes;").unwrap();
        apply_current_schema(&db).unwrap();
        assert_eq!(read_version(&db).unwrap(), SCHEMA_VERSION);
        assert_eq!(notes_tables(&db), 1);
    }

    #[test]
    fn notes_variant_missing_not_null_is_rejected() {
        let fixture = TempLibrary::new("variant-not-null");
        let path = fixture.db_path();
        let db = fixture.open();
        // The same seven names and types, except `title` lost its `NOT NULL`.
        db.execute_batch(
            "CREATE TABLE notes (
               id TEXT PRIMARY KEY, title TEXT, created_at INTEGER NOT NULL,
               duration_seconds REAL NOT NULL, status TEXT NOT NULL,
               segments TEXT NOT NULL DEFAULT '[]', error TEXT);
             INSERT INTO notes(id,title,created_at,duration_seconds,status)
               VALUES('keep','Interview',1,1.0,'done');",
        )
        .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("notes schema"), "unexpected: {error}");
        assert!(error.contains("'title'"), "unexpected: {error}");
        assert!(error.contains("NOT NULL"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);
        assert_eq!(journal_mode(&db), "delete");
        assert_eq!(text_of(&db, TITLE_OF, "keep"), "Interview");
        assert!(fixture.backups().is_empty());
    }

    #[test]
    fn notes_variant_missing_segments_default_is_rejected() {
        let fixture = TempLibrary::new("variant-default");
        let path = fixture.db_path();
        let db = fixture.open();
        // Seven matching columns, but `segments` no longer defaults to `'[]'`,
        // which `library::insert` relies on when it omits the column.
        db.execute_batch(
            "CREATE TABLE notes (
               id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL,
               duration_seconds REAL NOT NULL, status TEXT NOT NULL,
               segments TEXT NOT NULL, error TEXT);
             INSERT INTO notes(id,title,created_at,duration_seconds,status,segments)
               VALUES('keep','Interview',1,1.0,'done','[]');",
        )
        .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("notes schema"), "unexpected: {error}");
        assert!(error.contains("'segments'"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);
        assert_eq!(journal_mode(&db), "delete");
        assert_eq!(text_of(&db, SEGMENTS_OF, "keep"), "[]");
        assert!(fixture.backups().is_empty());
    }

    #[test]
    fn notes_variant_with_generated_column_is_rejected() {
        let fixture = TempLibrary::new("variant-generated");
        let path = fixture.db_path();
        let db = fixture.open();
        // Seven columns that all match on name, type, nullability, default and key
        // except that `error` is generated; only `PRAGMA table_xinfo` exposes that.
        db.execute_batch(
            "CREATE TABLE notes (
               id TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL,
               duration_seconds REAL NOT NULL, status TEXT NOT NULL,
               segments TEXT NOT NULL DEFAULT '[]',
               error TEXT GENERATED ALWAYS AS (title) VIRTUAL);
             INSERT INTO notes(id,title,created_at,duration_seconds,status,segments)
               VALUES('keep','Interview',1,1.0,'done','[]');",
        )
        .unwrap();

        let error = migrate(&db, &path).unwrap_err();
        assert!(error.contains("notes schema"), "unexpected: {error}");
        assert!(error.contains("hidden or generated"), "unexpected: {error}");
        assert_eq!(read_version(&db).unwrap(), LEGACY_PREVIEW_VERSION);
        assert_eq!(text_of(&db, TITLE_OF, "keep"), "Interview");
        assert!(fixture.backups().is_empty());
    }

    #[test]
    fn backup_failure_removes_the_reserved_file() {
        let fixture = TempLibrary::new("backup-failure");
        let path = fixture.db_path();
        // A snapshot that must survive the failed attempt untouched.
        let existing = fixture.path("library.pre-v7-1.sqlite3");
        fs::write(&existing, b"an existing backup").unwrap();

        let db = fixture.open_legacy();
        add_row(&db, "keep", "Interview", 1, 1.0, "done");
        // `VACUUM` cannot run inside a transaction, so `create_backup` fails after
        // it has already reserved the snapshot name. The reserved file must not
        // remain under a restoration-ready name.
        db.execute_batch("BEGIN;").unwrap();
        let error = create_backup(&db, &path).unwrap_err();
        db.execute_batch("ROLLBACK;").unwrap();

        assert!(error.contains("Could not back up"), "unexpected: {error}");
        assert_eq!(fixture.backups(), vec![existing.clone()]);
        assert_eq!(fs::read(&existing).unwrap(), b"an existing backup");
    }

    #[test]
    fn taking_a_snapshot_preserves_an_existing_one() {
        let fixture = TempLibrary::new("snapshots");
        let path = fixture.db_path();
        let db = fixture.open_legacy();
        add_row(&db, "keep", "Interview", 1, 1.0, "done");

        let first = create_backup(&db, &path).unwrap();
        let first_bytes = fs::read(&first).unwrap();
        let second = create_backup(&db, &path).unwrap();

        assert_ne!(first, second);
        assert_eq!(
            fs::read(&first).unwrap(),
            first_bytes,
            "an earlier snapshot must not be rewritten"
        );
        for snapshot in [&first, &second] {
            let check =
                Connection::open_with_flags(snapshot, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let integrity: String = check
                .query_row("PRAGMA quick_check", [], |row| row.get(0))
                .unwrap();
            assert_eq!(integrity, "ok");
            assert_eq!(text_of(&check, TITLE_OF, "keep"), "Interview");
        }
    }

    #[test]
    fn reserving_a_backup_never_reuses_an_existing_file() {
        let fixture = TempLibrary::new("names");
        let path = fixture.db_path();
        let millis = 1_760_000_000_000u128;

        let first = reserve_backup(&path, millis).unwrap();
        assert_eq!(
            first.file_name().unwrap(),
            "library.pre-v7-1760000000000.sqlite3"
        );
        assert!(first.is_file(), "the reserved name must be created empty");
        // Stand in for a completed snapshot a later upgrade must not disturb.
        fs::write(&first, b"an existing backup").unwrap();

        let second = reserve_backup(&path, millis).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            second.file_name().unwrap(),
            "library.pre-v7-1760000000000-1.sqlite3"
        );
        assert_eq!(fs::read(&first).unwrap(), b"an existing backup");
    }
    #[test]
    fn upgrade_from_version_two_preserves_search_organization_and_takes_a_snapshot() {
        let fixture = TempLibrary::new("version-two");
        let db = fixture.open();
        migrate(&db, &fixture.db_path()).unwrap();
        db.execute_batch("DROP TRIGGER transcription_jobs_cleanup; DROP TABLE transcription_jobs; DROP TRIGGER automation_note_cleanup; DROP TRIGGER automation_watch_cleanup; DROP TABLE export_tasks; DROP TABLE export_files; DROP TABLE watch_imports; DROP TABLE watch_files; DROP TABLE watch_folders; DROP TABLE automation_settings; DROP TRIGGER ai_note_cleanup;DROP TABLE ai_results;DROP TABLE ai_settings;DROP TRIGGER ai_automation_note_cleanup;DROP TABLE ai_automation_tasks;DROP TABLE ai_automation_rules;PRAGMA user_version=2;").unwrap();
        add_row(&db, "keep", "Interview", 1, 2.0, "done");
        db.execute(SET_SEGMENTS, rusqlite::params![SEGMENTS, "keep"])
            .unwrap();
        db.execute(
            r#"INSERT INTO organization(note_id,folder,tags) VALUES('keep','Work','["research"]')"#,
            [],
        )
        .unwrap();
        let migration = migrate(&db, &fixture.db_path()).unwrap();
        assert_eq!(migration.from_version, 2);
        assert!(migration.backup.is_some());
        let kept = library::get(&db, "keep").unwrap();
        assert_eq!(kept.folder, "Work");
        assert_eq!(kept.tags, vec!["research"]);
        assert_eq!(kept.segments[0].text, "Привіт");
        assert_eq!(
            library::query(&db, "Прив", "", "", "", 100, 0, None)
                .unwrap()
                .len(),
            1
        );
        assert!(crate::jobs::list(&db).unwrap().is_empty());
        let snapshot = Connection::open(migration.backup.unwrap()).unwrap();
        assert_eq!(read_version(&snapshot).unwrap(), 2);
        assert_eq!(text_of(&snapshot, TITLE_OF, "keep"), "Interview");
    }
    #[test]
    fn upgrade_from_version_three_preserves_queued_jobs_and_grants_no_automatic_access() {
        let fixture = TempLibrary::new("version-three");
        let db = fixture.open();
        migrate(&db, &fixture.db_path()).unwrap();
        db.execute_batch("DROP TRIGGER automation_note_cleanup;DROP TRIGGER automation_watch_cleanup;DROP TABLE export_tasks;DROP TABLE export_files;DROP TABLE watch_imports;DROP TABLE watch_files;DROP TABLE watch_folders;DROP TABLE automation_settings;DROP TRIGGER ai_note_cleanup;DROP TABLE ai_results;DROP TABLE ai_settings;DROP TRIGGER ai_automation_note_cleanup;DROP TABLE ai_automation_tasks;DROP TABLE ai_automation_rules;PRAGMA user_version=3;").unwrap();
        add_row(&db, "one", "Waiting job", 1, 1.0, "ready");
        crate::jobs::enqueue(&db, "one", "uk").unwrap();
        let result = migrate(&db, &fixture.db_path()).unwrap();
        assert_eq!(result.from_version, 3);
        assert!(result.backup.is_some());
        assert_eq!(crate::jobs::list(&db).unwrap()[0].status, "queued");
        assert_eq!(crate::jobs::list(&db).unwrap()[0].language, "uk");
        let settings = crate::automation::settings(&db).unwrap();
        assert!(!settings.export_enabled);
        assert!(settings.export_directory.is_none());
        assert!(crate::automation::folders(&db).unwrap().is_empty());
    }
}
