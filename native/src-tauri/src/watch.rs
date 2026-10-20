//! Stable, bounded local folder observation and content-hash deduplication.
use crate::{automation::WatchFolder, import};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};
pub const STABLE_MS: i64 = 5000;
#[derive(Debug)]
pub struct Entry {
    pub name: String,
    pub size: i64,
    pub modified: String,
}
pub fn entries(folder: &WatchFolder) -> Result<Vec<Entry>, String> {
    let mut output = vec![];
    for (index, item) in fs::read_dir(&folder.path)
        .map_err(|e| {
            format!("Cannot read folder. Reselect it or grant Files and Folders access: {e}")
        })?
        .enumerate()
    {
        if index >= 10000 {
            return Err("This folder has more than 10,000 entries. Choose a smaller folder; subfolders are not scanned.".into());
        }
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        if !path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            import::EXTENSIONS
                .iter()
                .any(|known| e.eq_ignore_ascii_case(known))
        }) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let name = item
            .file_name()
            .into_string()
            .map_err(|_| "Use Unicode names for watched audio files.")?;
        output.push(Entry {
            name,
            size: i64::try_from(metadata.len()).map_err(|_| "File size is not supported.")?,
            modified: modified(&metadata)?,
        });
    }
    Ok(output)
}
fn modified(metadata: &fs::Metadata) -> Result<String, String> {
    Ok(metadata
        .modified()
        .map_err(|e| e.to_string())?
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos()
        .to_string())
}
pub fn observe(
    db: &Connection,
    folder: &WatchFolder,
    entries: &[Entry],
    now: i64,
    baseline: bool,
) -> Result<(), String> {
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    observe_rows(&tx, folder, entries, now, baseline)?;
    tx.commit().map_err(|e| e.to_string())
}
pub fn observe_rows(
    db: &Connection,
    folder: &WatchFolder,
    entries: &[Entry],
    now: i64,
    baseline: bool,
) -> Result<(), String> {
    let mut statement=db.prepare_cached("INSERT INTO watch_files(folder_id,name,size,modified,stable_since,status) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(folder_id,name) DO UPDATE SET size=excluded.size,modified=excluded.modified,stable_since=excluded.stable_since,status='waiting',error=NULL WHERE watch_files.size!=excluded.size OR watch_files.modified!=excluded.modified").map_err(|e|e.to_string())?;
    for entry in entries {
        statement
            .execute(params![
                folder.id,
                entry.name,
                entry.size,
                entry.modified,
                now,
                if baseline { "ignored" } else { "waiting" }
            ])
            .map_err(|e| e.to_string())?;
    }
    drop(statement);
    db.execute(
        "UPDATE watch_folders SET error=NULL WHERE id=?1 AND error IS NOT NULL",
        [&folder.id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn candidate(db: &Connection, folder: &WatchFolder, now: i64) -> Result<Option<Entry>, String> {
    db.query_row("SELECT name,size,modified FROM watch_files WHERE folder_id=?1 AND status='waiting' AND stable_since<=?2 ORDER BY rowid LIMIT 1",params![folder.id,now-STABLE_MS],|row|Ok(Entry{name:row.get(0)?,size:row.get(1)?,modified:row.get(2)?})).optional().map_err(|e|e.to_string())
}
pub fn status(
    db: &Connection,
    folder: &str,
    name: &str,
    status: &str,
    error: Option<&str>,
) -> Result<(), String> {
    db.execute(
        "UPDATE watch_files SET status=?3,error=?4 WHERE folder_id=?1 AND name=?2",
        params![folder, name, status, error],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Debug)]
pub struct Snapshot {
    pub path: PathBuf,
    pub digest: String,
}
impl Drop for Snapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
pub fn snapshot(
    folder: &WatchFolder,
    entry: &Entry,
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<Snapshot, String> {
    if entry.size <= 0 || entry.size as u64 > import::MAX_BYTES {
        return Err("Choose a non-empty recording up to 512 MiB.".into());
    }
    if Path::new(&entry.name).components().count() != 1 {
        return Err("Invalid watched filename.".into());
    }
    let source = Path::new(&folder.path).join(&entry.name);
    let validate = || -> Result<(), String> {
        let metadata = fs::symlink_metadata(&source).map_err(|e| e.to_string())?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() != entry.size as u64
            || modified(&metadata)? != entry.modified
            || source.canonicalize().map_err(|e| e.to_string())?.parent()
                != Some(Path::new(&folder.path))
        {
            return Err(
                "The file changed or moved. Wait for it to finish copying, then retry.".into(),
            );
        }
        Ok(())
    };
    validate()?;
    if fs2::available_space(root).map_err(|e| e.to_string())? < entry.size as u64 + 60_000_000 {
        return Err("Not enough free disk space for the source snapshot and normalized audio. Free space, then retry.".into());
    }
    fs::create_dir_all(root.join("incoming")).map_err(|e| e.to_string())?;
    if fs::symlink_metadata(root.join("incoming"))
        .map_err(|e| e.to_string())?
        .file_type()
        .is_symlink()
    {
        return Err("Golos's staging directory must not be a symbolic link.".into());
    }
    let path = root.join("incoming").join(format!(
        "{}.{}",
        uuid::Uuid::new_v4(),
        source.extension().and_then(|e| e.to_str()).unwrap_or("wav")
    ));
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let mut snapshot = Snapshot {
        path,
        digest: String::new(),
    };
    let mut input = fs::File::open(&source).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 65536];
    let mut size = 0_i64;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Folder import cancelled.".into());
        }
        let count = input.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        size += count as i64;
        if size > entry.size {
            return Err("The source is still growing. Wait for copying to finish.".into());
        }
        hash.update(&buffer[..count]);
        output
            .write_all(&buffer[..count])
            .map_err(|e| e.to_string())?;
    }
    if size != entry.size {
        return Err("The source changed while copying. Retry when it is stable.".into());
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    validate()?;
    snapshot.digest = format!("{:x}", hash.finalize());
    Ok(snapshot)
}
pub fn duplicate(db: &Connection, folder: &str, digest: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM watch_imports WHERE folder_id=?1 AND digest=?2)",
        params![folder, digest],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{automation, batch, library};
    struct Fixture {
        root: PathBuf,
        store: PathBuf,
        folder: WatchFolder,
        db: Connection,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("golos-watch-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("source")).unwrap();
            fs::create_dir_all(root.join("store/audio")).unwrap();
            let root = root.canonicalize().unwrap();
            let store = root.join("store");
            let db = Connection::open(store.join("library.sqlite3")).unwrap();
            library::init(&db, &store.join("library.sqlite3")).unwrap();
            let path = root.join("source");
            let id = automation::add(&db, &path, "uk", false).unwrap();
            Self {
                root,
                store,
                folder: WatchFolder {
                    id,
                    path: path.to_str().unwrap().into(),
                    enabled: true,
                    auto_transcribe: false,
                    language: "uk".into(),
                    error: None,
                },
                db,
            }
        }
        fn wav(&self, name: &str) {
            let mut writer = hound::WavWriter::create(
                Path::new(&self.folder.path).join(name),
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 16000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for _ in 0..16000 {
                writer.write_sample(100_i16).unwrap();
            }
            writer.finalize().unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Ok(replacement) = Connection::open_in_memory() {
                drop(std::mem::replace(&mut self.db, replacement));
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn waits_for_stability_and_ignores_existing_files_unless_opted_in() {
        let fixture = Fixture::new();
        fixture.wav("existing.wav");
        let initial = entries(&fixture.folder).unwrap();
        observe(&fixture.db, &fixture.folder, &initial, 1000, true).unwrap();
        assert!(candidate(&fixture.db, &fixture.folder, 10000)
            .unwrap()
            .is_none());
        fixture.wav("new.wav");
        let current = entries(&fixture.folder).unwrap();
        observe(&fixture.db, &fixture.folder, &current, 11000, false).unwrap();
        assert!(candidate(&fixture.db, &fixture.folder, 15999)
            .unwrap()
            .is_none());
        assert_eq!(
            candidate(&fixture.db, &fixture.folder, 16000)
                .unwrap()
                .unwrap()
                .name,
            "new.wav"
        );
        let path = Path::new(&fixture.folder.path).join("new.wav");
        fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        observe(
            &fixture.db,
            &fixture.folder,
            &entries(&fixture.folder).unwrap(),
            16000,
            false,
        )
        .unwrap();
        assert!(candidate(&fixture.db, &fixture.folder, 20000)
            .unwrap()
            .is_none());
        assert!(candidate(&fixture.db, &fixture.folder, 21000)
            .unwrap()
            .is_some());
    }
    #[test]
    fn snapshots_and_deduplication_commit_with_the_note_and_survive_reopen() {
        let fixture = Fixture::new();
        fixture.wav("Interview.wav");
        let original = fs::read(Path::new(&fixture.folder.path).join("Interview.wav")).unwrap();
        let entries = entries(&fixture.folder).unwrap();
        observe(&fixture.db, &fixture.folder, &entries, 0, false).unwrap();
        let entry = candidate(&fixture.db, &fixture.folder, STABLE_MS)
            .unwrap()
            .unwrap();
        let snapshot = snapshot(
            &fixture.folder,
            &entry,
            &fixture.store,
            &AtomicBool::new(false),
        )
        .unwrap();
        let digest = snapshot.digest.clone();
        let note = batch::prepare(&fixture.store, &snapshot.path, &AtomicBool::new(false))
            .unwrap()
            .title("Interview")
            .commit_with(&fixture.db, Some("uk"), |db, note| {
                db.execute(
                    "INSERT INTO watch_imports(folder_id,digest,note_id) VALUES(?1,?2,?3)",
                    params![fixture.folder.id, digest, note.id],
                )
                .unwrap();
                status(db, &fixture.folder.id, &entry.name, "imported", None)
            })
            .unwrap();
        assert_eq!(note.title, "Interview");
        assert_eq!(crate::jobs::list(&fixture.db).unwrap().len(), 1);
        assert!(duplicate(&fixture.db, &fixture.folder.id, &digest).unwrap());
        drop(snapshot);
        let reopened = Connection::open(fixture.store.join("library.sqlite3")).unwrap();
        library::init(&reopened, &fixture.store.join("library.sqlite3")).unwrap();
        assert!(duplicate(&reopened, &fixture.folder.id, &digest).unwrap());
        reopened
            .execute("DELETE FROM notes WHERE id=?1", [note.id])
            .unwrap();
        assert!(
            duplicate(&reopened, &fixture.folder.id, &digest).unwrap(),
            "Deleting a note must not resurrect watched input"
        );
        assert_eq!(
            fs::read(Path::new(&fixture.folder.path).join("Interview.wav")).unwrap(),
            original
        );
        assert_eq!(
            fs::read_dir(fixture.store.join("incoming"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn rejects_changed_cancelled_and_oversized_sources_without_partial_snapshots() {
        let fixture = Fixture::new();
        fixture.wav("one.wav");
        let observed = entries(&fixture.folder).unwrap();
        let entry = &observed[0];
        assert!(snapshot(
            &fixture.folder,
            entry,
            &fixture.store,
            &AtomicBool::new(true)
        )
        .is_err());
        assert_eq!(
            fs::read_dir(fixture.store.join("incoming"))
                .unwrap()
                .count(),
            0
        );
        fs::OpenOptions::new()
            .append(true)
            .open(Path::new(&fixture.folder.path).join("one.wav"))
            .unwrap()
            .write_all(b"changed")
            .unwrap();
        assert!(snapshot(
            &fixture.folder,
            entry,
            &fixture.store,
            &AtomicBool::new(false)
        )
        .is_err());
        let oversized = Entry {
            name: "huge.wav".into(),
            size: import::MAX_BYTES as i64 + 1,
            modified: String::new(),
        };
        assert!(snapshot(
            &fixture.folder,
            &oversized,
            &fixture.store,
            &AtomicBool::new(false)
        )
        .unwrap_err()
        .contains("512 MiB"));
        let traversal = Entry {
            name: "../outside.wav".into(),
            size: 1,
            modified: String::new(),
        };
        assert!(snapshot(
            &fixture.folder,
            &traversal,
            &fixture.store,
            &AtomicBool::new(false)
        )
        .is_err());
    }
    #[cfg(unix)]
    #[test]
    fn skips_links_and_subfolders_and_rejects_private_storage_as_a_grant() {
        let fixture = Fixture::new();
        fixture.wav("one.wav");
        std::os::unix::fs::symlink(
            Path::new(&fixture.folder.path).join("one.wav"),
            Path::new(&fixture.folder.path).join("link.wav"),
        )
        .unwrap();
        fs::create_dir(Path::new(&fixture.folder.path).join("nested.wav")).unwrap();
        assert_eq!(entries(&fixture.folder).unwrap().len(), 1);
        assert!(automation::directory(&fixture.store, &fixture.store).is_err());
        assert!(automation::directory(&fixture.store.join("audio"), &fixture.store).is_err());
    }
    #[test]
    fn failed_import_is_retryable_and_metadata_is_retained_when_paused() {
        let fixture = Fixture::new();
        fixture.wav("one.wav");
        observe(
            &fixture.db,
            &fixture.folder,
            &entries(&fixture.folder).unwrap(),
            0,
            false,
        )
        .unwrap();
        status(
            &fixture.db,
            &fixture.folder.id,
            "one.wav",
            "failed",
            Some("Decode failed"),
        )
        .unwrap();
        assert!(candidate(&fixture.db, &fixture.folder, 10000)
            .unwrap()
            .is_none());
        automation::action(&fixture.db, &fixture.folder.id, "retry").unwrap();
        assert!(candidate(&fixture.db, &fixture.folder, 10000)
            .unwrap()
            .is_some());
        automation::action(&fixture.db, &fixture.folder.id, "pause").unwrap();
        assert!(!automation::folders(&fixture.db).unwrap()[0].enabled);
        automation::action(&fixture.db, &fixture.folder.id, "remove").unwrap();
        assert!(automation::files(&fixture.db).unwrap().is_empty());
        assert!(Path::new(&fixture.folder.path).join("one.wav").exists());
    }
    #[test]
    fn failed_deduplication_commit_rolls_back_the_note_audio_and_seen_digest() {
        let fixture = Fixture::new();
        fixture.wav("one.wav");
        let observed = entries(&fixture.folder).unwrap();
        let snapshot = snapshot(
            &fixture.folder,
            &observed[0],
            &fixture.store,
            &AtomicBool::new(false),
        )
        .unwrap();
        let digest = snapshot.digest.clone();
        let result = batch::prepare(&fixture.store, &snapshot.path, &AtomicBool::new(false))
            .unwrap()
            .commit_with(&fixture.db, Some("uk"), |db, note| {
                db.execute(
                    "INSERT INTO watch_imports(folder_id,digest,note_id) VALUES(?1,?2,?3)",
                    params![fixture.folder.id, digest, note.id],
                )
                .unwrap();
                Err("Simulated commit failure".into())
            });
        assert!(result.is_err());
        assert!(library::list(&fixture.db).unwrap().is_empty());
        assert!(crate::jobs::list(&fixture.db).unwrap().is_empty());
        assert!(!duplicate(&fixture.db, &fixture.folder.id, &digest).unwrap());
        assert_eq!(
            fs::read_dir(fixture.store.join("audio")).unwrap().count(),
            0
        );
    }
}
