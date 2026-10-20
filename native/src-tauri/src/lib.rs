mod ai;
mod ai_automation;
mod ai_automation_commands;
mod ai_commands;
mod ai_generation;
mod audio;
mod auto_export;
mod automation;
mod automation_commands;
mod automation_runtime;
mod batch;
mod batch_drop;
#[cfg(any(target_os = "macos", test))]
mod capture;
mod diarize;
mod docx;
mod download;
mod engine;
mod import;
mod jobs;
mod languages;
mod library;
mod local_ai;
mod migrations;
mod model;
mod onnx_metadata;
mod project;
mod recording;
mod speaker_models;
mod watch;

use library::{Note, Segment};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use whisper_rs::{WhisperContext, WhisperContextParameters};

/// The library's own normalized audio for one recording. The id has to be a
/// UUID, so the webview can never name a path outside the `audio/` directory.
fn audio_path(root: &Path, id: &str) -> Result<PathBuf, String> {
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid recording ID.".to_string())?;
    Ok(root.join("audio").join(format!("{id}.wav")))
}

/// Decide whether a recording's audio may be read: a note with this id must be
/// stored and its normalized WAV must exist. Returns the file to serve. The
/// frontend cannot influence anything but the id, so this is the whole
/// authorization for audio access.
fn authorize_audio(root: &Path, db: &Connection, id: &str) -> Result<PathBuf, String> {
    let path = audio_path(root, id)?;
    let known: bool = db
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !known {
        return Err("Recording not found.".into());
    }
    if !path.is_file() {
        return Err("Audio is unavailable for this recording.".into());
    }
    let canonical = path.canonicalize().map_err(|e| e.to_string())?;
    let directory = root
        .join("audio")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if canonical.parent() != Some(directory.as_path())
        || fs::symlink_metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("Audio must be a file in the local library.".into());
    }
    Ok(canonical)
}

struct AppState {
    root: PathBuf,
    db: Mutex<Connection>,
    busy: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    recording: Mutex<Option<ActiveRecording>>,
    automation_label: Mutex<Option<String>>,
    automation_paused: AtomicBool,
    batch_drop: batch_drop::DropStore,
}

struct ActiveRecording {
    session: recording::Session,
    _guard: BusyGuard,
    id: String,
    created_at: i64,
    title: String,
}
struct BusyGuard(Arc<AtomicBool>);
impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
impl AppState {
    fn claim(&self) -> Result<BusyGuard, String> {
        self.busy
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "Wait for the current operation to finish.".to_string())?;
        Ok(BusyGuard(self.busy.clone()))
    }
    fn claim_automation(&self, label: &str) -> Result<BusyGuard, String> {
        let guard = self.claim()?;
        let mut current = self.automation_label.lock().map_err(|e| e.to_string())?;
        if self.automation_paused.load(Ordering::SeqCst) {
            return Err("Automation is paused.".into());
        }
        self.cancelled.store(false, Ordering::SeqCst);
        *current = Some(label.into());
        Ok(guard)
    }
    fn set_automation_paused(&self, paused: bool) -> Result<(), String> {
        self.automation_paused.store(paused, Ordering::SeqCst);
        if paused
            && self
                .automation_label
                .lock()
                .map_err(|e| e.to_string())?
                .is_some()
        {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        self.db()?
            .execute(
                "UPDATE automation_settings SET paused=?1 WHERE id=1",
                [paused],
            )
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn audio_path(&self, id: &str) -> Result<PathBuf, String> {
        audio_path(&self.root, id)
    }
    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.db
            .lock()
            .map_err(|_| "Library is unavailable. Restart Golos.".into())
    }
}

/// Native drag/drop callback: reject drops while any operation is running so a
/// queued preview can never race an import or transcription. The webview sends
/// no paths; this is the only place a dropped path enters the app.
fn capture_batch_drop(
    state: &AppState,
    paths: Vec<PathBuf>,
) -> Result<batch_drop::DropPreview, String> {
    if state.busy.load(Ordering::SeqCst) {
        return Err("Finish or cancel the current operation before dropping files.".into());
    }
    state.batch_drop.replace(paths)
}

