//! Integration tests against real replays. Fetch them with `scripts/fetch-fixtures.sh`
//! (or point HONREPLAY_FIXTURES at a directory containing them). Missing fixtures skip.
use std::path::PathBuf;

use honreplay::{Replay, analysis};

fn fixture(name: &str) -> Option<PathBuf> {
    let dir = std::env::var("HONREPLAY_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"));
    let p = dir.join(name);
    if p.exists() {
        Some(p)
    } else {
        eprintln!(
            "skipping: fixture {} not found (run scripts/fetch-fixtures.sh)",
            p.display()
        );
        None
    }
}

fn decode_all(r: &Replay) -> (u64, u64, usize) {
    let mut fr = r.frames();
    let (mut frames, mut changes) = (0u64, 0u64);
    while let Some(f) = fr.next_frame() {
        let f = f.expect("frame decodes");
        frames += 1;
        changes += f.changes.len() as u64;
    }
    (frames, changes, fr.pool().len())
}

#[test]
fn v7_midwars_decodes_every_frame() {
    let Some(p) = fixture("13337970.honreplay") else {
        return;
    };
    let r = Replay::open(p).unwrap();
    assert_eq!(r.header.format_version, 7);
    assert_eq!(r.header.server_version, "0.13.3.0");
    assert_eq!(decode_all(&r), (165_153, 4_692_145, 523));
}

#[test]
fn v6_caldavar_decodes_every_frame() {
    let Some(p) = fixture("12857222.honreplay") else {
        return;
    };
    let r = Replay::open(p).unwrap();
    assert_eq!(r.header.format_version, 6);
    assert_eq!(decode_all(&r), (96_056, 4_051_805, 756));
}

#[test]
fn v7_report_matches_scoreboard() {
    let Some(p) = fixture("13337970.honreplay") else {
        return;
    };
    let r = Replay::open(p).unwrap();
    let rep = analysis::analyze(
        &r,
        analysis::Options {
            include_positions: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rep.summary.match_id, 13_337_970);
    assert_eq!(rep.summary.duration_ms, 1_585_540);
    assert_eq!(rep.summary.winner, "legion");
    assert_eq!(rep.players.len(), 10);
    let kda: Vec<(u32, i64, i64, i64)> = rep
        .players
        .iter()
        .map(|p| (p.client, p.totals.kills, p.totals.deaths, p.totals.assists))
        .collect();
    assert_eq!(
        kda,
        vec![
            (1, 9, 12, 30),
            (2, 12, 15, 30),
            (3, 7, 8, 31),
            (4, 16, 8, 33),
            (5, 7, 14, 40),
            (6, 17, 6, 22),
            (7, 18, 9, 32),
            (8, 10, 5, 41),
            (9, 6, 10, 41),
            (10, 1, 16, 30)
        ]
    );
    let heroes: Vec<&str> = rep.players.iter().map(|p| p.hero.as_str()).collect();
    assert_eq!(
        heroes,
        [
            "wretched_hag",
            "maliken",
            "accursed",
            "kinesis",
            "pollywog_priest",
            "master_of_arms",
            "swiftblade",
            "glacius",
            "tempest",
            "kraken"
        ]
    );
    // every hero kill is attributed to a death
    assert_eq!(
        rep.kills.len() as i64,
        rep.players.iter().map(|p| p.totals.deaths).sum::<i64>()
    );
    assert_eq!(
        rep.objectives.iter().filter(|o| o.name == "kongor").count(),
        3
    );
    assert!(rep.chat.iter().any(|c| c.message == "hwo is tank"));
    assert!(
        rep.players
            .iter()
            .all(|p| !p.skill_build.is_empty() && !p.items.is_empty())
    );
}

#[test]
fn v6_report_and_lanes() {
    let Some(p) = fixture("12857222.honreplay") else {
        return;
    };
    let r = Replay::open(p).unwrap();
    let rep = analysis::analyze(&r, analysis::Options::default()).unwrap();
    assert_eq!(rep.summary.match_id, 12_857_222);
    assert_eq!(rep.summary.winner, "hellbourne");
    assert_eq!(rep.summary.paused_ms, 73_500);
    let engineer = rep.players.iter().find(|p| p.hero == "engineer").unwrap();
    assert_eq!(engineer.name, "evilleader");
    assert_eq!(engineer.laning.as_ref().unwrap().lane, "mid");
    assert!(rep.buildings.len() >= 10);
    // game time equals the engine's own clock: Player.m_uiLastDeathTime
    assert!(rep.kills.iter().any(|k| k.t == 201_360));
}

#[test]
fn bit_reader_and_events() {
    let (len, unknown) = honreplay::events::payload_len(0x0202);
    assert_eq!((len, unknown), (11, false));
    assert_eq!(honreplay::events::payload_len(0x8204).0, 11);
    assert_eq!(honreplay::events::payload_len(0x0003_0212).0, 13);
}
