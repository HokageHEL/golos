//! Opt-in automatic local AI rules. Rules only apply to transcripts completed
//! after they are saved; every task captures its rule, source snapshot and local
//! model configuration so later settings changes cannot alter queued work.
use crate::{
    ai, ai_commands, automation,
    library::{self, Segment},
    local_ai, AppState,
};
use rusqlite::{
    params, params_from_iter, types::Value, Connection, OptionalExtension, Transaction,
    TransactionBehavior,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

/// Saved rules and durable background tasks. AI results stay in `ai_results`;
/// pruning a completed task never deletes a generated result, and deleting a
/// result clears the task link that named it so task metadata can never point at
/// work the user removed.
pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS ai_automation_rules(
 id TEXT PRIMARY KEY,
 position INTEGER NOT NULL,
 name TEXT NOT NULL,
 enabled INTEGER NOT NULL,
 kind TEXT NOT NULL,
 target TEXT NOT NULL DEFAULT '',
 prompt TEXT NOT NULL DEFAULT '',
 sources TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS ai_automation_rules_order ON ai_automation_rules(position,id);
CREATE TABLE IF NOT EXISTS ai_automation_tasks(
 id TEXT PRIMARY KEY,
 note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
 rule_id TEXT NOT NULL,
 rule_name TEXT NOT NULL,
 rule_hash TEXT NOT NULL DEFAULT '',
 kind TEXT NOT NULL,
 target TEXT NOT NULL,
 prompt TEXT NOT NULL,
 source TEXT NOT NULL,
 source_language TEXT NOT NULL,
 source_hash TEXT NOT NULL,
 segments TEXT NOT NULL,
 model TEXT NOT NULL,
 port INTEGER NOT NULL,
 context_window INTEGER NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('queued','running','done','failed','cancelled','skipped')),
 error TEXT,
 result_id TEXT,
 created_at INTEGER NOT NULL,
 updated_at INTEGER NOT NULL,
 UNIQUE(note_id,source_hash,rule_id,rule_hash,model,port,context_window)
);
CREATE INDEX IF NOT EXISTS ai_automation_tasks_pending ON ai_automation_tasks(status,created_at,id);
CREATE INDEX IF NOT EXISTS ai_automation_tasks_note ON ai_automation_tasks(note_id,created_at DESC);
CREATE TRIGGER IF NOT EXISTS ai_automation_note_cleanup AFTER DELETE ON notes BEGIN
 DELETE FROM ai_automation_tasks WHERE note_id=old.id;
END;
CREATE TRIGGER IF NOT EXISTS ai_automation_result_cleanup AFTER DELETE ON ai_results BEGIN
 UPDATE ai_automation_tasks SET result_id=NULL WHERE result_id=old.id;
END;
";
const KINDS: &[&str] = &["cleanup", "summary", "actions", "translate", "custom"];
const SOURCES: &[&str] = &["single", "batch", "watch"];
const TASK_LIMIT: usize = 100;
const SOURCE_LIMIT: usize = 2_000_000;
const PENDING_LIMIT: i64 = 1000;
const PENDING_BYTES: i64 = 64_000_000;
/// Unique key of the current automatic-task table, in declaration order. It
/// includes the semantic rule hash, so changing a rule's task, prompt or target
/// schedules new work instead of being taken for an earlier result.
const TASK_UNIQUE: [&str; 7] = [
    "note_id",
    "source_hash",
    "rule_id",
    "rule_hash",
    "model",
    "port",
    "context_window",
];
/// Columns every supported automatic-task table declares, in a fixed order, so
/// the version-7 rebuild reads and rewrites exactly the same set.
const TASK_COLUMNS: &str = "id,note_id,rule_id,rule_name,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,error,result_id,created_at,updated_at";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub kind: String,
    pub target: String,
    pub prompt: String,
    pub sources: Vec<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub note_id: String,
    pub title: String,
    pub kind: String,
    pub rule_name: String,
    pub status: String,
    pub error: Option<String>,
    pub result_id: Option<String>,
    pub model: String,
    pub source: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub rules: Vec<Rule>,
    pub tasks: Vec<Task>,
    pub model: String,
    pub label: Option<String>,
}
/// A task that has been marked running and whose result row now exists.
#[derive(Debug)]
pub struct Run {
    pub task_id: String,
    pub rule_name: String,
    pub work: ai::Work,
    pub port: u16,
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}
/// Validate and normalize a complete rule list. Cleanup always keeps the source
/// language (`target` is cleared), and custom rules require a prompt.
pub fn normalized(mut rules: Vec<Rule>) -> Result<Vec<Rule>, String> {
    if rules.len() > 10 {
        return Err("Use up to 10 automatic rules.".into());
    }
    let mut ids = HashSet::new();
    for rule in &mut rules {
        uuid::Uuid::parse_str(&rule.id)
            .map_err(|_| "Rule IDs must be UUIDs supplied by the app.")?;
        if !ids.insert(rule.id.clone()) {
            return Err("Automatic rule IDs must be unique.".into());
        }
        rule.name = rule.name.trim().into();
        if rule.name.is_empty() || rule.name.len() > 100 || rule.name.contains('\0') {
            return Err("Give each rule a name up to 100 bytes.".into());
        }
        if !KINDS.contains(&rule.kind.as_str()) {
            return Err("Choose a supported automatic rule type.".into());
        }
        if rule.target.len() > 80 || rule.target.contains('\0') {
            return Err("Choose a target up to 80 bytes.".into());
        }
        if rule.prompt.len() > 4000 || rule.prompt.contains('\0') {
            return Err("Use a prompt up to 4,000 bytes.".into());
        }
        if rule.kind == "cleanup" {
            if !rule.target.trim().is_empty() {
                return Err(
                    "Cleanup keeps the transcript's language; leave its target empty.".into(),
                );
            }
            rule.target.clear();
        } else {
            rule.target = rule.target.trim().into();
        }
        if rule.kind == "translate" && rule.target.trim().is_empty() {
            return Err("Automatic translation requires an explicit target language.".into());
        }
        if rule.kind != "custom" {
            rule.prompt.clear();
        }
        if rule.kind == "custom" && rule.prompt.trim().is_empty() {
            return Err("Custom rules require a prompt.".into());
        }
        if rule.sources.is_empty() {
            return Err("Choose at least one transcription source for each rule.".into());
        }
        let mut seen = HashSet::new();
        for source in &rule.sources {
            if !SOURCES.contains(&source.as_str()) || !seen.insert(source.clone()) {
                return Err("Use unique single, batch or watch sources.".into());
            }
        }
    }
    Ok(rules)
}
fn model_settings(db: &Connection) -> Result<ai::Settings, String> {
    let settings = ai::settings(db)?;
    local_ai::Client::new(settings.port)?;
    local_ai::local_tag(&settings.model)?;
    if !(8192..=32768).contains(&settings.context_window) {
        return Err(
            "Choose a local context between 8192 and 32768 tokens before enabling a rule.".into(),
        );
    }
    Ok(settings)
}
pub fn configure(db: &Connection, rules: Vec<Rule>) -> Result<(), String> {
    let rules = normalized(rules)?;
    // Saving settings never contacts Ollama. An enabled rule only requires the
    // already saved local model/context to be syntactically valid.
    if rules.iter().any(|rule| rule.enabled) {
        model_settings(db)?;
    }
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM ai_automation_rules", [])
        .map_err(|e| e.to_string())?;
    for (position, rule) in rules.iter().enumerate() {
        tx.execute(
            "INSERT INTO ai_automation_rules(id,position,name,enabled,kind,target,prompt,sources) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                rule.id,
                position as i64,
                rule.name,
                rule.enabled,
                rule.kind,
                rule.target,
                rule.prompt,
                serde_json::to_string(&rule.sources).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute(
        "UPDATE ai_automation_tasks SET status='skipped',error='The rule was disabled or removed before this task started.',updated_at=?1 WHERE status='queued' AND NOT EXISTS(SELECT 1 FROM ai_automation_rules r WHERE r.id=ai_automation_tasks.rule_id AND r.enabled=1)",
        [now()],
    )
    .map_err(|e| e.to_string())?;
    prune(&tx)?;
    tx.commit().map_err(|e| e.to_string())
}
pub fn rules(db: &Connection) -> Result<Vec<Rule>, String> {
    db.prepare("SELECT id,name,enabled,kind,target,prompt,sources FROM ai_automation_rules ORDER BY position,id")
        .map_err(|e| e.to_string())?
        .query_map([], |row| {
            let sources: String = row.get(6)?;
            Ok(Rule {
                id: row.get(0)?,
                name: row.get(1)?,
                enabled: row.get(2)?,
                kind: row.get(3)?,
                target: row.get(4)?,
                prompt: row.get(5)?,
                sources: serde_json::from_str(&sources).unwrap_or_default(),
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())
}
pub fn tasks(db: &Connection) -> Result<Vec<Task>, String> {
    db.prepare("SELECT t.id,t.note_id,COALESCE(n.title,''),t.kind,t.rule_name,t.status,t.error,t.result_id,t.model,t.source FROM ai_automation_tasks t LEFT JOIN notes n ON n.id=t.note_id ORDER BY CASE t.status WHEN 'running' THEN 0 WHEN 'queued' THEN 1 ELSE 2 END,CASE WHEN t.status IN ('running','queued') THEN t.rowid END,t.updated_at DESC,t.rowid DESC LIMIT ?1")
        .map_err(|e| e.to_string())?
        .query_map([TASK_LIMIT as i64], |row| {
            Ok(Task {
                id: row.get(0)?,
                note_id: row.get(1)?,
                title: row.get(2)?,
                kind: row.get(3)?,
                rule_name: row.get(4)?,
                status: row.get(5)?,
                error: row.get(6)?,
                result_id: row.get(7)?,
                model: row.get(8)?,
                source: row.get(9)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())
}
pub fn status(db: &Connection, label: Option<String>) -> Result<Status, String> {
    Ok(Status {
        rules: rules(db)?,
        tasks: tasks(db)?,
        model: ai::settings(db)?.model,
        label,
    })
}
/// Schedule one note's automatic work. Called inside the caller's transaction
/// through a savepoint so a scheduling problem can never roll back a successful
/// transcript. Returns the number of newly queued tasks.
pub fn schedule_atomic(db: &Connection, note: &str, source: &str) -> Result<usize, String> {
    db.execute_batch("SAVEPOINT golos_automatic_ai_schedule")
        .map_err(|e| e.to_string())?;
    match schedule(db, note, source) {
        Ok(count) => {
            db.execute_batch("RELEASE golos_automatic_ai_schedule")
                .map_err(|e| e.to_string())?;
            Ok(count)
        }
        Err(error) => {
            let _ = db.execute_batch(
                "ROLLBACK TO golos_automatic_ai_schedule; RELEASE golos_automatic_ai_schedule",
            );
            Err(error)
        }
    }
}
/// The semantic identity of a rule: the captured fields that decide what a task
/// generates. A renamed or reordered rule keeps its identity, while changing its
/// task, prompt or target gives it a new one.
fn semantic_rule_hash(kind: &str, target: &str, prompt: &str) -> Result<String, String> {
    let encoded =
        serde_json::to_vec(&serde_json::json!({"v":1,"kind":kind,"target":target,"prompt":prompt}))
            .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
fn schedule(db: &Connection, note_id: &str, source: &str) -> Result<usize, String> {
    if !SOURCES.contains(&source) {
        return Err("Unsupported automatic transcription source.".into());
    }
    let matching: Vec<Rule> = rules(db)?
        .into_iter()
        .filter(|rule| rule.enabled && rule.sources.iter().any(|item| item == source))
        .collect();
    if matching.is_empty() {
        return Ok(0);
    }
    let note: Option<(String, String)> = db
        .query_row(
            "SELECT status,segments FROM notes WHERE id=?1",
            [note_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let Some((status, source_json)) = note else {
        return Err("Recording not found.".into());
    };
    if status != "done" || source_json == "[]" {
        return Ok(0);
    }
    if source_json.len() > SOURCE_LIMIT {
        return Err("This transcript exceeds the local AI input limit (2 MB).".into());
    }
    let segments: Vec<Segment> = serde_json::from_str(&source_json)
        .map_err(|e| format!("Saved transcript is invalid: {e}"))?;
    if segments.is_empty() {
        return Ok(0);
    }
    let source_hash = ai::hash(&segments)?;
    let settings = model_settings(db)?;
    let at = now();
    let mut queued = 0;
    for rule in matching {
        let rule_hash = semantic_rule_hash(&rule.kind, &rule.target, &rule.prompt)?;
        let duplicate:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM ai_automation_tasks WHERE note_id=?1 AND source_hash=?2 AND rule_id=?3 AND rule_hash=?4 AND model=?5 AND port=?6 AND context_window=?7)",params![note_id,source_hash,rule.id,rule_hash,settings.model,settings.port,settings.context_window],|row|row.get(0)).map_err(|e|e.to_string())?;
        if duplicate {
            continue;
        }
        pending_capacity(db, (source_json.len() + rule.prompt.len()) as i64)?;
        let inserted = db
            .execute(
                "INSERT OR IGNORE INTO ai_automation_tasks(id,note_id,rule_id,rule_name,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,created_at,updated_at,rule_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'auto',?9,?10,?11,?12,?13,'queued',?14,?14,?15)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    note_id,
                    rule.id,
                    rule.name,
                    rule.kind,
                    rule.target,
                    rule.prompt,
                    source,
                    source_hash,
                    source_json,
                    settings.model,
                    settings.port,
                    settings.context_window,
                    at,rule_hash
                ],
            )
            .map_err(|e| e.to_string())?;
        queued += inserted;
    }
    prune(db)?;
    Ok(queued)
}
fn pending_capacity(db: &Connection, additional: i64) -> Result<(), String> {
    let(count,bytes):(i64,i64)=db.query_row("SELECT COUNT(*),COALESCE(SUM(length(CAST(segments AS BLOB))+length(CAST(prompt AS BLOB))),0) FROM ai_automation_tasks WHERE status IN ('queued','running')",[],|row|Ok((row.get(0)?,row.get(1)?))).map_err(|e|e.to_string())?;
    if count >= PENDING_LIMIT || bytes + additional > PENDING_BYTES {
        return Err("Automatic AI queue is full (1,000 pending tasks / 64 MB of snapshots). Finish or remove pending work; the transcript was saved. Use Local AI & translation to generate a result manually.".into());
    }
    Ok(())
}
fn prune(db: &Connection) -> Result<(), String> {
    // Keep every pending/running task and the 100 most recently finished tasks. Finished
    // metadata may be pruned; ai_results and their generated text are untouched.
    db.execute(
        "DELETE FROM ai_automation_tasks WHERE status IN ('done','failed','cancelled','skipped') AND rowid NOT IN (SELECT rowid FROM ai_automation_tasks WHERE status IN ('done','failed','cancelled','skipped') ORDER BY updated_at DESC,rowid DESC LIMIT ?1)",
        [TASK_LIMIT as i64],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Debug)]
struct Pending {
    id: String,
    note_id: String,
    rule_id: String,
    rule_name: String,
    kind: String,
    target: String,
    prompt: String,
    source_language: String,
    source_hash: String,
    segments: String,
    model: String,
    port: i64,
    context_window: i64,
    created_at: i64,
}
/// Mark the oldest runnable queued task running and create its separate
/// `ai_results` row. Stale or disabled-rule work is skipped with an explanation.
pub fn prepare_next(db: &Connection) -> Result<Option<Run>, String> {
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    loop {
        let pending: Option<Pending> = tx
            .query_row(
                "SELECT id,note_id,rule_id,rule_name,kind,target,prompt,source_language,source_hash,segments,model,port,context_window,created_at FROM ai_automation_tasks WHERE status='queued' ORDER BY rowid LIMIT 1",
                [],
                |row| {
                    Ok(Pending {
                        id: row.get(0)?,
                        note_id: row.get(1)?,
                        rule_id: row.get(2)?,
                        rule_name: row.get(3)?,
                        kind: row.get(4)?,
                        target: row.get(5)?,
                        prompt: row.get(6)?,
                        source_language: row.get(7)?,
                        source_hash: row.get(8)?,
                        segments: row.get(9)?,
                        model: row.get(10)?,
                        port: row.get(11)?,
                        context_window: row.get(12)?,
                        created_at: row.get(13)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(pending) = pending else {
            tx.commit().map_err(|e| e.to_string())?;
            return Ok(None);
        };
        let enabled: Option<bool> = tx
            .query_row(
                "SELECT enabled FROM ai_automation_rules WHERE id=?1",
                [&pending.rule_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if enabled != Some(true) {
            skip(
                &tx,
                &pending.id,
                "The rule was disabled or removed before this task started.",
            )?;
            continue;
        }
        let current: Option<(String, String)> = tx
            .query_row(
                "SELECT status,segments FROM notes WHERE id=?1",
                [&pending.note_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some((status, current_json)) = current else {
            skip(&tx, &pending.id, "The recording no longer exists.")?;
            continue;
        };
        if status != "done" || current_json == "[]" {
            skip(
                &tx,
                &pending.id,
                "The transcript is no longer available; this automatic task was skipped.",
            )?;
            continue;
        }
        let Ok(current_segments) = serde_json::from_str::<Vec<Segment>>(&current_json) else {
            skip(
                &tx,
                &pending.id,
                "The saved transcript is unreadable; this automatic task was skipped.",
            )?;
            continue;
        };
        if ai::hash(&current_segments)? != pending.source_hash {
            skip(
                &tx,
                &pending.id,
                "The transcript changed after this automatic task was queued, so it was skipped. To use the current saved transcript, open the recording and generate the result manually under Local AI & translation.",
            )?;
            continue;
        }
        if pending.segments.len() > SOURCE_LIMIT {
            skip(
                &tx,
                &pending.id,
                "The captured transcript exceeds the local AI input limit; this task was skipped.",
            )?;
            continue;
        }
        let segments: Vec<Segment> = match serde_json::from_str(&pending.segments) {
            Ok(segments) => segments,
            Err(_) => {
                skip(
                    &tx,
                    &pending.id,
                    "The captured transcript snapshot is unreadable; this task was skipped.",
                )?;
                continue;
            }
        };
        if ai::hash(&segments)? != pending.source_hash {
            skip(&tx,&pending.id,"The captured source checksum is invalid; this task was skipped. Your saved transcript is unchanged.")?;
            continue;
        }
        let (Ok(port), Ok(context_window)) = (
            u16::try_from(pending.port),
            u32::try_from(pending.context_window),
        ) else {
            skip(
                &tx,
                &pending.id,
                "The captured local model settings are invalid; this task was skipped.",
            )?;
            continue;
        };
        let work = ai::Work {
            id: uuid::Uuid::new_v4().to_string(),
            note_id: pending.note_id,
            kind: pending.kind,
            target: pending.target,
            prompt: pending.prompt,
            conversation: String::new(),
            source_language: pending.source_language,
            model: pending.model,
            context_window,
            source: segments,
            source_hash: pending.source_hash,
            status: "running".into(),
            content: String::new(),
            segments: vec![],
            created_at: pending.created_at,
            error: None,
            phase: format!("Automatic {} · Preparing local model", pending.rule_name),
        };
        ai::insert(&tx, &work)?;
        let changed = tx
            .execute(
                "UPDATE ai_automation_tasks SET status='running',error=NULL,result_id=?2,updated_at=?3 WHERE id=?1 AND status='queued'",
                params![pending.id, work.id, now()],
            )
            .map_err(|e| e.to_string())?;
        if changed != 1 {
            return Err("The automatic task changed while it was starting.".into());
        }
        let task_id = pending.id;
        let rule_name = pending.rule_name;
        tx.commit().map_err(|e| e.to_string())?;
        return Ok(Some(Run {
            task_id,
            rule_name,
            work,
            port,
        }));
    }
}
fn skip(db: &Connection, id: &str, error: &str) -> Result<(), String> {
    db.execute(
        "UPDATE ai_automation_tasks SET status='skipped',error=?2,updated_at=?3 WHERE id=?1 AND status='queued'",
        params![id, error, now()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
/// Record a finished generation (including a failed or cancelled result) on its
/// task. The generated `ai_results` row is already checkpointed by the runner.
pub fn finish(db: &Connection, task_id: &str, work: &ai::Work) -> Result<(), String> {
    let (status, error) = match work.status.as_str() {
        "done" => ("done", None),
        "cancelled" => ("cancelled", work.error.clone()),
        "failed" => ("failed", work.error.clone()),
        _ => (
            "failed",
            Some(work.error.clone().unwrap_or_else(|| {
                "Local generation ended without a result. Retry from the saved source.".into()
            })),
        ),
    };
    // The link is only recorded while the result it names still exists: deleting
    // a result clears the link, and a run that finishes afterwards must not put
    // it back.
    let changed = db
        .execute(
            "UPDATE ai_automation_tasks SET status=?2,error=?3,result_id=CASE WHEN EXISTS(SELECT 1 FROM ai_results r WHERE r.id=?4) THEN ?4 ELSE NULL END,updated_at=?5 WHERE id=?1 AND status='running'",
            params![task_id, status, error, work.id, now()],
        )
        .map_err(|e| e.to_string())?;
    if changed != 1 {
        return Err("The automatic task changed before its result could be recorded.".into());
    }
    prune(db)
}
/// Record an execution failure that prevented a normal checkpoint.
pub fn fail(
    db: &Connection,
    task_id: &str,
    rule_name: &str,
    result_id: &str,
    error: &str,
    cancelled: bool,
) -> Result<(), String> {
    let status = if cancelled { "cancelled" } else { "failed" };
    let phase = if cancelled {
        format!("Automatic {rule_name} · Cancelled")
    } else {
        format!("Automatic {rule_name} · Failed")
    };
    let tx = Transaction::new_unchecked(db, TransactionBehavior::Immediate)
        .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE ai_automation_tasks SET status=?2,error=?3,updated_at=?4 WHERE id=?1 AND status='running'",
        params![task_id, status, error, now()],
    )
    .map_err(|e| e.to_string())?;
    tx.execute(
        "UPDATE ai_results SET status=?2,error=?3,phase=?4 WHERE id=?1 AND status='running'",
        params![result_id, status, error, phase],
    )
    .map_err(|e| e.to_string())?;
    prune(&tx)?;
    tx.commit().map_err(|e| e.to_string())
}
/// Restart recovery: interrupted work becomes failed and retryable, while
/// pending queued tasks stay durable for the scheduler.
pub fn recover(db: &Connection) -> Result<(), String> {
    // A crash after the runner's final result checkpoint but before the task was
    // recorded leaves a finished result linked to a running task. That run did
    // complete, so its link finishes the task instead of reporting finished work
    // as interrupted and retryable.
    db.execute(
        "UPDATE ai_automation_tasks SET status='done',error=NULL,updated_at=?1 WHERE status='running' AND result_id IS NOT NULL AND EXISTS(SELECT 1 FROM ai_results r WHERE r.id=ai_automation_tasks.result_id AND r.status='done')",
        [now()],
    )
    .map_err(|e| e.to_string())?;
    // Generic AI recovery runs first. Restore automatic provenance on its
    // interrupted result, including when the last chunk checkpoint was untitled.
    db.execute("UPDATE ai_results SET phase='Automatic '||(SELECT t.rule_name FROM ai_automation_tasks t WHERE t.result_id=ai_results.id AND t.status='running' LIMIT 1)||' · Interrupted' WHERE status='failed' AND phase='Interrupted' AND id IN (SELECT result_id FROM ai_automation_tasks WHERE status='running')",[]).map_err(|e|e.to_string())?;
    db.execute(
        "UPDATE ai_automation_tasks SET status='failed',error='Automatic local AI was interrupted. Retry this task; queued tasks remain pending.',updated_at=?1 WHERE status='running'",
        [now()],
    )
    .map_err(|e| e.to_string())?;
    // A link whose result no longer exists can neither be reviewed nor retried,
    // so it is cleared instead of being shown as saved work.
    reconcile_result_links(db)
}
/// Clear task links whose result is gone. Deleting a result clears the link
/// through the schema trigger; this repairs libraries written before that
/// trigger and links left behind by a result deleted outside the app.
fn reconcile_result_links(db: &Connection) -> Result<(), String> {
    db.execute(
        "UPDATE ai_automation_tasks SET result_id=NULL WHERE result_id IS NOT NULL AND NOT EXISTS(SELECT 1 FROM ai_results r WHERE r.id=ai_automation_tasks.result_id)",
        [],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
/// Bring automatic local AI storage to the version-7 shape.
///
/// Development builds stamped two different task tables as version 6: one
/// without `rule_hash` and with `UNIQUE(note_id,source_hash,rule_id,model,port,context_window)`,
/// and one that added `rule_hash` to that key. The table is inspected rather
/// than assumed, so either shape upgrades. Rows keep their captured snapshot,
/// status, timestamps and result link, and gain the semantic rule hash the
/// scheduler computes, so migrated work is still recognised as already
/// scheduled instead of being generated twice.
pub fn upgrade(db: &Connection) -> Result<(), String> {
    // Every statement is `IF NOT EXISTS`, so this also creates storage a damaged
    // or older library is missing, including the result-link cleanup trigger.
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    if tasks_need_rebuild(db)? {
        rebuild_tasks(db)?;
    }
    reconcile_result_links(db)
}
/// True when the task table is not the current shape: its unique key does not
/// include the semantic rule hash, or a row's hash was never recorded.
fn tasks_need_rebuild(db: &Connection) -> Result<bool, String> {
    let exists: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='ai_automation_tasks')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        // The schema batch above just created the current table.
        return Ok(false);
    }
    if !has_semantic_unique_key(db)? {
        return Ok(true);
    }
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ai_automation_tasks WHERE rule_hash='')",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}
/// True when the table's own unique key is
/// `(note_id,source_hash,rule_id,rule_hash,model,port,context_window)`. The
/// primary key is skipped: only a `UNIQUE` constraint describes the schedule
/// identity.
fn has_semantic_unique_key(db: &Connection) -> Result<bool, String> {
    let mut statement = db
        .prepare("PRAGMA index_list(ai_automation_tasks)")
        .map_err(|e| e.to_string())?;
    let declared: Vec<(String, String)> = statement
        .query_map([], |row| Ok((row.get(1)?, row.get(3)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    for (index, origin) in declared {
        if origin != "u" {
            continue;
        }
        let columns = index_columns(db, &index)?;
        if columns.len() == TASK_UNIQUE.len()
            && columns
                .iter()
                .zip(TASK_UNIQUE.iter())
                .all(|(column, expected)| column.as_deref() == Some(*expected))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
/// Column names of one index, in index order. An expression index reports a null
/// name, which never matches a declared column.
fn index_columns(db: &Connection, index: &str) -> Result<Vec<Option<String>>, String> {
    let mut statement = db
        .prepare(&format!("PRAGMA index_info({index})"))
        .map_err(|e| e.to_string())?;
    let names = statement
        .query_map([], |row| row.get(2))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string());
    names
}
/// Replace the task table with the current definition and copy every row into it
/// with the semantic rule hash its captured rule fields imply.
///
/// The old table is renamed aside while the schema batch creates the replacement.
/// Its indexes and the triggers that name it are dropped first, because renaming
/// a table rewrites whatever still points at the old name.
fn rebuild_tasks(db: &Connection) -> Result<(), String> {
    db.execute_batch(
        "DROP TRIGGER IF EXISTS ai_automation_note_cleanup;
         DROP TRIGGER IF EXISTS ai_automation_result_cleanup;
         DROP INDEX IF EXISTS ai_automation_tasks_pending;
         DROP INDEX IF EXISTS ai_automation_tasks_note;
         ALTER TABLE ai_automation_tasks RENAME TO ai_automation_tasks_previous;",
    )
    .map_err(|e| e.to_string())?;
    db.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
    copy_tasks(db)?;
    db.execute_batch("DROP TABLE ai_automation_tasks_previous;")
        .map_err(|e| e.to_string())
}
/// Copy every task row across the rebuild. The captured rule fields decide the
/// recorded hash, so a row keeps deduplicating against the same rule afterwards.
fn copy_tasks(db: &Connection) -> Result<(), String> {
    let mut statement = db
        .prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM ai_automation_tasks_previous"
        ))
        .map_err(|e| e.to_string())?;
    let columns = statement.column_count();
    let mut rows = statement.query([]).map_err(|e| e.to_string())?;
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let kind: String = row.get(4).map_err(|e| e.to_string())?;
        let target: String = row.get(5).map_err(|e| e.to_string())?;
        let prompt: String = row.get(6).map_err(|e| e.to_string())?;
        let mut values = (0..columns)
            .map(|index| row.get::<_, Value>(index))
            .collect::<Result<Vec<Value>, _>>()
            .map_err(|e| e.to_string())?;
        values.push(Value::Text(semantic_rule_hash(&kind, &target, &prompt)?));
        // A rebuild cannot create duplicates: the previous unique key is a
        // subset of this one, so no two rows can collide here.
        db.execute(
            &format!(
                "INSERT INTO ai_automation_tasks({TASK_COLUMNS},rule_hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)"
            ),
            params_from_iter(values),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn has_queued(db: &Connection) -> Result<bool, String> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM ai_automation_tasks WHERE status='queued')",
        [],
        |row| row.get(0),
    )
    .map_err(|e| e.to_string())
}
pub fn action(db: &Connection, id: &str, action: &str) -> Result<(), String> {
    let (status, note_id, source_hash, rule_id): (String, String, String, String) = db
        .query_row(
            "SELECT status,note_id,source_hash,rule_id FROM ai_automation_tasks WHERE id=?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| "Automatic task not found.".to_string())?;
    match action {
        "retry" => {
            if !["failed", "cancelled", "skipped"].contains(&status.as_str()) {
                return Err(
                    "Only a failed, cancelled or skipped automatic task can be retried.".into(),
                );
            }
            let enabled: Option<bool> = db
                .query_row(
                    "SELECT enabled FROM ai_automation_rules WHERE id=?1",
                    [&rule_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            if enabled != Some(true) {
                return Err("Re-enable this rule before retrying its automatic task.".into());
            }
            let current_hash = ai::hash(&library::get(db, &note_id)?.segments)?;
            if current_hash != source_hash {
                return Err("This task captured an older transcript, so retrying it would not match what you see. Open the recording and generate the result manually under Local AI & translation from the current saved transcript.".into());
            }
            let bytes:i64=db.query_row("SELECT length(CAST(segments AS BLOB))+length(CAST(prompt AS BLOB)) FROM ai_automation_tasks WHERE id=?1",[id],|row|row.get(0)).map_err(|e|e.to_string())?;
            pending_capacity(db, bytes)?;
            db.execute(
                "UPDATE ai_automation_tasks SET status='queued',error=NULL,result_id=NULL,updated_at=?2 WHERE id=?1 AND status IN ('failed','cancelled','skipped')",
                params![id, now()],
            )
            .map_err(|e| e.to_string())?;
        }
        "skip" => {
            if status != "queued" {
                return Err(if status == "running" {
                    "Pause automation to cancel running work before skipping.".into()
                } else {
                    "Only a queued automatic task can be skipped.".into()
                });
            }
            skip(db, id, "Skipped before it started.")?;
        }
        "remove" => {
            if status == "running" {
                return Err("Pause automation to cancel running work before removing it.".into());
            }
            db.execute("DELETE FROM ai_automation_tasks WHERE id=?1", [id])
                .map_err(|e| e.to_string())?;
        }
        _ => return Err("Unsupported automatic task action.".into()),
    }
    Ok(())
}
/// Execute at most one queued task. The caller holds the application operation
/// guard; no DB lock is held across model inspection or generation.
pub async fn process_next(app: &tauri::AppHandle) -> Result<bool, String> {
    let run = {
        let state = app.state::<AppState>();
        let db = state.db()?;
        prepare_next(&db)?
    };
    let Some(run) = run else {
        return Ok(false);
    };
    let Run {
        task_id,
        rule_name,
        work,
        port,
    } = run;
    let result_id = work.id.clone();
    let result = ai_commands::automatic(app, work, port, &rule_name).await;
    let state = app.state::<AppState>();
    let cancelled = match &result {
        Ok(work) => work.status == "cancelled",
        Err(_) => state.cancelled.load(Ordering::SeqCst),
    };
    let db = state.db()?;
    match result {
        Ok(work) => finish(&db, &task_id, &work)?,
        Err(error) => fail(
            &db,
            &task_id,
            &rule_name,
            &result_id,
            &error,
            state.cancelled.load(Ordering::SeqCst),
        )?,
    }
    drop(db);
    if cancelled {
        // A generic cancel (rather than the user's explicit global pause) stops
        // the current task; pause automation as well so queued automatic work
        // cannot silently start on the next scheduler turn.
        let paused = automation::settings(&*state.db()?)?.paused;
        if !paused {
            state.set_automation_paused(true)?;
        }
    }
    let _ = app.emit("automation-changed", ());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Connection {
        let (db, _) = crate::ai::tests::fixture();
        crate::ai::configure(
            &db,
            &ai::Settings {
                port: 11434,
                model: "local:latest".into(),
                context_window: 8192,
            },
        )
        .unwrap();
        db
    }
    fn sample(id: &str, kind: &str, enabled: bool, sources: &[&str]) -> Rule {
        Rule {
            id: uuid::Uuid::new_v4().to_string(),
            name: id.into(),
            enabled,
            kind: kind.into(),
            target: if kind == "cleanup" {
                "English".into()
            } else {
                String::new()
            },
            prompt: if kind == "custom" {
                "List decisions".into()
            } else {
                String::new()
            },
            sources: sources.iter().map(|item| item.to_string()).collect(),
        }
    }
    #[tokio::test]
    #[ignore = "Requires public Whisper/audio fixtures and a cloud-disabled local Ollama"]
    async fn real_whisper_to_automatic_ai_and_backup() {
        let model_path = std::env::var("GOLOS_TEST_MODEL").unwrap();
        let audio_path = std::env::var("GOLOS_TEST_AUDIO").unwrap();
        let port: u16 = std::env::var("GOLOS_TEST_OLLAMA_PORT")
            .unwrap()
            .parse()
            .unwrap();
        let local_model = std::env::var("GOLOS_TEST_OLLAMA_MODEL").unwrap();
        let root = std::env::temp_dir().join(format!(
            "golos-auto-ai-integration-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(root.join("audio")).unwrap();
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, std::path::Path::new("in-memory")).unwrap();
        let samples = crate::audio::read_audio(std::path::Path::new(&audio_path)).unwrap();
        let note = library::Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: "Public JFK fixture".into(),
            created_at: 1,
            duration_seconds: samples.len() as f64 / 16000.,
            status: "ready".into(),
            segments: vec![],
            error: None,
            folder: "Test".into(),
            tags: vec![],
        };
        library::insert(&db, &note).unwrap();
        ai::configure(
            &db,
            &ai::Settings {
                port,
                model: local_model.clone(),
                context_window: 16384,
            },
        )
        .unwrap();
        let mut cleanup = sample("Local cleanup", "cleanup", true, &["batch"]);
        cleanup.target.clear();
        let summary = sample("Local summary", "summary", true, &["batch"]);
        let mut translated = sample("Local translation", "translate", true, &["batch"]);
        translated.target = "Ukrainian".into();
        configure(&db, vec![cleanup, summary, translated]).unwrap();
        crate::jobs::enqueue(&db, &note.id, "en").unwrap();
        crate::jobs::next(&db).unwrap();
        let transcript = crate::engine::transcribe(
            std::path::Path::new(&model_path),
            std::path::Path::new(&audio_path),
            "en",
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            |_| {},
        )
        .unwrap();
        assert!(
            crate::jobs::finish(&db, &note.id, &Ok(transcript.clone()), false)
                .unwrap()
                .is_none()
        );
        assert_eq!(tasks(&db).unwrap().len(), 3);
        let client = local_ai::Client::new(port).unwrap();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let model = client.inspect(&local_model, &cancel).await.unwrap();
        let mut generated = vec![];
        while let Some(run) = prepare_next(&db).unwrap() {
            let mut work = run.work;
            crate::ai_generation::generate(&mut work, &client, &model, "", &cancel, |work| {
                ai::checkpoint(&db, work)
            })
            .await
            .unwrap();
            assert!(!work.content.trim().is_empty());
            if ["cleanup", "translate"].contains(&work.kind.as_str()) {
                assert_eq!(work.segments.len(), transcript.len());
                for (a, b) in work.segments.iter().zip(&transcript) {
                    assert_eq!(
                        (a.start_ms, a.end_ms, &a.speaker),
                        (b.start_ms, b.end_ms, &b.speaker)
                    );
                }
            }
            work.status = "done".into();
            work.phase = format!("Automatic {} · Complete", run.rule_name);
            ai::checkpoint(&db, &work).unwrap();
            finish(&db, &run.task_id, &work).unwrap();
            generated.push(work);
        }
        assert!(tasks(&db).unwrap().iter().all(|task| task.status == "done"));
        assert_eq!(
            ai::hash(&library::get(&db, &note.id).unwrap().segments).unwrap(),
            ai::hash(&transcript).unwrap()
        );
        let saved = library::get(&db, &note.id).unwrap();
        let project = root.join("test.golos");
        crate::project::export(&project, std::path::Path::new(&audio_path), &saved, &db).unwrap();
        let restored = crate::project::import(&project, &root, &db).unwrap();
        assert_eq!(ai::list(&db, &restored.id).unwrap().len(), 3);
        assert!(ai::list(&db, &restored.id)
            .unwrap()
            .iter()
            .all(|result| result.phase.starts_with("Automatic ")));
        assert_eq!(tasks(&db).unwrap().len(), 3);
        println!("Real Whisper → queued automatic cleanup + summary + translation → saved separate results → portable restore passed ({} results); originals unchanged",generated.len());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn semantic_rule_changes_deduplicate_independently_and_snapshots_are_verified() {
        let db = fixture();
        let mut rule = sample("Summary", "summary", true, &["single"]);
        configure(&db, vec![rule.clone()]).unwrap();
        assert_eq!(schedule_atomic(&db, "source", "single").unwrap(), 1);
        assert_eq!(schedule_atomic(&db, "source", "single").unwrap(), 0);
        rule.name = "Renamed".into();
        configure(&db, vec![rule.clone()]).unwrap();
        assert_eq!(schedule_atomic(&db, "source", "single").unwrap(), 0);
        rule.kind = "custom".into();
        rule.prompt = "List decisions".into();
        configure(&db, vec![rule]).unwrap();
        assert_eq!(schedule_atomic(&db, "source", "single").unwrap(), 1);
        db.execute("UPDATE ai_automation_tasks SET segments='[]'", [])
            .unwrap();
        assert!(prepare_next(&db).unwrap().is_none());
        assert!(tasks(&db)
            .unwrap()
            .iter()
            .all(|task| task.status == "skipped"
                && task.error.as_ref().unwrap().contains("checksum")));
        assert!(ai::list(&db, "source").unwrap().is_empty());
    }
    #[test]
    fn pending_queue_budget_failure_rolls_back_scheduling_and_preserves_source() {
        let db = fixture();
        let rule = sample("Summary", "summary", true, &["single"]);
        configure(&db, vec![rule]).unwrap();
        assert!(pending_capacity(&db, PENDING_BYTES + 1).is_err());
        schedule_atomic(&db, "source", "single").unwrap();
        let source_bytes: i64 = db
            .query_row(
                "SELECT length(CAST(segments AS BLOB)) FROM notes WHERE id='source'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        db.execute(
            "UPDATE ai_automation_tasks SET segments=zeroblob(?1)",
            [PENDING_BYTES - source_bytes - 1],
        )
        .unwrap();
        crate::ai::configure(
            &db,
            &ai::Settings {
                model: "another:latest".into(),
                port: 11434,
                context_window: 8192,
            },
        )
        .unwrap();
        let mut existing = rules(&db).unwrap().remove(0);
        existing.sources = vec!["watch".into()];
        configure(
            &db,
            vec![
                existing,
                sample("New summary", "summary", true, &["single"]),
                sample("New actions", "actions", true, &["single"]),
            ],
        )
        .unwrap();
        assert!(schedule_atomic(&db, "source", "single")
            .unwrap_err()
            .contains("queue is full"));
        assert_eq!(tasks(&db).unwrap().len(), 1);
        assert_eq!(
            library::get(&db, "source").unwrap().segments[0].text,
            "We agreed to ship on Friday."
        );
    }
    #[test]
    fn rules_validate_normalize_scope_and_remain_off_by_default() {
        let db = fixture();
        assert!(rules(&db).unwrap().is_empty());
        let mut cleanup = sample(" clean ", "cleanup", true, &["single"]);
        cleanup.target = "   ".into();
        let mut invalid = cleanup.clone();
        invalid.sources = vec!["single".into(), "single".into()];
        assert!(normalized(vec![invalid]).is_err());
        assert!(normalized(vec![sample("x", "cleanup", true, &["single"])]).is_err());
        invalid = sample("x", "cleanup", true, &["single"]);
        invalid.name = "n".repeat(101);
        assert!(normalized(vec![invalid]).is_err());
        invalid = sample("x", "cleanup", true, &["single"]);
        invalid.target = "French".into();
        assert!(normalized(vec![invalid]).is_err());
        invalid = sample("x", "custom", true, &["single"]);
        invalid.prompt = "  ".into();
        assert!(normalized(vec![invalid]).is_err());
        invalid = sample("x", "summary", true, &["single"]);
        invalid.sources.clear();
        assert!(normalized(vec![invalid]).is_err());
        invalid = sample("x", "summary", true, &["single"]);
        invalid.kind = "translate".into();
        assert!(normalized(vec![invalid.clone()]).is_err());
        invalid.kind = "unknown".into();
        assert!(normalized(vec![invalid]).is_err());
        let many: Vec<Rule> = (0..11)
            .map(|_| sample("x", "summary", true, &["single"]))
            .collect();
        assert!(normalized(many).is_err());
        let normalized = normalized(vec![cleanup]).unwrap();
        assert_eq!(normalized[0].name, "clean");
        assert!(normalized[0].target.is_empty());
        assert!(normalized[0].sources == vec!["single"]);
        assert!(configure(&db, vec![sample("off", "summary", false, &["single"])]).is_ok());
        assert_eq!(rules(&db).unwrap().len(), 1);
        assert!(!rules(&db).unwrap()[0].enabled);
        assert!(!has_queued(&db).unwrap());
    }
    #[test]
    fn enabled_rules_require_valid_saved_local_model_without_network_access() {
        let (db, _) = crate::ai::tests::fixture();
        db.execute("UPDATE ai_settings SET model=''", []).unwrap();
        assert!(configure(&db, vec![sample("x", "summary", true, &["single"])]).is_err());
        db.execute("UPDATE ai_settings SET model='gpt:cloud'", [])
            .unwrap();
        assert!(configure(&db, vec![sample("x", "summary", true, &["single"])]).is_err());
        db.execute(
            "UPDATE ai_settings SET model='local:latest',context_window=4096",
            [],
        )
        .unwrap();
        assert!(configure(&db, vec![sample("x", "summary", true, &["single"])]).is_err());
        assert!(configure(&db, vec![sample("x", "summary", false, &["single"])]).is_ok());
    }
    #[test]
    fn scheduling_is_scoped_deduplicated_and_snapshots_source_and_model() {
        let db = fixture();
        let rule = sample("summary", "summary", true, &["single", "batch"]);
        configure(&db, vec![rule.clone()]).unwrap();
        let original = library::get(&db, "source").unwrap();
        assert_eq!(schedule_atomic(&db, "source", "watch").unwrap(), 0);
        assert_eq!(schedule_atomic(&db, "source", "single").unwrap(), 1);
        assert_eq!(schedule_atomic(&db, "source", "single").unwrap(), 0);
        let task = tasks(&db).unwrap().pop().unwrap();
        assert_eq!(task.status, "queued");
        assert_eq!(task.source, "single");
        assert_eq!(task.model, "local:latest");
        crate::ai::configure(
            &db,
            &ai::Settings {
                port: 11435,
                model: "other:latest".into(),
                context_window: 16384,
            },
        )
        .unwrap();
        let snapshot: (String, i64, i64) = db
            .query_row(
                "SELECT model,port,context_window FROM ai_automation_tasks WHERE id=?1",
                [&task.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(snapshot, ("local:latest".into(), 11434, 8192));
        assert_eq!(
            library::get(&db, "source").unwrap().segments[0].text,
            original.segments[0].text
        );
        assert_eq!(task.note_id, "source");
    }
    #[test]
    fn disabling_or_removing_a_rule_skips_queued_work_and_never_starts_it() {
        let db = fixture();
        let rule = sample("summary", "summary", true, &["single"]);
        configure(&db, vec![rule.clone()]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let mut off = rule;
        off.enabled = false;
        configure(&db, vec![off]).unwrap();
        let skipped = tasks(&db).unwrap()[0].clone();
        assert_eq!(skipped.status, "skipped");
        assert!(action(&db, &skipped.id, "retry")
            .unwrap_err()
            .contains("Re-enable"));
        assert!(prepare_next(&db).unwrap().is_none());
        let mut back = sample("summary", "summary", true, &["single"]);
        back.id = sample("new", "summary", true, &["single"]).id;
        configure(&db, vec![back]).unwrap();
        assert!(prepare_next(&db).unwrap().is_none());
    }
    #[test]
    fn stale_source_is_skipped_and_retry_is_refused_with_recovery() {
        let db = fixture();
        let rule = sample("summary", "summary", true, &["single"]);
        configure(&db, vec![rule]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let task = tasks(&db).unwrap()[0].clone();
        db.execute(
            "UPDATE notes SET segments='[{\"startMs\":0,\"endMs\":2000,\"speaker\":null,\"text\":\"Edited\"}]' WHERE id='source'",
            [],
        )
        .unwrap();
        assert!(prepare_next(&db).unwrap().is_none());
        let skipped = tasks(&db).unwrap()[0].clone();
        assert_eq!(skipped.status, "skipped");
        assert!(skipped.error.unwrap().contains("changed"));
        assert!(action(&db, &task.id, "retry")
            .unwrap_err()
            .contains("older transcript"));
    }
    #[test]
    fn running_tasks_recover_retry_with_fresh_results_and_preserve_original_text() {
        let db = fixture();
        let rule = sample("summary", "summary", true, &["single"]);
        configure(&db, vec![rule]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        let first_result = run.work.id.clone();
        assert_eq!(tasks(&db).unwrap()[0].status, "running");
        assert_eq!(
            tasks(&db).unwrap()[0].result_id.as_deref(),
            Some(first_result.as_str())
        );
        assert_eq!(
            ai::get(&db, &first_result).unwrap().phase,
            "Automatic summary · Preparing local model"
        );
        crate::ai::recover(&db).unwrap();
        recover(&db).unwrap();
        assert_eq!(
            ai::get(&db, &first_result).unwrap().phase,
            "Automatic summary · Interrupted"
        );
        let task = tasks(&db).unwrap()[0].clone();
        assert_eq!(task.status, "failed");
        assert!(task.error.unwrap().contains("interrupted"));
        assert_eq!(ai::get(&db, &first_result).unwrap().status, "failed");
        action(&db, &task.id, "retry").unwrap();
        assert_eq!(tasks(&db).unwrap()[0].status, "queued");
        assert!(tasks(&db).unwrap()[0].result_id.is_none());
        let second = prepare_next(&db).unwrap().unwrap();
        assert_ne!(second.work.id, first_result);
        assert!(ai::get(&db, &first_result).is_ok());
        assert_eq!(
            library::get(&db, "source").unwrap().segments[0].text,
            "We agreed to ship on Friday."
        );
    }
    /// Complete a running task's result the way the runner does, without
    /// recording it on the task yet.
    fn checkpoint_done(db: &Connection, run: &Run) -> ai::Work {
        let mut work = run.work.clone();
        work.status = "done".into();
        work.phase = format!("Automatic {} · Complete", run.rule_name);
        work.content = "Decision: Friday".into();
        ai::checkpoint(db, &work).unwrap();
        work
    }

    fn task_with(db: &Connection, id: &str) -> Task {
        tasks(db)
            .unwrap()
            .into_iter()
            .find(|task| task.id == id)
            .unwrap()
    }

    #[test]
    fn a_result_finished_before_the_task_was_recorded_recovers_as_done() {
        let db = fixture();
        let first = sample("Summary", "summary", true, &["single"]);
        let second = sample("Actions", "actions", true, &["single"]);
        configure(&db, vec![first, second]).unwrap();
        assert_eq!(schedule(&db, "source", "single").unwrap(), 2);
        let run = prepare_next(&db).unwrap().unwrap();
        // The crash window: the runner checkpointed the finished result, and the
        // process stopped before the task recorded it.
        let work = checkpoint_done(&db, &run);
        // A second task that really was interrupted: its result never finished.
        let pending = prepare_next(&db).unwrap().unwrap();

        crate::ai::recover(&db).unwrap();
        recover(&db).unwrap();

        let recovered = task_with(&db, &run.task_id);
        assert_eq!(recovered.status, "done");
        assert!(recovered.error.is_none());
        assert_eq!(recovered.result_id.as_deref(), Some(work.id.as_str()));
        assert_eq!(ai::get(&db, &work.id).unwrap().status, "done");
        assert_eq!(
            ai::get(&db, &work.id).unwrap().phase,
            "Automatic Summary · Complete"
        );
        // Genuinely interrupted work keeps its interrupted result and provenance.
        let interrupted = task_with(&db, &pending.task_id);
        assert_eq!(interrupted.status, "failed");
        assert!(interrupted
            .error
            .as_deref()
            .unwrap()
            .contains("interrupted"));
        let interrupted_result = ai::get(&db, &pending.work.id).unwrap();
        assert_eq!(interrupted_result.status, "failed");
        assert_eq!(interrupted_result.phase, "Automatic Actions · Interrupted");
        assert_eq!(
            interrupted.result_id.as_deref(),
            Some(pending.work.id.as_str())
        );
        assert!(!has_queued(&db).unwrap());
    }

    #[test]
    fn deleting_a_result_clears_the_task_link_and_keeps_the_finished_task() {
        let db = fixture();
        configure(&db, vec![sample("Summary", "summary", true, &["single"])]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        let work = checkpoint_done(&db, &run);
        finish(&db, &run.task_id, &work).unwrap();
        assert_eq!(
            task_with(&db, &run.task_id).result_id.as_deref(),
            Some(work.id.as_str())
        );

        // `ai_delete` removes only the result row.
        db.execute("DELETE FROM ai_results WHERE id=?1", [&work.id])
            .unwrap();
        let task = task_with(&db, &run.task_id);
        assert_eq!(task.status, "done");
        assert!(
            task.result_id.is_none(),
            "the deleted result releases the task link"
        );
        assert!(ai::get(&db, &work.id).is_err());

        // Recovery leaves the finished task and the cleared link alone.
        recover(&db).unwrap();
        let task = task_with(&db, &run.task_id);
        assert_eq!(task.status, "done");
        assert!(task.result_id.is_none());
    }

    #[test]
    fn a_result_deleted_while_its_task_runs_is_not_linked_back() {
        let db = fixture();
        configure(&db, vec![sample("Summary", "summary", true, &["single"])]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        db.execute("DELETE FROM ai_results WHERE id=?1", [&run.work.id])
            .unwrap();
        let work = checkpoint_done(&db, &run);
        finish(&db, &run.task_id, &work).unwrap();
        let task = task_with(&db, &run.task_id);
        assert_eq!(task.status, "done");
        assert!(
            task.result_id.is_none(),
            "a deleted result is never linked back in"
        );
    }

    #[test]
    fn recover_clears_a_link_left_by_a_library_without_the_cleanup_trigger() {
        let db = fixture();
        configure(&db, vec![sample("Summary", "summary", true, &["single"])]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        let work = checkpoint_done(&db, &run);
        finish(&db, &run.task_id, &work).unwrap();
        // The development version-6 schema had no cleanup trigger, so a deleted
        // result left the link behind.
        db.execute("DROP TRIGGER ai_automation_result_cleanup", [])
            .unwrap();
        db.execute("DELETE FROM ai_results WHERE id=?1", [&work.id])
            .unwrap();
        assert_eq!(
            task_with(&db, &run.task_id).result_id.as_deref(),
            Some(work.id.as_str())
        );

        recover(&db).unwrap();
        let task = task_with(&db, &run.task_id);
        assert_eq!(task.status, "done");
        assert!(task.result_id.is_none());
    }

    #[test]
    fn failed_and_cancelled_results_are_visible_and_never_retry_automatically() {
        let db = fixture();
        let rule = sample("summary", "summary", true, &["single"]);
        configure(&db, vec![rule]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        let mut work = run.work.clone();
        work.status = "failed".into();
        work.phase = format!("Automatic {} · Failed", run.rule_name);
        work.error = Some("Start Ollama and retry.".into());
        ai::checkpoint(&db, &work).unwrap();
        finish(&db, &run.task_id, &work).unwrap();
        let task = tasks(&db).unwrap()[0].clone();
        assert_eq!(task.status, "failed");
        assert!(task.error.unwrap().contains("Ollama"));
        assert!(!has_queued(&db).unwrap());
        assert_eq!(ai::get(&db, &work.id).unwrap().status, "failed");
        action(&db, &task.id, "retry").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        let mut work = run.work.clone();
        work.status = "cancelled".into();
        work.phase = format!("Automatic {} · Cancelled", run.rule_name);
        work.error = Some("Generation cancelled.".into());
        ai::checkpoint(&db, &work).unwrap();
        finish(&db, &run.task_id, &work).unwrap();
        assert_eq!(tasks(&db).unwrap()[0].status, "cancelled");
        assert!(!has_queued(&db).unwrap());
    }
    #[test]
    fn finish_links_result_provenance_and_pruning_keeps_results() {
        let db = fixture();
        let rule = sample("summary", "summary", true, &["single"]);
        configure(&db, vec![rule]).unwrap();
        schedule(&db, "source", "single").unwrap();
        let run = prepare_next(&db).unwrap().unwrap();
        let mut work = run.work.clone();
        work.status = "done".into();
        work.phase = format!("Automatic {} · Complete", run.rule_name);
        work.content = "Decision: Friday".into();
        ai::checkpoint(&db, &work).unwrap();
        finish(&db, &run.task_id, &work).unwrap();
        let task = tasks(&db).unwrap()[0].clone();
        assert_eq!(task.status, "done");
        assert_eq!(task.result_id.as_deref(), Some(work.id.as_str()));
        assert_eq!(ai::get(&db, &work.id).unwrap().phase, work.phase);
        for index in 0..110 {
            db.execute(
                "INSERT INTO ai_automation_tasks(id,note_id,rule_id,rule_name,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,created_at,updated_at) VALUES(?1,'source','old','Old','summary','','','single','auto',?2,'[]','local',11434,8192,'done',?3,?3)",
                params![format!("old-{index}"), format!("old-hash-{index}"), index],
            )
            .unwrap();
        }
        prune(&db).unwrap();
        assert!(tasks(&db).unwrap().len() <= TASK_LIMIT);
        assert!(ai::get(&db, &work.id).is_ok());
    }
}