/// Claim the shared operation guard before taking the one-shot drop grant.
/// If the app is busy, the grant stays intact for a later explicit retry.
fn claim_dropped_batch(
    state: &AppState,
    drop_id: &str,
) -> Result<(BusyGuard, Vec<PathBuf>), String> {
    let guard = state.claim()?;
    let paths = state.batch_drop.take(drop_id)?;
    Ok((guard, paths))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Setup {
    model_ready: bool,
    model_name: Option<String>,
    data_directory: String,
    diarization_ready: bool,
}

#[tauri::command]
fn setup_status(state: State<AppState>) -> Setup {
    let model_name = model::inspect(&state.root.join("model.bin")).ok();
    Setup {
        model_ready: model_name.is_some(),
        model_name,
        data_directory: state.root.to_string_lossy().into(),
        diarization_ready: speaker_models::active(&state.root)
            .ok()
            .flatten()
            .is_some_and(|(seg, emb)| diarize::validate_model_roles(&seg, &emb).is_ok()),
    }
}
#[tauri::command]
async fn list_notes(
    app: tauri::AppHandle,
    search: Option<String>,
    status: Option<String>,
    folder: Option<String>,
    tag: Option<String>,
    offset: Option<i64>,
) -> Result<Vec<Note>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        library::query(
            &db,
            &search.unwrap_or_default(),
            &status.unwrap_or_default(),
            &folder.unwrap_or_default(),
            &tag.unwrap_or_default(),
            100,
            offset.unwrap_or(0),
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn get_note(app: tauri::AppHandle, id: String) -> Result<Note, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        library::get(&db, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn organize_note(
    state: State<AppState>,
    id: String,
    title: String,
    folder: String,
    tags: Vec<String>,
) -> Result<(), String> {
    let _guard = state.claim()?;
    library::organize(&*state.db()?, &id, &title, &folder, &tags)
}

#[tauri::command]
async fn capture_info(
    app: tauri::AppHandle,
    applications: bool,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        {
            crate::capture::macos::info(
                &app.state::<AppState>().root,
                if applications { "list" } else { "capabilities" },
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (app, applications);
            Ok(serde_json::json!({"system":false,"mixed":false}))
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn start_recording(
    app: tauri::AppHandle,
    title: String,
    source: Option<String>,
    application: Option<u32>,
) -> Result<(), String> {
    if title.trim().is_empty() || title.len() > 200 {
        return Err("Choose a recording title of up to 200 bytes.".into());
    }
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut recording = state
            .recording
            .lock()
            .map_err(|_| "Recording is unavailable. Restart Golos.")?;
        let session = recording::Session::start(
            &state.root,
            source.as_deref().unwrap_or("microphone"),
            application.unwrap_or(0),
        )?;
        *recording = Some(ActiveRecording {
            session,
            title,
            _guard: guard,
            id: uuid::Uuid::new_v4().to_string(),
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_secs() as i64,
        });
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn recording_status(state: State<AppState>) -> Result<recording::Status, String> {
    state
        .recording
        .lock()
        .map_err(|_| "Recording is unavailable.")?
        .as_ref()
        .ok_or("No recording is active.")?
        .session
        .status()
}

#[tauri::command]
fn pause_recording(state: State<AppState>, paused: bool) -> Result<(), String> {
    state
        .recording
        .lock()
        .map_err(|_| "Recording is unavailable.")?
        .as_ref()
        .ok_or("No recording is active.")?
        .session
        .set_paused(paused)
}

#[derive(Serialize)]
struct SavedRecording {
    note: Note,
    warning: Option<String>,
}

#[tauri::command]
async fn stop_recording(
    app: tauri::AppHandle,
    discard: bool,
) -> Result<Option<SavedRecording>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut slot = state
            .recording
            .lock()
            .map_err(|_| "Recording is unavailable.")?;
        if discard {
            slot.take();
            return Ok(None);
        }
        let recording = slot.as_mut().ok_or("No recording is active.")?;
        let result = save_recording(&state, recording);
        if result.is_ok() {
            slot.take();
        }
        // On failure the source and busy guard remain available for retry.
        result.map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn save_recording(
    state: &AppState,
    recording: &mut ActiveRecording,
) -> Result<SavedRecording, String> {
    let capture_error = recording.session.finish().err();
    let target = state.audio_path(&recording.id)?;
    let result = (|| {
        let duration_seconds = import::normalize(&recording.session.source, &target)?;
        let note = Note {
            id: recording.id.clone(),
            title: recording.title.clone(),
            created_at: recording.created_at,
            duration_seconds,
            status: "ready".into(),
            segments: vec![],
            error: None,
            folder: String::new(),
            tags: vec![],
        };
        let warning = capture_error.or(recording.session.status()?.error);
        library::insert(&*state.db()?, &note)?;
        Ok(SavedRecording { note, warning })
    })();
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}

// Pickers run on a blocking worker, never on the webview thread. Paths are chosen
// natively: the webview cannot supply arbitrary filesystem paths to these commands.
#[tauri::command]
async fn import_audio(app: tauri::AppHandle) -> Result<Option<Note>, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Audio recordings", import::EXTENSIONS)
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let source = file.into_path().map_err(|e| e.to_string())?;
        let state = app.state::<AppState>();
        let prepared = batch::prepare(&state.root, &source, &AtomicBool::new(false))?;
        let db = state.db()?;
        prepared.commit(&db, None).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct BatchImport {
    notes: Vec<Note>,
    failures: Vec<String>,
    cancelled: bool,
    /// Canonical paths repeated within this one selection; no second note is made.
    duplicates: usize,
}

fn validate_language(language: &str) -> Result<(), String> {
    if language != "auto"
        && !languages::available()
            .iter()
            .any(|item| item.code == language)
    {
        return Err("Choose a supported language.".into());
    }
    Ok(())
}

/// Accept only an existing regular local file with a supported extension.
/// Directories, missing paths and unsupported files are reported individually.
fn validate_batch_source(path: &Path) -> Result<PathBuf, String> {
    let source = fs::canonicalize(path)
        .map_err(|_| "This file is no longer available. Drop or choose it again.".to_string())?;
    let metadata = fs::metadata(&source)
        .map_err(|_| "This file is no longer available. Drop or choose it again.".to_string())?;
    if !metadata.is_file() {
        return Err(
            "Only individual local audio files can be imported; folders are not supported.".into(),
        );
    }
    let extension = source
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default();
    if !import::EXTENSIONS
        .iter()
        .any(|supported| supported.eq_ignore_ascii_case(extension))
    {
        return Err(
            "Unsupported audio format. Try WAV, MP3, M4A/AAC, FLAC, Ogg (Vorbis or Opus) or AIFF."
                .into(),
        );
    }
    Ok(source)
}

/// The one import loop shared by the native picker and dropped selections.
/// Decoding never holds the library mutex; each file is normalized and queued
/// independently, so one failure does not discard other completed imports.
fn import_sources(
    state: &AppState,
    sources: Vec<PathBuf>,
    language: &str,
    mut changed: impl FnMut(),
) -> Result<BatchImport, String> {
    if sources.len() > batch_drop::MAX_FILES {
        return Err(format!(
            "Select up to {} files per import. The queue can hold 1,000 pending recordings.",
            batch_drop::MAX_FILES
        ));
    }
    let mut report = BatchImport::default();
    let mut seen = HashSet::new();
    for path in sources {
        if state.cancelled.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let source = match validate_batch_source(&path) {
            Ok(source) => source,
            Err(error) => {
                report
                    .failures
                    .push(format!("{}: {error}", batch_drop::display_name(&path)));
                continue;
            }
        };
        if !seen.insert(source.clone()) {
            report.duplicates += 1;
            continue;
        }
        let result = batch::prepare(&state.root, &source, &state.cancelled)
            .and_then(|prepared| prepared.commit(&*state.db()?, Some(language)));
        match result {
            Ok(note) => report.notes.push(note),
            Err(_error) if state.cancelled.load(Ordering::SeqCst) => {
                report.cancelled = true;
                break;
            }
            Err(error) => report
                .failures
                .push(format!("{}: {error}", batch_drop::display_name(&source))),
        }
        changed();
    }
    Ok(report)
}

#[tauri::command]
async fn import_batch(app: tauri::AppHandle, language: String) -> Result<BatchImport, String> {
    validate_language(&language)?;
    let state = app.state::<AppState>();
    let guard = state.claim()?;
    state.cancelled.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(files) = app
            .dialog()
            .file()
            .add_filter("Audio recordings (30 minutes each)", import::EXTENSIONS)
            .blocking_pick_files()
        else {
            return Ok(BatchImport::default());
        };
        let sources = files
            .into_iter()
            .map(|file| file.into_path().map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        let state = app.state::<AppState>();
        import_sources(&state, sources, &language, || {
            let _ = app.emit("queue-changed", ());
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Import the exact native paths captured for one dropped selection. The grant
/// is consumed before decoding starts and cannot be replayed, even on failure.
#[tauri::command]
async fn import_dropped_batch(
    app: tauri::AppHandle,
    drop_id: String,
    language: String,
) -> Result<BatchImport, String> {
    validate_language(&language)?;
    let (guard, sources) = {
        let state = app.state::<AppState>();
        claim_dropped_batch(&state, &drop_id)?
    };
    app.state::<AppState>()
        .cancelled
        .store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        import_sources(&state, sources, &language, || {
            let _ = app.emit("queue-changed", ());
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Discard the preview without importing. Only the matching grant is cleared,
/// so a stale UI event cannot invalidate a newer selection.
#[tauri::command]
fn discard_dropped_batch(state: State<AppState>, drop_id: String) -> Result<(), String> {
    state.batch_drop.discard(&drop_id);
    Ok(())
}

#[tauri::command]
async fn list_jobs(app: tauri::AppHandle) -> Result<Vec<jobs::Job>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        jobs::list(&db)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn queue_note(state: State<AppState>, id: String, language: String) -> Result<(), String> {
    let _guard = state.claim()?;
    if language != "auto"
        && !languages::available()
            .iter()
            .any(|item| item.code == language)
    {
        return Err("Choose a supported language.".into());
    }
    jobs::enqueue(&*state.db()?, &id, &language)
}

#[tauri::command]
fn job_action(state: State<AppState>, id: i64, action: String) -> Result<(), String> {
    let _guard = state.claim()?;
    jobs::action(&*state.db()?, id, &action)
}

fn process_queued_job(
    app: &tauri::AppHandle,
    state: &AppState,
    id: &str,
    language: &str,
) -> Result<(), String> {
    let _ = app.emit("queue-changed", ());
    let _ = app.emit(
        "transcription-progress",
        Progress {
            note_id: id.to_string(),
            percent: 0,
        },
    );
    let event_app = app.clone();
    let note_id = id.to_string();
    let result = state.audio_path(id).and_then(|path| {
        engine::transcribe(
            &state.root.join("model.bin"),
            &path,
            language,
            state.cancelled.clone(),
            move |percent| {
                let _ = event_app.emit(
                    "transcription-progress",
                    Progress {
                        note_id: note_id.clone(),
                        percent,
                    },
                );
            },
        )
    });
    let result = result.and_then(|segments| {
        let db = state.db()?;
        let note = library::get(&db, id)?;
        library::validate_segments(&segments, note.duration_seconds)?;
        Ok(segments)
    });
    if let Some(warning) = jobs::finish(
        &*state.db()?,
        id,
        &result,
        state.cancelled.load(Ordering::SeqCst),
    )? {
        let _ = app.emit(
            "automation-error",
            format!("Transcript saved; automatic local AI was not scheduled: {warning}"),
        );
    }
    match &result {
        Ok(_) => automation::schedule_export(&*state.db()?, id)?,
        Err(error) => {
            if state
                .automation_label
                .lock()
                .map_err(|e| e.to_string())?
                .is_some()
            {
                let _ = app.emit(
                    "automation-error",
                    format!("Automatic transcription needs retry in the batch queue: {error}"),
                );
            }
        }
    }
    let _ = app.emit("queue-changed", ());
    Ok(())
}

#[tauri::command]
async fn run_queue(app: tauri::AppHandle) -> Result<(), String> {
    let guard = app.state::<AppState>().claim()?;
    app.state::<AppState>()
        .cancelled
        .store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        model::inspect(&state.root.join("model.bin"))?;
        loop {
            if state.cancelled.load(Ordering::SeqCst) {
                break;
            }
            let next = jobs::next(&*state.db()?)?;
            let Some((id, language)) = next else {
                break;
            };
            process_queued_job(&app, &state, &id, &language)?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn import_model(app: tauri::AppHandle) -> Result<bool, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Whisper GGML model", &["bin"])
            .blocking_pick_file()
        else {
            return Ok(false);
        };
        let source = file.into_path().map_err(|e| e.to_string())?;
        model::inspect(&source)?;
        let state = app.state::<AppState>();
        let temp = state.root.join("model.pending");
        let result = (|| {
            fs::copy(source, &temp).map_err(|e| e.to_string())?;
            model::inspect(&temp)?;
            // Load once before replacing a working model: reject corrupt/incompatible files.
            let mut options = WhisperContextParameters::default();
            options.use_gpu(false);
            WhisperContext::new_with_params(
                temp.to_str().ok_or("Unsupported model path")?,
                options,
            )
            .map_err(|e| format!("Cannot load this model: {e}"))?;
            fs::rename(&temp, state.root.join("model.bin")).map_err(|e| e.to_string())?;
            Ok(true)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    received: u64,
    total: u64,
    phase: String,
}

#[tauri::command]
async fn download_models(app: tauri::AppHandle, speakers: bool) -> Result<(), String> {
    let guard = {
        let state = app.state::<AppState>();
        let guard = state.claim()?;
        state.cancelled.store(false, Ordering::SeqCst);
        guard
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        let required = if speakers { 120_000_000 } else { 200_000_000 };
        if fs2::available_space(&state.root).map_err(|e| e.to_string())? < required {
            return Err(format!(
                "Free at least {} MB of disk space and retry.",
                required / 1_000_000
            ));
        }
        let pending = state
            .root
            .join(format!("download-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&pending).map_err(|e| e.to_string())?;
        let result = (|| {
            let total = if speakers {
                download::SEGMENTATION.bytes + download::EMBEDDING.bytes
            } else {
                download::WHISPER.bytes
            };
            let notify = |received, phase: &str| {
                let _ = app.emit(
                    "model-download",
                    DownloadProgress {
                        received,
                        total,
                        phase: phase.into(),
                    },
                );
            };
            if speakers {
                download::fetch(
                    &download::SEGMENTATION,
                    &pending.join("seg.tar.bz2"),
                    &state.cancelled,
                    |n| notify(n, "Downloading segmentation model"),
                )?;
                download::extract_segmentation(
                    &pending.join("seg.tar.bz2"),
                    &pending.join("seg.onnx"),
                )?;
                download::fetch(
                    &download::EMBEDDING,
                    &pending.join("emb.onnx"),
                    &state.cancelled,
                    |n| {
                        notify(
                            n + download::SEGMENTATION.bytes,
                            "Downloading embedding model",
                        )
                    },
                )?;
                notify(total, "Validating speaker models");
                speaker_models::install_cancellable(
                    &state.root,
                    &pending.join("seg.onnx"),
                    &pending.join("emb.onnx"),
                    &state.cancelled,
                )?;
            } else {
                let file = pending.join("model.bin");
                download::fetch(&download::WHISPER, &file, &state.cancelled, |n| {
                    notify(n, "Downloading Whisper Small")
                })?;
                notify(total, "Validating Whisper model");
                download::publish(
                    &file,
                    &state.root.join("model.bin"),
                    &state.cancelled,
                    |path| {
                        model::inspect(path)?;
                        let mut options = WhisperContextParameters::default();
                        options.use_gpu(false);
                        WhisperContext::new_with_params(
                            path.to_str().ok_or("Unsupported model path")?,
                            options,
                        )
                        .map_err(|e| format!("Cannot load the recommended model: {e}"))?;
                        Ok(())
                    },
                )?;
            }
            Ok(())
        })();
        let _ = fs::remove_dir_all(pending);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Pick both roles explicitly. Cancellation of either dialog retains the active pair.
#[tauri::command]
async fn import_diarization_models(app: tauri::AppHandle) -> Result<bool, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(seg) = app
            .dialog()
            .file()
            .set_title("Choose pyannote segmentation ONNX model")
            .add_filter("Segmentation ONNX", &["onnx"])
            .blocking_pick_file()
        else {
            return Ok(false);
        };
        let Some(emb) = app
            .dialog()
            .file()
            .set_title("Choose speaker embedding ONNX model")
            .add_filter("Speaker embedding ONNX", &["onnx"])
            .blocking_pick_file()
        else {
            return Ok(false);
        };
        speaker_models::install(
            &app.state::<AppState>().root,
            &seg.into_path().map_err(|e| e.to_string())?,
            &emb.into_path().map_err(|e| e.to_string())?,
        )?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Returns an editable draft. No stored text or labels change until Save edits.
#[tauri::command]
async fn detect_speakers(
    app: tauri::AppHandle,
    id: String,
    speaker_count: Option<u32>,
) -> Result<Vec<Segment>, String> {
    let guard = {
        let state = app.state::<AppState>();
        let guard = state.claim()?;
        state.cancelled.store(false, Ordering::SeqCst);
        guard
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        let (path, segments, duration) = {
            let db = state.db()?;
            let path = authorize_audio(&state.root, &db, &id)?;
            let (json, duration): (String, f64) = db
                .query_row(
                    "SELECT segments,duration_seconds FROM notes WHERE id=?1 AND status='done'",
                    [&id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .map_err(|_| "Transcribe this recording before detecting speakers.".to_string())?;
            (
                path,
                serde_json::from_str::<Vec<Segment>>(&json).map_err(|e| e.to_string())?,
                duration,
            )
        };
        let (seg, emb) =
            speaker_models::active(&state.root)?.ok_or("Import speaker models first.")?;
        let turns = diarize::run(&path, &seg, &emb, speaker_count, &state.cancelled)?;
        if turns.is_empty() {
            return Err("No speech regions found by the speaker model. Check the model pair and recording. Your saved transcript is unchanged.".into());
        }
        let result = diarize::assign(&segments, &turns);
        if !result.iter().any(|segment| segment.speaker.is_some()) {
            return Err("Detected speech did not align with this transcript. Try another speaker model or correct labels manually. Your saved transcript is unchanged.".into());
        }
        library::validate_segments(&result, duration)?;
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
    note_id: String,
    percent: i32,
}

#[tauri::command]
fn transcription_languages() -> Vec<languages::Language> {
    languages::available()
}

#[tauri::command]
async fn transcribe(app: tauri::AppHandle, id: String, language: String) -> Result<(), String> {
    languages::validate(
        &language,
        &model::inspect(&app.state::<AppState>().root.join("model.bin"))?,
    )?;
    let guard = {
        let state = app.state::<AppState>();
        let guard = state.claim()?;
        state.cancelled.store(false, Ordering::SeqCst);
        guard
    };
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        let path = state.audio_path(&id)?;
        {
            let db = state.db()?;
            if db.execute("UPDATE notes SET status='processing',error=NULL WHERE id=?1 AND status!='done'", [&id])
                .map_err(|e| e.to_string())? != 1 { return Err("Recording is missing or already transcribed.".into()); }
        }
        let event_app = app.clone();
        let note_id = id.clone();
        let result = engine::transcribe(&state.root.join("model.bin"), &path, &language,
            state.cancelled.clone(), move |percent| {
                let _ = event_app.emit("transcription-progress", Progress { note_id: note_id.clone(), percent });
            });
        let db = state.db()?;
        match result {
            Ok(segments) => {
                let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
                tx.execute("UPDATE notes SET status='done',segments=?1,error=NULL WHERE id=?2",
                    params![serde_json::to_string(&segments).map_err(|e| e.to_string())?, id]).map_err(|e| e.to_string())?;
                let warning = ai_automation::schedule_atomic(&tx, &id, "single").err();
                tx.commit().map_err(|e| e.to_string())?;
                if let Some(error)=warning { let _=app.emit("automation-error",format!("Transcript saved; automatic local AI was not scheduled: {error}")); }
                if let Err(error)=automation::schedule_export(&db,&id) { let _=app.emit("automation-error",error); }
                Ok(()) }
            Err(error) => { db.execute("UPDATE notes SET status='failed',error=?1 WHERE id=?2", params![error, id]).map_err(|e| e.to_string())?; Err(error) }
        }
    }).await.map_err(|e| e.to_string())?
}

#[tauri::command]
fn cancel_transcription(state: State<AppState>) {
    state.cancelled.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn save_note(state: State<AppState>, id: String, segments: Vec<Segment>) -> Result<(), String> {
    let _guard = state.claim()?;
    let db = state.db()?;
    let duration: f64 = db
        .query_row(
            "SELECT duration_seconds FROM notes WHERE id=?1 AND status='done'",
            [&id],
            |r| r.get(0),
        )
        .map_err(|_| "Completed recording not found.".to_string())?;
    library::validate_segments(&segments, duration)?;
    db.execute(
        "UPDATE notes SET segments=?1 WHERE id=?2",
        params![
            serde_json::to_string(&segments).map_err(|e| e.to_string())?,
            id
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_note(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _guard = state.claim()?;
    let path = state.audio_path(&id)?;
    match fs::remove_file(&path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
    }
    // The file is gone; take back the asset-protocol grant for it as well.
    let _ = app.asset_protocol_scope().forbid_file(&path);
    state
        .db()?
        .execute("DELETE FROM notes WHERE id=?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Authorize one recording's normalized WAV for the asset protocol and return
/// its path. The frontend turns the path into an `asset:` URL with
/// `convertFileSrc`; Rust decides which file that may be, and only files the
/// library wrote for a stored note are ever allowed.
#[tauri::command]
fn audio_source(
    app: tauri::AppHandle,
    state: State<AppState>,
    id: String,
) -> Result<String, String> {
    let db = state.db()?;
    let path = authorize_audio(&state.root, &db, &id)?;
    app.asset_protocol_scope()
        .allow_file(&path)
        .map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
async fn export_transcript(
    app: tauri::AppHandle,
    format: String,
    content: String,
) -> Result<bool, String> {
    if !["txt", "srt", "vtt", "md", "html", "csv", "json"].contains(&format.as_str())
        || content.len() > 10_000_000
    {
        return Err("Invalid export.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name(format!("transcript.{format}"))
            .add_filter("Transcript", &[format.as_str()])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let target = file.into_path().map_err(|e| e.to_string())?;
        let pending = target.with_file_name(format!(".golos-export-{}", uuid::Uuid::new_v4()));
        let result = fs::write(&pending, content).and_then(|()| fs::rename(&pending, &target));
        let _ = fs::remove_file(pending);
        result.map_err(|e| e.to_string())?;
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn export_saved(app: tauri::AppHandle, id: String, format: String) -> Result<bool, String> {
    if !["docx", "wav", "golos"].contains(&format.as_str()) {
        return Err("Unsupported export.".into());
    }
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        let (note, audio) = {
            let db = state.db()?;
            (
                library::get(&db, &id)?,
                if format == "docx" {
                    None
                } else {
                    Some(authorize_audio(&state.root, &db, &id)?)
                },
            )
        };
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name(format!("recording.{format}"))
            .add_filter("Export", &[format.as_str()])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let target = file.into_path().map_err(|e| e.to_string())?;
        // Write beside the destination, so failed exports never truncate an existing backup.
        let pending = target.with_file_name(format!(".golos-export-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            match format.as_str() {
                "docx" => docx::export(&pending, &note)?,
                "golos" => project::export(
                    &pending,
                    audio.as_ref().ok_or("Audio is unavailable.")?,
                    &note,
                    &*state.db()?,
                )?,
                _ => {
                    fs::copy(audio.as_ref().ok_or("Audio is unavailable.")?, &pending)
                        .map_err(|e| e.to_string())?;
                }
            }
            fs::rename(&pending, &target).map_err(|e| e.to_string())?;
            Ok(true)
        })();
        let _ = fs::remove_file(pending);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn import_project(app: tauri::AppHandle) -> Result<Option<Note>, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(file) = app
            .dialog()
            .file()
            .add_filter("Golos project", &["golos"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let state = app.state::<AppState>();
        let db = state.db()?;
        project::import(
            &file.into_path().map_err(|e| e.to_string())?,
            &state.root,
            &db,
        )
        .map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // With no log/tracing feature enabled, hooks discard engine debug output
    // (which may contain recognized words) instead of writing it to stderr.
    whisper_rs::install_logging_hooks();
    let builder = tauri::Builder::default();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let root = app.path().app_data_dir()?;
            fs::create_dir_all(root.join("audio"))?;
            let db_path = root.join("library.sqlite3");
            let db = Connection::open(&db_path)?;
            // Migrations validate before mutating, so an unsupported or damaged
            // library stops startup instead of being altered. When an existing
            // library is upgraded, the pre-upgrade snapshot is reported here
            // because the app has no log file.
            let migration = library::init(&db, &db_path).map_err(std::io::Error::other)?;
            if let Some(backup) = &migration.backup {
                eprintln!(
                    "Golos migrated {} from schema version {} to {}; pre-upgrade backup: {}",
                    db_path.display(),
                    migration.from_version,
                    migration.to_version,
                    backup.display()
                );
            }
            db.execute_batch("PRAGMA foreign_keys=ON;")?;
            automation::recover_files(&root).map_err(std::io::Error::other)?;
            let _ = fs::remove_file(root.join("model.pending"));
            let automation_paused = AtomicBool::new(
                automation::settings(&db)
                    .map_err(std::io::Error::other)?
                    .paused,
            );
            app.manage(AppState {
                root,
                db: Mutex::new(db),
                busy: Arc::new(AtomicBool::new(false)),
                cancelled: Arc::new(AtomicBool::new(false)),
                recording: Mutex::new(None),
                automation_label: Mutex::new(None),
                automation_paused,
                batch_drop: batch_drop::DropStore::new(),
            });
            automation_runtime::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    if state.busy.load(Ordering::SeqCst) {
                        api.prevent_close();
                    }
                }
            }
            // Finder/WebKit deliver dropped files here. Capture them natively as
            // a one-shot grant and emit only names; the webview never gets paths.
            tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. })
                if window.label() == "main" =>
            {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    match capture_batch_drop(&state, paths.clone()) {
                        Ok(preview) => {
                            let _ = app.emit("batch-drop", preview);
                        }
                        Err(message) => {
                            let _ = app
                                .emit("batch-drop-rejected", batch_drop::DropRejected { message });
                        }
                    }
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            setup_status,
            transcription_languages,
            ai_commands::ai_status,
            ai_commands::ai_configure,
            ai_commands::ai_models,
            ai_commands::ai_generate,
            ai_commands::ai_retry,
            ai_commands::ai_get,
            ai_commands::ai_delete,
            ai_commands::ai_segments,
            ai_commands::ai_recording,
            ai_commands::ai_docx,
            ai_automation_commands::ai_automation_status,
            ai_automation_commands::ai_automation_configure,
            ai_automation_commands::ai_automation_action,
            ai_automation_commands::ai_automation_skip_queued,
            automation_commands::automation_status,
            automation_commands::add_watch_folder,
            automation_commands::watch_folder_action,
            automation_commands::pause_automation,
            automation_commands::choose_export_folder,
            automation_commands::configure_exports,
            automation_commands::export_queue_results,
            automation_commands::retry_exports,
            list_notes,
            get_note,
            organize_note,
            import_audio,
            import_batch,
            import_dropped_batch,
            discard_dropped_batch,
            list_jobs,
            queue_note,
            job_action,
            run_queue,
            start_recording,
            capture_info,
            recording_status,
            pause_recording,
            stop_recording,
            import_model,
            download_models,
            import_diarization_models,
            detect_speakers,
            transcribe,
            cancel_transcription,
            save_note,
            delete_note,
            export_transcript,
            export_saved,
            import_project,
            audio_source
        ])
        .run(tauri::generate_context!())
        .expect("Could not start Golos");
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn captured_audio_survives_failed_save_then_becomes_playable_library_entry() {
        let root = std::env::temp_dir().join(format!("golos-record-save-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let source = root.join("capture.wav");
        let mut writer = hound::WavWriter::create(
            &source,
            hound::WavSpec {
                channels: 1,
                sample_rate: 48_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..48_000 {
            writer.write_sample(1000i16).unwrap();
        }
        writer.finalize().unwrap();
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new("in-memory")).unwrap();
        let state = AppState {
            root: root.clone(),
            db: Mutex::new(db),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            recording: Mutex::new(None),
            automation_label: Mutex::new(None),
            automation_paused: AtomicBool::new(false),
            batch_drop: batch_drop::DropStore::new(),
        };
        let mut recording = ActiveRecording {
            session: recording::Session::from_saved_fixture(source.clone()),
            _guard: state.claim().unwrap(),
            id: uuid::Uuid::new_v4().to_string(),
            title: "Interview".into(),
            created_at: 123,
        };
        // Missing audio directory simulates a failed filesystem save.
        assert!(save_recording(&state, &mut recording).is_err());
        assert!(source.exists());
        assert!(state.claim().is_err());
        assert!(library::list(&state.db().unwrap()).unwrap().is_empty());
        fs::create_dir(root.join("audio")).unwrap();
        let saved = save_recording(&state, &mut recording).unwrap();
        assert_eq!(saved.note.title, "Interview");
        assert_eq!(saved.note.status, "ready");
        let audio = authorize_audio(&root, &state.db().unwrap(), &saved.note.id).unwrap();
        assert_eq!(audio::read_audio(&audio).unwrap().len(), 16_000);
        assert_eq!(library::list(&state.db().unwrap()).unwrap().len(), 1);
        drop(recording);
        assert!(!source.exists());
        assert!(state.claim().is_ok());
        assert!(audio.exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn insert_ready_note(db: &Connection, id: &str) {
        library::insert(
            db,
            &Note {
                id: id.into(),
                title: "Interview".into(),
                created_at: 0,
                duration_seconds: 3.0,
                status: "ready".into(),
                segments: vec![],
                error: None,
                folder: String::new(),
                tags: vec![],
            },
        )
        .unwrap();
    }

    /// Only a note UUID can name audio; nothing the webview sends can escape
    /// the library's own `audio/` directory.
    #[test]
    fn audio_paths_are_confined_to_note_uuids() {
        let root = Path::new("/library");
        for bad in [
            "",
            "..",
            "../../etc/passwd",
            "/etc/passwd",
            "stored.wav",
            "not-a-uuid",
        ] {
            assert!(audio_path(root, bad).is_err(), "{bad} should be refused");
        }
        let id = uuid::Uuid::new_v4().to_string();
        assert_eq!(
            audio_path(root, &id).unwrap(),
            root.join("audio").join(format!("{id}.wav"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn playback_rejects_symlinks_outside_the_library() {
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new("in-memory")).unwrap();
        let root = std::env::temp_dir().join(format!("golos-symlink-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let outside = root.join("private.txt");
        fs::write(&outside, b"private").unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        insert_ready_note(&db, &id);
        std::os::unix::fs::symlink(&outside, audio_path(&root, &id).unwrap()).unwrap();
        assert!(authorize_audio(&root, &db, &id).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    /// A recording's audio is authorized only while its note is stored and the
    /// normalized file the library wrote for it exists.
    #[test]
    fn audio_is_authorized_only_for_a_stored_note_whose_file_exists() {
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new("in-memory")).unwrap();
        let root = std::env::temp_dir().join(format!("golos-audio-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();

        let known = uuid::Uuid::new_v4().to_string();
        insert_ready_note(&db, &known);
        // Stored note, but its normalized file is missing.
        assert!(authorize_audio(&root, &db, &known).is_err());

        let stored = root.join("audio").join(format!("{known}.wav"));
        fs::write(&stored, b"RIFF").unwrap();
        assert_eq!(
            authorize_audio(&root, &db, &known).unwrap(),
            stored.canonicalize().unwrap()
        );

        // A same-shaped file with no stored note behind it is refused.
        let unknown = uuid::Uuid::new_v4().to_string();
        fs::write(root.join("audio").join(format!("{unknown}.wav")), b"RIFF").unwrap();
        assert!(authorize_audio(&root, &db, &unknown).is_err());

        // Traversal is refused before any file is looked at.
        assert!(authorize_audio(&root, &db, "../stored.wav").is_err());

        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn pausing_automation_cancels_background_work_and_blocks_restarts_without_blocking_manual_work()
    {
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new(":memory:")).unwrap();
        let state = AppState {
            root: PathBuf::new(),
            db: Mutex::new(db),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            recording: Mutex::new(None),
            automation_label: Mutex::new(None),
            automation_paused: AtomicBool::new(false),
            batch_drop: batch_drop::DropStore::new(),
        };
        let guard = state.claim_automation("Importing watched audio").unwrap();
        assert!(state.busy.load(Ordering::SeqCst));
        assert!(state.claim().is_err());
        state.set_automation_paused(true).unwrap();
        assert!(state.cancelled.load(Ordering::SeqCst));
        drop(guard);
        *state.automation_label.lock().unwrap() = None;
        assert!(state.claim_automation("Should not restart").is_err());
        assert!(!state.busy.load(Ordering::SeqCst));
        let manual = state.claim().unwrap();
        drop(manual);
        assert!(automation::settings(&state.db().unwrap()).unwrap().paused);
        state.set_automation_paused(false).unwrap();
        let guard = state.claim_automation("Resumed").unwrap();
        assert!(!state.cancelled.load(Ordering::SeqCst));
        drop(guard);
    }

    fn batch_test_state(root: &Path) -> AppState {
        let db = Connection::open_in_memory().unwrap();
        library::init(&db, Path::new("in-memory")).unwrap();
        AppState {
            root: root.to_path_buf(),
            db: Mutex::new(db),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            recording: Mutex::new(None),
            automation_label: Mutex::new(None),
            automation_paused: AtomicBool::new(false),
            batch_drop: batch_drop::DropStore::new(),
        }
    }

    fn write_batch_wav(path: &Path) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16_000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..16_000 {
            writer.write_sample(250_i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn dropped_batch_import_deduplicates_reports_bad_entries_and_preserves_sources() {
        let root = std::env::temp_dir().join(format!("golos-drop-import-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let real = root.join("Interview.wav");
        write_batch_wav(&real);
        let original = fs::read(&real).unwrap();
        let corrupt = root.join("broken.wav");
        fs::write(&corrupt, b"not audio").unwrap();
        let unsupported = root.join("notes.txt");
        fs::write(&unsupported, b"text").unwrap();
        let folder = root.join("Folder");
        fs::create_dir(&folder).unwrap();

        let state = batch_test_state(&root);
        let report = import_sources(
            &state,
            vec![
                real.clone(),
                real.clone(),
                corrupt.clone(),
                folder.clone(),
                unsupported.clone(),
            ],
            "uk",
            || {},
        )
        .unwrap();
        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].title, "Interview");
        assert_eq!(report.duplicates, 1);
        assert_eq!(report.failures.len(), 3);
        for name in ["broken.wav", "Folder", "notes.txt"] {
            assert!(
                report
                    .failures
                    .iter()
                    .any(|failure| failure.starts_with(name)),
                "missing failure for {name}: {:?}",
                report.failures
            );
        }
        // The webview only ever sees names, never the private absolute path.
        assert!(report
            .failures
            .iter()
            .all(|failure| !failure.contains(&*root.to_string_lossy())));
        let jobs = jobs::list(&state.db().unwrap()).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].language, "uk");
        assert_eq!(jobs[0].status, "queued");
        assert_eq!(fs::read(&real).unwrap(), original);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancelled_dropped_batch_import_keeps_completed_files_and_sources() {
        let root = std::env::temp_dir().join(format!("golos-drop-cancel-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("audio")).unwrap();
        let first = root.join("First.wav");
        let second = root.join("Second.wav");
        write_batch_wav(&first);
        write_batch_wav(&second);
        let first_original = fs::read(&first).unwrap();
        let second_original = fs::read(&second).unwrap();

        let state = batch_test_state(&root);
        let cancelled = state.cancelled.clone();
        let report = import_sources(&state, vec![first.clone(), second.clone()], "en", || {
            cancelled.store(true, Ordering::SeqCst);
        })
        .unwrap();
        assert!(report.cancelled);
        assert_eq!(report.notes.len(), 1);
        assert_eq!(report.notes[0].title, "First");
        assert_eq!(jobs::list(&state.db().unwrap()).unwrap().len(), 1);
        assert_eq!(fs::read(&first).unwrap(), first_original);
        assert_eq!(fs::read(&second).unwrap(), second_original);
        assert_eq!(fs::read_dir(root.join("audio")).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dropped_batch_capture_rejects_busy_and_oversized_selections_without_losing_the_grant() {
        let state = batch_test_state(Path::new("/library"));
        assert!(validate_language("xx").is_err());
        assert!(validate_language("auto").is_ok());

        let preview = capture_batch_drop(&state, vec![PathBuf::from("/dropped/One.wav")]).unwrap();
        assert_eq!(preview.names, ["One.wav"]);
        let held = state.claim().unwrap();
        assert!(
            capture_batch_drop(&state, vec![PathBuf::from("/dropped/Two.wav")])
                .unwrap_err()
                .contains("current operation")
        );
        // A busy import request must not consume the existing selection.
        assert!(claim_dropped_batch(&state, &preview.id)
            .err()
            .unwrap()
            .contains("current operation"));
        drop(held);
        let (guard, sources) = claim_dropped_batch(&state, &preview.id).unwrap();
        assert_eq!(sources, [PathBuf::from("/dropped/One.wav")]);
        drop(guard);
        assert!(state.batch_drop.take(&preview.id).is_err());

        let too_many = (0..=batch_drop::MAX_FILES)
            .map(|index| PathBuf::from(format!("/dropped/{index}.wav")))
            .collect();
        assert!(capture_batch_drop(&state, too_many)
            .unwrap_err()
            .contains("100"));
        assert!(state.batch_drop.take("missing").is_err());
    }
}
