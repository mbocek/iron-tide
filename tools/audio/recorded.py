"""The public-domain recordings in tools/audio/recordings (see import_recordings.py for their sources)."""
import array
import shutil
import subprocess
from functools import lru_cache
from pathlib import Path

from dsp import SAMPLE_RATE, peak, scaled

RECORDINGS_DIR = Path(__file__).resolve().parent / "recordings"
FALLBACK_FFMPEG = "/opt/homebrew/bin/ffmpeg"


@lru_cache(maxsize=None)
def _decoded(name):
    ffmpeg = shutil.which("ffmpeg") or FALLBACK_FFMPEG
    pcm = subprocess.run(
        [ffmpeg, "-v", "error", "-i", str(RECORDINGS_DIR / f"{name}.flac"), "-ac", "1", "-ar", str(SAMPLE_RATE),
         "-f", "f32le", "-"],
        capture_output=True, check=True,
    ).stdout
    decoded = array.array("f")
    decoded.frombytes(pcm)
    return tuple(scaled(list(decoded), 1.0 / peak(decoded)))


def recording(name, speed=1.0):
    """A recording at full scale; `speed` below 1 plays it slower and deeper, the way a bigger gun or charge sounds."""
    signal = list(_decoded(name))
    return signal if speed == 1.0 else varispeed(signal, speed)


def varispeed(signal, speed):
    length = int(len(signal) / speed)
    last = len(signal) - 1
    out = []
    append = out.append
    for i in range(length):
        position = i * speed
        index = int(position)
        if index >= last:
            append(signal[last])
            continue
        fraction = position - index
        append(signal[index] * (1.0 - fraction) + signal[index + 1] * fraction)
    return out
