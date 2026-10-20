//! Opt-in automatic local AI rules. Rules only apply to transcripts completed
//! after they are saved; every task captures its rule, source snapshot and local
//! model configuration so later settings changes cannot alter queued work.
use crate::{
    ai, ai_commands, automation,
    library::{self, Segment},
    local_ai, AppState,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::atomic::Ordering,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};

/// Saved rules and durable background tasks. AI results stay in `ai_results`;
/// pruning a completed task never deletes a generated result.
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
 UNIQUE(note_id,source_hash,rule_id,model,port,context_window)
);
CREATE INDEX IF NOT EXISTS ai_automation_tasks_pending ON ai_automation_tasks(status,created_at,id);
CREATE INDEX IF NOT EXISTS ai_automation_tasks_note ON ai_automation_tasks(note_id,created_at DESC);
CREATE TRIGGER IF NOT EXISTS ai_automation_note_cleanup AFTER DELETE ON notes BEGIN
 DELETE FROM ai_automation_tasks WHERE note_id=old.id;
END;
";
const KINDS: &[&str] = &["cleanup", "summary", "actions", "custom"];
const SOURCES: &[&str] = &["single", "batch", "watch"];
const TASK_LIMIT: usize = 100;
const SOURCE_LIMIT: usize = 2_000_000;
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
    db.prepare("SELECT t.id,t.note_id,COALESCE(n.title,''),t.kind,t.rule_name,t.status,t.error,t.result_id,t.model,t.source FROM ai_automation_tasks t LEFT JOIN notes n ON n.id=t.note_id ORDER BY t.rowid DESC LIMIT ?1")
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
fn schedule(db: &Connection, note_id: &str, source: &str) -> Result<usize, String> {
    if !SOURCES.contains(&source) {
        return Err("Unsupported automatic transcription source.".into());
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
    let matching: Vec<Rule> = rules(db)?
        .into_iter()
        .filter(|rule| rule.enabled && rule.sources.iter().any(|item| item == source))
        .collect();
    if matching.is_empty() {
        return Ok(0);
    }
    let settings = model_settings(db)?;
    let at = now();
    let mut queued = 0;
    for rule in matching {
        let inserted = db
            .execute(
                "INSERT OR IGNORE INTO ai_automation_tasks(id,note_id,rule_id,rule_name,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'auto',?9,?10,?11,?12,?13,'queued',?14,?14)",
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
                    at
                ],
            )
            .map_err(|e| e.to_string())?;
        queued += inserted;
    }
    prune(db)?;
    Ok(queued)
}
fn prune(db: &Connection) -> Result<(), String> {
    // Keep every pending/running task and the 100 most recent task rows. Finished
    // metadata may be pruned; ai_results and their generated text are untouched.
    db.execute(
        "DELETE FROM ai_automation_tasks WHERE status IN ('done','failed','cancelled','skipped') AND rowid NOT IN (SELECT rowid FROM ai_automation_tasks ORDER BY rowid DESC LIMIT ?1)",
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
                "The transcript changed after this automatic task was queued. It was skipped; run the rule again on the current transcript.",
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
    let changed = db
        .execute(
            "UPDATE ai_automation_tasks SET status=?2,error=?3,result_id=?4,updated_at=?5 WHERE id=?1 AND status='running'",
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
    db.execute(
        "UPDATE ai_automation_tasks SET status='failed',error='Automatic local AI was interrupted. Retry this task; queued tasks remain pending.',updated_at=?1 WHERE status='running'",
        [now()],
    )
    .map_err(|e| e.to_string())?;
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
                return Err("This task captured an older transcript. Retry is refused; run the rule again on the current transcript so the result is clearly based on what you see.".into());
            }
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
