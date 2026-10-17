# Committed audio regression fixtures

These three files are decoded by the default (non-ignored) `cargo test` run in
the `import` module, so the Ogg Opus and Vorbis paths are covered without any
external binary or network access at test time.

## Provenance

They are **synthetic** sine tones generated locally from scratch by
`scripts/create-audio-fixtures.py --synthetic`. They contain no third-party,
recorded, or licensed material — no speech samples, music, or metadata — so
there is nothing to attribute and no copyright encumbrance. Every sample is
`0.5 * sin(2*pi*f*t)` at 48 kHz, written as a 16-bit PCM WAV and then encoded
once.

## Exact generation

Rebuild from the repository root with:

```sh
python3 scripts/create-audio-fixtures.py --synthetic native/src-tauri/tests/fixtures
```

That script runs the following commands (one WAV per tone, here at 48 kHz mono
or stereo with amplitude 0.5, then removed):

| Output | Command |
| --- | --- |
| `opus-stereo.ogg` | `opusenc --quiet --bitrate 32 --serial 1001 opus-stereo.wav opus-stereo.ogg` |
| `opus-mono.ogg` | `opusenc --quiet --bitrate 24 --serial 1002 opus-mono.wav opus-mono.ogg` |
| `vorbis-stereo.ogg` | `oggenc -Q -q 3 -s 2001 -o vorbis-stereo.ogg vorbis-stereo.wav` |

Tool versions used to create the checked-in bytes (the Ogg stream serial is
pinned in the script so a rebuild is byte-identical on the same versions):

- `opusenc` — opus-tools 0.2 (libopus 1.6.1)
- `oggenc` — vorbis-tools 1.4.3

## Contents

| File | Duration | Channels | Tone | Codec | Bytes | SHA-256 |
| --- | --- | --- | --- | --- | --- | --- |
| `opus-stereo.ogg` | 2.0 s | 2 | 440 Hz | Opus (mapping family 0) | 14693 | `25e28995d6956c33ebae7b94d39064f922d263d3e53eafc6c67c226ba5ed4ce6` |
| `opus-mono.ogg` | 1.5 s | 1 | 220 Hz | Opus (mapping family 0) | 7066 | `5d574dc680fb487cd22095e074aad44267cb9661da5f2cdca3663b0157e6f3ca` |
| `vorbis-stereo.ogg` | 2.0 s | 2 | 440 Hz | Vorbis | 10501 | `9fdaa46d5f2f1bef72b54a85237d0549769089beeaab1f8e50a940b584ef73df` |

The tests also mutate in-memory copies of `opus-stereo.ogg` (repairing the Ogg
page CRC after each edit) to exercise a non-48 kHz input-rate field, a known
`-6.02 dB` OpusHead output gain, and rejection of multichannel/mapped headers.
No mutated file is written into this directory.
