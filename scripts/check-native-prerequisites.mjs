#!/usr/bin/env node
// Cross-platform prerequisite checker for the Golos native (Tauri) desktop app.
//
// Standard library only: it runs read-only version probes, downloads nothing,
// performs no network access, and never touches audio or Whisper models. Run it
// before `npm run native:app` locally and in CI so both share one source of
// truth about the toolchain the native build actually needs.
//
// Usage:
//   node scripts/check-native-prerequisites.mjs [--json] [--help]
//
// Exit status is 0 only when every required prerequisite for the current
// platform is present; otherwise 1. Warnings never change the exit status.

import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { basename, delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import process from 'node:process';

const HELP = `Golos native prerequisite checker

Verifies the toolchain needed to build the offline Tauri desktop app on the
current machine. Read-only: no installs, no network, no model or audio access.

Usage:
  node scripts/check-native-prerequisites.mjs [--json]

Options:
  --json   Print a machine-readable report instead of the human summary.
  --help   Show this message.

Exit status: 0 when every required prerequisite is present, 1 when one is
missing or the platform is unsupported, 2 for an unknown option.
`;

const MIN_NODE = '22.12.0';
const RUST_HINT = 'Install a Rust toolchain from https://rustup.rs and ensure cargo/rustc are on PATH.';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, '..');
const platform = process.platform;
const isWindows = platform === 'win32';
const asJson = process.argv.includes('--json');
const SUPPORTED_PLATFORMS = ['darwin', 'linux', 'win32'];

const KNOWN_FLAGS = new Set(['--json', '--help', '-h']);
const unknownFlags = process.argv.slice(2).filter((arg) => !KNOWN_FLAGS.has(arg));

if (process.argv.includes('--help') || process.argv.includes('-h')) {
  console.log(HELP);
  process.exit(0);
}

if (unknownFlags.length) {
  console.error(`Unknown option: ${unknownFlags.join(', ')}`);
  console.error('Run `node scripts/check-native-prerequisites.mjs --help` for usage.');
  process.exit(2);
}

if (!SUPPORTED_PLATFORMS.includes(platform)) {
  const hint = 'Golos native builds are supported on macOS, Linux, and Windows only.';
  if (asJson) {
    console.log(
      JSON.stringify(
        {
          platform,
          arch: process.arch,
          node: process.version,
          failed: 1,
          warned: 0,
          rows: [{ status: 'fail', name: 'Operating system', detail: platform, hint }],
        },
        null,
        2,
      ),
    );
  } else {
    console.error(`Unsupported platform: ${platform}`);
    console.error(`         → ${hint}`);
  }
  process.exit(1);
}

const rows = [];

function add(status, name, detail, hint) {
  rows.push({ status, name, detail, hint });
}

function probe(command, args = [], useShell = isWindows) {
  const result = spawnSync(command, args, {
    encoding: 'utf8',
    timeout: 20000,
    windowsHide: true,
    // Windows ships npm/cargo shims as .cmd; the shell resolves them. Absolute
    // paths that may contain spaces pass useShell = false.
    shell: useShell,
  });
  const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
  return { ok: !result.error && result.status === 0, output };
}

function firstLine(output) {
  return output.split('\n')[0].trim();
}

function version(command, args = ['--version'], useShell = isWindows) {
  const { ok, output } = probe(command, args, useShell);
  return ok && output ? firstLine(output) : null;
}

function meetsMinimum(actual, minimum) {
  const a = actual.split('.').map(Number);
  const m = minimum.split('.').map(Number);
  for (let i = 0; i < 3; i += 1) {
    if ((a[i] ?? 0) > (m[i] ?? 0)) return true;
    if ((a[i] ?? 0) < (m[i] ?? 0)) return false;
  }
  return true;
}

if (platform === 'darwin') {
  const swift = version('xcrun', ['swiftc', '--version'], false);
  add(swift ? 'ok' : 'fail', 'Swift / ScreenCaptureKit', swift ?? 'not found',
    'Install current Xcode Command Line Tools with a macOS 15+ SDK to build the audio capture helper.');
}

// --- Node.js / npm ---------------------------------------------------------

