import math
import random
from dataclasses import dataclass, field

from dsp import (
    SAMPLE_RATE, biquad, db_to_gain, fade_out, feedback_echoes, mix_into, one_pole_lowpass, pan_gains, reverb,
    rms, samples, saw, scaled, silence, soft_limit,
)
from instruments import (
    bass_voice, bell_voice, brass_voice, crash_cymbal, hi_hat, kick_drum, lead_voice, pad_voice, pluck_voice,
    snare_drum, timpani, war_drum,
)

PEAK_CEILING_DBFS = -2.0
NOTE_OFFSETS = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}
STEPS_PER_BEAT = 4
STEP_ACCENTS = {"X": 1.0, "x": 0.72, "o": 0.38}
ENCODER_FRAME_SAMPLES = 2048
SUBSONIC_CUTOFF_HZ = 32.0


def midi(name):
    """'C4' -> 60, 'Bb2' -> 46, 'F#5' -> 78."""
    letter, rest = name[0], name[1:]
    semitone = NOTE_OFFSETS[letter]
    while rest and rest[0] in "#b":
        semitone += 1 if rest[0] == "#" else -1
        rest = rest[1:]
    return 12 * (int(rest) + 1) + semitone


@dataclass
class Stem:
    length: int
    gain: float = 1.0
    pan: float = 0.0
    reverb_send: float = 0.0
    echo_beats: float = 0.0
    echo_feedback: float = 0.0
    echo_level: float = 0.0
    buffer: list = field(default=None)

    def __post_init__(self):
        self.buffer = silence(self.length)


class Arrangement:
    """A timeline of stems in beats; loops are folded so the end flows seamlessly into the start."""

    def __init__(self, tempo_bpm, total_beats, looping, seed, tail_seconds=6.0):
        self.length = samples(total_beats * 60.0 / tempo_bpm)
        if looping:
            self.length = loop_length_without_encoder_padding(self.length)
        self.seconds_per_beat = self.length / SAMPLE_RATE / total_beats
        self.tail = samples(tail_seconds)
        self.looping = looping
        self.rng = random.Random(seed)
        self.stems = {}

    def add_stem(self, name, **settings):
        self.stems[name] = Stem(self.length + self.tail, **settings)

    def offset(self, beat):
        return samples(beat * self.seconds_per_beat)

    def beats_to_seconds(self, beats):
        return beats * self.seconds_per_beat

    def place(self, stem, signal, beat, gain=1.0):
        mix_into(self.stems[stem].buffer, signal, self.offset(beat), gain)

    def humanized(self, gain, spread=0.08):
        return gain * (1.0 + self.rng.uniform(-spread, spread))

    def render(self, reverb_seconds, target_rms_dbfs):
        span = self.length if self.looping else self.length + self.tail
        left, right = silence(span), silence(span)
        reverb_bus = silence(span)
        for stem in self.stems.values():
            signal = self._folded(stem.buffer) if self.looping else stem.buffer
            if stem.echo_level > 0.0:
                echo_samples = self.offset(stem.echo_beats)
                echoes = self._processed(signal, lambda s: [feedback_echoes(s, echo_samples, stem.echo_feedback)])[0]
                echoes = one_pole_lowpass(echoes, 3500.0)
                mix_into(signal, echoes, 0, stem.echo_level)
            left_gain, right_gain = pan_gains(stem.pan)
            mix_into(left, signal, 0, stem.gain * left_gain)
            mix_into(right, signal, 0, stem.gain * right_gain)
            mix_into(reverb_bus, signal, 0, stem.gain * stem.reverb_send)
        wet_left, wet_right = self._processed(reverb_bus, lambda s: reverb(s, reverb_seconds, predelay=0.03, damping_cutoff=5000.0))
        mix_into(left, wet_left)
        mix_into(right, wet_right)
        left, right = self._processed(left, lambda s: [biquad(s, "highpass", SUBSONIC_CUTOFF_HZ)])[0], \
            self._processed(right, lambda s: [biquad(s, "highpass", SUBSONIC_CUTOFF_HZ)])[0]
        return mastered(left, right, target_rms_dbfs)

    def _folded(self, buffer):
        folded = buffer[:self.length]
        mix_into(folded, buffer[self.length:])
        return folded

    def _processed(self, signal, effect):
        """Applies `effect` (returning a tuple of channels); for loops, warmed up with the loop's own end."""
        if not self.looping:
            return effect(signal)
        warmup = min(self.tail, len(signal))
        channels = effect(signal[-warmup:] + signal)
        return tuple(channel[warmup:] for channel in channels)


