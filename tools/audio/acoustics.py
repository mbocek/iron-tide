"""Building blocks for believable outdoor gunfire and explosions.

A real shot is several sounds arriving together: the blast from the muzzle (an N-shaped pressure wave),
for rifles the crack of the bullet breaking the sound barrier, the mechanism, and then the battlefield
answering - reflections off ground, trees and hills arriving tens to hundreds of milliseconds later,
duller with every bounce. Heard from far away the high frequencies are gone and the reflections dominate.
"""
import random

from dsp import (
    biquad, brown_noise, exponential_decay, exponential_glide, fade_in, fit_length, mix_into, multiplied,
    normalized_stereo, one_pole_lowpass, pan_gains, reverb, samples, scaled, silence, sine, white_noise,
)

SFX_PEAK_DBFS = -1.5
FADE_OUT_SECONDS = 0.02
DISTANT_CUTOFF_HZ = 950.0
DISTANT_REVERB_SECONDS = 2.6
DISTANT_REVERB_WET = 0.7
DISTANT_ONSET_SECONDS = 0.018


def n_wave(length, duration):
    """The pressure signature of a supersonic shock or a muzzle blast: a jump up, a linear fall, a jump back."""
    wave = silence(length)
    span = max(2, samples(duration))
    for i in range(min(span, length)):
        wave[i] = 1.0 - 2.0 * i / span
    return wave


def muzzle_blast(rng, length, body_hz, size):
    """The expanding gas: a broadband thump whose weight grows with the calibre (`size` roughly 0.5-3)."""
    impulse = n_wave(length, 0.0015 * size)
    thump = one_pole_lowpass(one_pole_lowpass(impulse, body_hz * 2.0), body_hz * 2.0)
    blast = multiplied(white_noise(length, rng), exponential_decay(length, 0.008 * size))
    blast = biquad(blast, "lowpass", 5200.0 / size ** 0.4)
    low = multiplied(sine(exponential_glide(body_hz * 1.6, body_hz * 0.6, length, 0.02 * size), length),
                     exponential_decay(length, 0.03 * size))
    return [a * 2.2 + b * 1.4 + c * 0.9 * size ** 0.5 for a, b, c in zip(thump, blast, low)]


def supersonic_crack(rng, length):
    """The bullet's shock wave: a very short, bright N-wave, the sharpest part of a rifle shot."""
    crack = n_wave(length, 0.0004)
    hiss = multiplied(white_noise(length, rng), exponential_decay(length, 0.0015))
    return [a * 0.9 + b * 0.5 for a, b in zip(biquad(crack, "highpass", 1800.0), biquad(hiss, "highpass", 3000.0))]


def mechanism(rng, length, at_seconds, pitch_hz):
    """Bolt or breech clatter: a couple of short metallic ticks."""
    clatter = silence(length)
    for offset in (0.0, rng.uniform(0.012, 0.025)):
        tick = biquad(multiplied(white_noise(samples(0.01), rng), exponential_decay(samples(0.01), 0.0015)),
                      "bandpass", pitch_hz * rng.uniform(0.9, 1.15), 4.0)
        mix_into(clatter, tick, samples(at_seconds + offset), 0.6)
    return clatter


def outdoor_reflections(rng, dry, count, first, last, cutoff_hz, level):
    """Echoes from terrain and woods, each later one quieter and duller, scattered across the stereo field."""
    length = len(dry)
    left, right = silence(length), silence(length)
    for index in range(count):
        when = first + (last - first) * (index / max(1, count - 1)) ** 1.3 + rng.uniform(-0.01, 0.01)
        dullness = cutoff_hz / (1.0 + index * 0.6)
        echo = one_pole_lowpass(one_pole_lowpass(dry, dullness), dullness)
        gain = level * (0.75 ** index) * rng.uniform(0.7, 1.0)
        pan_left, pan_right = pan_gains(rng.uniform(-0.9, 0.9))
        mix_into(left, echo, samples(when), gain * pan_left)
        mix_into(right, echo, samples(when), gain * pan_right)
    return left, right


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
    """Earth, stones and metal raining down after a blast."""
    shower = silence(length)
    for _ in range(count):
        when = rng.uniform(start, end)
        loudness = gain * rng.uniform(0.25, 1.0) * (1.0 - 0.7 * (when - start) / max(0.01, end - start))
        piece_length = samples(0.06)
        piece = multiplied(white_noise(piece_length, rng), exponential_decay(piece_length, rng.uniform(0.003, 0.012)))
        piece = biquad(piece, "bandpass", rng.uniform(500.0, 3500.0), 1.3)
        mix_into(shower, piece, samples(when), loudness * 1.8)
    return shower


def explosion_blast(rng, length, size):
    """An explosive's shock, fireball roar and ground rumble; `size` 1 is a shell, 2-3 a vehicle or building."""
    shock = n_wave(length, 0.004 * size)
    shock = one_pole_lowpass(shock, 1800.0 / size ** 0.5)
    roar_cutoff = exponential_glide(6000.0 / size ** 0.3, 160.0, length, 0.12 * size)
    roar_envelope = multiplied([min(1.0, i / max(1, samples(0.004))) for i in range(length)],
                               exponential_decay(length, 0.12 * size))
    roar = multiplied(white_noise(length, rng), roar_envelope)
    roar = one_pole_lowpass(one_pole_lowpass(roar, roar_cutoff), roar_cutoff)
    boom = multiplied(sine(exponential_glide(70.0 / size ** 0.3, 24.0, length, 0.15 * size), length),
                      exponential_decay(length, 0.16 * size))
    rumble = one_pole_lowpass(multiplied(brown_noise(length, rng), exponential_decay(length, 0.3 * size)), 220.0)
    return [s * 4.0 + r * 4.0 + b * 1.4 + u * 1.2 for s, r, b, u in zip(shock, roar, boom, rumble)]


def seeded(seed):
    return random.Random(seed)
