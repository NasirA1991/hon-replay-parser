# honreplay

A fast, complete parser for **Heroes of Newerth** replays (`.honreplay`), written in Rust.

It decodes the whole binary stream: every entity of every frame, game events, chat,
string tables and the per-client message queues. It then turns that into OpenDota-style
match data: scoreboards, timelines, item and skill builds, ability usage, kills, a
who-damaged-whom matrix, wards, lanes, objectives and positions for heatmaps or a 2D
replay viewer.

```
$ honreplay damage 13436416.honreplay
attacker           victim                exact  matched    alloc     total  hits
No1end (swiftblade Mobutul (magebane)     1828      436      197      2461    19
No1end (swiftblade 77777777 (pyromanc     5468     1895      635      7998    25
No1end (swiftblade kobla007 (witch_sl     5087      296      324      5707    26
...

$ honreplay scoreboard 13436416.honreplay
cl  player              hero                 K   D   A   LH   NK  DN      NW   GPM   XPM HeroDmg BldDmg  Lv  WS
3   Mobutul             magebane            11   3  15  373  194   5   46749   752   521   53422   1659  25   0
4   77777777            pyromancer          11  18  14  217   52   0   28633   461   521   32246   3500  25   0
...
```

Most values are read straight from counters the server sends and are **exact**. A few
are **derived** by this crate (kill attribution, lanes, part of the damage matrix), and
the output and [docs/DATA.md](docs/DATA.md) say which is which.

* **S2R2 v6 and v7.** The format version is read from each replay, so the decoder picks
  the right rules on its own. Verified so far: v7 on a 0.13.3.0 server (a 62-minute
  Forests of Caldavar match and a Mid Wars match) and v6 on a 0.13.2.0 server (Forests of
  Caldavar); every frame decodes to its last bit. The 62-minute match is also checked
  against the server's own end-of-match statistics, which the replay contains. Exactly
  which game patch moved from v6 to v7 is not established yet. More sample replays are
  welcome.
* **Fast:** a 62-minute match (384k frames, 25M entity updates) decodes in about 2.5 s
  and is fully analysed in about 4 s.
* **Cross-platform:** Linux, macOS (Apple Silicon and Intel) and Windows. Nothing to
  install besides a Rust toolchain and a C compiler (Zstandard is built in).
* **Usable two ways:** as a CLI (tables, CSV, JSON or NDJSON) or as a library crate.

## Install

```sh
cargo install --git https://github.com/NasirA1991/hon-replay-parser
```

Or build from source with `cargo build --release`; the binary ends up in
`target/release/honreplay`.

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
| `damage` | who damaged whom: hero damage per attacker and victim, and how it was attributed |
| `stats` | the server's own end-of-match statistics document embedded in the replay |
| `wards` | ward placements: type, team, owner, position, lifetime, destroyed or expired |
| `objectives` | towers and buildings destroyed, Kongor kills, runes, courier deaths |
| `chat` | all and team chat |
| `positions` | hero positions as CSV (`--interval <s>`), for heatmaps and replay viewers |
| `entities` | full state of every entity at a frame (`--frame N`, `--type Hero`) |
| `stream` | raw frame-by-frame entity changes as NDJSON (`--values`, `--events`, `--from/--to`) |
| `schema` | the entity schema declared in the replay (53 types, about 1,160 fields) |
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
* **Damage:** every hit a player lands on an enemy hero (time, amount, physical or
  magic), and a who-damaged-whom matrix whose rows add up to the server's hero damage
  counter. Damage to illusions, creeps, neutrals and buildings per player.
* **Draft:** the hero each player picked.
* **Server statistics:** the end-of-match statistics document the server embeds in the
  replay, verbatim.
* **Map:** hero positions (per second, or per frame from the raw stream), lanes,
  lane partners and opponents, lane outcome at 10:00, wards with coordinates, tower and
  building kills, Kongor kills, runes, courier deaths.
* **Chat:** all and team, with sender.
* **Raw:** every networked entity (heroes, creeps, neutrals, projectiles, buffs,
  gadgets, buildings ...) with all fields, frame by frame, plus game events (effects
  with source and target positions) and per-client packet queues.

Units: times are milliseconds of game time (0 = the horn, negative = pre-game),
positions are world units, client numbers are 1-based as in the entity stream.

## How it works

**[docs/FORMAT.md](docs/FORMAT.md)** is a complete specification of the S2R2 format:
header, entity schemas, field encodings, the compressed field mask, entity records and
their v6/v7 differences, game events, packets and the game clock.

## Limitations

* Bans are not recorded (recording starts after the draft); each player's pick is.
* APM is not available: the server's action counter is never sent to clients.
* The who-damaged-whom matrix is exact for hits (about 80% of hero damage) and
  estimated for part of the damage over time; every cell says how much is which.
* The damage matrix has been validated on one full match. Heroes with unusual
  mechanics (reflected damage, summons) may be attributed less precisely.
* Replays older than S2R2 v6 (original HoN, before Reborn) are untested: v3-v5 headers
  are supported, but no sample is available.

## Tests

```sh
scripts/fetch-fixtures.sh     # downloads the three test replays into tests/fixtures
cargo test --release
```

The integration tests need real replays (matches 13337970, 12857222 and 13436416). They
are not stored in the repository, and replays expire from the HoN replay CDN, so copy
them into `tests/fixtures` by hand if the download fails. **A test whose replay is
missing is skipped, not failed**, and CI does not download them: a green run without
fixtures only covers the unit tests. Set `HONREPLAY_FIXTURES` to use another directory.

## Credits

The basic S2R2 header and bit layout were first documented by theli-ua's
[s2r2](https://github.com/theli-ua/s2r2) (Python, 2010). This project completes the format
for HoN Reborn (game events, variant bits, the v7 changes, time model) and adds the
analysis layer. Heroes of Newerth and its assets belong to their respective owners. This
is an unofficial community project.

## License

MIT, see [LICENSE](LICENSE).
