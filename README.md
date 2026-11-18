# Iron Tide

An original real-time strategy game in Rust ([Bevy](https://bevyengine.org) 0.20), version 1.1.0, in the
spirit of classic base-building RTS games and the open-source OpenRA engine: build a
base, harvest crystals, keep your power up, and destroy the enemy.
All code and 3D models are original. The models are generated procedurally with Blender.

## Running

```sh
make run-release      # or: make run (dev build)
make models           # regenerate the models in Blender 5.x (ONLY=tank,scout for a subset)
make models-fast      # export only, without rendering previews (no GPU needed)
make check            # cargo fmt --check + clippy -D warnings + tests
make package          # release build bundled with its assets in dist/
make capture CAPTURE=shot.png CAPTURE_AFTER=30 CAPTURE_FOCUS=52,10
make help             # every target
```

## Packages and releases

```sh
make package          # dist/iron-tide-<version>-<os>-<arch>.tar.gz: the release binary, its assets and this README
```

The game loads its assets from the folder next to the executable, so a package just needs unpacking.
CI (`.github/workflows/ci.yml`) runs `make check`, then packages Linux x86_64, macOS arm64 and Windows x86_64
(zip) builds and attaches them to the workflow run. Pushing a version tag also publishes a GitHub release with
the three packages:

```sh
git tag v1.1.0 && git push origin v1.1.0   # the tag should match the version in Cargo.toml
```

## Tests

```sh
make test               # everything (about 2 s in release)
make test-unit          # game logic: pathfinding, terrain, production, placement, combat math, settings, saves
make test-playability   # headless matches on a fixed 50 ms clock
cargo test --release --test missions   # campaign setups, objectives, multiple enemies, new units
cargo test --release --test fog        # fog of war: hidden enemies, exploration, spotting, saves
cargo test --release --test sound      # the simulation asks for the right sounds at the right moments
cargo test --release --test orders     # attack-move, patrol, waypoint queues, rally points
```

The playability tests run the real simulation systems (`SimulationPlugin`) without a window: units cross the
river through a ford, a refinery's harvester earns credits, barracks train queued infantry, low power slows
production, a tank beats a rifle squad, a gun turret stops a scout, artillery splash hits a group, destroyed
buildings free their ground, the enemy AI builds an economy and attacks, a defenceless player loses, a strong
scripted army wins, and saving then loading restores the battle.

## Balance

```sh
make balance              # 12 AI matches per pairing, about a minute on 10 cores
make balance MATCHES=40   # tighter numbers
```

`src/bin/balance.rs` runs the headless simulation (`iron_tide::headless`) in parallel and prints a Markdown report:

- **Equal-cost duels:** $3000 of one unit type attack-moves into $3000 of another, on both sides of the river.
  The score is the row's remaining army value minus the column's (+100 = the row wins untouched).
- **AI against AI:** the computer plays the Coalition side too. With fog of war, Normal plays a mirror, then
  Hard and Easy challenge Normal; a last mirror without fog shows what scouting changes. Sides alternate every
  match, so a favoured base site shows up in its own column.
- **Unit ledger:** how many of each unit the AIs built and lost, and the damage they dealt per credit spent.

Results after tuning (40 matches per pairing):

| Duel ($3000 a side) | Rifleman | Rocketeer | Scout | Tank | Bulwark | Howitzer |
| --- | --- | --- | --- | --- | --- | --- |
| **Rifleman** | 0 | +79 | −81 | −64 | −16 | −69 |
| **Rocketeer** | −79 | 0 | −40 | +57 | +53 | −62 |
| **Scout Buggy** | +81 | +40 | 0 | −49 | −45 | +22 |
| **Battle Tank** | +64 | −59 | +49 | 0 | −8 | +44 |
| **Bulwark** | +15 | −53 | +45 | +8 | 0 | +58 |
| **Howitzer** | +74 | +36 | −22 | −40 | −57 | −2 |

| Challenger | vs | fog | wins : losses (unfinished) | median length | SW : NE site wins |
| --- | --- | --- | --- | --- | --- |
| Normal | Normal | on | 13 : 21 (6) | 11.0 min | 18 : 16 |
| Hard | Normal | on | 21 : 6 (13) | 7.2 min | 15 : 12 |
| Easy | Normal | on | 1 : 32 (7) | 7.9 min | 18 : 15 |
| Normal | Normal | off | 19 : 14 (7) | 10.2 min | 11 : 22 |

Every unit has a counter: riflemen beat rocketeers, scouts hunt infantry, rocketeers kill armour, tanks and the
Bulwark beat light vehicles, and howitzers shell infantry but lose to anything fast that reaches them.
With fog of war neither corner is favoured (the map is point-symmetric, see Rules). Unfinished matches are
mostly mutual exhaustion: both sides out of units, money and harvesters, with only turrets left standing.
Without fog the north-east has had a slight edge over several runs; the cause is not yet known.

## Controls

| Input | Action |
| --- | --- |
| LMB / drag | select unit / box-select units (Shift adds) |
| RMB on ground | move (formation) |
| RMB on enemy | attack |
| Ctrl + RMB | attack-move: fight anything met on the way, then carry on |
| Alt + RMB | patrol between the current position and the target |
| Shift + RMB | queue the order behind the current ones (route shown for selected units) |
| RMB with a barracks or factory selected | set its rally point |
| RMB on crystals / refinery (harvester) | harvest / unload |
| RMB on a structure with engineers selected | capture it (enemy) / repair it fully (own, damaged) |
| Menu: tile LMB | build; a finished structure: click, then place it on the map |
| Menu: tile RMB | cancel and refund |
| Wall tile, then LMB drag on the map | lay a straight wall, $50 per segment (RMB / Esc ends) |
| T or REPAIR, then LMB on own structure | start or stop repairing it (stays on for more clicks; RMB / Esc ends it) |
| Y or SELL, then LMB on own structure | sell it for half its price (stays on for more clicks; RMB / Esc ends it) |
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

Settings (graphics: preset, shadows, AO, bloom, SMAA, vegetation, wind, VSync, frame rate limit, fullscreen;
gameplay: game speed, edge scrolling, scroll speed, damage numbers, health bars, overlay) are saved
automatically to `~/.config/iron_tide/settings.json` (Windows: `%APPDATA%\iron_tide`). Saved games live in
the `saves/` directory next to it: a quick save plus 3 slots.

The frame rate limit (30 / 60 / 120 fps or unlimited, default 60) keeps the game from spending CPU on frames
a display cannot show. While the window is in the background the game keeps playing, but at no more than
30 frames per second. Measured in the main menu on the development Mac: unlimited 79 % of one core, 60 fps 67 %,
30 fps 37 %.

What the CPU goes on, measured at a fixed 30 fps in a match: the scenery used to be 45 000 entities (a glTF
scene per grass tuft); merging each scenery model into one mesh brought that to 11 000 but did not change the
CPU, as Bevy skips static entities cheaply. The Low quality preset saves about 13 %. Running the game's
schedules on one thread saves about 13 % per frame, because most systems are chained and the multi-threaded
executor mostly paid for waking worker threads. The rest is Bevy's fixed cost per frame.

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
  1. Bridgehead (1 enemy, destroy everything; with a step-by-step tutorial)
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
  "available_buildings": [],
  "fog_of_war": true,
  "tutorial": false
}
```

Sites: `SouthWest`, `NorthEast`, `NorthWest`, `SouthEast`. Objectives: `"DestroyAllEnemies"`,
`"DestroyCommandCenters"`, `{ "Survive": { "minutes": N } }`, `{ "ReachCredits": { "credits": N } }`.
Empty `available_*` lists allow everything. `"tutorial": true` shows the step-by-step tutorial panel
(select, move, power plant, refinery, barracks, a rifleman, attack-move, destroy the enemy); each step
completes as soon as the player does it, and the panel can be hidden. Difficulty changes how fast the AI thinks, the size of its first
attack wave and its income (75 % / 100 % / 135 %).

## Rules

- **Factions:** Coalition (blue, the player) against up to three allied Syndicate commanders
  (Crimson red, Amber orange, Violet purple), each with its own base, economy and AI.
- **Economy:** the harvester mines crystals and unloads them at the refinery. Every refinery comes with a free harvester.
- **Power:** when consumption exceeds production, all production runs at half speed.
- **Engineer:** unarmed $500 infantry from the Barracks. Right-click an enemy structure (not a wall) with
  engineers selected: they walk in and capture it, damage and all. Right-click a damaged own structure: they
  repair it fully. Either way the engineer is used up. Other selected units attack the structure as usual.
- **Walls:** $50 concrete segments that block movement. Click the Wall tile, then drag on the map to lay a
  straight line; each segment is paid as it is placed and the mode stays on until right click / Esc. Every
  segment extends the area you may build in, so walls can reach out from the base. Walls do not keep a
  defeated side alive, and the computer does not pick them as targets for its attack waves.
- **Repair and sell:** a structure under repair regains 4 % of its health per second and pays as it goes:
  a repair from rubble to full health costs half its price (the Command Center counts as $3000). Repairs pause
  while the credits run out and stop at full health; a green cross turns above it meanwhile. Selling refunds
  half the price and the structure sinks into the ground. The Command Center cannot be sold. The computer
  repairs its own structures below 60 % health when it has credits to spare.
- **Tech tree:** Command Center → Power Plant → Barracks / Refinery → War Factory; Barracks → Gun Turret, Wall.
- **Units:** Rifleman, Rocketeer (anti-armour missiles), Field Medic (heals nearby infantry), Engineer, Scout Buggy,
  Battle Tank, Bulwark (heavy twin-cannon tank), Howitzer (long-range artillery with splash damage), Harvester.
- **Fair corners:** the map is point-symmetric around its centre: the river and its fords, the crystal fields
  and every tree or rock that blocks movement have a twin in the opposite corner. Bases in the far half face
  the other way, so every base's doors, refinery dock and new units point towards the middle of the map.
- **Veterancy:** units earn experience worth the price of every enemy unit or structure they destroy (the last
  hit counts). At their own price in experience they become Veterans (+20 % damage and health, one gold
  chevron), at three times it Elites (+40 %, two chevrons, and they slowly mend their own wounds). Structures
  that shoot do not gain ranks. Ranks are kept in saves.
- **Victory:** depends on the mission objective (destroy everything, destroy the Command Centers, survive, stockpile credits).
- **Fog of war:** unexplored ground is black; explored ground stays visible but enemy units show only within
  the sight of your units and buildings. Enemy structures stay on the map once spotted. Your units only open
  fire on enemies someone on your side can see, so long-range artillery needs a spotter. The computer
  opponents play by the same rules: allied Syndicate commanders share one view of the map, remember the
  structures they have spotted, and send a scout (a Scout Buggy if they have one) and then their attack
  waves to base sites they have not explored yet. Once their economy stands they shield each gun turret
  with a short L of wall facing the middle of the map, and once they know an enemy structure they keep an
  engineer that follows their attack wave to capture it. Levels can switch the fog off (`"fog_of_war": false`, or in the
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
| `structures`, `structure_tools` | repairing and selling structures; the repair / sell mouse modes, buttons and hint |
| `orders` | selection and player orders |
| `ai` | computer opponents: build plan, saving up for structures, gathered attack waves, base defense |
| `headless` | windowless battle harness shared by the tests and the balance tool |
| `terrain`, `terrain_material` | terrain height, roads, texture layers, WESL shader, water |
| `scenery`, `flat_scenery` | forests, meadows, reeds, mountains with boulders; scenery models merged into one mesh each |
| `minimap`, `notifications` | minimap, status messages |
| `ui`, `overlay` | sidebar (tabs, tiles with icons), health bars, selection |
| `settings`, `menu`, `perf_overlay`, `frame_pacing` | settings and their persistence, in-game menu, performance overlay, frame rate limit |
| `savegame` | saving and loading games (JSON) |
| `lib.rs` | `SimulationPlugin` (battle logic, also used headless by the tests) and `GamePlugin` (the full game) |
| `levels`, `match_flow`, `main_menu` | level definitions, campaign and custom levels; match setup and states; main menu and battle editor |
| `medical` | field medics healing infantry |
| `unit_orders` | move, attack-move, patrol and queued waypoints |
| `veterancy` | experience from kills, Veteran and Elite ranks and their bonuses |
| `engineers` | engineers capturing enemy structures and repairing own ones |
| `tutorial` | the step-by-step tutorial of levels with `"tutorial": true` |
| `sound` | sound cues from the simulation, playback with distance and fog, music director |
| `fog`, `fog_view` | fog of war: what the player knows, and how the terrain, water, minimap and enemies show it |
| `intel` | what each side knows: the Syndicate's shared view and the enemy structures each side has spotted |
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
