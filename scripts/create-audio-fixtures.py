"""Generate local codec fixtures from the public whisper.cpp JFK sample.

Usage: python3 scripts/create-audio-fixtures.py /path/to/ffmpeg /path/to/jfk.wav /tmp/golos-formats
FFmpeg is test tooling only; Golos neither bundles nor invokes it.
"""
import pathlib
import subprocess
import sys

ffmpeg, source, destination = sys.argv[1:]
root = pathlib.Path(destination)
root.mkdir(parents=True, exist_ok=True)
formats = {
    "speech.mp3": "libmp3lame",
    "speech-aac.m4a": "aac",
    "speech-alac.m4a": "alac",
    "speech.aac": "aac",
    "speech.flac": "flac",
    "speech.ogg": "libvorbis",
    "speech.aiff": "pcm_s16be",
    "speech-24bit.wav": "pcm_s24le",
    "unsupported-opus.ogg": "libopus",
}
for name, codec in formats.items():
    subprocess.run([ffmpeg, "-hide_banner", "-loglevel", "error", "-n", "-i", source,
                    "-ar", "48000", "-ac", "2", "-c:a", codec, str(root / name)], check=True)
print(f"Created {len(formats)} fixtures in {root}")
