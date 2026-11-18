# Sound and music

All sound effects and music are original and synthesized by code (`tools/audio/generate_audio.py`,
pure Python plus ffmpeg), so there are no third-party licenses:

```sh
python3 tools/audio/generate_audio.py --out assets/audio
```

Gunfire is built like the real thing (`tools/audio/acoustics.py`): the muzzle blast as an N-shaped pressure
wave, a rifle bullet's supersonic crack, the bolt or breech, and the terrain answering with echoes 30 ms to
1.4 s later, each duller than the one before. Explosions add the shock, the fireball and debris falling. Every
weapon and blast also has a version heard from afar: no highs, a soft onset, mostly echo.

Where a sound is on screen decides how it is heard: the middle of the view is loudest and centred, the edges
are quieter and come from the left or right speaker, and off screen a sound fades by how far its kind
carries (a rifle half a screen, a collapsing building nearly three). Sounds off screen play their distant
version and arrive late, at the speed of sound; zooming out makes the battle quieter and distant too.
Sounds under the fog of war stay silent, and repeated sounds are throttled so a firing squad stays readable.

Units also make steady noise: tracked and wheeled engines that idle quietly and roar when they drive, the
Gunship's rotors, a squad's boots on the march, a harvester grinding crystal and engineers hammering and
welding at a structure under repair or just placed. Each is one seamless loop for every unit making it,
louder the more of them are in view (but ten tanks are not ten times one) and coming from where most of them
are; it follows the same rules of screen, zoom and fog and falls silent while the game is paused.

A radio operator reports in the interface language: unit ready, construction and research complete, the base
or a harvester under attack and a structure lost. The recordings are made by espeak-ng
(`tools/audio/voice.py`, `brew install espeak-ng`) and sent through a field radio: a narrow band, light
overdrive, hiss and squelch. One report plays at a time, after its chime, and routine news is not repeated
for a few seconds.

The menu has its own theme and matches alternate two battle themes. Each battle theme has a calm layer of
the same length (pads, bass and arpeggio); it plays while nothing fights on screen, and the full mix with its
drums and melody rises as the fighting in view grows and falls back slowly once it stops. The end of a match
plays a victory or defeat sting. Repairs start with a ratchet, sales ring up
coins, a captured structure sounds a radio fanfare and a promoted unit a short bugle call. Master, music and
effects volume live in the menu's *Audio* tab.
