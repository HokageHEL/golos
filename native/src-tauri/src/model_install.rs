//! Copy and validate in a private staging directory, then publish a new immutable
//! generation. Never rename over a live model (which is not portable to Windows).
use crate::model;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const RESERVE_BYTES: u64 = 16 * 1024 * 1024;
const CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub phase: &'static str,
    pub copied_bytes: u64,
    pub total_bytes: u64,
}

fn cancelled(flag: &AtomicBool) -> Result<(), String> {
    if flag.load(Ordering::SeqCst) {
        Err("Model import cancelled. Your previous model is unchanged.".into())
    } else {
        Ok(())
    }
}

fn directory(root: &Path) -> Result<PathBuf, String> {
    let path = root.join("whisper-models");
    match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.file_type().is_dir() => {
            return Err("The model directory must be a local directory, not a link.".into());
        }
        Ok(_) => (),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|e| format!("Cannot create model directory: {e}"))?;
        }
        Err(error) => return Err(error.to_string()),
    }
    Ok(path)
}

fn generation_number(name: &str) -> Option<u64> {
    let value = name.strip_prefix("generation-")?;
    (value.len() == 20 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn generations(root: &Path) -> Result<Vec<(u64, PathBuf)>, String> {
    let path = root.join("whisper-models");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Ok(meta) if meta.file_type().is_dir() => (),
        _ => return Err("Cannot read the local model directory.".into()),
    }
    let mut values = vec![];
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let Some(number) = entry.file_name().to_str().and_then(generation_number) else {
            continue;
        };
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            values.push((number, entry.path()));
        }
    }
    values.sort_by_key(|(number, _)| *number);
    Ok(values)
}

pub fn active(root: &Path) -> Result<PathBuf, String> {
    for (_, path) in generations(root)?.into_iter().rev() {
        let model = path.join("model.bin");
        if fs::symlink_metadata(&model).is_ok_and(|meta| meta.file_type().is_file()) {
            return Ok(model);
        }
    }
    // Existing preview installations keep working without copying or moving
    // their model on upgrade. The first successful replacement uses generations.
    Ok(root.join("model.bin"))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("Cannot flush model directory: {e}"))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), String> {
    // Opening a directory with std::fs::File is not portable to Windows.
    // File contents are flushed explicitly before the unique-name rename.
    Ok(())
}

pub fn recover(root: &Path) -> Result<(), String> {
    let path = directory(root)?;
    for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        let owned = name
            .to_str()
            .and_then(|name| name.strip_prefix("pending-"))
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_ok());
        if owned && entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            fs::remove_dir_all(entry.path())
                .map_err(|e| format!("Cannot clean interrupted model import: {e}"))?;
        }
    }
    sync_directory(&path)?;
    prune_old(root);
    Ok(())
}

// Retain the current and previous generations. Cleanup is best-effort because
// failure to reclaim an older copy must not turn a successful activation into
// an installation failure. A later startup retries it. Legacy model.bin stays
// untouched for compatibility and manual rollback.
fn prune_old(root: &Path) {
    let Ok(values) = generations(root) else {
        return;
    };
    let published: Vec<_> = values
        .into_iter()
        .filter(|(_, path)| {
            fs::read_dir(path).is_ok_and(|entries| {
                let entries: Vec<_> = entries.collect::<Result<_, _>>().unwrap_or_default();
                entries.len() == 1
                    && entries[0].file_name() == "model.bin"
                    && entries[0].file_type().is_ok_and(|kind| kind.is_file())
            })
        })
        .collect();
    let excess = published.len().saturating_sub(2);
    for (_, path) in published.into_iter().take(excess) {
        if let Err(error) = fs::remove_dir_all(&path) {
            eprintln!(
                "Could not remove previous model copy {}: {error}",
                path.display()
            );
        }
    }
}

