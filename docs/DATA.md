# What the parser extracts

There are two layers:

1. **Raw layer** (`Replay::frames()`, `honreplay stream`, `honreplay entities`): every
   networked entity with every field, frame by frame (100 frames per second on Reborn).
   Nothing is filtered or guessed. If the game networked it, it is here.
2. **Analysis layer** (`honreplay::analyze`, `honreplay report`): match statistics
   computed from the raw layer. Each item below says whether it is **exact** (read
   from a field the server sends) or **derived** (computed by this crate).

## Raw layer: entity types in a typical match

Counts are entity creations in a 26-minute Mid Wars replay.

| type | count | what it is | useful fields |
|------|------:|------------|---------------|
| `Player` | 10 | one per client | name/clan string indices, account id, team, slot, role, gold, all stat totals (below), ping, disconnect time, cosmetics, last death time, hero entity index |
| `Hero` | 181 | heroes, illusions, re-creates | position (x, y, z), facing, health/mana and max, experience, death and respawn time, buybacks, buyback cost, animation, team, owner client |
| `Ability` | 1794 | hero and item abilities | owner, slot, level, charges, cooldown start and duration, channel start and duration, timers |
| `Item` | 431 | inventory items | owner, slot (48-53 inventory, 54-119 stash, 120+ innate), level, charges, cooldowns, purchaser, purchase time |
| `State` | 7953 | buffs and debuffs | owner, inflictor, level, creation time, lifetime, charges |
| `Gadget` | 1204 | wards, totems, tombstones, ability gadgets | position, team, owner client, spawn time, lifetime, health, death time |
| `Creep` / `Neutral` / `Pet` / `Critter` | 538 / 47 / 237 / ... | lane creeps, jungle and Kongor, summons and couriers | position, health, team, level |
| `Building` | 17 | towers, barracks, base, wells, shops | position, health, team, attack target, last damage time |
| `Projectile`, `Affector`, `LinearAffector` | 5239 / 1945 / ... | projectiles and area effects | position, owner, target |
| `Powerup` | 50 | runes | position, type |
| `GameInfo` | 1 | match state | phase, phase start/duration, match id, mode, options, match length, pause time, passive GPM, votes |
| `TeamInfo` | 4 | teams | tower counts, tower kills, base health, base building, picks/bans fields, team target |
| `Waypoint`, `Vanity`, `KillScreen`, `Shop_*`, `Entity_*` | | map pings, cosmetics, shop stock, spawners | |

Run `honreplay schema <replay>` to list every field of every type (about 1,200 fields).

### Player statistics fields (exact, updated live)

`m_aStatTotals[PLAYER_STAT_*]`: `HERO_KILLS`, `DEATHS`, `ASSISTS`, `CREEP_KILLS`,
`NEUTRAL_KILLS`, `DENIES`, `STARTING_GOLD`, `GOLD_SPENT`, `PLAYER_KILL_GOLD`,
`PLAYER_ASSIST_GOLD`, `BUILDING_GOLD`, `DEATH_GOLD`, `CREEP_GOLD`, `NEUTRAL_GOLD`,
`WARDS_PLACED`, `WARDS_DESTROYED`, `CAMP_STACKS`.

`m_aFloatStatTotals[PLAYER_STAT_*]`: `HERO_DAMAGE`, `BUILDING_DAMAGE`,
`MAGIC_DAMAGE_RECEIVED`, `PHYSICAL_DAMAGE_RECEIVED`, `PURE_DAMAGE_RECEIVED`.

Also `m_fGold`, `m_yKillStreak`, `m_eRole`, `m_uiLastDeathTime`, `m_uiTerminationTime`
(leave or disconnect), `m_unPing`, `m_uiHeroAvatarKey`, courier, ward, taunt and announcer
cosmetics.

## Analysis layer (`Report`)

Times are game time in milliseconds (0 = horn; negative = pre-game).