const nodeVersion = process.versions.node;
add(
  meetsMinimum(nodeVersion, MIN_NODE) ? 'ok' : 'fail',
  'Node.js',
  `v${nodeVersion} (requires >= ${MIN_NODE})`,
  `Install Node.js ${MIN_NODE} or newer from https://nodejs.org.`,
);

const npmVersion = version('npm');
add(
  npmVersion ? 'ok' : 'fail',
  'npm',
  npmVersion ?? 'not found',
  'npm ships with Node.js; reinstall Node.js 22.12+ and reopen the terminal.',
);

// --- Git and Rust ----------------------------------------------------------

const gitVersion = version('git');
add(
  gitVersion ? 'ok' : 'warn',
  'git',
  gitVersion ?? 'not found',
  'Install git to clone the repository; not required for an existing checkout.',
);

const rustcVersion = version('rustc');
add(rustcVersion ? 'ok' : 'fail', 'rustc', rustcVersion ?? 'not found', RUST_HINT);

const cargoVersion = version('cargo');
add(cargoVersion ? 'ok' : 'fail', 'cargo', cargoVersion ?? 'not found', RUST_HINT);

const rustupVersion = version('rustup');
add(
  rustupVersion ? 'ok' : 'warn',
  'rustup',
  rustupVersion ?? 'not found',
  'rustup is optional; it simplifies installing toolchains and mobile targets (https://rustup.rs).',
);

// --- Native build tools (whisper.cpp needs CMake + a C/C++ compiler) -------

const cmakeHint = {
  darwin: 'Install CMake from https://cmake.org/download or `brew install cmake`.',
  linux: 'Install CMake with `sudo apt-get install cmake`.',
  win32: 'Install CMake from https://cmake.org/download; the Visual Studio C++ workload bundles it.',
}[platform];
const cmakeVersion = version('cmake');
add(cmakeVersion ? 'ok' : 'fail', 'CMake', cmakeVersion ?? 'not found', cmakeHint);

const compilerHint = {
  darwin: 'Install the Xcode Command Line Tools with `xcode-select --install`.',
  linux: 'Install a C/C++ toolchain with `sudo apt-get install build-essential`.',
  win32: 'Install the "Desktop development with C++" workload from the Visual Studio Build Tools.',
}[platform];

function detectCompiler() {
  if (isWindows) {
    const programFilesX86 = process.env['ProgramFiles(x86)'] ?? 'C:\\Program Files (x86)';
    const vswhere = join(programFilesX86, 'Microsoft Visual Studio', 'Installer', 'vswhere.exe');
    if (existsSync(vswhere)) {
      const { ok, output } = probe(
        vswhere,
        ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'],
        false,
      );
      if (ok && output) return `MSVC (${firstLine(output)})`;
    }
    return version('cl', ['/?']) ?? version('clang') ?? version('gcc');
  }
  for (const name of ['cc', 'clang', 'gcc']) {
    const found = version(name);
    if (found) return `${name}: ${found}`;
  }
  return null;
}

const compiler = detectCompiler();
add(compiler ? 'ok' : 'fail', 'C/C++ compiler', compiler ?? 'not found', compilerHint);

// --- libclang (bindgen generates the whisper-rs bindings at build time) ----

// File names that actually provide libclang on this platform. A path counts as
// ready only when it points at one of these libraries (or a directory containing
// one); an arbitrary existing path must not produce a false "ready" report.
const LIBCLANG_LIBRARY = {
  darwin: /^libclang(\.\d+(\.\d+)*)?\.dylib$/,
  linux: /^libclang(-\d+(\.\d+)*)?\.so(\.\d+)*$/,
  win32: /^libclang\.dll$/,
}[platform];

function isLibclangLibrary(name) {
  return LIBCLANG_LIBRARY ? LIBCLANG_LIBRARY.test(name) : false;
}

function listDir(dir) {
  try {
    return readdirSync(dir);
  } catch {
    return []; // Missing or unreadable directory: report as empty, never throw.
  }
}

// Accepts a libclang library file or a directory containing one.
function resolveLibclang(pathOrDir) {
  if (!pathOrDir || !existsSync(pathOrDir)) return null;
  if (isLibclangLibrary(basename(pathOrDir))) return pathOrDir;
  let stats;
  try {
    stats = statSync(pathOrDir);
  } catch {
    return null;
  }
  if (!stats.isDirectory()) return null;
  const match = listDir(pathOrDir).find(isLibclangLibrary);
  return match ? join(pathOrDir, match) : null;
}

