use crate::library::{self, Note};
use serde::Deserialize;
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

/// Validated layout for a manual DOCX export. Defaults never travel here:
/// omitting options keeps the historical output through `export`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub include_timestamps: bool,
    pub include_speakers: bool,
    pub grouping: String,
}

impl Options {
    pub fn validate(self) -> Result<Self, String> {
        if self.grouping != "paragraph" && self.grouping != "same-speaker" {
            return Err("Choose paragraph or adjacent same-speaker DOCX grouping.".into());
        }
        Ok(self)
    }
}

struct Block {
    start_ms: i64,
    end_ms: i64,
    speaker: Option<String>,
    text: String,
}

fn blocks(note: &Note, grouping: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for segment in &note.segments {
        if grouping == "same-speaker" {
            if let Some(previous) = blocks.last_mut() {
                if segment.speaker.is_some() && previous.speaker == segment.speaker {
                    if !previous.text.is_empty() && !segment.text.is_empty() {
                        previous.text.push('\n');
                    }
                    previous.text.push_str(&segment.text);
                    previous.end_ms = previous.end_ms.max(segment.end_ms);
                    continue;
                }
            }
        }
        blocks.push(Block {
            start_ms: segment.start_ms,
            end_ms: segment.end_ms,
            speaker: segment.speaker.clone(),
            text: segment.text.clone(),
        });
    }
    blocks
}

fn speaker_label(value: &Option<String>) -> String {
    value
        .as_deref()
        .map(|name| {
            name.strip_prefix("speaker_")
                .filter(|number| number.parse::<u32>().is_ok())
                .map(|number| format!("Speaker {number}"))
                .unwrap_or_else(|| name.to_string())
        })
        .unwrap_or_default()
}

