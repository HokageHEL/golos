use crate::{ai, ai_generation, authorize_audio, docx, engine, library, local_ai, model, AppState};
use rusqlite::params;
use serde::Serialize;
use std::{fs, sync::atomic::Ordering};
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
#[derive(Serialize)]
pub(crate) struct Status {
    settings: ai::Settings,
    results: Vec<ai::Summary>,
}
#[tauri::command]
pub(crate) async fn ai_status(
    app: tauri::AppHandle,
    note_id: Option<String>,
    offset: Option<i64>,
) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        Ok(Status {
            settings: ai::settings(&db)?,
            results: if let Some(id) = note_id {
                if offset.unwrap_or(0) == 0 {
                    ai::list(&db, &id)?
                } else {
                    ai::list_page(&db, &id, offset.unwrap_or(0))?
                }
            } else {
                vec![]
            },
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn ai_configure(state: State<AppState>, settings: ai::Settings) -> Result<(), String> {
    let _guard = state.claim()?;
    ai::configure(&*state.db()?, &settings)
}
#[tauri::command]
pub(crate) async fn ai_models(
    app: tauri::AppHandle,
    port: u16,
) -> Result<Vec<local_ai::Model>, String> {
    let state = app.state::<AppState>();
    let _guard = state.claim()?;
    state.cancelled.store(false, Ordering::SeqCst);
    local_ai::Client::new(port)?.models(&state.cancelled).await
}
#[tauri::command]
pub(crate) async fn ai_get(app: tauri::AppHandle, id: String) -> Result<ai::Work, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let db = state.db()?;
        ai::get(&db, &id)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub(crate) fn ai_delete(state: State<AppState>, id: String) -> Result<(), String> {
    let _guard = state.claim()?;
    state
        .db()?
        .execute("DELETE FROM ai_results WHERE id=?1", [id])
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub(crate) fn ai_segments(
    state: State<AppState>,
    id: String,
) -> Result<Vec<library::Segment>, String> {
    ai::applicable(&*state.db()?, &id)
}
#[tauri::command]
pub(crate) async fn ai_generate(
    app: tauri::AppHandle,
    request: ai::Request,
) -> Result<ai::Work, String> {
    let state = app.state::<AppState>();
    let _guard = state.claim()?;
    state.cancelled.store(false, Ordering::SeqCst);
    let (work, port) = {
        let db = state.db()?;
        let settings = ai::settings(&db)?;
        let speech = request.kind == "speech_english";
        let name = if speech {
            model::inspect(&state.root.join("model.bin"))?
        } else {
            settings.model.clone()
        };
        (
            ai::create(
                &db,
                &request,
                &name,
                if speech { 0 } else { settings.context_window },
            )?,
            settings.port,
        )
    };
    run(&app, work, port).await
}
#[tauri::command]
pub(crate) async fn ai_retry(app: tauri::AppHandle, id: String) -> Result<ai::Work, String> {
    let state = app.state::<AppState>();
    let _guard = state.claim()?;
    state.cancelled.store(false, Ordering::SeqCst);
    let (work, port) = {
        let db = state.db()?;
        let old = ai::get(&db, &id)?;
        let settings = ai::settings(&db)?;
        let speech = old.kind == "speech_english";
        let name = if speech {
            model::inspect(&state.root.join("model.bin"))?
        } else {
            settings.model.clone()
        };
        (
            ai::retry(
                &db,
                &id,
                &name,
                if speech { 0 } else { settings.context_window },
            )?,
            settings.port,
        )
    };
    run(&app, work, port).await
}
async fn run(app: &tauri::AppHandle, work: ai::Work, port: u16) -> Result<ai::Work, String> {
    run_with(app, work, port, None).await
}
/// Shared runner for opt-in automatic rules. The final saved phase makes the
/// result's provenance visible in the workspace and in portable backups.
pub(crate) async fn automatic(
    app: &tauri::AppHandle,
    work: ai::Work,
    port: u16,
    rule: &str,
) -> Result<ai::Work, String> {
    run_with(app, work, port, Some(rule)).await
}
fn automatic_phase(rule: Option<&str>, phase: &str) -> String {
    rule.map(|rule| format!("Automatic {rule} · {phase}"))
        .unwrap_or_else(|| phase.into())
}
async fn run_with(
    app: &tauri::AppHandle,
    mut work: ai::Work,
    port: u16,
    automatic: Option<&str>,
) -> Result<ai::Work, String> {
    let state = app.state::<AppState>();
    let result=async{
        if work.kind=="speech_english"{
            let path={let db=state.db()?;authorize_audio(&state.root,&db,&work.note_id)?};let model=state.root.join("model.bin");let cancel=state.cancelled.clone();let language=work.source_language.clone();let event=app.clone();let id=work.id.clone();let note=work.note_id.clone();
            work.phase="Translating speech with Whisper".into();ai::checkpoint(&*state.db()?,&work)?;
            let mut segments=tauri::async_runtime::spawn_blocking(move||engine::transcribe_task(&model,&path,&language,true,cancel,move|percent|{let _=event.emit("ai-progress",serde_json::json!({"id":id,"noteId":note,"phase":format!("Translating speech · {percent}%")}));})).await.map_err(|e|e.to_string())??;
            ai_generation::carry_speakers(&mut segments,&work.source);work.content=segments.iter().map(|segment|segment.text.as_str()).collect::<Vec<_>>().join("\n\n");work.segments=segments;
        }else{
            let client=local_ai::Client::new(port)?;let model=client.inspect(&work.model,&state.cancelled).await?;let history=ai::chat_history(&*state.db()?,&work)?;
            ai_generation::generate(&mut work,&client,&model,&history,&state.cancelled,|current|{ai::checkpoint(&*state.db()?,current)?;let _=app.emit("ai-progress",serde_json::json!({"id":current.id,"noteId":current.note_id,"phase":current.phase}));Ok(())}).await?;
        }
        let duration=library::get(&*state.db()?,&work.note_id)?.duration_seconds;library::validate_segments(&work.segments,duration)?;
        if state.cancelled.load(Ordering::SeqCst){return Err("Generation cancelled. Earlier results and the source were preserved.".into());}Ok::<(),String>(())
    }.await;
    match result {
        Ok(()) => {
            work.status = "done".into();
            work.phase = automatic_phase(automatic, "Complete");
        }
        Err(error) => {
            work.status = if state.cancelled.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            work.phase = automatic_phase(
                automatic,
                if work.status == "cancelled" {
                    "Cancelled"
                } else {
                    "Failed"
                },
            );
            work.error = Some(error);
        }
    }
    ai::checkpoint(&*state.db()?, &work)?;
    let _ = app.emit(
        "ai-progress",
        serde_json::json!({"id":work.id,"noteId":work.note_id,"phase":work.phase}),
    );
    Ok(work)
}
#[tauri::command]
pub(crate) async fn ai_recording(
    app: tauri::AppHandle,
    id: String,
) -> Result<library::Note, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        let db = state.db()?;
        let work = ai::get(&db, &id)?;
        let audio = authorize_audio(&state.root, &db, &work.note_id)?;
        clone_recording(&db, &state.root, &audio, work)
    })
    .await
    .map_err(|e| e.to_string())?
}
fn clone_recording(
    db: &rusqlite::Connection,
    root: &std::path::Path,
    audio: &std::path::Path,
    work: ai::Work,
) -> Result<library::Note, String> {
    if work.status != "done" || work.segments.is_empty() {
        return Err("A completed translated or cleaned transcript is required.".into());
    }
    let mut note = library::get(db, &work.note_id)?;
    library::validate_segments(&work.segments, note.duration_seconds)?;
    note.id = uuid::Uuid::new_v4().to_string();
    let title = format!(
        "[{}] {}",
        if work.kind == "cleanup" {
            "Cleaned"
        } else if work.kind == "speech_english" {
            "English translation"
        } else {
            &work.target
        },
        note.title
    );
    note.title = title
        .char_indices()
        .take_while(|(index, ch)| index + ch.len_utf8() <= 200)
        .map(|(_, ch)| ch)
        .collect();
    note.created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs() as i64;
    note.status = "done".into();
    note.error = None;
    note.segments = work.segments;
    let target = root.join("audio").join(format!("{}.wav", note.id));
    let mut published = false;
    let result = (|| {
        let tx = db.unchecked_transaction().map_err(|e| e.to_string())?;
        library::insert(&tx, &note)?;
        tx.execute(
            "UPDATE notes SET segments=?1 WHERE id=?2",
            params![
                serde_json::to_string(&note.segments).map_err(|e| e.to_string())?,
                note.id
            ],
        )
        .map_err(|e| e.to_string())?;
        tx.execute(
            "INSERT INTO organization(note_id,folder,tags) VALUES(?1,?2,?3)",
            params![
                note.id,
                note.folder,
                serde_json::to_string(&note.tags).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        fs::hard_link(audio, &target).map_err(|e| e.to_string())?;
        published = true;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(note)
    })();
    if result.is_err() && published {
        let _ = fs::remove_file(target);
    }
    result
}
#[tauri::command]
pub(crate) async fn ai_docx(app: tauri::AppHandle, id: String) -> Result<bool, String> {
    let guard = app.state::<AppState>().claim()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = guard;
        let state = app.state::<AppState>();
        let (work, mut note) = {
            let db = state.db()?;
            let work = ai::get(&db, &id)?;
            let note = library::get(&db, &work.note_id)?;
            (work, note)
        };
        if work.status != "done" {
            return Err("Finish generation before exporting.".into());
        }
        let Some(file) = app
            .dialog()
            .file()
            .set_file_name("generated-result.docx")
            .add_filter("Word document", &["docx"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let target = file.into_path().map_err(|e| e.to_string())?;
        let pending = target.with_file_name(format!(".golos-ai-{}.pending", uuid::Uuid::new_v4()));
        note.title = format!(
            "{} · {}{}{}",
            note.title,
            work.kind,
            if work.target.is_empty() { "" } else { " · " },
            work.target
        );
        let result = (|| {
            if work.segments.is_empty() {
                docx::export_text(&pending, &note.title, &work.content)?;
            } else {
                note.segments = work.segments;
                docx::export(&pending, &note)?;
            }
            fs::rename(&pending, target).map_err(|e| e.to_string())?;
            Ok(true)
        })();
        let _ = fs::remove_file(pending);
        result
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_copy_preserves_original_audio_transcript_and_organization() {
        let (db, request) = crate::ai::tests::fixture();
        crate::library::organize(
            &db,
            "source",
            "Private interview",
            "Research",
            &["work".into()],
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!("golos-ai-clone-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("audio")).unwrap();
        let audio = root.join("original.wav");
        std::fs::write(&audio, b"test owned audio").unwrap();
        let mut work = crate::ai::create(&db, &request, "local", 8192).unwrap();
        assert!(super::clone_recording(&db, &root, &audio, work.clone()).is_err());
        work.status = "done".into();
        work.segments = work.source.clone();
        work.segments[0].text = "Ми домовилися".into();
        let a = super::clone_recording(&db, &root, &audio, work.clone()).unwrap();
        let b = super::clone_recording(&db, &root, &audio, work).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(a.folder, "Research");
        assert_eq!(a.tags, vec!["work"]);
        assert_eq!(a.segments[0].speaker.as_deref(), Some("Alice"));
        assert_eq!(
            crate::library::get(&db, "source").unwrap().segments[0].text,
            "We agreed to ship on Friday."
        );
        std::fs::remove_file(root.join("audio").join(format!("{}.wav", a.id))).unwrap();
        assert_eq!(
            std::fs::read(root.join("audio").join(format!("{}.wav", b.id))).unwrap(),
            b"test owned audio"
        );
        assert!(audio.exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
