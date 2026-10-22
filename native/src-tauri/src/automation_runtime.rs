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
        // Selection and import share one path with the regression coverage, so
        // the library mutex cannot be held across the import again.
        let (outcome, operation) = import_watched_folder(&state, folder, now(), || {
            Operation::begin(app, "Importing watched audio")
        })?;
        match outcome {
            WatchedOutcome::NoCandidate => {}
            WatchedOutcome::Busy => return Ok(()),
            WatchedOutcome::Imported | WatchedOutcome::Duplicate | WatchedOutcome::Failed(_) => {
                let _ = app.emit("queue-changed", ());
                // The shared operation is released after the queue event, like
                // the previous inline import path.
                drop(operation);
                return Ok(());
            }
        }
    }
    if !ai_first && process_automatic_ai(app)? {
        return Ok(());
    }

    Ok(())
}
/// What one watched-folder import attempt did.
#[derive(Debug, PartialEq)]
enum WatchedOutcome {
    /// No stable candidate was waiting in the folder.
    NoCandidate,
    /// Another native operation already held the shared busy guard.
    Busy,
    /// A new recording, queue entry and watch provenance were committed.
    Imported,
    /// The same content was already imported for this folder.
    Duplicate,
    /// The import failed; the watched file is marked failed.
    Failed(String),
}

/// Select a stable watched candidate and import it as one step.
///
/// The scheduler tick and the regression coverage both call this function, so
/// candidate selection cannot borrow the library mutex across the import that
/// follows it. `begin` claims the shared native operation exactly where the
/// scheduler claimed it before: after a candidate exists and before any
/// snapshot, decode or database work. The claimed guard is returned so the
/// caller keeps holding it until the import's events are emitted.
fn import_watched_folder<O>(
    state: &AppState,
    folder: &automation::WatchFolder,
    at: i64,
    begin: impl FnOnce() -> Result<O, String>,
) -> Result<(WatchedOutcome, Option<O>), String> {
    let Some(entry) = next_watched_candidate(state, folder, at)? else {
        return Ok((WatchedOutcome::NoCandidate, None));
    };
    let Ok(operation) = begin() else {
        return Ok((WatchedOutcome::Busy, None));
    };
    // The import must not start while the library mutex is held. A guard kept
    // across candidate selection deadlocks the first database read, so the
    // regression coverage checks this inside the shared path.
    #[cfg(test)]
    assert!(
        state.db.try_lock().is_ok(),
        "Watched import ran while the library mutex was held"
    );
    let outcome = match import_watched_candidate(state, folder, &entry) {
        Ok(outcome) => outcome,
        Err(error) => {
            if !state.cancelled.load(Ordering::SeqCst) {
                watch::status(
                    &*state.db()?,
                    &folder.id,
                    &entry.name,
                    "failed",
                    Some(&error),
                )?;
            }
            WatchedOutcome::Failed(error)
        }
    };
    Ok((outcome, Some(operation)))
}

/// Return an owned candidate, releasing the database mutex before the caller's
/// import body starts. A mutex guard borrowed in a candidate scrutinee would
/// otherwise survive the whole import and deadlock on its first subsequent
/// database read.
fn next_watched_candidate(
    state: &AppState,
    folder: &automation::WatchFolder,
    at: i64,
) -> Result<Option<watch::Entry>, String> {
    let db = state.db()?;
    watch::candidate(&db, folder, at)
}

