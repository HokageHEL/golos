mod audio;
mod diarize;
mod download;
mod engine;
mod import;
mod library;
mod migrations;
mod model;
mod onnx_metadata;
mod recording;
mod speaker_models;

use library::{Note, Segment};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
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
    fn audio_path(&self, id: &str) -> Result<PathBuf, String> {
        audio_path(&self.root, id)
    }
    fn db(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.db
            .lock()
            .map_err(|_| "Library is unavailable. Restart Golos.".into())
    }
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
fn list_notes(state: State<AppState>) -> Result<Vec<Note>, String> {
    library::list(&*state.db()?)
}

#[tauri::command]
async fn start_recording(app: tauri::AppHandle, title: String) -> Result<(), String> {
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
        let session = recording::Session::start(&state.root)?;
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
        let id = uuid::Uuid::new_v4().to_string();
        let target = state.audio_path(&id)?;
        let result = (|| {
            let duration_seconds = import::normalize(&source, &target)?;
            let note = Note {
                id,
                title: source
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                created_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs() as i64,
                duration_seconds,
                status: "ready".into(),
                segments: vec![],
                error: None,
            };
            library::insert(&*state.db()?, &note)?;
            Ok(Some(note))
        })();
        if result.is_err() {
            let _ = fs::remove_file(target);
        }
        result
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
                diarize::validate_models(&pending.join("seg.onnx"), &pending.join("emb.onnx"))?;
                download::check_cancel(&state.cancelled)?;
                speaker_models::install(
                    &state.root,
                    &pending.join("seg.onnx"),
                    &pending.join("emb.onnx"),
                )?;
            } else {
                let file = pending.join("model.bin");
                download::fetch(&download::WHISPER, &file, &state.cancelled, |n| {
                    notify(n, "Downloading Whisper Small")
                })?;
                notify(total, "Validating Whisper model");
                model::inspect(&file)?;
                let mut options = WhisperContextParameters::default();
                options.use_gpu(false);
                WhisperContext::new_with_params(
                    file.to_str().ok_or("Unsupported model path")?,
                    options,
                )
                .map_err(|e| format!("Cannot load the recommended model: {e}"))?;
                download::check_cancel(&state.cancelled)?;
                fs::rename(file, state.root.join("model.bin")).map_err(|e| e.to_string())?;
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
async fn transcribe(app: tauri::AppHandle, id: String, language: String) -> Result<(), String> {
    if !["auto", "en", "uk"].contains(&language.as_str()) {
        return Err("Unsupported language.".into());
    }
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
            Ok(segments) => { db.execute("UPDATE notes SET status='done',segments=?1,error=NULL WHERE id=?2",
                params![serde_json::to_string(&segments).map_err(|e| e.to_string())?, id]).map_err(|e| e.to_string())?; Ok(()) }
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
    if !["txt", "srt"].contains(&format.as_str()) || content.len() > 10_000_000 {
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
        fs::write(file.into_path().map_err(|e| e.to_string())?, content)
            .map_err(|e| e.to_string())?;
        Ok(true)
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
            let _ = fs::remove_file(root.join("model.pending"));
            app.manage(AppState {
                root,
                db: Mutex::new(db),
                busy: Arc::new(AtomicBool::new(false)),
                cancelled: Arc::new(AtomicBool::new(false)),
                recording: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_status,
            list_notes,
            import_audio,
            start_recording,
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
}
