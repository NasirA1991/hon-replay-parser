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

/// Caldavar (Phoenix) 62-minute ranked match, cross-checked against ward-up.com
/// (which shows the official HoN match statistics).
#[test]
fn v7_caldavar_matches_official_stats() {
    let Some(p) = fixture("13436416.honreplay") else { return };
    let r = Replay::open(p).unwrap();
    let rep = analysis::analyze(&r, analysis::Options { include_positions: false, ..Default::default() }).unwrap();
    assert_eq!(rep.summary.duration_ms, 3_728_800); // 62:08
    assert_eq!(rep.summary.winner, "hellbourne");
    // (name, K, D, A, creep kills, denies, net worth, hero damage, building damage, sight wards)
    let official = [
        ("Mobutul", 11, 3, 15, 373, 5, 46749, 53422, 1659, 0),
        ("77777777", 11, 18, 14, 217, 0, 28633, 32246, 3500, 0),
        ("kobla007", 2, 16, 20, 64, 5, 18251, 17164, 597, 11),
        ("Victuur", 22, 9, 20, 203, 4, 31758, 69015, 5892, 4),
        ("ZergoMraz6", 16, 13, 16, 295, 27, 36818, 67993, 1090, 0),
        ("No1end", 13, 14, 13, 218, 11, 32054, 27108, 9467, 1),
        ("hi_v1", 14, 9, 25, 174, 1, 34301, 53977, 1559, 0),
        ("MrEOO", 6, 12, 30, 198, 4, 29373, 37307, 5356, 1),
        ("evilleader", 10, 17, 21, 191, 3, 27627, 22105, 920, 22),
        ("Rembo", 15, 10, 26, 275, 13, 38371, 47770, 5535, 1),
    ];
    for (name, k, d, a, ck, dn, nw, hd, bd, ws) in official {
        let p = rep.players.iter().find(|p| p.name == name).unwrap_or_else(|| panic!("{name} missing"));
        let t = &p.totals;
        assert_eq!((t.kills, t.deaths, t.assists, t.last_hits, t.denies, t.net_worth), (k, d, a, ck, dn, nw), "{name}");
        assert_eq!((t.hero_damage.floor() as i64, t.building_damage.floor() as i64, t.wards_placed), (hd, bd, ws), "{name}");
    }
    // revelation wards per owner from the ward lifecycle (official WR column)
    for (name, wr) in [("kobla007", 14), ("evilleader", 18), ("hi_v1", 3), ("MrEOO", 3), ("Rembo", 12), ("Victuur", 0)] {
        let c = rep.players.iter().find(|p| p.name == name).unwrap().client;
        assert_eq!(rep.wards.iter().filter(|w| w.owner == Some(c) && w.kind == "revelation").count(), wr, "{name}");
    }
    assert_eq!(rep.objectives.iter().filter(|o| o.name == "kongor").count(), 5);
    assert_eq!(rep.objectives.iter().filter(|o| o.name == "boss_phoenix").count(), 4);
}
