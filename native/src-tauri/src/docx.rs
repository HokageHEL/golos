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
    let mut zip = ZipWriter::new(File::create(path).map_err(|e| e.to_string())?);
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
    fn xml_escapes_text_without_invalid_controls() {
        assert_eq!(escape("<Alice> & \u{0}"), "&lt;Alice&gt; &amp; ");
    }
}