| section | content | source |
|---------|---------|--------|
| `summary` | match id, map, mode, options, server, version, date, winner, duration, pause time, passive GPM, fps, frames | exact (GameInfo + replayinfo) |
| `players[].totals` | K/D/A, last hits, neutral kills, denies, gold (current, spent, earned, by source, lost on death), hero and building damage, damage received by type, wards placed and destroyed, camps stacked, kill streak, experience, buybacks | exact |
| `players[].totals.level` | level from experience (standard 25-level table) | derived |
| `players[].totals.net_worth` | gold spent + unspent gold (HoN's "net worth") | exact; equals the official value |
| `players[].totals.gpm/xpm` | net worth and experience per minute of game time (the official definition) | derived |
| `players[].timeline` | every N seconds (default 10): gold, gold earned, XP, level, K/D/A, LH, DN, hero damage, damage received, HP/MP | exact samples |
| `players[].positions` | hero x/y every N seconds (default 1) plus alive flag | exact samples |
| `players[].items` | `purchased` / `acquired` / `moved` / `removed` with time, slot and slot kind | exact entity lifecycle; the purchase/acquire split uses the `GOLD_SPENT` delta |
| `players[].final_inventory` | items held at the end | exact |
| `players[].skill_build` | ability level-ups with time | exact |
| `players[].casts` | every ability and item use (new cooldown start) with cooldown length | exact; toggles and passives without cooldown are not counted |
| `players[].deaths` | time, killer, assisters, position, respawn time, gold lost | time/position exact; killer and assisters derived (see below) |
| `players[].laning` | lane (top, mid, bottom, jungle, roaming), confidence, partners, opponents, gold/XP at 10:00 and difference to the lane opponents, outcome | derived from positions 1:00-10:00 |
| `kills` | all hero kills with killer, assisters, position | derived |
| `wards` | sight and revelation wards: time, team, owner, position, removal time, destroyed or expired | exact (fate derived from lifetime) |
| `buildings` | towers, barracks, bases: destruction time and position | exact (health reaching 0) |
| `objectives` | Kongor kills | exact (health reaching 0) |
| `runes` | rune spawns and pick-ups or expiry | exact entity lifecycle |
| `couriers` | courier deaths | exact |
| `advantage` | Legion minus Hellbourne earned gold and XP over time | derived |
| `chat` | all and team chat with sender | exact |
| `teams` | kills, deaths, tower kills, towers remaining, gold, XP | exact and summed |

### How derived values are computed

* **Kill attribution.** In the frame where a player's `DEATHS` counter increases, the
  enemy whose `HERO_KILLS` counter increases is the killer, and enemies whose `ASSISTS`
  increase are assisters. Deaths to creeps, towers or Kongor have no killer.
* **Lanes.** Both base buildings define the map axes: mid is the base-to-base
  diagonal, top and bottom are the L-shaped edge paths. Hero positions sampled every
  second from 1:00 to 10:00 (excluding the fountain area) are assigned to the closest
  lane, or `jungle` when far from all three. The lane with the most samples wins; under
  40% means `roaming`. On Mid Wars every hero is `mid`.
* **Level.** `HoN` does not network hero level, so it is computed from experience.

## Verification against official statistics

Match 13436416 (Forests of Caldavar with Phoenix, 62:08, S2R2 v7) was compared with the
official per-player statistics shown on ward-up.com. These match exactly for all 10 players
and are locked in by `tests/replays.rs`:

kills, deaths, assists, creep kills, denies, net worth, hero damage, building damage,
sight wards placed, revelation wards placed (from ward lifecycles), team kill totals
(62-58), duration, winner, and the bombardier skill build (24 points, order and level
of every pick).

GPM and XPM match after rounding. Known discrepancy: for one player (No1end, Swiftblade)
the replay's hero experience reaches the level-25 cap at about 55:00. The official site shows
level 23 and XPM 467. The other nine players' levels and XPM agree. This is not resolved yet.

## Not available from replays

* **Picks and bans.** Recording starts after hero selection. The `TeamInfo` ban and pick
  fields are exported raw when present.
* **Per-hit damage logs (who damaged whom).** The server sends damage *totals* per player
  and health per unit, not individual hits. The raw packet queues contain per-client
  combat popups that may allow reconstructing this. They are preserved in
  `Frame::packets` and documented as future work.
* **APM.** Player orders are not part of the recorded stream.