def loop_length_without_encoder_padding(length):
    """The Vorbis encoder pads the last frame with silence, which would click at the loop point."""
    return round(length / ENCODER_FRAME_SAMPLES) * ENCODER_FRAME_SAMPLES


def mastered(left, right, target_rms_dbfs):
    loudness = math.sqrt(0.5 * (rms(left) ** 2 + rms(right) ** 2)) or 1.0
    gain = db_to_gain(target_rms_dbfs) / loudness
    ceiling = db_to_gain(PEAK_CEILING_DBFS)
    left = soft_limit(scaled(left, gain), ceiling)
    right = soft_limit(scaled(right, gain), ceiling)
    return left, right


def place_pattern(arrangement, stem, bar, pattern, hit, gain=1.0, beats_per_bar=4):
    """`pattern` is a 16th-note grid: X accent, x normal, o ghost, '.' rest; `hit()` returns a sample."""
    for step, symbol in enumerate(pattern):
        if symbol in STEP_ACCENTS:
            beat = bar * beats_per_bar + step / STEPS_PER_BEAT
            arrangement.place(stem, hit(), beat, arrangement.humanized(gain * STEP_ACCENTS[symbol]))


def place_melody(arrangement, stem, start_beat, phrase, voice, gain=1.0, transpose=0, legato=0.92):
    """`phrase` is a list of (note name or None for a rest, length in beats)."""
    beat = start_beat
    for note, beats in phrase:
        if note is not None:
            signal = voice(midi(note) + transpose, round(arrangement.beats_to_seconds(beats * legato), 3))
            arrangement.place(stem, signal, beat, arrangement.humanized(gain, 0.05))
        beat += beats


def chord_runs(progression):
    """Groups consecutive equal chord names: ['Dm','Dm','Bb'] -> [('Dm', 0, 2), ('Bb', 2, 1)]."""
    runs = []
    for bar, chord in enumerate(progression):
        if runs and runs[-1][0] == chord:
            name, start, count = runs[-1]
            runs[-1] = (name, start, count + 1)
        else:
            runs.append((chord, bar, 1))
    return runs


def place_pads(arrangement, stem, first_bar, progression, voicings, gain=1.0, voice=pad_voice, beats_per_bar=4):
    for chord, bar, bar_count in chord_runs(progression):
        seconds = round(arrangement.beats_to_seconds(bar_count * beats_per_bar), 3)
        for note in voicings[chord]:
            arrangement.place(stem, voice(midi(note), seconds), (first_bar + bar) * beats_per_bar, gain)


def variant_picker(rng, factory, variants=4):
    return lambda: factory(rng.randrange(variants))


def loop_seamlessly(signal, loop_length, crossfade):
    """Turns a continuous render of loop_length + crossfade samples into a click-free periodic loop."""
    out = signal[:loop_length]
    for i in range(crossfade):
        t = i / crossfade
        out[i] = out[i] * math.sin(0.5 * math.pi * t) + signal[loop_length + i] * math.cos(0.5 * math.pi * t)
    return out


BATTLE_ONE_VOICINGS = {
    "Dm": ("A3", "D4", "F4", "A4"),
    "Bb": ("Bb3", "D4", "F4", "Bb4"),
    "Gm": ("G3", "Bb3", "D4", "G4"),
    "A": ("A3", "C#4", "E4", "A4"),
    "C": ("G3", "C4", "E4", "G4"),
    "Eb": ("Bb3", "Eb4", "G4", "Bb4"),
}
BATTLE_ONE_ROOTS = {"Dm": "D2", "Bb": "Bb1", "Gm": "G1", "A": "A1", "C": "C2", "Eb": "Eb2"}
BATTLE_ONE_MAIN = ["Dm", "Dm", "Bb", "Bb", "Gm", "Gm", "A", "A"]
BATTLE_ONE_BRIDGE = ["Bb", "C", "Dm", "Dm", "Eb", "Eb", "A", "A"]
BATTLE_ONE_GALLOP = ((0, 0), (2, 0), (3, 0), (4, 12), (6, 0), (7, 0), (8, 0), (10, 0), (11, 0), (12, 12), (14, 7), (15, 0))
BATTLE_ONE_MOTIF = [
    ("A4", 1.5), ("G4", 0.5), ("A4", 1), ("D5", 1),
    ("C5", 1.5), ("A4", 0.5), ("F4", 2),
    ("D5", 1.5), ("C5", 0.5), ("Bb4", 1), ("D5", 1),
    ("F5", 2.5), ("D5", 0.5), (None, 1),
    ("G5", 1.5), ("F5", 0.5), ("D5", 1), ("Bb4", 1),
    ("C5", 1.5), ("D5", 0.5), ("Bb4", 2),
    ("A4", 1), ("C#5", 1), ("E5", 1), ("G5", 1),
    ("A5", 1.5), ("G5", 0.5), ("E5", 1), ("C#5", 1),
]
BATTLE_ONE_BRIDGE_MOTIF = [
    ("F5", 1), ("D5", 1), ("Bb4", 1), ("C5", 1),
    ("E5", 2), ("G5", 1), ("E5", 1),
    ("F5", 3), ("E5", 0.5), ("D5", 0.5),
    ("D5", 2), ("A4", 1), ("D5", 1),
    ("Eb5", 1.5), ("F5", 0.5), ("G5", 1), ("Bb5", 1),
    ("G5", 2), ("Eb5", 2),
    ("E5", 1.5), ("C#5", 0.5), ("A4", 1), ("E5", 1),
    ("G5", 1), ("F5", 1), ("E5", 1), ("C#5", 1),
]


