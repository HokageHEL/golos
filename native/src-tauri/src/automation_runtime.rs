use crate::{
    ai_automation, auto_export, automation, batch, jobs, model, process_queued_job, watch,
    AppState, BusyGuard,
};
use rusqlite::params;
use std::{
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
/// Alternate priorities so a long automatic-AI backlog cannot starve watched
/// imports, and continuous imports cannot starve the automatic queue.
static SCHEDULER_TURN: AtomicU64 = AtomicU64::new(0);
use tauri::{Emitter, Manager};
struct Operation {
    app: tauri::AppHandle,
    guard: Option<BusyGuard>,
}
impl Operation {
    fn begin(app: &tauri::AppHandle, label: &str) -> Result<Self, String> {
        let state = app.state::<AppState>();
        let guard = state.claim_automation(label)?;
        let _ = app.emit("automation-state", Some(label));
        Ok(Self {
            app: app.clone(),
            guard: Some(guard),
        })
    }
}
impl Drop for Operation {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        if let Ok(mut label) = state.automation_label.lock() {
            *label = None;
        }
        drop(self.guard.take());
        let _ = self.app.emit("automation-state", Option::<String>::None);
        let _ = self.app.emit("automation-changed", ());
    }
}
pub fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        if let Err(error) = tick(&app) {
            let _ = app.emit("automation-error", error);
        }
    });
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}
fn tick(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.busy.load(Ordering::SeqCst) || automation::settings(&*state.db()?)?.paused {
        return Ok(());
    }
    let export = auto_export::next(&*state.db()?)?;
    if let Some(id) = export {
        let Ok(_operation) = Operation::begin(app, "Exporting saved transcripts") else {
            return Ok(());
        };
        if let Err(error) = auto_export::process(&state.db, &id, &state.cancelled) {
            let _ = app.emit(
                "automation-error",
                format!("Transcript saved; local export needs retry: {error}"),
            );
        }
        return Ok(());
    }
    let ai_first = SCHEDULER_TURN
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(2);
    if ai_first && process_automatic_ai(app)? {
        return Ok(());
    }
    let eligible:Option<(String,String)>=state.db()?.query_row("SELECT j.note_id,j.language FROM transcription_jobs j JOIN watch_imports w ON w.note_id=j.note_id JOIN watch_folders f ON f.id=w.folder_id WHERE j.status='queued' AND f.enabled=1 AND f.auto_transcribe=1 ORDER BY j.id LIMIT 1",[],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(|e|e.to_string())?;
    if let Some((id, _)) = eligible {
        if model::inspect(&state.root.join("model.bin")).is_err() {
            return Ok(());
        }
        let Ok(_operation) = Operation::begin(app, "Transcribing watched audio") else {
            return Ok(());
        };
        let next = {
            let db = state.db()?;
            let eligible:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM watch_imports w JOIN watch_folders f ON f.id=w.folder_id WHERE w.note_id=?1 AND f.enabled=1 AND f.auto_transcribe=1)",[&id],|row|row.get(0)).map_err(|e|e.to_string())?;
            if eligible {
                jobs::next_for(&db, Some(&id))?
            } else {
                None
            }
        };
        if let Some((id, language)) = next {
            process_queued_job(app, &state, &id, &language)?;
        }
        return Ok(());
    }
    let folders = automation::folders(&*state.db()?)?;
    for folder in &folders {
        if !folder.enabled {
            continue;
        }
        let entries = match watch::entries(folder) {
            Ok(entries) => entries,
            Err(error) => {
                state
                    .db()?
                    .execute(
                        "UPDATE watch_folders SET error=?2 WHERE id=?1",
                        params![folder.id, error],
                    )
                    .map_err(|e| e.to_string())?;
                continue;
            }
        };
        {
            let db = state.db()?;
            if !automation::folders(&db)?
                .iter()
                .any(|current| current.id == folder.id && current.enabled)
            {
                continue;
            }
            watch::observe(&db, folder, &entries, now(), false)?;
        }
        if let Some(entry) = watch::candidate(&*state.db()?, folder, now())? {
            let Ok(_operation) = Operation::begin(app, "Importing watched audio") else {
                return Ok(());
            };
            let result = (|| -> Result<(), String> {
                let snapshot = watch::snapshot(folder, &entry, &state.root, &state.cancelled)?;
                if watch::duplicate(&*state.db()?, &folder.id, &snapshot.digest)? {
                    watch::status(&*state.db()?, &folder.id, &entry.name, "duplicate", None)?;
                    return Ok(());
                }
                let title = Path::new(&entry.name)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Recording");
                let prepared =
                    batch::prepare(&state.root, &snapshot.path, &state.cancelled)?.title(title);
                prepared.commit_with(&*state.db()?, Some(&folder.language), |db, note| {
                    let enabled: bool = db
                        .query_row(
                            "SELECT enabled FROM watch_folders WHERE id=?1",
                            [&folder.id],
                            |row| row.get(0),
                        )
                        .map_err(|_| "Folder observation was removed.".to_string())?;
                    if !enabled || state.cancelled.load(Ordering::SeqCst) {
                        return Err("Folder import cancelled.".into());
                    }
                    db.execute(
                        "INSERT INTO watch_imports(folder_id,digest,note_id) VALUES(?1,?2,?3)",
                        params![folder.id, snapshot.digest, note.id],
                    )
                    .map_err(|e| e.to_string())?;
                    watch::status(db, &folder.id, &entry.name, "imported", None)
                })?;
                Ok(())
            })();
            if let Err(error) = result {
                if !state.cancelled.load(Ordering::SeqCst) {
                    watch::status(
                        &*state.db()?,
                        &folder.id,
                        &entry.name,
                        "failed",
                        Some(&error),
                    )?;
                }
            }
            let _ = app.emit("queue-changed", ());
            return Ok(());
        }
    }
    if !ai_first && process_automatic_ai(app)? {
        return Ok(());
    }

    Ok(())
}
/// Run one queued automatic task. The operation guard serializes it with
/// transcription/imports and lets the existing global pause cancel it.
fn process_automatic_ai(app: &tauri::AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    if !ai_automation::has_queued(&*state.db()?)? {
        return Ok(false);
    }
    let Ok(_operation) = Operation::begin(app, "Applying automatic local AI") else {
        return Ok(false);
    };
    match tauri::async_runtime::block_on(ai_automation::process_next(app)) {
        Ok(processed) => Ok(processed),
        Err(error) => {
            let _ = app.emit(
                "automation-error",
                format!("Transcripts are saved; automatic local AI needs retry: {error}"),
            );
            Ok(true)
        }
    }
}
use rusqlite::OptionalExtension;
