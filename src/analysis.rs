//! Match analytics built on top of the raw entity stream: the data a stats site needs.
//!
//! Everything here is derived only from what the replay contains. Derived values (for
//! example hero level from experience, or kill attribution) are documented in
//! `docs/DATA.md` together with how they are computed.
use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::entity::{Change, Entity, Pool};
use crate::error::Result;
use crate::header::Header;
use crate::replay::{Frame, Replay};
use crate::schema::Value;

pub const PARSER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Cumulative experience required for levels 1..=25.
pub const LEVEL_XP: [i64; 25] = [
    0, 200, 500, 900, 1400, 2000, 2700, 3500, 4400, 5400, 6500, 7700, 9000, 10400, 11900, 13500,
    15200, 17000, 18900, 20900, 23000, 25200, 27500, 29900, 32400,
];
pub fn level_from_xp(xp: i64) -> u32 {
    LEVEL_XP
        .iter()
        .rposition(|t| xp >= *t)
        .map(|i| i as u32 + 1)
        .unwrap_or(1)
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Sampling interval of timelines in milliseconds of game time.
    pub sample_ms: i64,
    /// Sampling interval of hero positions in milliseconds.
    pub position_ms: i64,
    pub include_positions: bool,
    pub include_events: bool,
}
impl Default for Options {
    fn default() -> Self {
        Options {
            sample_ms: 10_000,
            position_ms: 1_000,
            include_positions: true,
            include_events: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MatchSummary {
    pub match_id: u64,
    pub map: String,
    pub map_fancy_name: String,
    pub game_mode: String,
    pub game_options: String,
    pub game_name: String,
    pub server_name: String,
    pub server_version: String,
    pub format_version: u32,
    pub team_size: u32,
    pub date: String,
    pub time: String,
    /// "legion", "hellbourne" or "" when unknown.
    pub winner: String,
    /// Game time at which the match ended (ms): start of the post-game phase, else the
    /// reported match length.
    pub duration_ms: i64,
    /// GameInfo.m_uiMatchLength as reported by the server (can differ from the timer when
    /// the game ended by concede/vote).
    pub reported_match_length_ms: Option<i64>,
    /// Total time the game was paused (GameInfo.m_uiPauseTime).
    pub paused_ms: i64,
    pub server_fps: u32,
    pub frames: u64,
    /// Server clock (ms) at which the match timer starts (game time 0).
    pub start_clock: u32,
    pub first_clock: u32,
    pub last_clock: u32,
    pub passive_gpm: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Totals {
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub last_hits: i64,
    pub neutral_kills: i64,
    pub denies: i64,
    pub gold: f64,
    pub gold_spent: i64,
    /// Gold spent + unspent gold: HoN's net worth as shown in game and on ward-up.
    pub net_worth: i64,
    pub gold_earned: i64,
    pub gold_starting: i64,
    pub gold_hero_kills: i64,
    pub gold_assists: i64,
    pub gold_buildings: i64,
    pub gold_creeps: i64,
    pub gold_neutrals: i64,
    pub gold_lost_to_deaths: i64,
    pub hero_damage: f64,
    pub building_damage: f64,
    pub damage_received_magic: f64,
    pub damage_received_physical: f64,
    pub damage_received_pure: f64,
    pub wards_placed: i64,
    pub wards_destroyed: i64,
    pub camps_stacked: i64,
    pub kill_streak: i64,
    pub experience: i64,
    pub level: u32,
    pub buybacks: i64,
    pub gpm: f64,
    pub xpm: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Sample {
    /// Game time (ms).
    pub t: i64,
    pub gold: f64,
    pub net_worth: i64,
    pub gold_earned: i64,
    pub experience: i64,
    pub level: u32,
    pub kills: i64,
    pub deaths: i64,
    pub assists: i64,
    pub last_hits: i64,
    pub denies: i64,
    pub hero_damage: f64,
    pub damage_received: f64,
    pub health: i64,
    pub max_health: f64,
    pub mana: i64,
    pub max_mana: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PositionSample {
    pub t: i64,
    pub x: i32,
    pub y: i32,
    pub alive: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ItemEvent {
    pub t: i64,
    /// "purchased" (created while the player's gold spent increased), "acquired" (created
    /// without spending: combines, pick-ups, gifts), "moved" (stash/inventory transfer or slot
    /// change), "removed" (sold, consumed, combined or dropped).
    pub action: String,
    pub item: String,
    pub type_id: u16,
    pub entity: u32,
    pub slot: Option<i64>,
    /// "inventory", "stash" or "innate".
    pub slot_kind: String,
    pub charges: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InventorySlot {
    pub slot: i64,
    pub slot_kind: String,
    pub item: String,
    pub charges: i64,
    pub acquired_t: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillUp {
    pub t: i64,
    pub ability: String,
    pub slot: i64,
    pub level: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Cast {
    pub t: i64,
    pub ability: String,
    pub slot: i64,
    pub cooldown_ms: i64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Death {
    pub t: i64,
    pub killer: Option<u32>,
    pub assisters: Vec<u32>,
    pub x: i32,
    pub y: i32,
    pub respawn_t: Option<i64>,
    pub gold_lost: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerReport {
    pub client: u32,
    pub name: String,
    pub clan: String,
    pub account_id: String,
    pub team: String,
    pub team_slot: i64,
    pub role: i64,
    pub hero: String,
    pub hero_type_id: u16,
    pub hero_entity: u32,
    pub hero_variant: Option<u32>,
    pub hero_avatar_key: Option<i64>,
    pub disconnected_at: Option<i64>,
    pub totals: Totals,
    pub timeline: Vec<Sample>,
    pub positions: Vec<PositionSample>,
    pub items: Vec<ItemEvent>,
    pub final_inventory: Vec<InventorySlot>,
    pub skill_build: Vec<SkillUp>,
    pub casts: Vec<Cast>,
    pub deaths: Vec<Death>,
    pub kills: Vec<u32>,
    pub laning: Option<Laning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Laning {
    /// "top", "mid", "bottom", "jungle" or "roaming".
    pub lane: String,
    /// Share of laning-phase samples (1:00-10:00) spent in that lane.
    pub confidence: f64,
    pub partners: Vec<u32>,
    pub opponents: Vec<u32>,
    /// Stats at 10:00 (or the last sample before).
    pub at_10: Option<Sample>,
    /// Gold earned / experience difference against the lane opponents' average at 10:00.
    pub gold_diff_10: Option<i64>,
    pub xp_diff_10: Option<i64>,
    /// "won", "lost" or "even" from the combined gold+xp difference.
    pub outcome: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KillEvent {
    pub t: i64,
    pub victim: u32,
    pub killer: Option<u32>,
    pub assisters: Vec<u32>,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WardEvent {
    pub kind: String,
    pub team: String,
    pub owner: Option<u32>,
    pub placed_t: i64,
    pub removed_t: Option<i64>,
    pub x: i32,
    pub y: i32,
    /// "expired", "destroyed" or "" while still alive at the end.
    pub fate: String,
    pub lifetime_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnitEvent {
    pub t: i64,
    pub name: String,
    pub team: String,
    pub event: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatLine {
    pub t: i64,
    pub client: u32,
    pub name: String,
    pub channel: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamReport {
    pub team: String,
    pub kills: i64,
    pub deaths: i64,
    pub tower_kills: i64,
    pub towers_remaining: Option<i64>,
    pub net_worth: i64,
    pub gold_earned: i64,
    pub experience: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdvantageSample {
    pub t: i64,
    pub gold: i64,
    pub experience: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub parser_version: String,
    pub summary: MatchSummary,
    pub teams: Vec<TeamReport>,
    pub players: Vec<PlayerReport>,
    pub kills: Vec<KillEvent>,
    pub wards: Vec<WardEvent>,
    pub buildings: Vec<UnitEvent>,
    pub objectives: Vec<UnitEvent>,
    pub runes: Vec<UnitEvent>,
    pub couriers: Vec<UnitEvent>,
    /// Legion minus Hellbourne.
    pub advantage: Vec<AdvantageSample>,
    pub chat: Vec<ChatLine>,
    pub warnings: Vec<String>,
}

/// Field index lookup by name, per typedesc.
pub struct FieldIndex {
    maps: Vec<HashMap<String, usize>>,
}
impl FieldIndex {
    pub fn new(h: &Header) -> Self {
        FieldIndex {
            maps: h
                .typedescs
                .iter()
                .map(|t| {
                    t.fields
                        .iter()
                        .enumerate()
                        .map(|(i, f)| (f.name.clone(), i))
                        .collect()
                })
                .collect(),
        }
    }
    pub fn get(&self, td: usize, name: &str) -> Option<usize> {
        self.maps.get(td)?.get(name).copied()
    }
}

fn team_name(team_id: i64) -> String {
    match team_id {
        5 | 1 => "legion".into(),
        6 | 2 => "hellbourne".into(),
        4 => "spectators".into(),
        _ => format!("team{team_id}"),
    }
}

/// `064/heroes/<hero>/<variant>/...` style resource paths -> short names.
pub fn short_name(path: &str) -> String {
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        [_, "heroes", "_innate_abilities", name, ..] => name.to_string(),
        [_, "heroes", hero, _variant, "hero.entity"] => hero.to_string(),
        [_, "heroes", hero, _variant, ab, ..] if ab.starts_with("ability_") => {
            format!("{hero}/{ab}")
        }
        [_, "items", _cat, name, ..] => name.to_string(),
        _ => {
            let file = parts
                .last()
                .copied()
                .unwrap_or("")
                .trim_end_matches(".entity");
            let parent = if parts.len() >= 2 {
                parts[parts.len() - 2]
            } else {
                ""
            };
            if matches!(
                file,
                "gadget"
                    | "item"
                    | "ability"
                    | "powerup"
                    | "state"
                    | "pet"
                    | "projectile"
                    | "affector"
            ) {
                parent.to_string()
            } else {
                file.to_string()
            }
        }
    }
}

struct Ctx<'a> {
    h: &'a Header,
    fi: FieldIndex,
}
impl Ctx<'_> {
    fn tname(&self, e: &Entity) -> &str {
        &self.h.typedescs[e.typedesc].name
    }
    fn f<'e>(&self, e: &'e Entity, name: &str) -> Option<&'e Value> {
        self.fi.get(e.typedesc, name).map(|i| &e.fields[i])
    }
    fn i(&self, e: &Entity, name: &str) -> i64 {
        self.f(e, name).and_then(|v| v.as_i64()).unwrap_or(0)
    }
    fn fl(&self, e: &Entity, name: &str) -> f64 {
        self.f(e, name).and_then(|v| v.as_f64()).unwrap_or(0.0)
    }
    fn path(&self, e: &Entity) -> &str {
        self.h.resource_path(e.type_id).unwrap_or("")
    }
    fn pos(&self, e: &Entity) -> (i32, i32) {
        if let Some(v) = self.f(e, "m_v3Position.xy").and_then(|v| v.as_xyz()) {
            return (v[0] as i32, v[1] as i32);
        }
        if let Some(v) = self.f(e, "m_v3Position").and_then(|v| v.as_xyz()) {
            return (v[0] as i32, v[1] as i32);
        }
        (0, 0)
    }
    fn field_name(&self, e: &Entity, idx: u16) -> &str {
        &self.h.typedescs[e.typedesc].fields[idx as usize].name
    }
}

const NONE32: i64 = 0xFFFF_FFFF;

struct PlayerState {
    report: PlayerReport,
    player_entity: u32,
    last_kills: i64,
    last_deaths: i64,
    last_assists: i64,
    next_sample: i64,
    next_pos: i64,
    abilities: HashMap<u32, (String, i64, i64)>, // entity -> (name, slot, level)
    items: HashMap<u32, (String, i64, i64, u16)>, // entity -> (name, slot, acquired_t, type)
    cooldowns: HashMap<u32, i64>,
    last_spent: i64,
    spent_delta: i64,
    lane_counts: HashMap<&'static str, u32>,
    next_lane: i64,
}

pub fn analyze(replay: &Replay, opts: Options) -> Result<Report> {
    let h = &replay.header;
    let ctx = Ctx {
        h,
        fi: FieldIndex::new(h),
    };
    let info = &replay.info;
    let mut summary = MatchSummary {
        match_id: info.match_id,
        map: if info.map_name.is_empty() {
            h.map_name.clone()
        } else {
            info.map_name.clone()
        },
        map_fancy_name: info.map_fancy_name.clone(),
        game_mode: info.game_mode.clone(),
        game_options: info.game_options.clone(),
        game_name: info.game_name.clone(),
        server_name: info.server_name.clone(),
        server_version: h.server_version.clone(),
        format_version: h.format_version,
        team_size: info.team_size as u32,
        date: info.date.clone(),
        time: info.time.clone(),
        winner: {
            let w = info.winner.to_ascii_lowercase();
            if w.contains("legion") || w == "1" {
                "legion".into()
            } else if w.contains("hell") || w == "2" {
                "hellbourne".into()
            } else {
                String::new()
            }
        },
        duration_ms: info.match_length_ms as i64,
        server_fps: replay.fps(),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    let mut players: BTreeMap<u32, PlayerState> = BTreeMap::new(); // client -> state
    let mut hero_owner: HashMap<u32, u32> = HashMap::new(); // hero entity -> client
    let mut start_clock: Option<u32> = None;
    let mut start_confirmed = false;
    let mut end_clock: Option<i64> = None;
    let mut kills: Vec<KillEvent> = Vec::new();
    let mut wards: HashMap<u32, WardEvent> = HashMap::new();
    let mut wards_done: Vec<WardEvent> = Vec::new();
    let mut buildings = Vec::new();
    let mut objectives = Vec::new();
    let mut runes = Vec::new();
    let mut couriers = Vec::new();
    let mut advantage = Vec::new();
    let mut next_adv = 0i64;
    let mut chat = Vec::new();
    let mut chat_seen: HashMap<(i64, u32, String), ()> = HashMap::new();
    let mut bases: HashMap<String, (f64, f64)> = HashMap::new();
    let mut team_towers: BTreeMap<String, (i64, Option<i64>)> = BTreeMap::new();
    let mut frames = replay.frames();
    let mut nframes = 0u64;
    let mut prev_pool_snapshot: HashMap<u32, Entity> = HashMap::new();

    while let Some(frame) = frames.next_frame() {
        let frame: Frame = match frame {
            Ok(f) => f,
            Err(e) => {
                warnings.push(format!("stopped early: {e}"));
                break;
            }
        };
        nframes += 1;
        if summary.first_clock == 0 {
            summary.first_clock = frame.clock;
        }
        summary.last_clock = frame.clock;
        let pool: &Pool = frames.pool();

        // --- match clock -------------------------------------------------------------
        for ch in &frame.changes {
            let id = match ch {
                Change::Create { id, .. } | Change::Update { id, .. } => *id,
                _ => continue,
            };
            let Some(e) = pool.get(id) else { continue };
            if ctx.tname(e) == "GameInfo" {
                let phase = ctx.i(e, "m_uiGamePhase");
                let pst = ctx.i(e, "m_uiPhaseStartTime");
                let pdur = ctx.i(e, "m_uiPhaseDuration");
                if phase == 4 && !start_confirmed {
                    start_clock = Some(pst as u32);
                    start_confirmed = true;
                }
                if phase == 3 && !start_confirmed && pdur != NONE32 {
                    start_clock = Some((pst + pdur) as u32);
                }
                let ml = ctx.i(e, "m_uiMatchLength");
                if ml != 0 && ml != NONE32 {
                    summary.reported_match_length_ms = Some(ml);
                }
                if phase >= 5 && end_clock.is_none() && start_confirmed {
                    end_clock = Some(pst);
                }
                let pt = ctx.i(e, "m_uiPauseTime");
                if pt != NONE32 {
                    summary.paused_ms = pt;
                }
                let pg = ctx.fl(e, "m_fPassiveGpm");
                if pg > 0.0 {
                    summary.passive_gpm = Some(pg as f32);
                }
            }
        }
        let t = match start_clock {
            Some(s) => frame.clock as i64 - s as i64,
            None => -1,
        };
        let game_t = |clock: i64| -> i64 {
            match start_clock {
                Some(s) => clock - s as i64,
                None => -1,
            }
        };

        // --- players -------------------------------------------------------------------
        for ch in &frame.changes {
            if let Change::Create { id, .. } = ch {
                let e = pool.get(*id).unwrap();
                if ctx.tname(e) == "Player" {
                    let client = ctx.i(e, "m_iClientNumber") as u32;
                    if ctx.i(e, "m_uiTeamID") == 4 || client > 63 {
                        continue;
                    }
                    let name = frames
                        .string(2, ctx.i(e, "m_unNameIndex") as u32)
                        .unwrap_or("")
                        .to_string();
                    let clan = frames
                        .string(2, ctx.i(e, "m_unClanNameIndex") as u32)
                        .unwrap_or("")
                        .to_string();
                    let acc = info
                        .players
                        .iter()
                        .find(|p| p.client_number as u32 + 1 == client)
                        .map(|p| p.account_id.clone())
                        .unwrap_or_else(|| match ctx.f(e, "m_iAccountID") {
                            Some(Value::Wide(w)) if *w != 0 => format_uuid(*w),
                            Some(v) => v.as_i64().map(|x| x.to_string()).unwrap_or_default(),
                            None => String::new(),
                        });
                    players.entry(client).or_insert_with(|| PlayerState {
                        report: PlayerReport {
                            client,
                            name,
                            clan,
                            account_id: acc,
                            team: String::new(),
                            team_slot: 0,
                            role: 0,
                            hero: String::new(),
                            hero_type_id: 0,
                            hero_entity: 0,
                            hero_variant: None,
                            hero_avatar_key: None,
                            disconnected_at: None,
                            totals: Totals::default(),
                            timeline: Vec::new(),
                            positions: Vec::new(),
                            items: Vec::new(),
                            final_inventory: Vec::new(),
                            skill_build: Vec::new(),
                            casts: Vec::new(),
                            deaths: Vec::new(),
                            kills: Vec::new(),
                            laning: None,
                        },
                        player_entity: *id,
                        last_kills: 0,
                        last_deaths: 0,
                        last_assists: 0,
                        next_sample: 0,
                        next_pos: 0,
                        abilities: HashMap::new(),
                        items: HashMap::new(),
                        cooldowns: HashMap::new(),
                        last_spent: 0,
                        spent_delta: 0,
                        lane_counts: HashMap::new(),
                        next_lane: 0,
                    });
                }
            }
        }
        // refresh player identity/hero links (cheap: 10 players)
        for st in players.values_mut() {
            let Some(pe) = pool.get(st.player_entity) else {
                continue;
            };
            let r = &mut st.report;
            r.team = team_name(ctx.i(pe, "m_uiTeamID"));
            r.team_slot = ctx.i(pe, "m_iTeamIndex");
            r.role = ctx.i(pe, "m_eRole");
            let hk = ctx.i(pe, "m_uiHeroAvatarKey");
            if hk != NONE32 {
                r.hero_avatar_key = Some(hk);
            }
            let nm = frames
                .string(2, ctx.i(pe, "m_unNameIndex") as u32)
                .unwrap_or("");
            if !nm.is_empty() {
                r.name = nm.to_string();
            }
            let hero = ctx.i(pe, "m_uiHeroIndex");
            if hero != 0xFFFF && hero != NONE32 {
                if let Some(he) = pool.get(hero as u32) {
                    if ctx.tname(he) == "Hero" {
                        r.hero_entity = hero as u32;
                        r.hero_type_id = he.type_id;
                        r.hero = short_name(ctx.path(he));
                        r.hero_variant = he.variant;
                        hero_owner.insert(hero as u32, r.client);
                    }
                }
            }
            let term = ctx.i(pe, "m_uiTerminationTime");
            if term != NONE32
                && term != 0
                && r.disconnected_at.is_none()
                && start_clock.is_some()
                && game_t(term) >= 0
                && game_t(term) < summary.duration_ms.max(1) - 5000
            {
                r.disconnected_at = Some(game_t(term));
            }
            if term == NONE32 {
                r.disconnected_at = None;
            }
        }

        // --- deaths / kills ------------------------------------------------------------
        let mut frame_kill_delta: Vec<(u32, i64, i64, i64)> = Vec::new(); // client, dk, dd, da
        for st in players.values_mut() {
            let Some(pe) = pool.get(st.player_entity) else {
                continue;
            };
            let spent = ctx.i(pe, "m_aStatTotals[PLAYER_STAT_GOLD_SPENT]");
            st.spent_delta = spent - st.last_spent;
            st.last_spent = spent;
            let k = ctx.i(pe, "m_aStatTotals[PLAYER_STAT_HERO_KILLS]");
            let d = ctx.i(pe, "m_aStatTotals[PLAYER_STAT_DEATHS]");
            let a = ctx.i(pe, "m_aStatTotals[PLAYER_STAT_ASSISTS]");
            if k != st.last_kills || d != st.last_deaths || a != st.last_assists {
                frame_kill_delta.push((
                    st.report.client,
                    k - st.last_kills,
                    d - st.last_deaths,
                    a - st.last_assists,
                ));
                st.last_kills = k;
                st.last_deaths = d;
                st.last_assists = a;
            }
        }
        let mut killer_pool: Vec<u32> = frame_kill_delta
            .iter()
            .flat_map(|x| std::iter::repeat_n(x.0, x.1.max(0) as usize))
            .collect();
        for (victim, _, dd, _) in frame_kill_delta.iter().filter(|x| x.2 > 0) {
            let _ = dd;
            let vteam = players[victim].report.team.clone();
            let killer = killer_pool
                .iter()
                .position(|c| players[c].report.team != vteam)
                .map(|i| killer_pool.remove(i));
            let assisters: Vec<u32> = frame_kill_delta
                .iter()
                .filter(|x| x.3 > 0 && players[&x.0].report.team != vteam)
                .map(|x| x.0)
                .collect();
            let st = players.get(victim).unwrap();
            let (x, y) = pool
                .get(st.report.hero_entity)
                .map(|he| ctx.pos(he))
                .unwrap_or((0, 0));
            let respawn = pool
                .get(st.report.hero_entity)
                .map(|he| ctx.i(he, "m_uiRespawnTime"))
                .filter(|v| *v != NONE32)
                .map(game_t);
            let pe = pool.get(st.player_entity);
            let lost = pe
                .map(|p| ctx.i(p, "m_aStatTotals[PLAYER_STAT_DEATH_GOLD]"))
                .unwrap_or(0);
            kills.push(KillEvent {
                t,
                victim: *victim,
                killer,
                assisters: assisters.clone(),
                x,
                y,
            });
            let st = players.get_mut(victim).unwrap();
            st.report.deaths.push(Death {
                t,
                killer,
                assisters,
                x,
                y,
                respawn_t: respawn,
                gold_lost: lost,
            });
            if let Some(k) = killer {
                players.get_mut(&k).unwrap().report.kills.push(*victim);
            }
        }

        // --- items, abilities, casts ---------------------------------------------------
        let mut items_new: Vec<(u32, u32, String, i64, u16, i64)> = Vec::new();
        let mut items_gone: Vec<(u32, u32, String, i64, u16)> = Vec::new();
        for ch in &frame.changes {
            match ch {
                Change::Create { id, .. } => {
                    let e = pool.get(*id).unwrap();
                    let tn = ctx.tname(e);
                    if tn == "Item" || tn == "Ability" {
                        let owner = ctx.i(e, "m_uiOwnerIndex") as u32;
                        if let Some(client) = hero_owner.get(&owner).copied() {
                            let name = short_name(ctx.path(e));
                            let slot = ctx.i(e, "m_ySlot");
                            let st = players.get_mut(&client).unwrap();
                            if tn == "Item" {
                                if st
                                    .items
                                    .insert(*id, (name.clone(), slot, t, e.type_id))
                                    .is_some()
                                {
                                    continue;
                                } // re-created (variant swap)
                                items_new.push((
                                    client,
                                    *id,
                                    name,
                                    slot,
                                    e.type_id,
                                    ctx.i(e, "m_unCharges"),
                                ));
                            } else {
                                let lvl = ctx.i(e, "m_uiLevel");
                                st.abilities.insert(*id, (name.clone(), slot, lvl));
                                if lvl > 0 && t > -60_000 {
                                    st.report.skill_build.push(SkillUp {
                                        t,
                                        ability: name,
                                        slot,
                                        level: lvl,
                                    });
                                }
                            }
                            st.cooldowns
                                .insert(*id, ctx.i(e, "m_uiApprentCooldownTime"));
                        }
                    }
                }
                Change::Update { id, fields } => {
                    let e = pool.get(*id).unwrap();
                    let tn = ctx.tname(e);
                    if tn != "Item" && tn != "Ability" {
                        continue;
                    }
                    let owner = ctx.i(e, "m_uiOwnerIndex") as u32;
                    let Some(client) = hero_owner.get(&owner).copied() else {
                        continue;
                    };
                    let st = players.get_mut(&client).unwrap();
                    for fidx in fields {
                        match ctx.field_name(e, *fidx) {
                            "m_uiLevel" if tn == "Ability" => {
                                let lvl = ctx.i(e, "m_uiLevel");
                                let entry = st.abilities.entry(*id).or_insert((
                                    short_name(ctx.path(e)),
                                    ctx.i(e, "m_ySlot"),
                                    0,
                                ));
                                if lvl > entry.2 {
                                    st.report.skill_build.push(SkillUp {
                                        t,
                                        ability: entry.0.clone(),
                                        slot: entry.1,
                                        level: lvl,
                                    });
                                }
                                entry.2 = lvl;
                            }
                            "m_uiApprentCooldownTime" => {
                                let cd = ctx.i(e, "m_uiApprentCooldownTime");
                                let prev = st.cooldowns.insert(*id, cd).unwrap_or(NONE32);
                                if cd != NONE32 && cd != prev && cd != 0 {
                                    let dur = ctx.i(e, "m_uiApprentCooldownDuration");
                                    st.report.casts.push(Cast {
                                        t: game_t(cd),
                                        ability: short_name(ctx.path(e)),
                                        slot: ctx.i(e, "m_ySlot"),
                                        cooldown_ms: if dur == NONE32 { 0 } else { dur },
                                        kind: tn.to_ascii_lowercase(),
                                    });
                                }
                            }
                            "m_ySlot" if tn == "Item" => {
                                let slot = ctx.i(e, "m_ySlot");
                                if let Some(it) = st.items.get_mut(id) {
                                    if it.1 != slot {
                                        it.1 = slot;
                                        st.report.items.push(ItemEvent {
                                            t,
                                            action: "moved".into(),
                                            item: it.0.clone(),
                                            type_id: e.type_id,
                                            entity: *id,
                                            slot: Some(slot),
                                            slot_kind: slot_kind(slot).into(),
                                            charges: None,
                                        });
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Change::Delete { id, .. } => {
                    for st in players.values_mut() {
                        if let Some((name, slot, _, tid)) = st.items.remove(id) {
                            items_gone.push((st.report.client, *id, name, slot, tid));
                        }
                        st.abilities.remove(id);
                    }
                }
            }
        }

        for (client, id, name, slot, tid, charges) in items_new {
            let st = players.get_mut(&client).unwrap();
            let action =
                if let Some(i) = items_gone.iter().position(|g| g.0 == client && g.4 == tid) {
                    items_gone.remove(i);
                    "moved"
                } else if st.spent_delta > 0 || t < 0 {
                    "purchased"
                } else {
                    "acquired"
                };
            st.report.items.push(ItemEvent {
                t,
                action: action.into(),
                item: name,
                type_id: tid,
                entity: id,
                slot: Some(slot),
                slot_kind: slot_kind(slot).into(),
                charges: Some(charges),
            });
        }
        for (client, id, name, slot, tid) in items_gone {
            let st = players.get_mut(&client).unwrap();
            st.report.items.push(ItemEvent {
                t,
                action: "removed".into(),
                item: name,
                type_id: tid,
                entity: id,
                slot: Some(slot),
                slot_kind: slot_kind(slot).into(),
                charges: None,
            });
        }

        // --- wards, buildings, objectives, runes, couriers -----------------------------
        for ch in &frame.changes {
            match ch {
                Change::Create { id, .. } => {
                    let e = pool.get(*id).unwrap();
                    let path = ctx.path(e);
                    let tn = ctx.tname(e);
                    if tn == "Gadget" && path.contains("/items/basic/ward_of_") {
                        let (x, y) = ctx.pos(e);
                        let owner = ctx.i(e, "m_iOwnerClientNumber");
                        let life = ctx.i(e, "m_uiLifetime");
                        wards.insert(
                            *id,
                            WardEvent {
                                kind: if path.contains("revelation") {
                                    "revelation".into()
                                } else {
                                    "sight".into()
                                },
                                team: team_name(ctx.i(e, "m_uiTeamID")),
                                owner: if (0..64).contains(&owner) {
                                    Some(owner as u32)
                                } else {
                                    None
                                },
                                placed_t: t,
                                removed_t: None,
                                x,
                                y,
                                fate: String::new(),
                                lifetime_ms: if life == NONE32 || life == 0 {
                                    None
                                } else {
                                    Some(life)
                                },
                            },
                        );
                    } else if tn == "Powerup" {
                        let (x, y) = ctx.pos(e);
                        runes.push(UnitEvent {
                            t,
                            name: short_name(path),
                            team: String::new(),
                            event: "spawned".into(),
                            x,
                            y,
                        });
                    }
                }
                Change::Delete { id, .. } => {
                    if let Some(mut w) = wards.remove(id) {
                        w.removed_t = Some(t);
                        w.fate = match w.lifetime_ms {
                            Some(l) if t - w.placed_t + 1500 < l => "destroyed".into(),
                            _ => "expired".into(),
                        };
                        wards_done.push(w);
                    } else if let Some(e) = prev_pool_snapshot.get(id) {
                        let tn = ctx.tname(e);
                        let path = ctx.path(e);
                        let (x, y) = ctx.pos(e);
                        let team = team_name(ctx.i(e, "m_uiTeamID"));
                        let _ = (&team, x, y);
                        if tn == "Powerup" {
                            runes.push(UnitEvent {
                                t,
                                name: short_name(path),
                                team: String::new(),
                                event: "taken_or_expired".into(),
                                x,
                                y,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        // unit deaths: health reaching zero (buildings stay in the world when destroyed)
        for ch in &frame.changes {
            let Change::Update { id, .. } = ch else {
                continue;
            };
            let e = pool.get(*id).unwrap();
            let tn = ctx.tname(e);
            if !matches!(tn, "Building" | "Neutral" | "Pet" | "Creep") {
                continue;
            }
            let path = ctx.path(e);
            let is_boss = tn == "Neutral" && path.contains("/boss/");
            let is_courier = tn == "Pet" && path.contains("courier");
            if !(tn == "Building" || is_boss || is_courier) {
                continue;
            }
            let before = prev_pool_snapshot
                .get(id)
                .map(|p| ctx.i(p, "m_fHealth"))
                .unwrap_or(1);
            let now = ctx.i(e, "m_fHealth");
            if before > 0 && now <= 0 && t >= 0 {
                let (x, y) = ctx.pos(e);
                let ev = UnitEvent {
                    t,
                    name: short_name(path),
                    team: team_name(ctx.i(e, "m_uiTeamID")),
                    event: if tn == "Building" {
                        "destroyed".into()
                    } else {
                        "killed".into()
                    },
                    x,
                    y,
                };
                if tn == "Building" {
                    buildings.push(ev)
                } else if is_boss {
                    objectives.push(ev)
                } else {
                    couriers.push(ev)
                }
            }
        }
        // keep a cheap snapshot of interesting entities so deletions can be described
        for ch in &frame.changes {
            match ch {
                Change::Create { id, .. } | Change::Update { id, .. } => {
                    let e = pool.get(*id).unwrap();
                    if matches!(ctx.tname(e), "Building" | "Neutral" | "Powerup" | "Pet") {
                        prev_pool_snapshot.insert(*id, e.clone());
                    }
                }
                Change::Delete { id, .. } => {
                    prev_pool_snapshot.remove(id);
                }
            }
        }

        // --- timelines -------------------------------------------------------------------
        if start_clock.is_some() {
            for st in players.values_mut() {
                let Some(pe) = pool.get(st.player_entity) else {
                    continue;
                };
                let he = pool.get(st.report.hero_entity);
                if t >= st.next_sample && t <= summary.duration_ms.max(t) {
                    let tot = totals(&ctx, pe, he);
                    st.report.timeline.push(Sample {
                        t,
                        gold: tot.gold,
                        gold_earned: tot.gold_earned, net_worth: tot.net_worth,
                        experience: tot.experience,
                        level: tot.level,
                        kills: tot.kills,
                        deaths: tot.deaths,
                        assists: tot.assists,
                        last_hits: tot.last_hits,
                        denies: tot.denies,
                        hero_damage: tot.hero_damage,
                        damage_received: tot.damage_received_magic
                            + tot.damage_received_physical
                            + tot.damage_received_pure,
                        health: he.map(|h| ctx.i(h, "m_fHealth")).unwrap_or(0),
                        max_health: he.map(|h| ctx.fl(h, "m_fMaxHealth")).unwrap_or(0.0),
                        mana: he.map(|h| ctx.i(h, "m_fMana")).unwrap_or(0),
                        max_mana: he.map(|h| ctx.fl(h, "m_fMaxMana")).unwrap_or(0.0),
                    });
                    st.next_sample = (t / opts.sample_ms + 1) * opts.sample_ms;
                }
                if opts.include_positions
                    && t >= st.next_pos
                    && t >= 0
                    && t <= summary.duration_ms.max(t)
                {
                    if let Some(he) = he {
                        let (x, y) = ctx.pos(he);
                        let alive = ctx.i(he, "m_fHealth") > 0;
                        st.report.positions.push(PositionSample { t, x, y, alive });
                    }
                    st.next_pos = (t / opts.position_ms + 1) * opts.position_ms;
                }
                if t >= st.next_lane && (60_000..=600_000).contains(&t) {
                    st.next_lane = (t / 1000 + 1) * 1000;
                    if let (Some(he), Some(l), Some(hb)) =
                        (he, bases.get("legion"), bases.get("hellbourne"))
                    {
                        if ctx.i(he, "m_fHealth") > 0 {
                            let (x, y) = ctx.pos(he);
                            if let Some(lane) =
                                classify_lane((x as f64, y as f64), *l, *hb, &st.report.team)
                            {
                                *st.lane_counts.entry(lane).or_default() += 1;
                            }
                        }
                    }
                }
            }
            if t >= next_adv && t >= 0 {
                let (mut g, mut x) = (0i64, 0i64);
                for st in players.values() {
                    let Some(pe) = pool.get(st.player_entity) else {
                        continue;
                    };
                    let tot = totals(&ctx, pe, pool.get(st.report.hero_entity));
                    let s = if st.report.team == "legion" {
                        1
                    } else if st.report.team == "hellbourne" {
                        -1
                    } else {
                        0
                    };
                    g += s * tot.net_worth;
                    x += s * tot.experience;
                }
                advantage.push(AdvantageSample {
                    t,
                    gold: g,
                    experience: x,
                });
                next_adv = (t / opts.sample_ms + 1) * opts.sample_ms;
            }
        }

        // --- team info ---------------------------------------------------------------------
        for ch in &frame.changes {
            if let Change::Create { id, .. } | Change::Update { id, .. } = ch {
                let e = pool.get(*id).unwrap();
                if ctx.tname(e) == "TeamInfo" {
                    let tn = team_name(ctx.i(e, "m_uiTeamID"));
                    let bi = ctx.i(e, "m_uiBaseBuildingIndex");
                    if let Some(b) = pool.get(bi as u32) {
                        let (x, y) = ctx.pos(b);
                        if x != 0 || y != 0 {
                            bases.insert(tn.clone(), (x as f64, y as f64));
                        }
                    }
                    team_towers.insert(
                        tn,
                        (
                            ctx.i(e, "m_aStatTotals[TEAM_STAT_TOWER_KILLS]"),
                            Some(ctx.i(e, "m_uiCurrentTowerCount")),
                        ),
                    );
                }
            }
        }

        // --- chat --------------------------------------------------------------------------
        let mut groups: HashMap<(u8, u8, &[u8]), Vec<u32>> = HashMap::new();
        for p in &frame.packets {
            let d = &p.data;
            if d.len() < 4 || (d[0] != 2 && d[0] != 3) {
                continue;
            }
            let Some(end) = d[2..].iter().position(|b| *b == 0) else {
                continue;
            };
            if end == 0 || end > 1000 {
                continue;
            }
            groups
                .entry((d[0], d[1], &d[2..2 + end]))
                .or_default()
                .push(p.receiver);
        }
        for ((code, sender, msg), receivers) in groups {
            let Ok(text) = std::str::from_utf8(msg) else {
                continue;
            };
            if text.chars().any(|c| c.is_control()) || text.trim().is_empty() {
                continue;
            }
            let Some(st) = players.get(&(sender as u32)) else {
                continue;
            };
            let _ = &receivers;
            let key = (t, sender as u32, text.to_string());
            if chat_seen.insert(key, ()).is_some() {
                continue;
            }
            chat.push(ChatLine {
                t,
                client: sender as u32,
                name: st.report.name.clone(),
                channel: if code == 3 {
                    "team".into()
                } else {
                    "all".into()
                },
                message: text.to_string(),
            });
        }
    }

    // --- finalize -------------------------------------------------------------------------
    summary.frames = nframes;
    if let (Some(s0), Some(e)) = (start_clock, end_clock) {
        summary.duration_ms = e - s0 as i64;
    } else if let Some(ml) = summary.reported_match_length_ms {
        if summary.duration_ms == 0 {
            summary.duration_ms = ml;
        }
    }
    summary.start_clock = start_clock.unwrap_or(summary.first_clock);
    if !start_confirmed {
        warnings.push(
            "match start (GameInfo phase 4) not seen; game time is derived from the pre-game phase"
                .into(),
        );
    }
    let pool = frames.pool();
    let minutes = (summary.duration_ms.max(1) as f64) / 60_000.0;
    for st in players.values_mut() {
        if let Some(pe) = pool.get(st.player_entity) {
            st.report.totals = totals(&ctx, pe, pool.get(st.report.hero_entity));
            st.report.totals.gpm = st.report.totals.net_worth as f64 / minutes;
            st.report.totals.xpm = st.report.totals.experience as f64 / minutes;
        }
        let end = summary.duration_ms;
        let mut inv: Vec<InventorySlot> = st
            .items
            .iter()
            .filter_map(|(id, (name, slot, acq, _))| {
                pool.get(*id).map(|e| InventorySlot {
                    slot: *slot,
                    slot_kind: slot_kind(*slot).into(),
                    item: name.clone(),
                    charges: ctx.i(e, "m_unCharges"),
                    acquired_t: *acq,
                })
            })
            .collect();
        inv.sort_by_key(|s| s.slot);
        st.report.final_inventory = inv;
        st.report
            .casts
            .retain(|c| c.t >= -120_000 && c.t <= end + 60_000);
        st.report.casts.sort_by_key(|c| c.t);
    }
    let single_lane = summary.map.to_ascii_lowercase().contains("midwars");
    let lanes: Vec<(u32, String, String, f64)> = players
        .values()
        .map(|st| {
            let total: u32 = st.lane_counts.values().sum();
            let best = st
                .lane_counts
                .iter()
                .max_by_key(|(_, n)| **n)
                .map(|(l, n)| (l.to_string(), *n as f64 / total.max(1) as f64));
            let (lane, conf) = if single_lane {
                ("mid".to_string(), 1.0)
            } else {
                best.unwrap_or(("unknown".into(), 0.0))
            };
            let lane = if !single_lane && conf < 0.4 {
                "roaming".to_string()
            } else {
                lane
            };
            (st.report.client, st.report.team.clone(), lane, conf)
        })
        .collect();
    let at10 = |st: &PlayerState| {
        st.report
            .timeline
            .iter()
            .filter(|s| s.t <= 600_000)
            .last()
            .cloned()
    };
    let snapshot: HashMap<u32, Option<Sample>> = players
        .values()
        .map(|st| (st.report.client, at10(st)))
        .collect();
    for (client, team, lane, conf) in &lanes {
        let partners: Vec<u32> = lanes
            .iter()
            .filter(|o| o.0 != *client && o.1 == *team && o.2 == *lane)
            .map(|o| o.0)
            .collect();
        let opponents: Vec<u32> = if matches!(lane.as_str(), "top" | "mid" | "bottom") {
            lanes
                .iter()
                .filter(|o| o.1 != *team && o.2 == *lane)
                .map(|o| o.0)
                .collect()
        } else {
            Vec::new()
        };
        let me = snapshot.get(client).cloned().flatten();
        let opp: Vec<Sample> = opponents
            .iter()
            .filter_map(|o| snapshot.get(o).cloned().flatten())
            .collect();
        let (gd, xd) = match (&me, opp.is_empty()) {
            (Some(m), false) => {
                let n = opp.len() as i64;
                let og: i64 = opp.iter().map(|s| s.net_worth).sum::<i64>() / n;
                let ox: i64 = opp.iter().map(|s| s.experience).sum::<i64>() / n;
                (Some(m.net_worth - og), Some(m.experience - ox))
            }
            _ => (None, None),
        };
        let outcome = match (gd, xd) {
            (Some(g), Some(x)) if g + x > 500 => "won",
            (Some(g), Some(x)) if g + x < -500 => "lost",
            (Some(_), Some(_)) => "even",
            _ => "",
        }
        .to_string();
        if let Some(st) = players.get_mut(client) {
            st.report.laning = Some(Laning {
                lane: lane.clone(),
                confidence: *conf,
                partners,
                opponents,
                at_10: me,
                gold_diff_10: gd,
                xp_diff_10: xd,
                outcome,
            });
        }
    }
    for (_, w) in wards.drain() {
        wards_done.push(w);
    }
    wards_done.sort_by_key(|w| w.placed_t);
    let player_list: Vec<PlayerReport> = players.into_values().map(|s| s.report).collect();
    let mut teams = Vec::new();
    for tn in ["legion", "hellbourne"] {
        let ps: Vec<&PlayerReport> = player_list.iter().filter(|p| p.team == tn).collect();
        let tt = team_towers.get(tn).cloned().unwrap_or((0, None));
        teams.push(TeamReport {
            team: tn.into(),
            kills: ps.iter().map(|p| p.totals.kills).sum(),
            deaths: ps.iter().map(|p| p.totals.deaths).sum(),
            tower_kills: tt.0,
            towers_remaining: tt.1,
            gold_earned: ps.iter().map(|p| p.totals.gold_earned).sum(), net_worth: ps.iter().map(|p| p.totals.net_worth).sum(),
            experience: ps.iter().map(|p| p.totals.experience).sum(),
        });
    }
    Ok(Report {
        parser_version: PARSER_VERSION.into(),
        summary,
        teams,
        players: player_list,
        kills,
        wards: wards_done,
        buildings,
        objectives,
        runes,
        couriers,
        advantage,
        chat,
        warnings,
    })
}

fn totals(ctx: &Ctx, pe: &Entity, he: Option<&Entity>) -> Totals {
    let s = |n: &str| ctx.i(pe, &format!("m_aStatTotals[PLAYER_STAT_{n}]"));
    let f = |n: &str| ctx.fl(pe, &format!("m_aFloatStatTotals[PLAYER_STAT_{n}]"));
    let xp = he.map(|h| ctx.i(h, "m_fExperience")).unwrap_or(0);
    let mut t = Totals {
        kills: s("HERO_KILLS"),
        deaths: s("DEATHS"),
        assists: s("ASSISTS"),
        last_hits: s("CREEP_KILLS"),
        neutral_kills: s("NEUTRAL_KILLS"),
        denies: s("DENIES"),
        gold: ctx.fl(pe, "m_fGold"),
        gold_spent: s("GOLD_SPENT"),
        gold_starting: s("STARTING_GOLD"),
        gold_hero_kills: s("PLAYER_KILL_GOLD"),
        gold_assists: s("PLAYER_ASSIST_GOLD"),
        gold_buildings: s("BUILDING_GOLD"),
        gold_creeps: s("CREEP_GOLD"),
        gold_neutrals: s("NEUTRAL_GOLD"),
        gold_lost_to_deaths: s("DEATH_GOLD"),
        hero_damage: f("HERO_DAMAGE"),
        building_damage: f("BUILDING_DAMAGE"),
        damage_received_magic: f("MAGIC_DAMAGE_RECEIVED"),
        damage_received_physical: f("PHYSICAL_DAMAGE_RECEIVED"),
        damage_received_pure: f("PURE_DAMAGE_RECEIVED"),
        wards_placed: s("WARDS_PLACED"),
        wards_destroyed: s("WARDS_DESTROYED"),
        camps_stacked: s("CAMP_STACKS"),
        kill_streak: ctx.i(pe, "m_yKillStreak"),
        experience: xp,
        level: level_from_xp(xp),
        buybacks: he.map(|h| ctx.i(h, "m_uiNumBuyBacks")).unwrap_or(0),
        ..Default::default()
    };
    t.gold_earned =
        t.gold_hero_kills + t.gold_assists + t.gold_buildings + t.gold_creeps + t.gold_neutrals;
    t.net_worth = t.gold_spent + t.gold.floor() as i64;
    t
}

/// Format a 128-bit account id as a UUID string.
pub fn format_uuid(w: u128) -> String {
    let h = format!("{w:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// Inventory slot classification used by HoN: 48-53 = inventory, 54-? / 63+ = stash,
/// 120+ = innate (raven, teleport stone, ...).
pub fn slot_kind(slot: i64) -> &'static str {
    match slot {
        48..=53 => "inventory",
        54..=119 => "stash",
        s if s >= 120 => "innate",
        _ => "ability",
    }
}

fn seg_dist(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (a.0 + t * dx, a.1 + t * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

/// Classify a world position into a lane using the two bases: mid is the base-to-base
/// diagonal, top/bottom are the two L-shaped paths along the map edges.
pub fn classify_lane(
    p: (f64, f64),
    legion: (f64, f64),
    hell: (f64, f64),
    own_team: &str,
) -> Option<&'static str> {
    let own = if own_team == "legion" { legion } else { hell };
    let span = ((hell.0 - legion.0).powi(2) + (hell.1 - legion.1).powi(2)).sqrt();
    if ((p.0 - own.0).powi(2) + (p.1 - own.1).powi(2)).sqrt() < span * 0.18 {
        return None;
    } // in base / fountain
    let c_top = (legion.0, hell.1);
    let c_bot = (hell.0, legion.1);
    let d_mid = seg_dist(p, legion, hell);
    let d_top = seg_dist(p, legion, c_top).min(seg_dist(p, c_top, hell));
    let d_bot = seg_dist(p, legion, c_bot).min(seg_dist(p, c_bot, hell));
    let (lane, d) = [("mid", d_mid), ("top", d_top), ("bottom", d_bot)]
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    if d > span * 0.12 {
        Some("jungle")
    } else {
        Some(lane)
    }
}
