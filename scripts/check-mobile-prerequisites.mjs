#!/usr/bin/env node
// scripts/check-mobile-prerequisites.mjs
//
// Zero-secret, standard-library-only prerequisite checker for the Golos Tauri
// Android/iOS feasibility spikes. It inspects the *host* toolchains required to
// initialize and compile a mobile shell and reports what is missing. It never
// installs anything, never reads secrets, and makes no network calls. A passing
// run means "this host has the tools the official Tauri docs list" with each
// required probe actually running successfully (a present-but-broken executable
// counts as missing). It is not a compile, device, or store-validation claim.
//
// Usage:
//   node scripts/check-mobile-prerequisites.mjs [android|ios|all] [--json] [--no-color]
//
// Exit codes:
//   0  All selected prerequisite checks passed.
//   1  One or more required prerequisites are missing or unusable.
//   2  Invalid command-line usage.
//   3  A selected target cannot be built on this host OS (platform guard),
//      and no other prerequisite check failed.
//   4  Unexpected internal error while running the checker.
//
// See docs/MOBILE_FEASIBILITY.md for the requirement sources and the evidence
// level behind each check.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, '..');

const EXIT = { READY: 0, PREREQUISITES: 1, USAGE: 2, UNSUPPORTED_HOST: 3, INTERNAL: 4 };

/** Minimum tool versions, each with the source that justifies the floor. */
const MINIMUMS = {
  // package.json "engines". The native frontend also needs this to build.
  node: { major: 22, minor: 12, patch: 0, source: 'package.json engines.node >=22.12.0' },
  // tauri 2.12.4 and tauri-plugin-dialog 2.8.3 declare rust-version = 1.90
  // (crates.io metadata); edition 2024.
  rust: { major: 1, minor: 90, patch: 0, source: 'tauri 2.12.4 / tauri-plugin-dialog 2.8.3 rust-version' },
  // whisper-rs-sys 0.15.0 declares rust-version = 1.88.0 (lower than tauri).
  whisperRsSys: { major: 1, minor: 88, patch: 0, source: 'whisper-rs-sys 0.15.0 rust-version' },
  // Android Gradle Plugin 8.x requires JDK 17 (developer.android.com/build/jdks).
  jdk: { major: 17, source: 'Android Gradle Plugin 8.x requires JDK 17' },
  // Tauri config schema default for bundle.android.minSdkVersion.
  androidMinSdk: 24,
  // Tauri config schema default for bundle.iOS.minimumSystemVersion.
  iosMinSystem: '15.0',
};

// Rust targets the official Tauri macOS/Linux/Windows prerequisite docs add for
// each mobile platform. `required` targets block the spike; `advisory` targets
// cover older/less common devices or the other simulator architecture and are
// only warnings. The arm64 iOS device target is always `aarch64-apple-ios`; the
// simulator target follows the host CPU: Apple Silicon simulators are arm64
// (`aarch64-apple-ios-sim`), Intel simulators are x86_64 (`x86_64-apple-ios`,
// which is also the legacy x86 device target).
const RUST_TARGETS = {
  android: {
    required: ['aarch64-linux-android', 'x86_64-linux-android'],
    advisory: ['armv7-linux-androideabi', 'i686-linux-android'],
  },
};

function iosTargetSpec() {
  const hostIsIntel = process.arch === 'x64';
  const sim = hostIsIntel ? 'x86_64-apple-ios' : 'aarch64-apple-ios-sim';
  const otherSim = hostIsIntel ? 'aarch64-apple-ios-sim' : 'x86_64-apple-ios';
  return { required: ['aarch64-apple-ios', sim], advisory: [otherSim], hostNote: `host ${process.arch}, simulator target ${sim}` };
}

const COLOR = process.stdout.isTTY && !process.env.NO_COLOR && !process.argv.includes('--no-color');
const paint = (code, text) => (COLOR ? `\u001b[${code}m${text}\u001b[0m` : text);
const STYLE = {
  pass: (t) => paint('32', t),
  warn: (t) => paint('33', t),
  fail: (t) => paint('31', t),
  info: (t) => paint('36', t),
  skip: (t) => paint('90', t),
  bold: (t) => paint('1', t),
};
const MARK = { pass: 'PASS', warn: 'WARN', fail: 'FAIL', info: 'INFO', skip: 'SKIP' };

const checks = [];

const oneLine = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();

function add(section, id, label, status, detail, hint) {
  checks.push({ section, id, label, status, detail: oneLine(detail), hint: hint ?? null, hostGuard: false });
}

function addHostGuard(section, id, label, detail, hint) {
  checks.push({ section, id, label, status: 'fail', detail, hint: hint ?? null, hostGuard: true });
}

