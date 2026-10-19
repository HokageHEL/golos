//! Offline speaker diarization for user-requested speaker labels.
//!
//! This wraps the sherpa-onnx offline speaker diarization pipeline: a pyannote
//! segmentation model (converted from `pyannote/segmentation-3.0`, MIT) finds
//! speech regions, a speaker embedding model (for example 3D-Speaker CAM++,
//! Apache-2.0) turns those regions into embeddings, and clustering groups the
//! embeddings into speakers. Everything runs locally through the ONNX Runtime;
//! no audio, text, or embedding ever leaves the machine.
//!
//! [`run`] performs the inference and returns raw [`SpeakerTurn`]s. [`assign`]
//! then maps existing transcript segments onto those turns without changing any
//! text, timestamps, or word boundaries, so a stored transcript is relabeled
//! and never rewritten or split. Both are plain functions. Tauri commands in
//! lib.rs resolve installed model paths and run this on a blocking worker;
//! detection returns a draft, persisted only through the existing Save command.
//!
//! Cancellation limitation: the sherpa-onnx `process` call is one synchronous
//! function with no interrupt hook. The cancellation flag is only observed
//! before the models load, before inference starts, and after it returns. A
//! request made while inference is running cannot stop it immediately; it is
//! honored at the next checkpoint. UI copy must not promise instant
//! cancellation.

use crate::{audio, library::Segment};
use serde::{Deserialize, Serialize};
use sherpa_onnx::{
    FastClusteringConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    OfflineSpeakerDiarizationSegment, OfflineSpeakerSegmentationModelConfig,
    OfflineSpeakerSegmentationPyannoteModelConfig, SpeakerEmbeddingExtractorConfig,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Recordings are normalized to 16 kHz mono PCM on import (see [`crate::audio`]).
pub const SAMPLE_RATE: i32 = 16_000;

/// A user may say "two speakers" or leave it to automatic clustering. The range
/// is deliberately small: beyond this, clustering is unreliable and the UI
/// value is more likely a mistake than a real group size.
const MIN_SPEAKERS: u32 = 1;
const MAX_SPEAKERS: u32 = 20;

/// Message used for a cancelled or pre-empted job. Kept in one place so the
/// command wrapper, tests, and UI copy agree.
pub const CANCELLED: &str = "Speaker detection cancelled. You can try again.";

/// One speaker region produced by the engine, in milliseconds. Bounds are
/// already validated/clamped against the recording by [`run`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeakerTurn {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Raw engine speaker index. Only meaningful together with the whole turn
    /// list; [`assign`] turns it into a stable user-visible label.
    pub speaker: i32,
}

/// Run offline speaker diarization over a normalized 16 kHz mono recording.
///
/// `segmentation_model` is a pyannote segmentation ONNX file and
/// `embedding_model` is a speaker embedding ONNX file. `speaker_count` is the
/// user's known number of speakers, or `None` to cluster automatically.
///
/// Returns the turns sorted by start time. On error or cancellation the caller
/// must keep its **original** transcript segments untouched: this function
/// never returns partially assigned data.
pub fn run(
    audio_path: &Path,
    segmentation_model: &Path,
    embedding_model: &Path,
    speaker_count: Option<u32>,
    cancelled: &AtomicBool,
) -> Result<Vec<SpeakerTurn>, String> {
    validate_speaker_count(speaker_count)?;
    ensure_not_cancelled(cancelled)?;
    // Resolve model paths first: reject non-UTF-8 paths and interior NULs before
    // the safe wrapper turns them into C strings (a NUL would otherwise panic).
    let segmentation = model_path(segmentation_model, "segmentation")?;
    let embedding = model_path(embedding_model, "embedding")?;
    // `audio::read_audio` enforces 16 kHz, mono, 16-bit PCM, and returns
    // normalized f32 samples, so the samples and the models agree by
    // construction.
    let samples = audio::read_audio(audio_path)?;
    ensure_not_cancelled(cancelled)?;

    let diarizer = create_pipeline(segmentation, embedding, speaker_count)?;
    ensure_not_cancelled(cancelled)?;

    let diarized = diarizer.process(&samples).ok_or_else(|| {
        "Speaker detection failed. Try again or use a different recording.".to_string()
    })?;
    // There is no interrupt hook inside `process`, so this is the first chance
    // to observe a cancellation made while inference was running.
    ensure_not_cancelled(cancelled)?;

    let duration_ms = samples.len() as i64 * 1000 / SAMPLE_RATE as i64;
    Ok(sanitize_turns(diarized.sort_by_start_time(), duration_ms))
}

