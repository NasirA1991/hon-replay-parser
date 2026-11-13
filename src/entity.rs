//! Entity snapshot decoding: the core of the replay format.
//!
//! Each frame carries a list of entity records ordered by entity id. A record is
//!
//! ```text
//! 0                                   -> "implicit": advance to the next *active* entity, body follows
//! 1 <id delta> <has type> [type:16] [variant] <active> [variant]  body
//! ```
//!
//! * id delta: `1` => +1, `01 xxxx` => +1+x, `001 x8` => +0x11+x, `000 x15` => +0x111+x
//! * has type with type 0 => delete; type != 0 => create (re-create) with that type
//! * variant (dynamic types only): S2R2 v7 writes `1 bit present [+5 bit key]` before the
//!   active flag; S2R2 v6 writes a fixed per-type number of bits after it.
//! * body: compressed field mask + changed field values (see `schema`).
use std::collections::HashMap;

use serde::Serialize;

use crate::bits::BitReader;
use crate::header::Header;
use crate::schema::{self, Value};

#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    pub id: u32,
    /// Entity type: < 256 static engine type, >= 256 dynamic game type (resolve with
    /// `Header::resource_path`).
    pub type_id: u16,
    /// Index into `Header::typedescs`.
    pub typedesc: usize,
    /// Cosmetic variant / alternate definition selected at creation (alt avatars, ...).
    pub variant: Option<u32>,
    /// The engine's "active" flag; implicit records iterate over active entities.
    pub active: bool,
    pub fields: Vec<Value>,
    pub created_frame: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Change {
    Create {
        id: u32,
        type_id: u16,
        recreate: bool,
    },
    Update {
        id: u32,
        fields: Vec<u16>,
    },
    Delete {
        id: u32,
        type_id: u16,
    },
}

#[derive(Clone, Default)]
pub struct Pool {
    slots: Vec<Option<Entity>>,
    /// Bit set of entity ids with the "active" flag. Most records are implicit ("next
    /// active entity"), so finding the next set bit is the hottest lookup of the decoder.
    active: Vec<u64>,
    count: usize,
}

impl Pool {
    pub fn get(&self, id: u32) -> Option<&Entity> {
        self.slots.get(id as usize).and_then(|e| e.as_ref())
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = &Entity> {
        self.slots.iter().filter_map(|e| e.as_ref())
    }
    fn insert(&mut self, e: Entity) -> Option<Entity> {
        let id = e.id as usize;
        if id >= self.slots.len() {
            self.slots.resize(id + 1, None);
        }
        self.mark(e.id, e.active);
        let old = self.slots[id].replace(e);
        if old.is_none() {
            self.count += 1;
        }
        old
    }
    fn remove(&mut self, id: u32) -> Option<Entity> {
        let e = self.slots.get_mut(id as usize)?.take()?;
        self.mark(id, false);
        self.count -= 1;
        Some(e)
    }
    pub fn set_active(&mut self, id: u32, on: bool) {
        if let Some(Some(e)) = self.slots.get_mut(id as usize) {
            e.active = on;
        }
        self.mark(id, on);
    }
    fn mark(&mut self, id: u32, on: bool) {
        let (word, bit) = ((id >> 6) as usize, 1u64 << (id & 63));
        if on {
            if word >= self.active.len() {
                self.active.resize(word + 1, 0);
            }
            self.active[word] |= bit;
        } else if let Some(w) = self.active.get_mut(word) {
            *w &= !bit;
        }
    }
    /// Smallest active entity id greater than `after`.
    pub fn next_active(&self, after: u32) -> Option<u32> {
        let from = after.checked_add(1)?;
        let mut word = (from >> 6) as usize;
        let mut bits = *self.active.get(word)? & (u64::MAX << (from & 63));
        loop {
            if bits != 0 {
                return Some((word as u32) << 6 | bits.trailing_zeros());
            }
            word += 1;
            bits = *self.active.get(word)?;
        }
    }
}

/// Decoder state that persists across frames.
#[derive(Clone)]
pub struct Decoder {
    pub format_version: u32,
    pub pool: Pool,
    /// v6: number of variant bits per dynamic entity type (learned on first create).
    pub variant_bits: HashMap<u16, u8>,
    defaults: Vec<Vec<Value>>,
    fields: Vec<std::sync::Arc<Vec<crate::header::FieldDesc>>>,
    static_td: HashMap<u16, usize>,
    dynamic_td: HashMap<u16, usize>,
    type_map: HashMap<u16, u16>,
    predicted_variant_bits: HashMap<u16, u8>,
    /// Print every record to stderr (debugging).
    pub trace: bool,
    /// Stop decoding (successfully) at this bit (debugging).
    pub stop_at_bit: Option<usize>,
    /// id cursor when decoding stopped.
    pub last_cursor: u32,
}

struct Rec {
    implicit: bool,
    delta: u32,
    type_id: Option<u16>,
    active: bool,
    variant: Option<u32>,
    end: usize,
}

#[derive(Debug)]
pub struct DecodeFailure {
    pub bit: usize,
    pub message: String,
}

impl Decoder {
    pub fn new(header: &Header) -> Self {
        let defaults = header
            .typedescs
            .iter()
            .map(schema::decode_defaults)
            .collect();
        let mut static_td = HashMap::new();
        let mut dynamic_td = HashMap::new();
        for (i, td) in header.typedescs.iter().enumerate() {
            if td.table == 0 {
                static_td.insert(td.type_id, i);
            } else {
                dynamic_td.insert(td.type_id, i);
            }
        }
        let type_map = header.type_map.iter().map(|(k, v)| (*k, *v)).collect();
        let fields = header
            .typedescs
            .iter()
            .map(|t| std::sync::Arc::new(t.fields.clone()))
            .collect();
        Decoder {
            format_version: header.format_version,
            pool: Pool::default(),
            variant_bits: HashMap::new(),
            defaults,
            fields,
            static_td,
            dynamic_td,
            type_map,
            predicted_variant_bits: predict_variant_bits(header),
            trace: false,
            stop_at_bit: None,
            last_cursor: 0,
        }
    }

