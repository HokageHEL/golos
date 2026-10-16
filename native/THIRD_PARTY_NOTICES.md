# Native components

The application links to Tauri, whisper-rs/whisper.cpp, SQLite via rusqlite,
hound, Symphonia, Rubato, serde, and other crates listed in `src-tauri/Cargo.lock`. The UI uses React
and Tauri's JavaScript API, locked in the root `package-lock.json`.

- [whisper.cpp license](https://github.com/ggml-org/whisper.cpp/blob/master/LICENSE)
- [OpenAI Whisper model/code license](https://github.com/openai/whisper/blob/main/LICENSE)
- [whisper-rs license](https://github.com/tazz4843/whisper-rs)
- [Tauri licenses](https://github.com/tauri-apps/tauri)
- [Symphonia license (MPL-2.0)](https://github.com/pdeljanov/Symphonia/blob/master/LICENSE)
- [Rubato license (MIT)](https://github.com/HEnquist/rubato/blob/master/LICENSE.txt)

Models are supplied by the user in this preview and are not bundled. No audio
fixtures are distributed. Before shipping a binary or adding model downloads,
assemble the actual license texts/notices for the selected dependencies and
models into the distribution. This file is an inventory, not that completed bundle.
