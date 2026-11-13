//! Count created entities by base type and resource path.
use honreplay::*;
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = Replay::open(&a[1]).unwrap();
    let mut fr = r.frames();
    let mut c: BTreeMap<(String, String), u32> = BTreeMap::new();
    while let Some(f) = fr.next_frame() {
        let f = f.unwrap();
        for ch in &f.changes {
            if let Change::Create { id, type_id, .. } = ch {
                let e = fr.pool().get(*id).unwrap();
                *c.entry((
                    r.header.typedescs[e.typedesc].name.clone(),
                    r.header.resource_path(*type_id).unwrap_or("-").to_string(),
                ))
                .or_default() += 1;
            }
        }
    }
    for ((t, p), n) in c {
        if a.len() < 3 || t == a[2] {
            println!("{n:6} {t:16} {p}");
        }
    }
}