    pub fn typedesc_of(&self, type_id: u16) -> Option<usize> {
        if type_id < 256 {
            self.static_td.get(&type_id).copied()
        } else {
            self.dynamic_td.get(self.type_map.get(&type_id)?).copied()
        }
    }

    fn read_rec(&self, b: &mut BitReader, v6_bits: Option<u8>) -> Result<Rec, Option<u16>> {
        if !b.bit() {
            return Ok(Rec {
                implicit: true,
                delta: 0,
                type_id: None,
                active: true,
                variant: None,
                end: b.pos(),
            });
        }
        let delta = if b.bit() {
            1
        } else if b.bit() {
            1 + b.read(4) as u32
        } else if b.bit() {
            0x11 + b.read(8) as u32
        } else {
            0x111 + b.read(15) as u32
        };
        let type_id = if b.bit() {
            Some(b.read(16) as u16)
        } else {
            None
        };
        let dynamic = matches!(type_id, Some(t) if t >= 256);
        let mut variant = None;
        if dynamic && self.format_version >= 7 && b.bit() {
            variant = Some(b.read(5) as u32);
        }
        if dynamic && self.format_version == 6 {
            let t = type_id.unwrap();
            let n = match v6_bits.or_else(|| self.variant_bits.get(&t).copied()) {
                Some(n) => n,
                None => return Err(Some(t)),
            };
            variant = Some(b.read(n as usize) as u32);
        }
        let active = b.bit();
        Ok(Rec {
            implicit: false,
            delta,
            type_id,
            active,
            variant,
            end: b.pos(),
        })
    }

    /// Decode the entity section of one frame starting at bit `start`.
    pub fn decode(
        &mut self,
        body: &[u8],
        start: usize,
        frame: u64,
        changes: &mut Vec<Change>,
    ) -> Result<(), DecodeFailure> {
        self.decode_from(body, start, 0, frame, changes)
    }

