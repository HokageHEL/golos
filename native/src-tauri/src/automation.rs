//! User-granted local folder configuration and durable automation state.
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};

pub const SCHEMA:&str="
CREATE TABLE automation_settings (id INTEGER PRIMARY KEY CHECK(id=1), paused INTEGER NOT NULL DEFAULT 0, export_enabled INTEGER NOT NULL DEFAULT 0, export_directory TEXT, export_formats TEXT NOT NULL DEFAULT '[\"txt\"]');
INSERT INTO automation_settings(id) VALUES(1);
CREATE TABLE watch_folders(id TEXT PRIMARY KEY,path TEXT NOT NULL UNIQUE,enabled INTEGER NOT NULL,auto_transcribe INTEGER NOT NULL,language TEXT NOT NULL,error TEXT);
CREATE TABLE watch_files(folder_id TEXT NOT NULL REFERENCES watch_folders(id) ON DELETE CASCADE,name TEXT NOT NULL,size INTEGER NOT NULL,modified TEXT NOT NULL,stable_since INTEGER NOT NULL,status TEXT NOT NULL,error TEXT,PRIMARY KEY(folder_id,name));
CREATE INDEX watch_files_waiting ON watch_files(folder_id,status,stable_since);
CREATE TABLE watch_imports(folder_id TEXT NOT NULL REFERENCES watch_folders(id) ON DELETE CASCADE,digest TEXT NOT NULL,note_id TEXT NOT NULL,PRIMARY KEY(folder_id,digest));
CREATE TABLE export_tasks(note_id TEXT PRIMARY KEY REFERENCES notes(id) ON DELETE CASCADE,directory TEXT NOT NULL,formats TEXT NOT NULL,status TEXT NOT NULL,error TEXT);
CREATE INDEX export_tasks_pending ON export_tasks(status);
CREATE TABLE export_files(note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,format TEXT NOT NULL,directory TEXT NOT NULL,digest TEXT NOT NULL,path TEXT NOT NULL,PRIMARY KEY(note_id,format,directory,digest));
CREATE TRIGGER automation_note_cleanup AFTER DELETE ON notes BEGIN DELETE FROM export_tasks WHERE note_id=old.id;DELETE FROM export_files WHERE note_id=old.id;END;
CREATE TRIGGER automation_watch_cleanup AFTER DELETE ON watch_folders BEGIN DELETE FROM watch_files WHERE folder_id=old.id;DELETE FROM watch_imports WHERE folder_id=old.id;END;
";
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolder {
    pub id: String,
    pub path: String,
    pub enabled: bool,
    pub auto_transcribe: bool,
    pub language: String,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchFile {
    pub folder_id: String,
    pub name: String,
    pub status: String,
    pub error: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub paused: bool,
    pub export_enabled: bool,
    pub export_directory: Option<String>,
    pub export_formats: Vec<String>,
}
pub fn settings(db: &Connection) -> Result<Settings, String> {
    let (paused,enabled,directory,formats):(bool,bool,Option<String>,String)=db.query_row("SELECT paused,export_enabled,export_directory,export_formats FROM automation_settings WHERE id=1",[],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).map_err(|e|e.to_string())?;
    Ok(Settings {
        paused,
        export_enabled: enabled,
        export_directory: directory,
        export_formats: serde_json::from_str(&formats).map_err(|e| e.to_string())?,
    })
}
pub fn folders(db: &Connection) -> Result<Vec<WatchFolder>, String> {
    db.prepare(
        "SELECT id,path,enabled,auto_transcribe,language,error FROM watch_folders ORDER BY path",
    )
    .map_err(|e| e.to_string())?
    .query_map([], |row| {
        Ok(WatchFolder {
            id: row.get(0)?,
            path: row.get(1)?,
            enabled: row.get(2)?,
            auto_transcribe: row.get(3)?,
            language: row.get(4)?,
            error: row.get(5)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<_, _>>()
    .map_err(|e| e.to_string())
}
pub fn files(db: &Connection) -> Result<Vec<WatchFile>, String> {
    db.prepare("SELECT folder_id,name,status,error FROM watch_files WHERE status!='ignored' ORDER BY CASE status WHEN 'failed' THEN 0 WHEN 'waiting' THEN 1 ELSE 2 END,rowid DESC LIMIT 200").map_err(|e|e.to_string())?.query_map([],|row|Ok(WatchFile{folder_id:row.get(0)?,name:row.get(1)?,status:row.get(2)?,error:row.get(3)?})).map_err(|e|e.to_string())?.collect::<Result<_,_>>().map_err(|e|e.to_string())
}
pub fn directory(path: &Path, root: &Path) -> Result<PathBuf, String> {
    let canonical=path.canonicalize().map_err(|e|format!("Cannot access folder. Reselect it or grant Files and Folders access in System Settings: {e}"))?;
    if !canonical.is_dir() || canonical.starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("Choose a folder outside Golos's private application-data directory.".into());
    }
    canonical
        .to_str()
        .ok_or("Use a folder with a Unicode path.")?;
    Ok(canonical)
}
pub fn add(db: &Connection, path: &Path, language: &str, auto: bool) -> Result<String, String> {
    let count: i64 = db
        .query_row("SELECT count(*) FROM watch_folders", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;
    if count >= 10 {
        return Err("Use up to 10 watched folders. Remove one before adding another.".into());
    }
    if language != "auto"
        && !crate::languages::available()
            .iter()
            .any(|item| item.code == language)
    {
        return Err("Choose a supported language.".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    db.execute(
        "INSERT INTO watch_folders(id,path,enabled,auto_transcribe,language) VALUES(?1,?2,1,?3,?4)",
        params![
            id,
            path.to_str().ok_or("Unsupported folder path")?,
            auto,
            language
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(id)
}
pub fn action(db: &Connection, id: &str, action: &str) -> Result<(), String> {
    let count=match action {
        "pause"=>db.execute("UPDATE watch_folders SET enabled=0 WHERE id=?1",[id]),
        "resume"=>db.execute("UPDATE watch_folders SET enabled=1,error=NULL WHERE id=?1",[id]),
        "auto"=>db.execute("UPDATE watch_folders SET auto_transcribe=1 WHERE id=?1",[id]),
        "manual"=>db.execute("UPDATE watch_folders SET auto_transcribe=0 WHERE id=?1",[id]),
        "remove"=>db.execute("DELETE FROM watch_folders WHERE id=?1",[id]),
        "retry"=>db.execute("UPDATE watch_files SET status='waiting',error=NULL WHERE folder_id=?1 AND status='failed'",[id]),
        _=>return Err("Unsupported watched-folder action.".into()),
    }.map_err(|e|e.to_string())?;
    if count == 0 && action != "retry" {
        return Err("Watched folder not found.".into());
    }
    Ok(())
}
pub fn recover(db: &Connection) -> Result<(), String> {
    db.execute("UPDATE export_tasks SET status='failed',error='Export was interrupted. Retry exports to complete the remaining formats.' WHERE status='running'",[]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn schedule_export(db: &Connection, note: &str) -> Result<(), String> {
    let settings = settings(db)?;
    if settings.export_enabled {
        crate::auto_export::schedule(
            db,
            note,
            Path::new(
                settings
                    .export_directory
                    .as_deref()
                    .ok_or("Choose an export folder")?,
            ),
            &settings.export_formats,
        )?;
    }
    Ok(())
}

/// Discard only recognized uncommitted cache names after a successful library open.
/// Committed audio, source files, unknown files and symlinks are never removed.
pub fn recover_files(root: &Path) -> Result<(), String> {
    for (directory, raw) in [(root.join("incoming"), true), (root.join("audio"), false)] {
        let metadata = match std::fs::symlink_metadata(&directory) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.to_string()),
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            continue;
        }
        for item in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            let path = item.path();
            let metadata = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let owned = if raw {
                path.file_stem()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| uuid::Uuid::parse_str(name).is_ok())
                    && path
                        .extension()
                        .and_then(|extension| extension.to_str())
                        .is_some_and(|extension| {
                            crate::import::EXTENSIONS
                                .iter()
                                .any(|known| extension.eq_ignore_ascii_case(known))
                        })
            } else {
                name.strip_suffix(".pending.wav")
                    .is_some_and(|stem| uuid::Uuid::parse_str(stem).is_ok())
            };
            if owned {
                std::fs::remove_file(path).map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn startup_cleanup_only_removes_owned_staging_files_and_keeps_committed_links() {
        let root = std::env::temp_dir().join(format!("golos-owned-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("incoming")).unwrap();
        std::fs::create_dir_all(root.join("audio")).unwrap();
        let id = uuid::Uuid::new_v4();
        let pending = root.join("audio").join(format!("{id}.pending.wav"));
        let audio = root.join("audio").join(format!("{id}.wav"));
        std::fs::write(&pending, "Complete recording").unwrap();
        std::fs::hard_link(&pending, &audio).unwrap();
        let raw = root
            .join("incoming")
            .join(format!("{}.wav", uuid::Uuid::new_v4()));
        std::fs::write(&raw, "temporary").unwrap();
        let unknown = root.join("incoming").join("keep.wav");
        std::fs::write(&unknown, "User file").unwrap();
        recover_files(&root).unwrap();
        assert!(!raw.exists());
        assert!(!pending.exists());
        assert_eq!(
            std::fs::read_to_string(&audio).unwrap(),
            "Complete recording"
        );
        assert_eq!(std::fs::read_to_string(unknown).unwrap(), "User file");
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "Set GOLOS_TEST_MODEL and GOLOS_TEST_AUDIO to a local Whisper model and the public JFK sample"]
    fn watched_audio_exports_offline() {
        use crate::{auto_export, batch, engine, jobs, library, watch};
        use std::{
            fs,
            sync::{atomic::AtomicBool, Arc},
        };
        whisper_rs::install_logging_hooks();
        let model = std::env::var("GOLOS_TEST_MODEL").expect("GOLOS_TEST_MODEL required");
        let sample = std::env::var("GOLOS_TEST_AUDIO").expect("GOLOS_TEST_AUDIO required");
        let root = std::env::temp_dir().join(format!("golos-watch-real-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("store/audio")).unwrap();
        fs::create_dir(root.join("source")).unwrap();
        fs::create_dir(root.join("output")).unwrap();
        let root = root.canonicalize().unwrap();
        let store = root.join("store");
        let source = root.join("source/JFK.wav");
        fs::copy(&sample, &source).unwrap();
        let original = fs::read(&source).unwrap();
        let db = Connection::open(store.join("library.sqlite3")).unwrap();
        library::init(&db, &store.join("library.sqlite3")).unwrap();
        let id = add(&db, &root.join("source"), "en", true).unwrap();
        let folder = folders(&db).unwrap().pop().unwrap();
        assert_eq!(id, folder.id);
        let observed = watch::entries(&folder).unwrap();
        watch::observe(&db, &folder, &observed, 0, false).unwrap();
        let entry = watch::candidate(&db, &folder, watch::STABLE_MS)
            .unwrap()
            .unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let snapshot = watch::snapshot(&folder, &entry, &store, &cancelled).unwrap();
        let note = batch::prepare(&store, &snapshot.path, &cancelled)
            .unwrap()
            .title("Offline JFK pipeline")
            .commit_with(&db, Some("en"), |db, note| {
                db.execute(
                    "INSERT INTO watch_imports(folder_id,digest,note_id) VALUES(?1,?2,?3)",
                    params![folder.id, snapshot.digest, note.id],
                )
                .map_err(|e| e.to_string())?;
                watch::status(db, &folder.id, &entry.name, "imported", None)
            })
            .unwrap();
        let (queued, language) = jobs::next_for(&db, Some(&note.id)).unwrap().unwrap();
        assert_eq!(queued, note.id);
        let result = engine::transcribe(
            Path::new(&model),
            &store.join("audio").join(format!("{}.wav", note.id)),
            &language,
            cancelled.clone(),
            |_| {},
        );
        assert!(result.is_ok());
        jobs::finish(&db, &note.id, &result, false).unwrap();
        db.execute("UPDATE automation_settings SET export_enabled=1,export_directory=?1,export_formats='[\"json\",\"vtt\",\"docx\"]' WHERE id=1",[root.join("output").to_str().unwrap()]).unwrap();
        schedule_export(&db, &note.id).unwrap();
        auto_export::process(&db, &note.id, &cancelled).unwrap();
        assert_eq!(auto_export::list(&db).unwrap()[0].status, "done");
        assert_eq!(fs::read_dir(root.join("output")).unwrap().count(), 3);
        let complete = library::get(&db, &note.id).unwrap();
        assert!(complete
            .segments
            .iter()
            .any(|segment| segment.text.to_lowercase().contains("country")));
        assert_eq!(fs::read(&source).unwrap(), original);
        drop(snapshot);
        drop(db);
        fs::remove_dir_all(root).unwrap();
    }
}
