import math
import random
from functools import lru_cache

from dsp import (
    SAMPLE_RATE, adsr, biquad, exponential_decay, exponential_glide, fade_out, fm_tone, midi_to_frequency,
    mix_into, multiplied, one_pole_highpass, one_pole_lowpass, partials, samples, saw, scaled, segments,
    sine, soft_clip, square, summed, white_noise,
)

END_FADE_SECONDS = 0.01
VIBRATO_HZ = 5.3


def _rng(*key):
    return random.Random(hash_key(key))


def hash_key(parts):
    """Stable seed from a tuple of numbers/strings (Python's hash() is salted per process)."""
    seed = 2166136261
    for character in repr(parts):
        seed = ((seed ^ ord(character)) * 16777619) & 0xFFFFFFFF
    return seed


@lru_cache(maxsize=None)
def kick_drum(variant=0, start_hz=150.0, end_hz=48.0, decay=0.2):
    rng = _rng("kick", variant)
    length = samples(decay * 2.5)
    pitch = exponential_glide(start_hz, end_hz, length, 0.03)
    body = soft_clip(multiplied(sine(pitch, length), exponential_decay(length, decay)), 1.6)
    click_length = samples(0.006)
    click = multiplied(one_pole_highpass(white_noise(click_length, rng), 1500.0), exponential_decay(click_length, 0.0015))
    mix_into(body, click, 0, 0.35)
    return fade_out(body, END_FADE_SECONDS)


@lru_cache(maxsize=None)
def snare_drum(variant=0, tone_hz=190.0, decay=0.13):
    rng = _rng("snare", variant)
    length = samples(decay * 3.0)
    tone = summed(
        scaled(multiplied(sine(exponential_glide(tone_hz * 1.4, tone_hz, length, 0.01), length), exponential_decay(length, 0.04)), 0.7),
        scaled(multiplied(sine(tone_hz * 1.72, length), exponential_decay(length, 0.025)), 0.3),
    )
    rattle = biquad(white_noise(length, rng), "highpass", 1200.0)
    rattle = biquad(rattle, "lowpass", 9000.0)
    rattle = multiplied(rattle, exponential_decay(length, decay))
    return fade_out(summed(tone, scaled(rattle, 0.9)), END_FADE_SECONDS)


@lru_cache(maxsize=None)
def hi_hat(variant=0, decay=0.03):
    rng = _rng("hat", variant)
    length = samples(decay * 4.0 + 0.01)
    metal = summed(*[square(f, length) for f in (317.0, 451.0, 588.0, 739.0, 834.0, 1047.0)])
    noise = white_noise(length, rng)
    body = [0.08 * m + n for m, n in zip(metal, noise)]
    body = biquad(biquad(body, "highpass", 7000.0), "highpass", 7000.0)
    return fade_out(multiplied(body, exponential_decay(length, decay)), END_FADE_SECONDS)


@lru_cache(maxsize=None)
def crash_cymbal(variant=0, decay=0.9):
    rng = _rng("crash", variant)
    length = samples(decay * 3.0)
    metal = summed(*[square(f, length) for f in (297.0, 413.0, 569.0, 761.0, 887.0, 1153.0, 1409.0)])
    body = [0.06 * m + n for m, n in zip(metal, white_noise(length, rng))]
    body = biquad(body, "highpass", 4200.0)
    body = one_pole_lowpass(body, 12000.0)
    attack = segments([(0.0, 0.0), (0.004, 1.0)], length)
    return fade_out(multiplied(multiplied(body, exponential_decay(length, decay)), attack), 0.05)


@lru_cache(maxsize=None)
def war_drum(variant=0, pitch_hz=95.0, decay=0.32):
    """Big low tom / taiko-like hit."""
    rng = _rng("war_drum", variant)
    length = samples(decay * 3.0)
    pitch = exponential_glide(pitch_hz * 1.6, pitch_hz, length, 0.025)
    shell = multiplied(sine(pitch, length), exponential_decay(length, decay))
    overtone = multiplied(sine([p * 1.58 for p in pitch], length), exponential_decay(length, decay * 0.35))
    skin = multiplied(one_pole_lowpass(white_noise(length, rng), 1400.0), exponential_decay(length, 0.03))
    hit = summed(shell, scaled(overtone, 0.3), scaled(skin, 0.6))
    return fade_out(soft_clip(hit, 1.4), END_FADE_SECONDS)


@lru_cache(maxsize=None)
def timpani(midi_note, decay=0.9, variant=0):
    rng = _rng("timpani", midi_note, variant)
    frequency = midi_to_frequency(midi_note)
    length = samples(decay * 3.0)
    tone = partials(
        [frequency, frequency * 1.5, frequency * 1.98, frequency * 2.44],
        [1.0, 0.5, 0.3, 0.15],
        [decay, decay * 0.6, decay * 0.4, decay * 0.25],
        length,
    )
    mallet_length = samples(0.03)
    mallet = multiplied(one_pole_lowpass(white_noise(mallet_length, rng), 900.0), exponential_decay(mallet_length, 0.008))
    mix_into(tone, mallet, 0, 0.8)
    return fade_out(tone, END_FADE_SECONDS)