def place_bass_ostinato(arrangement, stem, first_bar, progression, roots, figure, gain, step_seconds_factor=0.85):
    step_seconds = round(arrangement.beats_to_seconds(1.0 / STEPS_PER_BEAT) * step_seconds_factor, 3)
    for bar, chord in enumerate(progression):
        root = midi(roots[chord])
        for step, interval in figure:
            accent = 1.0 if step % 4 == 0 else 0.8
            arrangement.place(stem, bass_voice(root + interval, step_seconds), (first_bar + bar) * 4 + step / STEPS_PER_BEAT,
                              arrangement.humanized(gain * accent, 0.04))


def place_arpeggio(arrangement, stem, first_bar, progression, voicings, gain, order=(0, 1, 2, 3, 2, 1, 2, 3), steps_per_note=1, transpose=12):
    note_beats = steps_per_note / STEPS_PER_BEAT
    for bar, chord in enumerate(progression):
        notes = [midi(n) + transpose for n in voicings[chord]]
        for index in range(int(4 / note_beats)):
            note = notes[order[index % len(order)]]
            signal = pluck_voice(note, round(arrangement.beats_to_seconds(note_beats), 3))
            arrangement.place(stem, signal, (first_bar + bar) * 4 + index * note_beats, arrangement.humanized(gain, 0.1))


