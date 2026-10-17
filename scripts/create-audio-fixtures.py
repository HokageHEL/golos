"""Create local audio fixtures for the Golos importer.

Two modes:

* Speech corpus (needs FFmpeg; test tooling only, Golos never bundles or
  invokes it):
      python3 scripts/create-audio-fixtures.py FFMPEG SOURCE_WAV DEST
  `SOURCE_WAV` is the public whisper.cpp JFK sample (~11 s of speech). It
  writes the fixtures that the ignored `decodes_real_compressed_formats` test
  loads through ``GOLOS_TEST_FORMATS``.

* Committed regression fixtures (needs opus-tools and vorbis-tools; no FFmpeg):
      python3 scripts/create-audio-fixtures.py --synthetic DEST
  Rebuilds the tiny tone files checked in under
  `native/src-tauri/tests/fixtures/`, so the default `cargo test` run needs
  neither external binaries nor network access.

Both modes only *write* fixtures; nothing here runs during a normal build or at
application runtime.
"""
import math
import pathlib
import struct
import subprocess
import sys
import wave

# Speech-corpus fixtures: name -> FFmpeg audio codec. The extension and codec
# together decide which container/decoder the importer must handle.
SPEECH_FORMATS = {
    "speech.mp3": "libmp3lame",
    "speech-aac.m4a": "aac",
    "speech-alac.m4a": "alac",
    "speech.aac": "aac",
    "speech.flac": "flac",
    "speech.ogg": "libvorbis",
    "speech-opus.ogg": "libopus",
    "speech-opus.opus": "libopus",
    "speech.aiff": "pcm_s16be",
    "speech-24bit.wav": "pcm_s24le",
    # Six-channel Opus (channel mapping family 1) must be refused, not downmixed.
    "multichannel-opus.ogg": "libopus",
}

# Committed regression fixtures:
#   name -> (seconds, channels, frequency, opus bitrate, Ogg stream serial).
# The serial is fixed so a rebuild is byte-identical on the same tool versions.
SYNTHETIC_TONES = {
    "opus-stereo.ogg": (2.0, 2, 440.0, 32, 1001),
    "opus-mono.ogg": (1.5, 1, 220.0, 24, 1002),
    "vorbis-stereo.ogg": (2.0, 2, 440.0, 0, 2001),
}
SAMPLE_RATE = 48_000
AMPLITUDE = 0.5


def write_tone(path, seconds, channels, frequency):
    """Write a 16-bit PCM sine WAV; the input every encoder below shares."""
    frames = round(seconds * SAMPLE_RATE)
    with wave.open(str(path), "wb") as output:
        output.setnchannels(channels)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        samples = bytearray()
        for frame in range(frames):
            value = round(
                AMPLITUDE * 32767.0
                * math.sin(2.0 * math.pi * frequency * frame / SAMPLE_RATE)
            )
            samples += struct.pack("<h", value) * channels
        output.writeframes(bytes(samples))


def create_synthetic(destination):
    """Rebuild the committed fixtures with opusenc/oggenc (deterministic)."""
    root = pathlib.Path(destination)
    root.mkdir(parents=True, exist_ok=True)
    for name, (seconds, channels, frequency, bitrate, serial) in SYNTHETIC_TONES.items():
        wav = root / f"{pathlib.Path(name).stem}.wav"
        write_tone(wav, seconds, channels, frequency)
        target = root / name
        if name.startswith("opus"):
            command = [
                "opusenc", "--quiet", "--bitrate", str(bitrate),
                "--serial", str(serial), str(wav), str(target),
            ]
        else:
            command = ["oggenc", "-Q", "-q", "3", "-s", str(serial), "-o", str(target), str(wav)]
        subprocess.run(command, check=True)
        wav.unlink()
    print(f"Created {len(SYNTHETIC_TONES)} fixtures in {root}")


def create_speech(ffmpeg, source, destination):
    root = pathlib.Path(destination)
    root.mkdir(parents=True, exist_ok=True)
    for name, codec in SPEECH_FORMATS.items():
        channels = "6" if name.startswith("multichannel") else "2"
        subprocess.run(
            [
                ffmpeg, "-hide_banner", "-loglevel", "error", "-n", "-i", source,
                "-ar", "48000", "-ac", channels, "-c:a", codec, str(root / name),
            ],
            check=True,
        )
    print(f"Created {len(SPEECH_FORMATS)} fixtures in {root}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--synthetic":
        create_synthetic(sys.argv[2])
    elif len(sys.argv) == 4:
        create_speech(*sys.argv[1:])
    else:
        print(__doc__)
        raise SystemExit(2)
