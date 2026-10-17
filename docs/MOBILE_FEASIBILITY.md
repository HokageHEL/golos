# Mobile feasibility: Android and iOS for Golos

Baseline: 2026-10-16, clean git commit `b20e492`. This document is the P0
"platform risk spike" output for the mobile half of work package F in
[`OFFLINE_FIRST_PLAN.md`](OFFLINE_FIRST_PLAN.md). It is a feasibility record, not
a support claim: **no Android or iOS binary was compiled here, and no device or
simulator was run.**

## Evidence levels used below

Every claim is tagged so it cannot be confused with an executed build:

| Tag | Meaning |
| --- | --- |
| **[source]** | Read from repository source, dependency metadata (crates.io/docs.rs/Cargo.lock), or official Tauri/Android docs. |
| **[prereq]** | Produced by `scripts/check-mobile-prerequisites.mjs` or a Tauri CLI probe that was actually executed on this host. |
| **[compile]** | A mobile artifact was built. *Nothing in this document is [compile].* |
| **[device]** | Run on physical hardware. *Nothing in this document is [device].* |

The checker added with this document is the only executable artifact. It inspects
host toolchains; a green run means "the official prerequisites are present", not
"the app compiles" and not "the app works on a phone".

## Host used for this pass

- macOS 27.0.1 (build 26A434), arm64; Node v22.22.2, npm 10.9.7; project-local
  `@tauri-apps/cli 2.12.3`. **[prereq]**
- `rustc`, `cargo`, `rustup`, `cmake`, `adb`, `sdkmanager`, `gradle`, `ninja`:
  **absent**. `ANDROID_HOME`/`ANDROID_SDK_ROOT`/`NDK_HOME` are unset. **[prereq]**
- `java`/`javac` and `xcodebuild` exist on `PATH` but are non-functional macOS
  stubs: `java -version` reports no runtime and `xcodebuild -version` fails
  because only Command Line Tools are selected. The checker now fails both of
  these rather than passing a present-but-broken executable. **[prereq]**
- `xcode-select -p` → `/Library/Developer/CommandLineTools` (full Xcode not
  installed; `xcrun --sdk iphoneos` cannot locate an SDK). **[prereq]**
- `clang` 21.0.0, `make`, `swift`/`swiftc` 6.4, CocoaPods (`pod 1.16.2`) and
  Homebrew 7.0.9 are present and run successfully. **[prereq]**

Because there is no Rust toolchain, the task instruction "initialize and build a
mobile shell in an isolated scratch copy only" could not proceed past its first
step. The isolated-copy attempt and its exact failure are recorded next.

## What was actually executed

Run in an isolated `git clone --no-hardlinks` of this repository under
`$TMPDIR`, with `npm ci` installed inside the clone; the shared checkout was not
modified. The clone was at `bd08fe7`, and `native/` is unchanged from the
`b20e492` source baseline, so the line references below still apply. The commands
are exactly the clone recipe documented under "Exact commands for the next
spike", so that recipe is verified rather than aspirational. **[prereq]**

| Command (from the clone) | Result |
| --- | --- |
| `git clone --no-hardlinks <repo> "$SPIKE_DIR"` | clone created; `node_modules/` and `native/dist/` absent as expected |
| `npm ci` (clone root) | 455 packages added |
| `../node_modules/.bin/tauri --version` (cwd `native/`) | `tauri-cli 2.12.3` |
| `tauri android init --ci --skip-targets-install` (cwd `native/`) | **exit 1**, no `src-tauri/gen/` created: `failed to run 'cargo metadata' command to get workspace directory … No such file or directory (os error 2)` |
| `tauri ios init --ci --skip-targets-install` (cwd `native/`) | **exit 1**, identical `cargo metadata` error, no `src-tauri/gen/` |
| `node scripts/check-mobile-prerequisites.mjs android --json` | `4 passed, 0 warnings, 8 failures — exit 1 (prerequisites missing)` |
| `node scripts/check-mobile-prerequisites.mjs ios --json` | `6 passed, 0 warnings, 8 failures — exit 1 (prerequisites missing)` |
| `node scripts/check-mobile-prerequisites.mjs all --json` | `6 passed, 0 warnings, 12 failures — exit 1 (prerequisites missing)` |