def battle_one():
    arrangement = Arrangement(tempo_bpm=128, total_beats=64 * 4, looping=True, seed=1101)
    rng = arrangement.rng
    arrangement.add_stem("kick", gain=0.5)
    arrangement.add_stem("snare", gain=1.0, reverb_send=0.18)
    arrangement.add_stem("hats", gain=0.5, pan=0.25)
    arrangement.add_stem("cymbals", gain=0.45, pan=-0.2, reverb_send=0.1)
    arrangement.add_stem("toms", gain=0.48, pan=-0.1, reverb_send=0.2)
    arrangement.add_stem("bass", gain=0.36)
    arrangement.add_stem("pad", gain=0.21, reverb_send=0.35)
    arrangement.add_stem("lead", gain=0.38, pan=0.08, reverb_send=0.3, echo_beats=0.75, echo_feedback=0.3, echo_level=0.25)
    arrangement.add_stem("arp", gain=0.2, pan=-0.35, reverb_send=0.25, echo_beats=0.75, echo_feedback=0.35, echo_level=0.35)

    kick = variant_picker(rng, kick_drum)
    snare = variant_picker(rng, snare_drum)
    hat = variant_picker(rng, hi_hat)
    open_hat = lambda: hi_hat(rng.randrange(2), 0.16)
    crash = lambda: crash_cymbal(0)
    tom_high = lambda: war_drum(rng.randrange(3), 120.0, 0.22)
    tom_low = lambda: war_drum(rng.randrange(3), 82.0, 0.3)

    sections = [
        ("intro", BATTLE_ONE_MAIN), ("motif", BATTLE_ONE_MAIN), ("motif_high", BATTLE_ONE_MAIN),
        ("bridge", BATTLE_ONE_BRIDGE), ("breakdown", BATTLE_ONE_MAIN), ("motif", BATTLE_ONE_MAIN),
        ("bridge_high", BATTLE_ONE_BRIDGE), ("finale", BATTLE_ONE_MAIN),
    ]
    for index, (section, progression) in enumerate(sections):
        first_bar = index * 8
        place_pads(arrangement, "pad", first_bar, progression, BATTLE_ONE_VOICINGS, gain=1.0 if section != "breakdown" else 1.2)
        if section == "breakdown":
            for bar, chord in enumerate(progression):
                arrangement.place("bass", bass_voice(midi(BATTLE_ONE_ROOTS[chord]), round(arrangement.beats_to_seconds(3.8), 3), 0.5),
                                  (first_bar + bar) * 4, 0.9)
        else:
            place_bass_ostinato(arrangement, "bass", first_bar, progression, BATTLE_ONE_ROOTS, BATTLE_ONE_GALLOP, 1.0)

        if section in ("motif", "finale"):
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_ONE_MOTIF, lead_voice)
        elif section == "motif_high":
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_ONE_MOTIF, lead_voice, transpose=12, gain=0.75)
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_ONE_MOTIF, lead_voice, gain=0.45)
        elif section == "bridge":
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_ONE_BRIDGE_MOTIF, lead_voice, transpose=-12)
        elif section == "bridge_high":
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_ONE_BRIDGE_MOTIF, lead_voice)
        if section in ("motif_high", "breakdown", "bridge_high", "finale"):
            place_arpeggio(arrangement, "arp", first_bar, progression, BATTLE_ONE_VOICINGS, 1.0)

        for bar in range(8):
            absolute_bar = first_bar + bar
            phrase_end = bar == 7
            if bar == 0 and section != "breakdown":
                arrangement.place("cymbals", crash(), absolute_bar * 4, 1.0)
            if section == "intro":
                place_pattern(arrangement, "kick", absolute_bar, "X.......X.......", kick)
                place_pattern(arrangement, "hats", absolute_bar, "x.x.x.x.x.x.x.x.", hat)
                if bar % 2 == 1:
                    place_pattern(arrangement, "toms", absolute_bar, "............X.x.", tom_low, 0.7)
            elif section == "breakdown":
                place_pattern(arrangement, "toms", absolute_bar, "X..x..X.....x.x.", tom_low)
                place_pattern(arrangement, "toms", absolute_bar, "......x.x.....x.", tom_high, 0.6)
                if bar >= 6:
                    roll = "o.o.o.o.x.x.x.x." if bar == 6 else "xxxxxxxxXXXXXXXX"
                    place_pattern(arrangement, "snare", absolute_bar, roll, snare, 0.5 + 0.1 * (bar - 6))
            elif section.startswith("bridge"):
                place_pattern(arrangement, "kick", absolute_bar, "X.....x...X.....", kick)
                place_pattern(arrangement, "snare", absolute_bar, "........X.......", snare)
                place_pattern(arrangement, "hats", absolute_bar, "x.xox.xox.xox.xo", hat)
            else:
                place_pattern(arrangement, "kick", absolute_bar, "X...x.x.X...x...", kick)
                place_pattern(arrangement, "snare", absolute_bar, "....X......oX..." if not phrase_end else "....X...x.xxXXXX", snare)
                place_pattern(arrangement, "hats", absolute_bar, "xoxoxoxoxoxoxo.." if bar % 2 else "xoxoxoxoxoxoxoxo", hat)
                if bar % 2 == 1 and not phrase_end:
                    place_pattern(arrangement, "hats", absolute_bar, "..............x.", open_hat, 0.8)
            if phrase_end and section in ("intro", "bridge", "bridge_high"):
                place_pattern(arrangement, "toms", absolute_bar, "........X.x.XxXx", lambda: war_drum(rng.randrange(3), 105.0, 0.25), 0.9)
    return arrangement.render(reverb_seconds=1.8, target_rms_dbfs=-15.9)