function detectLibclang() {
  const envPath = process.env.LIBCLANG_PATH;
  if (envPath) {
    const resolved = resolveLibclang(envPath);
    if (resolved) return `LIBCLANG_PATH=${resolved}`;
  }

  if (platform === 'darwin') {
    const developerDir = version('xcode-select', ['-p']);
    // `xcrun --find clang` follows the selected developer directory, so full
    // Xcode and the Command Line Tools both resolve here.
    const clangPath = version('xcrun', ['--find', 'clang']);
    const candidates = [
      // Command Line Tools directory and selected developer directory.
      '/Library/Developer/CommandLineTools/usr/lib/libclang.dylib',
      developerDir ? join(developerDir, 'usr/lib/libclang.dylib') : null,
      // Full Xcode keeps libclang inside its default toolchain.
      developerDir ? join(developerDir, 'Toolchains/XcodeDefault.xctoolchain/usr/lib/libclang.dylib') : null,
      // Resolve the selected clang back to its sibling ../lib/libclang.dylib.
      clangPath ? resolve(dirname(clangPath), '..', 'lib', 'libclang.dylib') : null,
      '/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/lib/libclang.dylib',
    ];
    for (const candidate of candidates) {
      if (candidate && existsSync(candidate)) return candidate;
    }
    return null;
  }

  if (platform === 'linux') {
    const dirs = ['/usr/lib', '/usr/lib/x86_64-linux-gnu', '/usr/lib/aarch64-linux-gnu', '/usr/local/lib'];
    for (const base of ['/usr/lib', '/usr/local/lib']) {
      for (const entry of listDir(base)) {
        if (entry.startsWith('llvm-')) dirs.push(join(base, entry, 'lib'));
      }
    }
    for (const dir of dirs) {
      if (!existsSync(dir)) continue;
      const match = listDir(dir).find(isLibclangLibrary);
      if (match) return join(dir, match);
    }
    const ldconfig = probe('ldconfig', ['-p']);
    if (ldconfig.ok && /libclang/.test(ldconfig.output)) return 'libclang (found via ldconfig)';
    return null;
  }

  // win32: libclang.dll next to clang.exe on PATH, or a default LLVM install.
  if (isWindows) {
    for (const dir of (process.env.PATH ?? '').split(delimiter)) {
      if (dir && existsSync(join(dir, 'libclang.dll'))) return join(dir, 'libclang.dll');
    }
    for (const base of ['C:\\Program Files\\LLVM\\bin', 'C:\\Program Files (x86)\\LLVM\\bin']) {
      if (existsSync(join(base, 'libclang.dll'))) return join(base, 'libclang.dll');
    }
  }
  return null;
}

const libclangHint = {
  darwin: 'Install the Xcode Command Line Tools (`xcode-select --install`); they provide libclang.dylib.',
  linux: 'Install libclang with `sudo apt-get install libclang-dev`.',
  win32: 'Install LLVM (https://releases.llvm.org) and set LIBCLANG_PATH to its bin directory.',
}[platform];
const envLibclang = process.env.LIBCLANG_PATH;
const envLibclangInvalid = Boolean(envLibclang) && !resolveLibclang(envLibclang);
const libclang = detectLibclang();
add(
  libclang ? 'ok' : 'fail',
  'libclang',
  libclang ??
    (envLibclangInvalid ? `not found (LIBCLANG_PATH=${envLibclang} has no libclang library)` : 'not found'),
  `${libclangHint} As a last resort, WHISPER_DONT_GENERATE_BINDINGS=1 reuses pre-generated bindings.`,
);

// --- Tauri CLI (installed as a project dev dependency) ---------------------

const tauriWrapper = join(repoRoot, 'node_modules', '.bin', isWindows ? 'tauri.cmd' : 'tauri');
const tauriPackage = join(repoRoot, 'node_modules', '@tauri-apps', 'cli', 'package.json');
if (existsSync(tauriWrapper) || existsSync(tauriPackage)) {
  let tauriDetail = 'installed';
  if (existsSync(tauriPackage)) {
    try {
      tauriDetail = `tauri-cli ${JSON.parse(readFileSync(tauriPackage, 'utf8')).version}`;
    } catch {
      tauriDetail = 'installed (version unreadable)';
    }
  }
  add('ok', 'Tauri CLI', tauriDetail, null);
} else {
  add(
    'fail',
    'Tauri CLI',
    'not installed',
    'Run `npm ci` from the repository root to install @tauri-apps/cli.',
  );
}

