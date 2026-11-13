//! Histogram of packet message ids and receivers in the frame tails.
use honreplay::*;
use std::collections::BTreeMap;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = Replay::open(&a[1]).unwrap();
    let mut fr = r.frames();
    let mut cmd: BTreeMap<(u32, usize), u64> = BTreeMap::new();
    let mut pk: BTreeMap<(u8, u8, u32), u64> = BTreeMap::new();
    let mut samples: BTreeMap<u8, Vec<String>> = BTreeMap::new();
    while let Some(f) = fr.next_frame() {
        let f = f.unwrap();
        for c in &f.commands {
            *cmd.entry((c.kind, c.data.len())).or_default() += 1;
        }
        for p in &f.packets {
            let b0 = *p.data.first().unwrap_or(&255);
            *pk.entry((p.queue, b0, p.receiver)).or_default() += 1;
            let s = samples.entry(b0).or_default();
            if s.len() < 3 {
                s.push(
                    p.data
                        .iter()
                        .take(40)
                        .map(|x| format!("{x:02x}"))
                        .collect::<String>(),
                );
            }
        }
    }
    println!("commands {:?}", cmd.iter().take(30).collect::<Vec<_>>());
    let mut byb: BTreeMap<(u8, u8), u64> = BTreeMap::new();
    for ((q, b, _), n) in &pk {
        *byb.entry((*q, *b)).or_default() += n;
    }
    println!("packets by (queue, first byte): {:?}", byb);
    let mut recv: BTreeMap<u32, u64> = BTreeMap::new();
    for ((_, _, r), n) in &pk {
        *recv.entry(*r).or_default() += n;
    }
    println!("receivers {:?}", recv);
    for (k, v) in samples {
        println!("{k:02x}: {:?}", v);
    }
}
