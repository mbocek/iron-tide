# Balance

```sh
make balance              # 12 AI matches per pairing, about a minute on 10 cores
make balance MATCHES=40   # tighter numbers
```

`src/bin/balance.rs` runs the headless simulation (`iron_tide::headless`) in parallel and prints a Markdown report:

- **Equal-cost duels:** $3000 of one unit type attack-moves into $3000 of another, on both sides of the river.
  The score is the row's remaining army value minus the column's (+100 = the row wins untouched).
- **AI against AI:** the computer plays the Coalition side too. With fog of war, Normal plays a mirror, then
  Hard and Easy challenge Normal and Brutal challenges Hard; a last mirror without fog shows what scouting changes. Sides alternate every
  match, so a favoured base site shows up in its own column.
- **Unit ledger:** how many of each unit the AIs built and lost, and the damage they dealt per credit spent.

Results after tuning (40 matches per pairing; the simulation is deterministic, so each pair of matches is played
on its own map variant, once from each corner):

| Duel ($3000 a side) | Rifleman | Rocketeer | Grenadier | Sniper | Scout Buggy | Raider Bike | Battle Tank | Stealth Tank | Bulwark | Howitzer | Gunship | Bomber | Flak Track | Flame Trooper | Missile Truck |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **Rifleman** | 0 | +83 | −85 | +42 | −79 | +76 | −50 | +63 | −16 | −91 | +80 | +48 | −16 | −10 | −90 |
| **Rocketeer** | −83 | 0 | −88 | −99 | −12 | +85 | +68 | +75 | +59 | −79 | +92 | +50 | +60 | −78 | −47 |
| **Grenadier** | +85 | +88 | −9 | −100 | −57 | +74 | −66 | −32 | −66 | −81 | −100 | −100 | −59 | +90 | −80 |
| **Sniper** | −42 | +99 | +100 | 0 | −98 | −95 | −97 | −96 | −97 | −98 | −100 | −100 | −96 | +23 | −97 |
| **Scout Buggy** | +79 | +12 | +57 | +98 | 0 | −10 | −49 | +32 | −58 | +26 | +83 | +16 | +80 | +89 | +32 |
| **Raider Bike** | −76 | −85 | −74 | +95 | +10 | 0 | −45 | +50 | −28 | +33 | +42 | −84 | +82 | −61 | +42 |
| **Battle Tank** | +50 | −68 | +66 | +97 | +49 | +45 | 0 | +64 | −12 | +62 | −100 | −100 | +87 | +79 | +68 |
| **Stealth Tank** | −63 | −75 | +32 | +96 | −32 | −50 | −64 | 0 | −67 | +48 | 0 | 0 | +75 | +21 | +41 |
| **Bulwark** | +16 | −59 | +66 | +97 | +57 | +28 | +12 | +67 | 0 | +70 | −100 | −100 | +86 | +81 | +70 |
| **Howitzer** | +91 | +79 | +81 | +98 | −26 | −33 | −62 | −48 | −70 | 0 | −100 | −100 | +11 | +88 | +14 |
| **Gunship** | −80 | −92 | +100 | +100 | −83 | −42 | +100 | 0 | +100 | +100 | 0 | +100 | −89 | +100 | +100 |
| **Bomber** | −48 | −50 | +100 | +100 | −16 | +84 | +100 | 0 | +100 | +100 | −100 | 0 | −50 | +100 | +100 |
| **Flak Track** | +16 | −60 | +59 | +96 | −80 | −82 | −87 | −75 | −86 | −11 | +89 | +50 | 0 | +80 | −45 |
| **Flame Trooper** | +10 | +78 | −90 | −23 | −90 | +61 | −79 | −21 | −81 | −88 | −100 | −100 | −80 | 0 | −93 |
| **Missile Truck** | +90 | +47 | +80 | +97 | −32 | −42 | −68 | −41 | −70 | −14 | −100 | −100 | +45 | +93 | 0 |

| Challenger | vs | fog | wins : losses (unfinished) | median length | SW : NE site wins |
| --- | --- | --- | --- | --- | --- |
| Normal | Normal | on | 17 : 22 (1) | 7.7 min | 22 : 17 |
| Hard | Normal | on | 31 : 5 (4) | 7.9 min | 18 : 18 |
| Normal | Hard | on | 2 : 32 (6) | 7.5 min | 18 : 16 |
| Easy | Normal | on | 2 : 37 (1) | 8.3 min | 22 : 17 |
| Brutal | Hard | on | 36 : 3 (1) | 9.1 min | 23 : 16 |
| Normal | Normal | off | 13 : 25 (2) | 4.8 min | 26 : 12 |

The challenger plays the Coalition and the defender the Syndicate. The defender wins the Normal mirror more
often, but it does the same with both sides on one faction or with the factions swapped, so that is the
computer playing the player's team (which a person does in a real battle), not the factions. The south-west
corner also wins more often (about 3:2 in 300-match runs, most clearly without fog). It is not the map: every
open cell has a twin with the same ground and crystal, and both corners play the same opening move for move
until the commanders' dice first differ (`tests/symmetry.rs`). Scouting ties, ring scans, stacked-unit pushes
and the minefield direction were mirrored without changing it, so the cause lies later in the mid-game. A commander that picks a
vehicle it cannot afford yet saves for it instead of choosing again, so the heavy units of the Tech Center
(Bulwark, Missile Truck, Howitzer, Gunship) now appear in a long match. Every unit has a counter: riflemen beat rocketeers, flame troopers burn any infantry but melt before
vehicles, scouts hunt infantry, rocketeers kill armour, tanks and the Bulwark beat light vehicles, and howitzers
and missile trucks shell infantry but lose to anything fast that reaches them. Unfinished matches are mostly
mutual exhaustion: both sides out of units, money and harvesters, with only turrets left standing. Hard used to
think every 0.6 s and lost to Normal because of it; it thinks as often as Normal and wins on 30 % more income and
a first wave of eight instead of six. Brutal, the level above it, has 60 % more income, a first wave of nine and
waves that grow by three units instead of two. The new Grenadier beats infantry and loses to every vehicle, the
Raider Bike hunts artillery and loses to infantry, and the Stealth Tank loses a stand-up fight to the Battle Tank:
it is meant to fire first from hiding, which equal-cost duels do not show. Units now also prefer targets that can
hurt them (a tank picks the rocketeer before the rifleman next to it), which made matches shorter and left fewer
unfinished.
