import array
import math
import wave

SAMPLE_RATE = 44100
TWO_PI = 2.0 * math.pi
NYQUIST_SAFE_HZ = 0.45 * SAMPLE_RATE

FREEVERB_COMB_DELAYS = (1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617)
FREEVERB_ALLPASS_DELAYS = (556, 441, 341, 225)
FREEVERB_STEREO_SPREAD = 23


def samples(seconds):
    return int(round(seconds * SAMPLE_RATE))


def silence(length):
    return [0.0] * length


def midi_to_frequency(midi_note):
    return 440.0 * 2.0 ** ((midi_note - 69) / 12.0)


def db_to_gain(decibels):
    return 10.0 ** (decibels / 20.0)


def _increments(frequency, length):
    if isinstance(frequency, (int, float)):
        return [frequency / SAMPLE_RATE] * length
    return [f / SAMPLE_RATE for f in frequency[:length]]


def _phases(increments, start_phase=0.0):
    phases = []
    append = phases.append
    phase = start_phase
    for increment in increments:
        append(phase)
        phase += increment
        if phase >= 1.0:
            phase -= 1.0
    return phases


def _poly_blep(t, dt):
    if t < dt:
        t /= dt
        return t + t - t * t - 1.0
    if t > 1.0 - dt:
        t = (t - 1.0) / dt
        return t * t + t + t + 1.0
    return 0.0


def sine(frequency, length, phase=0.0):
    sin = math.sin
    return [sin(TWO_PI * p) for p in _phases(_increments(frequency, length), phase)]


def saw(frequency, length, phase=0.0):
    increments = _increments(frequency, length)
    return [2.0 * t - 1.0 - _poly_blep(t, dt) for t, dt in zip(_phases(increments, phase), increments)]


def square(frequency, length, phase=0.0, duty=0.5):
    increments = _increments(frequency, length)
    out = []
    append = out.append
    for t, dt in zip(_phases(increments, phase), increments):
        level = 1.0 if t < duty else -1.0
        falling_phase = t - duty if t >= duty else t - duty + 1.0
        append(level + _poly_blep(t, dt) - _poly_blep(falling_phase, dt))
    return out


def triangle(frequency, length, phase=0.0):
    return [4.0 * abs(t - 0.5) - 1.0 for t in _phases(_increments(frequency, length), phase)]


def fm_tone(frequency, ratio, index, length):
    """Two-operator FM; `index` is a modulation depth (scalar or per-sample list)."""
    sin = math.sin
    carrier = _phases(_increments(frequency, length))
    modulator = _phases(_increments(frequency * ratio, length))
    if isinstance(index, (int, float)):
        index = [index] * length
    return [sin(TWO_PI * c + i * sin(TWO_PI * m)) for c, m, i in zip(carrier, modulator, index)]


def partials(frequencies, amplitudes, decays, length):
    out = silence(length)
    for frequency, amplitude, decay in zip(frequencies, amplitudes, decays):
        if frequency >= NYQUIST_SAFE_HZ:
            continue
        tone = multiplied(sine(frequency, length), exponential_decay(length, decay))
        mix_into(out, tone, 0, amplitude)
    return out


def white_noise(length, rng):
    random = rng.random
    return [2.0 * random() - 1.0 for _ in range(length)]


def pink_noise(length, rng):
    random = rng.random
    b0 = b1 = b2 = 0.0
    out = []
    append = out.append
    for _ in range(length):
        white = 2.0 * random() - 1.0
        b0 = 0.99765 * b0 + white * 0.0990460
        b1 = 0.96300 * b1 + white * 0.2965164
        b2 = 0.57000 * b2 + white * 1.0526913
        append(0.25 * (b0 + b1 + b2 + white * 0.1848))
    return out


def brown_noise(length, rng):
    random = rng.random
    level = 0.0
    out = []
    append = out.append
    for _ in range(length):
        level = 0.995 * level + 0.05 * (2.0 * random() - 1.0)
        append(level)
    return out


def exponential_decay(length, time_constant, start=1.0):
    factor = math.exp(-1.0 / (time_constant * SAMPLE_RATE))
    out = []
    append = out.append
    level = start
    for _ in range(length):
        append(level)
        level *= factor
    return out


def exponential_glide(start, end, length, time_constant):
    span = start - end
    return [end + span * e for e in exponential_decay(length, time_constant)]


