use crate::{
    import, jobs,
    library::{self, Note},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Owns only newly reserved files; dropping an incomplete import rolls them back.
pub struct PreparedImport {
    note: Note,
    pending: PathBuf,
    target: PathBuf,
    published: bool,
    committed: bool,
}
impl Drop for PreparedImport {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.pending);
        if self.published && !self.committed {
            let _ = fs::remove_file(&self.target);
        }
    }
}
impl PreparedImport {
    pub fn commit(mut self, db: &Connection, language: Option<&str>) -> Result<Note, String> {
        fs::hard_link(&self.pending, &self.target)
            .map_err(|e| format!("Cannot publish imported audio: {e}"))?;
        self.published = true;
        let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
            .map_err(|e| e.to_string())?;
        library::insert(&tx, &self.note)?;
        if let Some(language) = language {
            jobs::enqueue(&tx, &self.note.id, language)?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        self.committed = true;
        Ok(self.note.clone())
    }
}

/// Decode without holding the library mutex; only publication needs a short DB transaction.
pub fn prepare(
    root: &Path,
    source: &Path,
    cancelled: &AtomicBool,
) -> Result<PreparedImport, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let pending = root.join("audio").join(format!("{id}.pending.wav"));
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    let mut prepared = PreparedImport {
        target: root.join("audio").join(format!("{id}.wav")),
        pending,
        published: false,
        committed: false,
        note: Note {
            id,
            title: source
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .char_indices()
                .take_while(|(index, ch)| index + ch.len_utf8() <= 200)
                .map(|(_, ch)| ch)
                .collect(),
            created_at: 0,
            duration_seconds: 0.0,
            status: "ready".into(),
            segments: vec![],
            error: None,
            folder: String::new(),
            tags: vec![],
        },
    };
    prepared.note.duration_seconds =
        import::normalize_cancellable(source, &prepared.pending, cancelled)?;
    if cancelled.load(Ordering::SeqCst) {
        return Err("Import cancelled. Your source file was not changed.".into());
    }
    prepared.note.created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    Ok(prepared)
}

#[cfg(test)]
fn import_source(
    db: &Connection,
    root: &Path,
    source: &Path,
    cancelled: &AtomicBool,
    language: Option<&str>,
) -> Result<Note, String> {
    prepare(root, source, cancelled)?.commit(db, language)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_queue_is_atomic_cancellable_and_preserves_sources() {
        let root = std::env::temp_dir().join(format!("golos-batch-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let source = root.join("Interview.wav");
        let mut writer = hound::WavWriter::create(
            &source,
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
        let original = fs::read(&source).unwrap();
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new(":memory:")).unwrap();
        assert!(
            import_source(&db, &root, &source, &AtomicBool::new(true), Some("auto"))
                .unwrap_err()
                .contains("cancelled")
        );
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 0);
        let one = import_source(&db, &root, &source, &AtomicBool::new(false), Some("uk")).unwrap();
        let two = import_source(&db, &root, &source, &AtomicBool::new(false), Some("en")).unwrap();
        assert_ne!(one.id, two.id);
        assert_eq!(jobs::list(&db).unwrap().len(), 2);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 2);
        // A failure while queueing rolls back the note and removes only its new audio.
        db.execute_batch("CREATE TRIGGER refuse_jobs BEFORE INSERT ON transcription_jobs BEGIN SELECT RAISE(ABORT,'disk full simulation'); END;").unwrap();
        assert!(import_source(&db, &root, &source, &AtomicBool::new(false), Some("auto")).is_err());
        assert_eq!(library::list(&db).unwrap().len(), 2);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 2);
        assert_eq!(fs::read(&source).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }
}
