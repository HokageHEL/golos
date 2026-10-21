//! Tauri commands for the global dictation feature.
//!
//! The commands are thin: they validate input, hand the request to the Rust
//! runtime and return the current state. No command registers a shortcut, runs
//! a microphone or writes to the clipboard on its own.
use crate::dictation::{
    InsertOutcome, PermissionKind, Permissions,
};
use crate::dictation_runtime::Status;
use crate::dictation_settings::Settings;
use crate::AppState;
use tauri::{Manager, State};

#[tauri::command]
pub(crate) fn dictation_status(state: State<AppState>) -> Status {
    state.dictation.status()
}

#[tauri::command]
pub(crate) fn dictation_settings(state: State<AppState>) -> Settings {
    state.dictation.settings()
}

/// Apply new settings. The shortcut is registered before the old one is
/// removed, and nothing is persisted when the registration fails.
#[tauri::command]
pub(crate) fn dictation_configure(
    state: State<AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    state.dictation.configure(&state, settings)
}

#[tauri::command]
pub(crate) fn dictation_start(state: State<AppState>) -> Result<Status, String> {
    state.dictation.start(&state)?;
    Ok(state.dictation.status())
}

/// Stop and save. The save, transcription and optional processing run on a
/// worker, so the overlay can be hidden without interrupting the job.
#[tauri::command]
pub(crate) async fn dictation_stop(app: tauri::AppHandle) -> Result<Status, String> {
    let runtime = app.state::<AppState>().dictation.clone();
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = worker.state::<AppState>();
        runtime.stop_and_run(&state)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(app.state::<AppState>().dictation.status())
}

#[tauri::command]
pub(crate) fn dictation_pause(
    state: State<AppState>,
    paused: bool,
) -> Result<Status, String> {
    state.dictation.pause(&state, paused)?;
    Ok(state.dictation.status())
}

#[tauri::command]
pub(crate) fn dictation_retry_save(state: State<AppState>) -> Result<Status, String> {
    state.dictation.retry_save(&state)?;
    Ok(state.dictation.status())
}

#[tauri::command]
pub(crate) fn dictation_retry_transcribe(state: State<AppState>) -> Result<Status, String> {
    state.dictation.retry_transcribe(&state)?;
    Ok(state.dictation.status())
}

#[tauri::command]
pub(crate) fn dictation_retry_process(state: State<AppState>) -> Result<Status, String> {
    state.dictation.retry_process(&state)?;
    Ok(state.dictation.status())
}

/// Store the reviewed text. It stays in the job and is never inserted
/// automatically.
#[tauri::command]
pub(crate) fn dictation_preview_text(
    state: State<AppState>,
    text: String,
) -> Result<Status, String> {
    state.dictation.update_preview(&text)?;
    Ok(state.dictation.status())
}

/// Ask the native adapter for one single-use paste. The result reports
/// "paste requested", never "inserted".
#[tauri::command]
pub(crate) fn dictation_insert(
    state: State<AppState>,
) -> Result<InsertOutcome, String> {
    state.dictation.insert(&state)
}

#[tauri::command]
pub(crate) fn dictation_dismiss(state: State<AppState>) -> Result<Status, String> {
    state.dictation.dismiss(&state)?;
    Ok(state.dictation.status())
}

/// Explicitly discard the captured audio. The UI asks for confirmation first.
#[tauri::command]
pub(crate) fn dictation_discard(state: State<AppState>) -> Result<Status, String> {
    state.dictation.discard(&state)?;
    Ok(state.dictation.status())
}

#[tauri::command]
pub(crate) fn dictation_permissions(state: State<AppState>) -> Result<Permissions, String> {
    state.dictation.permissions()
}

#[tauri::command]
pub(crate) fn dictation_request_permission(
    state: State<AppState>,
    kind: PermissionKind,
) -> Result<Permissions, String> {
    state.dictation.request_permission(kind)
}

#[tauri::command]
pub(crate) fn dictation_open_permission_settings(
    state: State<AppState>,
    kind: PermissionKind,
) -> Result<(), String> {
    state.dictation.open_permission_settings(kind)
}

/// Hide the overlay. The Rust job keeps running.
#[tauri::command]
pub(crate) fn dictation_hide_overlay(state: State<AppState>) {
    state.dictation.hide_overlay();
}