BATTLE_TWO_VOICINGS = {
    "Cm": ("G3", "C4", "Eb4"),
    "Db": ("Ab3", "Db4", "F4"),
    "Ab": ("Ab3", "C4", "Eb4"),
    "Fm": ("Ab3", "C4", "F4"),
    "G": ("G3", "B3", "D4"),
    "Eb": ("G3", "Bb3", "Eb4"),
    "Bb": ("F3", "Bb3", "D4"),
}
BATTLE_TWO_ROOTS = {"Cm": "C2", "Db": "Db2", "Ab": "Ab1", "Fm": "F1", "G": "G1", "Eb": "Eb2", "Bb": "Bb1"}
BATTLE_TWO_MAIN = ["Cm", "Cm", "Db", "Cm", "Ab", "Fm", "G", "G"]
BATTLE_TWO_SECOND = ["Fm", "Fm", "Db", "Db", "Eb", "Bb", "G", "G"]
BATTLE_TWO_RIFF = ((0, 0), (3, 0), (6, 12), (8, 0), (11, 0), (14, 7))
BATTLE_TWO_MOTIF = [
    ("C4", 2), ("Eb4", 1), ("G4", 1),
    ("G4", 3), ("Ab4", 0.5), ("G4", 0.5),
    ("Ab4", 2), ("F4", 1), ("Db4", 1),
    ("Eb4", 2), ("D4", 1), ("C4", 1),
    ("C5", 2), ("Bb4", 1), ("Ab4", 1),
    ("Ab4", 2), ("G4", 1), ("F4", 1),
    ("G4", 1), ("B4", 1), ("D5", 1), ("F5", 1),
    ("D5", 2), ("B4", 2),
]
BATTLE_TWO_SECOND_MOTIF = [
    ("C5", 2), ("Ab4", 1), ("F4", 1),
    ("G4", 1), ("Ab4", 1), ("C5", 2),
    ("Db5", 3), ("C5", 1),
    ("Ab4", 4),
    ("G4", 1.5), ("Bb4", 0.5), ("Eb5", 2),
    ("D5", 2), ("F5", 2),
    ("G5", 2), ("F5", 1), ("Eb5", 1),
    ("D5", 2), ("B4", 2),
]