/// Validate a pair before activating it; no inference or audio access.
pub fn validate_models(segmentation: &Path, embedding: &Path) -> Result<(), String> {
    create_pipeline(
        model_path(segmentation, "segmentation")?,
        model_path(embedding, "embedding")?,
        None,
    )?;
    Ok(())
}

fn create_pipeline(
    segmentation: String,
    embedding: String,
    speaker_count: Option<u32>,
) -> Result<OfflineSpeakerDiarization, String> {
    // sherpa-onnx 1.13.8 can construct a diarizer with a null embedding
    // implementation for an unsupported model. sample_rate() still succeeds,
    // but process() then segfaults in SpeakerEmbeddingExtractor::Dim(). Check
    // both roles in Rust before entering FFI, including already-installed pairs.
    validate_model_roles(Path::new(&segmentation), Path::new(&embedding))?;
    let config = OfflineSpeakerDiarizationConfig {
        segmentation: OfflineSpeakerSegmentationModelConfig {
            pyannote: OfflineSpeakerSegmentationPyannoteModelConfig {
                model: Some(segmentation),
                window_shift_ratio: 0.1,
            },
            num_threads: worker_threads(),
            debug: false,
            provider: Some("cpu".into()),
        },
        embedding: SpeakerEmbeddingExtractorConfig {
            model: Some(embedding),
            num_threads: worker_threads(),
            debug: false,
            provider: Some("cpu".into()),
        },
        clustering: FastClusteringConfig {
            // A positive value fixes the speaker count; -1 uses the threshold.
            num_clusters: speaker_count.map(|n| n as i32).unwrap_or(-1),
            threshold: 0.5,
            compute_confidence: false,
        },
        ..Default::default()
    };

    let diarizer = OfflineSpeakerDiarization::create(&config).ok_or_else(|| {
        "Could not load the speaker models. Import both ONNX model files again.".to_string()
    })?;
    if diarizer.sample_rate() != SAMPLE_RATE {
        return Err(format!(
            "The segmentation model expects {} Hz audio, but recordings are {SAMPLE_RATE} Hz.",
            diarizer.sample_rate()
        ));
    }

    Ok(diarizer)
}

pub fn validate_model_roles(segmentation: &Path, embedding: &Path) -> Result<(), String> {
    let read = |path, role| {
        crate::onnx_metadata::read(path)
            .map_err(|_| format!("Cannot read the {role} ONNX model. Import speaker models again."))
    };
    let segmentation = read(segmentation, "segmentation")?;
    if segmentation.get("model_type").map(String::as_str) != Some("pyannote-segmentation-3.0") {
        return Err("The first speaker model must be a pyannote segmentation-3.0 ONNX model. Import speaker models again.".into());
    }
    let embedding = read(embedding, "embedding")?;
    if !matches!(
        embedding.get("framework").map(String::as_str),
        Some("nemo" | "wespeaker" | "3d-speaker")
    ) {
        return Err("The second speaker model must be a speaker embedding model, such as nemo_en_titanet_small.onnx. A segmentation model cannot be used for both roles. Import speaker models again.".into());
    }
    Ok(())
}

