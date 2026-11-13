# honreplay

A fast, complete parser for **Heroes of Newerth** replays (`.honreplay`), written in Rust.

It decodes the whole binary stream: every entity of every frame, game events, chat and
string tables. It then turns that into OpenDota-style match data: scoreboards,
per-second timelines, item and skill builds, ability usage, kills, wards, lanes,
objectives and positions for heatmaps or a 2D replay viewer.

```
$ honreplay scoreboard 13337970.honreplay
cl  player              hero                 K   D   A    LH   DN    GPM    XPM HeroDmg  BldDmg  Lv Wards
4   ExDees              kinesis             16   8  33    33    1    512   1107   37786    1490  23     1
7   Redvel              swiftblade          18   9  32    59    3    685   1107   49850    5432  23     2
...
```

* **S2R2 v6 and v7.** The format version is read from each replay, so the decoder picks
  the right rules on its own. Verified so far: v6 on a 0.13.2.0 server (Forests of Caldavar)
  and v7 on a 0.13.3.0 server (Mid Wars); every frame of both decodes bit-exactly. Exactly
  which game patch moved from v6 to v7 is not established yet. More sample replays are
  welcome.
* **Fast:** a 26-minute match (165k frames, 4.7M entity updates) decodes in about 2 s.
* **Cross-platform:** Linux, macOS (Apple Silicon and Intel) and Windows. Pure Rust, no
  native dependencies.
* **Usable two ways:** as a CLI (tables, CSV, JSON or NDJSON) or as a library crate.

## Install

```sh
cargo install --git https://github.com/NasirA1991/hon-replay-parser
```

Or build from source with `cargo build --release`; the binary ends up in
`target/release/honreplay`. Prebuilt binaries are attached to GitHub releases.

## CLI

```
honreplay <command> <replay.honreplay> [options]
```

| command | output |
|---------|--------|
| `info` | match id, map, mode, version, date, duration, winner, players, heroes, account ids |
| `scoreboard` | end-of-game scoreboard (K/D/A, LH/DN, GPM/XPM, damage, level, wards) |
| `report` | **everything** as one JSON document (`--pretty`, `-o file`, `--sample <s>`, `--positions <s>`) |
| `timeline` | per-player stats every N seconds as CSV (`--player`, `--sample`) |
| `items` | purchases, combines, sells and stash moves with timings, plus final inventory |
| `skills` | skill build (ability level-ups with timings) |
| `casts` | every ability and item use, with cooldowns |
| `kills` | hero kills with killer, assisters and map position |
| `wards` | ward placements: type, team, owner, position, lifetime, destroyed or expired |
| `objectives` | towers and buildings destroyed, Kongor kills, runes, courier deaths |
| `chat` | all and team chat |
| `positions` | hero positions as CSV (`--interval <s>`), for heatmaps and replay viewers |
| `entities` | full state of every entity at a frame (`--frame N`, `--type Hero`) |
| `stream` | raw frame-by-frame entity changes as NDJSON (`--values`, `--events`, `--from/--to`) |
| `schema` | the entity schema declared in the replay (53 types, ~1,150 fields) |
| `strings` | header string tables (`--set 0..3`) |
| `verify` | decode every frame and report integrity and timing |

Every command accepts `--json`. Examples:

```sh
honreplay report match.honreplay --pretty -o match.json
honreplay timeline match.honreplay --player 7 --sample 30 > swiftblade.csv
honreplay positions match.honreplay --interval 1 > positions.csv
honreplay stream match.honreplay --values --from 4500 --to 4600 | jq .
honreplay entities match.honreplay --frame 60000 --type Hero --json
```

## Library

```rust
use honreplay::{Replay, Change, analysis};

let replay = Replay::open("match.honreplay")?;

// High level: a full match report.
let report = analysis::analyze(&replay, analysis::Options::default())?;
for p in &report.players {
    println!("{} {} {}/{}/{}", p.name, p.hero, p.totals.kills, p.totals.deaths, p.totals.assists);
}

// Low level: walk the entity stream yourself.
let mut frames = replay.frames();
while let Some(frame) = frames.next_frame() {
    let frame = frame?;
    for change in &frame.changes {
        if let Change::Update { id, fields } = change {
            let entity = frames.pool().get(*id).unwrap();
            let typedesc = &replay.header.typedescs[entity.typedesc];
            // typedesc.fields[i].name / entity.fields[i] ...
        }
    }
}
# Ok::<(), honreplay::Error>(())
```

## What you get

See **[docs/DATA.md](docs/DATA.md)** for the full list and for how each value is
obtained (exact field from the server, or derived). In short:

* **Match:** id, map, mode, options, server, version, date, winner, duration, pause time.
* **Players:** name, clan, account id, team, slot, role, hero, alt-avatar, disconnects.
* **Totals:** kills, deaths, assists, last hits, neutral kills, denies, gold (current,
  earned, spent, by source, lost on death), GPM/XPM, hero and building damage, damage
  received (magic, physical, pure), wards placed and destroyed, camps stacked, kill
  streak, experience, level, buybacks.
* **Timelines:** all of the above every N seconds, plus HP and MP, and team gold/XP
  advantage.
* **Builds:** item purchases, combines, sells, stash and inventory moves; final
  inventory; skill order.
* **Actions:** every ability and item use with cooldown.
* **Fights:** kills with killer, assisters and position; deaths with respawn time and
  gold lost.
* **Map:** hero positions (per second, or per frame from the raw stream), lanes,
  lane partners and opponents, lane outcome at 10:00, wards with coordinates, tower and
  building kills, Kongor kills, runes, courier deaths.
* **Chat:** all and team, with sender.
* **Raw:** every networked entity (heroes, creeps, neutrals, projectiles, buffs,
  gadgets, buildings ...) with all fields, frame by frame, plus game events (effects
  with source and target positions) and per-client packet queues.

## How it works

**[docs/FORMAT.md](docs/FORMAT.md)** is a complete specification of the S2R2 format:
header, entity schemas, field encodings, the compressed field mask, entity records and
their v6/v7 differences, game events, packets and the game clock.

## Limitations

* Hero picks and bans are not recorded (recording starts after the draft).
* The server sends damage totals, not individual hits, so a "who damaged whom" matrix
  is not available yet (see DATA.md).
* Replays older than S2R2 v6 (original HoN, pre-Reborn) should work; v3-v5 headers are
  supported. They are untested because no sample is available.

## Tests

```sh
scripts/fetch-fixtures.sh     # downloads the two public test replays from the HoN CDN
cargo test --release
```

## Credits

The basic S2R2 header and bit layout were first documented by theli-ua's
[s2r2](https://github.com/theli-ua/s2r2) (Python, 2010). This project completes the format
for HoN Reborn (game events, variant bits, the v7 changes, time model) and adds the
analysis layer. Heroes of Newerth and its assets belong to their respective owners. This
is an unofficial community project.

## License

MIT, see [LICENSE](LICENSE).