def battle_two():
    arrangement = Arrangement(tempo_bpm=112, total_beats=56 * 4, looping=True, seed=2202)
    rng = arrangement.rng
    arrangement.add_stem("kick", gain=0.5)
    arrangement.add_stem("snare", gain=1.2, reverb_send=0.3)
    arrangement.add_stem("hats", gain=0.45, pan=-0.25)
    arrangement.add_stem("cymbals", gain=0.45, pan=0.2, reverb_send=0.1)
    arrangement.add_stem("war_drums", gain=0.41, pan=0.1, reverb_send=0.3)
    arrangement.add_stem("bass", gain=0.29)
    arrangement.add_stem("pad", gain=0.26, reverb_send=0.4)
    arrangement.add_stem("brass", gain=0.72, pan=-0.12, reverb_send=0.35)
    arrangement.add_stem("lead", gain=0.39, pan=0.12, reverb_send=0.35, echo_beats=1.5, echo_feedback=0.25, echo_level=0.2)
    arrangement.add_stem("arp", gain=0.14, pan=0.4, reverb_send=0.3, echo_beats=0.5, echo_feedback=0.3, echo_level=0.3)

    kick = lambda: kick_drum(rng.randrange(4), 120.0, 42.0, 0.26)
    snare = lambda: snare_drum(rng.randrange(4), 165.0, 0.2)
    hat = variant_picker(rng, hi_hat)
    crash = lambda: crash_cymbal(1, 1.2)
    war_low = lambda: war_drum(rng.randrange(3), 62.0, 0.4)
    war_mid = lambda: war_drum(rng.randrange(3), 88.0, 0.3)
    heavy_bass = lambda note, seconds: bass_voice(note, seconds, 0.7, 2.6)

    sections = [
        ("intro", BATTLE_TWO_MAIN), ("motif", BATTLE_TWO_MAIN), ("second", BATTLE_TWO_SECOND),
        ("breakdown", BATTLE_TWO_MAIN), ("motif_high", BATTLE_TWO_MAIN), ("second_high", BATTLE_TWO_SECOND),
        ("finale", BATTLE_TWO_MAIN),
    ]
    step_seconds = round(arrangement.beats_to_seconds(0.7), 3)
    for index, (section, progression) in enumerate(sections):
        first_bar = index * 8
        place_pads(arrangement, "pad", first_bar, progression, BATTLE_TWO_VOICINGS)
        for bar, chord in enumerate(progression):
            absolute_bar = first_bar + bar
            root = midi(BATTLE_TWO_ROOTS[chord])
            if section == "breakdown":
                arrangement.place("bass", heavy_bass(root, round(arrangement.beats_to_seconds(3.6), 3)), absolute_bar * 4, 0.8)
            else:
                for step, interval in BATTLE_TWO_RIFF:
                    arrangement.place("bass", heavy_bass(root + interval, step_seconds), absolute_bar * 4 + step / 4,
                                      arrangement.humanized(1.0 if step in (0, 8) else 0.85, 0.04))
            if section not in ("intro", "breakdown") and bar % 2 == 0:
                for note in BATTLE_TWO_VOICINGS[chord]:
                    arrangement.place("brass", brass_voice(midi(note), round(arrangement.beats_to_seconds(0.6), 3), 0.8), absolute_bar * 4, 0.8)
                    arrangement.place("brass", brass_voice(midi(note), round(arrangement.beats_to_seconds(0.4), 3), 0.6), absolute_bar * 4 + 1.5, 0.55)

        if section in ("motif", "finale"):
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_TWO_MOTIF, lead_voice)
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_TWO_MOTIF, lead_voice, transpose=-12, gain=0.5)
        elif section == "motif_high":
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_TWO_MOTIF, lead_voice, transpose=12, gain=0.8)
        elif section == "second":
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_TWO_SECOND_MOTIF, lead_voice)
        elif section == "second_high":
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_TWO_SECOND_MOTIF, lead_voice, transpose=12, gain=0.7)
            place_melody(arrangement, "lead", first_bar * 4, BATTLE_TWO_SECOND_MOTIF, lead_voice, gain=0.5)
        elif section == "breakdown":
            place_melody(arrangement, "arp", first_bar * 4, BATTLE_TWO_MOTIF, lambda n, s: bell_voice(n, 2.0, 0.6), gain=2.2)
        if section in ("motif_high", "second_high", "finale"):
            place_arpeggio(arrangement, "arp", first_bar, progression, BATTLE_TWO_VOICINGS, 1.0, order=(2, 1, 0, 1), steps_per_note=2)

        for bar in range(8):
            absolute_bar = first_bar + bar
            phrase_end = bar == 7
            if bar == 0 and section != "breakdown":
                arrangement.place("cymbals", crash(), absolute_bar * 4, 1.0)
            if section == "intro":
                place_pattern(arrangement, "kick", absolute_bar, "X.........X.....", kick)
                place_pattern(arrangement, "war_drums", absolute_bar, "X.....x.X..x....", war_low)
                place_pattern(arrangement, "hats", absolute_bar, "x...x...x...x...", hat)
            elif section == "breakdown":
                place_pattern(arrangement, "war_drums", absolute_bar, "X..x..x.X.....x.", war_low)
                place_pattern(arrangement, "war_drums", absolute_bar, "....x.......x.xx" if bar >= 6 else "....x.......x...", war_mid, 0.7)
            else:
                place_pattern(arrangement, "kick", absolute_bar, "X..x......X..x.." if bar % 2 == 0 else "X..x......X.xx..", kick)
                place_pattern(arrangement, "snare", absolute_bar, "........X.....o." if not phrase_end else "........X.xxXXXX", snare)
                place_pattern(arrangement, "hats", absolute_bar, "x.xxx.xxx.xxx.xx", hat)
                if section in ("second", "second_high", "finale"):
                    place_pattern(arrangement, "war_drums", absolute_bar, "X.....x.........", war_low, 0.8)
            if phrase_end and section in ("intro", "breakdown"):
                place_pattern(arrangement, "war_drums", absolute_bar, "........XxXxXXXX", war_mid, 0.9)
    return arrangement.render(reverb_seconds=2.2, target_rms_dbfs=-15.6)


MENU_VOICINGS = {
    "Em": ("G3", "B3", "E4"),
    "C": ("G3", "C4", "E4"),
    "Am": ("A3", "C4", "E4"),
    "B": ("F#3", "B3", "D#4"),
}
MENU_PROGRESSION = ["Em", "Em", "C", "C", "Am", "Am", "B", "B"]
MENU_MOTIF = [
    ("E5", 3), ("D5", 1),
    ("B4", 4),
    ("E5", 1.5), ("D5", 0.5), ("C5", 2),
    ("B4", 2), ("G4", 2),
    ("A4", 3), ("C5", 1),
    ("E5", 4),
    ("D#5", 2), ("F#5", 2),
    ("B4", 4),
]
MENU_OSTINATO_ROOTS = {"Em": "E2", "C": "C2", "Am": "A1", "B": "B1"}


