"""Places recorded gunfire and explosions on an open battlefield.

The recordings are dry; outdoors the battlefield answers - reflections off ground, trees and hills arriving
tens to hundreds of milliseconds later, duller with every bounce. Heard from far away the high frequencies
are gone and the reflections dominate.
"""
import random

from dsp import (
    allpass, biquad, exponential_decay, fade_in, fit_length, mix_into, multiplied, normalized_stereo,
    one_pole_lowpass, pan_gains, reverb, samples, silence, white_noise,
)

SFX_PEAK_DBFS = -1.5
FADE_OUT_SECONDS = 0.02
DISTANT_CUTOFF_HZ = 950.0
DISTANT_REVERB_SECONDS = 2.6
DISTANT_REVERB_WET = 0.7
DISTANT_ONSET_SECONDS = 0.018


def pressure_punch(rng, length, centre_hz, decay):
    """A push of air felt in the chest: band-limited noise rather than a tone, which would ring like a drum."""
    punch = multiplied(white_noise(length, rng), exponential_decay(length, decay))
    punch = biquad(biquad(punch, "lowpass", centre_hz * 2.2, 0.8), "lowpass", centre_hz * 2.2, 0.8)
    return biquad(punch, "highpass", centre_hz * 0.45, 0.7)


def outdoor_reflections(rng, dry, count, first, last, cutoff_hz, level):
    """Echoes from terrain and woods, each later one quieter, duller and more smeared, scattered across the
    stereo field. Exact copies of the shot would comb-filter into a metallic ring and repeat it like a drum."""
    length = len(dry)
    left, right = silence(length), silence(length)
    for index in range(count):
        when = first + (last - first) * (index / max(1, count - 1)) ** 1.3 + rng.uniform(-0.01, 0.01)
        dullness = cutoff_hz / (1.0 + index * 0.6)
        echo = scattered(rng, one_pole_lowpass(one_pole_lowpass(dry, dullness), dullness), 0.004 + 0.003 * index)
        gain = level * (0.75 ** index) * rng.uniform(0.7, 1.0)
        pan_left, pan_right = pan_gains(rng.uniform(-0.9, 0.9))
        mix_into(left, echo, samples(when), gain * pan_left)
        mix_into(right, echo, samples(when), gain * pan_right)
    return left, right


def scattered(rng, signal, spread_seconds):
    """Smears a reflection over `spread_seconds` the way trees, grass and uneven ground scatter it."""
    for share in (1.0, 0.61, 0.37):
        signal = allpass(signal, max(1, samples(spread_seconds * share * rng.uniform(0.85, 1.15))), 0.6)
    return signal


def outdoor_shot(dry, rng, duration, reflections=6, first=0.03, last=0.32, reflection_cutoff=2400.0,
                 reflection_level=0.45, tail_seconds=1.2, tail_wet=0.18):
    """A dry mono shot placed outdoors: reflections, a soft tail, trimmed and normalized to stereo."""
    length = samples(duration)
    dry = fit_length(dry, length)
    echo_left, echo_right = outdoor_reflections(rng, dry, reflections, first, last, reflection_cutoff, reflection_level)
    tail_left, tail_right = reverb(one_pole_lowpass(dry, 3000.0), tail_seconds, predelay=0.02, damping_cutoff=3500.0)
    left = [d + e + t * tail_wet for d, e, t in zip(dry, echo_left, fit_length(tail_left, length))]
    right = [d + e + t * tail_wet for d, e, t in zip(dry, echo_right, fit_length(tail_right, length))]
    return finished_pair(left, right)


def finished_pair(left, right):
    fade = samples(FADE_OUT_SECONDS)
    for channel in (left, right):
        for i in range(min(fade, len(channel))):
            channel[len(channel) - 1 - i] *= i / fade
    return normalized_stereo(fade_in(left, 0.0008), fade_in(right, 0.0008), SFX_PEAK_DBFS)


def distant_version(render):
    """The same sound heard from across the battlefield: no highs, a soft onset, and mostly echo."""
    def rendered():
        left, right = render()
        mono = [(a + b) * 0.5 for a, b in zip(left, right)]
        tail = samples(DISTANT_REVERB_SECONDS * 0.8)
        mono = mono + silence(tail)
        muffled = biquad(biquad(mono, "lowpass", DISTANT_CUTOFF_HZ), "lowpass", DISTANT_CUTOFF_HZ * 1.3)
        muffled = biquad(muffled, "highpass", 45.0)
        muffled = fade_in(muffled, DISTANT_ONSET_SECONDS)
        wet_left, wet_right = reverb(muffled, DISTANT_REVERB_SECONDS, predelay=0.05, damping_cutoff=1500.0)
        left = [d * 0.55 + w * DISTANT_REVERB_WET for d, w in zip(muffled, fit_length(wet_left, len(muffled)))]
        right = [d * 0.55 + w * DISTANT_REVERB_WET for d, w in zip(muffled, fit_length(wet_right, len(muffled)))]
        return finished_pair(left, right)
    return rendered


def debris(rng, length, count, start, end, gain):
    """Earth and stones raining down after a blast: dull thuds and gravel, never ringing tin."""
    shower = silence(length)
    for _ in range(count):
        when = rng.uniform(start, end)
        loudness = gain * rng.uniform(0.25, 1.0) * (1.0 - 0.7 * (when - start) / max(0.01, end - start))
        piece_length = samples(0.08)
        piece = multiplied(white_noise(piece_length, rng), exponential_decay(piece_length, rng.uniform(0.006, 0.02)))
        piece = biquad(piece, "lowpass", rng.uniform(350.0, 1600.0), 0.6)
        mix_into(shower, piece, samples(when), loudness * 2.6)
    return shower


def seeded(seed):
    return random.Random(seed)
