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

/// Includes queued tasks outside the bounded status list. The shared operation
/// guard prevents a task from being claimed while this single update runs.
#[tauri::command]
pub(crate) fn ai_automation_skip_queued(state: State<AppState>) -> Result<usize, String> {
    let _guard = state.claim()?;
    skip_queued(&*state.db()?)
}
fn skip_queued(db: &rusqlite::Connection) -> Result<usize, String> {
    db.execute("UPDATE ai_automation_tasks SET status='skipped',error='Skipped by user.',updated_at=?1 WHERE status='queued'", [std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_millis().min(i64::MAX as u128) as i64])
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bulk_skip_covers_hidden_queue_without_touching_other_states_or_results() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::library::init(&db, std::path::Path::new(":memory:")).unwrap();
        db.execute("INSERT INTO notes(id,title,created_at,duration_seconds,status) VALUES('note','Original',1,1,'done')",[]).unwrap();
        for index in 0..155 {
            let status = match index {
                150 => "running",
                151 => "done",
                152 => "failed",
                153 => "cancelled",
                154 => "skipped",
                _ => "queued",
            };
            db.execute("INSERT INTO ai_automation_tasks(id,note_id,rule_id,rule_name,kind,target,prompt,source,source_language,source_hash,segments,model,port,context_window,status,result_id,created_at,updated_at) VALUES(?1,'note',?1,'Summary','summary','','','single','auto','hash','[]','fixture',11434,8192,?2,'saved-result',1,1)",rusqlite::params![index.to_string(),status]).unwrap();
        }
        assert_eq!(ai_automation::tasks(&db).unwrap().len(), 100);
        assert_eq!(skip_queued(&db).unwrap(), 150);
        assert_eq!(skip_queued(&db).unwrap(), 0);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM ai_automation_tasks WHERE status='queued'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        for (index, status) in [
            (150, "running"),
            (151, "done"),
            (152, "failed"),
            (153, "cancelled"),
            (154, "skipped"),
        ] {
            let row: (String, Option<String>) = db
                .query_row(
                    "SELECT status,result_id FROM ai_automation_tasks WHERE id=?1",
                    [index.to_string()],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(row, (status.into(), Some("saved-result".into())));
        }
        assert_eq!(
            db.query_row("SELECT title FROM notes WHERE id='note'", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "Original"
        );
    }
}
