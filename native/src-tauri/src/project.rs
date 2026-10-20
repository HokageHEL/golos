//! Portable, versioned ZIP with fixed member names; imports always get a new UUID.
use crate::library::{self, Note};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

const MANIFEST_LIMIT: u64 = 10_000_000;
const AUDIO_LIMIT: u64 = 16_000 * 2 * 1800 + 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    note: Note,
}

pub fn export(path: &Path, audio: &Path, note: &Note) -> Result<(), String> {
    let file = fs::File::create(path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("project.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(
        &serde_json::to_vec(&Manifest {
            version: 1,
            note: note.clone(),
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    zip.start_file("audio.wav", options)
        .map_err(|e| e.to_string())?;
    std::io::copy(
        &mut fs::File::open(audio).map_err(|e| e.to_string())?,
        &mut zip,
    )
    .map_err(|e| e.to_string())?;
    zip.finish()
        .and_then(|f| f.sync_all().map_err(zip::result::ZipError::Io))
        .map_err(|e| e.to_string())
}
pub fn import(path: &Path, root: &Path, db: &Connection) -> Result<Note, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > AUDIO_LIMIT + MANIFEST_LIMIT {
        return Err("Project is too large (30-minute audio limit).".into());
    }
    let mut zip = ZipArchive::new(fs::File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("Invalid project archive: {e}"))?;
    if zip.len() != 2 {
        return Err("Project must contain only project.json and audio.wav.".into());
    }
    let mut json = String::new();
    {
        let mut entry = zip.by_name("project.json").map_err(|e| e.to_string())?;
        if entry.size() > MANIFEST_LIMIT || entry.is_dir() {
            return Err("Invalid project manifest size.".into());
        }
        entry
            .by_ref()
            .take(MANIFEST_LIMIT + 1)
            .read_to_string(&mut json)
            .map_err(|e| e.to_string())?;
        if json.len() as u64 > MANIFEST_LIMIT {
            return Err("Project manifest is too large.".into());
        }
    }
    let manifest: Manifest =
        serde_json::from_str(&json).map_err(|e| format!("Invalid project manifest: {e}"))?;
    if manifest.version != 1 {
        return Err("Unsupported project version.".into());
    }
    let mut note = manifest.note;
    if !note.duration_seconds.is_finite()
        || note.duration_seconds <= 0.
        || note.duration_seconds > 1800.
        || !["ready", "done", "failed"].contains(&note.status.as_str())
        || note.created_at < 0
    {
        return Err("Invalid project recording metadata.".into());
    }
    library::validate_segments(&note.segments, note.duration_seconds)?;
    if note.title.trim().is_empty()
        || note.title.len() > 200
        || note.folder.len() > 200
        || note.tags.len() > 30
        || note
            .tags
            .iter()
            .any(|tag| tag.trim().is_empty() || tag.len() > 100)
    {
        return Err("Invalid title, folder or tags.".into());
    }
    note.id = uuid::Uuid::new_v4().to_string();
    note.error = None;
    let pending = root.join(format!("project-{}.pending.wav", note.id));
    let target = root.join("audio").join(format!("{}.wav", note.id));
    let mut published = false;
    let result = (|| {
        let mut entry = zip.by_name("audio.wav").map_err(|e| e.to_string())?;
        if entry.size() > AUDIO_LIMIT
            || entry.is_dir()
            || entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
        {
            return Err("Invalid project audio.".into());
        }
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        let copied = std::io::copy(&mut entry.by_ref().take(AUDIO_LIMIT + 1), &mut file)
            .map_err(|e| e.to_string())?;
        if copied > AUDIO_LIMIT {
            return Err("Project audio exceeds the limit.".into());
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        // Validate every sample and require the same normalized format as the library.
        let samples = crate::audio::read_audio(&pending)?;
        let actual = samples.len() as f64 / 16000.;
        if (actual - note.duration_seconds).abs() > 0.01 {
            return Err("Project audio duration does not match the manifest.".into());
        }
        note.duration_seconds = actual;
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        library::insert(&tx, &note)?;
        tx.execute(
            "UPDATE notes SET segments=?1 WHERE id=?2",
            params![
                serde_json::to_string(&note.segments).map_err(|e| e.to_string())?,
                note.id
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO organization(note_id,folder,tags) VALUES(?1,?2,?3)",
            params![
                note.id,
                note.folder,
                serde_json::to_string(&note.tags).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        // Hard-link publication cannot replace an existing file, even on collision.
        fs::hard_link(&pending, &target).map_err(|e| e.to_string())?;
        published = true;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(note)
    })();
    let _ = fs::remove_file(pending);
    if result.is_err() && published {
        let _ = fs::remove_file(target);
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_imports_are_distinct_and_invalid_projects_leave_library_untouched() {
        let root = std::env::temp_dir().join(format!("golos-project-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new("in-memory")).unwrap();
        let source = root.join("source.wav");
        let mut wav = hound::WavWriter::create(
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
            wav.write_sample(100i16).unwrap();
        }
        wav.finalize().unwrap();
        let note = Note {
            id: "original".into(),
            title: "Title".into(),
            created_at: 1,
            duration_seconds: 1.,
            status: "done".into(),
            segments: vec![library::Segment {
                start_ms: 0,
                end_ms: 1000,
                speaker: Some("Alice".into()),
                text: "Hello".into(),
            }],
            error: None,
            folder: "Work".into(),
            tags: vec!["test".into()],
        };
        let path = root.join("backup.golos");
        export(&path, &source, &note).unwrap();
        let a = import(&path, &root, &db).unwrap();
        let b = import(&path, &root, &db).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(a.folder, "Work");
        assert_eq!(
            library::get(&db, &a.id).unwrap().segments[0]
                .speaker
                .as_deref(),
            Some("Alice")
        );
        let mut bad = note.clone();
        bad.duration_seconds = 2.;
        export(&path, &source, &bad).unwrap();
        assert!(import(&path, &root, &db).is_err());
        bad.duration_seconds = 1.;
        bad.segments[0].end_ms = 9000;
        export(&path, &source, &bad).unwrap();
        assert!(import(&path, &root, &db).is_err());
        fs::write(&path, b"not a zip").unwrap();
        assert!(import(&path, &root, &db).is_err());
        assert_eq!(library::list(&db).unwrap().len(), 2);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
