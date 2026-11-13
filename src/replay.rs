//! High level replay reader: header + frame iteration with persistent entity state.
use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::archive::Archive;
use crate::bits::BitReader;
use crate::entity::{Change, Decoder, Pool};
use crate::error::{Error, Result};
use crate::events::{self, GameEvent};
use crate::header::{self, Header};
use crate::replayinfo::ReplayInfo;

pub struct Replay {
    pub info: ReplayInfo,
    pub header: Header,
    pub data: Vec<u8>,
    pub compressed_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Packet {
    pub queue: u8,
    pub receiver: u32,
    #[serde(with = "hexbytes")]
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Command {
    pub kind: u32,
    #[serde(with = "hexbytes")]
    pub data: Vec<u8>,
}

mod hexbytes {
    pub fn serialize<S: serde::Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }
}

/// One server frame.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub index: u64,
    pub sequence: u32,
    pub previous_sequence: u32,
    /// Server clock in milliseconds.
    pub clock: u32,
    pub events: Vec<GameEvent>,
    /// Per-frame state flags (32-bit masks over state block 0 groups).
    pub state_masks: Vec<(u16, u32)>,
    pub changes: Vec<Change>,
    pub packets: Vec<Packet>,
    pub string_updates: Vec<(u32, usize)>,
    pub commands: Vec<Command>,
}

impl Replay {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let a = Archive::open(path)?;
        Self::from_archive(a)
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_archive(Archive::from_bytes(bytes)?)
    }
    fn from_archive(a: Archive) -> Result<Self> {
        let info = if a.replayinfo.is_empty() {
            ReplayInfo::default()
        } else {
            ReplayInfo::parse(&a.replayinfo)?
        };
        let header = Header::parse(&a.replaydata)?;
        Ok(Replay {
            info,
            header,
            data: a.replaydata,
            compressed_bytes: a.compressed_bytes,
        })
    }

    /// Server frames per second (svr_gameFPS, default 20).
    pub fn fps(&self) -> u32 {
        self.header
            .server_var("svr_gameFPS")
            .and_then(|v| v.parse().ok())
            .unwrap_or(20)
    }

    pub fn frames(&self) -> FrameReader<'_> {
        FrameReader {
            replay: self,
            pos: self.header.header_len,
            index: 0,
            decoder: Decoder::new(&self.header),
            string_sets: self.header.string_sets.clone(),
            keep_raw_events: false,
            done: false,
        }
    }
}

pub struct FrameReader<'a> {
    replay: &'a Replay,
    pos: usize,
    index: u64,
    pub decoder: Decoder,
    /// Current string tables (frames replace whole sets).
    pub string_sets: Vec<BTreeMap<String, String>>,
    pub keep_raw_events: bool,
    done: bool,
}