    pub fn decode_from(
        &mut self,
        body: &[u8],
        start: usize,
        start_id: u32,
        frame: u64,
        changes: &mut Vec<Change>,
    ) -> Result<(), DecodeFailure> {
        let mut pos = start;
        let mut id: u32 = start_id;
        let mut mask: Vec<u16> = Vec::with_capacity(160);
        let total = body.len() * 8;
        loop {
            if is_end(body, pos) || self.stop_at_bit.is_some_and(|s| pos >= s) {
                self.last_cursor = id;
                return Ok(());
            }
            let mut b = BitReader::at(body, pos);
            let rec = match self.read_rec(&mut b, None) {
                Ok(r) => r,
                Err(Some(t)) => return self.branch_variant_bits(body, pos, id, frame, changes, t),
                Err(None) => unreachable!(),
            };
            let rec_start = pos;
            if self.trace {
                eprintln!(
                    "  rec@{pos} implicit={} delta={} type={:?} active={} variant={:?} hdr_end={}",
                    rec.implicit, rec.delta, rec.type_id, rec.active, rec.variant, rec.end
                );
            }
            if rec.implicit {
                id = match self.pool.next_active(id) {
                    Some(n) => n,
                    None => {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: "implicit record with no further active entity".into(),
                        });
                    }
                };
            } else {
                id = id.checked_add(rec.delta).ok_or(DecodeFailure {
                    bit: rec_start,
                    message: "id overflow".into(),
                })?;
            }
            if self.trace {
                eprintln!(
                    "    -> id {id} existing={:?}",
                    self.pool
                        .get(id)
                        .map(|e| (e.type_id, e.typedesc, e.fields.len()))
                );
            }
            match rec.type_id {
                Some(0) => {
                    let Some(e) = self.pool.remove(id) else {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: format!("delete of unknown entity {id}"),
                        });
                    };
                    changes.push(Change::Delete {
                        id,
                        type_id: e.type_id,
                    });
                    pos = b.pos();
                }
                Some(t) => {
                    let Some(tdi) = self.typedesc_of(t) else {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: format!("unknown entity type {t}"),
                        });
                    };
                    let nfields = self.defaults[tdi].len();
                    let fields_desc = self.desc_fields(tdi);
                    if !schema::read_mask_set(&mut b, nfields, &mut mask) {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: "invalid field mask on create".into(),
                        });
                    }
                    let mut fields = self.defaults[tdi].clone();
                    for &i in &mask {
                        fields[i as usize] = schema::read_field(&mut b, &fields_desc[i as usize]);
                    }
                    if b.pos() > total {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: "create body past frame end".into(),
                        });
                    }
                    let recreate = self
                        .pool
                        .insert(Entity {
                            id,
                            type_id: t,
                            typedesc: tdi,
                            variant: rec.variant,
                            active: rec.active,
                            fields,
                            created_frame: frame,
                        })
                        .is_some();
                    changes.push(Change::Create {
                        id,
                        type_id: t,
                        recreate,
                    });
                    pos = b.pos();
                }
                None => {
                    let Some(e) = self.pool.get(id) else {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: format!("update of unknown entity {id}"),
                        });
                    };
                    let tdi = e.typedesc;
                    let fields_desc = self.desc_fields(tdi);
                    if !schema::read_mask_set(&mut b, fields_desc.len(), &mut mask) {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: "invalid field mask on update".into(),
                        });
                    }
                    // A record that runs past the frame end is a desync and aborts the
                    // replay, so the fields can be applied as they are read.
                    let changed = mask.clone();
                    if let Some(Some(e)) = self.pool.slots.get_mut(id as usize) {
                        for &i in &mask {
                            let i = i as usize;
                            match (&mut e.fields[i], schema::read_field(&mut b, &fields_desc[i])) {
                                (Value::DeltaVec(a, _), Value::DeltaVec(d, _)) => {
                                    for k in 0..3 {
                                        a[k] = a[k].wrapping_add(d[k]);
                                    }
                                }
                                (slot, v) => *slot = v,
                            }
                        }
                    }
                    if b.pos() > total {
                        return Err(DecodeFailure {
                            bit: rec_start,
                            message: "update body past frame end".into(),
                        });
                    }
                    if !rec.implicit {
                        self.pool.set_active(id, rec.active);
                    }
                    changes.push(Change::Update {
                        id,
                        fields: changed,
                    });
                    pos = b.pos();
                }
            }
        }
    }

    fn desc_fields(&self, tdi: usize) -> std::sync::Arc<Vec<crate::header::FieldDesc>> {
        self.fields[tdi].clone()
    }

    /// S2R2 v6 only: the number of variant bits of a dynamic type is not stored in the
    /// replay. Try candidates (predicted first) and keep the one that lets the rest of the
    /// frame decode to its exact end.
    fn branch_variant_bits(
        &mut self,
        body: &[u8],
        pos: usize,
        id: u32,
        frame: u64,
        changes: &mut Vec<Change>,
        t: u16,
    ) -> Result<(), DecodeFailure> {
        let mut order: Vec<u8> = Vec::new();
        if let Some(p) = self.predicted_variant_bits.get(&t) {
            order.push(*p);
        }
        for n in 0..=4 {
            if !order.contains(&n) {
                order.push(n);
            }
        }
        let mut last = None;
        for n in order {
            let mut trial = self.clone();
            trial.variant_bits.insert(t, n);
            let mut c2 = changes.clone();
            match trial.decode_from(body, pos, id, frame, &mut c2) {
                Ok(()) => {
                    *self = trial;
                    *changes = c2;
                    return Ok(());
                }
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or(DecodeFailure {
            bit: pos,
            message: format!("no variant width fits type {t}"),
        }))
    }
}

fn is_end(body: &[u8], pos: usize) -> bool {
    let total = body.len() * 8;
    if pos >= total {
        return true;
    }
    if total - pos >= 8 {
        return false;
    }
    let mut b = BitReader::at(body, pos);
    (pos..total).all(|_| !b.bit())
}

/// Predict v6 variant widths from the resource paths: a dynamic type defined under
/// `.../<name>/base/<rest>` has as many variants as there are sibling directories that
/// define the same `<rest>` file; the width is ceil(log2(variants)).
fn predict_variant_bits(header: &Header) -> HashMap<u16, u8> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut groups: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    if let Some(ss3) = header.string_sets.get(3) {
        for p in ss3.values() {
            let parts: Vec<&str> = p.split('/').collect();
            for i in 1..parts.len().saturating_sub(1) {
                groups
                    .entry((parts[..i].join("/"), parts[i + 1..].join("/")))
                    .or_default()
                    .insert(parts[i].to_string());
            }
        }
    }
    let mut out = HashMap::new();
    for (t, _) in header.type_map.iter() {
        let Some(path) = header.resource_path(*t) else {
            continue;
        };
        let parts: Vec<&str> = path.split('/').collect();
        let n = match parts.iter().position(|c| *c == "base") {
            Some(i) => groups
                .get(&(parts[..i].join("/"), parts[i + 1..].join("/")))
                .map(|s| s.len())
                .unwrap_or(1),
            None => 1,
        };
        let bits = if n <= 1 {
            0
        } else {
            (usize::BITS - (n - 1).leading_zeros()) as u8
        };
        out.insert(*t, bits);
    }
    out
}
