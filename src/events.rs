//! CGameEvent records carried at the start of every snapshot. They are the engine's
//! "play this effect/sound here" messages: ability visuals, kill effects, pings, etc.
use serde::Serialize;

use crate::bits::BitReader;

/// Payload size in bytes for each mask bit, in wire order. Bits 16 and 17 are flags
/// without payload. Verified on S2R2 v6 and v7 replays; see docs/FORMAT.md.
const LAYOUT: [(u32, usize); 14] = [
    (1 << 0, 4),
    (1 << 1, 8),
    (1 << 2, 6),
    (1 << 3, 3),
    (1 << 4, 2),
    (1 << 5, 8),
    (1 << 6, 6),
    (1 << 7, 3),
    (1 << 8, 2),
    (1 << 9, 3),
    (1 << 10, 2),
    (1 << 11, 2),
    (1 << 12, 2),
    (1 << 15, 2),
];
const PAYLOADLESS: u32 = (1 << 16) | (1 << 17);

#[derive(Debug, Clone, Serialize)]
pub struct GameEvent {
    pub mask: u32,
    /// Source entity index (mask bit 1).
    pub source_entity: Option<u16>,
    /// World position of the source (bits 1 or 2).
    pub source_pos: Option<[u16; 3]>,
    /// Target entity index (bit 5).
    pub target_entity: Option<u16>,
    /// Target world position (bits 5 or 6).
    pub target_pos: Option<[u16; 3]>,
    /// Effect resource id (bit 9), resolvable through string set 3.
    pub effect: Option<u16>,
    /// Raw payload bytes (after the 4-byte mask).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub raw: Vec<u8>,
    /// True when the mask contains bits whose payload size is not known.
    pub unknown_bits: bool,
}

pub fn payload_len(mask: u32) -> (usize, bool) {
    let mut n = 0;
    let mut rest = mask & !PAYLOADLESS;
    for (bit, len) in LAYOUT {
        if mask & bit != 0 {
            n += len;
            rest &= !bit;
        }
    }
    (n, rest != 0)
}

fn pos3(b: &[u8]) -> [u16; 3] {
    [
        u16::from_le_bytes([b[0], b[1]]),
        u16::from_le_bytes([b[2], b[3]]),
        u16::from_le_bytes([b[4], b[5]]),
    ]
}

pub fn read_event(r: &mut BitReader, keep_raw: bool) -> GameEvent {
    let mask = r.read(32) as u32;
    let (len, unknown_bits) = payload_len(mask);
    let raw = r.read_bytes(len);
    let mut ev = GameEvent {
        mask,
        source_entity: None,
        source_pos: None,
        target_entity: None,
        target_pos: None,
        effect: None,
        raw: Vec::new(),
        unknown_bits,
    };
    let mut p = 0usize;
    for (bit, l) in LAYOUT {
        if mask & bit == 0 {
            continue;
        }
        let s = &raw[p..p + l];
        match bit {
            0x2 => {
                ev.source_entity = Some(u16::from_le_bytes([s[0], s[1]]));
                ev.source_pos = Some(pos3(&s[2..]));
            }
            0x4 => {
                ev.source_pos = Some(pos3(s));
            }
            0x20 => {
                ev.target_entity = Some(u16::from_le_bytes([s[0], s[1]]));
                ev.target_pos = Some(pos3(&s[2..]));
            }
            0x40 => {
                ev.target_pos = Some(pos3(s));
            }
            0x200 => {
                ev.effect = Some(u16::from_le_bytes([s[0], s[1]]));
            }
            _ => {}
        }
        p += l;
    }
    if keep_raw {
        ev.raw = raw;
    }
    ev
}
