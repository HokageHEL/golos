use crate::library::Note;
use std::{fs::File, io::Write, path::Path};
use zip::{write::SimpleFileOptions, ZipWriter};
fn escape(text: &str) -> String {
    text.chars()
        .filter(|c| *c >= ' ' || ['\n', '\t', '\r'].contains(c))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
pub fn export(path: &Path, note: &Note) -> Result<(), String> {
    let mut body = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", escape(&note.title));
    for segment in &note.segments {
        let label = segment.speaker.as_deref().unwrap_or("");
        let text = format!(
            "[{:.3}–{:.3}] {}{}{}",
            segment.start_ms as f64 / 1000.,
            segment.end_ms as f64 / 1000.,
            label,
            if label.is_empty() { "" } else { ": " },
            segment.text
        );
        body.push_str("<w:p>");
        for (n, line) in text.lines().enumerate() {
            if n > 0 {
                body.push_str("<w:r><w:br/></w:r>");
            }
            body.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                escape(line)
            ));
        }
        body.push_str("</w:p>");
    }
    write_document(path, &body)
}
pub fn export_text(path: &Path, title: &str, text: &str) -> Result<(), String> {
    let mut body = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", escape(title));
    for paragraph in text.split("\n\n") {
        body.push_str("<w:p>");
        for (index, line) in paragraph.lines().enumerate() {
            if index > 0 {
                body.push_str("<w:r><w:br/></w:r>");
            }
            body.push_str(&format!(
                "<w:r><w:t xml:space=\"preserve\">{}</w:t></w:r>",
                escape(line)
            ));
        }
        body.push_str("</w:p>");
    }
    write_document(path, &body)
}
fn write_document(path: &Path, body: &str) -> Result<(), String> {
    let mut zip = ZipWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"#
    );
    for (name, content) in [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/document.xml", document.as_str()),
    ] {
        zip.start_file(name, SimpleFileOptions::default())
            .map_err(|e| e.to_string())?;
        zip.write_all(content.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    zip.finish()
        .and_then(|f| f.sync_all().map_err(zip::result::ZipError::Io))
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untimed_generated_text_exports_without_fabricated_timestamps() {
        use std::io::Read;
        let path =
            std::env::temp_dir().join(format!("golos-ai-docx-{}.docx", uuid::Uuid::new_v4()));
        export_text(
            &path,
            "Summary & notes",
            "Привіт <world>\nNext line\n\nDecision [s1]",
        )
        .unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("Summary &amp; notes"));
        assert!(xml.contains("&lt;world&gt;"));
        assert!(xml.contains("Decision [s1]"));
        assert!(!xml.contains("0.000"));
        drop(zip);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn writes_complete_docx_with_title_timestamps_labels_and_unicode() {
        use std::io::Read;
        let path = std::env::temp_dir().join(format!("golos-docx-{}.docx", uuid::Uuid::new_v4()));
        let note = Note {
            id: "one".into(),
            title: "Інтерв’ю & research".into(),
            created_at: 1,
            duration_seconds: 1.,
            status: "done".into(),
            segments: vec![crate::library::Segment {
                start_ms: 0,
                end_ms: 1000,
                speaker: Some("Alice".into()),
                text: "Hello <world>\nNext line".into(),
            }],
            error: None,
            folder: String::new(),
            tags: vec![],
        };
        export(&path, &note).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(zip.len(), 3);
        assert!(zip.by_name("[Content_Types].xml").is_ok());
        assert!(zip.by_name("_rels/.rels").is_ok());
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        assert!(xml.contains("Інтерв’ю &amp; research"));
        assert!(xml.contains("[0.000–1.000] Alice: Hello &lt;world&gt;"));
        assert!(xml.contains("<w:br/>"));
        fs_remove(&path);
    }
    fn fs_remove(path: &Path) {
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn xml_escapes_text_without_invalid_controls() {
        assert_eq!(escape("<Alice> & \u{0}"), "&lt;Alice&gt; &amp; ");
    }
}
