//! The S2R2 header that precedes the frame stream.
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDesc {
    pub name: String,
    /// Wire type code (see `schema::FieldKind`).
    pub type_code: u8,
    /// Explicit bit width (0 = use the type's default width).
    pub bits: u32,
    /// Second descriptor argument (range/offset hint; preserved verbatim).
    pub arg: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeDesc {
    /// Which of the two typedesc tables this came from: 0 = static engine types
    /// (Player, TeamInfo, ...), 1 = base classes of dynamic game entities (Hero, Item, ...).
    pub table: u8,
    pub type_id: u16,
    pub name: String,
    pub flags: u32,
    pub fields: Vec<FieldDesc>,
    /// Serialized default snapshot (entity header + body) for this type.
    pub default_snapshot: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub format_version: u32,
    pub server_version: String,
    pub typedescs: Vec<TypeDesc>,
    /// Dynamic entity type id (>= 256) -> table-1 base typedesc id.
    pub type_map: BTreeMap<u16, u16>,
    pub map_name: String,
    /// String tables. Index 0: server info, 1: game cvars, 2: dynamic strings (player
    /// names, ...), 3: resource paths keyed by resource id. Updated by frames.
    pub string_sets: Vec<BTreeMap<String, String>>,
    pub state_blocks: Vec<Vec<u8>>,
    /// Dynamic entity type id -> resource id (key into `string_sets[3]`).
    pub entity_resources: BTreeMap<u16, u32>,
    /// Byte offset of the first frame in `replaydata`.
    pub header_len: usize,
}

struct Cursor<'a> {
    d: &'a [u8],
    p: usize,
}
impl<'a> Cursor<'a> {
    fn need(&self, n: usize) -> Result<()> {
        if self.p + n > self.d.len() {
            Err(Error::malformed(self.p, "unexpected end of header"))
        } else {
            Ok(())
        }
    }
    fn u8(&mut self) -> Result<u8> {
        self.need(1)?;
        let v = self.d[self.p];
        self.p += 1;
        Ok(v)
    }
    fn u16(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.d[self.p], self.d[self.p + 1]]);
        self.p += 2;
        Ok(v)
    }
    fn u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = u32::from_le_bytes(self.d[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        Ok(v)
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let v = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(v)
    }
    fn cstr(&mut self) -> Result<&'a [u8]> {
        let end = self.d[self.p..]
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| Error::malformed(self.p, "unterminated string"))?;
        let v = &self.d[self.p..self.p + end];
        self.p += end + 1;
        Ok(v)
    }
}

pub(crate) fn latin1(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => b.iter().map(|c| *c as char).collect(),
    }
}

/// String sets are serialized as `key 0xFF value 0xFF key 0xFF value ...`.
pub(crate) fn parse_string_set(raw: &[u8]) -> BTreeMap<String, String> {
    let parts: Vec<&[u8]> = raw.split(|b| *b == 0xff).collect();
    let mut out = BTreeMap::new();
    for pair in parts.chunks(2) {
        if pair.len() == 2 {
            out.insert(latin1(pair[0]), latin1(pair[1]));
        }
    }
    out
}

impl Header {
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(Error::malformed(0, "replaydata too short"));
        }
        if &data[0..4] != b"S2R2" {
            return Err(Error::BadMagic(data[0..4].try_into().unwrap()));
        }
        let mut c = Cursor { d: data, p: 4 };
        let format_version = c.u32()?;
        if !(3..=7).contains(&format_version) {
            return Err(Error::UnsupportedVersion(format_version));
        }
        let sv = c.bytes(4)?;
        let server_version = format!("{}.{}.{}.{}", sv[0], sv[1], sv[2], sv[3]);
        let mut typedescs = Vec::new();
        for table in 0..2u8 {
            let count = c.u32()?;
            if count > 4096 {
                return Err(Error::malformed(c.p, "implausible typedesc count"));
            }
            for _ in 0..count {
                let type_id = c.u16()?;
                let name = latin1(c.cstr()?);
                let flags = c.u32()?;
                let n = c.u8()?;
                let mut fields = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    let fname = latin1(c.cstr()?);
                    let type_code = c.u8()?;
                    let bits = c.u32()?;
                    let arg = c.u32()?;
                    fields.push(FieldDesc {
                        name: fname,
                        type_code,
                        bits,
                        arg,
                    });
                }
                let len = c.u16()? as usize;
                let default_snapshot = c.bytes(len)?.to_vec();
                typedescs.push(TypeDesc {
                    table,
                    type_id,
                    name,
                    flags,
                    fields,
                    default_snapshot,
                });
            }
        }
        let tm_count = c.u32()?;
        let mut type_map = BTreeMap::new();
        for _ in 0..tm_count {
            let k = c.u16()?;
            let v = c.u16()?;
            type_map.insert(k, v);
        }
        let map_name = latin1(c.cstr()?);
        let ss_count = if format_version > 3 { c.u32()? } else { 5 };
        if !(1..=64).contains(&ss_count) {
            return Err(Error::malformed(c.p, "implausible string-set count"));
        }
        let mut string_sets = Vec::new();
        for _ in 0..ss_count - 1 {
            string_sets.push(parse_string_set(c.cstr()?));
        }
        let sb_count = if format_version > 3 { c.u32()? } else { 3 };
        if !(1..=64).contains(&sb_count) {
            return Err(Error::malformed(c.p, "implausible state-block count"));
        }
        let mut state_blocks = Vec::new();
        for _ in 0..sb_count - 1 {
            let n = c.u32()? as usize;
            state_blocks.push(c.bytes(n)?.to_vec());
        }
        let mut entity_resources = BTreeMap::new();
        if let Some(sb1) = state_blocks.get(1) {
            for ch in sb1.chunks_exact(6) {
                entity_resources.insert(
                    u16::from_le_bytes([ch[0], ch[1]]),
                    u32::from_le_bytes([ch[2], ch[3], ch[4], ch[5]]),
                );
            }
        }
        Ok(Header {
            format_version,
            server_version,
            typedescs,
            type_map,
            map_name,
            string_sets,
            state_blocks,
            entity_resources,
            header_len: c.p,
        })
    }

    /// Number of 32-entry groups in the per-frame state mask (derived from state block 0).
    pub fn state_groups(&self) -> usize {
        self.state_blocks
            .first()
            .map(|b| (b.len() / 4 + 31) / 32)
            .unwrap_or(0)
    }

    pub fn string(&self, set: usize, key: u32) -> Option<&str> {
        self.string_sets
            .get(set)?
            .get(&key.to_string())
            .map(|s| s.as_str())
    }

    /// Server cvar from string set 0 (e.g. `svr_gameFPS`).
    pub fn server_var(&self, name: &str) -> Option<&str> {
        self.string_sets.first()?.get(name).map(|s| s.as_str())
    }

    /// Resource path (e.g. `064/heroes/parasite/base/hero.entity`) of a dynamic entity type.
    pub fn resource_path(&self, entity_type: u16) -> Option<&str> {
        let res = *self.entity_resources.get(&entity_type)?;
        self.string(3, res)
    }
}
