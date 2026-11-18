# Balance

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
| Normal | Normal | on | 24 : 30 (6) | 10.2 min | 31 : 23 |
| Hard | Normal | on | 27 : 12 (21) | 6.8 min | 12 : 27 |
| Normal | Hard | on | 9 : 30 (21) | 7.0 min | 16 : 23 |
| Easy | Normal | on | 6 : 43 (11) | 11.1 min | 31 : 18 |
| Normal | Normal | off | 27 : 24 (9) | 4.6 min | 27 : 24 |

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
think every 0.6 s and lost to Normal because of it; it now thinks as often as Normal and wins on 25 % more
income and a first wave of seven instead of six. The size of that first wave matters far more than income:
with eight, Hard won 41:5.