pub fn install(
    root: &Path,
    source: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<(), String> {
    install_with(
        root,
        source,
        cancel,
        progress,
        validate_model,
        available_space,
    )
}

fn validate_model(path: &Path) -> Result<(), String> {
    let mut options = whisper_rs::WhisperContextParameters::default();
    options.use_gpu(false);
    whisper_rs::WhisperContext::new_with_params(
        path.to_str().ok_or("Unsupported model path")?,
        options,
    )
    .map_err(|e| format!("Cannot load this model: {e}"))?;
    Ok(())
}

fn available_space(path: &Path) -> Result<u64, String> {
    fs2::available_space(path).map_err(|e| format!("Cannot check free disk space: {e}"))
}

struct Transfer<R> {
    reader: R,
    total: u64,
    sha256: Option<String>,
    phase: &'static str,
}

pub fn install_download(
    root: &Path,
    reader: impl Read,
    total: u64,
    sha256: &str,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
) -> Result<(), String> {
    install_transfer(
        root,
        Transfer {
            reader,
            total,
            sha256: Some(sha256.into()),
            phase: "downloading",
        },
        cancel,
        progress,
        validate_model,
        available_space,
    )
}

fn install_with(
    root: &Path,
    source: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(Progress),
    validate: impl FnOnce(&Path) -> Result<(), String>,
    available: impl FnOnce(&Path) -> Result<u64, String>,
) -> Result<(), String> {
    cancelled(cancel)?;
    model::inspect(source)?;
    let input = File::open(source).map_err(|e| format!("Cannot open model: {e}"))?;
    let metadata = input.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err("Choose a model file.".into());
    }
    install_transfer(
        root,
        Transfer {
            reader: input,
            total: metadata.len(),
            sha256: None,
            phase: "copying",
        },
        cancel,
        progress,
        validate,
        available,
    )
}