// ---------------------------------------------------------------------------
// Command inspection helpers (no third-party dependencies).
// ---------------------------------------------------------------------------

function resolveCommand(name) {
  const isWin = process.platform === 'win32';
  const exts = isWin ? (process.env.PATHEXT || '.COM;.EXE;.BAT;.CMD').split(';') : [''];
  const dirs = (process.env.PATH || '').split(isWin ? ';' : path.delimiter);
  for (const dir of dirs) {
    if (!dir) continue;
    for (const ext of exts) {
      const candidate = path.join(dir, name + ext);
      try {
        if (fs.statSync(candidate).isFile() || (isWin && fs.statSync(candidate).isSymbolicLink())) {
          return candidate;
        }
      } catch {
        /* keep looking */
      }
    }
  }
  return null;
}

function run(cmd, args = [], { timeout = 15000 } = {}) {
  const resolved = resolveCommand(cmd);
  if (!resolved) return { found: false, ok: false, code: null, output: '', error: 'not found on PATH' };
  const spawnOptions = {
    encoding: 'utf8',
    timeout,
    windowsHide: true,
    maxBuffer: 4 * 1024 * 1024,
  };
  let result;
  if (process.platform === 'win32' && /\.(cmd|bat)$/i.test(resolved)) {
    // Batch shims (for example npm.cmd) cannot be spawned directly on Windows
    // since Node 18.20/20.12; route them through the command interpreter. The
    // required tools are normally .exe files, so this only matters for scripts.
    const comspec = process.env.ComSpec || 'cmd.exe';
    const commandLine = [resolved, ...args].map((part) => `"${String(part).replace(/"/g, '""')}"`).join(' ');
    result = spawnSync(comspec, ['/d', '/s', '/c', commandLine], spawnOptions);
  } else {
    result = spawnSync(resolved, args, spawnOptions);
  }
  const output = `${result.stdout || ''}${result.stderr || ''}`.trim();
  return {
    found: true,
    ok: !result.error && result.status === 0,
    code: result.status,
    output,
    error: result.error ? String(result.error.message || result.error) : null,
  };
}

const firstMatch = (text, regex) => {
  const match = regex.exec(text || '');
  return match || null;
};

// Lexicographic major.minor.patch comparison. The previous mixed `&&`/`||`
// expression was easy to misread; this returns true when `actual >= minimum`.
const gte = (actual, minimum) => {
  const a = [actual.major ?? 0, actual.minor ?? 0, actual.patch ?? 0];
  const b = [minimum.major ?? 0, minimum.minor ?? 0, minimum.patch ?? 0];
  for (let i = 0; i < b.length; i += 1) {
    if (a[i] !== b[i]) return a[i] > b[i];
  }
  return true;
};

const fmt = (v) => [v.major, v.minor, v.patch].filter((n) => n !== undefined).join('.');

function existingDir(p) {
  try {
    return fs.statSync(p).isDirectory();
  } catch {
    return false;
  }
}

function readJson(p) {
  try {
    return JSON.parse(fs.readFileSync(p, 'utf8'));
  } catch {
    return null;
  }
}

function dirsIn(p) {
  try {
    return fs
      .readdirSync(p, { withFileTypes: true })
      .filter((e) => e.isDirectory())
      .map((e) => e.name)
      .sort();
  } catch {
    return [];
  }
}

// ---------------------------------------------------------------------------
// Shared checks
// ---------------------------------------------------------------------------

function checkHost() {
  const platform = process.platform;
  const pretty =
    platform === 'darwin' ? 'macOS' : platform === 'win32' ? 'Windows' : platform === 'linux' ? 'Linux' : platform;
  add('host', 'host.os', 'Host operating system', 'info', `${pretty} (${process.arch}), Node ${process.version}`);
}

function checkNode() {
  const min = MINIMUMS.node;
  const [major, minor, patch] = process.versions.node.split('.').map(Number);
  const actual = { major, minor, patch };
  if (gte(actual, min)) {
    add('host', 'node.version', 'Node.js version', 'pass', `${fmt(actual)} (needs >=${fmt(min)})`);
  } else {
    add(
      'host',
      'node.version',
      'Node.js version',
      'fail',
      `${fmt(actual)} is older than the required ${fmt(min)}`,
      `Upgrade Node (${min.source}).`,
    );
  }
}

