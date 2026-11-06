//! Finding the windows in building textures, for lit windows at night. Colour alone can't
//! tell glass from blue cloth, slate or an awning, so this looks for window *shapes*: the
//! texture is split into regions of like colour, and a region counts as a pane when it is
//! a compact rectangle (or an arch) of a sensible size, inside the texture rather than at
//! its edge, neutral or cool in colour, and set off by a frame around it. Panes side by side
//! make a window; a cluster that sprawls across the texture is a wall of bricks or tiles,
//! not a window. The result is a mask the shader reads: red, where the glass is; green, a
//! number per window (so a whole window is lit or dark together).

use ro_formats::image::Rgba;

/// Panes found in a window, at most (more is brickwork).
const MAX_PANES: usize = 24;
/// Windows found in a texture, at most (more is a pattern of tiles or stones).
const MAX_WINDOWS: usize = 14;

struct Region {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    count: u32,
    sum: [f32; 3],
    edge: bool,
}

impl Region {
    fn w(&self) -> u32 {
        self.x1 - self.x0 + 1
    }
    fn h(&self) -> u32 {
        self.y1 - self.y0 + 1
    }
    fn mean(&self) -> [f32; 3] {
        self.sum.map(|s| s / self.count as f32)
    }
}

/// Textures are searched at most this size across (an HD texture is shrunk first: its
/// windows show as well, for a sixteenth of the work). The mask is this size too; it is
/// sampled by the texture's own coordinates.
const SEARCH: u32 = 256;

