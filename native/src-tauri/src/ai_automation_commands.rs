use crate::{ai_automation, AppState};
use tauri::{Manager, State};

#[tauri::command]
pub(crate) async fn ai_automation_status(
    app: tauri::AppHandle,
) -> Result<ai_automation::Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        let label = state
            .automation_label
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        ai_automation::status(&db, label)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn ai_automation_configure(
    state: State<AppState>,
    rules: Vec<ai_automation::Rule>,
) -> Result<(), String> {
    let _guard = state.claim()?;
    ai_automation::configure(&*state.db()?, rules)
}

#[tauri::command]
pub(crate) fn ai_automation_action(
    state: State<AppState>,
    id: String,
    action: String,
) -> Result<(), String> {
    let _guard = state.claim()?;
    ai_automation::action(&*state.db()?, &id, &action)
}
