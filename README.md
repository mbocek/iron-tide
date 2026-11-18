# Iron Tide

An original real-time strategy game in Rust ([Bevy](https://bevyengine.org) 0.20), version 1.2.0, in the
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
make dmg              # macOS: Iron Tide.app on a disk image in dist/
make capture CAPTURE=shot.png CAPTURE_AFTER=30 CAPTURE_FOCUS=52,10
make help             # every target
```

## Packages and releases

```sh
make package          # dist/iron-tide-<version>-<os>-<arch>.tar.gz: the release binary, its assets and this README
```

The game loads its assets from the folder next to the executable, so a package just needs unpacking.
CI (`.github/workflows/ci.yml`) runs `make check`, then packages Linux x86_64, macOS arm64, macOS x86_64
(Intel, cross-compiled on the Apple Silicon runner) and Windows x86_64 (zip) builds. Both macOS builds also
come as a disk image with `Iron Tide.app` (`make dmg` builds one locally). Pushing a version tag also
publishes a GitHub release with all the packages:

```sh
git tag v1.2.0 && git push origin v1.2.0   # the tag should match the version in Cargo.toml
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

Results after tuning (60 matches per pairing; the simulation is deterministic, so each pair of matches is played
on its own map variant, once from each corner):

| Duel ($3000 a side) | Rifleman | Rocketeer | Scout | Tank | Bulwark | Howitzer | Flame Trooper | Missile Truck |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Rifleman** | 0 | +80 | −84 | −70 | −20 | −68 | −40 | −84 |
| **Rocketeer** | −80 | 0 | −42 | +57 | +53 | −65 | −75 | −72 |
| **Scout Buggy** | +84 | +41 | 0 | −49 | −56 | +23 | +93 | +35 |
| **Battle Tank** | +68 | −57 | +49 | 0 | −8 | +53 | +84 | +62 |
| **Flame Trooper** | +39 | +77 | −93 | −84 | −83 | −92 | 0 | −93 |
| **Missile Truck** | +84 | +72 | −35 | −62 | −68 | +1 | +93 | 0 |

| Challenger | vs | fog | wins : losses (unfinished) | median length | SW : NE site wins |
| --- | --- | --- | --- | --- | --- |
| Normal | Normal | on | 24 : 26 (10) | 10.5 min | 29 : 21 |
| Hard | Normal | on | 41 : 5 (14) | 6.2 min | 24 : 22 |
| Normal | Hard | on | 8 : 38 (14) | 6.7 min | 22 : 24 |
| Easy | Normal | on | 8 : 38 (14) | 10.8 min | 32 : 14 |
| Normal | Normal | off | 33 : 26 (1) | 4.7 min | 33 : 26 |

The challenger plays the Coalition and the defender the Syndicate. The defender wins the Normal mirror more
often, but it does the same with both sides on one faction or with the factions swapped, so that is the
computer playing the player's team (which a person does in a real battle), not the factions. The south-west
corner also wins somewhat more often; scouting ties were ruled out as the cause. A commander that picks a
vehicle it cannot afford yet saves for it instead of choosing again, so the heavy units of the Tech Center
(Bulwark, Missile Truck, Howitzer, Gunship) now appear in a long match. Every unit has a counter: riflemen beat rocketeers, flame troopers burn any infantry but melt before
vehicles, scouts hunt infantry, rocketeers kill armour, tanks and the Bulwark beat light vehicles, and howitzers
and missile trucks shell infantry but lose to anything fast that reaches them. Unfinished matches are mostly
mutual exhaustion: both sides out of units, money and harvesters, with only turrets left standing. Hard used to
think every 0.6 s and lost to Normal because of it; it now thinks as often as Normal and wins on income and
bigger waves.

## Controls

| Input | Action |
| --- | --- |
| LMB / drag | select unit / box-select units (Shift adds); the box counts the units inside, selected units get white corner marks |
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
| F10, Esc (when nothing is selected), MENU button | menu: game, graphics, gameplay, keys, audio, save/load (pauses the game) |

The letter and function keys above are defaults: the menu's *Keys* tab rebinds camera scrolling, rotation,
back to base, stop, repair, sell, quick save / load and F1-F3 (click an action, press the new key; a key
already in use swaps with it). Arrows, Home, Esc, F10, the digits and the modifiers stay fixed. The
controls overview, buttons and tutorial show the current keys.

## Settings and saves

Settings (graphics: preset, shadows, AO, bloom, SMAA, vegetation, wind, VSync, frame rate limit, fullscreen;
gameplay: language, game speed, edge scrolling, scroll speed, damage numbers, health bars, overlay; key
bindings, stored by key name and only where they differ from the defaults) are saved
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

Profiling a match with `cargo build --release --features bevy/trace_chrome` (per-system spans in a Chrome
trace; capture runs render only the screenshot camera so the numbers match real play) showed the game's own
systems under 1 ms per second each, and the time in rendering. Every crystal is some fifteen meshes, so the
crystal fields were thousands of shadow casters: their floating shards and glowing cores no longer cast
shadows, and crystals off screen stop animating. Shadow queueing fell by about 70 % and the render thread
by about 10 %. Interface systems now write text, colours and sizes only when they change, because any write
makes Bevy measure text and lay out the whole interface again; that took about 7 % off the main thread.

## Updates

At start-up the game asks GitHub for the latest release of this repository (one request to
`api.github.com`, sent as `iron-tide/<version>` with nothing about the player). When a newer version has a
package for this computer, the main menu shows a banner; clicking it downloads the package into the
Downloads folder and then opens it: a disk image is mounted on macOS, the zip or archive is shown in its
folder on Windows and Linux. Installing stays in the player's hands. Release builds are signed and notarized once the certificates are configured in CI (see [docs/signing.md](docs/signing.md)); until then they are unsigned. The check
can be switched off with *Check for updates* in the menu's *Gameplay* tab, and capture runs never check.

## Determinism (groundwork for multiplayer)

Multiplayer will keep every machine in step by exchanging only orders, so the simulation must play out
identically from the same orders. All chance in the simulation (artillery scatter, the computer's unit
choices, where new units gather) comes from one generator seeded by the level. Units and structures get a
simulation id in the order the simulation spawned them, because Bevy's entity ids also count effects, which
differ from machine to machine. Schedules run on one thread in a fixed order, in the game and in the tests.
The battle advances in fixed 50 ms ticks (20 per second, the same step the tests use) whatever the frame rate;
units and projectiles are drawn part way between their last two ticks, and get their exact simulated
transform back before each tick, so drawing never changes the battle.
Presentation adds its own components to units at moments that differ from machine to machine, which
changes the order in which Bevy's queries visit them; so every simulation step whose outcome depends on that
order (firing, projectile hits, destruction, harvesting, healing, repairs, engineers, the computer's army
and turret walls, choosing a producer or a refinery) goes through `in_simulation_order` or a tie-break on
position. `determinism::simulation_checksum` fingerprints units, structures and credits. One test plays
the same computer-against-computer battle twice for eight minutes and compares the fingerprints every 15
seconds; another does the same while reshuffling the storage of units, structures and projectiles in one
of the two battles every second.

Everything the player does to the battle (orders to units, rally points, engineers, production and its
cancelling, placing structures and walls, repairing and selling) becomes a `PlayerCommand`, which names
units and structures by simulation id and is carried out at the start of a later tick
(`COMMAND_DELAY_TICKS`). The simulation checks that a team only commands its own units and structures.
Selecting, the camera and the menus stay local. Feedback such as the acknowledgement sound and the order
marker is immediate; the change itself happens one tick (50 ms) later. These commands are what machines
exchange in a networked match.

In a networked match (`network`) both machines run the whole battle. For every tick each sends a turn with
its player's commands for the tick four ahead (200 ms, `NETWORK_COMMAND_DELAY_TICKS`), and the simulation
takes a tick only once the other player's turn for it has arrived, so neither machine can run ahead. Messages
travel over TCP as length-prefixed JSON. Every second each machine sends the checksum of its battle; differing
checksums end the match as out of step, and a lost connection stops it. Tests join two battles in one process
over a real local TCP connection: their orders reach each other, the battles stay identical, a deliberate
drift is noticed by both, and a dropped connection halts the other side.

Each machine shows the battle from its own player's side (`LocalTeam`, read through `LocalView`): the fog
of war, hidden enemies, the minimap, sounds, health bars, the sidebar's credits, power and production,
selection, notifications, the camera's home and the victory or defeat banner all follow the local team,
while the simulation keeps every team's view and stays the same on both machines. If the connection drops
or the battles drift apart, the player is told once and the battle stops.

Up to four people can play: the host sets *Network players* (2 to 4) in the battle editor, the guests
take the Syndicate corners in the order they join, and the computer plays any corner left over. A
four-player battle with the editor's default alliance becomes a free-for-all; *Two against two* pairs the
host with the Amber player. Every guest connects to the host, which passes the guests' turns, checksums and
chat on to each other; a tick runs once the turns of every player have arrived. To play, everyone opens
*Network battle* in the main menu. The host clicks *Host a battle*: the battle
editor's settings with one opponent are used, the game listens on TCP port 47300 and shows this computer's
local address. The other player types that address (the port may be left out; the last address is
remembered) and clicks *Join*. Once connected, the host offers the battle and the guest reads it (objective,
bases, map, landscape, season, available units) before clicking *I'm ready*; both need the same game
version. When every guest is ready the host clicks *Start the battle*; all machines count down three seconds and
the battle begins, the host leading the faction chosen in the battle editor and the guests the other one.
Anyone who joins while every seat is taken is offered to watch instead: a spectator sees the whole map without
fog, follows everyone's orders and gives none, has no build menu or chat, and leaving does not stop the battle.
Spectators must join before the host starts. Every machine also records the battle as a replay. Either side can leave the lobby: a host whose guest leaves goes back to waiting
for another player. While hosting, the network page shows the battle editor's settings (all but the number
of players), and the host can still change them: every guest at once sees the new battle, is told the host
changed it, and has to click *I'm ready* again. Leaving or quitting a battle closes the connection at once, so the other player is told
straight away. During a network battle the round trip to the other machine (measured every second
with a ping, smoothed) shows in the corner of the battlefield, and when the battle has waited more than half
a second for the other player's orders a banner says so and counts the seconds. The players can chat: in the lobby,
once connected, typing goes into a message line and Enter sends it; in the battle Enter opens a *Say:* line
(the keyboard then types instead of commanding), Enter sends and Esc drops it, and lines appear among the
notifications. Lines are trimmed to 120 printable characters. Restarting, saving and loading are disabled during a
network battle; pausing one machine makes the other wait. Over the internet the host's port 47300 must be
forwarded. Capture runs can automate this (`IRON_TIDE_CAPTURE_HOST=1`, `IRON_TIDE_CAPTURE_JOIN=127.0.0.1`).

Both machines must compute floating point identically, also across processors and operating systems. Basic
arithmetic and square roots are exact under IEEE 754 everywhere, but Rust's own `sin`, `atan2` and `powf`
call each platform's maths library, whose last bit differs, and SIMD quaternion maths may round differently
on x86 and ARM. The simulation therefore takes those functions from the portable `libm` crate
(`sim_math`) and keeps positions and directions in scalar `Vec3`; rotations it writes serve only the
drawing. `tests/cross_platform.rs` replays a fixed computer-against-computer battle and compares its
fingerprints with `tests/fingerprints.txt`, recorded on an Apple Silicon Mac; CI runs the test on x86
Linux. When the rules change on purpose, record them again with
`UPDATE_FINGERPRINTS=1 cargo test --test cross_platform`.

## Languages

The interface speaks English, Slovak and Czech: choose it with the *Language* button in the main menu or in
the menu's *Gameplay* tab. Menus, the sidebar, notifications, the tutorial, the end screen and unit and
structure names and descriptions, and the campaign missions' names and briefings are translated; names and
briefings written for custom levels show as they were typed. Translations live
in `assets/i18n/sk.json` and `assets/i18n/cs.json`, each mapping the English text to its translation; they are
built into the game, so a change needs a new build. A text missing there simply stays English, and the tests
check that both files list the same texts with the same `{}` placeholders.
`tools/add_translations.py add "English" "Slovak" "Czech"` (or `remove "English"`) edits both files at once.

## Sound and music

All sound effects and music are original and synthesized by code (`tools/audio/generate_audio.py`,
pure Python plus ffmpeg), so there are no third-party licenses:

```sh
python3 tools/audio/generate_audio.py --out assets/audio
```

Battle sounds fade with distance from the camera and stay silent under the fog of war; repeated sounds
are throttled so a firing squad stays readable. The menu has its own theme, matches alternate two battle
themes, and the end of a match plays a victory or defeat sting. Repairs start with a ratchet, sales ring up
coins, a captured structure sounds a radio fanfare and a promoted unit a short bugle call. Master, music and
effects volume live in the menu's *Audio* tab.

## Missions and custom levels

The game opens in a main menu:
- **Campaign:** ten Coalition missions and three Syndicate missions (S1-S3) (`assets/levels/*.json`), each
  with its own objective. A level's `player_faction` decides which armoury the player builds from; every computer
  side fights for the other faction.
  1. Bridgehead (1 enemy, destroy everything; with a step-by-step tutorial)
  2. Hold the Ford (survive 8 minutes, introduces Rocketeers and Medics; in the rain)
  3. Iron Hammer (destroy both enemy Command Centers, introduces the Bulwark; on the lake)
  4. Three Fronts (three Hard enemies; on the plains)
  5. Silent Takeover (capture the walled-in enemy Command Center with an engineer; on the plains)
  6. Every Man for Himself (three cartels in a free-for-all; on the lake, in the rain)
  7. Rotor Storm (survive 12 minutes against a Crimson air force of gunships; flak and rocketeers; autumn)
  8. White Silence (destroy two Command Centers on a large winter plain in falling snow; snipers to scout)
  9. Marksmen (infantry only: snipers, medics and an engineer to capture the Crimson Command Center; autumn rain)
  10. The Iron Tide (two against two on a huge map: with Amber as an ally, destroy Crimson and Violet)
  - S1. Red Ledger (Syndicate: burn out a small Coalition post with Flame Troopers)
  - S2. Bunker Line (Syndicate: hold 12 minutes against Bulwarks and gunships behind Rocket Bunkers)
  - S3. Rocket Rain (Syndicate: shell the defences with Missile Trucks and destroy both Command Centers; large
    winter plain)
- **Mines:** with engineers selected, M (rebindable) buries a mine under each of them for $100. A mine arms after
  1.5 s and goes off under the first enemy ground unit that touches it (110 damage with splash); its own side
  and allies see it as a small disc, enemies never do. Computer commanders with money to spare send an engineer
  to mine the approach to their base, up to four mines each. Mines are not kept in save games.
- **Camouflage:** a sniper that stands still for 3 s without firing is hidden from enemies: they neither see nor
  target it until one of them comes within 8 m. A shot gives it away for 2 s.
- **Map preview:** the main menu shows the map of the selected mission, custom level, random battle or battle
  editor setup beside its description: ground, water, forests and rocks, crystal fields and every side's base,
  generated by the same code as the battle.
- **Random battle:** choose 1-3 computer opponents, their difficulty (or *Mixed*, a roll for each), the map size,
  the starting credits (the same for every side) and your faction (or a random one). The dice pick the landscape,
  season, weather, the corners, the alliances between several opponents and the shape of the land: how the
  river winds and where its outer fords lie, the lake's size, ponds on the plains, the hills and mountains. They
  also scatter extra forests, rock outcrops and crystal fields. Everything is point-symmetric around the map
  centre, so neither corner is favoured. A level's `terrain_seed` shapes the land; levels without one keep the
  classic landscape. *New map* rolls
  again; changing the options keeps the map. The rolled battle can be played, saved as a custom level or opened
  in the map editor.
- **Battle editor:** 1-3 enemy commanders (allied or free-for-all), landscape, difficulty Easy/Normal/Hard, starting credits, objective
  (destroy everything / destroy Command Centers / survive 10 minutes / stockpile credits), available units,
  map variant and your starting corner. *Save as custom level* writes a JSON file to
  `~/.config/iron_tide/levels/`; every valid file there is listed under *Custom levels*.
- **Continue:** resumes the quick save. Saves remember their level.
- **Replays:** every match is recorded and saved when it ends (or as *Unfinished* when you leave it or close
  the game). The newest 10 are listed under *Replays* and kept in `~/.config/iron_tide/replays/`. Watching one
  rebuilds the map and shows every unit and structure where it was, walking and driving, with shells,
  missiles and explosions, without the fog of war: Space pauses, `+` / `-` changes the speed (1x-8x), the Left
  and Right arrows jump 10 s, Home starts over, Esc returns to the menu. A replay stores snapshots twice a
  second plus the shots and blasts in between (a few MB for half an hour), not the inputs: the simulation is
  not deterministic enough to re-run them.

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
  "tutorial": false,
  "alliance": "CoalitionAgainstSyndicate",
  "landscape": "River",
  "weather": "Clear",
  "season": "Summer",
  "map_size": "Standard"
}
```

Landscapes: `River` (a winding river with three fords), `Lake` (a long lake between the bases with one
causeway across its middle) and `Plains` (open ground without water); all of them are point-symmetric.
Weather: `Clear` or `Rain` (falling rain, a dimmer sun and sky).
Map sizes: `Standard` (64 × 64 cells, every campaign mission), `Large` (96 × 96) and `Huge` (128 × 128), also
in the battle editor. Landscapes, roads and crystal fields are authored for the standard map: on bigger maps
the bases stay in the corners, crystal fields and road bends near a base keep their distance from it, and
everything else spreads out from the centre (the fords move apart, the lake grows longer), while the river
keeps its width. Forests, rocks and meadows grow with the area, so a Huge map has about 2.5 times as much
scenery and costs more to draw. The map size is chosen when a match starts and kept per thread: every schedule runs on the thread that updates the app, so tests can play battles of different sizes side by side.
Seasons: `Summer`, `Autumn` (birches turn yellow, broadleaf trees orange, shrubs red, grass straw; pines stay
green; a warmer sun) or `Winter` (snow on open flat ground and on top of rocks, trees and logs, frosted
leaves, a colder sun, and rain falls as snow). Each scenery model's colours are turned once per season when
it is loaded, so seasons cost nothing while playing. The campaign's third and eighth missions are fought in winter, its fifth, seventh and
ninth in autumn.
Sites: `SouthWest`, `NorthEast`, `NorthWest`, `SouthEast`. Objectives: `"DestroyAllEnemies"`,
`"DestroyCommandCenters"`, `"CaptureCommandCenter"`, `{ "Survive": { "minutes": N } }`, `{ "ReachCredits": { "credits": N } }`.
Empty `available_*` lists allow everything. `"tutorial": true` shows the step-by-step tutorial panel
(select, move, power plant, refinery, barracks, a rifleman, attack-move, destroy the enemy); each step
completes as soon as the player does it, and the panel can be hidden. Difficulty changes how fast the AI thinks, the size of its first
attack wave and its income (75 % / 100 % / 135 %).

## Rules

- **Factions:** Coalition (blue, the player) against up to three Syndicate commanders (Crimson red, Amber
  orange, Violet purple), each with its own base, economy and AI. They are allies and share what they see,
  unless the level says otherwise with `"alliance"` (or *Opponents* in the battle editor): `"FreeForAll"`
  makes everyone fight everyone, each seeing only through its own units and buildings, and `"TwoAgainstTwo"`
  pairs the Coalition with Amber against Crimson and Violet (the two western corners against the two eastern
  ones with the default sites); allies share what they see. Older levels' `"free_for_all": true` still works.
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
- **Tech tree:** Command Center → Power Plant → Barracks / Refinery → War Factory → Tech Center, Repair Bay,
  Helipad; Barracks → Gun Turret, Flak Cannon, Rocket Bunker, Wall. The Bulwark, the Missile Truck, the Howitzer
  and the Gunship also need a Tech Center.
- **Factions:** the Coalition and the Syndicate share most of the arsenal, but each has its own: only the
  Coalition trains Snipers and builds Bulwarks; only the Syndicate trains Flame Troopers (short-range flamers that
  burn infantry and buildings), builds Missile Trucks (wheeled rocket artillery, 18 m range) and Rocket Bunkers
  (a defense that hits vehicles and aircraft). The build menu shows only your own faction's items.
- **Units:** Rifleman, Rocketeer (anti-armour missiles), Sniper (long-range anti-infantry), Field Medic (heals
  nearby infantry), Engineer, Scout Buggy, Battle Tank, Bulwark (heavy twin-cannon tank), Howitzer (long-range
  artillery with splash damage), Flak Track (mobile anti-aircraft gun), Gunship, Harvester.
- **Map editor:** EDIT MAP on the battle editor page or on a custom level opens the map in the editor. Paint
  crystal fields, forests and rocks with the left button (keys 1-4 pick the brush), clear ground with the right
  one, in brushes of 1, 3 or 5 cells. Mirroring, on by default, paints the opposite cell too so both halves stay
  equally fair; base sites cannot be built over. SAVE writes the level (with its `map_edits`) to the custom
  levels, overwriting the custom level it was opened from. The LEVEL page sets the name, the briefing (click the
  field and type, Enter ends), the objective, and which corner each side starts in, with one to three computer
  opponents and their difficulty. Bases stay in the four corners, where the ground is levelled for them. PLAY IT
  starts a battle on the level as it stands.
- **Research:** the RESEARCH tab of the build menu offers three one-time upgrades from the Tech Center, which
  must stand while the research runs: Composite Armour ($1500, vehicles and aircraft take 20 % less damage),
  High-Explosive Warheads ($2000, every weapon deals 15 % more damage) and Combat Drills ($1000, infantry
  reload 25 % faster). The computer researches them too once it has credits to spare.
- **Field repairs:** a vehicle standing still on a cell touching its own Repair Bay ($800, after the War Factory)
  regains 4 % of its health per second for free.
- **Aircraft:** the Gunship (Helipad, $1100) takes off from the pad and flies straight to where it is sent, over
  water, walls and units, and tears through armour with its rocket pods. An aircraft hovering over its own
  Helipad (within 3 m of the pad's centre, not moving) regains 4 % of its health per second for free. Tank cannons and howitzers cannot shoot upwards at all;
  riflemen and turrets hit it weakly, scout buggies better and rocketeers well. The dedicated counters are
  the Flak Track ($700, War Factory) and the Flak Cannon ($700 structure, after the Barracks), whose bursting
  shells do double damage to aircraft; the Flak Cannon cannot shoot at the ground at all.
- **Fair corners:** the map is point-symmetric around its centre: the river and its fords, the crystal fields
  and every tree or rock that blocks movement have a twin in the opposite corner. Bases in the far half face
  the other way, so every base's doors, refinery dock and new units point towards the middle of the map.
- **Veterancy:** units earn experience worth the price of every enemy unit or structure they destroy (the last
  hit counts). At their own price in experience they become Veterans (+20 % damage and health, one gold
  chevron), at three times it Elites (+40 %, two chevrons, and they slowly mend their own wounds). Structures
  that shoot do not gain ranks. Ranks are kept in saves.
- **Battlefield:** shells and explosions leave scorch marks that fade after 40 s; destroyed vehicles leave
  their charred hulls, smoking for a while, until they sink into the ground; damaged structures smoke and
  burn. All of it only where you can see.
- **Victory:** depends on the mission objective (destroy everything, destroy the Command Centers, capture one, survive,
  stockpile credits). The end screen compares you with your opponents (units trained and lost, structures built and
  lost, enemies destroyed, damage, credits harvested, match time) and offers the replay of the match.
- **Fog of war:** unexplored ground is black; explored ground stays visible but enemy units show only within
  the sight of your units and buildings. Enemy structures stay on the map once spotted. Your units only open
  fire on enemies someone on your side can see, so long-range artillery needs a spotter. The computer
  opponents play by the same rules: allied Syndicate commanders share one view of the map, remember the
  structures they have spotted, and send a scout (a Scout Buggy if they have one) and then their attack
  waves to base sites they have not explored yet. Once their economy stands they shield each gun turret
  with a short L of wall facing the middle of the map, and once they know an enemy structure they keep an
  engineer that follows their attack wave to capture it. They also count the enemy infantry, vehicles and
  aircraft they have seen lately (older sightings fade) and train more of whatever answers them best,
  judged by each unit's weapon against those armour classes; against aircraft they dig in up to two Flak
  Cannons first. Their vehicles below 35 % health fall back to a Repair Bay and their aircraft to a Helipad,
  and rejoin the fight at 90 %. Up to three scout buggies or gunships waiting at home dash off to hunt any
  enemy harvester they can see, and a wave of six or more splits: every other unit marches round a flank
  point 24 m beside the direct road before joining the attack. Levels can switch the fog off (`"fog_of_war": false`, or in the
  battle editor).

## Code layout

| Module | Responsibility |
| --- | --- |
| `catalog` | unit and building statistics, weapons, armor |
| `map` | the cell grid (64, 96 or 128 cells a side), terrain with a river and fords, crystals, A* pathfinding |
| `spawn` | spawning units and buildings from the glTF models |
| `movement` | path following, unit separation |
| `combat` | target acquisition, chasing, turrets, projectiles, damage |
| `economy` | credits, power, harvester state machine |
| `production` | production queues, placing structures |
| `structures`, `structure_tools` | repairing and selling structures; the repair / sell mouse modes, buttons and hint |
| `orders` | selection and player orders |
| `ai` | computer opponents: build plan, saving up for structures, gathered attack waves, base defense |
| `army_mix` | what the computer has seen of the enemy army and which units counter it |
| `headless` | windowless battle harness shared by the tests and the balance tool |
| `terrain`, `terrain_material` | landscapes (river, lake, plains): terrain height, water, roads, texture layers, WESL shader |
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
| `i18n` | interface languages: loads `assets/i18n/*.json`, `tr` / `tr_fmt` |
| `interpolation` | drawing moving things between simulation ticks |
| `lobby` | hosting and joining a network battle from the main menu |
| `local_view` | the battle as this machine's player sees it, for drawing, sound and the interface |
| `network_hud` | the ping and the banner shown while waiting for the other player |
| `network` | the lockstep session with a player on another machine: turns, checksums, disconnection |
| `player_commands` | player commands, their queue by tick, and carrying them out in the simulation |
| `sim_math` | the portable trigonometry the simulation uses so every processor computes the same battle |
| `determinism` | the seeded simulation randomness, simulation ids, single-threaded schedules and the state checksum |
| `updates` | checking GitHub for a newer release, downloading and opening its package |
| `season` | summer, autumn and winter: terrain tint, snow, foliage colours, light |
| `key_bindings` | rebindable hotkeys: actions, their default and bindable keys, swapping on conflict |
| `engineers` | engineers capturing enemy structures and repairing own ones |
| `tutorial` | the step-by-step tutorial of levels with `"tutorial": true` |
| `sound` | sound cues from the simulation, playback with distance and fog, music director |
| `fog`, `fog_view` | fog of war: what the player knows, and how the terrain, water, minimap and enemies show it |
| `intel` | what each side knows: the Syndicate's shared view and the enemy structures each side has spotted |
| `outcome`, `match_stats` | mission objectives, victory and defeat; the tally shown at the end of a match |
| `replay` | recording matches, storing and listing replays, watching them |
| `battlefield` | scorch marks, vehicle wrecks and rain |
| `capture` | `IRON_TIDE_CAPTURE=shot.png` saves a screenshot and quits (for development; `_SAVE_SLOT` / `_LOAD_SLOT` test saves, `_REPLAY=1` watches the newest replay, `_SELECT_ALL=1` selects every own unit, `_ZOOM=150` sets the camera distance, `_MENU=keys` opens that menu page; `_MISSION` numbers past the campaign
continue with the custom levels) |

## Model contract (Blender → game)

1 Blender unit = 1 m, one cell = 2×2 m, the model's front faces Blender +X, the origin sits at the center of
its footprint on the ground. Rotating turrets are separate objects named `Turret` with their pivot at the origin.
Team variants are exported as `<id>_blue.glb` / `<id>_red.glb`.

## Third-party assets

- Terrain textures (`assets/textures/terrain/`): [Poly Haven](https://polyhaven.com), CC0 -
  leafy_grass, forrest_ground_01, rocks_ground_05, river_small_rocks, gravel_concrete_02.
- Font Rajdhani (`assets/fonts/`): Indian Type Foundry, SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).

(c) 2026 Iron Tide