struct Cur<'a> {
    d: &'a [u8],
    p: usize,
}
impl Cur<'_> {
    fn u32(&mut self) -> Result<u32> {
        if self.p + 4 > self.d.len() {
            return Err(Error::malformed(self.p, "truncated frame tail"));
        }
        let v = u32::from_le_bytes(self.d[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        Ok(v)
    }
    fn bytes(&mut self, n: usize) -> Result<&[u8]> {
        if self.p + n > self.d.len() {
            return Err(Error::malformed(self.p, "truncated frame tail"));
        }
        let v = &self.d[self.p..self.p + n];
        self.p += n;
        Ok(v)
    }
}

impl FrameReader<'_> {
    pub fn pool(&self) -> &Pool {
        &self.decoder.pool
    }
    pub fn string(&self, set: usize, key: u32) -> Option<&str> {
        self.string_sets
            .get(set)?
            .get(&key.to_string())
            .map(|s| s.as_str())
    }
    pub fn frame_index(&self) -> u64 {
        self.index
    }

    /// Decode the next frame, updating entity state. `None` at end of stream.
    pub fn next_frame(&mut self) -> Option<Result<Frame>> {
        if self.done {
            return None;
        }
        let data = &self.replay.data;
        if self.pos + 4 > data.len() {
            self.done = true;
            return None;
        }
        let len = u32::from_le_bytes(data[self.pos..self.pos + 4].try_into().unwrap()) as usize;
        if len == 0 || self.pos + 4 + len > data.len() {
            self.done = true;
            return None;
        }
        let body = &data[self.pos + 4..self.pos + 4 + len];
        let frame_index = self.index;
        let r = self.decode_frame(body, frame_index);
        let mut c = Cur {
            d: data,
            p: self.pos + 4 + len,
        };
        let tail = self.read_tail(&mut c);
        self.pos = c.p;
        self.index += 1;
        match (r, tail) {
            (Ok(mut f), Ok((packets, string_updates, commands))) => {
                f.packets = packets;
                f.string_updates = string_updates;
                f.commands = commands;
                Some(Ok(f))
            }
            (Err(e), _) | (_, Err(e)) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }

    fn decode_frame(&mut self, body: &[u8], frame_index: u64) -> Result<Frame> {
        let mut b = BitReader::new(body);
        let sequence = b.read(32) as u32;
        let previous_sequence = if b.bit() {
            sequence.wrapping_sub(1)
        } else if b.bit() {
            sequence.wrapping_add(!(b.read(4) as u32))
        } else {
            b.read(32) as u32
        };
        let clock = b.read(32) as u32;
        let _reserved = b.read(8);
        let n_events = b.read(8) as usize;
        let mut events = Vec::with_capacity(n_events);
        for _ in 0..n_events {
            events.push(events::read_event(&mut b, self.keep_raw_events));
        }
        let groups = self.replay.header.state_groups();
        let mut state_masks = Vec::new();
        let mut gm = Vec::new();
        crate::schema::read_mask(&mut b, groups, &mut gm);
        for (g, on) in gm.iter().enumerate() {
            if *on {
                state_masks.push((g as u16, b.read(32) as u32));
            }
        }
        let mut changes = Vec::new();
        self.decoder
            .decode(body, b.pos(), frame_index, &mut changes)
            .map_err(|e| Error::Desync {
                frame: frame_index,
                bit: e.bit,
                message: e.message,
            })?;
        Ok(Frame {
            index: frame_index,
            sequence,
            previous_sequence,
            clock,
            events,
            state_masks,
            changes,
            packets: Vec::new(),
            string_updates: Vec::new(),
            commands: Vec::new(),
        })
    }

    #[allow(clippy::type_complexity)]
    fn read_tail(&mut self, c: &mut Cur) -> Result<(Vec<Packet>, Vec<(u32, usize)>, Vec<Command>)> {
        let mut packets = Vec::new();
        for queue in 0..2u8 {
            let n = c.u32()?;
            if n > 1_000_000 {
                return Err(Error::malformed(c.p, "implausible packet count"));
            }
            for _ in 0..n {
                let receiver = c.u32()?;
                let len = c.u32()? as usize;
                packets.push(Packet {
                    queue,
                    receiver,
                    data: c.bytes(len)?.to_vec(),
                });
            }
        }
        let mut string_updates = Vec::new();
        let n = c.u32()?;
        for _ in 0..n {
            let set = c.u32()?;
            let start = c.p;
            let end = c.d[start..]
                .iter()
                .position(|b| *b == 0)
                .ok_or_else(|| Error::malformed(start, "unterminated string update"))?;
            let parsed = header::parse_string_set(&c.d[start..start + end]);
            c.p = start + end + 1;
            if set >= 1 && (set as usize) <= self.string_sets.len() {
                let count = parsed.len();
                self.string_sets[set as usize - 1] = parsed;
                string_updates.push((set, count));
            }
        }
        let mut commands = Vec::new();
        if self.replay.header.format_version > 3 {
            let n = c.u32()?;
            for _ in 0..n {
                let kind = c.u32()?;
                let len = c.u32()? as usize;
                commands.push(Command {
                    kind,
                    data: c.bytes(len)?.to_vec(),
                });
            }
        }
        Ok((packets, string_updates, commands))
    }
}
