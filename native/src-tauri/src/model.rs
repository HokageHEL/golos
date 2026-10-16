use std::{fs::File, io::Read, path::Path};

/// Bound the engine to Tiny, Base or Small before loading weights into memory.
/// Prefer multilingual small-q5_1; quantized models reduce disk/download size,
/// but runtime working buffers still require additional memory.
pub fn inspect(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|_| "Import a Whisper Small, Base or Tiny GGML model first.".to_string())?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if !(1_000_000..=512 * 1024 * 1024).contains(&size) {
        return Err("Use a Whisper Small, Base or Tiny model up to 512 MiB. Recommended: ggml-small-q5_1.bin.".into());
    }
    let mut header = [0u8; 24];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let word = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
    if word(0) != 0x67676d6c {
        return Err("This is not a Whisper GGML model.".into());
    }
    let family = match word(20) {
        4 => "Tiny", 6 => "Base", 12 => "Small",
        _ => return Err("Golos uses lightweight Whisper models: Small, Base or Tiny. Medium and Large are not supported.".into()),
    };
    Ok(format!(
        "Whisper {family}{}",
        if word(4) == 51864 {
            " · English only"
        } else {
            " · multilingual"
        }
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom, Write};
    #[test]
    fn accepts_small_and_rejects_large_before_loading_weights() {
        let path = std::env::temp_dir().join(format!("golos-model-{}", uuid::Uuid::new_v4()));
        let mut file = File::create(&path).unwrap();
        file.set_len(1_000_000).unwrap();
        file.write_all(&0x67676d6cu32.to_le_bytes()).unwrap();
        file.write_all(&51865u32.to_le_bytes()).unwrap();
        file.seek(SeekFrom::Start(20)).unwrap();
        file.write_all(&12u32.to_le_bytes()).unwrap();
        assert_eq!(inspect(&path).unwrap(), "Whisper Small · multilingual");
        file.seek(SeekFrom::Start(20)).unwrap();
        file.write_all(&32u32.to_le_bytes()).unwrap();
        assert!(inspect(&path).unwrap_err().contains("Medium and Large"));
        file.set_len(600 * 1024 * 1024).unwrap();
        assert!(inspect(&path).unwrap_err().contains("512 MiB"));
        drop(file);
        std::fs::remove_file(path).unwrap();
    }
}
