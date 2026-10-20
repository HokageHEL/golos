//! Saved local AI work is separate from the reviewed transcript.
use crate::library::{self, Segment};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
pub const SCHEMA:&str="
CREATE TABLE ai_settings(id INTEGER PRIMARY KEY CHECK(id=1),port INTEGER NOT NULL DEFAULT 11434,model TEXT NOT NULL DEFAULT '',context_window INTEGER NOT NULL DEFAULT 8192);
INSERT INTO ai_settings(id) VALUES(1);
CREATE TABLE ai_results(id TEXT PRIMARY KEY,note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,kind TEXT NOT NULL,target TEXT NOT NULL,prompt TEXT NOT NULL,conversation TEXT NOT NULL,source_language TEXT NOT NULL,model TEXT NOT NULL,context_window INTEGER NOT NULL,source TEXT NOT NULL,source_hash TEXT NOT NULL,status TEXT NOT NULL,content TEXT NOT NULL DEFAULT '',segments TEXT NOT NULL DEFAULT '[]',created_at INTEGER NOT NULL,error TEXT,phase TEXT NOT NULL DEFAULT 'Queued');
CREATE INDEX ai_results_note ON ai_results(note_id,created_at DESC);
CREATE TRIGGER ai_note_cleanup AFTER DELETE ON notes BEGIN DELETE FROM ai_results WHERE note_id=old.id;END;
";
pub const KINDS: &[&str] = &[
    "summary",
    "actions",
    "chat",
    "custom",
    "cleanup",
    "translate",
    "speech_english",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub port: u16,
    pub model: String,
    pub context_window: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Work {
    pub id: String,
    pub note_id: String,
    pub kind: String,
    pub target: String,
    pub prompt: String,
    pub conversation: String,
    pub source_language: String,
    pub model: String,
    pub context_window: u32,
    pub source: Vec<Segment>,
    pub source_hash: String,
    pub status: String,
    pub content: String,
    pub segments: Vec<Segment>,
    pub created_at: i64,
    pub error: Option<String>,
    pub phase: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub model: String,
    pub status: String,
    pub created_at: i64,
    pub phase: String,
    pub error: Option<String>,
    pub stale: bool,
    pub conversation: String,
}
pub fn hash(segments: &[Segment]) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(segments).map_err(|e| e.to_string())?)
    ))
}
pub fn settings(db: &Connection) -> Result<Settings, String> {
    db.query_row(
        "SELECT port,model,context_window FROM ai_settings WHERE id=1",
        [],
        |row| {
            Ok(Settings {
                port: row.get(0)?,
                model: row.get(1)?,
                context_window: row.get(2)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}
pub fn configure(db: &Connection, settings: &Settings) -> Result<(), String> {
    crate::local_ai::Client::new(settings.port)?;
    crate::local_ai::local_tag(&settings.model)?;
    if !(4096..=32768).contains(&settings.context_window) {
        return Err("Choose a context between 4096 and 32768 tokens.".into());
    }
    db.execute(
        "UPDATE ai_settings SET port=?1,model=?2,context_window=?3 WHERE id=1",
        params![settings.port, settings.model, settings.context_window],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub note_id: String,
    pub kind: String,
    pub target: String,
    pub prompt: String,
    pub conversation: String,
    pub source_language: String,
}
pub fn create(
    db: &Connection,
    request: &Request,
    model: &str,
    context: u32,
) -> Result<Work, String> {
    let note_id = request.note_id.as_str();
    let kind = request.kind.as_str();
    let target = request.target.as_str();
    let prompt = request.prompt.as_str();
    let conversation = request.conversation.as_str();
    let source_language = request.source_language.as_str();
    if !KINDS.contains(&kind)
        || target.len() > 80
        || prompt.len() > 4000
        || prompt.contains('\0')
        || source_language.len() > 16
        || ((kind == "translate") && target.trim().is_empty())
        || (["chat", "custom"].contains(&kind) && prompt.trim().is_empty())
    {
        return Err("Choose a supported task, a target language for translation, and a prompt up to 4,000 bytes where required.".into());
    }
    let note = library::get(db, note_id)?;
    if kind != "speech_english" && (note.status != "done" || note.segments.is_empty()) {
        return Err("Save a completed transcript before using local AI.".into());
    }
    let source = serde_json::to_string(&note.segments).map_err(|e| e.to_string())?;
    if source.len() > 2_000_000 {
        return Err("This transcript exceeds the local AI input limit (2 MB). Split or shorten it before generating; saved work is unchanged.".into());
    }
    let conversation = if kind == "chat" {
        if conversation.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            uuid::Uuid::parse_str(conversation).map_err(|_| "Invalid chat session")?;
            conversation.to_string()
        }
    } else {
        String::new()
    };
    let work = Work {
        id: uuid::Uuid::new_v4().to_string(),
        note_id: note_id.into(),
        kind: kind.into(),
        target: target.into(),
        prompt: prompt.into(),
        conversation,
        source_language: source_language.into(),
        model: model.into(),
        context_window: context,
        source_hash: hash(&note.segments)?,
        source: note.segments,
        status: "running".into(),
        content: String::new(),
        segments: vec![],
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs() as i64,
        error: None,
        phase: "Preparing local model".into(),
    };
    insert(db, &work)?;
    Ok(work)
}
pub fn insert(db: &Connection, work: &Work) -> Result<(), String> {
    db.execute("INSERT INTO ai_results(id,note_id,kind,target,prompt,conversation,source_language,model,context_window,source,source_hash,status,content,segments,created_at,error,phase) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",params![work.id,work.note_id,work.kind,work.target,work.prompt,work.conversation,work.source_language,work.model,work.context_window,serde_json::to_string(&work.source).map_err(|e|e.to_string())?,work.source_hash,work.status,work.content,serde_json::to_string(&work.segments).map_err(|e|e.to_string())?,work.created_at,work.error,work.phase]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn get(db: &Connection, id: &str) -> Result<Work, String> {
    db.query_row("SELECT id,note_id,kind,target,prompt,conversation,source_language,model,context_window,source,source_hash,status,content,segments,created_at,error,phase FROM ai_results WHERE id=?1",[id],|row|{
        let source:String=row.get(9)?;let segments:String=row.get(13)?;
        Ok(Work{id:row.get(0)?,note_id:row.get(1)?,kind:row.get(2)?,target:row.get(3)?,prompt:row.get(4)?,conversation:row.get(5)?,source_language:row.get(6)?,model:row.get(7)?,context_window:row.get(8)?,source:serde_json::from_str(&source).map_err(|e|rusqlite::Error::FromSqlConversionFailure(9,rusqlite::types::Type::Text,Box::new(e)))?,source_hash:row.get(10)?,status:row.get(11)?,content:row.get(12)?,segments:serde_json::from_str(&segments).map_err(|e|rusqlite::Error::FromSqlConversionFailure(13,rusqlite::types::Type::Text,Box::new(e)))?,created_at:row.get(14)?,error:row.get(15)?,phase:row.get(16)?})
    }).map_err(|e|e.to_string())
}
pub fn list(db: &Connection, note_id: &str) -> Result<Vec<Summary>, String> {
    let current = hash(&library::get(db, note_id)?.segments)?;
    db.prepare("SELECT id,kind,target,model,status,created_at,phase,error,source_hash,conversation FROM ai_results WHERE note_id=?1 ORDER BY rowid DESC LIMIT 100").map_err(|e|e.to_string())?.query_map([note_id],|row|Ok(Summary{id:row.get(0)?,kind:row.get(1)?,target:row.get(2)?,model:row.get(3)?,status:row.get(4)?,created_at:row.get(5)?,phase:row.get(6)?,error:row.get(7)?,stale:row.get::<_,String>(8)?!=current,conversation:row.get(9)?})).map_err(|e|e.to_string())?.collect::<Result<_,_>>().map_err(|e|e.to_string())
}
pub fn checkpoint(db: &Connection, work: &Work) -> Result<(), String> {
    if work.content.len() > 2_000_000
        || serde_json::to_vec(&work.segments)
            .map_err(|e| e.to_string())?
            .len()
            > 2_000_000
    {
        return Err("Generated result exceeded 2 MB. Previous saved results are unchanged.".into());
    }
    db.execute(
        "UPDATE ai_results SET status=?2,content=?3,segments=?4,error=?5,phase=?6 WHERE id=?1",
        params![
            work.id,
            work.status,
            work.content,
            serde_json::to_string(&work.segments).map_err(|e| e.to_string())?,
            work.error,
            work.phase
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn recover(db: &Connection) -> Result<(), String> {
    db.execute("UPDATE ai_results SET status='failed',error='Local generation was interrupted. Retry to generate a new result; earlier results and the source were kept.',phase='Interrupted' WHERE status='running'",[]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn chat_history(db: &Connection, work: &Work) -> Result<String, String> {
    if work.kind != "chat" {
        return Ok(String::new());
    }
    let mut history = String::new();
    let mut statement=db.prepare("SELECT prompt,content,source_hash FROM ai_results WHERE note_id=?1 AND conversation=?2 AND status='done' ORDER BY rowid").map_err(|e|e.to_string())?;
    for row in statement
        .query_map(params![work.note_id, work.conversation], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| e.to_string())?
    {
        let (question, answer, hash) = row.map_err(|e| e.to_string())?;
        if hash != work.source_hash {
            return Err("The source changed during this conversation. Start a new chat to use the current saved transcript.".into());
        }
        history.push_str(&format!("User: {question}\nAssistant: {answer}\n"));
        if history.len() > 8000 {
            return Err("This chat has reached its conversation budget. Start a new chat; earlier answers are saved and no history was silently dropped.".into());
        }
    }
    Ok(history)
}
pub fn applicable(db: &Connection, id: &str) -> Result<Vec<Segment>, String> {
    let result = get(db, id)?;
    let note = library::get(db, &result.note_id)?;
    if result.status != "done"
        || result.segments.is_empty()
        || note.status != "done"
        || hash(&note.segments)? != result.source_hash
    {
        return Err("Apply requires a completed structured result based on the unchanged saved transcript. Create a new recording or regenerate after saving edits.".into());
    }
    library::validate_segments(&result.segments, note.duration_seconds)?;
    Ok(result.segments)
}

pub fn retry(db: &Connection, id: &str, model: &str, context: u32) -> Result<Work, String> {
    let mut work = get(db, id)?;
    library::get(db, &work.note_id)?;
    work.id = uuid::Uuid::new_v4().to_string();
    work.status = "running".into();
    work.content.clear();
    work.segments.clear();
    work.error = None;
    work.phase = "Retrying from saved source".into();
    work.model = model.into();
    work.context_window = context;
    work.created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    insert(db, &work)?;
    Ok(work)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn fixture() -> (Connection, Request) {
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, std::path::Path::new("in-memory")).unwrap();
        let note = library::Note {
            id: "source".into(),
            title: "Private interview".into(),
            created_at: 1,
            duration_seconds: 2.,
            folder: "Research".into(),
            tags: vec!["work".into()],
            status: "done".into(),
            error: None,
            segments: vec![Segment {
                start_ms: 0,
                end_ms: 2000,
                speaker: Some("Alice".into()),
                text: "We agreed to ship on Friday.".into(),
            }],
        };
        library::insert(&db, &note).unwrap();
        db.execute(
            "UPDATE notes SET segments=?1 WHERE id=?2",
            params![serde_json::to_string(&note.segments).unwrap(), note.id],
        )
        .unwrap();
        (
            db,
            Request {
                note_id: "source".into(),
                kind: "translate".into(),
                target: "Ukrainian".into(),
                prompt: String::new(),
                conversation: String::new(),
                source_language: "auto".into(),
            },
        )
    }
    #[test]
    fn results_retry_recovery_and_apply_preserve_original() {
        let (db, request) = fixture();
        let original = library::get(&db, "source").unwrap();
        let mut work = create(&db, &request, "local:latest", 8192).unwrap();
        work.content = "Ми домовилися".into();
        work.segments = work.source.clone();
        work.segments[0].text = work.content.clone();
        checkpoint(&db, &work).unwrap();
        assert!(applicable(&db, &work.id).is_err());
        recover(&db).unwrap();
        assert_eq!(get(&db, &work.id).unwrap().status, "failed");
        let retried = retry(&db, &work.id, "other:latest", 16384).unwrap();
        assert_ne!(retried.id, work.id);
        assert_eq!(retried.source_hash, work.source_hash);
        assert!(retried.content.is_empty());
        assert_eq!(get(&db, &work.id).unwrap().content, work.content);
        work.status = "done".into();
        checkpoint(&db, &work).unwrap();
        assert_eq!(
            applicable(&db, &work.id).unwrap()[0].speaker.as_deref(),
            Some("Alice")
        );
        assert_eq!(
            library::get(&db, "source").unwrap().segments[0].text,
            original.segments[0].text
        );
        db.execute("UPDATE notes SET segments='[]' WHERE id='source'", [])
            .unwrap();
        assert!(applicable(&db, &work.id).is_err());
        assert!(list(&db, "source").unwrap().iter().all(|item| item.stale));
        db.execute("DELETE FROM notes WHERE id='source'", [])
            .unwrap();
        assert!(get(&db, &work.id).is_err());
    }
    #[test]
    fn chat_retains_history_and_refuses_changed_source_or_oversized_history() {
        let (db, mut request) = fixture();
        request.kind = "chat".into();
        request.prompt = "When?".into();
        let mut first = create(&db, &request, "local", 8192).unwrap();
        first.status = "done".into();
        first.content = "Friday [s1]".into();
        checkpoint(&db, &first).unwrap();
        request.conversation = first.conversation.clone();
        request.prompt = "Who agreed?".into();
        let mut next = create(&db, &request, "local", 8192).unwrap();
        assert!(chat_history(&db, &next).unwrap().contains("Friday [s1]"));
        next.source_hash = "changed".into();
        assert!(chat_history(&db, &next).is_err());
        first.content = "x".repeat(8001);
        checkpoint(&db, &first).unwrap();
        next.source_hash = first.source_hash;
        assert!(chat_history(&db, &next).is_err());
    }
    #[test]
    fn invalid_requests_and_remote_configuration_are_refused() {
        let (db, mut request) = fixture();
        request.target.clear();
        assert!(create(&db, &request, "local", 8192).is_err());
        assert!(configure(
            &db,
            &Settings {
                port: 443,
                model: "local".into(),
                context_window: 8192
            }
        )
        .is_err());
        assert!(configure(
            &db,
            &Settings {
                port: 11434,
                model: "gpt:cloud".into(),
                context_window: 8192
            }
        )
        .is_err());
        assert!(configure(
            &db,
            &Settings {
                port: 11434,
                model: "local".into(),
                context_window: 512
            }
        )
        .is_err());
    }
}
