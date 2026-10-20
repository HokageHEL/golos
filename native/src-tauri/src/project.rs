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

const MANIFEST_LIMIT: u64 = 64_000_000;
const AUDIO_LIMIT: u64 = 16_000 * 2 * 1800 + 4096;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    note: Note,
    #[serde(default, rename = "aiResults", skip_serializing_if = "Vec::is_empty")]
    ai_results: Vec<crate::ai::Work>,
}

pub fn export(path: &Path, audio: &Path, note: &Note, db: &Connection) -> Result<(), String> {
    let ai_results = portable_results(db, &note.id)?;
    let manifest = serde_json::to_vec(&Manifest {
        version: if ai_results.is_empty() { 1 } else { 2 },
        note: note.clone(),
        ai_results,
    })
    .map_err(|e| e.to_string())?;
    if manifest.len() as u64 > MANIFEST_LIMIT {
        return Err("Project metadata exceeds 64 MB. Export results separately before creating this backup.".into());
    }
    let file = fs::File::create(path).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("project.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(&manifest).map_err(|e| e.to_string())?;
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
    #[derive(Deserialize)]
    struct Version {
        version: u32,
    }
    let version: Version =
        serde_json::from_str(&json).map_err(|e| format!("Invalid project manifest: {e}"))?;
    if ![1, 2].contains(&version.version) {
        return Err("Unsupported project version.".into());
    }
    let manifest: Manifest =
        serde_json::from_str(&json).map_err(|e| format!("Invalid project manifest: {e}"))?;
    if ![1, 2].contains(&manifest.version)
        || (manifest.version == 1 && !manifest.ai_results.is_empty())
    {
        return Err("Unsupported project version.".into());
    }
    let mut note = manifest.note;
    if manifest.ai_results.len() > 1000 {
        return Err("Too many local AI results in this project.".into());
    }
    for result in &manifest.ai_results {
        validate_result(result, &note)?;
    }
    let mut ai_results = manifest.ai_results;
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
    let mut conversations = std::collections::HashMap::new();
    for result in &mut ai_results {
        result.id = uuid::Uuid::new_v4().to_string();
        result.note_id = note.id.clone();
        if !result.conversation.is_empty() {
            result.conversation = conversations
                .entry(result.conversation.clone())
                .or_insert_with(|| uuid::Uuid::new_v4().to_string())
                .clone();
        }
        if result.status == "running" {
            result.status = "failed".into();
            result.error=Some("Generation was in progress when this backup was made. Retry locally to generate a new result.".into());
            result.phase = "Imported interrupted result".into();
        }
    }
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
        for result in &ai_results {
            crate::ai::insert(&tx, result)?;
        }
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
fn portable_results(db: &Connection, note_id: &str) -> Result<Vec<crate::ai::Work>, String> {
    let (count,size):(i64,i64)=db.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(source AS BLOB))+length(CAST(content AS BLOB))+length(CAST(segments AS BLOB))+length(CAST(prompt AS BLOB))+4096),0) FROM ai_results WHERE note_id=?1",[note_id],|row|Ok((row.get(0)?,row.get(1)?))).map_err(|e|e.to_string())?;
    if count > 1000 || size > 60_000_000 {
        return Err("Saved AI results exceed the portable project limit (1,000 results / 60 MB). Export results separately first; none were omitted.".into());
    }
    let ids: Vec<String> = db
        .prepare("SELECT id FROM ai_results WHERE note_id=?1 ORDER BY rowid")
        .map_err(|e| e.to_string())?
        .query_map([note_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    ids.iter().map(|id| crate::ai::get(db, id)).collect()
}
fn validate_result(work: &crate::ai::Work, note: &Note) -> Result<(), String> {
    if work.note_id != note.id
        || uuid::Uuid::parse_str(&work.id).is_err()
        || !crate::ai::KINDS.contains(&work.kind.as_str())
        || !["running", "done", "failed", "cancelled"].contains(&work.status.as_str())
        || work.target.len() > 80
        || work.prompt.len() > 4000
        || work.source_language.len() > 16
        || work.model.len() > 200
        || work.content.len() > 2_000_000
        || work.phase.len() > 1000
        || work.error.as_ref().is_some_and(|e| e.len() > 10000)
        || work.created_at < 0
        || (!work.conversation.is_empty() && uuid::Uuid::parse_str(&work.conversation).is_err())
        || (work.kind != "speech_english" && !(8192..=32768).contains(&work.context_window))
        || work.source_hash != crate::ai::hash(&work.source)?
    {
        return Err("Invalid saved AI result metadata or source checksum.".into());
    }
    for segments in [&work.source, &work.segments] {
        crate::library::validate_segments(segments, note.duration_seconds)?;
        if serde_json::to_vec(segments)
            .map_err(|e| e.to_string())?
            .len()
            > 2_000_000
        {
            return Err("Saved AI transcript exceeds 2 MB.".into());
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ai_backups_remap_ids_preserve_history_and_reject_corrupt_sources() {
        let (db, mut request) = crate::ai::tests::fixture();
        let root = std::env::temp_dir().join(format!("golos-ai-backup-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let audio = root.join("source.wav");
        let mut wav = hound::WavWriter::create(
            &audio,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..32000 {
            wav.write_sample(0i16).unwrap();
        }
        wav.finalize().unwrap();
        request.kind = "chat".into();
        request.prompt = "When?".into();
        let mut work = crate::ai::create(&db, &request, "local:latest", 8192).unwrap();
        work.status = "done".into();
        work.content = "Friday [s1]".into();
        crate::ai::checkpoint(&db, &work).unwrap();
        request.conversation = work.conversation.clone();
        let pending = crate::ai::create(&db, &request, "local:latest", 8192).unwrap();
        let note = library::get(&db, "source").unwrap();
        let path = root.join("backup.golos");
        export(&path, &audio, &note, &db).unwrap();
        let a = import(&path, &root, &db).unwrap();
        let b = import(&path, &root, &db).unwrap();
        let ar = crate::ai::list(&db, &a.id).unwrap();
        let br = crate::ai::list(&db, &b.id).unwrap();
        assert_eq!(ar.len(), 2);
        assert_ne!(ar[0].id, work.id);
        assert_ne!(ar[0].conversation, work.conversation);
        assert_ne!(ar[0].conversation, br[0].conversation);
        assert_eq!(ar[0].conversation, ar[1].conversation);
        let imported = crate::ai::get(&db, &ar[1].id).unwrap();
        assert_eq!(imported.content, work.content);
        assert_eq!(imported.source_hash, work.source_hash);
        assert_eq!(ar[0].status, "failed");
        assert_eq!(crate::ai::get(&db, &pending.id).unwrap().status, "running");
        db.execute(
            "UPDATE ai_results SET source_hash='corrupt' WHERE id=?1",
            [&work.id],
        )
        .unwrap();
        export(&path, &audio, &note, &db).unwrap();
        assert!(import(&path, &root, &db).is_err());
        assert_eq!(library::list(&db).unwrap().len(), 3);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
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
        export(&path, &source, &note, &db).unwrap();
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
        export(&path, &source, &bad, &db).unwrap();
        assert!(import(&path, &root, &db).is_err());
        bad.duration_seconds = 1.;
        bad.segments[0].end_ms = 9000;
        export(&path, &source, &bad, &db).unwrap();
        assert!(import(&path, &root, &db).is_err());
        fs::write(&path, b"not a zip").unwrap();
        assert!(import(&path, &root, &db).is_err());
        assert_eq!(library::list(&db).unwrap().len(), 2);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}