def linear_glide(start, end, length):
    if length <= 1:
        return [start] * length
    step = (end - start) / (length - 1)
    return [start + step * i for i in range(length)]


def segments(points, length):
    """Piecewise-linear curve through (seconds, value) points, holding the last value."""
    out = []
    for (t0, v0), (t1, v1) in zip(points, points[1:]):
        count = samples(t1) - samples(t0)
        if count > 0:
            step = (v1 - v0) / count
            out.extend(v0 + step * i for i in range(count))
    out.extend([points[-1][1]] * max(0, length - len(out)))
    return out[:length]


def adsr(length, attack, decay, sustain, release):
    """Envelope whose release starts `release` seconds before the end of `length` samples."""
    release_samples = min(samples(release), length)
    gate = length - release_samples
    attack_samples = max(1, samples(attack))
    decay_samples = max(1, samples(decay))
    out = []
    append = out.append
    for i in range(gate):
        if i < attack_samples:
            append(i / attack_samples)
        elif i < attack_samples + decay_samples:
            append(1.0 - (1.0 - sustain) * (i - attack_samples) / decay_samples)
        else:
            append(sustain)
    level_at_release = out[-1] if out else 0.0
    for i in range(release_samples):
        append(level_at_release * (1.0 - (i + 1) / release_samples))
    return out


def _one_pole_coefficient(cutoff):
    return 1.0 - math.exp(-TWO_PI * min(cutoff, NYQUIST_SAFE_HZ) / SAMPLE_RATE)


def one_pole_lowpass(signal, cutoff):
    out = []
    append = out.append
    state = 0.0
    if isinstance(cutoff, (int, float)):
        coefficient = _one_pole_coefficient(cutoff)
        for x in signal:
            state += coefficient * (x - state)
            append(state)
        return out
    for x, c in zip(signal, [_one_pole_coefficient(c) for c in cutoff]):
        state += c * (x - state)
        append(state)
    return out


def one_pole_highpass(signal, cutoff):
    return [x - low for x, low in zip(signal, one_pole_lowpass(signal, cutoff))]


def _biquad_coefficients(kind, frequency, q):
    frequency = min(max(frequency, 10.0), NYQUIST_SAFE_HZ)
    w0 = TWO_PI * frequency / SAMPLE_RATE
    cos_w0 = math.cos(w0)
    alpha = math.sin(w0) / (2.0 * q)
    if kind == "lowpass":
        b0 = b2 = (1.0 - cos_w0) / 2.0
        b1 = 1.0 - cos_w0
    elif kind == "highpass":
        b0 = b2 = (1.0 + cos_w0) / 2.0
        b1 = -(1.0 + cos_w0)
    elif kind == "bandpass":
        b0, b1, b2 = alpha, 0.0, -alpha
    else:
        raise ValueError(f"unknown filter kind {kind}")
    a0 = 1.0 + alpha
    return b0 / a0, b1 / a0, b2 / a0, -2.0 * cos_w0 / a0, (1.0 - alpha) / a0


def biquad(signal, kind, frequency, q=0.707, update_interval=32):
    """RBJ biquad; a per-sample `frequency` list is resampled every `update_interval` samples."""
    sweeping = not isinstance(frequency, (int, float))
    b0, b1, b2, a1, a2 = _biquad_coefficients(kind, frequency[0] if sweeping else frequency, q)
    x1 = x2 = y1 = y2 = 0.0
    out = []
    append = out.append
    for i, x in enumerate(signal):
        if sweeping and i % update_interval == 0:
            b0, b1, b2, a1, a2 = _biquad_coefficients(kind, frequency[i], q)
        y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, x, y1, y
        append(y)
    return out


def feedback_echoes(signal, delay_samples, feedback):
    """Only the repeats (no dry signal) of a feedback delay, same length as the input."""
    echoes = silence(delay_samples)
    for start in range(delay_samples, len(signal), delay_samples):
        previous_input = signal[start - delay_samples:start]
        previous_echo = echoes[start - delay_samples:start]
        echoes.extend([x + feedback * e for x, e in zip(previous_input, previous_echo)])
    return echoes[:len(signal)]


def _comb(signal, delay, gain):
    out = signal[:delay]
    for start in range(delay, len(signal), delay):
        out.extend([x + gain * y for x, y in zip(signal[start:start + delay], out[start - delay:start])])
    return out


