//! The water of a map and the ground textures under it:
//! cargo run --example waterkinds -- map.gnd map.rsw
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let gnd = ro_formats::gnd::Gnd::parse(&std::fs::read(&a[1])?)?;
    let rsw = ro_formats::rsw::Rsw::parse(&std::fs::read(&a[2])?)?;
    let Some(w) = rsw.water.or(gnd.water) else {
        println!("no water");
        return Ok(());
    };
    println!("water {w:?}");
    let (lo, hi) = gnd.cubes.iter().flat_map(|c| c.heights).fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(h), b.max(h)));
    println!("heights {lo}..{hi}");
    let mut under = std::collections::BTreeMap::new();
    for c in &gnd.cubes {
        // heights are y-down: a corner below the surface is deeper than the level
        if c.top >= 0 && c.heights.iter().any(|&h| h > w.level) {
            let t = gnd.surfaces[c.top as usize].texture;
            *under.entry(gnd.textures.get(t as usize).cloned().unwrap_or_default()).or_insert(0) += 1;
        }
    }
    let mut v: Vec<_> = under.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (t, n) in v.iter().take(8) {
        println!("{n:6} {t}");
    }
    Ok(())
}
