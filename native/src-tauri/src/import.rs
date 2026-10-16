//! Decode one packet at a time, downmix, and resample into the library's canonical
//! WAV format. Never buffer the full source recording or ship an external decoder.
use crate::audio::MAX_SECONDS;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use std::{
    fs::{self, File},
    io::{BufWriter, ErrorKind},
    path::Path,
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

pub const EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "mp4", "aac", "flac", "ogg", "oga", "aif", "aiff",
];
pub const MAX_BYTES: u64 = 512 * 1024 * 1024;
const CHUNK: usize = 1024;
const OUTPUT_RATE: u32 = 16_000;
type Writer = hound::WavWriter<BufWriter<File>>;

struct Normalizer {
    rate: u32,
    channels: usize,
    resampler: Option<SincFixedIn<f32>>,
    pending: Vec<f32>,
    delay: usize,
    input_frames: u64,
    output_frames: u64,
    output: Writer,
}

impl Normalizer {
    fn new(rate: u32, channels: usize, output: Writer) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&rate) || !(1..=8).contains(&channels) {
            return Err(
                "Supported audio has 1–8 channels and a sample rate between 8 and 192 kHz.".into(),
            );
        }
        let resampler = if rate == OUTPUT_RATE {
            None
        } else {
            Some(
                SincFixedIn::new(
                    OUTPUT_RATE as f64 / rate as f64,
                    1.0,
                    SincInterpolationParameters {
                        sinc_len: 128,
                        f_cutoff: 0.95,
                        interpolation: SincInterpolationType::Linear,
                        oversampling_factor: 128,
                        window: WindowFunction::BlackmanHarris2,
                    },
                    CHUNK,
                    1,
                )
                .map_err(|e| e.to_string())?,
            )
        };
        let delay = resampler.as_ref().map(|r| r.output_delay()).unwrap_or(0);
        Ok(Self {
            rate,
            channels,
            resampler,
            pending: Vec::with_capacity(CHUNK),
            delay,
            input_frames: 0,
            output_frames: 0,
            output,
        })
    }

    fn write(&mut self, samples: &[f32], limit: u64) -> Result<(), String> {
        for sample in samples {
            if self.delay > 0 {
                self.delay -= 1;
                continue;
            }
            if self.output_frames >= limit {
                break;
            }
            if !sample.is_finite() {
                return Err("Audio contains invalid samples.".into());
            }
            let pcm = (sample.clamp(-1.0, 1.0) * 32768.0)
                .round()
                .clamp(-32768.0, 32767.0) as i16;
            self.output
                .write_sample(pcm)
                .map_err(|e| format!("Cannot save decoded audio: {e}"))?;
            self.output_frames += 1;
        }
        Ok(())
    }

    fn process_chunk(&mut self, limit: u64) -> Result<(), String> {
        let output = self
            .resampler
            .as_mut()
            .ok_or("Resampler unavailable")?
            .process(&[self.pending.as_slice()], None)
            .map_err(|e| e.to_string())?;
        self.pending.clear();
        self.write(&output[0], limit)
    }

    fn push(&mut self, samples: &[f32]) -> Result<(), String> {
        if !samples.len().is_multiple_of(self.channels) {
            return Err("Incomplete audio frame.".into());
        }
        let frames = (samples.len() / self.channels) as u64;
        if self.input_frames + frames > self.rate as u64 * MAX_SECONDS as u64 {
            return Err("Recordings must be no longer than 30 minutes.".into());
        }
        self.input_frames += frames;
        for frame in samples.chunks_exact(self.channels) {
            if frame.iter().any(|s| !s.is_finite()) {
                return Err("Audio contains invalid samples.".into());
            }
            let mono = frame.iter().map(|s| s.clamp(-1.0, 1.0)).sum::<f32>() / self.channels as f32;
            if self.resampler.is_none() {
                self.write(&[mono], u64::MAX)?;
            } else {
                self.pending.push(mono);
                if self.pending.len() == CHUNK {
                    self.process_chunk(u64::MAX)?;
                }
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<f64, String> {
        let expected = self.input_frames * OUTPUT_RATE as u64 / self.rate as u64;
        if expected == 0 {
            return Err("No audio was found in this file.".into());
        }
        if self.resampler.is_some() {
            // Pad only the final block, then flush the sinc filter's delayed tail.
            // Clamp output to actual source duration so padding adds no silence.
            for _ in 0..4 {
                if self.output_frames >= expected {
                    break;
                }
                self.pending.resize(CHUNK, 0.0);
                self.process_chunk(expected)?;
            }
        }
        if self.output_frames != expected {
            return Err("Could not finish resampling this recording.".into());
        }
        self.output
            .finalize()
            .map_err(|e| format!("Cannot finish saving audio: {e}"))?;
        Ok(expected as f64 / OUTPUT_RATE as f64)
    }
}

/// Only a complete, validated WAV is retained. Partial output is removed on every
/// error; the original user-selected recording is never modified or copied whole.
pub fn normalize(source: &Path, target: &Path) -> Result<f64, String> {
    let result = normalize_inner(source, target);
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}

fn normalize_inner(source: &Path, target: &Path) -> Result<f64, String> {
    let file = File::open(source).map_err(|e| format!("Cannot open recording: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_BYTES {
        return Err("Choose a non-empty audio file up to 512 MiB.".into());
    }
    let mut hint = Hint::new();
    if let Some(extension) = source.extension().and_then(|e| e.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe().format(&hint,
        MediaSourceStream::new(Box::new(file), Default::default()),
        &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|e| format!("Cannot read this audio format: {e}. Try WAV, MP3, M4A/AAC, FLAC, Ogg/Vorbis or AIFF."))?.format;
    let (track_id, mut decoder) = format.tracks().iter().find_map(|track|
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())
            .ok().map(|decoder| (track.id, decoder)))
        .ok_or("No supported audio track found. M4A needs AAC-LC or ALAC; Ogg needs Vorbis. Opus, DRM and video-only files are not supported.")?;
    let mut normalizer: Option<Normalizer> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(format!("Cannot read audio packets: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Cannot decode audio: {e}"))?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if decoded.frames() == 0 {
            continue;
        }
        // Reject implausibly large packets before allocating the interleaved buffer.
        if decoded.capacity() > spec.rate as usize * 10 {
            return Err("Audio packet is too large.".into());
        }
        if normalizer.is_none() {
            let output = hound::WavWriter::create(
                target,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: OUTPUT_RATE,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .map_err(|e| e.to_string())?;
            normalizer = Some(Normalizer::new(spec.rate, channels, output)?);
        }
        let normalizer = normalizer.as_mut().ok_or("No audio found")?;
        if normalizer.rate != spec.rate || normalizer.channels != channels {
            return Err(
                "Audio that changes sample rate or channel layout mid-file is not supported."
                    .into(),
            );
        }
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        normalizer.push(buffer.samples())?;
    }
    if decoder.finalize().verify_ok == Some(false) {
        return Err("Audio integrity check failed.".into());
    }
    normalizer
        .ok_or("No audio was found in this file.")?
        .finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        source: std::path::PathBuf,
        output: std::path::PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let stem = std::env::temp_dir().join(format!("golos-audio-{}", uuid::Uuid::new_v4()));
            Self {
                source: stem.with_extension("wav"),
                output: stem.with_extension("normalized.wav"),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.source);
            let _ = fs::remove_file(&self.output);
        }
    }
    fn float_wav(path: &Path, rate: u32, channels: u16, frames: usize, frequency: f32) {
        let mut writer = hound::WavWriter::create(
            path,
            hound::WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for frame in 0..frames {
            let sample =
                (std::f32::consts::TAU * frequency * frame as f32 / rate as f32).sin() * 0.5;
            for _ in 0..channels {
                writer.write_sample(sample).unwrap();
            }
        }
        writer.finalize().unwrap();
    }
    #[test]
    fn accepts_stereo_float_wav_at_common_sample_rates_without_padding_duration() {
        for rate in [8_000, 16_000, 22_050, 44_100, 48_000, 96_000, 192_000] {
            let fixture = Fixture::new();
            float_wav(&fixture.source, rate, 2, rate as usize / 2, 1000.0);
            assert_eq!(normalize(&fixture.source, &fixture.output).unwrap(), 0.5);
            let samples = crate::audio::read_audio(&fixture.output).unwrap();
            assert_eq!(samples.len(), 8000);
            let rms = (samples[500..7500].iter().map(|s| s * s).sum::<f32>() / 7000.0).sqrt();
            assert!(
                (0.33..0.37).contains(&rms),
                "Unexpected speech-band amplitude at {rate}: {rms}"
            );
        }
    }
    #[test]
    fn downmixes_channels_and_rejects_nonfinite_samples() {
        let fixture = Fixture::new();
        let output = hound::WavWriter::create(
            &fixture.output,
            hound::WavSpec {
                channels: 1,
                sample_rate: OUTPUT_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        let mut normalizer = Normalizer::new(16_000, 2, output).unwrap();
        normalizer.push(&[0.5, -0.25, 0.5, -0.25]).unwrap();
        normalizer.finish().unwrap();
        assert_eq!(
            crate::audio::read_audio(&fixture.output).unwrap(),
            vec![0.125, 0.125]
        );
        float_wav(&fixture.source, 16_000, 1, 10, 1000.0);
        let mut bytes = fs::read(&fixture.source).unwrap();
        let end = bytes.len();
        bytes[end - 4..].copy_from_slice(&f32::NAN.to_le_bytes());
        fs::write(&fixture.source, bytes).unwrap();
        assert!(normalize(&fixture.source, &fixture.output).is_err());
        assert!(
            !fixture.output.exists(),
            "Failed imports must not leave audio behind"
        );
    }
    #[test]
    fn suppresses_frequencies_that_would_alias_into_speech_band() {
        let fixture = Fixture::new();
        float_wav(&fixture.source, 48_000, 1, 48_000, 12_000.0);
        normalize(&fixture.source, &fixture.output).unwrap();
        let samples = crate::audio::read_audio(&fixture.output).unwrap();
        let rms = (samples[1000..15000].iter().map(|s| s * s).sum::<f32>() / 14000.0).sqrt();
        assert!(rms < 0.01, "Downsampling aliased a 12 kHz tone: {rms}");
    }
    #[test]
    fn rejects_empty_corrupt_and_oversized_files_and_cleans_up() {
        let fixture = Fixture::new();
        for bytes in [&b""[..], &b"not an audio file"[..]] {
            fs::write(&fixture.source, bytes).unwrap();
            assert!(normalize(&fixture.source, &fixture.output).is_err());
            assert!(!fixture.output.exists());
        }
        let file = File::create(&fixture.source).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(normalize(&fixture.source, &fixture.output)
            .unwrap_err()
            .contains("512 MiB"));
    }
    #[test]
    fn rejects_duration_over_limit_without_truncating() {
        let fixture = Fixture::new();
        let output = hound::WavWriter::create(
            &fixture.output,
            hound::WavSpec {
                channels: 1,
                sample_rate: OUTPUT_RATE,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        let mut normalizer = Normalizer::new(16_000, 1, output).unwrap();
        normalizer.input_frames = MAX_SECONDS as u64 * OUTPUT_RATE as u64;
        assert!(normalizer.push(&[0.1]).unwrap_err().contains("30 minutes"));
    }
    #[test]
    #[ignore = "Set GOLOS_TEST_FORMATS to the folder generated by scripts/create-audio-fixtures.py"]
    fn decodes_real_compressed_formats() {
        let root = std::path::PathBuf::from(
            std::env::var("GOLOS_TEST_FORMATS").expect("GOLOS_TEST_FORMATS required"),
        );
        for name in [
            "speech.mp3",
            "speech-aac.m4a",
            "speech-alac.m4a",
            "speech.aac",
            "speech.flac",
            "speech.ogg",
            "speech.aiff",
            "speech-24bit.wav",
        ] {
            let fixture = Fixture::new();
            let original = fs::read(root.join(name)).unwrap();
            let duration = normalize(&root.join(name), &fixture.output)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(
                (10.0..12.0).contains(&duration),
                "{name}: duration {duration}"
            );
            let samples = crate::audio::read_audio(&fixture.output).unwrap();
            assert!(
                samples.iter().any(|s| s.abs() > 0.1),
                "{name} became silent"
            );
            assert_eq!(
                fs::read(root.join(name)).unwrap(),
                original,
                "Import modified {name}"
            );
        }
        let fixture = Fixture::new();
        assert!(normalize(&root.join("unsupported-opus.ogg"), &fixture.output).is_err());
        assert!(!fixture.output.exists());
    }
}