function checkRust() {
  const cargo = run('cargo', ['--version']);
  if (!cargo.found) {
    add('rust', 'rust.cargo', 'Cargo', 'fail', 'cargo is not on PATH', 'Install a Rust toolchain via rustup (https://rustup.rs).');
  } else {
    add('rust', 'rust.cargo', 'Cargo', cargo.ok ? 'pass' : 'fail', cargo.output || `exit ${cargo.code}`, cargo.ok ? null : 'Run `cargo --version` and check the Rust installation.');
  }

  const rustc = run('rustc', ['--version']);
  if (!rustc.found) {
    add('rust', 'rust.rustc', 'rustc (MSRV)', 'fail', 'rustc is not on PATH', 'Install a Rust toolchain via rustup.');
  } else if (!rustc.ok) {
    add('rust', 'rust.rustc', 'rustc (MSRV)', 'fail', rustc.output || `rustc --version exited with code ${rustc.code}`, 'rustc exists but does not run; repair the Rust toolchain via rustup.');
  } else {
    const match = firstMatch(rustc.output, /rustc\s+(\d+)\.(\d+)\.(\d+)/);
    if (!match) {
      add('rust', 'rust.rustc', 'rustc (MSRV)', 'fail', rustc.output || 'unparsed version', 'rustc did not report a version in the expected format; reinstall the toolchain via rustup.');
    } else {
      const actual = { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3]) };
      const min = MINIMUMS.rust;
      if (gte(actual, min)) {
        add('rust', 'rust.rustc', 'rustc (MSRV)', 'pass', `${actual.major}.${actual.minor}.${actual.patch} (needs >=${fmt(min)})`);
      } else {
        add(
          'rust',
          'rust.rustc',
          'rustc (MSRV)',
          'fail',
          `${actual.major}.${actual.minor}.${actual.patch} is below the required ${fmt(min)}`,
          `Update rustc (${min.source}).`,
        );
      }
    }
  }

  const rustup = run('rustup', ['--version']);
  const rustupDetail = rustup.ok ? rustup.output.split('\n')[0] : rustup.found ? rustup.output || `rustup --version exited with code ${rustup.code}` : 'rustup is not on PATH';
  add('rust', 'rust.rustup', 'rustup', rustup.ok ? 'pass' : 'fail', rustupDetail, rustup.ok ? null : 'rustup is required to install and enumerate the per-target standard libraries.');

  const cmake = run('cmake', ['--version']);
  if (!cmake.ok) {
    add('rust', 'rust.cmake', 'CMake', 'fail', cmake.found ? cmake.output || `cmake --version exited with code ${cmake.code}` : 'cmake is not on PATH', 'whisper-rs-sys 0.15.0 builds whisper.cpp through the cmake crate; install a working CMake and put it on PATH.');
  } else {
    const match = firstMatch(cmake.output, /cmake version\s+(\d+)\.(\d+)\.(\d+)/);
    if (match) add('rust', 'rust.cmake', 'CMake', 'pass', `cmake ${match[1]}.${match[2]}.${match[3]}`);
    else add('rust', 'rust.cmake', 'CMake', 'fail', cmake.output || 'unparsed version', 'cmake did not report a version in the expected format; reinstall CMake.');
  }

  const clang = run('clang', ['--version']);
  const clangDetail = clang.ok ? clang.output.split('\n')[0] : clang.found ? clang.output || `clang --version exited with code ${clang.code}` : 'clang is not on PATH';
  add('rust', 'rust.clang', 'C/C++ compiler (clang)', clang.ok ? 'pass' : 'fail', clangDetail, clang.ok ? 'bindgen needs libclang; set LIBCLANG_PATH if the build cannot find it.' : 'whisper-rs-sys uses bindgen (needs libclang) and compiles C/C++ sources; install a working clang.');

  add(
    'rust',
    'rust.libclang_path',
    'LIBCLANG_PATH',
    process.env.LIBCLANG_PATH ? 'info' : 'info',
    process.env.LIBCLANG_PATH || 'not set (usually fine when clang ships libclang)',
  );

  const ninja = run('ninja', ['--version']);
  add('rust', 'rust.ninja', 'Ninja (optional)', ninja.ok ? 'info' : 'skip', ninja.ok ? ninja.output.split('\n')[0] : ninja.found ? 'installed but not usable' : 'not installed');
}

