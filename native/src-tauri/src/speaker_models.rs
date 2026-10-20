//! Publish a validated pair together. Pending directories are never active;
//! completed generations are immutable, so failed replacement retains the old pair.
use std::{
    fs,
    path::{Path, PathBuf},
};

const SEGMENTATION: &str = "segmentation.onnx";
const EMBEDDING: &str = "embedding.onnx";

fn generations(root: &Path) -> Result<Vec<(u64, PathBuf)>, String> {
    let directory = root.join("speakers");
    if !directory.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(n) = name
            .to_str()
            .and_then(|name| name.strip_prefix("generation-"))
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        result.push((n, entry.path()));
    }
    result.sort_by_key(|(n, _)| *n);
    Ok(result)
}

pub fn active(root: &Path) -> Result<Option<(PathBuf, PathBuf)>, String> {
    Ok(generations(root)?.into_iter().rev().find_map(|(_, dir)| {
        let seg = dir.join(SEGMENTATION);
        let emb = dir.join(EMBEDDING);
        if seg.is_file() && emb.is_file() {
            Some((seg, emb))
        } else {
            None
        }
    }))
}

pub fn install(root: &Path, segmentation: &Path, embedding: &Path) -> Result<(), String> {
    install_validated(
        root,
        segmentation,
        embedding,
        crate::diarize::validate_models,
    )
}

pub fn install_cancellable(
    root: &Path,
    segmentation: &Path,
    embedding: &Path,
    cancelled: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    let license = fs::read(segmentation.with_extension("LICENSE"))
        .map_err(|e| format!("Missing segmentation model license: {e}"))?;
    install_validated(root, segmentation, embedding, |seg, emb| {
        crate::diarize::validate_models(seg, emb)?;
        let notice = seg
            .parent()
            .ok_or("Missing speaker installation directory.")?
            .join("LICENSE-segmentation.txt");
        fs::write(&notice, &license).map_err(|e| e.to_string())?;
        fs::File::open(notice)
            .and_then(|file| file.sync_all())
            .map_err(|e| e.to_string())?;
        crate::download::check_cancel(cancelled)
    })
}

fn install_validated(
    root: &Path,
    segmentation: &Path,
    embedding: &Path,
    validate: impl FnOnce(&Path, &Path) -> Result<(), String>,
) -> Result<(), String> {
    let directory = root.join("speakers");
    fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let next = generations(root)?
        .last()
        .map(|(n, _)| *n)
        .unwrap_or(0)
        .checked_add(1)
        .ok_or("Too many model generations.")?;
    let pending = directory.join(format!("pending-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&pending).map_err(|e| e.to_string())?;
    let result = (|| {
        for (source, name) in [(segmentation, SEGMENTATION), (embedding, EMBEDDING)] {
            let destination = pending.join(name);
            fs::copy(source, &destination)
                .map_err(|e| format!("Cannot copy speaker model: {e}"))?;
            fs::File::open(&destination)
                .and_then(|file| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        validate(&pending.join(SEGMENTATION), &pending.join(EMBEDDING))?;
        let _ = fs::File::open(&pending).and_then(|file| file.sync_all());
        // No replacement of a live directory, including on Windows. AppState's
        // busy guard serializes installs in the single desktop instance.
        fs::rename(&pending, directory.join(format!("generation-{next:020}")))
            .map_err(|e| e.to_string())?;
        let _ = fs::File::open(&directory).and_then(|file| file.sync_all());
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&pending);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_publishes_pairs_and_failed_or_interrupted_install_keeps_previous() {
        let root = std::env::temp_dir().join(format!("golos-speakers-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("source");
        fs::write(&source, b"model").unwrap();
        install_validated(&root, &source, &source, |_, _| Ok(())).unwrap();
        let original = active(&root).unwrap().unwrap();
        assert!(install_validated(&root, &source, &source, |_, _| Err("invalid".into())).is_err());
        assert_eq!(active(&root).unwrap().unwrap(), original);
        assert!(install_validated(&root, &source, &root.join("missing"), |_, _| Ok(())).is_err());
        assert_eq!(active(&root).unwrap().unwrap(), original);
        fs::create_dir(root.join("speakers/pending-interrupted")).unwrap();
        assert_eq!(active(&root).unwrap().unwrap(), original);
        install_validated(&root, &source, &source, |_, _| Ok(())).unwrap();
        assert_ne!(active(&root).unwrap().unwrap(), original);
        assert!(original.0.exists() && original.1.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