def menu_drone(loop_length, bars_per_cycle_count):
    crossfade = samples(3.0)
    length = loop_length + crossfade
    voices = [(midi("E1"), 0.5), (midi("E2"), 0.35), (midi("B2"), 0.22), (midi("E2") + 0.07, 0.3)]
    body = silence(length)
    for note, level in voices:
        frequency = 440.0 * 2.0 ** ((note - 69) / 12.0)
        mix_into(body, saw(frequency, length), 0, level)
    cutoff = [190.0 + 230.0 * (0.5 - 0.5 * math.cos(2.0 * math.pi * bars_per_cycle_count * i / loop_length)) for i in range(length)]
    body = one_pole_lowpass(one_pole_lowpass(body, cutoff), cutoff)
    swell = [0.8 + 0.2 * math.sin(2.0 * math.pi * 5 * i / loop_length) for i in range(length)]
    body = [b * s for b, s in zip(body, swell)]
    return loop_seamlessly(body, loop_length, crossfade)


def menu_theme():
    bars = 24
    arrangement = Arrangement(tempo_bpm=64, total_beats=bars * 4, looping=True, seed=3303, tail_seconds=8.0)
    rng = arrangement.rng
    arrangement.add_stem("drone", gain=0.29, reverb_send=0.15)
    arrangement.add_stem("pad", gain=0.27, reverb_send=0.55)
    arrangement.add_stem("pulse", gain=0.38, reverb_send=0.45)
    arrangement.add_stem("ostinato", gain=0.5, pan=-0.2, reverb_send=0.35)
    arrangement.add_stem("ping", gain=0.19, pan=0.35, reverb_send=0.8, echo_beats=1.5, echo_feedback=0.4, echo_level=0.4)
    arrangement.add_stem("bell", gain=0.36, pan=0.15, reverb_send=0.6, echo_beats=1.0, echo_feedback=0.3, echo_level=0.3)
    arrangement.add_stem("shimmer", gain=0.38, pan=-0.3, reverb_send=0.7)

    mix_into(arrangement.stems["drone"].buffer, menu_drone(arrangement.length, 2))
    slow_pad = lambda note, seconds: pad_voice(note, seconds, 900.0, 1.6, 2.4, 12.0)
    pulse = lambda: one_pole_lowpass(war_drum(rng.randrange(3), 55.0, 0.45), 600.0)

    for phrase in range(3):
        first_bar = phrase * 8
        place_pads(arrangement, "pad", first_bar, MENU_PROGRESSION, MENU_VOICINGS, voice=slow_pad)
        for bar in range(8):
            absolute_bar = first_bar + bar
            arrangement.place("pulse", pulse(), absolute_bar * 4, 0.9)
            arrangement.place("pulse", pulse(), absolute_bar * 4 + 0.75, 0.55)
            if phrase == 2:
                arrangement.place("pulse", pulse(), absolute_bar * 4 + 2, 0.6)
            if bar % 2 == 0:
                arrangement.place("ping", bell_voice(midi("E6"), 1.6, 0.3), absolute_bar * 4 + 2.5, 1.0)
            if phrase >= 1:
                root = midi(MENU_OSTINATO_ROOTS[MENU_PROGRESSION[bar]])
                for eighth in range(8):
                    interval = 12 if eighth in (3, 6) else 0
                    note = pluck_voice(root + interval, round(arrangement.beats_to_seconds(0.5), 3), 0.18)
                    arrangement.place("ostinato", note, absolute_bar * 4 + eighth * 0.5, arrangement.humanized(1.0 if eighth % 2 == 0 else 0.7))
        if phrase >= 1:
            place_melody(arrangement, "bell", first_bar * 4, MENU_MOTIF, lambda n, s: bell_voice(n, 3.0, 0.7), legato=1.0)
        if phrase == 2:
            place_melody(arrangement, "bell", first_bar * 4, MENU_MOTIF, lambda n, s: bell_voice(n, 3.0, 0.4), gain=0.35, transpose=12, legato=1.0)
            for chord, bar, bar_count in chord_runs(MENU_PROGRESSION):
                seconds = round(arrangement.beats_to_seconds(bar_count * 4), 3)
                for note in MENU_VOICINGS[chord][1:]:
                    arrangement.place("shimmer", slow_pad(midi(note) + 24, seconds), (first_bar + bar) * 4)
    return arrangement.render(reverb_seconds=3.5, target_rms_dbfs=-17.0)