fn copy_stream(
    input: &mut impl Read,
    output: &mut impl Write,
    total: u64,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<String, String> {
    let mut copied = 0;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; CHUNK_BYTES];
    let mut reported_at = Instant::now();
    progress(0);
    loop {
        cancelled(cancel)?;
        let count = input
            .read(&mut buffer)
            .map_err(|e| format!("Cannot read model: {e}"))?;
        if count == 0 {
            break;
        }
        cancelled(cancel)?;
        copied += count as u64;
        if copied > total {
            return Err("Model size differs from the expected size. The source may have changed; try again.".into());
        }
        output
            .write_all(&buffer[..count])
            .map_err(|e| format!("Cannot copy model (check free disk space): {e}"))?;
        hash.update(&buffer[..count]);
        // Network readers can yield just a few KiB at a time. Avoid flooding
        // the webview with one event per TCP read while preserving cancellation.
        if copied == total || reported_at.elapsed() >= Duration::from_millis(100) {
            progress(copied);
            reported_at = Instant::now();
        }
    }
    if copied != total {
        return Err(
            "Model size differs from the expected size. The source may have changed; try again."
                .into(),
        );
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn install_transfer(
    root: &Path,
    mut transfer: Transfer<impl Read>,
    cancel: &AtomicBool,
    mut progress: impl FnMut(Progress),
    validate: impl FnOnce(&Path) -> Result<(), String>,
    available: impl FnOnce(&Path) -> Result<u64, String>,
) -> Result<(), String> {
    cancelled(cancel)?;
    let total = transfer.total;
    let required = total
        .checked_add(RESERVE_BYTES)
        .ok_or("Model is too large to copy.")?;
    let directory = directory(root)?;
    if available(&directory)? < required {
        return Err(format!("Not enough free disk space. Model import needs {} MiB free for the new copy and a 16 MiB reserve. The current model is retained.", required.div_ceil(1024 * 1024)));
    }
    let next = generations(root)?
        .last()
        .map_or(0, |(n, _)| *n)
        .checked_add(1)
        .ok_or("Too many model generations.")?;
    let pending = directory.join(format!("pending-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&pending).map_err(|e| format!("Cannot stage model: {e}"))?;
    let result = (|| {
        let destination = pending.join("model.bin");
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
            .map_err(|e| format!("Cannot create model copy: {e}"))?;
        let checksum = copy_stream(&mut transfer.reader, &mut output, total, cancel, |copied| {
            progress(Progress {
                phase: transfer.phase,
                copied_bytes: copied,
                total_bytes: total,
            })
        })?;
        if transfer.sha256.is_some_and(|expected| checksum != expected) {
            return Err("Model download failed its SHA-256 integrity check. Your previous model is unchanged; try downloading again.".into());
        }
        output
            .sync_all()
            .map_err(|e| format!("Cannot flush model copy: {e}"))?;
        drop(output);
        cancelled(cancel)?;
        model::inspect(&destination)?;
        progress(Progress {
            phase: "validating",
            copied_bytes: total,
            total_bytes: total,
        });
        cancelled(cancel)?;
        validate(&destination)?;
        cancelled(cancel)?;
        sync_directory(&pending)?;
        fs::rename(&pending, directory.join(format!("generation-{next:020}")))
            .map_err(|e| format!("Cannot activate model: {e}"))?;
        sync_directory(&directory)?;
        prune_old(root);
        progress(Progress {
            phase: "complete",
            copied_bytes: total,
            total_bytes: total,
        });
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
    use std::cell::RefCell;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("golos-install-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn model(&self) -> PathBuf {
            let path = self.0.join("source.bin");
            let mut header = [0u32; 12];
            header[0] = 0x67676d6c;
            header[1] = 51865;
            header[5] = 4;
            header[9] = 4;
            let mut file = File::create(&path).unwrap();
            for word in header {
                file.write_all(&word.to_le_bytes()).unwrap();
            }
            file.set_len((CHUNK_BYTES * 2 + 48) as u64).unwrap();
            path
        }
        fn install(
            &self,
            source: &Path,
            cancel: &AtomicBool,
            progress: impl FnMut(Progress),
            validate: impl FnOnce(&Path) -> Result<(), String>,
        ) -> Result<(), String> {
            install_with(&self.0, source, cancel, progress, validate, |_| {
                Ok(u64::MAX)
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn replacement_is_published_only_after_validation_and_legacy_is_preserved() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        let legacy = f.0.join("model.bin");
        fs::copy(&source, &legacy).unwrap();
        assert_eq!(active(&f.0).unwrap(), legacy);
        f.install(
            &source,
            &flag,
            |_| {},
            |_| {
                assert_eq!(active(&f.0).unwrap(), legacy);
                Ok(())
            },
        )
        .unwrap();
        let first = active(&f.0).unwrap();
        assert_ne!(first, legacy);
        assert!(legacy.exists());
        f.install(&source, &flag, |_| {}, |_| Err("bad weights".into()))
            .unwrap_err();
        assert_eq!(active(&f.0).unwrap(), first);
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        assert_ne!(active(&f.0).unwrap(), first);
        assert!(first.exists());
        recover(&f.0).unwrap();
        assert!(active(&f.0).unwrap().exists());
    }

    #[test]
    fn checks_space_before_copying_and_reports_progress() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        let error = install_with(
            &f.0,
            &source,
            &flag,
            |_| panic!("must not copy"),
            |_| panic!("must not load"),
            |_| Ok(0),
        )
        .unwrap_err();
        assert!(error.contains("free disk space"));
        assert!(generations(&f.0).unwrap().is_empty());
        let events = RefCell::new(vec![]);
        f.install(&source, &flag, |p| events.borrow_mut().push(p), |_| Ok(()))
            .unwrap();
        let events = events.borrow();
        assert_eq!(events[0].copied_bytes, 0);
        assert_eq!(events.last().unwrap().phase, "complete");
        assert_eq!(
            events.last().unwrap().copied_bytes,
            fs::metadata(source).unwrap().len()
        );
        assert!(events
            .windows(2)
            .all(|pair| pair[0].copied_bytes <= pair[1].copied_bytes));
    }

    #[test]
    fn cancellation_during_copy_and_validation_retains_previous_model() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        let old = active(&f.0).unwrap();
        f.install(
            &source,
            &flag,
            |p| {
                if p.copied_bytes > 0 {
                    flag.store(true, Ordering::SeqCst);
                }
            },
            |_| panic!("cancelled before loading"),
        )
        .unwrap_err();
        assert_eq!(active(&f.0).unwrap(), old);
        flag.store(false, Ordering::SeqCst);
        f.install(
            &source,
            &flag,
            |_| {},
            |_| {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(active(&f.0).unwrap(), old);
        assert_eq!(fs::read_dir(f.0.join("whisper-models")).unwrap().count(), 1);
    }

    #[test]
    fn interrupted_staging_is_cleaned_without_touching_unowned_files() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        let old = active(&f.0).unwrap();
        let dir = f.0.join("whisper-models");
        let pending = dir.join(format!("pending-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&pending).unwrap();
        fs::write(pending.join("model.bin"), b"partial").unwrap();
        fs::create_dir(dir.join("pending-user-files")).unwrap();
        recover(&f.0).unwrap();
        recover(&f.0).unwrap();
        assert!(!pending.exists());
        assert!(dir.join("pending-user-files").exists());
        assert_eq!(active(&f.0).unwrap(), old);
    }

    #[test]
    fn successful_replacements_keep_only_current_and_previous_generations() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        let first = active(&f.0).unwrap();
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        let second = active(&f.0).unwrap();
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        assert!(!first.exists());
        assert!(second.exists());
        assert_eq!(generations(&f.0).unwrap().len(), 2);
    }

    #[test]
    fn source_changes_never_publish() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        let error = f
            .install(
                &source,
                &flag,
                |p| {
                    if p.phase == "copying" && p.copied_bytes == 0 {
                        File::create(&source).unwrap();
                    }
                },
                |_| panic!("must not validate"),
            )
            .unwrap_err();
        assert!(error.contains("changed"));
        assert!(generations(&f.0).unwrap().is_empty());
    }

    #[test]
    fn stream_write_and_network_failures_are_reported() {
        struct FullDisk;
        impl Write for FullDisk {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let flag = AtomicBool::new(false);
        let error = copy_stream(&mut &b"model"[..], &mut FullDisk, 5, &flag, |_| {}).unwrap_err();
        assert!(error.contains("disk full"));
        struct BrokenConnection;
        impl Read for BrokenConnection {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "connection stopped",
                ))
            }
        }
        let f = Fixture::new();
        let source = f.model();
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        let old = active(&f.0).unwrap();
        let error = install_transfer(
            &f.0,
            Transfer {
                reader: BrokenConnection,
                total: 10,
                sha256: None,
                phase: "downloading",
            },
            &flag,
            |_| {},
            |_| panic!("must not validate"),
            |_| Ok(u64::MAX),
        )
        .unwrap_err();
        assert!(error.contains("connection stopped"));
        assert_eq!(active(&f.0).unwrap(), old);
        assert_eq!(fs::read_dir(f.0.join("whisper-models")).unwrap().count(), 1);
    }

    #[test]
    fn downloads_require_exact_length_and_checksum_before_loading() {
        let f = Fixture::new();
        let source = f.model();
        let flag = AtomicBool::new(false);
        f.install(&source, &flag, |_| {}, |_| Ok(())).unwrap();
        let old = active(&f.0).unwrap();
        let bytes = fs::read(source).unwrap();
        let checksum = format!("{:x}", Sha256::digest(&bytes));
        for (total, hash) in [
            (bytes.len() as u64 - 1, checksum.clone()),
            (bytes.len() as u64 + 1, checksum.clone()),
            (bytes.len() as u64, "0".repeat(64)),
        ] {
            install_transfer(
                &f.0,
                Transfer {
                    reader: bytes.as_slice(),
                    total,
                    sha256: Some(hash),
                    phase: "downloading",
                },
                &flag,
                |_| {},
                |_| panic!("invalid transfer must not be loaded"),
                |_| Ok(u64::MAX),
            )
            .unwrap_err();
            assert_eq!(active(&f.0).unwrap(), old);
        }
        install_transfer(
            &f.0,
            Transfer {
                reader: bytes.as_slice(),
                total: bytes.len() as u64,
                sha256: Some(checksum),
                phase: "downloading",
            },
            &flag,
            |_| {},
            |_| Ok(()),
            |_| Ok(u64::MAX),
        )
        .unwrap();
        assert_ne!(active(&f.0).unwrap(), old);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_and_discovery_do_not_follow_symlinks() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let source = f.model();
        let dir = directory(&f.0).unwrap();
        let external = f.0.join("external");
        fs::create_dir(&external).unwrap();
        fs::copy(&source, external.join("model.bin")).unwrap();
        symlink(&external, dir.join("generation-00000000000000000099")).unwrap();
        symlink(
            &external,
            dir.join(format!("pending-{}", uuid::Uuid::new_v4())),
        )
        .unwrap();
        recover(&f.0).unwrap();
        assert!(external.join("model.bin").exists());
        assert_eq!(active(&f.0).unwrap(), f.0.join("model.bin"));
    }
}