function checkRustTargets(target) {
  const spec = target === 'ios' ? iosTargetSpec() : RUST_TARGETS.android;
  const list = run('rustup', ['target', 'list', '--installed']);
  if (!list.found || !list.ok) {
    add(
      target,
      `rust.targets.${target}`,
      `${target} Rust targets`,
      'fail',
      list.found ? list.output || `exit ${list.code}` : 'rustup is not available to enumerate targets',
      `Install rustup, then: rustup target add ${[...spec.required, ...spec.advisory].join(' ')}`,
    );
    return;
  }
  const installed = new Set(list.output.split(/\s+/).filter(Boolean));
  const missingRequired = spec.required.filter((t) => !installed.has(t));
  const missingAdvisory = spec.advisory.filter((t) => !installed.has(t));
  const hostNote = spec.hostNote ? ` (${spec.hostNote})` : '';
  if (missingRequired.length === 0 && missingAdvisory.length === 0) {
    add(target, `rust.targets.${target}`, `${target} Rust targets`, 'pass', `installed: ${spec.required.concat(spec.advisory).join(', ')}${hostNote}`);
    return;
  }
  const detail = [
    missingRequired.length ? `missing required: ${missingRequired.join(', ')}` : null,
    missingAdvisory.length ? `missing advisory: ${missingAdvisory.join(', ')}` : null,
  ]
    .filter(Boolean)
    .join('; ');
  const hint = `rustup target add ${[...missingRequired, ...missingAdvisory].join(' ')}`;
  add(
    target,
    `rust.targets.${target}`,
    `${target} Rust targets`,
    missingRequired.length ? 'fail' : 'warn',
    `${detail}${hostNote}`,
    hint,
  );
}

function checkTauriCli() {
  const pkgPath = path.join(REPO_ROOT, 'node_modules', '@tauri-apps', 'cli', 'package.json');
  const pkg = readJson(pkgPath);
  const bin = ['tauri', 'tauri.cmd', 'tauri.exe']
    .map((name) => path.join(REPO_ROOT, 'node_modules', '.bin', name))
    .find((candidate) => fs.existsSync(candidate));
  if (!pkg?.version || !bin) {
    const detail = pkg?.version ? `@tauri-apps/cli ${pkg.version} is installed but no node_modules/.bin/tauri shim was found` : 'node_modules/@tauri-apps/cli is not installed';
    add('project', 'project.tauri_cli', 'Tauri CLI', 'fail', detail, 'Run `npm ci` from the repository root; `tauri android/ios init` cannot run without the project-local CLI.');
    return;
  }
  add('project', 'project.tauri_cli', 'Tauri CLI', 'pass', `@tauri-apps/cli ${pkg.version}`);
}

function checkProjectConfig() {
  const confPath = path.join(REPO_ROOT, 'native', 'src-tauri', 'tauri.conf.json');
  const conf = readJson(confPath);
  if (!conf) {
    add('project', 'project.tauri_conf', 'tauri.conf.json', 'fail', `cannot read/parse ${confPath}`);
    return;
  }
  const problems = [];
  if (!conf.identifier) problems.push('missing identifier');
  if (!conf.build?.frontendDist) problems.push('missing build.frontendDist');
  add(
    'project',
    'project.tauri_conf',
    'tauri.conf.json',
    problems.length ? 'fail' : 'pass',
    problems.length ? problems.join(', ') : `identifier ${conf.identifier}`,
  );

  const androidIcons = existingDir(path.join(REPO_ROOT, 'native', 'src-tauri', 'icons', 'android'));
  const iosIcons = existingDir(path.join(REPO_ROOT, 'native', 'src-tauri', 'icons', 'ios'));
  add(
    'project',
    'project.mobile_icons',
    'Mobile icon sets',
    androidIcons && iosIcons ? 'info' : 'warn',
    `android icons: ${androidIcons ? 'present' : 'missing'}, ios icons: ${iosIcons ? 'present' : 'missing'}`,
    androidIcons && iosIcons ? null : 'Run `npm run tauri icon <source>` to (re)generate platform icons.',
  );

  const gen = path.join(REPO_ROOT, 'native', 'src-tauri', 'gen');
  add(
    'project',
    'project.mobile_project',
    'Mobile projects initialized',
    existingDir(gen) ? 'info' : 'info',
    existingDir(gen) ? 'src-tauri/gen exists' : 'src-tauri/gen is absent (run `tauri android init` / `tauri ios init`)',
  );

  const libRs = path.join(REPO_ROOT, 'native', 'src-tauri', 'src', 'lib.rs');
  let hasEntryPoint = false;
  try {
    hasEntryPoint = /mobile_entry_point/.test(fs.readFileSync(libRs, 'utf8'));
  } catch {
    hasEntryPoint = false;
  }
  add(
    'project',
    'project.mobile_entry_point',
    'Mobile entry point attribute',
    hasEntryPoint ? 'info' : 'warn',
    hasEntryPoint ? '#[cfg_attr(mobile, tauri::mobile_entry_point)] present' : 'not found in native/src-tauri/src/lib.rs',
    hasEntryPoint ? null : 'Tauri mobile builds require the mobile entry point attribute in the Rust crate.',
  );
}

// ---------------------------------------------------------------------------
// Android checks
// ---------------------------------------------------------------------------