def victory_sting():
    arrangement = Arrangement(tempo_bpm=96, total_beats=9.6, looping=False, seed=4404, tail_seconds=0.0)
    rng = arrangement.rng
    arrangement.add_stem("brass", gain=0.5, reverb_send=0.4)
    arrangement.add_stem("lead", gain=0.6, pan=0.1, reverb_send=0.4)
    arrangement.add_stem("bass", gain=0.22)
    arrangement.add_stem("drums", gain=0.35, reverb_send=0.3)
    arrangement.add_stem("cymbals", gain=0.25, reverb_send=0.2)

    for step in range(8):
        arrangement.place("drums", snare_drum(rng.randrange(4), 200.0, 0.1), step * 0.125, 0.3 + 0.07 * step)
    place_melody(arrangement, "lead", 0, [("A4", 1 / 3), ("D5", 1 / 3), ("F#5", 1 / 3), ("A5", 2), ("F5", 1), ("G5", 1), ("A5", 4.6)],
                 lambda n, s: brass_voice(n, s, 1.2), legato=0.97)
    chords = [
        (1, 2, ("D4", "F#4", "A4", "D5"), "D2"),
        (3, 1, ("D4", "F4", "Bb4", "D5"), "Bb1"),
        (4, 1, ("E4", "G4", "C5", "E5"), "C2"),
        (5, 4.6, ("D4", "F#4", "A4", "D5", "F#5"), "D2"),
    ]
    for beat, beats, notes, bass in chords:
        seconds = round(arrangement.beats_to_seconds(beats * 0.95), 3)
        for note in notes:
            arrangement.place("brass", brass_voice(midi(note), seconds), beat, 0.55)
        arrangement.place("bass", bass_voice(midi(bass), seconds, 0.4, 1.2), beat)
        arrangement.place("drums", timpani(midi(bass) + 12, 0.8), beat, 1.0)
        arrangement.place("drums", kick_drum(0), beat, 0.7)
    arrangement.place("cymbals", crash_cymbal(2, 1.6), 1, 1.0)
    arrangement.place("cymbals", crash_cymbal(3, 1.8), 5, 1.2)
    for step in range(12):
        arrangement.place("drums", timpani(midi("D3"), 0.5, step % 3), 5.5 + step * 0.125, 0.12 + 0.04 * step * (1 - step / 14))
    left, right = arrangement.render(reverb_seconds=2.4, target_rms_dbfs=-16.2)
    return fade_out(left, 1.2), fade_out(right, 1.2)


def defeat_sting():
    arrangement = Arrangement(tempo_bpm=60, total_beats=6.0, looping=False, seed=5505, tail_seconds=0.0)
    arrangement.add_stem("pad", gain=0.4, reverb_send=0.5)
    arrangement.add_stem("lead", gain=0.5, pan=0.1, reverb_send=0.5)
    arrangement.add_stem("bass", gain=0.25, reverb_send=0.1)
    arrangement.add_stem("drums", gain=0.4, reverb_send=0.45)

    dark_pad = lambda note, seconds: pad_voice(note, seconds, 1000.0, 0.25, 1.2, 10.0)
    chords = [
        (0.0, 2.0, ("D3", "F3", "A3", "D4"), "D2"),
        (2.0, 1.0, ("D3", "G3", "Bb3", "D4"), "Bb1"),
        (3.0, 1.5, ("C#3", "E3", "A3", "C#4"), "A1"),
        (4.5, 1.5, ("D3", "F3", "A3", "D4"), "D1"),
    ]
    for beat, beats, notes, bass in chords:
        seconds = round(arrangement.beats_to_seconds(beats), 3)
        for note in notes:
            arrangement.place("pad", dark_pad(midi(note), seconds), beat, 0.8)
        arrangement.place("bass", bass_voice(midi(bass) + 12, seconds, 0.2, 1.1), beat, 0.9)
    place_melody(arrangement, "lead", 0, [("A4", 1), ("G4", 1), ("F4", 1), ("E4", 1.5), ("D4", 1.5)],
                 lambda n, s: lead_voice(n, s, 0.3, 0.004), legato=0.97)
    arrangement.place("drums", war_drum(0, 55.0, 0.6), 0.0, 1.0)
    arrangement.place("drums", timpani(midi("A2"), 1.0), 3.0, 0.6)
    arrangement.place("drums", war_drum(1, 48.0, 0.8), 4.5, 1.0)
    left, right = arrangement.render(reverb_seconds=3.0, target_rms_dbfs=-16.6)
    return fade_out(left, 1.0), fade_out(right, 1.0)


MUSIC = {
    "music/menu_theme": menu_theme,
    "music/battle_1": battle_one,
    "music/battle_2": battle_two,
    "music/victory": victory_sting,
    "music/defeat": defeat_sting,
}
