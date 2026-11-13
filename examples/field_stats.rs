//! Count how often each field of one entity type is updated: cargo run --release --example field_stats -- <replay> Hero
use honreplay::*;
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = Replay::open(&a[1]).unwrap();
    let want = &a[2];
    let mut fr = r.frames();
    let mut c: BTreeMap<String, u64> = BTreeMap::new();
    let mut sample: BTreeMap<String, Vec<String>> = BTreeMap::new();
    while let Some(f) = fr.next_frame() {
        let f = f.unwrap();
        for ch in &f.changes {
            if let Change::Update { id, fields } = ch {
                let e = fr.pool().get(*id).unwrap();
                let td = &r.header.typedescs[e.typedesc];
                if &td.name == want {
                    for fi in fields {
                        let n = td.fields[*fi as usize].name.clone();
                        *c.entry(n.clone()).or_default() += 1;
                        let s = sample.entry(n).or_default();
                        if s.len() < 6 {
                            s.push(serde_json::to_string(&e.fields[*fi as usize]).unwrap());
                        }
                    }
                }
            }
        }
    }
    for (k, v) in c {
        println!("{v:8} {k:40} {:?}", sample[&k]);
    }
}