function findAndroidSdk() {
  const candidates = [];
  if (process.env.ANDROID_HOME) candidates.push(process.env.ANDROID_HOME);
  if (process.env.ANDROID_SDK_ROOT) candidates.push(process.env.ANDROID_SDK_ROOT);
  if (process.platform === 'darwin') candidates.push(path.join(process.env.HOME || '', 'Library', 'Android', 'sdk'));
  if (process.platform === 'linux') {
    candidates.push(path.join(process.env.HOME || '', 'Android', 'Sdk'));
    candidates.push('/usr/local/share/android-sdk');
  }
  if (process.platform === 'win32' && process.env.LOCALAPPDATA) {
    candidates.push(path.join(process.env.LOCALAPPDATA, 'Android', 'Sdk'));
  }
  for (const candidate of candidates) {
    if (candidate && existingDir(candidate)) return { dir: candidate, fromEnv: candidate === process.env.ANDROID_HOME || candidate === process.env.ANDROID_SDK_ROOT };
  }
  return { dir: null, fromEnv: false, candidates };
}

function checkAndroid() {
  // JDK -------------------------------------------------------------------
  const javaHome = process.env.JAVA_HOME;
  const java = run('java', ['-version']);
  const jdkMin = MINIMUMS.jdk;
  if (!java.found) {
    add('android', 'android.jdk', `JDK (>=${jdkMin.major})`, 'fail', 'java is not on PATH', 'Install a JDK and set JAVA_HOME (the JDK bundled with Android Studio works).');
  } else if (!java.ok) {
    add('android', 'android.jdk', `JDK (>=${jdkMin.major})`, 'fail', java.output || `java -version exited with code ${java.code}`, 'java exists but is not a working JDK (macOS ships a stub); install a JDK and set JAVA_HOME.');
  } else {
    // java -version prints to stderr as: openjdk version "17.0.12" 2024-07-16
    const match = firstMatch(java.output, /version\s+"(\d+)(?:\.(\d+))?/);
    const major = match ? Number(match[1]) : NaN;
    if (!match) {
      add('android', 'android.jdk', `JDK (>=${jdkMin.major})`, 'fail', java.output || 'could not parse `java -version`', 'Check the JDK installation and JAVA_HOME.');
    } else if (major >= jdkMin.major) {
      add('android', 'android.jdk', `JDK (>=${jdkMin.major})`, 'pass', match[0].replace('version ', '') + (javaHome ? ` (JAVA_HOME=${javaHome})` : ' (JAVA_HOME not set)'), javaHome ? null : 'Set JAVA_HOME so Gradle and the Tauri CLI use this JDK.');
    } else {
      add('android', 'android.jdk', `JDK (>=${jdkMin.major})`, 'fail', `found major ${major}`, `Android Gradle Plugin 8.x requires JDK ${jdkMin.major} (${jdkMin.source}); set JAVA_HOME.`);
    }
  }

  // Android SDK -----------------------------------------------------------
  const sdk = findAndroidSdk();
  if (!sdk.dir) {
    add(
      'android',
      'android.sdk',
      'Android SDK',
      'fail',
      'ANDROID_HOME/ANDROID_SDK_ROOT is unset and no default SDK directory was found',
      'Install the Android SDK components and set ANDROID_HOME (see https://v2.tauri.app/start/prerequisites/#android).',
    );
  } else {
    add(
      'android',
      'android.sdk',
      'Android SDK',
      sdk.fromEnv ? 'pass' : 'warn',
      `${sdk.dir}${sdk.fromEnv ? '' : ' (found by default path; ANDROID_HOME is not set)'}`,
      sdk.fromEnv ? null : 'Set ANDROID_HOME to this path so the Tauri CLI and Gradle find the SDK.',
    );

    const platforms = dirsIn(path.join(sdk.dir, 'platforms'));
    const platformLevels = platforms
      .map((name) => Number((name.match(/^android-(\d+)$/) || [])[1]))
      .filter((n) => Number.isFinite(n));
    const maxLevel = platformLevels.length ? Math.max(...platformLevels) : 0;
    const buildTools = dirsIn(path.join(sdk.dir, 'build-tools'));
    const cmdline = dirsIn(path.join(sdk.dir, 'cmdline-tools'));
    const platformTools = existingDir(path.join(sdk.dir, 'platform-tools'));

    add(
      'android',
      'android.sdk.platforms',
      'Android SDK Platform',
      platforms.length ? (maxLevel >= MINIMUMS.androidMinSdk ? 'pass' : 'warn') : 'fail',
      platforms.length
        ? `${platforms.join(', ')} (highest API ${maxLevel}; Tauri default minSdkVersion is ${MINIMUMS.androidMinSdk})`
        : 'no platforms/* directory',
      platforms.length
        ? maxLevel >= MINIMUMS.androidMinSdk
          ? null
          : `Install a platform with API >= ${MINIMUMS.androidMinSdk} (Tauri default minSdkVersion).`
        : 'Install "Android SDK Platform" via the SDK Manager.',
    );
    add(
      'android',
      'android.sdk.build_tools',
      'Android SDK Build-Tools',
      buildTools.length ? 'pass' : 'fail',
      buildTools.length ? buildTools.join(', ') : 'no build-tools/* directory',
      buildTools.length ? null : 'Install "Android SDK Build-Tools" via the SDK Manager.',
    );
    add(
      'android',
      'android.sdk.platform_tools',
      'Android SDK Platform-Tools',
      platformTools ? 'pass' : 'warn',
      platformTools ? 'platform-tools present' : 'platform-tools missing',
      platformTools ? null : 'Install "Android SDK Platform-Tools" to deploy to devices/emulators.',
    );
    add(
      'android',
      'android.sdk.cmdline_tools',
      'Android SDK Command-line Tools',
      cmdline.length ? 'pass' : 'warn',
      cmdline.length ? cmdline.join(', ') : 'no cmdline-tools/* directory',
      cmdline.length ? null : 'Install "Android SDK Command-line Tools" for sdkmanager/license handling.',
    );
  }

  // NDK -------------------------------------------------------------------
  const ndkCandidates = [];
  if (process.env.NDK_HOME) ndkCandidates.push(process.env.NDK_HOME);
  if (process.env.ANDROID_NDK_HOME) ndkCandidates.push(process.env.ANDROID_NDK_HOME);
  const ndkHandle = process.env.NDK_HOME || process.env.ANDROID_NDK_HOME;
  if (sdk.dir) {
    const ndkRoot = path.join(sdk.dir, 'ndk');
    for (const version of dirsIn(ndkRoot)) ndkCandidates.push(path.join(ndkRoot, version));
  }
  const ndk = ndkCandidates.find((candidate) => candidate && existingDir(candidate)) || null;
  if (ndk) {
    const toolchain = path.join(ndk, 'build', 'cmake', 'android.toolchain.cmake');
    const hasToolchain = fs.existsSync(toolchain);
    add(
      'android',
      'android.ndk',
      'Android NDK',
      ndkHandle ? 'pass' : 'warn',
      `${ndk}${ndkHandle ? '' : ' (found under the SDK; NDK_HOME is not set)'}`,
      ndkHandle ? null : 'Set NDK_HOME to this path. whisper.cpp C/C++ sources are cross-compiled with it.',
    );
    add(
      'android',
      'android.ndk.toolchain',
      'NDK CMake toolchain',
      hasToolchain ? 'pass' : 'fail',
      hasToolchain ? 'build/cmake/android.toolchain.cmake present' : 'android.toolchain.cmake not found',
      hasToolchain ? null : 'Reinstall the NDK (Side by side) so CMake can cross-compile whisper.cpp.',
    );
  } else {
    add(
      'android',
      'android.ndk',
      'Android NDK',
      'fail',
      'NDK_HOME/ANDROID_NDK_HOME is unset and no ndk/<version> was found under the SDK',
      'Install "NDK (Side by side)" and set NDK_HOME. whisper-rs-sys cross-compiles whisper.cpp with the NDK.',
    );
  }

  // Helpers ---------------------------------------------------------------
  const adb = run('adb', ['version']);
  add(
    'android',
    'android.adb',
    'adb (device deploy)',
    adb.found ? 'info' : 'skip',
    adb.found ? adb.output.split('\n')[0] : 'adb not on PATH (platform-tools)',
  );
  const gradle = run('gradle', ['--version']);
  add(
    'android',
    'android.gradle',
    'Gradle',
    'info',
    gradle.found ? gradle.output.split('\n')[0] : 'not on PATH (the Tauri CLI uses a generated Gradle wrapper)',
  );

  checkRustTargets('android');
}

// ---------------------------------------------------------------------------
// iOS checks
// ---------------------------------------------------------------------------

function checkIos() {
  if (process.platform !== 'darwin') {
    addHostGuard(
      'ios',
      'ios.host',
      'Host operating system',
      `iOS builds require macOS; this host is ${process.platform}`,
      'Run the iOS checks on a Mac with full Xcode installed.',
    );
    add('ios', 'ios.xcode', 'Xcode', 'skip', 'not evaluated on a non-macOS host');
    add('ios', 'ios.sdk.iphoneos', 'iPhoneOS SDK', 'skip', 'not evaluated on a non-macOS host');
    add('ios', 'ios.cocoapods', 'CocoaPods', 'skip', 'not evaluated on a non-macOS host');
    checkRustTargets('ios');
    return;
  }

  // Xcode selection -------------------------------------------------------
  const select = run('xcode-select', ['-p']);
  const selected = select.output || '';
  const xcodebuild = run('xcodebuild', ['-version']);
  const xcodeMatch = firstMatch(xcodebuild.output, /Xcode\s+(\d+\.\d+)/);
  const isXcodeApp = /Xcode[\w .]*\.app/.test(selected) && !/CommandLineTools/.test(selected);
  if (xcodebuild.found && xcodebuild.ok && isXcodeApp) {
    add('ios', 'ios.xcode', 'Xcode', 'pass', `${xcodeMatch ? `Xcode ${xcodeMatch[1]}` : xcodebuild.output.split('\n')[0]} (${selected})`);
  } else if (xcodebuild.found && xcodebuild.ok) {
    add('ios', 'ios.xcode', 'Xcode', 'warn', `xcodebuild works but the selected developer directory is ${selected}`, 'Select full Xcode with `sudo xcode-select -s /Applications/Xcode.app` for Simulator/device builds.');
  } else {
    add(
      'ios',
      'ios.xcode',
      'Xcode',
      'fail',
      xcodebuild.found ? xcodebuild.output || `exit ${xcodebuild.code}` : 'xcodebuild / full Xcode not available',
      'Install full Xcode (not only Command Line Tools), open it once, and select it with `sudo xcode-select -s /Applications/Xcode.app`.',
    );
  }

  // SDKs ------------------------------------------------------------------
  const iphoneos = run('xcrun', ['--sdk', 'iphoneos', '--show-sdk-version']);
  add(
    'ios',
    'ios.sdk.iphoneos',
    'iPhoneOS SDK',
    iphoneos.ok ? 'pass' : 'fail',
    iphoneos.ok ? `SDK ${iphoneos.output}` : iphoneos.output || 'xcrun could not locate the iphoneos SDK',
    iphoneos.ok ? null : 'Install the iOS platform in Xcode (Settings > Components).',
  );
  const iphonesim = run('xcrun', ['--sdk', 'iphonesimulator', '--show-sdk-version']);
  add(
    'ios',
    'ios.sdk.iphonesimulator',
    'iPhoneSimulator SDK',
    iphonesim.ok ? 'pass' : 'fail',
    iphonesim.ok ? `SDK ${iphonesim.output}` : iphonesim.output || 'xcrun could not locate the iphonesimulator SDK',
    iphonesim.ok ? null : 'Simulator builds need the iphonesimulator SDK from Xcode.',
  );
  if (iphoneos.ok) {
    const sdkMajor = Number((iphoneos.output.match(/^(\d+)/) || [])[1] || 0);
    const minSystem = Number(MINIMUMS.iosMinSystem.split('.')[0]);
    add(
      'ios',
      'ios.deployment_target',
      'Deployment target',
      sdkMajor >= minSystem ? 'info' : 'warn',
      `Tauri iOS default minimumSystemVersion is ${MINIMUMS.iosMinSystem}; installed SDK is ${iphoneos.output}`,
      sdkMajor >= minSystem ? null : `Installed SDK is older than the default ${MINIMUMS.iosMinSystem} deployment target.`,
    );
  }

  // CocoaPods / Homebrew --------------------------------------------------
  const pod = run('pod', ['--version']);
  if (!pod.found) {
    add(
      'ios',
      'ios.cocoapods',
      'CocoaPods',
      'fail',
      'pod is not on PATH',
      'Install CocoaPods (`brew install cocoapods`) as required by the Tauri iOS prerequisites.',
    );
  } else if (!pod.ok) {
    // A present-but-broken `pod` must not report pass: the version probe has to
    // run successfully. This catches a stub/shim on PATH and a CocoaPods install
    // that exists but errors (for example a broken Ruby/gem environment).
    add(
      'ios',
      'ios.cocoapods',
      'CocoaPods',
      'fail',
      pod.output || `pod --version exited with code ${pod.code}`,
      'pod exists on PATH but `pod --version` did not run successfully; repair the CocoaPods installation (`brew reinstall cocoapods` or fix its Ruby/gem environment).',
    );
  } else {
    add('ios', 'ios.cocoapods', 'CocoaPods', 'pass', `CocoaPods ${pod.output.split('\n')[0]}`);
  }
  const brew = run('brew', ['--version']);
  add('ios', 'ios.homebrew', 'Homebrew (optional)', brew.found ? 'info' : 'skip', brew.found ? brew.output.split('\n')[0] : 'not installed');
  const swiftc = run('swiftc', ['--version']);
  add(
    'ios',
    'ios.swiftc',
    'Swift compiler',
    swiftc.found ? 'pass' : 'warn',
    swiftc.found ? swiftc.output.split('\n')[0] : 'swiftc not on PATH',
    swiftc.found ? null : 'The Tauri iOS plugin layer is Swift; the compiler normally ships with Xcode.',
  );

  checkRustTargets('ios');
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

function summarize(target) {
  const failures = checks.filter((c) => c.status === 'fail');
  const hostGuard = failures.some((c) => c.hostGuard);
  const otherFailures = failures.some((c) => !c.hostGuard);
  let code = EXIT.READY;
  if (otherFailures) code = EXIT.PREREQUISITES;
  else if (hostGuard) code = EXIT.UNSUPPORTED_HOST;
  return { target, code, failures, hostGuard, otherFailures };
}

function printHuman(target, result) {
  console.log(STYLE.bold(`Golos mobile prerequisites — target: ${target}`));
  console.log('');
  let section = null;
  for (const check of checks) {
    if (check.section !== section) {
      section = check.section;
      console.log(STYLE.bold(`[${section}]`));
    }
    const rawStatus = MARK[check.status].padEnd(4);
    const statusLabel = STYLE[check.status] ? STYLE[check.status](rawStatus) : rawStatus;
    const detail = check.detail ? ` ${check.detail}` : '';
    console.log(`  ${statusLabel} ${check.label}${detail}`);
    if (check.hint && check.status !== 'pass' && check.status !== 'info') {
      console.log(`         ↳ ${check.hint}`);
    }
  }
  console.log('');
  const passed = checks.filter((c) => c.status === 'pass').length;
  const warned = checks.filter((c) => c.status === 'warn').length;
  const failed = result.failures.length;
  console.log(
    `${passed} passed, ${warned} warning(s), ${failed} failure(s) — exit ${result.code} (${
      result.code === 0
        ? 'ready'
        : result.code === 3
          ? 'unsupported host'
          : result.code === 1
            ? 'prerequisites missing'
            : 'see exit code'
    })`,
  );
  if (result.code === 1) console.log('Install the missing prerequisites above, then re-run this check.');
  if (result.code === 3) console.log('This target cannot be built on this host; run it on a supported OS (see docs/MOBILE_FEASIBILITY.md).');
  console.log('');
  console.log('Note: this only inspects host prerequisites. It is not a compile, device, or store-validation result.');
}

function printJson(target, result) {
  process.stdout.write(
    JSON.stringify(
      {
        target,
        host: { platform: process.platform, arch: process.arch, node: process.version },
        exitCode: result.code,
        summary: {
          passed: checks.filter((c) => c.status === 'pass').length,
          warnings: checks.filter((c) => c.status === 'warn').length,
          failed: result.failures.length,
        },
        checks,
      },
      null,
      2,
    ) + '\n',
  );
}

function usage() {
  return [
    'Usage: node scripts/check-mobile-prerequisites.mjs [android|ios|all] [--json] [--no-color]',
    '',
    'Inspects host toolchains needed to initialize and compile the Golos Tauri',
    'Android/iOS shells. Installs nothing and reads no secrets.',
    '',
    'Exit codes: 0 ready, 1 missing prerequisites, 2 usage error,',
    '            3 target unsupported on this host, 4 internal error.',
  ].join('\n');
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

function parseArgs(argv) {
  const options = { target: 'all', json: false, help: false };
  for (const arg of argv) {
    if (arg === '--json') options.json = true;
    else if (arg === '--no-color') continue;
    else if (arg === '--help' || arg === '-h') options.help = true;
    else if (['android', 'ios', 'all'].includes(arg)) options.target = arg;
    else return { error: `Unknown argument: ${arg}` };
  }
  return options;
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.error) {
    console.error(`Error: ${options.error}\n\n${usage()}`);
    process.exit(EXIT.USAGE);
  }
  if (options.help) {
    console.log(usage());
    process.exit(EXIT.READY);
  }

  checkHost();
  checkNode();
  checkRust();
  checkTauriCli();
  checkProjectConfig();
  if (options.target === 'android' || options.target === 'all') checkAndroid();
  if (options.target === 'ios' || options.target === 'all') checkIos();

  const result = summarize(options.target);
  if (options.json) printJson(options.target, result);
  else printHuman(options.target, result);
  process.exit(result.code);
}

try {
  main();
} catch (error) {
  console.error(`checker internal error: ${error && error.stack ? error.stack : error}`);
  process.exit(EXIT.INTERNAL);
}
