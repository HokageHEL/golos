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
}

/// Open the library schema for use: migrate to the current version (see
/// [`migrations::migrate`]) and make interrupted recordings retryable.
pub fn init(db: &Connection, db_path: &Path) -> Result<Migration, String> {
    migrations::migrate(db, db_path)
}

pub fn list(db: &Connection) -> Result<Vec<Note>, String> {
    let mut statement = db
        .prepare(
            "SELECT id,title,created_at,duration_seconds,status,segments,error
        FROM notes ORDER BY created_at DESC, id DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |row| {
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
            })
        })
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