fn format_time(ms: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn push_paragraph(body: &mut String, text: &str) {
    body.push_str("<w:p>");
    for (index, line) in text.lines().enumerate() {
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
        push_paragraph(&mut body, &text);
    }
    write_document(path, &body)
}

/// Manual DOCX export with explicit, validated layout options.
pub fn export_layout(path: &Path, note: &Note, options: Options) -> Result<(), String> {
    let options = options.validate()?;
    library::validate_segments(&note.segments, note.duration_seconds)?;
    let mut body = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", escape(&note.title));
    for block in blocks(note, &options.grouping) {
        let mut text = String::new();
        if options.include_timestamps {
            text.push_str(&format!(
                "[{}–{}] ",
                format_time(block.start_ms),
                format_time(block.end_ms)
            ));
        }
        if options.include_speakers {
            let label = speaker_label(&block.speaker);
            if !label.is_empty() {
                text.push_str(&label);
                text.push_str(": ");
            }
        }
        text.push_str(&block.text);
        push_paragraph(&mut body, &text);
    }
    write_document(path, &body)
}

pub fn export_text(path: &Path, title: &str, text: &str) -> Result<(), String> {
    let mut body = format!("<w:p><w:r><w:t>{}</w:t></w:r></w:p>", escape(title));
    for paragraph in text.split("\n\n") {
        push_paragraph(&mut body, paragraph);
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
    use std::io::Read;

    fn fixture_note(title: &str) -> Note {
        Note {
            id: "one".into(),
            title: title.into(),
            created_at: 1,
            duration_seconds: 3.,
            status: "done".into(),
            segments: vec![
                crate::library::Segment {
                    start_ms: 0,
                    end_ms: 1000,
                    speaker: Some("speaker_1".into()),
                    text: "Hello <world>\nNext line".into(),
                },
                crate::library::Segment {
                    start_ms: 1000,
                    end_ms: 2000,
                    speaker: Some("speaker_1".into()),
                    text: "Привіт 🌍".into(),
                },
                crate::library::Segment {
                    start_ms: 2000,
                    end_ms: 3000,
                    speaker: Some("Bob".into()),
                    text: "Third".into(),
                },
            ],
            error: None,
            folder: String::new(),
            tags: vec![],
        }
    }

    fn read_xml(path: &Path) -> String {
        let mut zip = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut xml = String::new();
        zip.by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        xml
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("golos-{name}-{}.docx", uuid::Uuid::new_v4()))
    }

    #[test]
    fn untimed_generated_text_exports_without_fabricated_timestamps() {
        let path = temp_path("ai-docx");
        export_text(
            &path,
            "Summary & notes",
            "Привіт <world>\nNext line\n\nDecision [s1]",
        )
        .unwrap();
        let xml = read_xml(&path);
        assert!(xml.contains("Summary &amp; notes"));
        assert!(xml.contains("&lt;world&gt;"));
        assert!(xml.contains("Decision [s1]"));
        assert!(!xml.contains("0.000"));
        drop(xml);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn default_export_keeps_legacy_timestamps_labels_and_unicode() {
        let path = temp_path("docx");
        let note = fixture_note("Інтерв’ю & research");
        export(&path, &note).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(zip.len(), 3);
        assert!(zip.by_name("[Content_Types].xml").is_ok());
        assert!(zip.by_name("_rels/.rels").is_ok());
        drop(zip);
        let xml = read_xml(&path);
        assert!(xml.contains("Інтерв’ю &amp; research"));
        assert!(xml.contains("[0.000–1.000] speaker_1: Hello &lt;world&gt;"));
        assert!(xml.contains("<w:br/>"));
        assert!(xml.contains("Привіт 🌍"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn manual_layout_can_hide_timestamps_and_speakers_and_group_same_speaker() {
        let path = temp_path("layout");
        let note = fixture_note("Layout");
        export_layout(
            &path,
            &note,
            Options {
                include_timestamps: false,
                include_speakers: false,
                grouping: "same-speaker".into(),
            },
        )
        .unwrap();
        let xml = read_xml(&path);
        assert!(!xml.contains("[00:00:00.000"));
        assert!(!xml.contains("Speaker 1"));
        assert!(!xml.contains("Bob"));
        // Title + merged speaker_1 block + Bob block: adjacent same-speaker
        // paragraphs are combined while the speaker change closes the block.
        assert_eq!(xml.matches("<w:p>").count(), 3);
        assert!(xml.contains("Hello &lt;world&gt;"));
        assert!(xml.contains("Next line"));
        assert!(xml.contains("Привіт 🌍"));
        assert!(xml.contains(">Third<"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn manual_layout_includes_explicit_headers_and_keeps_distinct_speakers_apart() {
        let path = temp_path("layout-headers");
        let note = fixture_note("Headers");
        export_layout(
            &path,
            &note,
            Options {
                include_timestamps: true,
                include_speakers: true,
                grouping: "same-speaker".into(),
            },
        )
        .unwrap();
        let xml = read_xml(&path);
        assert!(xml.contains("[00:00:00.000–00:00:02.000] Speaker 1: "));
        assert!(xml.contains("[00:00:02.000–00:00:03.000] Bob: Third"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn options_reject_unknown_grouping_and_unsafe_segments_are_not_exported() {
        assert!(Options {
            include_timestamps: true,
            include_speakers: true,
            grouping: "turn".into(),
        }
        .validate()
        .is_err());
        let path = temp_path("invalid");
        let mut note = fixture_note("Invalid");
        note.segments[1].start_ms = -1;
        assert!(export_layout(
            &path,
            &note,
            Options {
                include_timestamps: true,
                include_speakers: true,
                grouping: "paragraph".into(),
            },
        )
        .is_err());
        assert!(!path.exists());
    }

    #[test]
    fn options_deserialize_from_camel_case_command_payload() {
        let options: Options = serde_json::from_str(
            r#"{"includeTimestamps":false,"includeSpeakers":true,"grouping":"same-speaker"}"#,
        )
        .unwrap();
        assert!(!options.include_timestamps);
        assert!(options.include_speakers);
        assert_eq!(options.grouping, "same-speaker");
    }

    #[test]
    fn xml_escapes_text_without_invalid_controls() {
        assert_eq!(escape("<Alice> & \u{0}"), "&lt;Alice&gt; &amp; ");
    }
}
