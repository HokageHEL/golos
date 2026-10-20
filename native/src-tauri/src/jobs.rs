//! Durable serial jobs. A result and its job state commit together; existing
//! transcripts are never replaced by a queued operation.
use crate::library::Segment;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::Serialize;

pub const SCHEMA: &str = "
CREATE TABLE transcription_jobs (
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 note_id TEXT NOT NULL UNIQUE REFERENCES notes(id) ON DELETE CASCADE,
 language TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('queued','running','done','failed','cancelled')),
 error TEXT,
 created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX transcription_jobs_pending ON transcription_jobs(status,id);
CREATE TRIGGER transcription_jobs_cleanup AFTER DELETE ON notes BEGIN
 DELETE FROM transcription_jobs WHERE note_id=old.id;
END;";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: i64,
    pub note_id: String,
    pub title: String,
    pub duration_seconds: f64,
    pub language: String,
    pub status: String,
    pub error: Option<String>,
}

pub fn list(db: &Connection) -> Result<Vec<Job>, String> {
    db.prepare("SELECT j.id,note_id,title,duration_seconds,language,j.status,j.error FROM transcription_jobs j JOIN notes n ON n.id=j.note_id ORDER BY CASE j.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 WHEN 'failed' THEN 2 WHEN 'cancelled' THEN 3 ELSE 4 END,j.id LIMIT 1000")
        .map_err(|e| e.to_string())?
        .query_map([], |row| Ok(Job { id: row.get(0)?, note_id: row.get(1)?, title: row.get(2)?, duration_seconds: row.get(3)?, language: row.get(4)?, status: row.get(5)?, error: row.get(6)? }))
        .map_err(|e| e.to_string())?.collect::<Result<_,_>>().map_err(|e| e.to_string())
}

pub fn enqueue(db: &Connection, note: &str, language: &str) -> Result<(), String> {
    let count: i64 = db
        .query_row(
            "SELECT count(*) FROM transcription_jobs WHERE status IN ('queued','running')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count >= 1000 {
        return Err(
            "The queue holds up to 1,000 pending recordings. Finish or remove some jobs first."
                .into(),
        );
    }
    let inserted = db.execute("INSERT INTO transcription_jobs(note_id,language,status) SELECT id,?2,'queued' FROM notes WHERE id=?1 AND status IN ('ready','failed') AND segments='[]' ON CONFLICT(note_id) DO UPDATE SET language=excluded.language,status='queued',error=NULL WHERE transcription_jobs.status IN ('failed','cancelled')", params![note, language]).map_err(|e| e.to_string())?;
    if inserted != 1 {
        return Err("Only an untranscribed recording without an active job can be queued. Saved transcripts are preserved.".into());
    }
    Ok(())
}

/// Called while the application-wide operation guard is held.
pub fn next(db: &Connection) -> Result<Option<(String, String)>, String> {
    next_for(db, None)
}