/// The window mask of a texture (at most `SEARCH` across), or None if it has no windows.
pub fn window_mask(img: &Rgba) -> Option<Rgba> {
    let small;
    let img = if img.width.max(img.height) > SEARCH {
        small = shrink(img, img.width.max(img.height).div_ceil(SEARCH));
        &small
    } else {
        img
    };
    let (w, h) = (img.width, img.height);
    if w < 32 || h < 32 {
        return None;
    }
    let px = |x: u32, y: u32| -> [f32; 4] {
        let i = ((y * w + x) * 4) as usize;
        let p = &img.pixels[i..i + 4];
        [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0, p[3] as f32 / 255.0]
    };
    let lum = |c: [f32; 3]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    // Regions of like colour: grown pixel to pixel (so a pane painted with a gradient or a
    // streak of reflection holds together), never across a step in colour.
    let mut label = vec![u32::MAX; (w * h) as usize];
    let mut regions: Vec<Region> = vec![];
    let mut stack = vec![];
    for sy in 0..h {
        for sx in 0..w {
            let si = (sy * w + sx) as usize;
            if label[si] != u32::MAX || px(sx, sy)[3] < 0.5 {
                continue;
            }
            let id = regions.len() as u32;
            let mut r = Region { x0: sx, y0: sy, x1: sx, y1: sy, count: 0, sum: [0.0; 3], edge: false };
            label[si] = id;
            stack.push((sx, sy));
            while let Some((x, y)) = stack.pop() {
                let c = px(x, y);
                r.count += 1;
                for (sum, c) in r.sum.iter_mut().zip(c) {
                    *sum += c;
                }
                r.x0 = r.x0.min(x);
                r.y0 = r.y0.min(y);
                r.x1 = r.x1.max(x);
                r.y1 = r.y1.max(y);
                r.edge |= x == 0 || y == 0 || x == w - 1 || y == h - 1;
                let near = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                for (nx, ny) in near {
                    if nx >= w || ny >= h {
                        continue;
                    }
                    let ni = (ny * w + nx) as usize;
                    if label[ni] != u32::MAX {
                        continue;
                    }
                    let n = px(nx, ny);
                    if n[3] < 0.5 {
                        // Beside a see-through part: glass sits inside a solid wall.
                        r.edge = true;
                        continue;
                    }
                    let d = (n[0] - c[0]).abs() + (n[1] - c[1]).abs() + (n[2] - c[2]).abs();
                    if d < 0.16 {
                        label[ni] = id;
                        stack.push((nx, ny));
                    }
                }
            }
            regions.push(r);
        }
    }
    // Panes, and how strongly each is framed (the colour step to the ring around it).
    let min_side = (w.min(h) / 40).max(4);
    let mut framing = vec![0.0f32; regions.len()];
    let pane: Vec<bool> = regions
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let (rw, rh) = (r.w(), r.h());
            if r.edge || rw < min_side || rh < min_side || rw > w * 2 / 5 || rh > h * 2 / 5 {
                return false;
            }
            if rw.max(rh) > rw.min(rh) * 4 {
                return false;
            }
            // Compact: a rectangle fills its box, an arch most of it.
            if (r.count as f32) < (rw * rh) as f32 * 0.72 {
                return false;
            }
            let m = r.mean();
            let (hi, lo) = (m[0].max(m[1]).max(m[2]), m[0].min(m[1]).min(m[2]));
            let sat = (hi - lo) / hi.max(1e-3);
            // Glass: not bright, not vivid, not warm (wood, brick, plaster, cloth are). Blue or
            // green glass may be fairly strongly coloured.
            let cool = m[2] >= m[0] || m[1] >= m[0] + 0.05;
            // Pale glass (a sky-blue or white pane) is cool; pale plaster is warm or grey.
            let pale_glass = m[2] >= m[0] + 0.02 && m[2] >= m[1] - 0.02;
            if lum(m) > if pale_glass { 0.88 } else { 0.62 } || sat > if cool { 0.6 } else { 0.35 } || m[0] > m[2] + 0.07 && !cool {
                return false;
            }
            // Set off by a frame: the ring just outside it differs.
            let (mut ring, mut n) = ([0.0f32; 3], 0.0f32);
            // (A step out from the pane: in an upscaled texture the edge is soft for a few texels.)
            let out = (w.min(h) / 256).max(1);
            let (x0, y0, x1, y1) = (r.x0.saturating_sub(out), r.y0.saturating_sub(out), (r.x1 + out).min(w - 1), (r.y1 + out).min(h - 1));
            for x in x0..=x1 {
                for y in [y0, y1] {
                    let c = px(x, y);
                    ring = [ring[0] + c[0], ring[1] + c[1], ring[2] + c[2]];
                    n += 1.0;
                }
            }
            for y in y0..=y1 {
                for x in [x0, x1] {
                    let c = px(x, y);
                    ring = [ring[0] + c[0], ring[1] + c[1], ring[2] + c[2]];
                    n += 1.0;
                }
            }
            let ring = ring.map(|v| v / n);
            let contrast = (ring[0] - m[0]).abs() + (ring[1] - m[1]).abs() + (ring[2] - m[2]).abs();
            // Painted white around darker glass: a window's own frame (mortar, and the gaps
            // between slates, are mid-tones).
            framing[ri] = if lum(ring) > 0.72 && lum(ring) > lum(m) + 0.3 { 1.0 } else { 0.0 };
            contrast > 0.2
        })
        .collect();
    // Windows: panes near each other (across a mullion) of like colour.
    let ids: Vec<usize> = (0..regions.len()).filter(|&i| pane[i]).collect();
    if ids.is_empty() {
        return None;
    }
    let mut group: Vec<usize> = (0..ids.len()).collect();
    fn root(g: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    let gap = (w.min(h) / 32).max(3);
    for a in 0..ids.len() {
        for b in a + 1..ids.len() {
            let (ra, rb) = (&regions[ids[a]], &regions[ids[b]]);
            let dx = (rb.x0 as i64 - ra.x1 as i64).max(ra.x0 as i64 - rb.x1 as i64);
            let dy = (rb.y0 as i64 - ra.y1 as i64).max(ra.y0 as i64 - rb.y1 as i64);
            let (ma, mb) = (ra.mean(), rb.mean());
            let like = (ma[0] - mb[0]).abs() + (ma[1] - mb[1]).abs() + (ma[2] - mb[2]).abs() < 0.3;
            if dx <= gap as i64 && dy <= gap as i64 && like {
                let (x, y) = (root(&mut group, a), root(&mut group, b));
                group[x] = y;
            }
        }
    }
    let mut windows: std::collections::HashMap<usize, Vec<usize>> = std::collections::HashMap::new();
    for (i, &id) in ids.iter().enumerate() {
        let g = root(&mut group, i);
        windows.entry(g).or_default().push(id);
    }
    let mut mask = vec![0u8; (w * h * 4) as usize];
    let mut found = 0usize;
    let mut covered = 0u64;
    for (n, panes) in windows.values().enumerate() {
        let (x0, y0) = panes.iter().map(|&i| (regions[i].x0, regions[i].y0)).fold((u32::MAX, u32::MAX), |a, b| (a.0.min(b.0), a.1.min(b.1)));
        let (x1, y1) = panes.iter().map(|&i| (regions[i].x1, regions[i].y1)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
        let (gw, gh) = (x1 - x0 + 1, y1 - y0 + 1);
        // Brickwork or tiling, not a window: too many panes, or sprawling. A lone pane must be
        // a fair size (a fleck of stone or slate is no window).
        if panes.len() > MAX_PANES || gw > w / 2 || gh > h / 2 || (gw * gh) as f32 > (w * h) as f32 * 0.16 {
            continue;
        }
        if panes.len() == 1 && (gw.min(gh) < (w.min(h) / 32).max(5) || regions[panes[0]].count < 30) {
            continue;
        }
        // A window has some size to it (flecks of roof tiles don't).
        if gw.min(gh) < (w.min(h) / 24).max(5) {
            continue;
        }
        // A window stands alone in a wall of another colour; a slate or a block of stone is
        // one of many like it. In a band around the window, little may look like its glass.
        let glass = {
            let (mut s, mut n) = ([0.0f32; 3], 0.0f32);
            for &i in panes {
                let m = regions[i].mean();
                let c = regions[i].count as f32;
                s = [s[0] + m[0] * c, s[1] + m[1] * c, s[2] + m[2] * c];
                n += c;
            }
            s.map(|v| v / n)
        };
        let margin = (gw.max(gh) / 3).max(4);
        let (bx0, by0, bx1, by1) = (x0.saturating_sub(margin), y0.saturating_sub(margin), (x1 + margin).min(w - 1), (y1 + margin).min(h - 1));
        let (mut alike, mut seen) = (0u32, 0u32);
        for y in by0..=by1 {
            for x in bx0..=bx1 {
                if x >= x0 && x <= x1 && y >= y0 && y <= y1 {
                    continue;
                }
                let c = px(x, y);
                if c[3] < 0.5 {
                    continue;
                }
                seen += 1;
                if (c[0] - glass[0]).abs() + (c[1] - glass[1]).abs() + (c[2] - glass[2]).abs() < 0.2 {
                    alike += 1;
                }
            }
        }
        // (A white-painted frame is a window's own mark: such a window may sit in timber the
        // colour of its glass.)
        let framed = panes.iter().map(|&i| framing[i]).sum::<f32>() / panes.len() as f32;
        if framed < 0.5 && (seen == 0 || alike as f32 > seen as f32 * 0.18) {
            continue;
        }
        let id = 40 + ((n as u32 * 97 + x0 * 13 + y0 * 7) % 200) as u8;
        for &i in panes {
            let r = &regions[i];
            for y in r.y0..=r.y1 {
                for x in r.x0..=r.x1 {
                    let k = ((y * w + x) * 4) as usize;
                    mask[k] = 255;
                    mask[k + 1] = id;
                    mask[k + 3] = 255;
                    covered += 1;
                }
            }
        }
        found += 1;
    }
    // A texture mostly "windows", or with a great many of them, is something else: a
    // tiled roof, a patterned wall.
    if found == 0 || found > MAX_WINDOWS || covered as f32 > (w * h) as f32 * 0.3 {
        return None;
    }
    Some(Rgba { width: w, height: h, pixels: mask })
}

/// `img` shrunk by `f` each way, averaging each f x f block.
fn shrink(img: &Rgba, f: u32) -> Rgba {
    let (w, h) = ((img.width / f).max(1), (img.height / f).max(1));
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0u32; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let (sx, sy) = ((x * f + dx).min(img.width - 1), (y * f + dy).min(img.height - 1));
                    let k = ((sy * img.width + sx) * 4) as usize;
                    for (sum, &p) in sum.iter_mut().zip(&img.pixels[k..k + 4]) {
                        *sum += p as u32;
                    }
                }
            }
            let k = ((y * w + x) * 4) as usize;
            for c in 0..4 {
                pixels[k + c] = (sum[c] / (f * f)) as u8;
            }
        }
    }
    Rgba { width: w, height: h, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plaster wall (warm beige) 128 square.
    fn wall() -> Rgba {
        Rgba { width: 128, height: 128, pixels: [200u8, 180, 150, 255].repeat(128 * 128) }
    }

    fn fill(img: &mut Rgba, x0: u32, y0: u32, x1: u32, y1: u32, c: [u8; 4]) {
        for y in y0..y1 {
            for x in x0..x1 {
                let k = ((y * img.width + x) * 4) as usize;
                img.pixels[k..k + 4].copy_from_slice(&c);
            }
        }
    }

    #[test]
    fn a_framed_window_is_found_and_only_its_glass_is_marked() {
        let mut img = wall();
        // A white frame with four dark blue-grey panes.
        fill(&mut img, 40, 40, 80, 90, [240, 240, 240, 255]);
        for (x, y) in [(43, 43), (61, 43), (43, 66), (61, 66)] {
            fill(&mut img, x, y, x + 16, y + 21, [60, 70, 85, 255]);
        }
        let m = window_mask(&img).expect("a window");
        let at = |x: u32, y: u32| m.pixels[((y * m.width + x) * 4) as usize];
        assert_eq!(at(50, 50), 255, "glass");
        assert_eq!(at(41, 41), 0, "frame");
        assert_eq!(at(10, 10), 0, "wall");
    }

    #[test]
    fn plain_walls_cloth_and_slates_have_no_windows() {
        assert!(window_mask(&wall()).is_none());
        // A sheet of blue cloth: one big region touching the edges.
        let cloth = Rgba { width: 128, height: 128, pixels: [70u8, 90, 160, 255].repeat(128 * 128) };
        assert!(window_mask(&cloth).is_none());
        // Slates: a grid of like blue-grey tiles with dark gaps between.
        let mut slates = Rgba { width: 128, height: 128, pixels: [30u8, 32, 38, 255].repeat(128 * 128) };
        for ty in 0..10 {
            for tx in 0..10 {
                fill(&mut slates, tx * 12 + 3, ty * 12 + 3, tx * 12 + 13, ty * 12 + 13, [80, 95, 115, 255]);
            }
        }
        assert!(window_mask(&slates).is_none());
    }

    #[test]
    fn big_textures_are_searched_shrunk() {
        let mut img = Rgba { width: 1024, height: 1024, pixels: [200u8, 180, 150, 255].repeat(1024 * 1024) };
        fill(&mut img, 320, 320, 640, 720, [240, 240, 240, 255]);
        for (x, y) in [(344, 344), (488, 344), (344, 528), (488, 528)] {
            fill(&mut img, x, y, x + 128, y + 168, [60, 70, 85, 255]);
        }
        let m = window_mask(&img).expect("a window");
        assert_eq!((m.width, m.height), (256, 256));
    }
}
