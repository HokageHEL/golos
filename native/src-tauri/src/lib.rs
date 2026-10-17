mod audio;
mod engine;
mod import;
mod library;
mod migrations;
mod model;

use library::{Note, Segment};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use whisper_rs::{WhisperContext, WhisperContextParameters};

struct AppState {
    root: PathBuf,
    db: Mutex<Connection>,
    busy: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
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
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid recording ID.".to_string())?;
        Ok(self.root.join("audio").join(format!("{id}.wav")))
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
}

#[tauri::command]
fn setup_status(state: State<AppState>) -> Setup {
    let model_name = model::inspect(&state.root.join("model.bin")).ok();
    Setup {
        model_ready: model_name.is_some(),
        model_name,
        data_directory: state.root.to_string_lossy().into(),
    }
}
#[tauri::command]
fn list_notes(state: State<AppState>) -> Result<Vec<Note>, String> {
    library::list(&*state.db()?)
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
fn delete_note(state: State<AppState>, id: String) -> Result<(), String> {
    let _guard = state.claim()?;
    let path = state.audio_path(&id)?;
    match fs::remove_file(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
    }
    state
        .db()?
        .execute("DELETE FROM notes WHERE id=?1", [&id])
        .map_err(|e| e.to_string())?;
    Ok(())
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
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_status,
            list_notes,
            import_audio,
            import_model,
            transcribe,
            cancel_transcription,
            save_note,
            delete_note,
            export_transcript
        ])
        .run(tauri::generate_context!())
        .expect("Could not start Golos");
}