def allpass(signal, delay, gain):
    out = [-gain * x for x in signal[:delay]]
    for start in range(delay, len(signal), delay):
        out.extend([
            -gain * x + x_delayed + gain * y_delayed
            for x, x_delayed, y_delayed in zip(
                signal[start:start + delay], signal[start - delay:start], out[start - delay:start])
        ])
    return out


def _reverb_channel(signal, decay_seconds, spread):
    combined = silence(len(signal))
    comb_count = len(FREEVERB_COMB_DELAYS)
    for base_delay in FREEVERB_COMB_DELAYS:
        delay = base_delay + spread
        gain = 10.0 ** (-3.0 * delay / (decay_seconds * SAMPLE_RATE))
        mix_into(combined, _comb(signal, delay, gain), 0, 1.0 / comb_count)
    for base_delay in FREEVERB_ALLPASS_DELAYS:
        combined = allpass(combined, base_delay + spread, 0.5)
    return combined


def reverb(signal, decay_seconds, predelay=0.02, damping_cutoff=4500.0):
    """Stereo Schroeder/Freeverb-style reverb (wet only); damping is applied to the input."""
    damped = one_pole_lowpass(signal, damping_cutoff)
    predelay_samples = samples(predelay)
    delayed = silence(predelay_samples) + damped[:len(damped) - predelay_samples]
    return _reverb_channel(delayed, decay_seconds, 0), _reverb_channel(delayed, decay_seconds, FREEVERB_STEREO_SPREAD)


def mix_into(target, source, offset=0, gain=1.0):
    if offset < 0:
        source = source[-offset:]
        offset = 0
    end = min(len(target), offset + len(source))
    if end <= offset:
        return
    target[offset:end] = [t + gain * s for t, s in zip(target[offset:end], source)]


def scaled(signal, gain):
    return [gain * x for x in signal]


def multiplied(signal, envelope):
    return [x * e for x, e in zip(signal, envelope)]


def summed(*signals):
    out = silence(max(len(s) for s in signals))
    for signal in signals:
        mix_into(out, signal)
    return out


def fit_length(signal, length):
    return signal[:length] + silence(max(0, length - len(signal)))


def soft_clip(signal, drive=1.0):
    tanh = math.tanh
    normalizer = 1.0 / tanh(drive)
    return [normalizer * tanh(drive * x) for x in signal]


def soft_limit(signal, ceiling, knee_fraction=0.75):
    """Linear below the knee, tanh-compressed above it so nothing exceeds `ceiling`."""
    tanh = math.tanh
    knee = ceiling * knee_fraction
    headroom = ceiling - knee
    out = []
    append = out.append
    for x in signal:
        magnitude = abs(x)
        if magnitude <= knee:
            append(x)
        else:
            limited = knee + headroom * tanh((magnitude - knee) / headroom)
            append(limited if x > 0 else -limited)
    return out


def fade_in(signal, seconds):
    count = min(len(signal), samples(seconds))
    out = list(signal)
    for i in range(count):
        out[i] *= i / count
    return out


def fade_out(signal, seconds):
    count = min(len(signal), samples(seconds))
    out = list(signal)
    length = len(out)
    for i in range(count):
        out[length - count + i] *= 1.0 - (i + 1) / count
    return out


def pan_gains(position):
    """Equal-power gains for a pan position in [-1 (left), 1 (right)]."""
    angle = (position + 1.0) * math.pi / 4.0
    return math.cos(angle), math.sin(angle)


def peak(signal):
    return max((abs(x) for x in signal), default=0.0)


def rms(signal):
    return math.sqrt(sum(x * x for x in signal) / max(1, len(signal)))


def normalized_stereo(left, right, peak_dbfs):
    loudest = max(peak(left), peak(right))
    if loudest == 0.0:
        return left, right
    gain = db_to_gain(peak_dbfs) / loudest
    return scaled(left, gain), scaled(right, gain)


def write_wav(path, left, right):
    def to_pcm(x):
        return int(max(-1.0, min(1.0, x)) * 32767.0)

    interleaved = [0] * (2 * len(left))
    interleaved[0::2] = [to_pcm(x) for x in left]
    interleaved[1::2] = [to_pcm(x) for x in right]
    with wave.open(str(path), "wb") as wav:
        wav.setnchannels(2)
        wav.setsampwidth(2)
        wav.setframerate(SAMPLE_RATE)
        wav.writeframes(array.array("h", interleaved).tobytes())
