//! Print GameInfo phase changes and the final state of Player and Hero entities.
use honreplay::*;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = Replay::open(&a[1]).unwrap();
    let td = |e: &Entity| &r.header.typedescs[e.typedesc];
    let fidx = |tdi: usize, name: &str| {
        r.header.typedescs[tdi]
            .fields
            .iter()
            .position(|f| f.name == name)
    };
    let mut fr = r.frames();
    let mut last_phase = -1i64;
    let mut n = 0u64;
    let mut last_clock = 0;
    while let Some(f) = fr.next_frame() {
        let f = f.unwrap();
        n += 1;
        last_clock = f.clock;
        for e in fr.pool().iter() {
            if td(e).name == "GameInfo" {
                let p = e.fields[fidx(e.typedesc, "m_uiGamePhase").unwrap()]
                    .as_i64()
                    .unwrap();
                if p != last_phase {
                    let st = e.fields[fidx(e.typedesc, "m_uiPhaseStartTime").unwrap()]
                        .as_i64()
                        .unwrap();
                    let du = e.fields[fidx(e.typedesc, "m_uiPhaseDuration").unwrap()]
                        .as_i64()
                        .unwrap();
                    println!(
                        "frame {} clock {} phase {} start {} dur {} matchlen {:?}",
                        f.index,
                        f.clock,
                        p,
                        st,
                        du,
                        e.fields[fidx(e.typedesc, "m_uiMatchLength").unwrap()]
                    );
                    last_phase = p;
                }
            }
        }
    }
    println!("frames {n} last clock {last_clock}");
    // final state dump of Player + GameStats
    for e in fr.pool().iter() {
        let t = td(e);
        if t.name == "Player" || t.name == "GameStats" || t.name == "Hero" {
            let mut s = format!(
                "{} id={} type={} {:?} variant={:?}:",
                t.name,
                e.id,
                e.type_id,
                r.header.resource_path(e.type_id),
                e.variant
            );
            for (i, f) in t.fields.iter().enumerate() {
                let v = &e.fields[i];
                let zero = matches!(v, Value::Int(0)) || matches!(v, Value::Float(x) if *x == 0.0);
                if !zero
                    && !f.name.contains("Recent")
                    && !f.name.contains("Fav")
                    && !f.name.contains("Account")
                {
                    s += &format!(" {}={}", f.name, serde_json::to_string(v).unwrap());
                }
            }
            println!("{s}\n");
        }
    }
}
