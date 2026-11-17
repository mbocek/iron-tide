# Iron Tide

An original real-time strategy game in Rust ([Bevy](https://bevyengine.org) 0.20), version 1.0.0, in the
spirit of classic base-building RTS games and the open-source OpenRA engine: build a
base, harvest crystals, keep your power up, and destroy the enemy.
All code and 3D models are original. The models are generated procedurally with Blender.

## Running

```sh
make run-release      # or: make run (dev build)
make models           # regenerate the models in Blender 5.x (ONLY=tank,scout for a subset)
make models-fast      # export only, without rendering previews (no GPU needed)
make check            # cargo fmt --check + clippy -D warnings
make capture CAPTURE=shot.png CAPTURE_AFTER=30 CAPTURE_FOCUS=52,10
make help             # every target
```

## Tests

```sh
make test               # everything (about 2 s in release)
make test-unit          # game logic: pathfinding, terrain, production, placement, combat math, settings, saves
make test-playability   # headless matches on a fixed 50 ms clock
cargo test --release --test missions   # campaign setups, objectives, multiple enemies, new units
cargo test --release --test fog        # fog of war: hidden enemies, exploration, spotting, saves
cargo test --release --test sound      # the simulation asks for the right sounds at the right moments
```

The playability tests run the real simulation systems (`SimulationPlugin`) without a window: units cross the
river through a ford, a refinery's harvester earns credits, barracks train queued infantry, low power slows
production, a tank beats a rifle squad, a gun turret stops a scout, artillery splash hits a group, destroyed
buildings free their ground, the enemy AI builds an economy and attacks, a defenceless player loses, a strong
scripted army wins, and saving then loading restores the battle.

## Controls

| Input | Action |
| --- | --- |
| LMB / drag | select unit / box-select units (Shift adds) |
| RMB on ground | move (formation) |
| RMB on enemy | attack |
| RMB on crystals / refinery (harvester) | harvest / unload |
| Menu: tile LMB | build; a finished structure: click, then place it on the map |
| Menu: tile RMB | cancel and refund |
| WASD, arrows, screen edge | scroll the camera (Shift = faster) |
| Middle mouse button + drag | drag the map |
| Minimap: click / drag | jump to that spot |
| Mouse wheel / Q, E / R | zoom / rotate / reset rotation |
| Space or Home | back to base |
| X / Esc | stop / deselect, cancel placement |
| Ctrl+1-9 (Cmd+1-9) | save the selection as a group |
| 1-9 / twice quickly | select a group / move the camera to it |
| Shift+1-9 | add a group to the selection |
| F1 | show controls in-game |
| F2 | graphics quality Low / Medium / High (also `IRON_TIDE_QUALITY=low\|medium\|high`) |
| F3 | performance overlay: FPS, frame time, GPU time, CPU (game / system), memory, entities |
| F5 / F9 | quick save / quick load |
| F10, Esc (when nothing is selected), MENU button | menu: game, graphics, gameplay, save/load (pauses the game) |

## Settings and saves

Settings (graphics: preset, shadows, AO, bloom, SMAA, vegetation, wind, VSync, fullscreen; gameplay:
game speed, edge scrolling, scroll speed, damage numbers, health bars, overlay) are saved automatically to
`~/.config/iron_tide/settings.json` (Windows: `%APPDATA%\iron_tide`). Saved games live in the `saves/`
directory next to it: a quick save plus 3 slots.

## Sound and music

All sound effects and music are original and synthesized by code (`tools/audio/generate_audio.py`,
pure Python plus ffmpeg), so there are no third-party licenses:

```sh
python3 tools/audio/generate_audio.py --out assets/audio
```

Battle sounds fade with distance from the camera and stay silent under the fog of war; repeated sounds
are throttled so a firing squad stays readable. The menu has its own theme, matches alternate two battle
themes, and the end of a match plays a victory or defeat sting. Master, music and effects volume live in
the menu's *Audio* tab.

## Missions and custom levels

The game opens in a main menu:
- **Campaign:** four missions (`assets/levels/*.json`), each with its own objective and unlocking new units.
  1. Bridgehead (1 enemy, destroy everything)
  2. Hold the Ford (survive 8 minutes, introduces Rocketeers and Medics)
  3. Iron Hammer (destroy both enemy Command Centers, introduces the Bulwark)
  4. Three Fronts (three Hard enemies)
- **Battle editor:** 1-3 enemy commanders, difficulty Easy/Normal/Hard, starting credits, objective
  (destroy everything / destroy Command Centers / survive 10 minutes / stockpile credits), available units,
  map variant and your starting corner. *Save as custom level* writes a JSON file to
  `~/.config/iron_tide/levels/`; every valid file there is listed under *Custom levels*.
- **Continue:** resumes the quick save. Saves remember their level.

A level file is plain JSON; omitted fields take defaults:

```json
{
  "name": "My battle",
  "briefing": "Text shown before the mission.",
  "map_seed": 1234,
  "player": { "site": "SouthWest", "credits": 5000, "buildings": ["PowerPlant"], "units": ["Rifleman", "Tank"] },
  "enemies": [
    { "site": "NorthEast", "credits": 5000, "difficulty": "Hard", "units": ["Rifleman", "Tank"] },
    { "site": "SouthEast", "credits": 4000, "difficulty": "Easy" }
  ],
  "objective": { "Survive": { "minutes": 10.0 } },
  "available_units": [],
  "available_buildings": []
}
```

Sites: `SouthWest`, `NorthEast`, `NorthWest`, `SouthEast`. Objectives: `"DestroyAllEnemies"`,
`"DestroyCommandCenters"`, `{ "Survive": { "minutes": N } }`, `{ "ReachCredits": { "credits": N } }`.
Empty `available_*` lists allow everything. Difficulty changes how fast the AI thinks, the size of its first
attack wave and its income (75 % / 100 % / 135 %).

## Rules

- **Factions:** Coalition (blue, the player) against up to three allied Syndicate commanders
  (Crimson red, Amber orange, Violet purple), each with its own base, economy and AI.
- **Economy:** the harvester mines crystals and unloads them at the refinery. Every refinery comes with a free harvester.
- **Power:** when consumption exceeds production, all production runs at half speed.
- **Tech tree:** Command Center → Power Plant → Barracks / Refinery → War Factory; Barracks → Gun Turret.
- **Units:** Rifleman, Rocketeer (anti-armour missiles), Field Medic (heals nearby infantry), Scout Buggy,
  Battle Tank, Bulwark (heavy twin-cannon tank), Howitzer (long-range artillery with splash damage), Harvester.
- **Victory:** depends on the mission objective (destroy everything, destroy the Command Centers, survive, stockpile credits).
- **Fog of war:** unexplored ground is black; explored ground stays visible but enemy units show only within
  the sight of your units and buildings. Enemy structures stay on the map once spotted. Your units only open
  fire on enemies someone on your side can see, so long-range artillery needs a spotter. The computer
  opponents currently see the whole map. Levels can switch the fog off (`"fog_of_war": false`, or in the
  battle editor).

## Code layout

| Module | Responsibility |
| --- | --- |
| `catalog` | unit and building statistics, weapons, armor |
| `map` | 64×64 cell grid, terrain with a river and fords, crystals, A* pathfinding |
| `spawn` | spawning units and buildings from the glTF models |
| `movement` | path following, unit separation |
| `combat` | target acquisition, chasing, turrets, projectiles, damage |
| `economy` | credits, power, harvester state machine |
| `production` | production queues, placing structures |
| `orders` | selection and player orders |
| `ai` | enemy AI: build plan, army, attack waves, base defense |
| `terrain`, `terrain_material` | terrain height, roads, texture layers, WESL shader, water |
| `scenery` | forests, meadows, reeds, mountains with boulders |
| `minimap`, `notifications` | minimap, status messages |
| `ui`, `overlay` | sidebar (tabs, tiles with icons), health bars, selection |
| `settings`, `menu`, `perf_overlay` | settings and their persistence, in-game menu, performance overlay |
| `savegame` | saving and loading games (JSON) |
| `lib.rs` | `SimulationPlugin` (battle logic, also used headless by the tests) and `GamePlugin` (the full game) |
| `levels`, `match_flow`, `main_menu` | level definitions, campaign and custom levels; match setup and states; main menu and battle editor |
| `medical` | field medics healing infantry |
| `sound` | sound cues from the simulation, playback with distance and fog, music director |
| `fog`, `fog_view` | fog of war: what the player knows, and how the terrain, water, minimap and enemies show it |
| `outcome` | mission objectives, victory and defeat |
| `capture` | `IRON_TIDE_CAPTURE=shot.png` saves a screenshot and quits (for development; `_SAVE_SLOT` / `_LOAD_SLOT` test saves) |

## Model contract (Blender → game)

1 Blender unit = 1 m, one cell = 2×2 m, the model's front faces Blender +X, the origin sits at the center of
its footprint on the ground. Rotating turrets are separate objects named `Turret` with their pivot at the origin.
Team variants are exported as `<id>_blue.glb` / `<id>_red.glb`.

## Third-party assets

- Terrain textures (`assets/textures/terrain/`): [Poly Haven](https://polyhaven.com), CC0 -
  leafy_grass, forrest_ground_01, rocks_ground_05, river_small_rocks, gravel_concrete_02.
- Font Rajdhani (`assets/fonts/`): Indian Type Foundry, SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).

(c) 2026 Iron Tide
