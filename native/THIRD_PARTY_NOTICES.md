# Native components

The application links to Tauri, whisper-rs/whisper.cpp, SQLite via rusqlite,
hound, Symphonia, Rubato, sherpa-onnx (with the bundled ONNX Runtime), serde,
reqwest/Tokio for the optional loopback AI adapter,
and other crates listed in `src-tauri/Cargo.lock`. The UI uses React
and Tauri's JavaScript API, locked in the root `package-lock.json`.

- [whisper.cpp license](https://github.com/ggml-org/whisper.cpp/blob/master/LICENSE)
- [OpenAI Whisper model/code license](https://github.com/openai/whisper/blob/main/LICENSE)
- [whisper-rs license](https://github.com/tazz4843/whisper-rs)
- [Tauri licenses](https://github.com/tauri-apps/tauri)
- [Symphonia license (MPL-2.0)](https://github.com/pdeljanov/Symphonia/blob/master/LICENSE)
- [Rubato license (MIT)](https://github.com/HEnquist/rubato/blob/master/LICENSE.txt)
- [sherpa-onnx license (Apache-2.0)](https://github.com/k2-fsa/sherpa-onnx/blob/master/LICENSE)
  — the `sherpa-onnx` Rust crate is also Apache-2.0. The `static` feature links
  prebuilt sherpa-onnx and ONNX Runtime libraries; the build script downloads
  them from the upstream releases.
- [ONNX Runtime license (MIT)](https://github.com/microsoft/onnxruntime/blob/main/LICENSE)

Speaker diarization models are downloaded from upstream on request or imported by the user and are
**not** bundled. When a user imports them, the applicable licenses are:

- [pyannote/segmentation-3.0 (MIT)](https://huggingface.co/pyannote/segmentation-3.0)
  via the
  [sherpa-onnx-pyannote-segmentation-3-0](https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-segmentation-models)
  conversion.
- [3D-Speaker CAM++ (Apache-2.0)](https://github.com/modelscope/3D-Speaker)
  via the
  [speaker recognition models](https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models)
  release. Each model in that release has its own license; check the
  corresponding upstream repository before redistribution.

Models are downloaded from upstream on request or imported by the user and are not bundled. Guided segmentation installs retain the archive’s MIT LICENSE in the model generation directory. Only synthetic audio regression fixtures are distributed. Before distributing a release binary,
assemble the actual license texts/notices for the selected dependencies and
models into the distribution. This file is an inventory, not that completed bundle.

The locally tested alternative embedding is `nemo_en_titanet_small.onnx`,
converted by sherpa-onnx from NVIDIA NeMo. Consult [NVIDIA's TitaNet small model card](https://catalog.ngc.nvidia.com/orgs/nvidia/teams/nemo/models/titanet_small) and license for weight terms; the NeMo software license alone does not
license model weights. No NeMo weights are distributed with Golos.

Ollama and local instruct-model weights are installed separately by the user and are not bundled. Their software/model licenses apply independently; the starter smoke test used Qwen2.5-1.5B-Instruct via Ollama.
