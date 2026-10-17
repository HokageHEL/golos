# Offline speaker detection

After transcribing, choose **Detect speakers** to assign recurring anonymous
voices to paragraphs. This is a separate sherpa-onnx segmentation/embedding/
clustering pass, not Whisper's turn markers and not real-person identification.
No account or runtime network connection is used.

## Install the models

Download and extract the [pyannote segmentation archive](https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2)
and download [nemo_en_titanet_small.onnx](https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/nemo_en_titanet_small.onnx).
This is the pair tested locally. Keep the segmentation archive's MIT LICENSE.
Embedding-model weights have separate terms from the inference engine; consult
[the upstream model release](https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models)
and [NVIDIA’s TitaNet small model card](https://catalog.ngc.nvidia.com/orgs/nvidia/teams/nemo/models/titanet_small) before redistribution. Golos bundles no weights.

1. Choose **Import speaker models** in Golos.
2. In the first dialog choose the extracted `model.int8.onnx` segmentation file.
3. In the second dialog choose `nemo_en_titanet_small.onnx` (embedding).
4. Open a completed transcript. Select Auto or the known number of speakers,
   then choose **Detect speakers**.
5. Review the labels. Correct an individual paragraph's Speaker field if needed.
   Choose **Save edits**, or **Discard edits** to retain the saved version.

Files are copied to `speakers/generation-<number>/` in the application data
directory. The pair is validated before a new generation is published. A failed
replacement keeps the previous pair. Pending directories left by interruption
are ignored. Older generations remain on disk; with Golos closed they may be
removed if no longer needed. The latest complete generation is selected.

## Playback and labels

Play/pause, speed (0.75–2×), and seeking use the library's normalized local WAV.
Click a paragraph's timestamp to seek there. Highlighting follows paragraph
start/end timestamps, with no highlight during gaps. It is not word-level
alignment. Switching recordings or opening delete confirmation stops playback.
TXT and SRT exports preserve the paragraph's current speaker labels.

A paragraph gets the speaker with greatest overlapping speech duration. Repeated
intervals for one speaker count once. Ties and gaps remain unknown. Text and
timestamps are unchanged; a paragraph containing several people gets at most one
label and may need manual correction. Labels are local to a recording. Auto
speaker counts, overlapping speech, brief turns, and Ukrainian quality remain
subject to real-recording evaluation.

## Cancellation, builds, and platforms

The native engine's process call cannot be interrupted mid-pass. **Cancel after
current pass** waits for inference to return, discards its result, and leaves
saved content unchanged. There is no fabricated percentage progress.

The `sherpa-onnx` static dependency downloads upstream native/ONNX Runtime build
libraries during the first build; subsequent runtime inference is offline.
This increases application size. The dependency's `SHERPA_ONNX_LIB_DIR` and
`SHERPA_ONNX_ARCHIVE_DIR` support pre-provisioned build artifacts. See
[upstream diarization documentation](https://k2-fsa.github.io/sherpa/onnx/speaker-diarization/models.html).
macOS is the validated build target here. Android document URI import, mobile
memory/lifecycle behavior, and Windows/Linux packages still need validation.

## Verification — 2026-10-17

Checks passed: native frontend build, ESLint, Rust formatting, Clippy with
all targets and warnings denied, 123 JavaScript tests, and 54 native tests
(three opt-in tests skipped in the default run). The real diarization test was
also run separately and passed as described below.

Native unit tests exercise speaker assignment, cancellation checkpoints, coherent
model-pair replacement and audio authorization. Playback helper tests cover
boundaries, gaps, overlaps, seeking and rates. Browser UI checks use synthetic
WAV and mocked native IPC; they are not native webview/device evidence.

A real CPU inference test ran on the upstream public `0-four-speakers-zh.wav`
sample, with int8 pyannote and NeMo small, using a fixed count of four. It returned
four distinct speakers with valid turns. This is a smoke test, not a diarization
accuracy benchmark. A CAM++ alternative returned only three speakers on that
sample; no accuracy claim is made for it. Models and sample are not committed.

Tested SHA-256 values:

- `model.int8.onnx`: `d582f4b4c6b48205de7e0643c57df0df5615a3c176189be3fc461e9d18827b5d`
- `nemo_en_titanet_small.onnx`: `ad4a1802485d8b34c722d2a9d04249662f2ece5d28a7a039063ca22f515a789e`
- `0-four-speakers-zh.wav`: `bedf036caed208386c67b4ef4b11f83d74dd0d420b102163a1c33cd09cde7010`

Opt-in test, with normalized public audio and local model paths:

```sh
GOLOS_TEST_SEG_MODEL=/path/model.int8.onnx \
GOLOS_TEST_EMB_MODEL=/path/nemo_en_titanet_small.onnx \
GOLOS_TEST_AUDIO=/path/0-four-speakers-zh.wav GOLOS_TEST_SPEAKERS=4 \
cargo test --locked --manifest-path native/src-tauri/Cargo.toml \
  diarizes_real_audio_offline -- --ignored
```
