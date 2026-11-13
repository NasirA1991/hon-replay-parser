//! `honreplay` command line interface.
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use honreplay::analysis::{self, Options, Report};
use honreplay::{Change, Replay};
use serde_json::{Value as J, json};

#[derive(Parser)]
#[command(
    name = "honreplay",
    version,
    about = "Parse Heroes of Newerth replays (.honreplay)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct Common {
    /// Replay file (.honreplay)
    replay: PathBuf,
    /// Print JSON instead of a table
    #[arg(long)]
    json: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Match summary: map, mode, duration, winner, players and heroes
    Info(Common),
    /// Full analysis report as JSON (everything below in one document)
    Report {
        replay: PathBuf,
        /// Timeline sampling interval in seconds
        #[arg(long, default_value_t = 10)]
        sample: i64,
        /// Hero position sampling interval in seconds (0 disables positions)
        #[arg(long, default_value_t = 1)]
        positions: i64,
        /// Pretty-print
        #[arg(long)]
        pretty: bool,
        /// Write to a file instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// End-of-game scoreboard
    Scoreboard(Common),
    /// Per-player stat timeline (CSV unless --json)
    Timeline {
        #[command(flatten)]
        c: Common,
        /// Client number (1-10); all players when omitted
        #[arg(long)]
        player: Option<u32>,
        #[arg(long, default_value_t = 60)]
        sample: i64,
    },
    /// Item purchases / sells / moves and final inventories
    Items {
        #[command(flatten)]
        c: Common,
        #[arg(long)]
        player: Option<u32>,
    },
    /// Ability skill order
    Skills {
        #[command(flatten)]
        c: Common,
        #[arg(long)]
        player: Option<u32>,
    },
    /// Ability and item uses (from cooldown starts)
    Casts {
        #[command(flatten)]
        c: Common,
        #[arg(long)]
        player: Option<u32>,
    },
    /// Hero kills with killer, assisters and position
    Kills(Common),
    /// Ward placements with position, owner and lifetime
    Wards(Common),
    /// Buildings destroyed, Kongor kills, runes, couriers
    Objectives(Common),
    /// All and team chat
    Chat(Common),
    /// Hero positions (CSV unless --json)
    Positions {
        #[command(flatten)]
        c: Common,
        #[arg(long)]
        player: Option<u32>,
        #[arg(long, default_value_t = 1)]
        interval: i64,
    },
    /// Entity state at a frame (or at the end)
    Entities {
        #[command(flatten)]
        c: Common,
        /// Frame index (default: last frame)
        #[arg(long)]
        frame: Option<u64>,
        /// Only entities whose base type (e.g. Hero, Item, Player) matches
        #[arg(long = "type")]
        kind: Option<String>,
    },
    /// Raw entity change stream as NDJSON (one line per frame)
    Stream {
        replay: PathBuf,
        #[arg(long)]
        from: Option<u64>,
        #[arg(long)]
        to: Option<u64>,
        /// Include full field values of changed fields
        #[arg(long)]
        values: bool,
        /// Include game events and chat packets
        #[arg(long)]
        events: bool,
    },
    /// Entity schema (typedescs) declared in the replay header
    Schema(Common),
    /// String tables from the header (0 server, 1 cvars, 2 dynamic, 3 resources)
    Strings {
        #[command(flatten)]
        c: Common,
        #[arg(long, default_value_t = 3)]
        set: usize,
    },
    /// Decode every frame and report timing and integrity
    Verify(Common),
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

type R = Result<(), Box<dyn std::error::Error>>;

fn ms(t: i64) -> String {
    let s = t.abs() / 1000;
    format!("{}{}:{:02}", if t < 0 { "-" } else { "" }, s / 60, s % 60)
}

fn report(
    path: &PathBuf,
    sample: i64,
    positions: i64,
) -> Result<Report, Box<dyn std::error::Error>> {
    let replay = Replay::open(path)?;
    Ok(analysis::analyze(
        &replay,
        Options {
            sample_ms: sample.max(1) * 1000,
            position_ms: positions.max(1) * 1000,
            include_positions: positions > 0,
            include_events: false,
        },
    )?)
}

fn out() -> BufWriter<std::io::StdoutLock<'static>> {
    BufWriter::new(std::io::stdout().lock())
}

fn pname(r: &Report, c: u32) -> String {
    r.players
        .iter()
        .find(|p| p.client == c)
        .map(|p| format!("{} ({})", p.name, p.hero))
        .unwrap_or_else(|| format!("client {c}"))
}