pub fn next_for(db: &Connection, note: Option<&str>) -> Result<Option<(String, String)>, String> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let running: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM transcription_jobs WHERE status='running')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if running {
        return Err("A queue job is already running. Finish it or restart Golos to recover interrupted work.".into());
    }
    // Manual transcription or future imports can make a pending job obsolete.
    tx.execute("UPDATE transcription_jobs SET status='cancelled',error='Recording already has a transcript; it was preserved.' WHERE status='queued' AND note_id IN (SELECT id FROM notes WHERE status='done' OR segments!='[]')", []).map_err(|e| e.to_string())?;
    let pending: Option<(String,String)> = tx.query_row("SELECT note_id,language FROM transcription_jobs WHERE status='queued' AND (?1 IS NULL OR note_id=?1) ORDER BY id LIMIT 1", [note], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(|e| e.to_string())?;
    if let Some((note, _)) = &pending {
        tx.execute(
            "UPDATE notes SET status='processing',error=NULL WHERE id=?1",
            [note],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "UPDATE transcription_jobs SET status='running',error=NULL WHERE note_id=?1",
            [note],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(pending)
}

pub fn finish(
    db: &Connection,
    note: &str,
    result: &Result<Vec<Segment>, String>,
    cancelled: bool,
) -> Result<(), String> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    let expected: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM transcription_jobs j JOIN notes n ON n.id=j.note_id WHERE j.note_id=?1 AND j.status='running' AND n.status='processing' AND n.segments='[]')",[note],|row|row.get(0)).map_err(|e|e.to_string())?;
    if !expected {
        return Err(
            "The recording changed while its job was running. Existing work was preserved.".into(),
        );
    }
    if let Ok(segments) = result {
        let duration: f64 = tx
            .query_row(
                "SELECT duration_seconds FROM notes WHERE id=?1",
                [note],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        crate::library::validate_segments(segments, duration)?;
    }
    match result {
        Ok(segments) => {
            tx.execute("UPDATE notes SET status='done',segments=?1,error=NULL WHERE id=?2 AND status='processing' AND segments='[]'", params![serde_json::to_string(segments).map_err(|e| e.to_string())?, note]).map_err(|e| e.to_string())?;
            tx.execute("UPDATE transcription_jobs SET status='done',error=NULL WHERE note_id=?1 AND status='running'", [note]).map_err(|e| e.to_string())?;
        }
        Err(error) => {
            tx.execute(
                "UPDATE notes SET status='failed',error=?1 WHERE id=?2 AND status='processing'",
                params![error, note],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("UPDATE transcription_jobs SET status=?1,error=?2 WHERE note_id=?3 AND status='running'", params![if cancelled {"cancelled"} else {"failed"},error,note]).map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

pub fn recover(db: &Connection) -> Result<(), String> {
    db.execute("UPDATE transcription_jobs SET status='failed',error='Batch processing was interrupted. Retry this recording to continue.' WHERE status='running'", []).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn action(db: &Connection, id: i64, action: &str) -> Result<(), String> {
    if action == "retry" {
        let (note,language):(String,String)=db.query_row("SELECT note_id,language FROM transcription_jobs WHERE id=?1 AND status IN ('failed','cancelled')",[id],|row|Ok((row.get(0)?,row.get(1)?))).map_err(|_|"This job cannot be retried.".to_string())?;
        return enqueue(db, &note, &language);
    }
    let changed = match action {
        "remove" => db.execute(
            "DELETE FROM transcription_jobs WHERE id=?1 AND status!='running'",
            [id],
        ),
        _ => return Err("Unsupported queue action.".into()),
    }
    .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("The job changed or cannot be retried. Refresh the queue. Existing transcripts are never replaced.".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        crate::library::init(&db, std::path::Path::new(":memory:")).unwrap();
        for id in ["one", "two", "three"] {
            db.execute("INSERT INTO notes(id,title,created_at,duration_seconds,status) VALUES(?1,?1,1,2,'ready')", [id]).unwrap();
        }
        db
    }

    #[test]
    fn jobs_are_serial_and_failure_does_not_block_later_recordings() {
        let db = database();
        enqueue(&db, "one", "uk").unwrap();
        enqueue(&db, "two", "en").unwrap();
        assert!(enqueue(&db, "one", "uk").is_err());
        assert_eq!(next(&db).unwrap(), Some(("one".into(), "uk".into())));
        finish(&db, "one", &Err("Missing audio".into()), false).unwrap();
        assert_eq!(next(&db).unwrap(), Some(("two".into(), "en".into())));
        let result = Ok(vec![Segment {
            start_ms: 0,
            end_ms: 1000,
            speaker: None,
            text: "Saved text".into(),
        }]);
        finish(&db, "two", &result, false).unwrap();
        assert!(next(&db).unwrap().is_none());
        assert!(enqueue(&db, "two", "en").is_err());
        assert_eq!(
            crate::library::get(&db, "two").unwrap().segments[0].text,
            "Saved text"
        );
        assert_eq!(
            list(&db)
                .unwrap()
                .iter()
                .map(|job| job.status.as_str())
                .collect::<Vec<_>>(),
            vec!["failed", "done"]
        );
    }

    #[test]
    fn cancellation_and_restart_recovery_keep_pending_jobs_for_explicit_resume() {
        let db = database();
        enqueue(&db, "one", "auto").unwrap();
        enqueue(&db, "two", "auto").unwrap();
        next(&db).unwrap();
        finish(&db, "one", &Err("Cancelled".into()), true).unwrap();
        let jobs = list(&db).unwrap();
        assert_eq!(
            jobs.iter().find(|job| job.note_id == "two").unwrap().status,
            "queued"
        );
        let first = jobs.iter().find(|job| job.note_id == "one").unwrap();
        assert_eq!(first.status, "cancelled");
        action(&db, first.id, "retry").unwrap();
        next(&db).unwrap();
        recover(&db).unwrap();
        assert_eq!(
            list(&db)
                .unwrap()
                .iter()
                .find(|job| job.note_id == "one")
                .unwrap()
                .status,
            "failed"
        );
        assert_eq!(
            list(&db)
                .unwrap()
                .iter()
                .find(|job| job.note_id == "two")
                .unwrap()
                .status,
            "queued"
        );
    }

    #[test]
    fn stale_queue_never_overwrites_a_manually_completed_transcript_and_delete_cleans_jobs() {
        let db = database();
        enqueue(&db, "one", "auto").unwrap();
        db.execute("UPDATE notes SET status='done',segments='[{\"startMs\":0,\"endMs\":1,\"speaker\":null,\"text\":\"Edited\"}]' WHERE id='one'",[]).unwrap();
        assert!(next(&db).unwrap().is_none());
        assert_eq!(
            crate::library::get(&db, "one").unwrap().segments[0].text,
            "Edited"
        );
        assert!(action(&db, list(&db).unwrap()[0].id, "retry").is_err());
        db.execute("DELETE FROM notes WHERE id='one'", []).unwrap();
        assert!(list(&db).unwrap().is_empty());
    }
    #[test]
    fn queue_persists_on_disk_and_recovers_only_the_interrupted_item() {
        let root =
            std::env::temp_dir().join(format!("golos-queue-recovery-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("library.sqlite3");
        let db = Connection::open(&path).unwrap();
        crate::library::init(&db, &path).unwrap();
        for id in ["one", "two"] {
            db.execute("INSERT INTO notes(id,title,created_at,duration_seconds,status) VALUES(?1,?1,1,2,'ready')",[id]).unwrap();
            enqueue(&db, id, "uk").unwrap();
        }
        next(&db).unwrap();
        assert!(next(&db).is_err());
        drop(db);
        let db = Connection::open(&path).unwrap();
        crate::library::init(&db, &path).unwrap();
        let jobs = list(&db).unwrap();
        assert_eq!(
            jobs.iter().find(|job| job.note_id == "one").unwrap().status,
            "failed"
        );
        assert_eq!(
            jobs.iter().find(|job| job.note_id == "two").unwrap().status,
            "queued"
        );
        assert_eq!(crate::library::get(&db, "one").unwrap().status, "failed");
        assert_eq!(next(&db).unwrap(), Some(("two".into(), "uk".into())));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn queue_limits_and_conflicting_results_preserve_existing_work() {
        let db = database();
        enqueue(&db, "one", "en").unwrap();
        next(&db).unwrap();
        db.execute("UPDATE notes SET status='done',segments='[{\"startMs\":0,\"endMs\":1,\"speaker\":null,\"text\":\"User revision\"}]' WHERE id='one'",[]).unwrap();
        assert!(finish(&db, "one", &Ok(vec![]), false).is_err());
        assert_eq!(
            crate::library::get(&db, "one").unwrap().segments[0].text,
            "User revision"
        );
        db.execute_batch("WITH RECURSIVE numbers(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM numbers WHERE x<999) INSERT INTO notes(id,title,created_at,duration_seconds,status) SELECT 'bulk-'||x,'Bulk',1,1,'ready' FROM numbers; INSERT INTO transcription_jobs(note_id,language,status) SELECT id,'en','queued' FROM notes WHERE id LIKE 'bulk-%';").unwrap();
        assert!(enqueue(&db, "two", "en").unwrap_err().contains("1,000"));
        assert_eq!(crate::library::get(&db, "two").unwrap().status, "ready");
    }
}
