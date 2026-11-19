"""Cuts the public-domain (CC0) recordings the weapon sounds are built from out of their source packs.

The excerpts are committed in tools/audio/recordings, so this only runs again to change or add one:

    python3 tools/audio/import_recordings.py --firearms DIR --bangs DIR --naval DIR

Sources (all CC0, https://creativecommons.org/publicdomain/zero/1.0/):
  firearms  The Free Firearm Sound Library, https://opengameart.org/content/the-free-firearm-sound-library
  bangs     25 CC0 bang / firework SFX, https://opengameart.org/content/25-cc0-bang-firework-sfx
  naval     Tiny Naval Battle Sounds Set (Iwan Gabovitch), https://opengameart.org/content/tiny-naval-battle-sounds-set
"""
import argparse
import subprocess
from pathlib import Path

from generate_audio import find_ffmpeg

RECORDINGS_DIR = Path(__file__).resolve().parent / "recordings"
SAMPLE_RATE = "44100"

# name: (pack, file in the pack, start seconds, length seconds); starts sit ~15 ms before the shot.
EXCERPTS = {
    "ak47_1": ("firearms", "Prepared SFX Library/AK-47/C_28P.wav", 0.59, 1.0),
    "ak47_2": ("firearms", "Prepared SFX Library/AK-47/C_28P.wav", 3.23, 1.0),
    "sks_1": ("firearms", "Prepared SFX Library/SKS/U_14P.wav", 3.56, 1.0),
    "sks_2": ("firearms", "Prepared SFX Library/SKS/U_14P.wav", 6.73, 1.0),
    "ar15_1": ("firearms", "Prepared SFX Library/AR-15/D_32P.wav", 0.68, 0.8),
    "ar15_2": ("firearms", "Prepared SFX Library/AR-15/D_32P.wav", 5.63, 0.8),
    "tikka": ("firearms", "Prepared SFX Library/Tikka/W_29P.wav", 0.55, 1.6),
    "shotgun": ("firearms", "Prepared SFX Library/Model 12/K_22P.wav", 0.82, 1.0),
    "naval_gun": ("naval", "qubodup-NavalBattleSoundSet-cc0/GunShot.wav", 0.0, 1.0),
    "metal_explosion": ("naval", "qubodup-NavalBattleSoundSet-cc0/ExplosionMetal.wav", 0.0, 1.51),
    "firework_cannon": ("bangs", "cannon_04.ogg", 0.0, 0.92),
    "firework_bang_1": ("bangs", "bang_01.ogg", 0.0, 1.03),
    "firework_bang_2": ("bangs", "bang_06.ogg", 0.0, 1.36),
    "firework_bang_3": ("bangs", "bang_09.ogg", 0.0, 1.1),
}


def cut(ffmpeg, source, start, length, target):
    subprocess.run(
        [ffmpeg, "-hide_banner", "-loglevel", "error", "-y", "-ss", str(start), "-t", str(length), "-i", str(source),
         "-ac", "1", "-ar", SAMPLE_RATE, "-sample_fmt", "s16", "-map_metadata", "-1", str(target)],
        check=True,
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    for pack in ("firearms", "bangs", "naval"):
        parser.add_argument(f"--{pack}", type=Path, required=True)
    packs = vars(parser.parse_args())
    RECORDINGS_DIR.mkdir(exist_ok=True)
    ffmpeg = find_ffmpeg()
    for name, (pack, file, start, length) in EXCERPTS.items():
        cut(ffmpeg, packs[pack] / file, start, length, RECORDINGS_DIR / f"{name}.flac")
        print(name)


if __name__ == "__main__":
    main()
