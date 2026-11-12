//! Recover filesystem/database operations without changing the library schema.
//! Each operation owns only its UUID-named journal directory and staged file.
use crate::{
    audio_path,
    library::{self, Note},
};
use rusqlite::Connection;
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

fn sync_dir(path: &Path) -> Result<(), String> {
    // Windows does not generally permit opening directories as ordinary files.
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn real_dir(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(format!("Not a library directory: {}", path.display()));
    }
    Ok(())
}

fn journal(root: &Path) -> Result<PathBuf, String> {
    let audio = root.join("audio");
    real_dir(&audio)?;
    let dir = audio.join(".operations");
    match fs::create_dir(&dir) {
        Ok(()) => sync_dir(&audio)?,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.to_string()),
    }
    real_dir(&dir)?;
    Ok(dir)
}

fn operation(root: &Path, kind: &str, id: &str) -> Result<PathBuf, String> {
    // Canonical UUID spelling prevents aliases and path traversal in journal names.
    if uuid::Uuid::parse_str(id)
        .map_err(|_| "Invalid recording ID.")?
        .to_string()
        != id
    {
        return Err("Invalid recording ID.".into());
    }
    let parent = journal(root)?;
    let op = parent.join(format!("{kind}-{id}"));
    fs::create_dir(&op).map_err(|e| e.to_string())?;
    sync_dir(&parent)?;
    Ok(op)
}

fn exists(db: &Connection, id: &str) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM notes WHERE id=?1)",
        [id],
        |r| r.get(0),
    )
    .map_err(|e| e.to_string())
}

