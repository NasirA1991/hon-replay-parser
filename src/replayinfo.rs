//! The `replayinfo` XML stored next to the binary stream.
use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InfoPlayer {
    pub name: String,
    /// Account id as stored by the server (numeric on old servers, UUID on HoN Reborn).
    pub account_id: String,
    pub team: u8,
    pub team_index: u8,
    pub client_number: u8,
    pub hero_name: String,
    pub hero_icon: String,
    pub avatar: String,
    /// Every attribute of the `<player>` element, verbatim.
    pub attributes: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplayInfo {
    pub version: String,
    pub match_id: u64,
    pub map_name: String,
    pub map_fancy_name: String,
    pub game_mode: String,
    pub game_options: String,
    pub game_name: String,
    pub server_name: String,
    pub team_size: u8,
    pub arranged_match_type: String,
    pub winner: String,
    pub date: String,
    pub time: String,
    pub match_length_ms: u64,
    pub players: Vec<InfoPlayer>,
    /// Every attribute of the root element, verbatim.
    pub attributes: Vec<(String, String)>,
}

impl ReplayInfo {
    pub fn parse(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        let mut info = ReplayInfo::default();
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Err(e) => return Err(Error::InvalidInfo(e.to_string())),
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let attrs: Vec<(String, String)> = e
                        .attributes()
                        .flatten()
                        .map(|a| {
                            (
                                String::from_utf8_lossy(a.key.as_ref()).into_owned(),
                                String::from_utf8_lossy(a.value.as_ref()).into_owned(),
                            )
                        })
                        .collect();
                    let get = |k: &str| {
                        attrs
                            .iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case(k))
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default()
                    };
                    match e.name().as_ref() {
                        b"replayinfo" => {
                            info.version = get("version");
                            info.match_id = get("matchid").parse().unwrap_or(0);
                            info.map_name = get("mapname");
                            info.map_fancy_name = get("mapfancyname");
                            info.game_mode = get("gamemode");
                            info.game_options = get("gameoptions");
                            info.game_name = get("gamename");
                            info.server_name = get("servername");
                            info.team_size = get("teamsize").parse().unwrap_or(0);
                            info.arranged_match_type = get("arrangedmatchtype");
                            info.winner = get("winner");
                            info.date = get("date");
                            info.time = get("time");
                            info.match_length_ms = get("matchlength").parse().unwrap_or(0);
                            info.attributes = attrs.clone();
                        }
                        b"player" => info.players.push(InfoPlayer {
                            name: get("name"),
                            account_id: get("accountid"),
                            team: get("team").parse().unwrap_or(0),
                            team_index: get("teamindex").parse().unwrap_or(0),
                            client_number: get("clientnum").parse().unwrap_or(0),
                            hero_name: get("heroname"),
                            hero_icon: get("heroicon"),
                            avatar: get("avatar"),
                            attributes: attrs.clone(),
                        }),
                        _ => {}
                    }
                }
                _ => {}
            }
            buf.clear();
        }
        info.players.sort_by_key(|p| p.client_number);
        Ok(info)
    }
}