// --- Platform system dependencies ------------------------------------------

if (platform === 'linux') {
  const pkgConfig = version('pkg-config');
  if (!pkgConfig) {
    add(
      'warn',
      'pkg-config',
      'not found',
      'Install pkg-config (`sudo apt-get install pkg-config`) to verify the Tauri system libraries.',
    );
  } else {
    const modules = [
      ['alsa', 'ALSA microphone capture', 'fail', 'sudo apt-get install libasound2-dev'],
      ['webkit2gtk-4.1', 'WebKitGTK 4.1', 'fail', 'sudo apt-get install libwebkit2gtk-4.1-dev'],
      ['ayatana-appindicator3-0.1', 'Ayatana AppIndicator3', 'fail', 'sudo apt-get install libayatana-appindicator3-dev'],
      ['librsvg-2.0', 'librsvg', 'fail', 'sudo apt-get install librsvg2-dev'],
      ['openssl', 'OpenSSL', 'warn', 'sudo apt-get install libssl-dev'],
      ['xdo', 'libxdo', 'warn', 'sudo apt-get install libxdo-dev'],
    ];
    for (const [mod, label, status, hint] of modules) {
      const present = probe('pkg-config', ['--exists', mod]).ok;
      add(present ? 'ok' : status, label, present ? 'present (pkg-config)' : 'missing', hint);
    }
    const patchelf = version('patchelf');
    add(
      patchelf ? 'ok' : 'warn',
      'patchelf',
      patchelf ?? 'not found',
      'Needed when bundling .deb/.AppImage: `sudo apt-get install patchelf`.',
    );
  }
}

if (platform === 'darwin') {
  const developerDir = version('xcode-select', ['-p']);
  const cltInstalled = Boolean(developerDir) && existsSync(developerDir);
  add(
    cltInstalled ? 'ok' : 'fail',
    'Xcode command-line tools',
    developerDir ?? 'not configured',
    'Run `xcode-select --install`; Xcode or the Command Line Tools are required.',
  );
  if (developerDir) {
    const sdk = version('xcrun', ['--sdk', 'macosx', '--show-sdk-path']);
    add(
      sdk ? 'ok' : 'warn',
      'macOS SDK',
      sdk ?? 'not found',
      'Open Xcode once so it finishes installing components, or reinstall the Command Line Tools.',
    );
  }
}

if (isWindows) {
  const webviewDir = [
    'C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application',
    'C:\\Program Files\\Microsoft\\EdgeWebView\\Application',
  ].find((dir) => existsSync(dir));
  add(
    webviewDir ? 'ok' : 'warn',
    'Microsoft Edge WebView2',
    webviewDir ?? 'not detected',
    'Install the WebView2 Evergreen Runtime; it is preinstalled on Windows 10 (1803+) and Windows 11.',
  );
}

// --- Report ----------------------------------------------------------------

const failed = rows.filter((row) => row.status === 'fail');
const warned = rows.filter((row) => row.status === 'warn');

if (asJson) {
  console.log(
    JSON.stringify(
      { platform, arch: process.arch, node: process.version, failed: failed.length, warned: warned.length, rows },
      null,
      2,
    ),
  );
} else {
  const label = { ok: ' ok ', warn: 'warn', fail: 'FAIL' };
  console.log(`Golos native prerequisites — ${platform}/${process.arch}, Node ${process.version}`);
  console.log('');
  for (const row of rows) {
    console.log(`[${label[row.status]}] ${row.name} — ${row.detail}`);
    if (row.status !== 'ok' && row.hint) console.log(`         → ${row.hint}`);
  }
  console.log('');
  if (failed.length) {
    console.log(`${failed.length} required prerequisite(s) missing; ${warned.length} warning(s).`);
  } else {
    console.log(`All required prerequisites present${warned.length ? ` (${warned.length} warning(s))` : ''}.`);
  }
}

process.exit(failed.length ? 1 : 0);