fn regular(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => Ok(true),
        Ok(_) => Err(format!("Expected a regular audio file: {}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

fn finish(op: &Path) -> Result<(), String> {
    fs::remove_dir(op).map_err(|e| e.to_string())?;
    sync_dir(op.parent().ok_or("Invalid operation directory")?)
}

// Publish without replacing an existing file, even if something changed since staging.
fn publish(stage: &Path, target: &Path) -> Result<(), String> {
    if !regular(stage)? {
        return Err("Staged audio is missing.".into());
    }
    fs::hard_link(stage, target).map_err(|e| format!("Cannot publish audio: {e}"))?;
    sync_dir(target.parent().ok_or("Invalid audio path")?)?;
    Ok(())
}

fn same_contents(a: &Path, b: &Path) -> Result<bool, String> {
    let mut a = fs::File::open(a).map_err(|e| e.to_string())?;
    let mut b = fs::File::open(b).map_err(|e| e.to_string())?;
    if a.metadata().map_err(|e| e.to_string())?.len()
        != b.metadata().map_err(|e| e.to_string())?.len()
    {
        return Ok(false);
    }
    let mut left = [0; 65536];
    let mut right = [0; 65536];
    loop {
        let n = a.read(&mut left).map_err(|e| e.to_string())?;
        b.read_exact(&mut right[..n]).map_err(|e| e.to_string())?;
        if left[..n] != right[..n] {
            return Ok(false);
        }
        if n == 0 {
            return Ok(true);
        }
    }
}

fn reconcile(root: &Path, db: &Connection, kind: &str, id: &str, op: &Path) -> Result<(), String> {
    real_dir(op)?;
    let stage = op.join("audio.wav");
    let target = audio_path(root, id)?;
    let staged = regular(&stage)?;
    let note = exists(db, id)?;
    if !staged {
        return finish(op);
    }
    let present = regular(&target)?;
    match kind {
        "import" if note && staged && !present => publish(&stage, &target)?,
        "delete" if note && staged && !present => publish(&stage, &target)?,
        // A crash after a hard link leaves both names. Accept only identical content;
        // otherwise retain both and ask for manual intervention.
        "import" | "delete" if note && staged && present && !same_contents(&stage, &target)? => {
            return Err("Cannot recover audio over a different existing file.".into())
        }
        _ => (),
    }
    if staged && note {
        sync_dir(target.parent().ok_or("Invalid audio path")?)?;
    }
    if staged {
        fs::remove_file(&stage).map_err(|e| e.to_string())?;
        sync_dir(op)?;
    }
    finish(op)
}

pub fn recover(root: &Path, db: &Connection) -> Result<(), String> {
    let parent = journal(root)?;
    for entry in fs::read_dir(&parent).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((kind, id)) = name.split_once('-') else {
            continue;
        };
        if !["import", "delete"].contains(&kind)
            || uuid::Uuid::parse_str(id)
                .ok()
                .is_none_or(|u| u.to_string() != id)
        {
            continue;
        }
        reconcile(root, db, kind, id, &entry.path())?;
    }
    Ok(())
}

pub fn import_audio(
    root: &Path,
    db: &Connection,
    note: &mut Note,
    write: impl FnOnce(&Path) -> Result<f64, String>,
) -> Result<(), String> {
    if regular(&audio_path(root, &note.id)?)? || exists(db, &note.id)? {
        return Err("Recording ID already exists.".into());
    }
    let op = operation(root, "import", &note.id)?;
    let stage = op.join("audio.wav");
    let mut committed = false;
    let result = (|| {
        if regular(&audio_path(root, &note.id)?)? || exists(db, &note.id)? {
            return Err("Recording ID already exists.".into());
        }
        note.duration_seconds = write(&stage)?;
        if !regular(&stage)? {
            return Err("Decoded audio is missing.".into());
        }
        fs::File::open(&stage)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        sync_dir(&op)?;
        library::insert(db, note)?;
        committed = true;
        publish(&stage, &audio_path(root, &note.id)?)?;
        Ok(())
    })();
    // A committed note is completed by recovery; a failed insert only discards staging.
    match reconcile(root, db, "import", &note.id, &op) {
        // A first publication failure may have been transient. Once recovery
        // completes a committed import, report success rather than inviting a
        // duplicate import of an already stored recording.
        Ok(()) if committed => Ok(()),
        Ok(()) => result,
        Err(error) => Err(format!("Audio recovery could not finish: {error}. Restart Golos to retry; local data was retained.")),
    }
}

pub fn delete_audio(root: &Path, db: &Connection, id: &str) -> Result<(), String> {
    let target = audio_path(root, id)?;
    if !exists(db, id)? {
        return Err("Recording not found.".into());
    }
    regular(&target)?;
    let op = operation(root, "delete", id)?;
    let stage = op.join("audio.wav");
    let result = (|| {
        if regular(&target)? {
            fs::rename(&target, &stage).map_err(|e| e.to_string())?;
            sync_dir(target.parent().ok_or("Invalid audio path")?)?;
            sync_dir(&op)?;
        }
        db.execute("DELETE FROM notes WHERE id=?1", [id])
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    let recovery = reconcile(root, db, "delete", id, &op);
    result.and(recovery)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::migrations;

    struct Fixture {
        root: PathBuf,
        db: Connection,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("golos-recovery-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("audio")).unwrap();
            let db = Connection::open(root.join("library.sqlite3")).unwrap();
            migrations::migrate(&db, &root.join("library.sqlite3")).unwrap();
            Self { root, db }
        }
        fn note() -> Note {
            Note {
                id: uuid::Uuid::new_v4().to_string(),
                title: "test".into(),
                created_at: 1,
                duration_seconds: 1.0,
                status: "ready".into(),
                segments: vec![],
                error: None,
            }
        }
        fn reopen(&mut self) {
            self.db = Connection::open(self.root.join("library.sqlite3")).unwrap();
            recover(&self.root, &self.db).unwrap();
            recover(&self.root, &self.db).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn interrupted_import_boundaries_recover_idempotently() {
        for boundary in 0..4 {
            let mut f = Fixture::new();
            let note = Fixture::note();
            let op = operation(&f.root, "import", &note.id).unwrap();
            let stage = op.join("audio.wav");
            if boundary >= 1 {
                fs::write(&stage, b"audio").unwrap();
            }
            if boundary >= 2 {
                library::insert(&f.db, &note).unwrap();
            }
            if boundary >= 3 {
                publish(&stage, &audio_path(&f.root, &note.id).unwrap()).unwrap();
            }
            f.reopen();
            assert_eq!(exists(&f.db, &note.id).unwrap(), boundary >= 2);
            assert_eq!(
                audio_path(&f.root, &note.id).unwrap().exists(),
                boundary >= 2
            );
            assert!(!op.exists());
        }
    }

    #[test]
    fn interrupted_deletion_restores_or_finishes() {
        for boundary in 0..4 {
            let mut f = Fixture::new();
            let note = Fixture::note();
            library::insert(&f.db, &note).unwrap();
            let target = audio_path(&f.root, &note.id).unwrap();
            fs::write(&target, b"audio").unwrap();
            let op = operation(&f.root, "delete", &note.id).unwrap();
            let stage = op.join("audio.wav");
            if boundary >= 1 {
                fs::rename(&target, &stage).unwrap();
            }
            if boundary >= 2 {
                f.db.execute("DELETE FROM notes WHERE id=?1", [&note.id])
                    .unwrap();
            }
            if boundary >= 3 {
                fs::remove_file(&stage).unwrap();
            }
            f.reopen();
            assert_eq!(exists(&f.db, &note.id).unwrap(), boundary < 2);
            assert_eq!(target.exists(), boundary < 2);
            assert!(!op.exists());
        }
    }

    #[test]
    fn failed_writes_and_database_mutations_restore_state() {
        let f = Fixture::new();
        let mut note = Fixture::note();
        assert!(import_audio(&f.root, &f.db, &mut note, |stage| {
            fs::write(stage, b"partial").unwrap();
            Err("disk full".into())
        })
        .is_err());
        assert!(!exists(&f.db, &note.id).unwrap());
        f.db.execute_batch("CREATE TRIGGER fail_insert BEFORE INSERT ON notes BEGIN SELECT RAISE(FAIL, 'disk full'); END;").unwrap();
        assert!(import_audio(&f.root, &f.db, &mut note, |stage| {
            fs::write(stage, b"audio").unwrap();
            Ok(1.0)
        })
        .is_err());
        assert!(!audio_path(&f.root, &note.id).unwrap().exists());
        f.db.execute_batch("DROP TRIGGER fail_insert;").unwrap();
        import_audio(&f.root, &f.db, &mut note, |stage| {
            fs::write(stage, b"audio").unwrap();
            Ok(1.0)
        })
        .unwrap();
        f.db.execute_batch("CREATE TRIGGER fail_delete BEFORE DELETE ON notes BEGIN SELECT RAISE(FAIL, 'disk full'); END;").unwrap();
        assert!(delete_audio(&f.root, &f.db, &note.id).is_err());
        assert!(exists(&f.db, &note.id).unwrap());
        assert_eq!(
            fs::read(audio_path(&f.root, &note.id).unwrap()).unwrap(),
            b"audio"
        );
    }

    #[test]
    fn unrelated_files_and_missing_audio_are_preserved() {
        let mut f = Fixture::new();
        let note = Fixture::note();
        library::insert(&f.db, &note).unwrap();
        let other = f.root.join("audio/unrelated.wav");
        fs::write(&other, b"other").unwrap();
        let op = operation(&f.root, "delete", &note.id).unwrap();
        f.reopen();
        assert!(!op.exists());
        assert!(exists(&f.db, &note.id).unwrap());
        assert_eq!(fs::read(other).unwrap(), b"other");
        delete_audio(&f.root, &f.db, &note.id).unwrap();
        assert!(!exists(&f.db, &note.id).unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_never_followed() {
        let f = Fixture::new();
        let note = Fixture::note();
        library::insert(&f.db, &note).unwrap();
        let outside = f.root.join("outside");
        fs::write(&outside, b"private").unwrap();
        std::os::unix::fs::symlink(&outside, audio_path(&f.root, &note.id).unwrap()).unwrap();
        assert!(delete_audio(&f.root, &f.db, &note.id).is_err());
        assert!(exists(&f.db, &note.id).unwrap());
        assert_eq!(fs::read(outside).unwrap(), b"private");
    }
}