Both `init` commands abort before generating anything because the Tauri CLI
resolves the Cargo workspace first. So the very first blocker is "install Rust",
and the documented findings below are from **source inspection plus official
docs**, not from a generated project. `tauri android/ios init` never reached the
frontend build hook, so the hook's working directory was verified from the
`tauri-cli 2.12.3` source (`run_hook` runs the command in `dirs.frontend`, which
resolves to the clone root because `native/` has no `package.json`) rather than
by execution.

Tested behavior of the checker (see the script header for exit codes 0–4).
Results are from the real host and from a stub-toolchain harness that builds a
fake `cargo`/`rustc`/`rustup`/`cmake`/`clang`/`java`/`xcodebuild`/`xcrun`/`pod`
plus a fake SDK/NDK tree:

- Real host: `android` → exit 1; `ios` → exit 1; `all` → exit 1. Valid `--json`
  on each. **[prereq]**
- Stubbed host with every required probe succeeding: `android`, `ios`, and `all`
  → exit 0. **[prereq]**
- Stubbed host with an advisory-only iOS target missing (Intel simulator target)
  → exit 0 with a `WARN` on `rust.targets.ios`. **[prereq]**
- Required tools present but broken: `cmake` exiting nonzero, `rustc` exiting
  nonzero, `rustc` exiting 0 with unparsable output, `clang` exiting nonzero, and
  `java` exiting nonzero each produce a `FAIL` and exit 1. **[prereq]**
- Missing NDK CMake toolchain (`android.toolchain.cmake` removed) → `FAIL` and
  exit 1 (previously a warning). **[prereq]**
- Missing project-local Tauri CLI (repository root without `node_modules`) →
  `project.tauri_cli` `FAIL` and exit 1. **[prereq]**
- Forced non-darwin host with every other check satisfied, `ios` → exit 3
  (unsupported host). **[prereq]**
- Invalid argument → exit 2; `--help` → exit 0. **[prereq]**

## Verdict

- **Android: source-level feasibility, blocked now on the toolchain and on one
  concrete code defect.** The dialog plugin returns `content://` URIs on Android
  while the Rust import path assumes a filesystem path, so import and model
  install cannot work as written (R1). Reading the dependency sources, the rest
  of the dependency set is *expected* to cross-compile — the crates are
  pure-Rust or build through CMake with the NDK toolchain — but this is a
  **source-based expectation, not a measured result**: no dependency has been
  compiled for a mobile target here, so every crate's Android cross-compile
  status remains unverified until `tauri android build` succeeds.
- **iOS: source-level feasibility on macOS, blocked now on the toolchain** (full
  Xcode + Rust). By default the dialog plugin copies picked documents into the
  app sandbox, so a `file://` path is returned and `into_path()` can succeed —
  but this must be confirmed on a device, and the "scoped" access mode would
  break it. Dependency cross-compilation is likewise unverified: there is no
  `[compile]` evidence for iOS either.

Neither platform is advertised as supported. Full mobile delivery stays in P4.

## Minimum toolchains supported by the sources

Floors are taken from the files that actually constrain the build, not guesses:

