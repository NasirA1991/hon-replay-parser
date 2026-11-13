//! Field encodings of entity snapshots.
//!
//! Every networked entity type is described by a typedesc in the replay header: an
//! ordered list of fields, each with a wire type code and an optional bit width.
//! An entity record's body is a compressed "which fields changed" mask followed by the
//! changed field values in declaration order.
use serde::{Serialize, Serializer};

use crate::bits::BitReader;
use crate::header::{FieldDesc, TypeDesc};

/// Decoded field value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Integer fields (type codes 0,1,2,4,6,7,9,10,11,12 and unknown codes).
    Int(i64),
    /// IEEE float (type 3) or byte fraction (type 13, `raw / 255`).
    Float(f32),
    /// Angle stored as a byte (type 8), in degrees.
    Angle(f32),
    /// Three 32-bit floats (type 5).
    Vec3f([f32; 3]),
    /// Three unsigned 16-bit world coordinates (type 14).
    Pos3([u16; 3]),
    /// Delta-coded integer vector (types 15/16/17: 1, 2 or 3 components). Absolute on
    /// create, added component-wise on update.
    DeltaVec([i32; 3], u8),
    /// Wide raw value (type 18, 128 bits; account ids and guids).
    Wide(u128),
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Float(f) | Value::Angle(f) => Some(*f as i64),
            Value::Wide(w) => Some(*w as i64),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(v) => Some(*v as f64),
            Value::Float(f) | Value::Angle(f) => Some(*f as f64),
            _ => None,
        }
    }
    /// x/y/z of any positional value.
    pub fn as_xyz(&self) -> Option<[f32; 3]> {
        match self {
            Value::Vec3f(v) => Some(*v),
            Value::Pos3(p) => Some([p[0] as f32, p[1] as f32, p[2] as f32]),
            Value::DeltaVec(v, n) => Some([
                v[0] as f32,
                if *n > 1 { v[1] as f32 } else { 0.0 },
                if *n > 2 { v[2] as f32 } else { 0.0 },
            ]),
            _ => None,
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Int(v) => s.serialize_i64(*v),
            Value::Float(f) | Value::Angle(f) => s.serialize_f32(*f),
            Value::Vec3f(v) => v.serialize(s),
            Value::Pos3(p) => p.serialize(s),
            Value::DeltaVec(v, n) => v[..*n as usize].serialize(s),
            Value::Wide(w) => s.serialize_str(&format!("{w:032x}")),
        }
    }
}

/// Default bit width of a type code when the field does not declare one.
pub fn default_bits(type_code: u8) -> usize {
    match type_code {
        0 | 8 | 13 => 8,
        1 | 6 | 7 | 9 | 10 | 11 | 12 | 15 => 16,
        2 | 3 | 16 => 32,
        4 => 64,
        5 => 96,
        14 | 17 => 48,
        18 => 128,
        _ => 0,
    }
}

pub fn is_delta_vec(type_code: u8) -> bool {
    (15..=17).contains(&type_code)
}

/// Read one field value.
pub fn read_field(b: &mut BitReader, f: &FieldDesc) -> Value {
    let tc = f.type_code;
    if is_delta_vec(tc) {
        let n = (tc - 14) as usize;
        let mut v = [0i32; 3];
        for slot in v.iter_mut().take(n) {
            let raw = if b.bit() {
                b.read(5)
            } else if b.bit() {
                b.read(6) + 32
            } else {
                b.read(15)
            } as i64;
            *slot = (if raw & 1 != 0 {
                (-1 - raw) >> 1
            } else {
                raw >> 1
            }) as i32;
        }
        return Value::DeltaVec(v, n as u8);
    }
    let width = if f.bits != 0 {
        f.bits as usize
    } else {
        default_bits(tc)
    };
    match (tc, width) {
        (3, 32) => Value::Float(f32::from_bits(b.read(32) as u32)),
        (13, w) => Value::Float(b.read(w.min(64)) as f32 / 255.0),
        (8, w) => Value::Angle(b.read(w.min(64)) as f32 * 360.0 / (1u64 << w.min(63)) as f32),
        (5, 96) => Value::Vec3f([
            f32::from_bits(b.read(32) as u32),
            f32::from_bits(b.read(32) as u32),
            f32::from_bits(b.read(32) as u32),
        ]),
        (14, 48) => Value::Pos3([b.read(16) as u16, b.read(16) as u16, b.read(16) as u16]),
        (_, w) if w > 64 => Value::Wide(b.read_u128(w.min(128))),
        (_, w) => Value::Int(b.read(w) as i64),
    }
}

