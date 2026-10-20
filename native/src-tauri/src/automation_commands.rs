use crate::{auto_export, automation, watch, AppState};
use rusqlite::params;
use serde::Serialize;
use std::{path::Path, sync::atomic::Ordering};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct View {
    settings: automation::Settings,
    folders: Vec<automation::WatchFolder>,
    files: Vec<automation::WatchFile>,
    exports: Vec<auto_export::ExportTask>,
    label: Option<String>,
}
#[tauri::command]
pub(crate) async fn automation_status(app: tauri::AppHandle) -> Result<View, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        let label = state
            .automation_label
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        Ok(View {
            settings: automation::settings(&db)?,
            folders: automation::folders(&db)?,
            files: automation::files(&db)?,
            exports: auto_export::list(&db)?,
            label,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) async fn add_watch_folder(
    app: tauri::AppHandle,
    language: String,
    auto_transcribe: bool,
    include_existing: bool,
) -> Result<bool, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(file) = app.dialog().file().blocking_pick_folder() else {
            return Ok(false);
        };
        let state = app.state::<AppState>();
        let path =
            automation::directory(&file.into_path().map_err(|e| e.to_string())?, &state.root)?;
        let mut folder = automation::WatchFolder {
            id: String::new(),
            path: path.to_str().ok_or("Unsupported folder path")?.into(),
            enabled: true,
            auto_transcribe,
            language: language.clone(),
            error: None,
        };
        let entries = watch::entries(&folder)?;
        let db = state.db()?;
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        folder.id = automation::add(&tx, &path, &language, auto_transcribe)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis() as i64;
        watch::observe_rows(&tx, &folder, &entries, now, !include_existing)?;
        tx.commit().map_err(|e| e.to_string())?;
        let _ = app.emit("automation-changed", ());
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn watch_folder_action(
    app: tauri::AppHandle,
    id: String,
    action: String,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    if ["pause", "remove", "manual"].contains(&action.as_str())
        && state
            .automation_label
            .lock()
            .map_err(|e| e.to_string())?
            .is_some()
    {
        state.cancelled.store(true, Ordering::SeqCst);
    }
    automation::action(&*state.db()?, &id, &action)?;
    let _ = app.emit("automation-changed", ());
    Ok(())
}
#[tauri::command]
pub(crate) fn pause_automation(app: tauri::AppHandle, paused: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.set_automation_paused(paused)?;
    let _ = app.emit("automation-changed", ());
    Ok(())
}
#[tauri::command]
pub(crate) async fn choose_export_folder(app: tauri::AppHandle) -> Result<bool, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let Some(file) = app.dialog().file().blocking_pick_folder() else {
            return Ok(false);
        };
        let state = app.state::<AppState>();
        let path =
            automation::directory(&file.into_path().map_err(|e| e.to_string())?, &state.root)?;
        state
            .db()?
            .execute(
                "UPDATE automation_settings SET export_directory=?1 WHERE id=1",
                [path.to_str().ok_or("Unsupported folder path")?],
            )
            .map_err(|e| e.to_string())?;
        let _ = app.emit("automation-changed", ());
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn configure_exports(
    state: State<AppState>,
    enabled: bool,
    formats: Vec<String>,
) -> Result<(), String> {
    let _guard = state.claim()?;
    auto_export::formats(&formats)?;
    let db = state.db()?;
    if enabled {
        let settings = automation::settings(&db)?;
        let path = settings
            .export_directory
            .as_deref()
            .ok_or("Choose an export folder first.")?;
        automation::directory(Path::new(path), &state.root)?;
    }
    db.execute(
        "UPDATE automation_settings SET export_enabled=?1,export_formats=?2 WHERE id=1",
        params![
            enabled,
            serde_json::to_string(&formats).map_err(|e| e.to_string())?
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub(crate) fn export_queue_results(state: State<AppState>) -> Result<u32, String> {
    let _guard = state.claim()?;
    let db = state.db()?;
    let settings = automation::settings(&db)?;
    let path = Path::new(
        settings
            .export_directory
            .as_deref()
            .ok_or("Choose an export folder first.")?,
    );
    let ids:Vec<String>=db.prepare("SELECT n.id FROM notes n JOIN transcription_jobs j ON j.note_id=n.id WHERE n.status='done' AND j.status='done'").map_err(|e|e.to_string())?.query_map([],|row|row.get(0)).map_err(|e|e.to_string())?.collect::<Result<_,_>>().map_err(|e|e.to_string())?;
    for id in &ids {
        auto_export::schedule(&db, id, path, &settings.export_formats)?;
    }
    Ok(ids.len() as u32)
}
#[tauri::command]
pub(crate) fn retry_exports(state: State<AppState>) -> Result<(), String> {
    let _guard = state.claim()?;
    state
        .db()?
        .execute(
            "UPDATE export_tasks SET status='pending',error=NULL WHERE status='failed'",
            [],
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}
