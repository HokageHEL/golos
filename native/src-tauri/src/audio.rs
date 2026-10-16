use std::path::Path;

pub const MAX_SECONDS: u32 = 30 * 60;

pub fn read_audio(path: &Path) -> Result<Vec<f32>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("Cannot read WAV: {e}"))?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != 16_000
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(
            "Choose a 16 kHz, mono, 16-bit PCM WAV file. More audio formats are coming.".into(),
        );
    }
    if reader.duration() == 0 || reader.duration() > MAX_SECONDS * 16_000 {
        return Err("Choose a non-empty recording of up to 30 minutes.".into());
    }
    reader
        .samples::<i16>()
        .map(|s| {
            s.map(|s| s as f32 / 32768.0)
                .map_err(|e| format!("Damaged WAV data: {e}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wav(rate: u32, samples: &[i16]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("golos-{}.wav", uuid::Uuid::new_v4()));
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 1,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for sample in samples {
            writer.write_sample(*sample).unwrap();
        }
        writer.finalize().unwrap();
        path
    }
    #[test]
    fn decodes_and_normalizes_pcm() {
        let path = wav(16_000, &[i16::MIN, 0, i16::MAX]);
        let decoded = read_audio(&path).unwrap();
        assert_eq!(decoded[0], -1.0);
        assert_eq!(decoded[1], 0.0);
        assert!(decoded[2] < 1.0);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_unsupported_and_empty_audio() {
        for (rate, samples) in [(44_100, vec![1]), (16_000, vec![])] {
            let path = wav(rate, &samples);
            assert!(read_audio(&path).is_err());
            std::fs::remove_file(path).unwrap();
        }
    }
}