fn run(cli: Cli) -> R {
    let mut w = out();
    match cli.cmd {
        Cmd::Info(c) => {
            let r = report(&c.replay, 60, 0)?;
            if c.json {
                serde_json::to_writer_pretty(
                    &mut w,
                    &json!({"summary": r.summary, "players": r.players.iter().map(|p| json!({"client": p.client, "name": p.name, "account_id": p.account_id, "team": p.team, "hero": p.hero, "role": p.role})).collect::<Vec<_>>()}),
                )?;
                writeln!(w)?;
                return Ok(());
            }
            let s = &r.summary;
            writeln!(
                w,
                "Match {}  {} ({})  mode {}  S2R2 v{}  server {}",
                s.match_id,
                s.map_fancy_name,
                s.map,
                s.game_mode,
                s.format_version,
                s.server_version
            )?;
            writeln!(
                w,
                "Date {} {}  duration {}  winner {}  frames {} @ {} fps",
                s.date,
                s.time,
                ms(s.duration_ms),
                if s.winner.is_empty() { "?" } else { &s.winner },
                s.frames,
                s.server_fps
            )?;
            for team in ["legion", "hellbourne"] {
                writeln!(w, "\n{}", team.to_uppercase())?;
                for p in r.players.iter().filter(|p| p.team == team) {
                    writeln!(
                        w,
                        "  {:>2} {:<22} {:<20} acc {}",
                        p.client, p.name, p.hero, p.account_id
                    )?;
                }
            }
        }
        Cmd::Report {
            replay,
            sample,
            positions,
            pretty,
            output,
        } => {
            let r = report(&replay, sample, positions)?;
            let s = if pretty {
                serde_json::to_string_pretty(&r)?
            } else {
                serde_json::to_string(&r)?
            };
            match output {
                Some(p) => std::fs::write(p, s)?,
                None => writeln!(w, "{s}")?,
            }
        }
        Cmd::Scoreboard(c) => {
            let r = report(&c.replay, 60, 0)?;
            if c.json {
                serde_json::to_writer_pretty(&mut w, &r.players.iter().map(|p| json!({"client": p.client, "name": p.name, "team": p.team, "hero": p.hero, "totals": p.totals})).collect::<Vec<_>>())?;
                writeln!(w)?;
                return Ok(());
            }
            writeln!(
                w,
                "{:<4}{:<20}{:<18}{:>4}{:>4}{:>4}{:>6}{:>5}{:>7}{:>7}{:>8}{:>8}{:>4}{:>6}",
                "cl",
                "player",
                "hero",
                "K",
                "D",
                "A",
                "LH",
                "DN",
                "GPM",
                "XPM",
                "HeroDmg",
                "BldDmg",
                "Lv",
                "Wards"
            )?;
            for team in ["legion", "hellbourne"] {
                for p in r.players.iter().filter(|p| p.team == team) {
                    let t = &p.totals;
                    writeln!(
                        w,
                        "{:<4}{:<20}{:<18}{:>4}{:>4}{:>4}{:>6}{:>5}{:>7.0}{:>7.0}{:>8.0}{:>8.0}{:>4}{:>6}",
                        p.client,
                        trunc(&p.name, 19),
                        trunc(&p.hero, 17),
                        t.kills,
                        t.deaths,
                        t.assists,
                        t.last_hits + t.neutral_kills,
                        t.denies,
                        t.gpm,
                        t.xpm,
                        t.hero_damage,
                        t.building_damage,
                        t.level,
                        t.wards_placed
                    )?;
                }
                writeln!(w)?;
            }
        }
        Cmd::Timeline { c, player, sample } => {
            let r = report(&c.replay, sample, 0)?;
            let ps: Vec<_> = r
                .players
                .iter()
                .filter(|p| player.is_none_or(|x| x == p.client))
                .collect();
            if c.json {
                serde_json::to_writer(
                    &mut w,
                    &ps.iter()
                        .map(
                            |p| json!({"client": p.client, "name": p.name, "timeline": p.timeline}),
                        )
                        .collect::<Vec<_>>(),
                )?;
                writeln!(w)?;
                return Ok(());
            }
            writeln!(
                w,
                "client,name,t_ms,gold,gold_earned,experience,level,kills,deaths,assists,last_hits,denies,hero_damage,damage_received,health,max_health,mana,max_mana"
            )?;
            for p in ps {
                for s in &p.timeline {
                    writeln!(
                        w,
                        "{},{},{},{:.0},{},{},{},{},{},{},{},{},{:.0},{:.0},{},{:.0},{},{:.0}",
                        p.client,
                        p.name.replace(',', " "),
                        s.t,
                        s.gold,
                        s.gold_earned,
                        s.experience,
                        s.level,
                        s.kills,
                        s.deaths,
                        s.assists,
                        s.last_hits,
                        s.denies,
                        s.hero_damage,
                        s.damage_received,
                        s.health,
                        s.max_health,
                        s.mana,
                        s.max_mana
                    )?;
                }
            }
        }
        Cmd::Items { c, player } => {
            let r = report(&c.replay, 60, 0)?;
            for p in r
                .players
                .iter()
                .filter(|p| player.is_none_or(|x| x == p.client))
            {
                if c.json {
                    serde_json::to_writer(
                        &mut w,
                        &json!({"client": p.client, "name": p.name, "items": p.items, "final_inventory": p.final_inventory}),
                    )?;
                    writeln!(w)?;
                    continue;
                }
                writeln!(w, "== {} {} ({})", p.client, p.name, p.hero)?;
                for i in p.items.iter().filter(|i| i.action != "moved") {
                    writeln!(
                        w,
                        "  {:>7} {:<9} {:<28} {} {}",
                        ms(i.t),
                        i.action,
                        i.item,
                        i.slot_kind,
                        i.slot.unwrap_or(-1)
                    )?;
                }
                writeln!(
                    w,
                    "  final: {}",
                    p.final_inventory
                        .iter()
                        .filter(|s| s.slot_kind == "inventory")
                        .map(|s| s.item.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                )?;
            }
        }
        Cmd::Skills { c, player } => {
            let r = report(&c.replay, 60, 0)?;
            for p in r
                .players
                .iter()
                .filter(|p| player.is_none_or(|x| x == p.client))
            {
                if c.json {
                    serde_json::to_writer(
                        &mut w,
                        &json!({"client": p.client, "name": p.name, "skill_build": p.skill_build}),
                    )?;
                    writeln!(w)?;
                    continue;
                }
                writeln!(w, "== {} {} ({})", p.client, p.name, p.hero)?;
                for s in &p.skill_build {
                    writeln!(w, "  {:>7} {:<36} -> {}", ms(s.t), s.ability, s.level)?;
                }
            }
        }
        Cmd::Casts { c, player } => {
            let r = report(&c.replay, 60, 0)?;
            for p in r
                .players
                .iter()
                .filter(|p| player.is_none_or(|x| x == p.client))
            {
                if c.json {
                    serde_json::to_writer(
                        &mut w,
                        &json!({"client": p.client, "name": p.name, "casts": p.casts}),
                    )?;
                    writeln!(w)?;
                    continue;
                }
                writeln!(
                    w,
                    "== {} {} ({}) {} casts",
                    p.client,
                    p.name,
                    p.hero,
                    p.casts.len()
                )?;
                for s in &p.casts {
                    writeln!(
                        w,
                        "  {:>7} {:<7} {:<36} cd {}s",
                        ms(s.t),
                        s.kind,
                        s.ability,
                        s.cooldown_ms / 1000
                    )?;
                }
            }
        }
        Cmd::Kills(c) => {
            let r = report(&c.replay, 60, 0)?;
            if c.json {
                serde_json::to_writer_pretty(&mut w, &r.kills)?;
                writeln!(w)?;
                return Ok(());
            }
            for k in &r.kills {
                writeln!(
                    w,
                    "{:>7}  {:<32} killed {:<32} assists: {}  at ({}, {})",
                    ms(k.t),
                    k.killer.map(|c| pname(&r, c)).unwrap_or("-".into()),
                    pname(&r, k.victim),
                    k.assisters
                        .iter()
                        .map(|a| pname(&r, *a))
                        .collect::<Vec<_>>()
                        .join(", "),
                    k.x,
                    k.y
                )?;
            }
        }
        Cmd::Wards(c) => {
            let r = report(&c.replay, 60, 0)?;
            if c.json {
                serde_json::to_writer_pretty(&mut w, &r.wards)?;
                writeln!(w)?;
                return Ok(());
            }
            for x in &r.wards {
                writeln!(
                    w,
                    "{:>7} {:<11} {:<11} {:<28} ({:>5},{:>5}) {:<10} {}",
                    ms(x.placed_t),
                    x.kind,
                    x.team,
                    x.owner.map(|c| pname(&r, c)).unwrap_or("-".into()),
                    x.x,
                    x.y,
                    x.fate,
                    x.removed_t.map(ms).unwrap_or_default()
                )?;
            }
        }
        Cmd::Objectives(c) => {
            let r = report(&c.replay, 60, 0)?;
            if c.json {
                serde_json::to_writer_pretty(
                    &mut w,
                    &json!({"buildings": r.buildings, "objectives": r.objectives, "runes": r.runes, "couriers": r.couriers, "teams": r.teams}),
                )?;
                writeln!(w)?;
                return Ok(());
            }
            for (label, list) in [
                ("buildings", &r.buildings),
                ("objectives", &r.objectives),
                ("couriers", &r.couriers),
                ("runes", &r.runes),
            ] {
                writeln!(w, "== {label}")?;
                for e in list.iter() {
                    writeln!(
                        w,
                        "  {:>7} {:<12} {:<34} {:<16} ({}, {})",
                        ms(e.t),
                        e.team,
                        e.name,
                        e.event,
                        e.x,
                        e.y
                    )?;
                }
            }
        }
        Cmd::Chat(c) => {
            let r = report(&c.replay, 60, 0)?;
            if c.json {
                serde_json::to_writer_pretty(&mut w, &r.chat)?;
                writeln!(w)?;
                return Ok(());
            }
            for m in &r.chat {
                writeln!(
                    w,
                    "{:>7} [{}] {}: {}",
                    ms(m.t),
                    m.channel.to_uppercase(),
                    m.name,
                    m.message
                )?;
            }
        }
        Cmd::Positions {
            c,
            player,
            interval,
        } => {
            let r = report(&c.replay, 60, interval)?;
            let ps: Vec<_> = r
                .players
                .iter()
                .filter(|p| player.is_none_or(|x| x == p.client))
                .collect();
            if c.json {
                serde_json::to_writer(&mut w, &ps.iter().map(|p| json!({"client": p.client, "hero": p.hero, "positions": p.positions})).collect::<Vec<_>>())?;
                writeln!(w)?;
                return Ok(());
            }
            writeln!(w, "client,hero,t_ms,x,y,alive")?;
            for p in ps {
                for s in &p.positions {
                    writeln!(
                        w,
                        "{},{},{},{},{},{}",
                        p.client, p.hero, s.t, s.x, s.y, s.alive
                    )?;
                }
            }
        }
        Cmd::Entities { c, frame, kind } => {
            let replay = Replay::open(&c.replay)?;
            let mut fr = replay.frames();
            while fr.frame_index() <= frame.unwrap_or(u64::MAX) {
                match fr.next_frame() {
                    Some(f) => {
                        f?;
                    }
                    None => break,
                }
            }
            let h = &replay.header;
            let mut list = Vec::new();
            for e in fr.pool().iter() {
                let td = &h.typedescs[e.typedesc];
                if let Some(k) = &kind {
                    if !td.name.eq_ignore_ascii_case(k) {
                        continue;
                    }
                }
                let fields: serde_json::Map<String, J> = td
                    .fields
                    .iter()
                    .zip(e.fields.iter())
                    .map(|(f, v)| (f.name.clone(), serde_json::to_value(v).unwrap()))
                    .collect();
                list.push(json!({"id": e.id, "type": td.name, "type_id": e.type_id, "resource": h.resource_path(e.type_id), "variant": e.variant, "active": e.active, "created_frame": e.created_frame, "fields": fields}));
            }
            if c.json {
                serde_json::to_writer_pretty(&mut w, &list)?;
                writeln!(w)?;
                return Ok(());
            }
            for e in &list {
                writeln!(
                    w,
                    "{:>6} {:<16} {}",
                    e["id"],
                    e["type"].as_str().unwrap_or(""),
                    e["resource"].as_str().unwrap_or("")
                )?;
            }
            writeln!(
                w,
                "{} entities at frame {}",
                list.len(),
                fr.frame_index().saturating_sub(1)
            )?;
        }
        Cmd::Stream {
            replay,
            from,
            to,
            values,
            events,
        } => {
            let rp = Replay::open(&replay)?;
            let h = &rp.header;
            let mut fr = rp.frames();
            while let Some(f) = fr.next_frame() {
                let f = f?;
                if from.is_some_and(|x| f.index < x) {
                    continue;
                }
                if to.is_some_and(|x| f.index > x) {
                    break;
                }
                let pool = fr.pool();
                let changes: Vec<J> = f.changes.iter().map(|ch| match ch {
                    Change::Create { id, type_id, recreate } => {
                        let e = pool.get(*id).unwrap(); let td = &h.typedescs[e.typedesc];
                        let mut o = json!({"op": "create", "id": id, "type": td.name, "type_id": type_id, "resource": h.resource_path(*type_id), "recreate": recreate, "variant": e.variant});
                        if values { o["fields"] = J::Object(td.fields.iter().zip(e.fields.iter()).map(|(f, v)| (f.name.clone(), serde_json::to_value(v).unwrap())).collect()); }
                        o
                    }
                    Change::Update { id, fields } => {
                        let e = pool.get(*id).unwrap(); let td = &h.typedescs[e.typedesc];
                        let mut o = json!({"op": "update", "id": id, "type": td.name});
                        if values { o["fields"] = J::Object(fields.iter().map(|i| (td.fields[*i as usize].name.clone(), serde_json::to_value(&e.fields[*i as usize]).unwrap())).collect()); }
                        else { o["fields"] = json!(fields.iter().map(|i| td.fields[*i as usize].name.clone()).collect::<Vec<_>>()); }
                        o
                    }
                    Change::Delete { id, type_id } => json!({"op": "delete", "id": id, "type_id": type_id}),
                }).collect();
                let mut line = json!({"frame": f.index, "clock": f.clock, "changes": changes});
                if events {
                    line["events"] = json!(
                        f.events
                            .iter()
                            .map(|e| {
                                let mut v = serde_json::to_value(e).unwrap();
                                if let Some(fx) = e.effect {
                                    v["effect_name"] = json!(h.string(3, fx as u32));
                                }
                                v
                            })
                            .collect::<Vec<_>>()
                    );
                    line["packets"] = json!(f.packets);
                }
                serde_json::to_writer(&mut w, &line)?;
                writeln!(w)?;
            }
        }
        Cmd::Schema(c) => {
            let replay = Replay::open(&c.replay)?;
            if c.json {
                serde_json::to_writer_pretty(&mut w, &replay.header.typedescs.iter().map(|t| json!({"table": t.table, "type_id": t.type_id, "name": t.name, "fields": t.fields})).collect::<Vec<_>>())?;
                writeln!(w)?;
                return Ok(());
            }
            for t in &replay.header.typedescs {
                writeln!(
                    w,
                    "[{}:{}] {} ({} fields)",
                    t.table,
                    t.type_id,
                    t.name,
                    t.fields.len()
                )?;
                for (i, f) in t.fields.iter().enumerate() {
                    writeln!(
                        w,
                        "  {:>3} {:<44} type {:>2} bits {}",
                        i, f.name, f.type_code, f.bits
                    )?;
                }
            }
        }
        Cmd::Strings { c, set } => {
            let replay = Replay::open(&c.replay)?;
            let Some(ss) = replay.header.string_sets.get(set) else {
                return Err("no such string set".into());
            };
            if c.json {
                serde_json::to_writer_pretty(&mut w, ss)?;
                writeln!(w)?;
                return Ok(());
            }
            for (k, v) in ss {
                writeln!(w, "{k}\t{v}")?;
            }
        }
        Cmd::Verify(c) => {
            let t0 = std::time::Instant::now();
            let replay = Replay::open(&c.replay)?;
            let t1 = t0.elapsed();
            let mut fr = replay.frames();
            let (mut n, mut ch, mut ev, mut err) = (0u64, 0u64, 0u64, None);
            while let Some(f) = fr.next_frame() {
                match f {
                    Ok(f) => {
                        n += 1;
                        ch += f.changes.len() as u64;
                        ev += f.events.len() as u64;
                    }
                    Err(e) => {
                        err = Some(e.to_string());
                        break;
                    }
                }
            }
            let res = json!({"ok": err.is_none(), "error": err, "format_version": replay.header.format_version, "server_version": replay.header.server_version, "frames": n, "entity_changes": ch, "game_events": ev, "entities_at_end": fr.pool().len(), "load_ms": t1.as_millis(), "decode_ms": (t0.elapsed() - t1).as_millis()});
            if c.json {
                serde_json::to_writer_pretty(&mut w, &res)?;
                writeln!(w)?;
            } else {
                writeln!(w, "{}", serde_json::to_string_pretty(&res)?)?;
            }
            if res["ok"] == false {
                return Err("replay did not decode completely".into());
            }
        }
    }
    w.flush()?;
    Ok(())
}

fn trunc(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}
