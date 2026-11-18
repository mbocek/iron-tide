"""The radio operator's announcements in every interface language, spoken by espeak-ng and sent through a radio.

espeak-ng (GPL) is only a tool here: the recordings it makes are ordinary assets. Without it installed the
voices are left out of a full regeneration and the committed recordings stay as they are.
"""
import random
import shutil
import subprocess
import tempfile
import wave
from pathlib import Path

from dsp import (
    SAMPLE_RATE, biquad, exponential_decay, mix_into, multiplied, normalized_stereo, samples, scaled,
    silence, soft_clip, white_noise,
)

VOICE_PEAK_DBFS = -2.0
SPEAKING_RATE_WPM = "150"
PITCH = "38"
RADIO_LOW_HZ = 380.0
RADIO_HIGH_HZ = 3000.0
RADIO_DRIVE = 2.2
HISS_LEVEL = 0.03
SQUELCH_SECONDS = 0.07

LANGUAGES = {"en": "en-gb", "sk": "sk", "cs": "cs"}

PHRASES = {
    "unit_ready": {"en": "Unit ready.", "sk": "Jednotka pripravená.", "cs": "Jednotka připravena."},
    "construction_complete": {"en": "Construction complete.", "sk": "Stavba dokončená.", "cs": "Stavba dokončena."},
    "research_complete": {"en": "Research complete.", "sk": "Výskum dokončený.", "cs": "Výzkum dokončen."},
    "base_under_attack": {
        "en": "Our base is under attack!", "sk": "Naša základňa je pod útokom!", "cs": "Naše základna je pod útokem!",
    },
    "harvester_under_attack": {
        "en": "Our harvester is under attack!", "sk": "Náš zberač je pod paľbou!", "cs": "Náš sběrač je pod palbou!",
    },
    "structure_lost": {"en": "Structure lost.", "sk": "Stratili sme budovu.", "cs": "Ztratili jsme budovu."},
}


def espeak():
    return shutil.which("espeak-ng")


def spoken(language, text):
    """espeak-ng's dry mono recording, resampled to the game's rate."""
    with tempfile.TemporaryDirectory(prefix="iron_tide_voice_") as scratch:
        path = Path(scratch) / "speech.wav"
        subprocess.run([espeak(), "-v", LANGUAGES[language], "-s", SPEAKING_RATE_WPM, "-p", PITCH,
                        "-w", str(path), text], check=True)
        with wave.open(str(path)) as recording:
            rate = recording.getframerate()
            frames = recording.readframes(recording.getnframes())
    pcm = [int.from_bytes(frames[i:i + 2], "little", signed=True) / 32768.0 for i in range(0, len(frames), 2)]
    return resampled(pcm, rate)


def resampled(signal, rate):
    step = rate / SAMPLE_RATE
    length = int(len(signal) / step)
    out = []
    for i in range(length):
        position = i * step
        index = int(position)
        fraction = position - index
        following = signal[min(index + 1, len(signal) - 1)]
        out.append(signal[index] * (1.0 - fraction) + following * fraction)
    return out


def squelch(rng):
    """The click and burst of static as the operator keys the radio or lets go."""
    length = samples(SQUELCH_SECONDS)
    burst = multiplied(white_noise(length, rng), exponential_decay(length, 0.02))
    return biquad(biquad(burst, "highpass", 1200.0), "lowpass", 5000.0)


def radio(speech, rng):
    """A field radio: a narrow band, a little overdriven, a bed of hiss, and squelch at both ends."""
    voice = biquad(biquad(speech, "highpass", RADIO_LOW_HZ), "highpass", RADIO_LOW_HZ)
    voice = biquad(biquad(voice, "lowpass", RADIO_HIGH_HZ), "lowpass", RADIO_HIGH_HZ)
    voice = soft_clip(scaled(voice, 1.0 / max(1e-9, max(abs(x) for x in voice))), RADIO_DRIVE)
    lead = samples(SQUELCH_SECONDS)
    length = lead + len(voice) + lead
    out = silence(length)
    mix_into(out, squelch(rng), 0, 0.35)
    mix_into(out, voice, lead)
    hiss = biquad(white_noise(length, rng), "bandpass", 2000.0, 0.5)
    mix_into(out, hiss, 0, HISS_LEVEL)
    mix_into(out, squelch(rng), length - lead, 0.5)
    return normalized_stereo(out, list(out), VOICE_PEAK_DBFS)


def announcement(phrase, language):
    def render():
        rng = random.Random(f"{phrase}/{language}")
        return radio(spoken(language, PHRASES[phrase][language]), rng)
    return render


VOICES = {
    f"voice/{language}/{phrase}": announcement(phrase, language)
    for phrase in PHRASES
    for language in LANGUAGES
} if espeak() else {}
