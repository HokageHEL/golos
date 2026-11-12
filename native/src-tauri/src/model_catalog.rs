//! Allowlisted multilingual Whisper models from one immutable upstream revision.
use crate::model_install::{self, Progress};
use serde::Serialize;
use std::{
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

// Exact byte sizes and SHA-256 values come from the upstream Hugging Face LFS
// metadata (`/api/models/ggerganov/whisper.cpp?blobs=true`), verified 2026-11-12.
// Update revision, size and digest together, then run the opt-in download test.
const REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const REPOSITORY: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve";
const MAX_REDIRECTS: usize = 5;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub file_name: &'static str,
    pub bytes: u64,
    pub description: &'static str,
    #[serde(skip)]
    sha256: &'static str,
}

static MODELS: [Model; 3] = [
    Model {
        id: "tiny",
        name: "Tiny",
        file_name: "ggml-tiny.bin",
        bytes: 77_691_713,
        description: "Smallest and fastest option, with lower transcription quality.",
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    Model {
        id: "base",
        name: "Base",
        file_name: "ggml-base.bin",
        bytes: 147_951_465,
        description: "A middle ground between speed, resource use, and quality.",
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    Model {
        id: "small",
        name: "Small (quantized)",
        file_name: "ggml-small-q5_1.bin",
        bytes: 190_085_487,
        description: "Higher quality potential, using more memory and processing time.",
        sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
    },
];

pub fn models() -> &'static [Model] {
    &MODELS
}

fn model_url(model: &Model) -> String {
    format!("{REPOSITORY}/{REVISION}/{}", model.file_name)
}

fn check_cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("Model download cancelled. Your previous model is unchanged.".into())
    } else {
        Ok(())
    }
}

pub fn download(
    root: &Path,
    id: &str,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
) -> Result<(), String> {
    let model = models()
        .iter()
        .find(|m| m.id == id)
        .ok_or("Unknown model.")?;
    check_cancelled(cancel)?;
    progress(Progress {
        phase: "connecting",
        copied_bytes: 0,
        total_bytes: model.bytes,
    });
    let agent = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(10))
        .redirects(0)
        .build();
    let mut url = model_url(model);
    for redirect in 0..=MAX_REDIRECTS {
        check_cancelled(cancel)?;
        // Follow redirects explicitly so a server cannot downgrade to HTTP.
        if !url.starts_with("https://") {
            return Err("Model server returned an insecure redirect.".into());
        }
        let response = match agent.get(&url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(code, response)) if (300..400).contains(&code) => response,
            Err(ureq::Error::Status(code, _)) => {
                return Err(format!("Model server returned HTTP {code}."))
            }
            Err(_) => return Err("Could not connect to the model server.".into()),
        };
        check_cancelled(cancel)?;
        if (300..400).contains(&response.status()) {
            if redirect == MAX_REDIRECTS {
                return Err("Too many model server redirects.".into());
            }
            url = response
                .header("Location")
                .ok_or("Model server returned an invalid redirect.")?
                .to_string();
            continue;
        }
        if let Some(value) = response.header("Content-Length") {
            let size: u64 = value
                .parse()
                .map_err(|_| "Model server returned an invalid content length.")?;
            if size != model.bytes {
                return Err("Model download size does not match the catalog.".into());
            }
        }
        return model_install::install_download(
            root,
            response.into_reader(),
            model.bytes,
            model.sha256,
            cancel,
            progress,
        );
    }
    Err("Too many model server redirects.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn unknown_id_is_rejected_without_network_or_filesystem_access() {
        let root = Path::new("/path/that/does/not/exist");
        let result = download(
            root,
            "https://example.com/model.bin",
            &AtomicBool::new(false),
            |_| panic!("no progress for unknown IDs"),
        );
        assert!(result.unwrap_err().contains("Unknown model"));
    }

    #[test]
    fn metadata_is_unique_valid_and_does_not_expose_hashes() {
        let mut ids = HashSet::new();
        for model in models() {
            assert!(ids.insert(model.id));
            assert!(!model.name.is_empty() && !model.description.is_empty() && model.bytes > 0);
            assert!(model.file_name.starts_with("ggml-") && model.file_name.ends_with(".bin"));
            assert!(!model.file_name.contains('/') && !model.file_name.contains('\\'));
            assert_eq!(model.sha256.len(), 64);
            assert!(model
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            assert!(serde_json::to_value(model).unwrap().get("sha256").is_none());
        }
    }

    #[test]
    fn urls_are_https_and_pin_the_full_revision() {
        assert_eq!(REVISION.len(), 40);
        assert!(REVISION.bytes().all(|b| b.is_ascii_hexdigit()));
        for model in models() {
            assert_eq!(
                model_url(model),
                format!(
                    "https://huggingface.co/ggerganov/whisper.cpp/resolve/{REVISION}/{}",
                    model.file_name
                )
            );
        }
    }

    #[test]
    #[ignore = "Set GOLOS_TEST_DOWNLOAD=tiny to download and load a pinned model in temporary storage"]
    fn downloads_and_loads_pinned_model() {
        struct Temporary(std::path::PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let id = std::env::var("GOLOS_TEST_DOWNLOAD")
            .expect("Set GOLOS_TEST_DOWNLOAD=tiny (or base/small)");
        let temporary = Temporary(
            std::env::temp_dir().join(format!("golos-download-test-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir(&temporary.0).unwrap();
        let mut phase = "";
        download(&temporary.0, &id, &AtomicBool::new(false), |progress| {
            if progress.phase != phase {
                phase = progress.phase;
                eprintln!("Model setup: {phase}");
            }
        })
        .unwrap();
        assert!(crate::model::inspect(&model_install::active(&temporary.0).unwrap()).is_ok());
    }
}