/// Read the K2 compressed field mask ("unpack enabled") as the ascending list of set
/// indices. Returns false when the mask marks an index past `count`, which means the
/// stream is misaligned.
///
/// For more than 8 entries the mask is a binary tree over `2^ceil(log2(count))` leaves,
/// written level by level: for every set node, `0` = only the left child, `1 0` = only
/// the right child, `1 1` = both. Only set nodes are visited, so the cost follows the
/// number of changed fields, not the size of the type.
pub fn read_mask_set(b: &mut BitReader, count: usize, out: &mut Vec<u16>) -> bool {
    out.clear();
    if count <= 8 {
        for i in 0..count {
            if b.bit() {
                out.push(i as u16);
            }
        }
        return true;
    }
    if !b.bit() {
        return true;
    }
    let size = count.next_power_of_two();
    // `out` holds the set nodes of the current level, in ascending order.
    let mut level: Vec<u32> = Vec::new();
    let mut stack_a = [0u32; 256];
    let mut stack_b = [0u32; 256];
    let (mut cur, mut next): (&mut [u32], &mut [u32]) = if size <= 256 {
        (&mut stack_a[..], &mut stack_b[..])
    } else {
        level.resize(size * 2, 0);
        level.split_at_mut(size)
    };
    cur[0] = 1;
    let mut n = 1usize;
    let mut width = 1usize;
    while width < size {
        let mut m = 0usize;
        for &node in &cur[..n] {
            let left = node * 2;
            if b.bit() {
                if b.bit() {
                    next[m] = left;
                    m += 1;
                }
                next[m] = left + 1;
            } else {
                next[m] = left;
            }
            m += 1;
        }
        std::mem::swap(&mut cur, &mut next);
        n = m;
        width *= 2;
    }
    let mut valid = true;
    for &leaf in &cur[..n] {
        let idx = leaf as usize - size;
        if idx < count {
            out.push(idx as u16);
        } else {
            valid = false;
        }
    }
    valid
}

/// `read_mask_set` as one flag per entry.
pub fn read_mask(b: &mut BitReader, count: usize, out: &mut Vec<bool>) -> bool {
    let mut set = Vec::new();
    let valid = read_mask_set(b, count, &mut set);
    out.clear();
    out.resize(count, false);
    for i in set {
        out[i as usize] = true;
    }
    valid
}

/// Decode a full default snapshot (header + body) into field values.
pub fn decode_defaults(td: &TypeDesc) -> Vec<Value> {
    let mut b = BitReader::new(&td.default_snapshot);
    // default snapshots carry an entity header: [1][delta][has type + 16-bit type][flag]
    if b.bit() {
        if !b.bit() {
            if b.bit() {
                b.read(4);
            } else if b.bit() {
                b.read(8);
            } else {
                b.read(15);
            }
        }
        if b.bit() {
            b.read(16);
        }
        b.bit();
    }
    let mut mask = Vec::new();
    read_mask(&mut b, td.fields.len(), &mut mask);
    td.fields
        .iter()
        .zip(mask.iter())
        .map(|(f, on)| {
            if *on {
                read_field(&mut b, f)
            } else {
                zero_value(f)
            }
        })
        .collect()
}

pub fn zero_value(f: &FieldDesc) -> Value {
    match f.type_code {
        15 | 16 | 17 => Value::DeltaVec([0; 3], f.type_code - 14),
        3 | 13 => Value::Float(0.0),
        8 => Value::Angle(0.0),
        5 => Value::Vec3f([0.0; 3]),
        14 => Value::Pos3([0; 3]),
        _ => Value::Int(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original level-by-level reference implementation.
    fn reference(b: &mut BitReader, count: usize) -> (Vec<bool>, bool) {
        let mut out = vec![false; count];
        if count <= 8 {
            for o in out.iter_mut() {
                *o = b.bit();
            }
            return (out, true);
        }
        if !b.bit() {
            return (out, true);
        }
        let size = count.next_power_of_two();
        let mut tree = vec![false; size * 2];
        tree[1] = true;
        let mut level = 1usize;
        while (1usize << level) <= size {
            let start = 1usize << level;
            let mut node = start;
            while node < start * 2 {
                if tree[node >> 1] {
                    if b.bit() {
                        if b.bit() {
                            tree[node] = true;
                        }
                        tree[node + 1] = true;
                    } else {
                        tree[node] = true;
                    }
                }
                node += 2;
            }
            level += 1;
        }
        let valid = !tree[size + count..size * 2].iter().any(|x| *x);
        out.copy_from_slice(&tree[size..size + count]);
        (out, valid)
    }

    #[test]
    fn mask_matches_reference() {
        // deterministic pseudo-random bit streams
        let mut x = 0x9e3779b97f4a7c15u64;
        let mut data = vec![0u8; 4096];
        for count in [1usize, 8, 9, 16, 17, 33, 63, 64, 147, 255, 300, 1000] {
            for round in 0..200 {
                for d in data.iter_mut() {
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    // sparse streams exercise deep trees, dense ones wide trees
                    *d = if round % 2 == 0 { (x & x >> 8 & x >> 16) as u8 } else { x as u8 };
                }
                let (mut a, mut b) = (BitReader::new(&data), BitReader::new(&data));
                let (want, want_valid) = reference(&mut a, count);
                let mut got = Vec::new();
                let got_valid = read_mask(&mut b, count, &mut got);
                assert_eq!((got_valid, a.pos()), (want_valid, b.pos()), "count {count}");
                if want_valid {
                    assert_eq!(got, want, "count {count}");
                }
            }
        }
    }
}
