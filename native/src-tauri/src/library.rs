use crate::migrations::{self, Migration};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub duration_seconds: f64,
    pub status: String,
    pub segments: Vec<Segment>,
    pub error: Option<String>,
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Open the library schema for use: migrate to the current version (see
/// [`migrations::migrate`]) and make interrupted recordings retryable.
pub fn init(db: &Connection, db_path: &Path) -> Result<Migration, String> {
    migrations::migrate(db, db_path)
}

#[cfg(test)]
pub fn list(db: &Connection) -> Result<Vec<Note>, String> {
    {
        let ids: Vec<String> = db
            .prepare("SELECT id FROM notes ORDER BY created_at DESC,id DESC")
            .map_err(|e| e.to_string())?
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
        ids.iter().map(|id| get(db, id)).collect()
    }
}

pub fn get(db: &Connection, id: &str) -> Result<Note, String> {
    query(db, "", "", "", "", 1, 0, Some(id))?
        .pop()
        .ok_or("Recording not found.".into())
}

#[allow(clippy::too_many_arguments)]
pub fn query(
    db: &Connection,
    search: &str,
    status: &str,
    folder: &str,
    tag: &str,
    limit: i64,
    offset: i64,
    id: Option<&str>,
) -> Result<Vec<Note>, String> {
    let mut statement = db
        .prepare(
            "SELECT n.id,title,created_at,duration_seconds,status,CASE WHEN ?7 IS NULL THEN '[]' ELSE segments END,error,COALESCE(o.folder,''),COALESCE(o.tags,'[]')
             FROM notes n LEFT JOIN organization o ON o.note_id=n.id
             WHERE (?7 IS NULL OR n.id=?7)
             AND (?1='' OR instr(lower(title),lower(?1))>0 OR EXISTS(SELECT 1 FROM json_each(segments) s WHERE instr(lower(json_extract(s.value,'$.text')),lower(?1))>0))
             AND (?2='' OR status=?2) AND (?3='' OR o.folder=?3)
             AND (?4='' OR EXISTS(SELECT 1 FROM json_each(o.tags) WHERE value=?4))
             ORDER BY created_at DESC,n.id DESC LIMIT ?5 OFFSET ?6",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map(
            params![
                search,
                status,
                folder,
                tag,
                limit.clamp(1, 1000),
                offset.max(0),
                id
            ],
            |row| {
                let json: String = row.get(5)?;
                let segments = serde_json::from_str(&json).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                Ok(Note {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    created_at: row.get(2)?,
                    duration_seconds: row.get(3)?,
                    status: row.get(4)?,
                    segments,
                    error: row.get(6)?,
                    folder: row.get(7)?,
                    tags: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
                })
            },
        )
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn insert(db: &Connection, note: &Note) -> Result<(), String> {
    db.execute(
        "INSERT INTO notes(id,title,created_at,duration_seconds,status) VALUES(?1,?2,?3,?4,?5)",
        params![
            note.id,
            note.title,
            note.created_at,
            note.duration_seconds,
            note.status
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn organize(
    db: &Connection,
    id: &str,
    title: &str,
    folder: &str,
    tags: &[String],
) -> Result<(), String> {
    if title.trim().is_empty()
        || title.len() > 200
        || folder.len() > 200
        || tags.len() > 30
        || tags.iter().any(|t| t.trim().is_empty() || t.len() > 100)
    {
        return Err("Use a title/folder up to 200 bytes and at most 30 tags of 100 bytes.".into());
    }
    let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
    if tx
        .execute(
            "UPDATE notes SET title=?1 WHERE id=?2",
            params![title.trim(), id],
        )
        .map_err(|e| e.to_string())?
        != 1
    {
        return Err("Recording not found.".into());
    }
    tx.execute("INSERT INTO organization(note_id,folder,tags) VALUES(?1,?2,?3) ON CONFLICT(note_id) DO UPDATE SET folder=excluded.folder,tags=excluded.tags",
        params![id,folder.trim(),serde_json::to_string(tags).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn validate_segments(segments: &[Segment], duration: f64) -> Result<(), String> {
    if segments.len() > 20_000 {
        return Err("Too many transcript segments.".into());
    }
    let mut previous_start = 0;
    for segment in segments {
        if segment.start_ms < previous_start
            || segment.end_ms < segment.start_ms
            || segment.end_ms > (duration * 1000.0).ceil() as i64 + 1000
            || segment.text.len() > 100_000
            || segment
                .speaker
                .as_ref()
                .is_some_and(|speaker| speaker.len() > 100)
        {
            return Err("Invalid transcript timestamps or text length.".into());
        }
        previous_start = segment.start_ms;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_organization_and_pagination_preserve_saved_text() {
        let db = Connection::open_in_memory().unwrap();
        init(&db, Path::new("in-memory")).unwrap();
        for n in 0..125 {
            insert(
                &db,
                &Note {
                    id: format!("{n:03}"),
                    title: format!("Meeting {n}"),
                    created_at: n,
                    duration_seconds: 3.,
                    status: "done".into(),
                    segments: vec![],
                    error: None,
                    folder: String::new(),
                    tags: vec![],
                },
            )
            .unwrap();
        }
        db.execute(
            "UPDATE notes SET segments=?1 WHERE id='001'",
            [r#"[{"startMs":0,"endMs":2000,"text":"needle in saved text","speaker":"Alice"}]"#],
        )
        .unwrap();
        organize(&db, "001", "Renamed", "Interviews", &["research".into()]).unwrap();
        let found = query(
            &db,
            "needle",
            "done",
            "Interviews",
            "research",
            100,
            0,
            None,
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].segments.is_empty());
        assert_eq!(
            get(&db, "001").unwrap().segments[0].speaker.as_deref(),
            Some("Alice")
        );
        assert_eq!(
            query(&db, "", "", "", "", 100, 100, None).unwrap().len(),
            25
        );
        assert!(query(&db, "needle", "ready", "", "", 100, 0, None)
            .unwrap()
            .is_empty());
        assert!(organize(&db, "001", "", "", &[]).is_err());
        assert_eq!(get(&db, "001").unwrap().title, "Renamed");
    }

    #[test]
    fn persists_edits_and_recovers_interrupted_jobs() {
        let db = Connection::open_in_memory().unwrap();
        let path = Path::new("in-memory");
        init(&db, path).unwrap();
        insert(
            &db,
            &Note {
                id: "one".into(),
                title: "Interview".into(),
                created_at: 1,
                duration_seconds: 3.0,
                status: "processing".into(),
                segments: vec![],
                error: None,
                folder: String::new(),
                tags: vec![],
            },
        )
        .unwrap();
        init(&db, path).unwrap();
        let notes = list(&db).unwrap();
        assert_eq!(notes[0].status, "failed");
        assert!(notes[0].error.as_ref().unwrap().contains("interrupted"));
        let segments = vec![Segment {
            start_ms: 0,
            end_ms: 1000,
            speaker: None,
            text: "Привіт".into(),
        }];
        db.execute(
            "UPDATE notes SET segments=?1 WHERE id='one'",
            [serde_json::to_string(&segments).unwrap()],
        )
        .unwrap();
        assert_eq!(list(&db).unwrap()[0].segments[0].text, "Привіт");
    }
    #[test]
    fn rejects_bad_timestamps() {
        assert!(validate_segments(
            &[Segment {
                start_ms: -1,
                end_ms: 1000,
                speaker: None,
                text: String::new()
            }],
            3.0
        )
        .is_err());
        assert!(validate_segments(
            &[Segment {
                start_ms: 0,
                end_ms: 9999,
                speaker: None,
                text: String::new()
            }],
            3.0
        )
        .is_err());
    }
}