/// Map transcript segments onto diarized turns without changing text or
/// timestamps.
///
/// Labels are assigned in order of first appearance over time, so the earliest
/// speaker is always `speaker_1` and a single-speaker recording is just
/// `speaker_1`. For each segment the speaker with the greatest accumulated
/// overlap is chosen; segments with no overlap (a gap between turns) or an
/// exact tie between speakers are left as `None` rather than guessing. Multiple
/// speakers are never claimed for one segment: a paragraph keeps its existing
/// text and time bounds and gets at most one label.
pub fn assign(segments: &[Segment], turns: &[SpeakerTurn]) -> Vec<Segment> {
    let order = first_seen_speakers(turns);
    segments
        .iter()
        .map(|segment| Segment {
            speaker: speaker_for(segment, turns, &order),
            ..segment.clone()
        })
        .collect()
}

/// Stable, user-visible label for the `index`-th distinct speaker.
pub fn speaker_label(index: usize) -> String {
    format!("speaker_{}", index + 1)
}

fn validate_speaker_count(count: Option<u32>) -> Result<(), String> {
    match count {
        Some(n) if (MIN_SPEAKERS..=MAX_SPEAKERS).contains(&n) => Ok(()),
        Some(_) => Err(format!(
            "Choose between {MIN_SPEAKERS} and {MAX_SPEAKERS} speakers."
        )),
        None => Ok(()),
    }
}

/// Reject a model path the safe wrapper cannot turn into a C string, or one
/// that is not a readable file, with a message the import UI can show.
fn model_path(path: &Path, kind: &str) -> Result<String, String> {
    let text = path
        .to_str()
        .ok_or_else(|| format!("The {kind} model path is not valid UTF-8."))?;
    if text.contains('\0') {
        return Err(format!("The {kind} model path is invalid."));
    }
    if !path.is_file() {
        return Err(format!(
            "The {kind} model file is missing. Import it again."
        ));
    }
    Ok(text.to_string())
}

/// Bound inference to a few cores so a background diarization job cannot starve
/// the UI or another engine running on the same machine.
fn worker_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 4) as i32)
        .unwrap_or(1)
}

fn ensure_not_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::SeqCst) {
        Err(CANCELLED.to_string())
    } else {
        Ok(())
    }
}

/// Drop non-finite turns, clamp the rest to the recording, drop empty turns,
/// and sort by start time. Clamping is defensive: the engine should already
/// report in-range seconds, but a bad bound must never become a stored segment
/// that violates the library's timestamp validation.
fn sanitize_turns(
    raw: Vec<OfflineSpeakerDiarizationSegment>,
    duration_ms: i64,
) -> Vec<SpeakerTurn> {
    let mut turns: Vec<SpeakerTurn> = raw
        .into_iter()
        .filter_map(|turn| {
            if !turn.start.is_finite() || !turn.end.is_finite() || turn.speaker < 0 {
                return None;
            }
            let start_ms = (turn.start * 1000.0).round() as i64;
            let end_ms = (turn.end * 1000.0).round() as i64;
            let start_ms = start_ms.clamp(0, duration_ms);
            let end_ms = end_ms.clamp(0, duration_ms);
            if end_ms <= start_ms {
                return None;
            }
            Some(SpeakerTurn {
                start_ms,
                end_ms,
                speaker: turn.speaker,
            })
        })
        .collect();
    turns.sort_by_key(|turn| (turn.start_ms, turn.end_ms, turn.speaker));
    turns
}

/// Distinct raw speaker indices in order of first appearance over time.
fn first_seen_speakers(turns: &[SpeakerTurn]) -> Vec<i32> {
    let mut ordered: Vec<&SpeakerTurn> = turns.iter().collect();
    ordered.sort_by_key(|turn| (turn.start_ms, turn.speaker));
    let mut seen = Vec::new();
    for turn in ordered {
        if !seen.contains(&turn.speaker) {
            seen.push(turn.speaker);
        }
    }
    seen
}

