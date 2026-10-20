//! Durable local multi-format export. Never replaces an existing destination.
use crate::library::{self, Note};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

pub const FORMATS: &[&str] = &["txt", "srt", "vtt", "md", "html", "csv", "json", "docx"];
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTask {
    pub note_id: String,
    pub title: String,
    pub status: String,
    pub error: Option<String>,
}

pub fn formats(values: &[String]) -> Result<(), String> {
    if values.is_empty()
        || values.len() > FORMATS.len()
        || values
            .iter()
            .any(|value| !FORMATS.contains(&value.as_str()))
    {
        return Err("Choose at least one supported export format.".into());
    }
    Ok(())
}
fn speaker(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(|name| {
            name.strip_prefix("speaker_")
                .filter(|n| n.parse::<u32>().is_ok())
                .map(|n| format!("Speaker {n}"))
                .unwrap_or_else(|| name.to_string())
        })
        .unwrap_or_default()
}
fn timestamp(ms: i64, separator: char) -> String {
    format!(
        "{:02}:{:02}:{:02}{}{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        separator,
        ms % 1000
    )
}
fn html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn md(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if "\\`*_{}[]<>#+.!|~-".contains(ch) {
                vec!['\\', ch]
            } else {
                vec![ch]
            }
        })
        .collect()
}
fn csv(value: &str) -> String {
    let value = if value
        .trim_start_matches(|ch: char| ch.is_whitespace() || ch.is_control() || ch == '\u{feff}')
        .starts_with(['=', '+', '-', '@'])
    {
        format!("'{value}")
    } else {
        value.to_string()
    };
    format!("\"{}\"", value.replace('"', "\"\""))
}

pub fn render(note: &Note, format: &str) -> Result<String, String> {
    library::validate_segments(&note.segments, note.duration_seconds)?;
    let text = |index: usize| {
        let segment = &note.segments[index];
        let label = speaker(&segment.speaker);
        if label.is_empty() {
            segment.text.clone()
        } else {
            format!("{label}: {}", segment.text)
        }
    };
    Ok(match format {
        "txt"=>(0..note.segments.len()).map(text).collect::<Vec<_>>().join("\n\n"),
        "srt"|"vtt"=>{
            let mut output=if format=="vtt"{"WEBVTT\n\n".to_string()}else{String::new()};
            for (index,segment) in note.segments.iter().enumerate(){
                if format=="srt" {output.push_str(&format!("{}\n",index+1));}
                output.push_str(&format!("{} --> {}\n{}\n\n",timestamp(segment.start_ms,if format=="srt"{','}else{'.'}),timestamp(segment.end_ms,if format=="srt"{','}else{'.'}),html(&text(index).replace("\r\n","\n").replace('\r',"\n").lines().filter(|line|!line.trim().is_empty()).collect::<Vec<_>>().join("\n"))));
            } output
        },
        "json"=>serde_json::to_string_pretty(&serde_json::json!({"version":1,"title":note.title,"timestampUnit":"milliseconds","segments":note.segments})).map_err(|e|e.to_string())?+"\n",
        "csv"=>{
            let mut output="\u{feff}\"start_ms\";\"end_ms\";\"speaker\";\"text\"\r\n".to_string();
            for segment in &note.segments{output.push_str(&[csv(&segment.start_ms.to_string()),csv(&segment.end_ms.to_string()),csv(&speaker(&segment.speaker)),csv(&segment.text)].join(";"));output.push_str("\r\n");} output
        },
        "md"=>{
            let mut output=format!("# {}\n\n",md(&note.title).replace('\n'," "));
            for segment in &note.segments {output.push_str(&format!("**{} – {} · {}**\n\n{}\n\n",timestamp(segment.start_ms,'.'),timestamp(segment.end_ms,'.'),md(&speaker(&segment.speaker)),md(&segment.text)));} output
        },
        "html"=>format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>{}</title><style>body{{font:17px/1.6 system-ui;max-width:52rem;margin:3rem auto;padding:0 1rem}}p{{white-space:pre-wrap}}section{{break-inside:avoid}}</style></head><body><h1>{}</h1>{}</body></html>",html(&note.title),html(&note.title),note.segments.iter().map(|segment|format!("<section><header>{} – {} · {}</header><p>{}</p></section>",timestamp(segment.start_ms,'.'),timestamp(segment.end_ms,'.'),html(&speaker(&segment.speaker)),html(&segment.text))).collect::<String>()),
        _=>return Err("Unsupported text export format.".into()),
    })
}

