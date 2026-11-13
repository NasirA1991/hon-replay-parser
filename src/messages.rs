//! Reliable and per-client messages carried in the frame packet queues.
//!
//! A packet is a concatenation of messages separated by a `0x5f` byte. Only some message
//! ids have a known layout, so instead of walking a packet message by message the
//! scanners below look for a known message at the packet start or right after a
//! separator, and validate it (length, terminator, timestamp) before accepting it.
//! See docs/FORMAT.md, section 6.
use std::collections::HashMap;
use std::io::Read;

use serde::Serialize;

const SEP: u8 = 0x5f;

/// Combat popup kinds with a known meaning.
pub mod popup_kind {
    /// Physical damage dealt by `player` to `entity`, after mitigation. Sent only to that
    /// player. (The physical/magic split is inferred: the amount lands in the victim's
    /// matching `*_DAMAGE_RECEIVED` counter in about 95% of unambiguous hits.)
    pub const DAMAGE_PHYSICAL: u8 = 0x10;
    /// Magic damage dealt by `player` to `entity`, after mitigation.
    pub const DAMAGE_MAGIC: u8 = 0x11;
    /// The same hit before mitigation.
    pub const DAMAGE_RAW: u8 = 0x12;
}

/// A floating combat number or status popup (`0x3e` without value, `0x3f` with value).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Popup {
    pub kind: u8,
    /// 0-based client index the popup belongs to (`None` = not tied to a player).
    pub player: Option<u8>,
    /// Entity the popup is shown on.
    pub entity: u16,
    /// Server clock (ms) of the event.
    pub time: u32,
    /// Auxiliary integer (a duration in ms for status popups, otherwise 0).
    pub aux: u32,
    /// Amount (`0x3f` only).
    pub value: Option<f32>,
    pub flag: u8,
}

fn u16le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}
fn u32le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Find every popup in one packet. `clock` is the frame's server clock; a popup's own
/// timestamp never lies in the future and is at most a few seconds old, which rejects
/// byte patterns that only look like a popup.
pub fn scan_popups(data: &[u8], clock: u32, out: &mut Vec<Popup>) {
    let mut i = 0;
    while i < data.len() {
        let at_boundary = i == 0 || data[i - 1] == SEP;
        let len = match data[i] {
            0x3e => 14,
            0x3f => 18,
            _ => 0,
        };
        if at_boundary && len != 0 && i + len <= data.len() {
            let m = &data[i..i + len];
            let terminated = i + len == data.len() || data[i + len] == SEP;
            let time = u32le(&m[5..9]);
            if terminated && time <= clock && clock - time <= 5_000 {
                out.push(Popup {
                    kind: m[1],
                    player: (m[2] != 0xff).then_some(m[2]),
                    entity: u16le(&m[3..5]),
                    time,
                    aux: u32le(&m[9..13]),
                    value: (len == 18).then(|| f32::from_le_bytes([m[13], m[14], m[15], m[16]])),
                    flag: m[len - 1],
                });
                i += len;
                continue;
            }
        }
        i += 1;
    }
}

/// Reassembles the match statistics document the server sends to every client: a
/// zlib-compressed JSON object split into chunks (`ff 29 <u32 len> bytes` ...,
/// closed by `ff 28 <u32 len> <u32 uncompressed len> bytes`). It is sent once when the
/// recording starts (all zero) and again at game over (final statistics).
#[derive(Default)]
pub struct StatsAssembler {
    partial: HashMap<u32, Vec<u8>>,
}

impl StatsAssembler {
    /// Feed one packet; returns a document when a chunk sequence completes in it.
    pub fn feed(&mut self, receiver: u32, data: &[u8]) -> Option<serde_json::Value> {
        let mut done = None;
        let mut i = 0;
        while i + 6 <= data.len() {
            let at_boundary = i == 0 || data[i - 1] == SEP;
            if !(at_boundary && data[i] == 0xff && (data[i + 1] == 0x29 || data[i + 1] == 0x28)) {
                i += 1;
                continue;
            }
            let last = data[i + 1] == 0x28;
            let n = u32le(&data[i + 2..i + 6]) as usize;
            let body = i + if last { 10 } else { 6 };
            let end = body + n;
            if n == 0 || end > data.len() || !(end == data.len() || data[end] == SEP) {
                i += 1;
                continue;
            }
            let buf = self.partial.entry(receiver).or_default();
            buf.extend_from_slice(&data[body..end]);
            if last {
                let expected = u32le(&data[i + 6..i + 10]) as usize;
                let compressed = self.partial.remove(&receiver).unwrap_or_default();
                if let Some(doc) = inflate_json(&compressed, expected) {
                    done = Some(doc);
                }
            }
            i = end;
        }
        done
    }
}

fn inflate_json(compressed: &[u8], expected: usize) -> Option<serde_json::Value> {
    if expected > 16 * 1024 * 1024 {
        return None;
    }
    let mut out = Vec::with_capacity(expected);
    flate2::read::ZlibDecoder::new(compressed)
        .take(expected as u64 + 1)
        .read_to_end(&mut out)
        .ok()?;
    if out.len() != expected {
        return None;
    }
    let doc: serde_json::Value = serde_json::from_slice(&out).ok()?;
    doc.get("MatchId").is_some().then_some(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_layout() {
        // 3f <kind> <player> <entity:2> <time:4> <aux:4> <value:4> <flag>
        let mut p = vec![0x3f, 0x11, 0x07, 0xfd, 0x10];
        p.extend_from_slice(&1000u32.to_le_bytes());
        p.extend_from_slice(&0u32.to_le_bytes());
        p.extend_from_slice(&57.5f32.to_le_bytes());
        p.push(0);
        let mut packet = vec![0x45, 0xa7, 0x10, 0x7b, SEP];
        packet.extend_from_slice(&p);
        let mut out = Vec::new();
        scan_popups(&packet, 1000, &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!((out[0].kind, out[0].player, out[0].entity), (0x11, Some(7), 0x10fd));
        assert_eq!(out[0].value, Some(57.5));
        // a timestamp from the future is not a popup
        out.clear();
        scan_popups(&packet, 999, &mut out);
        assert!(out.is_empty());
    }
}