@lru_cache(maxsize=None)
def bass_voice(midi_note, duration, brightness=1.0, drive=1.8):
    frequency = midi_to_frequency(midi_note)
    length = samples(duration + 0.04)
    oscillator = summed(scaled(saw(frequency, length), 0.7), scaled(square(frequency * 1.003, length, duty=0.4), 0.3))
    cutoff = exponential_glide(280.0 + 1800.0 * brightness, 220.0 + 260.0 * brightness, length, 0.07)
    filtered = one_pole_lowpass(one_pole_lowpass(oscillator, cutoff), cutoff)
    body = summed(scaled(filtered, 1.6), scaled(sine(frequency, length), 0.5))
    envelope = adsr(length, 0.003, 0.12, 0.75, 0.04)
    return soft_clip(multiplied(body, envelope), drive)


@lru_cache(maxsize=None)
def pad_voice(midi_note, duration, cutoff=1700.0, attack=0.35, release=0.9, detune_cents=9.0):
    frequency = midi_to_frequency(midi_note)
    length = samples(duration + release)
    rng = _rng("pad", midi_note)
    ratio = 2.0 ** (detune_cents / 1200.0)
    voices = [saw(frequency * r, length, rng.random()) for r in (1.0 / ratio, 1.0, ratio)]
    body = summed(*voices)
    body = biquad(biquad(body, "lowpass", cutoff, 0.6), "highpass", 160.0)
    envelope = adsr(length, attack, 0.5, 0.85, release)
    return scaled(multiplied(body, envelope), 1.0 / 3.0)


@lru_cache(maxsize=None)
def lead_voice(midi_note, duration, brightness=1.0, vibrato_depth=0.006):
    frequency = midi_to_frequency(midi_note)
    release = 0.12
    length = samples(duration + release)
    vibrato_onset = samples(0.18)
    pitch = [
        frequency * (1.0 + vibrato_depth * min(1.0, i / vibrato_onset) * math.sin(math.tau * VIBRATO_HZ * i / SAMPLE_RATE))
        for i in range(length)
    ]
    oscillator = summed(scaled(saw(pitch, length), 0.6), scaled(square([p * 1.002 for p in pitch], length, duty=0.32), 0.4))
    cutoff = exponential_glide(900.0 + 3600.0 * brightness, 900.0 + 1300.0 * brightness, length, 0.12)
    filtered = one_pole_lowpass(one_pole_lowpass(oscillator, cutoff), cutoff)
    filtered = one_pole_highpass(filtered, 220.0)
    return multiplied(filtered, adsr(length, 0.012, 0.25, 0.8, release))


@lru_cache(maxsize=None)
def brass_voice(midi_note, duration, brightness=1.0):
    frequency = midi_to_frequency(midi_note)
    release = 0.25
    length = samples(duration + release)
    pitch = exponential_glide(frequency * 0.97, frequency, length, 0.03)
    oscillator = summed(saw(pitch, length, 0.1), saw([p * 1.004 for p in pitch], length, 0.6))
    cutoff = segments(
        [(0.0, 300.0), (0.06, 900.0 + 3200.0 * brightness), (0.35, 700.0 + 1600.0 * brightness), (60.0, 700.0 + 1600.0 * brightness)],
        length,
    )
    filtered = one_pole_lowpass(one_pole_lowpass(oscillator, cutoff), cutoff)
    filtered = one_pole_highpass(filtered, 120.0)
    return scaled(multiplied(filtered, adsr(length, 0.035, 0.3, 0.8, release)), 0.5)


@lru_cache(maxsize=None)
def pluck_voice(midi_note, duration, decay=0.22):
    frequency = midi_to_frequency(midi_note)
    length = samples(min(duration, decay * 3.0) + 0.05)
    cutoff = exponential_glide(5000.0, 500.0, length, decay * 0.5)
    body = one_pole_lowpass(one_pole_lowpass(saw(frequency, length), cutoff), cutoff)
    body = one_pole_highpass(body, 200.0)
    return fade_out(multiplied(body, exponential_decay(length, decay)), END_FADE_SECONDS)


@lru_cache(maxsize=None)
def bell_voice(midi_note, ring_seconds=2.4, brightness=1.0):
    frequency = midi_to_frequency(midi_note)
    length = samples(ring_seconds)
    index = exponential_glide(2.2 * brightness, 0.2, length, 0.35)
    body = fm_tone(frequency, 3.5, index, length)
    fundamental = sine(frequency, length)
    tone = [0.55 * b + 0.45 * f for b, f in zip(body, fundamental)]
    tone = multiplied(tone, exponential_decay(length, ring_seconds / 3.5))
    return fade_out(tone, 0.05)


@lru_cache(maxsize=None)
def chime_voice(frequency, decay=0.35):
    length = samples(decay * 4.0)
    tone = partials(
        [frequency, frequency * 2.0, frequency * 3.0, frequency * 4.17],
        [1.0, 0.32, 0.12, 0.06],
        [decay, decay * 0.55, decay * 0.35, decay * 0.15],
        length,
    )
    attack = segments([(0.0, 0.0), (0.002, 1.0)], length)
    return fade_out(multiplied(tone, attack), END_FADE_SECONDS)