pub fn schedule(
    db: &Connection,
    note: &str,
    directory: &Path,
    selected: &[String],
) -> Result<(), String> {
    formats(selected)?;
    if !directory.is_absolute() {
        return Err("Choose an available export folder again.".into());
    }
    let changed=db.execute("INSERT INTO export_tasks(note_id,directory,formats,status) SELECT id,?2,?3,'pending' FROM notes WHERE id=?1 AND status='done' ON CONFLICT(note_id) DO UPDATE SET status='pending',error=NULL,directory=excluded.directory,formats=excluded.formats WHERE export_tasks.status!='running'",params![note,directory.to_str().ok_or("Unsupported folder path")?,serde_json::to_string(selected).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
    if changed != 1 {
        return Err("Only saved, completed transcripts can be exported.".into());
    }
    Ok(())
}
pub fn list(db: &Connection) -> Result<Vec<ExportTask>, String> {
    db.prepare("SELECT note_id,title,e.status,e.error FROM export_tasks e JOIN notes n ON n.id=e.note_id ORDER BY CASE e.status WHEN 'running' THEN 0 WHEN 'failed' THEN 1 WHEN 'pending' THEN 2 ELSE 3 END,e.rowid DESC LIMIT 1000").map_err(|e|e.to_string())?.query_map([],|row|Ok(ExportTask{note_id:row.get(0)?,title:row.get(1)?,status:row.get(2)?,error:row.get(3)?})).map_err(|e|e.to_string())?.collect::<Result<_,_>>().map_err(|e|e.to_string())
}
pub fn next(db: &Connection) -> Result<Option<String>, String> {
    db.query_row(
        "SELECT note_id FROM export_tasks WHERE status='pending' ORDER BY rowid LIMIT 1",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

fn safe_name(note: &Note) -> String {
    let name: String = note
        .title
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || [' ', '-', '_'].contains(&ch) {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .char_indices()
        .take_while(|(index, ch)| index + ch.len_utf8() <= 120)
        .map(|(_, ch)| ch)
        .collect();
    format!(
        "{}--{}",
        if name.trim().is_empty() {
            "Transcript"
        } else {
            name.trim()
        },
        note.id
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
            .take(80)
            .collect::<String>()
    )
}
fn publish(directory: &Path, note: &Note, format: &str) -> Result<PathBuf, String> {
    let pending = directory.join(format!(".golos-{}.pending", uuid::Uuid::new_v4()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        if format == "docx" {
            drop(file);
            crate::docx::export(&pending, note)?;
        } else {
            file.write_all(render(note, format)?.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
            drop(file);
        }
        for number in 0..1000 {
            let target = directory.join(format!(
                "{}{}.{format}",
                safe_name(note),
                if number == 0 {
                    String::new()
                } else {
                    format!("-{number}")
                }
            ));
            match fs::hard_link(&pending, &target) {
                Ok(()) => return Ok(target),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("Too many export filename collisions. Choose another output folder.".into())
    })();
    let _ = fs::remove_file(pending);
    result
}

/// Access only short database operations; external file writes never hold the library mutex.
pub trait ExportDatabase {
    fn access<T>(&self, action: impl FnOnce(&Connection) -> Result<T, String>)
        -> Result<T, String>;
}
impl ExportDatabase for Connection {
    fn access<T>(
        &self,
        action: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        action(self)
    }
}
impl ExportDatabase for std::sync::Mutex<Connection> {
    fn access<T>(
        &self,
        action: impl FnOnce(&Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let db = self
            .lock()
            .map_err(|_| "Library is unavailable. Restart Golos.".to_string())?;
        action(&db)
    }
}
pub fn process(
    db: &impl ExportDatabase,
    note_id: &str,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let (directory, selected): (String, String) = db.access(|db| {
        db.query_row(
            "SELECT directory,formats FROM export_tasks WHERE note_id=?1 AND status='pending'",
            [note_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| e.to_string())
    })?;
    db.access(|db| {
        db.execute(
            "UPDATE export_tasks SET status='running',error=NULL WHERE note_id=?1",
            [note_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;
    let result = (|| {
        let selected: Vec<String> = serde_json::from_str(&selected).map_err(|e| e.to_string())?;
        formats(&selected)?;
        if Path::new(&directory).canonicalize().map_err(|e| {
            format!("Export folder is unavailable. Reselect it or restore access: {e}")
        })? != Path::new(&directory)
        {
            return Err("Export folder moved or was replaced. Reselect it before retrying.".into());
        }
        let note = db.access(|db| library::get(db, note_id))?;
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&note).map_err(|e| e.to_string())?)
        );
        for format in selected {
            if cancelled.load(Ordering::SeqCst) {
                return Err("Export cancelled. Completed files were kept; retry to finish remaining formats.".into());
            }
            let completed:bool=db.access(|db|db.query_row("SELECT EXISTS(SELECT 1 FROM export_files WHERE note_id=?1 AND format=?2 AND directory=?3 AND digest=?4)",params![note_id,format,directory,digest],|row|row.get(0)).map_err(|e|e.to_string()))?;
            if completed {
                continue;
            }
            let target = publish(Path::new(&directory), &note, &format)?;
            db.access(|db|{db.execute("INSERT INTO export_files(note_id,format,directory,digest,path) VALUES(?1,?2,?3,?4,?5)",params![note_id,format,directory,digest,target.to_str().ok_or("Unsupported output path")?]).map_err(|e|e.to_string())?;Ok(())})?;
        }
        Ok(())
    })();
    db.access(|db| {
        db.execute(
            "UPDATE export_tasks SET status=?2,error=?3 WHERE note_id=?1",
            params![
                note_id,
                if result.is_ok() { "done" } else { "failed" },
                result.as_ref().err()
            ],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    })?;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Segment;
    struct Fixture {
        root: PathBuf,
        output: PathBuf,
        db: Connection,
        note: Note,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("golos-auto-export-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("output")).unwrap();
            let root = root.canonicalize().unwrap();
            let output = root.join("output");
            let db = Connection::open_in_memory().unwrap();
            library::init(&db, Path::new(":memory:")).unwrap();
            let note = Note {
                id: uuid::Uuid::new_v4().to_string(),
                title: "Інтерв'ю <one>".into(),
                created_at: 1,
                duration_seconds: 2.0,
                status: "done".into(),
                segments: vec![Segment {
                    start_ms: 100,
                    end_ms: 1200,
                    speaker: Some("Олена & Alice".into()),
                    text: "Привіт <script>alert(1)</script>\nNext line".into(),
                }],
                error: None,
                folder: String::new(),
                tags: vec![],
            };
            db.execute("INSERT INTO notes(id,title,created_at,duration_seconds,status,segments) VALUES(?1,?2,1,2,'done',?3)",params![note.id,note.title,serde_json::to_string(&note.segments).unwrap()]).unwrap();
            Self {
                root,
                output,
                db,
                note,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    #[test]
    fn exports_all_formats_without_overwriting_and_skips_identical_completed_results() {
        let fixture = Fixture::new();
        let selected = FORMATS
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>();
        let original = fixture
            .output
            .join(format!("{}.txt", safe_name(&fixture.note)));
        fs::write(&original, "Existing user file").unwrap();
        schedule(&fixture.db, &fixture.note.id, &fixture.output, &selected).unwrap();
        process(&fixture.db, &fixture.note.id, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read_to_string(original).unwrap(), "Existing user file");
        assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 9);
        let html_path: String = fixture
            .db
            .query_row(
                "SELECT path FROM export_files WHERE format='html'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let html = fs::read_to_string(html_path).unwrap();
        assert!(html.contains("&lt;script&gt;"));
        assert!(!html.contains("<script>"));
        let json_path: String = fixture
            .db
            .query_row(
                "SELECT path FROM export_files WHERE format='json'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(json_path).unwrap()).unwrap();
        assert_eq!(json["segments"][0]["startMs"], 100);
        assert_eq!(json["segments"][0]["speaker"], "Олена & Alice");
        schedule(&fixture.db, &fixture.note.id, &fixture.output, &selected).unwrap();
        process(&fixture.db, &fixture.note.id, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 9);
        fixture
            .db
            .execute(
                "UPDATE notes SET title='Changed title' WHERE id=?1",
                [&fixture.note.id],
            )
            .unwrap();
        schedule(&fixture.db, &fixture.note.id, &fixture.output, &selected).unwrap();
        process(&fixture.db, &fixture.note.id, &AtomicBool::new(false)).unwrap();
        assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 17);
    }
    #[test]
    fn cancellation_and_missing_output_are_retryable_without_retranscribing() {
        let fixture = Fixture::new();
        let selected = vec!["txt".into(), "vtt".into()];
        schedule(&fixture.db, &fixture.note.id, &fixture.output, &selected).unwrap();
        assert!(process(&fixture.db, &fixture.note.id, &AtomicBool::new(true)).is_err());
        assert_eq!(list(&fixture.db).unwrap()[0].status, "failed");
        assert_eq!(
            library::get(&fixture.db, &fixture.note.id).unwrap().status,
            "done"
        );
        assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 0);
        schedule(&fixture.db, &fixture.note.id, &fixture.output, &selected).unwrap();
        fs::remove_dir(&fixture.output).unwrap();
        assert!(process(&fixture.db, &fixture.note.id, &AtomicBool::new(false)).is_err());
        fs::create_dir(&fixture.output).unwrap();
        schedule(&fixture.db, &fixture.note.id, &fixture.output, &selected).unwrap();
        process(&fixture.db, &fixture.note.id, &AtomicBool::new(false)).unwrap();
        assert_eq!(list(&fixture.db).unwrap()[0].status, "done");
        assert_eq!(fs::read_dir(&fixture.output).unwrap().count(), 2);
    }
    #[test]
    fn validates_formats_neutralizes_csv_formulas_and_sanitizes_filenames() {
        let mut fixture = Fixture::new();
        assert!(formats(&[]).is_err());
        assert!(formats(&["../../wav".into()]).is_err());
        fixture.note.segments[0].text = "  =HYPERLINK(\"url\")".into();
        fixture.note.id = "../../escape".into();
        fixture.note.title = "../outside".into();
        assert!(render(&fixture.note, "csv")
            .unwrap()
            .contains("\"'  =HYPERLINK(\"\"url\"\")\""));
        assert!(!safe_name(&fixture.note).contains('/'));
        let path = publish(&fixture.output, &fixture.note, "json").unwrap();
        assert_eq!(path.parent(), Some(fixture.output.as_path()));
    }
    #[test]
    fn interrupted_export_recovery_preserves_saved_transcripts() {
        let fixture = Fixture::new();
        schedule(
            &fixture.db,
            &fixture.note.id,
            &fixture.output,
            &["txt".into()],
        )
        .unwrap();
        fixture
            .db
            .execute("UPDATE export_tasks SET status='running'", [])
            .unwrap();
        crate::automation::recover(&fixture.db).unwrap();
        assert_eq!(list(&fixture.db).unwrap()[0].status, "failed");
        assert_eq!(
            library::get(&fixture.db, &fixture.note.id)
                .unwrap()
                .segments[0]
                .text,
            fixture.note.segments[0].text
        );
    }
    #[test]
    fn subtitle_exports_keep_multiline_text_inside_one_valid_cue() {
        let mut fixture = Fixture::new();
        fixture.note.segments[0].text = "First\r\n\r\n\r\nLast".into();
        let result = render(&fixture.note, "vtt").unwrap();
        assert!(result.contains("00:00:00.100 --> 00:00:01.200"));
        assert!(!result.contains("First\n\nLast"));
        assert!(result.contains("First\nLast"));
        fixture.note.segments[0].text = "\u{feff}=SUM(1,2)".into();
        assert!(render(&fixture.note, "csv")
            .unwrap()
            .contains("'\u{feff}=SUM(1,2)"));
    }
}
