//! Only compiled-in upstream artifacts can be fetched. No recordings leave the app.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub struct Artifact {
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}
pub const WHISPER: Artifact = Artifact {
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small-q5_1.bin",
    bytes: 190085487,
    sha256: "ae85e4a935d7a567bd102fe55afc16bb595bdb618e11b2fc7591bc08120411bb",
};
pub const SEGMENTATION: Artifact = Artifact {
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2",
    bytes: 6958444, sha256: "24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488",
};
pub const EMBEDDING: Artifact = Artifact {
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/nemo_en_titanet_small.onnx",
    bytes: 40257283, sha256: "ad4a1802485d8b34c722d2a9d04249662f2ece5d28a7a039063ca22f515a789e",
};

pub fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::SeqCst) {
        Err("Download cancelled. Your installed models are unchanged.".into())
    } else {
        Ok(())
    }
}
pub fn fetch(
    artifact: &Artifact,
    path: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64),
) -> Result<(), String> {
    check_cancel(cancel)?;
    let response = ureq::AgentBuilder::new()
        .https_only(true)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(2))
        .redirects(8)
        .build()
        .get(artifact.url)
        .call()
        .map_err(|e| format!("Download failed: {e}. Check your connection and retry."))?;
    verified_copy(response.into_reader(), artifact, path, cancel, progress)
}
fn verified_copy(
    mut reader: impl Read,
    artifact: &Artifact,
    path: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64),
) -> Result<(), String> {
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut total = 0;
        let mut buffer = [0; 65536];
        loop {
            check_cancel(cancel)?;
            let n = reader
                .read(&mut buffer)
                .map_err(|e| format!("Download interrupted: {e}. Retry the download."))?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > artifact.bytes {
                return Err("Downloaded model exceeds its pinned size.".into());
            }
            hash.update(&buffer[..n]);
            file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
            progress(total);
        }
        if total != artifact.bytes || format!("{:x}", hash.finalize()) != artifact.sha256 {
            return Err(
                "Model checksum or size did not match. Retry from the trusted source.".into(),
            );
        }
        check_cancel(cancel)?;
        file.sync_all().map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}
pub fn extract_segmentation(archive: &Path, target: &Path) -> Result<(), String> {
    let decoder = bzip2::read::BzDecoder::new(fs::File::open(archive).map_err(|e| e.to_string())?);
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        // Never extract paths from an archive to the filesystem.
        if entry.path().map_err(|e| e.to_string())?.as_ref()
            == Path::new("sherpa-onnx-pyannote-segmentation-3-0/model.int8.onnx")
            && entry.header().entry_type().is_file()
        {
            if entry.size() > 20_000_000 {
                return Err("Segmentation model is too large.".into());
            }
            let mut file = fs::File::create(target).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
            return file.sync_all().map_err(|e| e.to_string());
        }
    }
    Err("Segmentation model is missing from the archive.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verifies_size_hash_cancellation_and_cleans_partial_files() {
        let path = std::env::temp_dir().join(format!("download-{}", uuid::Uuid::new_v4()));
        let spec = Artifact {
            url: "",
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        let cancel = AtomicBool::new(false);
        verified_copy(&b"abc"[..], &spec, &path, &cancel, |_| {}).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"abc");
        fs::remove_file(&path).unwrap();
        for data in [&b"bad"[..], &b"ab"[..], &b"abcd"[..]] {
            assert!(verified_copy(data, &spec, &path, &cancel, |_| {}).is_err());
            assert!(!path.exists());
        }
        assert!(verified_copy(&b"abc"[..], &spec, &path, &cancel, |_| {
            cancel.store(true, Ordering::SeqCst);
        })
        .is_err());
        assert!(!path.exists());
    }
}
