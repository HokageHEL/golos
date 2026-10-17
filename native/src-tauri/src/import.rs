//! Decode one packet at a time, downmix, and resample into the library's canonical
//! WAV format. Never buffer the full source recording or ship an external decoder.
use crate::audio::MAX_SECONDS;
use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};
use rusty_opus::OpusDecoder as OpusStream;
use std::{
    fs::{self, File},
    io::{BufWriter, ErrorKind},
    path::Path,
    sync::OnceLock,
};
use symphonia::core::{
    audio::{AsAudioBufferRef, AudioBuffer, AudioBufferRef, SampleBuffer, Signal, SignalSpec},
    codecs::{
        CodecDescriptor, CodecParameters, CodecRegistry, Decoder, DecoderOptions, FinalizeResult,
        CODEC_TYPE_OPUS,
    },
    errors::{decode_error, unsupported_error, Error, Result as SymphoniaResult},
    formats::{FormatOptions, Packet},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

pub const EXTENSIONS: &[&str] = &[
    "wav", "mp3", "m4a", "mp4", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff",
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

/// Largest Opus frame per channel: 120 ms at 48 kHz (RFC 6716 §3.2.5). It is
/// also the fixed output capacity of the decoder below, matching libopus.
const OPUS_MAX_FRAME_SAMPLES: usize = 5_760;

/// Parsed fields of an Ogg Opus identification header (`OpusHead`, RFC 7845 §5.1).
struct OpusHead {
    /// Encapsulation version; the high nibble is the incompatible major version.
    version: u8,
    channels: usize,
    /// Samples at 48 kHz the decoder must discard from the start (RFC 7845 §4.2).
    pre_skip: usize,
    /// Signed Q7.8 dB gain applied to decoded samples (RFC 7845 §5.1).
    gain: i16,
    /// Channel mapping family; only family 0 (plain mono/stereo) is supported.
    family: u8,
}

fn parse_opus_head(buf: &[u8]) -> Option<OpusHead> {
    if buf.len() < 19 || &buf[..8] != b"OpusHead" {
        return None;
    }
    Some(OpusHead {
        version: buf[8],
        channels: buf[9] as usize,
        pre_skip: u16::from_le_bytes([buf[10], buf[11]]) as usize,
        gain: i16::from_le_bytes([buf[16], buf[17]]),
        family: buf[18],
    })
}

/// Symphonia `Decoder` for Ogg Opus. Symphonia's Ogg demuxer parses the
/// identification header and granule positions (including gapless start/end
/// trims) but ships no Opus decoder, so this adapter supplies one backed by the
/// pure-Rust `rusty-opus` crate: no FFI, no bundled C library and no external
/// tool at build or run time.
///
/// Opus always decodes at 48 kHz here and is resampled to the library's 16 kHz
/// output by [`Normalizer`], reusing the existing anti-aliasing path. Symphonia
/// cannot express the OpusHead pre-skip through packet trims, so the adapter
/// discards it explicitly while also honoring Symphonia's per-packet trims, and
/// it applies the OpusHead output gain (RFC 7845 §5.1) that the demuxer drops.
struct OpusDecoderAdapter {
    stream: OpusStream,
    params: CodecParameters,
    buffer: AudioBuffer<f32>,
    scratch: Vec<f32>,
    channels: usize,
    /// Linear amplitude scale from the OpusHead output gain; 1.0 for 0 dB.
    gain: f32,
    initial_pre_skip: usize,
    pre_skip_remaining: usize,
}

impl OpusDecoderAdapter {
    fn head_of(params: &CodecParameters) -> SymphoniaResult<OpusHead> {
        match params.extra_data.as_deref().and_then(parse_opus_head) {
            Some(head) => Ok(head),
            None => unsupported_error("opus: invalid identification header"),
        }
    }
}

impl Decoder for OpusDecoderAdapter {
    fn try_new(params: &CodecParameters, _: &DecoderOptions) -> SymphoniaResult<Self> {
        if params.codec != CODEC_TYPE_OPUS {
            return unsupported_error("opus: invalid codec type");
        }
        let head = Self::head_of(params)?;
        // RFC 7845 §5.1: any version up to 15 is backwards compatible (the low
        // nibble is a minor version); a higher major version is not.
        if head.version > 15 {
            return unsupported_error("opus: unsupported identification header version");
        }
        // RFC 7845 §5.1.1: family 0 is a plain single stream mapped straight to
        // mono or stereo. Family 1 (even at one or two channels) and family 255
        // carry a channel mapping table this single-stream adapter does not
        // implement, so refuse them rather than decode channels that may be
        // misassigned. Multichannel family-1 streams are thereby rejected too.
        if head.family != 0 {
            return unsupported_error(
                "opus: only un-mapped mono or stereo Ogg Opus (channel mapping family 0) is supported",
            );
        }
        if head.channels == 0 || head.channels > 2 {
            return unsupported_error("opus: only mono and stereo Ogg Opus is supported");
        }
        let Some(layout) = params.channels else {
            return unsupported_error("opus: missing channel layout");
        };
        if layout.count() != head.channels {
            return unsupported_error("opus: inconsistent channel count");
        }
        let spec = SignalSpec::new(48_000, layout);
        let stream = match OpusStream::new(48_000, head.channels) {
            Ok(stream) => stream,
            Err(_) => return unsupported_error("opus: cannot initialize decoder"),
        };
        // RFC 7845 §5.1: the gain is 20*log10 of the linear scale factor, in
        // Q7.8, so the linear factor is 10^(gain / (20 * 256)).
        let gain = 10f32.powf(f32::from(head.gain) / (20.0 * 256.0));
        let scratch = vec![0f32; OPUS_MAX_FRAME_SAMPLES * head.channels];
        Ok(Self {
            stream,
            params: params.clone(),
            buffer: AudioBuffer::new(OPUS_MAX_FRAME_SAMPLES as u64, spec),
            scratch,
            channels: head.channels,
            gain,
            initial_pre_skip: head.pre_skip,
            pre_skip_remaining: head.pre_skip,
        })
    }

    fn supported_codecs() -> &'static [CodecDescriptor] {
        &[CodecDescriptor {
            codec: CODEC_TYPE_OPUS,
            short_name: "opus",
            long_name: "Opus",
            inst_func: |params, options| {
                Ok(Box::new(Self::try_new(params, options)?) as Box<dyn Decoder>)
            },
        }]
    }

    fn reset(&mut self) {
        // `rusty-opus` has no state reset; a fresh decoder is the cheapest way
        // to drop the inter-frame history, and it cannot fail for a rate that
        // was already accepted in `try_new`.
        if let Ok(stream) = OpusStream::new(48_000, self.channels) {
            self.stream = stream;
        }
        self.buffer.clear();
        self.pre_skip_remaining = self.initial_pre_skip;
    }

    fn codec_params(&self) -> &CodecParameters {
        &self.params
    }

    fn decode(&mut self, packet: &Packet) -> SymphoniaResult<AudioBufferRef<'_>> {
        // RFC 6716 §3.4: a zero-length audio packet is malformed. Never
        // substitute decoder-generated concealment audio for real input.
        if packet.buf().is_empty() {
            self.buffer.clear();
            return decode_error("opus: empty audio packet");
        }
        let decoded =
            match self
                .stream
                .decode(packet.buf(), OPUS_MAX_FRAME_SAMPLES, &mut self.scratch)
            {
                Ok(frames) => frames,
                Err(_) => {
                    // Decoders must leave no stale audio after a failed packet.
                    self.buffer.clear();
                    return decode_error("opus: cannot decode audio packet");
                }
            };
        let trim_start = packet.trim_start() as usize;
        let trim_end = packet.trim_end() as usize;
        if trim_start + trim_end > decoded {
            self.buffer.clear();
            return decode_error("opus: inconsistent packet trimming");
        }
        // Drop Symphonia's explicit frame trims, then the OpusHead pre-skip.
        let kept = decoded - trim_start - trim_end;
        let skip = self.pre_skip_remaining.min(kept);
        self.pre_skip_remaining -= skip;
        let front = trim_start + skip;
        let frames = kept - skip;
        self.buffer.clear();
        if frames > 0 {
            self.buffer.render_reserved(Some(frames));
            for channel in 0..self.channels {
                let samples = self.buffer.chan_mut(channel);
                for (frame, sample) in samples.iter_mut().enumerate() {
                    *sample = self.scratch[(front + frame) * self.channels + channel] * self.gain;
                }
            }
        }
        Ok(self.buffer.as_audio_buffer_ref())
    }

    fn finalize(&mut self) -> FinalizeResult {
        FinalizeResult::default()
    }

    fn last_decoded(&self) -> AudioBufferRef<'_> {
        self.buffer.as_audio_buffer_ref()
    }
}