| Component | Minimum / requirement | Source |
| --- | --- | --- |
| Node.js | **22.12.0** | `package.json` → `engines.node` **[source]** |
| Tauri CLI | project-local `@tauri-apps/cli` (via `npm ci`) | required to run `tauri android/ios init`; the checker fails when `node_modules/@tauri-apps/cli` or its `.bin/tauri` shim is absent **[source] [prereq]** |
| Rust | **1.90.0** (edition 2024) | `tauri 2.12.4` and `tauri-plugin-dialog 2.8.3` `rust-version` on crates.io; Tauri dialog docs "requires at least 1.90" **[source]** |
| Rust (engine floor) | 1.88.0 | `whisper-rs-sys 0.15.0` `rust-version` (lower than Tauri) **[source]** |
| CMake + `clang`/libclang + C/C++ compiler | required | `whisper-rs-sys 0.15.0` `build.rs` uses `cmake::Config` and `bindgen` **[source]** |
| JDK | **17** | Android Gradle Plugin 8.x requires JDK 17 ([developer.android.com/build/jdks](https://developer.android.com/build/jdks)) **[source]** |
| Android SDK | Platform, Build-Tools, Platform-Tools, Command-line Tools, NDK (Side by side) | Tauri prerequisites **[source]** |
| Android Rust targets | `aarch64-linux-android`, `armv7-linux-androideabi`, `i686-linux-android`, `x86_64-linux-android` | Tauri prerequisites **[source]** |
| Android min SDK | API **24** (Tauri default `minSdkVersion`) | `@tauri-apps/cli/config.schema.json` → `bundle.android.minSdkVersion` **[source]** |
| Full Xcode | not just Command Line Tools | Tauri prerequisites; also `xcodebuild -version` on this host **[source] [prereq]** |
| iOS SDKs | iPhoneOS **and** iPhoneSimulator | needed by `tauri ios build --target aarch64-sim`; `tauri ios build --help` lists `aarch64`, `aarch64-sim`, `x86_64` **[prereq]** |
| iOS Rust targets | device: `aarch64-apple-ios`; simulator: `aarch64-apple-ios-sim` on Apple Silicon hosts or `x86_64-apple-ios` on Intel hosts | Tauri prerequisites; `x86_64-apple-ios` is also the legacy x86 device target **[source]** |
| iOS minimum system | **15.0** (Tauri default `minimumSystemVersion`) | config schema → `bundle.iOS.minimumSystemVersion` **[source]** |
| CocoaPods + Homebrew | required / recommended | Tauri iOS prerequisites **[source]** |
| Gradle | not required on `PATH` | Tauri generates a Gradle wrapper during `android init` **[source]** |

Node, Rust/cargo/rustc, CMake, clang, and the Rust targets are required for
**both** platforms; the checker reports them once under `[rust]`.

On iOS, `aarch64-apple-ios` is the arm64 **device** target and stays required
for device builds. The simulator target must match the host CPU:
`aarch64-apple-ios-sim` on Apple Silicon, `x86_64-apple-ios` on Intel. The
checker requires the device target plus the host-appropriate simulator target and
treats the other simulator target as advisory, so an Intel Mac is not failed for
lacking the arm64 simulator target (and vice versa).

## Source-inspected mobile risks

Line references are under `native/src-tauri/` at commit `b20e492`.

### R1 — Android document URIs break import (blocking)

`import_audio` picks a file then calls `file.into_path()` (`lib.rs:90`);
`import_model` does the same (`lib.rs:137`). `FilePath::into_path()` delegates to
`Url::to_file_path()`, which only converts `file://` URLs. On Android the dialog
plugin returns a `content://` URI string (`plugins/dialog/android/.../DialogPlugin.kt`
→ `createPickFilesResult`, using `Intent.ACTION_GET_CONTENT`), so `into_path()`
returns `Err` and both commands fail before doing any work. Even if it returned,
`import.rs:173` and `lib.rs:142` read the source with `std::fs`, which cannot open
a `content://` URI. The Tauri dialog docs confirm: "On Android, content URIs are
returned" and the filesystem plugin "works with any path format out of the box".

Export is a separate and weaker claim. `export_transcript`
(`lib.rs:268`) writes its result with `std::fs::write`, which can only write a
real filesystem path. Whether Android's save dialog is available at all, and — if
it is — what it returns (a `content://` URI or a provider-resolved path), is
**not verified on a device**; this is inferred from the Android dialog plugin
source, not observed. If it returns a `content://` URI then `std::fs::write`
cannot write it, but the returned-destination behavior is unproven. Separately,
the `.file_stem()` used for the note title (`lib.rs:98`) is meaningless for a
URI; a display name must come from `ContentResolver`
(`OpenableColumns.DISPLAY_NAME`).

**Recommended direction (not implemented here — Rust/Cargo are out of this
task's scope):** resolve the URI to a real file, e.g. copy the stream into the
app cache/data directory with a small JNI/`ContentResolver` helper, or adopt the
`tauri-plugin-fs` path handling that the docs say accepts any path format. Do not
assume a picked Android "path" is `std::fs`-readable.

### R2 — iOS document-provider handling (verify on device)

The dialog plugin presents `UIDocumentPickerViewController(forOpeningContentTypes:asCopy:)`
where `asCopy = (file_access_mode != .scoped)` (`plugins/dialog/ios/.../DialogPlugin.swift`).
Golos never sets a file access mode, so the default is **copy**: the picked file
is placed inside the app sandbox and a `file://` URL is returned, which
`into_path()` can convert and `std::fs` can read. The photo/media pickers
(`PHPicker`/`UIImagePickerController`) always copy to the sandbox.

Risk: if a future change sets the **scoped** access mode, the plugin does *not*
call `startAccessingSecurityScopedResource`, and the returned URL may be
unreadable. Also unverified: the iOS save flow creates a placeholder file in the
app `Documents` directory and exports it via `UIDocumentPickerViewController(...,
in: .exportToService)`, returning a destination URL the app is then expected to
write to with `std::fs::write` — that destination may live in another provider.

**Recommended direction:** keep copy mode for import, verify on a real device, and
for large models consider scoped access + a security-scoped bookmark instead of
copying ~190 MB.

### R3 — Import creates audio before its DB row (already known, worse on mobile)

`lib.rs:95-112` writes the normalized WAV before `library::insert`. Mobile
processes are killed aggressively in the background, so the P1 recovery
protocols in `OFFLINE_FIRST_PLAN.md` matter more here, not less.

### R4 — Whole-file inference memory

`audio::read_audio` returns the entire normalized recording as `Vec<f32>`
(`audio.rs:5`, `engine.rs:40`) on top of the model's working set. At the 30-minute
cap (`audio.rs:3`) that is ~28.8 M samples ≈ **115 MB** of `f32` before the model,
plus whisper.cpp buffers and the model weights (Tiny~75 MB, Small q5_1 ~190 MB on
disk with larger runtime buffers). This is the main low-memory-device risk and the
reason the plan defers Small-on-every-phone. **[source]**

### R5 — CPU-only inference, unknown latency/battery

The engine forces CPU (`use_gpu(false)`, `engine.rs:44-45`) and uses
`min(available_parallelism, 8)` threads (`engine.rs:51-55`). No Mobile/Metal
acceleration is enabled; `whisper-rs-sys` explicitly disables Metal for Apple
targets and has no default mobile GPU path. Real-time factor, thermals and battery
are unmeasured and must be measured per device (see the measurement protocol).

### R6 — Storage and SQLite

`AppState` stores `library.sqlite3`, `audio/`, and `model.bin` under
`app_data_dir()` (`lib.rs:291-296`). `rusqlite` is built with the `bundled`
feature (`Cargo.toml:19`), compiling SQLite C sources — fine with the NDK, but it
must actually be cross-compiled. `model.pending` is removed on startup
(`lib.rs:296`), assuming a writable data dir. **[source]**

### R7 — Desktop-only plugin already excluded

`tauri-plugin-single-instance` is gated to non-mobile targets
(`Cargo.toml:32-33`, `lib.rs:282-288`) and the crate declares
`crate-type = ["staticlib", "cdylib", "rlib"]` (`Cargo.toml:9`), both of which
mobile requires. The mobile entry point attribute is present (`lib.rs:276-277`).
These are the correct mobile prerequisites and need no change. **[source]**

### R8 — Capabilities and bundle config

`capabilities/main.json` declares no `platforms` array, so it applies to **all**
targets including mobile, and targets window `main` (the mobile default label).
Its `$schema` points at the desktop schema, which is cosmetic only. The mobile
icon sets exist (`src-tauri/icons/android`, `src-tauri/icons/ios`). No
`bundle.android`/`bundle.iOS` overrides exist, so API 24 / iOS 15.0 defaults
apply. `"targets": ["app"]` (`tauri.conf.json:18`) is desktop-only and does not
affect `tauri android/ios build`. **[source]**

### R9 — UI and lifecycle

The UI has a single `@media (max-width: 900px)` breakpoint, but the window is
configured 1180×780 with `minWidth 760` (`tauri.conf.json:13`) and there is no
touch/safe-area handling. More importantly, the unsaved-edit guard relies on
`getCurrentWindow().onCloseRequested` (`App.tsx:53`) and `beforeunload`
(`App.tsx:47`) — neither maps to Android's back gesture or to the app being
backgrounded/killed, so edits could be lost. Recording, playback, search and DOCX
export are absent entirely, which is fine for a spike but means "import →
transcribe → export" is the only mobile-relevant path today. **[source]**

### R10 — Dialog threading on mobile

`import_audio`/`import_model`/`export_transcript` call `blocking_pick_file` /
`blocking_save_file` on a `spawn_blocking` worker (`lib.rs:80,127,258`). The mobile
plugin implementation runs the picker on its own `std::thread` and resolves
through `run_mobile_plugin` (`plugins/dialog/src/mobile.rs`), so the blocking
pattern is tolerated, but the interaction between a blocking Rust worker and the
Android/iOS main-thread UI callback is worth verifying on device. **[source]**

### R11 — Dependency cross-compilation notes

- `whisper-rs-sys 0.15.0` links `c++_shared` on Android and `Accelerate` on Apple
  targets, and builds whisper.cpp via CMake (`build.rs`). Cross-compiling needs
  `CMAKE_TOOLCHAIN_FILE=$NDK/build/cmake/android.toolchain.cmake` (plus
  `ANDROID_ABI`/`ANDROID_PLATFORM`) and, for bindgen, a working libclang;
  `WHISPER_DONT_GENERATE_BINDINGS=1` reuses the crate's pre-generated bindings
  when bindgen cannot run for the target. **[source]**
- `symphonia 0.5.5`, `rubato 0.16.2`, and `hound 3.5.1` are pure Rust and are not
  expected to need platform-specific work. **[source]**

Every statement in this section is source-read. None of these dependencies has
been compiled for a mobile target in this pass — there is **no `[compile]`
evidence**, so all Android/iOS cross-compile status is unverified.

## Android document URI handling (detail)

1. `import_audio` calls `app.dialog().file().add_filter(...).blocking_pick_file()`
   (`lib.rs:82-87`), which on Android runs `ACTION_GET_CONTENT` with
   `CATEGORY_OPENABLE`.
2. The plugin returns `files: ["content://…"]` (the URI string, never a resolved
   path — `FilePickerUtils.getPathFromUri` exists but is not used for the result).
3. `FilePath::Url` → `into_path()` → `Url::to_file_path()` fails for a
   `content://` scheme.
4. Result: `import_audio` returns `Err`; `import_model` returns `Err`. Export is
   *not* confirmed: if Android's save dialog returns a `content://` URI then
   `fs::write` cannot write it, but the save-dialog path has not been exercised
   on a device.

Additional consequences:

- The model filter `["bin"]` has no Android MIME mapping, so the picker falls back
  to `*/*` and cannot restrict to GGML files. The 512 MiB / family validation in
  `model.rs` still applies after a stream copy.
- The `file_stem()` title (`lib.rs:98`) must be replaced by a
  `ContentResolver` display-name lookup.

Next spike must decide between (a) a tiny Rust/JNI resolver that copies the stream
into app storage, or (b) the `tauri-plugin-fs` path abstraction the Tauri docs
recommend, then re-run import/model-install on a device.

## iOS document-provider handling (detail)

- Default picker (no file-access mode) → `asCopy = true` → sandboxed `file://` →
  `into_path()` OK, `std::fs` OK. Verify on device, including a large model copy.
- Scoped mode → `asCopy = false` → security-scoped URL → the plugin does not call
  `startAccessingSecurityScopedResource` → reading may fail. Avoid scoped mode
  until that is implemented and tested.
- Save/export → placeholder file in `Documents` + `.exportToService`; the app then
  writes to the returned URL. Writing into another provider's container from
  `std::fs` is unverified.
- Device installs require code signing / a development team; simulator builds are
  unsigned. `bundle.iOS.developmentTeam` is unset, so device runs need
  `APPLE_DEVELOPMENT_TEAM` or a config value.

## Offline EN/UK import + inference fixtures and the hash-recording protocol

Fixtures are **never committed** (`.gitignore` excludes `native/**/*.bin` and no
audio is tracked). The protocol below makes a run reproducible from recorded
provenance instead.

**Model artifact identity.** For each model used, record:

| Field | How |
| --- | --- |
| File name | e.g. `ggml-small-q5_1.bin` |
| Source URL | upstream `ggml-org/whisper.cpp` / Hugging Face URL and revision |
| SHA-256 | `shasum -a 256 <model>` (Linux: `sha256sum`) |
| Byte size | `stat -f%z <model>` / `stat -c%s` |
| GGML family | `model::inspect` output (reads magic `0x67676d6c` and family word; `model.rs:6-31`) |
| Quantization | from the file name |

**Audio fixtures.** EN: the public `whisper.cpp` JFK sample
(`samples/jfk.wav`) already used by the ignored native test. UK: a
public-domain/CC0 or explicitly consented Ukrainian speech sample with its
license recorded — **do not use personal audio**. Record SHA-256, duration,
sample rate, channel count, and source URL for each fixture, and generate the
compressed-format variants with `scripts/create-audio-fixtures.py` (FFmpeg is
test tooling only).

**Per-run record** (one row per model × language × fixture × device):

```
commit=<b20e492>
host_os_arch=<macOS 27.0.1 arm64 | Android <ver>/<device> | iOS <ver>/<device>>
toolchain=<rustc/cargo/NDK/Xcode versions>
model=<file> sha256=<hex> bytes=<n> family=<Tiny|Base|Small> quant=<q5_1|...>
audio=<file> sha256=<hex> duration_s=<n> sample_rate=<n> channels=<n> license=<...>
language=<auto|en|uk>
engine=<whisper-rs-> use_gpu=false threads=<n> strategy=greedy(best_of=1) duration_cap_s=1800
result=ok|failed|cancelled
transcript_sha256=<hex of the saved transcript text>
segments=<count>
rtf=<wall_seconds / audio_seconds>   # processing time ÷ audio duration; <1 is faster than real time
```

Determinism caveats to record with results: greedy decoding with a fixed thread
count is close to, but not guaranteed, bit-identical across runs; record the thread
count and note any transcript diff rather than asserting equality.

## Measurements still needed (not measured here)

| Measurement | Suggested method | Why |
| --- | --- | --- |
| Real-time factor | wall-clock around `engine::transcribe`; RTF = wall seconds ÷ audio seconds (processing time ÷ audio duration; <1 is faster than real time) | the only honest per-device speed figure |
| Peak RSS | macOS `time -l` / Instruments; Linux `/usr/bin/time -v`; Android `dumpsys meminfo <pkg>`; iOS Instruments | check R4 against device RAM |
| Model load time | time `WhisperContext::new_with_params` | model import UX and R3 |
| Cancellation latency | time from `cancel_transcription` to `status='failed'` with the "cancelled" message | the plan's cancellation gate |
| Thermal/battery | device instrumentation during a 30-minute run | R5 |
| Import throughput | time `import::normalize` on each codec | mobile storage/decode cost |
| Storage footprint | size of `library.sqlite3` + `audio/` + `model.bin` | low-storage devices |

Record all of the above with the per-run record above so a reviewer can
distinguish device evidence from source inspection.

## Exact commands for the next spike

Prerequisite install is a **global** action that this task deliberately did not
perform. On the spike machine:

```sh
# --- one-time host setup (global; not run by this task) -------------------
# Rust (both platforms)
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh   # then reopen the shell
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
# iOS device target plus the simulator target for THIS host:
#   Apple Silicon -> aarch64-apple-ios-sim ; Intel -> x86_64-apple-ios
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
brew install cmake cocoapods            # macOS; CMake is also needed by whisper-rs-sys

# Android: install Android Studio, then SDK Platform, Platform-Tools, Build-Tools,
# Command-line Tools, and "NDK (Side by side)"; install JDK 17+ and export:
export JAVA_HOME=/path/to/jdk-17
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/$(ls -1 "$ANDROID_HOME/ndk" | tail -1)"

# iOS: install full Xcode (not only Command Line Tools), open it once, then:
sudo xcode-select -s /Applications/Xcode.app

# --- isolated spike checkout (run from the repository root) ----------------
node scripts/check-mobile-prerequisites.mjs all      # expect exit 0
SPIKE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/golos-mobile-spike.XXXXXX")"
git clone --no-hardlinks "$PWD" "$SPIKE_DIR"         # committed state only
cd "$SPIKE_DIR"
npm ci                                               # installs the project-local Tauri CLI
cd native                                            # Tauri resolves src-tauri/ from here
../node_modules/.bin/tauri android init --ci         # installs rust targets unless --skip-targets-install
../node_modules/.bin/tauri ios init --ci
```

The clone is a self-contained copy of the repository: `native/` keeps its
`../node_modules` (the project-local Tauri CLI), and the `beforeBuildCommand`
(`npm run native:build`) resolves from the clone root because `native/` has no
`package.json` of its own. Do not copy `node_modules/` or `native/dist/` around;
let `npm ci` and the Tauri build hook recreate them. The shared checkout is left
untouched.

Build attempts (first real compile evidence) and the code fixes from R1/R2 must
land before any platform is described as working. Continue in the same clone,
with `native/` as the working directory:

```sh
cd "$SPIKE_DIR/native"
# Android (needs NDK cross-compile env for whisper-rs-sys)
../node_modules/.bin/tauri android build --debug --apk --target aarch64
# iOS simulator (unsigned); use --target x86_64 instead on an Intel host
../node_modules/.bin/tauri ios build --debug --target aarch64-sim
```

Until `tauri … init` succeeds, any `android`/`ios` result is a prerequisite
result, not a build result.

## About the optional CI workflow

`.github/workflows/mobile-feasibility.yml` is **manual only**
(`workflow_dispatch`). The `platform` input is honored through a `select` job that
builds the prerequisite matrix: `android` runs only the Ubuntu leg, `ios` only
the macOS leg, and `all` runs both. Each leg installs JavaScript dependencies and
runs the tested checker, uploading the JSON report without gating. The
`ios-simulator-build` job runs only when the selected platform includes iOS **and**
`run_ios_simulator_build` is true; it is **unverified** — it has never run, and a
green run would be the first iOS compile evidence and must be recorded as such.
It is not a device or store claim. The desktop workflows and `native/README.md`
are owned by another agent and are not touched here.

## References

- Tauri prerequisites (Android/iOS toolchains and Rust targets): <https://v2.tauri.app/start/prerequisites/>
- Tauri dialog plugin (Android content URIs, iOS `file://`, filesystem plugin handles any path): <https://v2.tauri.app/plugin/dialog/>
- `tauri-plugin-dialog` `FilePath` / `into_path` (uses `Url::to_file_path`): <https://docs.rs/tauri-plugin-dialog/2.8.3/tauri_plugin_dialog/enum.FilePath.html>
- Tauri capabilities (`platforms`, mobile): <https://v2.tauri.app/security/capabilities/>
- Android Gradle Plugin / JDK 17 requirement: <https://developer.android.com/build/jdks>
- Android document provider content URIs: <https://developer.android.com/guide/topics/providers/content-provider-basics>
- `whisper-rs-sys 0.15.0` build requirements (CMake/bindgen): <https://docs.rs/crate/whisper-rs-sys/0.15.0/source/build.rs>
