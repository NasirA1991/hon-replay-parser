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

Run `honreplay schema <replay>` to list every field of every type (about 1,160 fields).

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
| `damage.hits` | every damage popup of a player against an enemy hero or illusion: time, attacker, victim, amount, physical/magic | exact (per-hit server messages) |
| `damage.matrix` | hero damage per attacker and victim, split into `exact`, `matched` and `allocated` | exact + derived (see below) |
| `damage.players` | per player: hero damage counter, attributed and unattributed share, popup damage to illusions, creeps, neutrals and buildings | exact |
| `draft.picks` | the hero each player selected (`Player.m_unSelectedHeroID`) | exact |
| `draft.bans` | `TeamInfo.m_vHiddenHeroBans*` when set | exact; empty in every replay seen so far |
| `server_stats` | the server's end-of-match statistics JSON, verbatim | exact (embedded document) |
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
* **Level.** Computed from experience. `Hero.m_uiLevel` exists in the schema but stays 0
  for every hero, so it cannot be used. The computed level equals `HeroLevel` in the
  embedded server statistics for all ten players of the verified match.
* **Who damaged whom.** Three sources are combined, most reliable first:
  1. *Exact.* The server sends each player a popup message for every hit it deals
     (`0x3f`, kinds `0x10` physical and `0x11` magic: target entity and damage after
     mitigation). The replay records all ten client queues, so every such hit is there
     once. In the verified match this covers 79% of all hero damage, and a single popup
     equals the attacker's `HERO_DAMAGE` increase of that frame to the decimal.
  2. *Matched.* Damage over time and aura ticks have no popup. They show up as an
     increase of the attacker's `HERO_DAMAGE` counter. When only one enemy hero received
     damage in that frame, the pairing is unambiguous (12.5%).
  3. *Allocated.* When several enemy heroes received damage in that frame, the rest is
     split in proportion to what each of them received (8.7%). This is an estimate.

  Each matrix row adds up to the player's `HERO_DAMAGE` counter. It can exceed it by a
  small amount (under 2% for one player in the verified match) when a popup reports a
  hit the counter did not include. Damage to illusions is listed separately because the
  server does not count it as hero damage.

## Verification against official statistics

Match 13436416 (Forests of Caldavar with Phoenix, 62:08, S2R2 v7) was compared with the
official per-player statistics shown on ward-up.com. These match exactly for all 10 players
and are locked in by `tests/replays.rs`:

kills, deaths, assists, creep kills, denies, net worth, hero damage, building damage,
sight wards placed, revelation wards placed (from ward lifecycles), team kill totals
(62-58), duration, winner, and the bombardier skill build (24 points, order and level
of every pick).

GPM and XPM match after rounding. For one player (No1end, Swiftblade) ward-up.com shows
level 23 and XPM 467 while the replay gives level 25 and XPM 521. The replay is right:
the server statistics embedded in the same replay say `HeroLevel` 25 and `HeroExp` 32400,
and HoNBuff shows level 25 as well.

### Embedded server statistics

At game over the server broadcasts its statistics as zlib-compressed JSON (see
FORMAT.md, section 6). `report.server_stats` holds it verbatim: per player `HeroLevel`,
`HeroKills`, `HeroDeaths`, `HeroAssists`, `Networth`, `Gpm`, `Xpm`, `HeroDamage`,
`BuildingDamage`, `CreepsKilled`, `Denies`, `Role`, `Items`, ward and consumable counts.
`tests/replays.rs` checks it against the entity stream. Two definitions differ from the
values above and from what stats sites show:

* `Gpm` excludes the 600 starting gold; this crate and the sites use net worth per minute.
* `HeroDamage` is 2-12% higher than `PLAYER_STAT_HERO_DAMAGE` (a different server
  counter; what it adds is not known). Sites show `PLAYER_STAT_HERO_DAMAGE`.

## Not available from replays

* **Bans.** Recording starts one tick after the pre-game phase begins, after hero
  selection. The `TeamInfo` ban fields (`m_vHiddenHeroBans0-19`, `m_uiBanCount`) are at
  their defaults in every replay checked, including a ranked 10-player match, and the
  embedded server statistics have no ban field. `draft.bans` is filled if a replay ever
  carries them.
* **APM.** The server counts actions in `GameStats.m_uiActionCount`. The type is declared
  in the schema and `Player.m_uiStatsIndex` points at such an entity, but that entity is
  never sent, so the count is not in the replay. `PlayerView` (cursor, camera) and
  `Order` are declared and never sent either, and the per-frame command section is empty.
  Casts, item uses, purchases and skill-ups are available and could be counted, but that
  number is not APM.
