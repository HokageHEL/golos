use std::{fs::File, io::Read, path::Path};

/// GGML file magic: the bytes `ggml` read as a little-endian `u32`.
const GGML_MAGIC: u32 = 0x67676d6c;
/// Magic word plus the eleven `whisper_hparams` fields that follow it.
const HEADER_BYTES: usize = 48;
/// English-only Whisper vocabularies hold 51,864 tokens; multilingual ones hold
/// 51,865, and large-v3/turbo add one more (51,866).
const VOCAB_ENGLISH_ONLY: u32 = 51_864;
const VOCAB_MULTILINGUAL_V3: u32 = 51_866;

/// Identify a whisper.cpp GGML model from its header and return a label.
///
/// Golos no longer caps the file size or the model family: any standard Whisper
/// architecture that whisper.cpp can load is accepted, including quantized
/// Medium, Large, and Large V3 Turbo models. This is a cheap structural check
/// only, so `import_model` still loads the copied file once before replacing a
/// working model. Nothing here asserts that a model transcribes well.
pub fn inspect(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|_| "Import a Whisper GGML model (.bin) first.".to_string())?;
    let mut header = [0u8; HEADER_BYTES];
    file.read_exact(&mut header)
        .map_err(|_| "This file is too small to be a Whisper GGML model.".to_string())?;
    let word = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
    if word(0) != GGML_MAGIC {
        return Err("This is not a Whisper GGML model.".into());
    }
    // Hyperparameters are stored in file order after the magic; only the fields
    // needed to recognize the architecture are read.
    let n_vocab = word(4);
    let n_audio_layer = word(20);
    let n_text_layer = word(36);
    // Encoder depth names the family. large-v3-turbo keeps the 32-layer encoder
    // but distills the decoder to 4 layers, whereas large v1/v2/v3 keep all 32;
    // this mirrors whisper.cpp's own header parsing (`whisper_model_load`). The
    // label is derived from the real header, never from the file name.
    let family = match n_audio_layer {
        4 => "Tiny",
        6 => "Base",
        12 => "Small",
        24 => "Medium",
        32 if n_text_layer == 4 => "Large V3 Turbo",
        32 if n_vocab == VOCAB_MULTILINGUAL_V3 => "Large V3",
        32 => "Large",
        _ => {
            return Err(
                "Unsupported Whisper model architecture. Import a standard whisper.cpp GGML \
                 model (Tiny, Base, Small, Medium, Large, or Large V3 Turbo)."
                    .into(),
            )
        }
    };
    Ok(format!(
        "Whisper {family}{}",
        if n_vocab == VOCAB_ENGLISH_ONLY {
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

    /// The eleven GGML hyperparameters that follow the magic word, in file order.
    /// Only `n_vocab`, `n_audio_layer`, and `n_text_layer` affect identification,
    /// but the others carry representative values so the fixtures look like real
    /// model headers.
    struct Hparams {
        n_vocab: u32,
        n_audio_ctx: u32,
        n_audio_state: u32,
        n_audio_head: u32,
        n_audio_layer: u32,
        n_text_ctx: u32,
        n_text_state: u32,
        n_text_head: u32,
        n_text_layer: u32,
        n_mels: u32,
    }

    impl Hparams {
        fn words(&self) -> [u32; 12] {
            [
                GGML_MAGIC,
                self.n_vocab,
                self.n_audio_ctx,
                self.n_audio_state,
                self.n_audio_head,
                self.n_audio_layer,
                self.n_text_ctx,
                self.n_text_state,
                self.n_text_head,
                self.n_text_layer,
                self.n_mels,
                1, // ftype: f16
            ]
        }
    }

    fn tiny_english() -> Hparams {
        Hparams {
            n_vocab: VOCAB_ENGLISH_ONLY,
            n_audio_ctx: 1500,
            n_audio_state: 384,
            n_audio_head: 6,
            n_audio_layer: 4,
            n_text_ctx: 448,
            n_text_state: 384,
            n_text_head: 6,
            n_text_layer: 4,
            n_mels: 80,
        }
    }

    fn medium() -> Hparams {
        Hparams {
            n_vocab: 51_865,
            n_audio_ctx: 1500,
            n_audio_state: 1024,
            n_audio_head: 16,
            n_audio_layer: 24,
            n_text_ctx: 448,
            n_text_state: 1024,
            n_text_head: 16,
            n_text_layer: 24,
            n_mels: 80,
        }
    }

    fn large_v3() -> Hparams {
        Hparams {
            n_vocab: 51_866,
            n_audio_ctx: 1500,
            n_audio_state: 1280,
            n_audio_head: 20,
            n_audio_layer: 32,
            n_text_ctx: 448,
            n_text_state: 1280,
            n_text_head: 20,
            n_text_layer: 32,
            n_mels: 128,
        }
    }

    /// large-v3-turbo: the same encoder and vocabulary as large-v3, but a
    /// distilled 4-layer decoder.
    fn large_v3_turbo() -> Hparams {
        Hparams {
            n_text_layer: 4,
            ..large_v3()
        }
    }

    /// A temporary GGML file removed on drop.
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn path(&self) -> &Path {
            &self.0
        }

        /// A sparse file of `len` bytes holding `words` at the start. `set_len`
        /// reserves the size as a hole, so a several-hundred-MiB fixture costs no
        /// large buffer and no meaningful disk space.
        fn sparse(words: &[u32], len: u64) -> Self {
            let path = std::env::temp_dir().join(format!("golos-model-{}", uuid::Uuid::new_v4()));
            let mut file = File::create(&path).unwrap();
            file.set_len(len).unwrap();
            file.seek(SeekFrom::Start(0)).unwrap();
            for word in words {
                file.write_all(&word.to_le_bytes()).unwrap();
            }
            file.sync_all().unwrap();
            Self(path)
        }

        /// A short file holding exactly `words`, for truncation/corruption cases.
        fn short(words: &[u32]) -> Self {
            let path = std::env::temp_dir().join(format!("golos-model-{}", uuid::Uuid::new_v4()));
            let mut file = File::create(&path).unwrap();
            for word in words {
                file.write_all(&word.to_le_bytes()).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn accepts_sparse_models_larger_than_the_old_512_mib_ceiling() {
        // A sparse 600 MiB file: the previous build rejected anything above
        // 512 MiB, and large-v3-turbo q5_0 is larger than that on disk.
        let fixture = Fixture::sparse(&large_v3_turbo().words(), 600 * 1024 * 1024);
        assert_eq!(
            inspect(fixture.path()).unwrap(),
            "Whisper Large V3 Turbo · multilingual"
        );
        // A full-precision large model is much larger still and must pass the
        // header check as well (it is not the engine's job here to bound memory).
        let fixture = Fixture::sparse(&large_v3().words(), 3 * 1024 * 1024 * 1024);
        assert_eq!(
            inspect(fixture.path()).unwrap(),
            "Whisper Large V3 · multilingual"
        );
    }

    #[test]
    fn labels_medium_large_and_turbo_from_header_architecture() {
        let medium = Fixture::sparse(&medium().words(), 1024);
        assert_eq!(
            inspect(medium.path()).unwrap(),
            "Whisper Medium · multilingual"
        );
        let turbo = Fixture::sparse(&large_v3_turbo().words(), 1024);
        assert_eq!(
            inspect(turbo.path()).unwrap(),
            "Whisper Large V3 Turbo · multilingual"
        );
        // Turbo and Large share the 32-layer encoder; only the decoder depth
        // distinguishes them, so the label must come from the header and not
        // from the (always `model.bin`) file name.
        assert_ne!(
            inspect(turbo.path()).unwrap(),
            inspect(Fixture::sparse(&large_v3().words(), 1024).path()).unwrap()
        );
    }

    #[test]
    fn keeps_tiny_base_small_and_english_only_detection() {
        assert_eq!(
            inspect(Fixture::sparse(&tiny_english().words(), 1024).path()).unwrap(),
            "Whisper Tiny · English only"
        );
        let base = Hparams {
            n_vocab: 51_865,
            n_audio_state: 512,
            n_audio_head: 8,
            n_audio_layer: 6,
            n_text_state: 512,
            n_text_head: 8,
            n_text_layer: 6,
            ..tiny_english()
        };
        assert_eq!(
            inspect(Fixture::sparse(&base.words(), 1024).path()).unwrap(),
            "Whisper Base · multilingual"
        );
        let small = Hparams {
            n_vocab: 51_865,
            n_audio_state: 768,
            n_audio_head: 12,
            n_audio_layer: 12,
            n_text_state: 768,
            n_text_head: 12,
            n_text_layer: 12,
            ..tiny_english()
        };
        assert_eq!(
            inspect(Fixture::sparse(&small.words(), 1024).path()).unwrap(),
            "Whisper Small · multilingual"
        );
    }

    #[test]
    fn rejects_bad_magic_truncated_header_and_unknown_architecture() {
        let mut words = large_v3().words();
        words[0] = 0x1234_5678;
        assert!(inspect(Fixture::sparse(&words, 1024).path())
            .unwrap_err()
            .contains("not a Whisper GGML model"));

        // Fewer than 48 bytes: the file cannot even hold the header.
        assert!(inspect(Fixture::short(&large_v3().words()[..3]).path())
            .unwrap_err()
            .contains("too small"));

        // A recognized magic with an encoder depth outside the Whisper family is
        // refused instead of being mislabelled.
        let mut words = large_v3().words();
        words[5] = 99; // n_audio_layer
        assert!(inspect(Fixture::sparse(&words, 1024).path())
            .unwrap_err()
            .contains("Unsupported Whisper model architecture"));
    }
}