/// The speaker with the greatest accumulated overlap with `segment`, or `None`
/// for a gap, zero overlap, or an exact tie between speakers.
fn speaker_for(segment: &Segment, turns: &[SpeakerTurn], order: &[i32]) -> Option<String> {
    // Union same-speaker intervals first: overlapping or repeated turns must
    // not count the same millisecond twice and outweigh another speaker.
    let mut intervals = std::collections::BTreeMap::<i32, Vec<(i64, i64)>>::new();
    for turn in turns {
        let start = segment.start_ms.max(turn.start_ms);
        let end = segment.end_ms.min(turn.end_ms);
        if end > start && turn.speaker >= 0 {
            intervals
                .entry(turn.speaker)
                .or_default()
                .push((start, end));
        }
    }
    let totals: Vec<(i32, i64)> = intervals
        .into_iter()
        .map(|(speaker, mut spans)| {
            spans.sort_unstable();
            let mut total = 0;
            let mut previous_end = i64::MIN;
            for (start, end) in spans {
                total += (end - start.max(previous_end)).max(0);
                previous_end = previous_end.max(end);
            }
            (speaker, total)
        })
        .collect();
    let max = totals.iter().map(|(_, total)| *total).max()?;
    if max <= 0 {
        return None;
    }
    let mut winners = totals.iter().filter(|(_, total)| *total == max);
    let (speaker, _) = winners.next()?;
    if winners.next().is_some() {
        // Two speakers overlap the segment by the same amount: do not invent a
        // label the audio does not support.
        return None;
    }
    let index = order.iter().position(|candidate| candidate == speaker)?;
    Some(speaker_label(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start_ms: i64, end_ms: i64, text: &str) -> Segment {
        Segment {
            start_ms,
            end_ms,
            speaker: None,
            text: text.into(),
        }
    }

    fn turn(start_ms: i64, end_ms: i64, speaker: i32) -> SpeakerTurn {
        SpeakerTurn {
            start_ms,
            end_ms,
            speaker,
        }
    }

    fn raw(start: f32, end: f32, speaker: i32) -> OfflineSpeakerDiarizationSegment {
        OfflineSpeakerDiarizationSegment {
            start,
            end,
            speaker,
            confidence: 0.0,
        }
    }

    #[test]
    fn labels_start_at_one_and_follow_first_appearance() {
        assert_eq!(speaker_label(0), "speaker_1");
        assert_eq!(speaker_label(3), "speaker_4");

        // Raw indices are arbitrary and out of order; labels are by first time.
        let turns = vec![
            turn(2_000, 3_000, 7),
            turn(0, 1_000, 3),
            turn(1_000, 2_000, 5),
        ];
        let assigned = assign(
            &[segment(0, 1_000, "a"), segment(2_000, 3_000, "b")],
            &turns,
        );
        assert_eq!(assigned[0].speaker.as_deref(), Some("speaker_1"));
        assert_eq!(assigned[1].speaker.as_deref(), Some("speaker_3"));
    }

    #[test]
    fn chooses_greatest_accumulated_overlap_not_the_first_turn() {
        // Speaker 0 owns 0..500, speaker 1 owns 500..2000. The 0..1500 segment
        // overlaps speaker 0 by 500 ms and speaker 1 by 1000 ms, so it must be
        // labeled speaker_2 even though speaker 0's turn comes first.
        let turns = vec![turn(0, 500, 0), turn(500, 2_000, 1)];
        let assigned = assign(&[segment(0, 1_500, "one")], &turns);
        assert_eq!(assigned[0].speaker.as_deref(), Some("speaker_2"));

        // Accumulation across several short turns of one speaker can outweigh a
        // single long turn from another speaker.
        let turns = vec![
            turn(0, 400, 0),
            turn(400, 800, 0),
            turn(0, 900, 1),
            turn(900, 1_000, 0),
        ];
        // Speaker 0 total = 400 + 400 + 100 = 900; speaker 1 total = 900. Tie.
        let assigned = assign(&[segment(0, 1_000, "tie")], &turns);
        assert_eq!(assigned[0].speaker, None);

        let turns = vec![turn(0, 700, 0), turn(700, 1_000, 0), turn(0, 1_000, 1)];
        // Speaker 0 = 1000, speaker 1 = 1000 -> still a tie.
        assert_eq!(assign(&[segment(0, 1_000, "tie")], &turns)[0].speaker, None);

        let turns = vec![turn(0, 900, 0), turn(0, 500, 1)];
        // Speaker 0 = 900, speaker 1 = 500 -> speaker_1.
        assert_eq!(
            assign(&[segment(0, 1_000, "win")], &turns)[0]
                .speaker
                .as_deref(),
            Some("speaker_1")
        );
    }

    #[test]
    fn marks_gaps_and_zero_overlap_as_unknown() {
        let turns = vec![turn(0, 1_000, 0), turn(2_000, 3_000, 1)];
        let assigned = assign(
            &[
                segment(1_000, 2_000, "gap"),
                segment(1_500, 1_600, "inside gap"),
                segment(3_000, 3_500, "abuts end"),
                segment(0, 500, "covered"),
            ],
            &turns,
        );
        assert_eq!(assigned[0].speaker, None);
        assert_eq!(assigned[1].speaker, None);
        assert_eq!(assigned[2].speaker, None);
        assert_eq!(assigned[3].speaker.as_deref(), Some("speaker_1"));
    }

    #[test]
    fn unknown_turns_label_everything_unknown() {
        let segments = [segment(0, 500, "a"), segment(500, 1_000, "b")];
        let assigned = assign(&segments, &[]);
        assert!(assigned.iter().all(|segment| segment.speaker.is_none()));
        assert!(assign(&[], &[turn(0, 1_000, 0)]).is_empty());
    }

    #[test]
    fn recurring_speakers_stay_stable_and_text_is_untouched() {
        // Speakers appear in bursts; the same raw index must always map to the
        // same label, and text/timestamps must not move or split.
        let turns = vec![
            turn(0, 1_000, 42),
            turn(1_000, 2_000, 9),
            turn(2_000, 2_500, 42),
            turn(2_500, 3_000, 9),
            turn(3_000, 4_000, 42),
        ];
        let assigned = assign(
            &[
                segment(0, 1_000, "hello"),
                segment(1_000, 2_000, "there"),
                segment(2_000, 2_500, "again"),
                segment(2_500, 3_000, "yes"),
                segment(3_000, 4_000, "bye"),
            ],
            &turns,
        );
        let speakers: Vec<_> = assigned
            .iter()
            .map(|segment| segment.speaker.clone())
            .collect();
        assert_eq!(
            speakers,
            vec![
                Some("speaker_1".to_string()),
                Some("speaker_2".to_string()),
                Some("speaker_1".to_string()),
                Some("speaker_2".to_string()),
                Some("speaker_1".to_string()),
            ]
        );
        assert_eq!(assigned[0].text, "hello");
        assert_eq!(assigned[0].start_ms, 0);
        assert_eq!(assigned[0].end_ms, 1_000);
        assert_eq!(assigned[4].text, "bye");
        assert_eq!(assigned[4].end_ms, 4_000);
    }

    #[test]
    fn sorts_unsorted_turns_before_labeling() {
        let ordered = vec![turn(0, 1_000, 1), turn(1_000, 2_000, 2)];
        let shuffled = vec![turn(1_000, 2_000, 2), turn(0, 1_000, 1)];
        let segments = [segment(0, 1_000, "a"), segment(1_000, 2_000, "b")];
        let from_ordered = assign(&segments, &ordered);
        let from_shuffled = assign(&segments, &shuffled);
        let labels = |assigned: &[Segment]| {
            assigned
                .iter()
                .map(|segment| segment.speaker.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(labels(&from_ordered), labels(&from_shuffled));
    }

    #[test]
    fn duplicated_turns_do_not_double_count_one_voice() {
        let turns = [turn(0, 400, 0), turn(0, 400, 0), turn(400, 1000, 1)];
        assert_eq!(
            assign(&[segment(0, 1000, "hello")], &turns)[0]
                .speaker
                .as_deref(),
            Some("speaker_2")
        );
    }

    #[test]
    fn rejects_speaker_counts_outside_the_supported_range() {
        assert!(validate_speaker_count(None).is_ok());
        assert!(validate_speaker_count(Some(1)).is_ok());
        assert!(validate_speaker_count(Some(20)).is_ok());
        assert!(validate_speaker_count(Some(0)).is_err());
        assert!(validate_speaker_count(Some(21)).is_err());
        assert!(validate_speaker_count(Some(u32::MAX)).is_err());
    }

    #[test]
    fn clamps_non_finite_and_out_of_bounds_turn_bounds() {
        let turns = sanitize_turns(
            vec![
                raw(0.0, 1.0, 0),
                raw(-2.0, 0.5, 1),          // start clamped to 0
                raw(1.5, 99.0, 2),          // end clamped to duration
                raw(f32::NAN, 1.0, 3),      // dropped
                raw(0.5, f32::INFINITY, 4), // dropped
                raw(2.0, 2.0, 5),           // empty, dropped
                raw(3.0, 2.0, 6),           // inverted, dropped
            ],
            2_000,
        );
        assert_eq!(
            turns,
            vec![turn(0, 500, 1), turn(0, 1_000, 0), turn(1_500, 2_000, 2),]
        );
    }

    #[test]
    fn cancellation_is_checked_at_each_checkpoint() {
        assert!(ensure_not_cancelled(&AtomicBool::new(false)).is_ok());
        let cancelled = ensure_not_cancelled(&AtomicBool::new(true)).unwrap_err();
        assert!(cancelled.contains("cancelled"));

        // A cancelled job must fail before loading any model: bogus paths are
        // never touched, so there is no partial or fake result.
        let error = run(
            Path::new("/does/not/exist.wav"),
            Path::new("/does/not/exist-seg.onnx"),
            Path::new("/does/not/exist-emb.onnx"),
            Some(2),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(error.contains("cancelled"), "unexpected error: {error}");
    }

    #[test]
    fn validates_arguments_before_loading_models() {
        // Out-of-range counts fail first, even with a cancelled flag set.
        let error = run(
            Path::new("/does/not/exist.wav"),
            Path::new("/does/not/exist-seg.onnx"),
            Path::new("/does/not/exist-emb.onnx"),
            Some(0),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(
            error.contains("between 1 and 20"),
            "unexpected error: {error}"
        );

        // Missing model files are reported without loading anything.
        let error = run(
            Path::new("/does/not/exist.wav"),
            Path::new("/does/not/exist-seg.onnx"),
            Path::new("/does/not/exist-emb.onnx"),
            None,
            &AtomicBool::new(false),
        )
        .unwrap_err();
        assert!(
            error.contains("segmentation model file is missing"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn rejects_wrong_model_roles_before_native_loading() {
        // Minimal ModelProto metadata is enough for role checks. These are not
        // loadable neural networks: a regression entering FFI would fail here.
        let root = std::env::temp_dir().join(format!("golos-roles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let write = |name: &str, key: &str, value: &str| {
            let entry = [
                vec![10, key.len() as u8],
                key.as_bytes().to_vec(),
                vec![18, value.len() as u8],
                value.as_bytes().to_vec(),
            ]
            .concat();
            let bytes = [vec![114, entry.len() as u8], entry].concat();
            let path = root.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let seg = write("seg.onnx", "model_type", "pyannote-segmentation-3.0");
        for framework in ["nemo", "wespeaker", "3d-speaker"] {
            let emb = write("emb.onnx", "framework", framework);
            assert!(validate_model_roles(&seg, &emb).is_ok());
            assert!(validate_models(&emb, &seg)
                .unwrap_err()
                .contains("first speaker model"));
        }
        // The reported crash: two valid segmentation files, with one imported
        // as the embedding model. Reject during import and for an old install.
        assert!(validate_models(&seg, &seg)
            .unwrap_err()
            .contains("second speaker model"));
        let unknown = write("unknown.onnx", "framework", "unsupported");
        assert!(validate_models(&seg, &unknown).is_err());
        std::fs::write(&unknown, b"not an ONNX model").unwrap();
        assert!(validate_models(&seg, &unknown)
            .unwrap_err()
            .contains("Cannot read"));
        std::fs::remove_dir_all(root).unwrap();
    }

    // Explicit opt-in: no network calls, downloads, or private recordings.
    // Point the variables at the public sherpa-onnx pyannote segmentation model
    // and a 3D-Speaker embedding model plus a public multi-speaker WAV.
    #[test]
    #[ignore = "Set GOLOS_TEST_SEG_MODEL, GOLOS_TEST_EMB_MODEL and GOLOS_TEST_AUDIO"]
    fn diarizes_real_audio_offline() {
        let segmentation = std::env::var("GOLOS_TEST_SEG_MODEL").expect("GOLOS_TEST_SEG_MODEL");
        let embedding = std::env::var("GOLOS_TEST_EMB_MODEL").expect("GOLOS_TEST_EMB_MODEL");
        let audio = std::env::var("GOLOS_TEST_AUDIO").expect("GOLOS_TEST_AUDIO");

        // Real pyannote models used to pass import in both slots, then crash
        // only when processing a long enough recording to need embeddings.
        assert!(
            validate_models(Path::new(&segmentation), Path::new(&segmentation))
                .unwrap_err()
                .contains("second speaker model")
        );
        assert!(run(
            Path::new(&audio),
            Path::new(&segmentation),
            Path::new(&segmentation),
            None,
            &AtomicBool::new(false),
        )
        .unwrap_err()
        .contains("second speaker model"));

        let count = std::env::var("GOLOS_TEST_SPEAKERS")
            .ok()
            .map(|n| {
                n.parse::<u32>()
                    .expect("GOLOS_TEST_SPEAKERS must be a count")
            })
            .unwrap_or(2);
        let turns = run(
            Path::new(&audio),
            Path::new(&segmentation),
            Path::new(&embedding),
            Some(count),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!turns.is_empty(), "expected at least one speaker turn");
        assert!(turns
            .iter()
            .all(|turn| turn.start_ms >= 0 && turn.end_ms > turn.start_ms));
        let speakers: std::collections::BTreeSet<i32> =
            turns.iter().map(|turn| turn.speaker).collect();
        assert!(
            speakers.len() == count as usize,
            "expected {count} speakers, got {speakers:?}"
        );

        // Labels are stable across recurrences and preserve the transcript.
        let segments = vec![
            segment(turns[0].start_ms, turns[0].end_ms, "first"),
            segment(
                turns[turns.len() - 1].start_ms,
                turns[turns.len() - 1].end_ms,
                "last",
            ),
        ];
        let assigned = assign(&segments, &turns);
        assert!(assigned.iter().all(|segment| segment.speaker.is_some()));
        assert_eq!(assigned[0].text, "first");
        assert_eq!(assigned[0].start_ms, segments[0].start_ms);

        // A cancelled job fails before inference and keeps no partial output.
        let cancelled = run(
            Path::new(&audio),
            Path::new(&segmentation),
            Path::new(&embedding),
            Some(count),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert!(
            cancelled.contains("cancelled"),
            "unexpected error: {cancelled}"
        );
    }
}