/// Symphonia's default codecs plus this adapter's Opus decoder. Built once and
/// shared: the registry only holds descriptors, so it is thread-safe.
fn codec_registry() -> &'static CodecRegistry {
    static REGISTRY: OnceLock<CodecRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = CodecRegistry::new();
        symphonia::default::register_enabled_codecs(&mut registry);
        registry.register_all::<OpusDecoderAdapter>();
        registry
    })
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
        .map_err(|e| format!("Cannot read this audio format: {e}. Try WAV, MP3, M4A/AAC, FLAC, Ogg (Vorbis or Opus) or AIFF."))?.format;
    let registry = codec_registry();
    let (track_id, mut decoder) = format.tracks().iter().find_map(|track|
        registry.make(&track.codec_params, &DecoderOptions::default())
            .ok().map(|decoder| (track.id, decoder)))
        .ok_or_else(|| if format.tracks().iter().any(|track| track.codec_params.codec == CODEC_TYPE_OPUS) {
            "Ogg Opus is limited to mono or stereo channel mapping family 0. Multichannel or custom-mapped Opus is not supported.".to_string()
        } else {
            "No supported audio track found. M4A needs AAC-LC or ALAC; Ogg needs Vorbis or mono/stereo Opus. DRM and video-only files are not supported.".to_string()
        })?;
    let mut normalizer: Option<Normalizer> = None;
    loop {
        let packet =
            match format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(error)) if error.kind() == ErrorKind::UnexpectedEof => break,
                // A second logical stream (chained Ogg) would splice unrelated audio
                // onto the output; refuse it instead of producing a merged track.
                Err(Error::ResetRequired) => return Err(
                    "Chained Ogg streams are not supported. Import a single continuous recording."
                        .into(),
                ),
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
            Self::with_source_extension("wav")
        }
        fn with_source_extension(extension: &str) -> Self {
            let stem = std::env::temp_dir().join(format!("golos-audio-{}", uuid::Uuid::new_v4()));
            Self {
                source: stem.with_extension(extension),
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
    fn fixture_path(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }
    /// Standard Ogg CRC-32: polynomial 0x04c11db7, no reflection, zero init/xor.
    fn ogg_crc32(data: &[u8]) -> u32 {
        let mut crc: u32 = 0;
        for &byte in data {
            crc ^= u32::from(byte) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ 0x04c1_1db7
                } else {
                    crc << 1
                };
            }
        }
        crc
    }
    /// Recompute the CRC of the Ogg page starting at `page_start` in place.
    fn rechecksum_page(bytes: &mut [u8], page_start: usize) {
        let nseg = bytes[page_start + 26] as usize;
        let body: usize = bytes[page_start + 27..page_start + 27 + nseg]
            .iter()
            .map(|&segment| segment as usize)
            .sum();
        let end = page_start + 27 + nseg + body;
        bytes[page_start + 22..page_start + 26].fill(0);
        let crc = ogg_crc32(&bytes[page_start..end]);
        bytes[page_start + 22..page_start + 26].copy_from_slice(&crc.to_le_bytes());
    }
    fn page_start_for(bytes: &[u8], offset: usize) -> usize {
        (0..=offset)
            .rev()
            .find(|&i| bytes[i..].starts_with(b"OggS"))
            .expect("Ogg page start")
    }
    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }
    /// RMS of the middle half of a decoded clip, away from resampler edges.
    fn middle_rms(samples: &[f32]) -> f32 {
        rms(&samples[samples.len() / 4..samples.len() * 3 / 4])
    }
    /// Decode a committed fixture and assert the exact output duration and that
    /// real (non-silent) audio survived.
    fn decode_fixture(name: &str, expected_seconds: f64, min_rms: f32) -> Vec<f32> {
        let fixture = Fixture::new();
        let duration = normalize(&fixture_path(name), &fixture.output)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            (duration - expected_seconds).abs() < 1e-6,
            "{name}: duration {duration} != {expected_seconds}"
        );
        let samples = crate::audio::read_audio(&fixture.output).unwrap();
        assert_eq!(
            samples.len(),
            (expected_seconds * 16_000.0) as usize,
            "{name}: sample count"
        );
        let level = middle_rms(&samples);
        assert!(
            level > min_rms,
            "{name}: output is near-silent (rms {level})"
        );
        samples
    }
    #[test]
    fn decodes_real_ogg_opus_and_vorbis_fixtures() {
        decode_fixture("opus-stereo.ogg", 2.0, 0.2);
        decode_fixture("opus-mono.ogg", 1.5, 0.2);
        // Vorbis must keep working alongside the new Opus support.
        decode_fixture("vorbis-stereo.ogg", 2.0, 0.2);
    }
    #[test]
    fn applies_opushead_output_gain_to_decoded_amplitude() {
        // RFC 7845 §5.1: the output gain is a signed Q7.8 dB value that scales
        // the decoded samples. -1541/256 dB is about -6.02 dB, a linear factor
        // of ~0.5, so a stream relabelled with that gain must decode to about
        // half the amplitude of the unmodified fixture.
        let baseline = middle_rms(&decode_fixture("opus-stereo.ogg", 2.0, 0.2));
        let mut bytes = fs::read(fixture_path("opus-stereo.ogg")).unwrap();
        let head = bytes
            .windows(8)
            .position(|window| window == b"OpusHead")
            .expect("OpusHead");
        bytes[head + 16..head + 18].copy_from_slice(&(-1541i16).to_le_bytes());
        let page = page_start_for(&bytes, head);
        rechecksum_page(&mut bytes, page);
        let fixture = Fixture::new();
        fs::write(&fixture.source, &bytes).unwrap();
        assert_eq!(normalize(&fixture.source, &fixture.output).unwrap(), 2.0);
        let gained = middle_rms(&crate::audio::read_audio(&fixture.output).unwrap());
        let ratio = gained / baseline;
        assert!(
            (ratio - 0.5).abs() < 0.02,
            "output gain was not applied: ratio {ratio} (baseline {baseline}, gained {gained})"
        );
    }
    #[test]
    fn decodes_ogg_opus_with_a_non_48khz_input_rate_metadata() {
        // RFC 7845 §5.1: the "input sample rate" field is informational only.
        // Opus always decodes at 48 kHz, so a mismatched value must not change
        // the output or its exact duration.
        let mut bytes = fs::read(fixture_path("opus-stereo.ogg")).unwrap();
        let head = bytes
            .windows(8)
            .position(|window| window == b"OpusHead")
            .expect("OpusHead");
        bytes[head + 12..head + 16].copy_from_slice(&44_100u32.to_le_bytes());
        let page = page_start_for(&bytes, head);
        rechecksum_page(&mut bytes, page);
        let fixture = Fixture::new();
        fs::write(&fixture.source, &bytes).unwrap();
        assert_eq!(normalize(&fixture.source, &fixture.output).unwrap(), 2.0);
        assert_eq!(
            crate::audio::read_audio(&fixture.output).unwrap().len(),
            32_000
        );
    }
    #[test]
    fn corrupted_ogg_opus_packet_never_panics_or_pads_the_duration() {
        // Damage the payload of one middle audio packet (and repair the page CRC
        // so the demuxer still accepts it): a hostile packet must yield a clean
        // error or at most the true length, never a panic or invented audio.
        let mut bytes = fs::read(fixture_path("opus-stereo.ogg")).unwrap();
        let audio_start = bytes
            .windows(8)
            .position(|window| window == b"OpusTags")
            .expect("OpusTags");
        let page = page_start_for(&bytes, audio_start);
        let flip = page + (bytes.len() - page) / 2;
        bytes[flip] ^= 0xff;
        let damaged_page = page_start_for(&bytes, flip);
        rechecksum_page(&mut bytes, damaged_page);
        let fixture = Fixture::new();
        fs::write(&fixture.source, &bytes).unwrap();
        match normalize(&fixture.source, &fixture.output) {
            Ok(duration) => {
                assert!(
                    duration <= 2.0 + 1e-6,
                    "corrupted stream grew to {duration}"
                );
                assert!(crate::audio::read_audio(&fixture.output).unwrap().len() <= 32_000);
            }
            Err(_) => assert!(!fixture.output.exists(), "failed import left audio behind"),
        }
    }
    #[test]
    fn rejects_multichannel_opus_and_cleans_up() {
        // Relabel the stereo ID header as a six-channel mapping-family-1 stream;
        // this must be refused, not decoded into mangled audio.
        let mut bytes = fs::read(fixture_path("opus-stereo.ogg")).unwrap();
        let head = bytes
            .windows(8)
            .position(|window| window == b"OpusHead")
            .expect("OpusHead");
        bytes[head + 9] = 6;
        bytes[head + 18] = 1;
        let page = page_start_for(&bytes, head);
        rechecksum_page(&mut bytes, page);
        let fixture = Fixture::with_source_extension("ogg");
        fs::write(&fixture.source, &bytes).unwrap();
        let error = normalize(&fixture.source, &fixture.output).unwrap_err();
        assert!(
            error.contains("mono or stereo"),
            "unexpected error: {error}"
        );
        assert!(
            !fixture.output.exists(),
            "refused import must not leave audio"
        );
    }
    #[test]
    fn rejects_chained_ogg_streams_and_cleans_up() {
        let bytes = fs::read(fixture_path("opus-stereo.ogg")).unwrap();
        let mut chained = bytes.clone();
        chained.extend_from_slice(&bytes);
        let fixture = Fixture::with_source_extension("ogg");
        fs::write(&fixture.source, &chained).unwrap();
        let error = normalize(&fixture.source, &fixture.output).unwrap_err();
        assert!(error.contains("Chained"), "unexpected error: {error}");
        assert!(
            !fixture.output.exists(),
            "refused import must not leave audio"
        );
    }
    #[test]
    fn rejects_garbage_masquerading_as_ogg_and_cleans_up() {
        let fixture = Fixture::with_source_extension("ogg");
        fs::write(&fixture.source, b"OggS not really an Ogg Opus recording").unwrap();
        assert!(normalize(&fixture.source, &fixture.output).is_err());
        assert!(!fixture.output.exists());
    }
    #[test]
    fn truncated_ogg_opus_never_yields_fabricated_full_length_audio() {
        // A cut-off Ogg stream must either fail cleanly or decode only the audio
        // that is actually present -- never a padded full-length result.
        let bytes = fs::read(fixture_path("opus-stereo.ogg")).unwrap();
        let fixture = Fixture::with_source_extension("ogg");
        fs::write(&fixture.source, &bytes[..bytes.len() * 3 / 5]).unwrap();
        match normalize(&fixture.source, &fixture.output) {
            Ok(duration) => {
                assert!(
                    duration < 2.0,
                    "truncated stream reported full length {duration}"
                );
                assert!(crate::audio::read_audio(&fixture.output).unwrap().len() < 32_000);
            }
            Err(_) => assert!(!fixture.output.exists(), "failed import left audio behind"),
        }
    }
    #[test]
    fn rejects_opus_with_a_missing_or_empty_identification_header() {
        let mut params = CodecParameters::new();
        params.for_codec(CODEC_TYPE_OPUS);
        assert!(OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_err());
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_OPUS)
            .with_extra_data(Box::from(&b"OpusHead"[..]));
        assert!(OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_err());
    }
    /// Build an `OpusHead` identification header for decoder unit tests.
    fn opus_head(version: u8, channels: u8, family: u8, gain: i16) -> Vec<u8> {
        let mut extra = Vec::from(&b"OpusHead"[..]);
        extra.push(version);
        extra.push(channels);
        extra.extend_from_slice(&312u16.to_le_bytes()); // pre-skip
        extra.extend_from_slice(&48_000u32.to_le_bytes()); // input sample rate
        extra.extend_from_slice(&gain.to_le_bytes());
        extra.push(family);
        extra
    }
    fn opus_params(extra: &[u8], channels: symphonia::core::audio::Channels) -> CodecParameters {
        let mut params = CodecParameters::new();
        params
            .for_codec(CODEC_TYPE_OPUS)
            .with_channels(channels)
            .with_extra_data(Box::from(extra));
        params
    }
    #[test]
    fn accepts_supported_opushead_versions_and_rejects_incompatible_ones() {
        // RFC 7845 §5.1: minor-version changes (the low nibble) are compatible,
        // so any version up to 15 is readable; 16 and above is a new major
        // version this adapter must refuse.
        for version in [0u8, 1, 15] {
            let head = opus_head(version, 1, 0, 0);
            let params = opus_params(&head, symphonia::core::audio::Channels::FRONT_LEFT);
            assert!(
                OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_ok(),
                "version {version} should be accepted"
            );
        }
        for version in [16u8, 0x20, 0xff] {
            let head = opus_head(version, 1, 0, 0);
            let params = opus_params(&head, symphonia::core::audio::Channels::FRONT_LEFT);
            assert!(
                OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_err(),
                "version {version} should be rejected"
            );
        }
    }
    #[test]
    fn rejects_channel_mapping_family_one_even_for_mono_and_stereo() {
        // RFC 7845 §5.1.1: family 1 carries a channel mapping table this
        // single-stream adapter does not implement. Even a one- or two-channel
        // family-1 stream looks decodable but must be refused, never guessed.
        let cases = [
            (1u8, symphonia::core::audio::Channels::FRONT_LEFT, vec![0u8]),
            (
                2,
                symphonia::core::audio::Channels::FRONT_LEFT
                    | symphonia::core::audio::Channels::FRONT_RIGHT,
                vec![0, 1],
            ),
            (
                6,
                symphonia::core::audio::Channels::FRONT_LEFT,
                vec![0, 4, 1, 2, 3, 5],
            ),
        ];
        for (channels, layout, mapping) in cases {
            let mut head = opus_head(1, channels, 1, 0);
            head.push(1); // stream count
            head.push(channels - 1); // coupled stream count
            head.extend_from_slice(&mapping);
            let params = opus_params(&head, layout);
            assert!(
                OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_err(),
                "family-1 stream with {channels} channel(s) should be rejected"
            );
        }
    }
    #[test]
    fn rejects_unsupported_opus_channel_counts_and_layout_mismatches() {
        // Family 0 allows only 1 or 2 channels (RFC 7845 §5.1.1.1).
        for channels in [0u8, 3, 255] {
            let head = opus_head(1, channels, 0, 0);
            let params = opus_params(&head, symphonia::core::audio::Channels::FRONT_LEFT);
            assert!(
                OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_err(),
                "{channels} channels should be rejected"
            );
        }
        // The demuxer's channel layout must agree with the header's count.
        let head = opus_head(1, 2, 0, 0);
        let params = opus_params(&head, symphonia::core::audio::Channels::FRONT_LEFT);
        assert!(OpusDecoderAdapter::try_new(&params, &DecoderOptions::default()).is_err());
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
            "speech-opus.ogg",
            "speech-opus.opus",
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
        // Multichannel Opus must be refused rather than downmixed blindly.
        let fixture = Fixture::new();
        let error = normalize(&root.join("multichannel-opus.ogg"), &fixture.output).unwrap_err();
        assert!(
            error.contains("mono or stereo"),
            "unexpected error: {error}"
        );
        assert!(!fixture.output.exists());
    }
}
