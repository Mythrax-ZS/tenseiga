//! Contact sheets of the window finder over a folder of textures:
//! windowmask <folder> <out.png> [found|missed] [first] [count]
//! `found`: textures with windows found, the windows tinted magenta; `missed`: the rest.
//! The index of each tile and its file are printed.
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let mode = a.get(3).map(String::as_str).unwrap_or("found");
    let first: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let count: usize = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(64);
    let mut files: Vec<_> = std::fs::read_dir(&a[1])?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("bmp") || e.eq_ignore_ascii_case("tga"))).collect();
    files.sort();
    const TILE: u32 = 160;
    const COLS: u32 = 8;
    let mut tiles = vec![];
    for p in &files {
        let Ok(img) = ro_formats::image::decode(&std::fs::read(p)?, &p.to_string_lossy()) else { continue };
        let mask = ro_render::window_mask(&img);
        if (mode == "found") != mask.is_some() {
            continue;
        }
        tiles.push((p.file_name().unwrap().to_string_lossy().to_string(), img, mask));
    }
    let tiles: Vec<_> = tiles.into_iter().skip(first).take(count).collect();
    let rows = (tiles.len() as u32).div_ceil(COLS).max(1);
    let mut out = vec![40u8; (COLS * TILE * rows * TILE * 4) as usize];
    for (i, (name, img, mask)) in tiles.iter().enumerate() {
        println!("{:3} {name}", i + first);
        let (tx, ty) = ((i as u32 % COLS) * TILE, (i as u32 / COLS) * TILE);
        for y in 0..TILE - 4 {
            for x in 0..TILE - 4 {
                let (sx, sy) = (x * img.width / (TILE - 4), y * img.height / (TILE - 4));
                let k = ((sy * img.width + sx) * 4) as usize;
                let mut c = [img.pixels[k], img.pixels[k + 1], img.pixels[k + 2]];
                if img.pixels[k + 3] < 128 {
                    c = [0, 0, 0];
                }
                if let Some(m) = mask {
                    // (The mask may be smaller than the texture.)
                    let (mx, my) = (x * m.width / (TILE - 4), y * m.height / (TILE - 4));
                    if m.pixels[((my * m.width + mx) * 4) as usize] > 0 {
                        c = [c[0] / 3 + 170, c[1] / 3, c[2] / 3 + 170];
                    }
                }
                let o = (((ty + y) * COLS * TILE + tx + x) * 4) as usize;
                out[o..o + 3].copy_from_slice(&c);
                out[o + 3] = 255;
            }
        }
    }
    for k in (3..out.len()).step_by(4) {
        out[k] = 255;
    }
    image::save_buffer(&a[2], &out, COLS * TILE, rows * TILE, image::ExtendedColorType::Rgba8)?;
    Ok(())
}