/// Snapshot/decode outside the database lock, then publish the recording, queue
/// entry and watch provenance together. Shared by the scheduler and regression
/// coverage, without requiring a native window or permission prompt.
fn import_watched_candidate(
    state: &AppState,
    folder: &automation::WatchFolder,
    entry: &watch::Entry,
) -> Result<WatchedOutcome, String> {
    let snapshot = watch::snapshot(folder, entry, &state.root, &state.cancelled)?;
    if watch::duplicate(&*state.db()?, &folder.id, &snapshot.digest)? {
        watch::status(&*state.db()?, &folder.id, &entry.name, "duplicate", None)?;
        return Ok(WatchedOutcome::Duplicate);
    }
    let title = Path::new(&entry.name)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Recording");
    let prepared = batch::prepare(&state.root, &snapshot.path, &state.cancelled)?.title(title);
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
    Ok(WatchedOutcome::Imported)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ai, ai_automation::Rule, library, library::Segment};
    use std::{
        fs,
        sync::{atomic::AtomicBool, Arc, Mutex},
    };

    #[test]
    fn watched_candidate_releases_lock_before_import_and_keeps_watch_rule_scope() {
        let root =
            std::env::temp_dir().join(format!("golos-watch-runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let watched = root.join("isolated-input");
        fs::create_dir(&watched).unwrap();
        let watched = watched.canonicalize().unwrap();
        let source = watched.join("Fixture.wav");
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
            wav.write_sample(100_i16).unwrap();
        }
        wav.finalize().unwrap();
        let original = fs::read(&source).unwrap();
        let db = rusqlite::Connection::open_in_memory().unwrap();
        library::init(&db, Path::new(":memory:")).unwrap();
        automation::add(&db, &watched, "en", true).unwrap();
        let folder = automation::folders(&db).unwrap().remove(0);
        ai::configure(
            &db,
            &ai::Settings {
                model: "fixture:local".into(),
                port: 11439,
                context_window: 8192,
            },
        )
        .unwrap();
        // A watch-scoped rule and a batch-scoped rule: only the watch rule may
        // be queued for a watched import.
        ai_automation::configure(
            &db,
            vec![
                Rule {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "Watch summary".into(),
                    enabled: true,
                    kind: "summary".into(),
                    target: String::new(),
                    prompt: String::new(),
                    sources: vec!["watch".into()],
                },
                Rule {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "Batch summary".into(),
                    enabled: true,
                    kind: "summary".into(),
                    target: String::new(),
                    prompt: String::new(),
                    sources: vec!["batch".into()],
                },
            ],
        )
        .unwrap();
        let state = AppState {
            root: root.clone(),
            db: Mutex::new(db),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            recording: Mutex::new(None),
            automation_label: Mutex::new(None),
            automation_paused: AtomicBool::new(false),
            batch_drop: crate::batch_drop::DropStore::new(),
        };
        watch::observe(
            &state.db().unwrap(),
            &folder,
            &watch::entries(&folder).unwrap(),
            0,
            false,
        )
        .unwrap();
        // The scheduler and this test share one select/import path. The import
        // must start with the library mutex released: the original inline
        // candidate scrutinee kept its guard and deadlocked on the first read.
        let (outcome, operation) = import_watched_folder(&state, &folder, watch::STABLE_MS, || {
            state.claim_automation("Importing watched audio")
        })
        .unwrap();
        assert_eq!(outcome, WatchedOutcome::Imported);
        drop(operation);
        {
            let db = state.db().unwrap();
            let status: String = db
                .query_row(
                    "SELECT status FROM watch_files WHERE folder_id=?1 AND name=?2",
                    params![folder.id, "Fixture.wav"],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "imported");
        }
        // The same content observed again deduplicates instead of publishing a
        // second recording.
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&source, &original).unwrap();
        watch::observe(
            &state.db().unwrap(),
            &folder,
            &watch::entries(&folder).unwrap(),
            0,
            false,
        )
        .unwrap();
        let (outcome, operation) = import_watched_folder(&state, &folder, watch::STABLE_MS, || {
            state.claim_automation("Importing watched audio")
        })
        .unwrap();
        assert_eq!(outcome, WatchedOutcome::Duplicate);
        drop(operation);
        {
            let db = state.db().unwrap();
            let status: String = db
                .query_row(
                    "SELECT status FROM watch_files WHERE folder_id=?1 AND name=?2",
                    params![folder.id, "Fixture.wav"],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "duplicate");
            assert_eq!(library::list(&db).unwrap().len(), 1);
        }
        // The staged snapshot is always cleaned up, including the duplicate.
        assert_eq!(fs::read_dir(root.join("incoming")).unwrap().count(), 0);
        let db = state.db().unwrap();
        let notes = library::list(&db).unwrap();
        assert_eq!(notes.len(), 1);
        let note = &notes[0];
        assert_eq!(note.title, "Fixture");
        assert_eq!(
            jobs::next_for(&db, Some(&note.id)).unwrap(),
            Some((note.id.clone(), "en".into()))
        );
        // Synthetic ASR output drives the real transcript-commit/rule boundary;
        // this test makes no microphone or real-inference claim.
        assert!(jobs::finish(
            &db,
            &note.id,
            &Ok(vec![Segment {
                start_ms: 0,
                end_ms: 1000,
                speaker: None,
                text: "Isolated fixture text".into()
            }]),
            false
        )
        .unwrap()
        .is_none());
        let tasks = ai_automation::tasks(&db).unwrap();
        // Only the watch-scoped rule is queued for a watched import.
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].source, "watch");
        assert_eq!(tasks[0].rule_name, "Watch summary");
        assert_eq!(tasks[0].status, "queued");
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 1);
        drop(db);
        drop(state);
        fs::remove_dir_all(root).unwrap();
    }
}
