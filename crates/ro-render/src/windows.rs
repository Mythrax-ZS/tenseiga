//! Finding the windows in building textures, for lit windows at night. Colour alone can't
//! tell glass from blue cloth, slate or an awning, so this looks for window *shapes*: the
//! texture is split into regions of like colour, and a region counts as a pane when it is
//! a compact rectangle (or an arch) of a sensible size, inside the texture rather than at
//! its edge, neutral or cool in colour, and set off by a frame around it. Panes side by side
//! make a window; a cluster that sprawls across the texture is a wall of bricks or tiles,
//! not a window. The result is a mask the shader reads: red, where the glass is; green, a
//! number per window (so a whole window is lit or dark together).
//!
//! Lattice windows (Payon's paper screens, white-painted panes) are found apart: many small
//! light cells, each ringed by darker wood, packed into a block that stands alone in its
//! wall. Only the cells are marked, so the lattice stays dark against the glow.

use ro_formats::image::Rgba;

/// Panes found in a window, at most (more is brickwork).
const MAX_PANES: usize = 24;
/// Windows found in a texture, at most (more is a pattern of tiles or stones).
const MAX_WINDOWS: usize = 14;
/// Windows lined up in a band this many times as long as it is tall are a course of bricks
/// or a beam's pale band, broken into lengths: not windows.
const STRIP: u32 = 6;

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
    // Panes, and how strongly each is framed (the colour step to the ring around it); and
    // lattice cells (small light cells ringed by darker wood).
    let min_side = (w.min(h) / 40).max(4);
    let mut framing = vec![0.0f32; regions.len()];
    let mut cell = vec![false; regions.len()];
    let pane: Vec<bool> = regions
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let (rw, rh) = (r.w(), r.h());
            if r.edge || rw < 2 || rh < 2 || rw > w * 2 / 5 || rh > h * 2 / 5 {
                return false;
            }
            if rw.max(rh) > rw.min(rh) * 4 {
                return false;
            }
            let fill = r.count as f32 / (rw * rh) as f32;
            let m = r.mean();
            let ring = ring_mean(&px, r, w, h, 1);
            let (hi, lo) = (m[0].max(m[1]).max(m[2]), m[0].min(m[1]).min(m[2]));
            // A lattice cell: small, near white (paper, white paint; sandstone and plaster are
            // duller) and plain, its ring of wood clearly darker. (Its
            // shape may be an L or a T: a patterned lattice.)
            cell[ri] = fill >= 0.4 && rw.max(rh) <= (w.min(h) / 10).max(4) && lum(m) > 0.72 && (hi - lo) / hi.max(1e-3) < 0.25 && lum(ring) < lum(m) - 0.25;
            // Compact: a rectangle fills its box, an arch most of it.
            if fill < 0.72 || rw < min_side || rh < min_side {
                return false;
            }
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
            // Set off by a frame: the ring just outside it differs. (A step out from the pane:
            // in an upscaled texture the edge is soft for a few texels.)
            let ring = ring_mean(&px, r, w, h, (w.min(h) / 256).max(1));
            let contrast = (ring[0] - m[0]).abs() + (ring[1] - m[1]).abs() + (ring[2] - m[2]).abs();
            // Painted white around darker glass: a window's own frame (mortar, and the gaps
            // between slates, are mid-tones).
            framing[ri] = if lum(ring) > 0.72 && lum(ring) > lum(m) + 0.3 { 1.0 } else { 0.0 };
            contrast > 0.2
        })
        .collect();
    // Windows: panes near each other (across a mullion) of like colour.
    let lattices = lattice_windows(&regions, &cell, &px, w, h);
    // (Panes inside a lattice window are its paper; elsewhere a light pane may be glass.)
    let in_lattice = |r: &Region| lattices.iter().flat_map(|l| &l.panels).any(|p| r.x0 >= p.0 && r.x1 <= p.2 && r.y0 >= p.1 && r.y1 <= p.3);
    let ids: Vec<usize> = (0..regions.len()).filter(|&i| pane[i] && !in_lattice(&regions[i])).collect();
    if ids.is_empty() && lattices.is_empty() {
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
            let d = (ma[0] - mb[0]).abs() + (ma[1] - mb[1]).abs() + (ma[2] - mb[2]).abs();
            // Like colour, or touching (one window's panes, a painted reflection across them).
            let like = d < 0.3 || (d < 0.5 && dx <= 1 && dy <= 1);
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
    // A lattice window: its paper glows, its lattice stays dark.
    for (n, l) in lattices.iter().enumerate() {
        let id = 40 + ((n as u32 * 89 + l.x0 * 11 + l.y0 * 5 + 101) % 200) as u8;
        for &(x0, y0, x1, y1, paper) in &l.panels {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let c = px(x, y);
                    if 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2] > paper {
                        let k = ((y * w + x) * 4) as usize;
                        mask[k] = 255;
                        mask[k + 1] = id;
                        mask[k + 3] = 255;
                        covered += 1;
                    }
                }
            }
        }
        found += 1;
    }
    let mut mark = |panes: &[usize], id: u8, mask: &mut [u8]| {
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
    };
    let mut kept: Vec<Kept> = vec![];
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
        // A long strip is a pale band along a beam or a ridge (and with it, the rest of its
        // band below is no window either).
        let long = gw.max(gh) > gw.min(gh) * 5;
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
        kept.push(Kept { panes, id, bx: (x0, y0, x1, y1), long });
    }
    // Windows in a row (or a column) of three or more, like in size, whose band is a long
    // strip: the bricks of a course, the tile ends along a ridge. (Not a row of windows
    // taller than wide across the band, as tall narrow windows along a wall are.)
    let in_strip = |k: usize| {
        let a = kept[k].bx;
        [false, true].iter().any(|&down| {
            // (Across: x; along the band's thin way: y. Or the other way about.)
            let span = |b: (u32, u32, u32, u32)| if down { (b.0, b.2, b.1, b.3) } else { (b.1, b.3, b.0, b.2) };
            let (t0, t1, _, _) = span(a);
            let lined = |&(u0, u1, _, _): &(u32, u32, u32, u32)| {
                let over = t1.min(u1) as i64 - t0.max(u0) as i64 + 1;
                let (ta, ua) = (t1 - t0 + 1, u1 - u0 + 1);
                over as f32 >= 0.7 * ta.min(ua) as f32 && ta.max(ua) <= ta.min(ua) * 2
            };
            // (In line with a long strip lying along this way: the rest of that band.)
            if kept.iter().any(|k| k.long && (k.bx.2 - k.bx.0 > k.bx.3 - k.bx.1) != down && lined(&span(k.bx))) {
                return true;
            }
            let row: Vec<_> = kept.iter().map(|k| span(k.bx)).filter(lined).collect();
            let (lo, hi) = row.iter().fold((u32::MAX, 0), |(lo, hi), r| (lo.min(r.2), hi.max(r.3)));
            let (b0, b1) = row.iter().fold((u32::MAX, 0), |(lo, hi), r| (lo.min(r.0), hi.max(r.1)));
            let across = |r: &(u32, u32, u32, u32)| (r.1 - r.0 + 1) as f32 / (r.3 - r.2 + 1) as f32;
            let narrow = row.iter().filter(|r| across(r) > 1.25).count() * 2 > row.len();
            row.len() >= 3 && hi - lo + 1 > (b1 - b0 + 1) * STRIP && !narrow
        })
    };
    let strips: Vec<bool> = (0..kept.len()).map(|k| kept[k].long || in_strip(k)).collect();
    // (Strips still count toward too many windows: a tiled roof has them as well.)
    let mut patterned = 0;
    for (k, strip) in kept.iter().zip(strips) {
        if strip {
            patterned += 1;
        } else {
            mark(k.panes, k.id, &mut mask);
            found += 1;
        }
    }
    // A texture mostly "windows", or with a great many of them, is something else: a
    // tiled roof, a patterned wall.
    if found == 0 || found + patterned > MAX_WINDOWS || covered as f32 > (w * h) as f32 * 0.3 {
        return None;
    }
    Some(Rgba { width: w, height: h, pixels: mask })
}

/// A window that passed on its own: its panes, its number, its box, and whether it is a long
/// strip (see `STRIP`).
struct Kept<'a> {
    panes: &'a [usize],
    id: u8,
    bx: (u32, u32, u32, u32),
    long: bool,
}

/// The mean colour of the ring of texels `out` outside a region's box.
fn ring_mean(px: &impl Fn(u32, u32) -> [f32; 4], r: &Region, w: u32, h: u32, out: u32) -> [f32; 3] {
    let (mut ring, mut n) = ([0.0f32; 3], 0.0f32);
    let (x0, y0, x1, y1) = (r.x0.saturating_sub(out), r.y0.saturating_sub(out), (r.x1 + out).min(w - 1), (r.y1 + out).min(h - 1));
    let mut add = |x: u32, y: u32| {
        let c = px(x, y);
        ring = [ring[0] + c[0], ring[1] + c[1], ring[2] + c[2]];
        n += 1.0;
    };
    for x in x0..=x1 {
        add(x, y0);
        add(x, y1);
    }
    for y in y0..=y1 {
        add(x0, y);
        add(x1, y);
    }
    ring.map(|v| v / n.max(1.0))
}

/// A lattice window found: its panels' boxes and how light a texel must be to be paper.
struct Lattice {
    x0: u32,
    y0: u32,
    panels: Vec<(u32, u32, u32, u32, f32)>,
}

/// Lattice windows. Lattice cells close together (across a lattice bar) make a panel; a
/// panel counts when it has enough cells and its box is mostly light paper with dark lattice
/// through it. Panels side by side (across a window's frame) make a window, kept when it is
/// a fair size but not sprawling, and alone in its wall: little around it as light as its
/// paper (a whitewashed wall of panels is not one).
fn lattice_windows(regions: &[Region], cell: &[bool], px: &impl Fn(u32, u32) -> [f32; 4], w: u32, h: u32) -> Vec<Lattice> {
    let ids: Vec<usize> = (0..regions.len()).filter(|&i| cell[i]).collect();
    if ids.len() < 6 {
        return vec![];
    }
    fn root(g: &mut [usize], i: usize) -> usize {
        let mut i = i;
        while g[i] != i {
            g[i] = g[g[i]];
            i = g[i];
        }
        i
    }
    // Box gap between two boxes, each way (inclusive boxes: one a bar of b apart is b + 1).
    let apart = |a: (u32, u32, u32, u32), b: (u32, u32, u32, u32)| {
        let dx = (b.0 as i64 - a.2 as i64).max(a.0 as i64 - b.2 as i64);
        let dy = (b.1 as i64 - a.3 as i64).max(a.1 as i64 - b.3 as i64);
        dx.max(dy)
    };
    let bx = |r: &Region| (r.x0, r.y0, r.x1, r.y1);
    let mut group: Vec<usize> = (0..ids.len()).collect();
    let bar = (w.min(h) / 64).max(2) as i64;
    for a in 0..ids.len() {
        for b in a + 1..ids.len() {
            let (ra, rb) = (&regions[ids[a]], &regions[ids[b]]);
            let (ma, mb) = (ra.mean(), rb.mean());
            let d = (ma[0] - mb[0]).abs() + (ma[1] - mb[1]).abs() + (ma[2] - mb[2]).abs();
            if apart(bx(ra), bx(rb)) <= bar + 1 && d < 0.3 {
                let (x, y) = (root(&mut group, a), root(&mut group, b));
                group[x] = y;
            }
        }
    }
    let mut blocks: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, &id) in ids.iter().enumerate() {
        let g = root(&mut group, i);
        blocks.entry(g).or_default().push(id);
    }
    let lum = |c: [f32; 4]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    // Panels: (box, the lightness a texel needs to be paper).
    let mut panels: Vec<((u32, u32, u32, u32), f32)> = vec![];
    for cells in blocks.into_values() {
        if cells.len() < 8 || cells.len() > 200 {
            continue;
        }
        let (x0, y0) = cells.iter().map(|&i| (regions[i].x0, regions[i].y0)).fold((u32::MAX, u32::MAX), |a, b| (a.0.min(b.0), a.1.min(b.1)));
        let (x1, y1) = cells.iter().map(|&i| (regions[i].x1, regions[i].y1)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
        if (x1 - x0 + 1).min(y1 - y0 + 1) < (w.min(h) / 48).max(5) {
            continue;
        }
        let paper = {
            let (mut s, mut n) = (0.0f32, 0.0f32);
            for &i in &cells {
                let (m, c) = (regions[i].mean(), regions[i].count as f32);
                s += (0.299 * m[0] + 0.587 * m[1] + 0.114 * m[2]) * c;
                n += c;
            }
            s / n
        };
        // In the box: mostly paper, with lattice (clearly darker) through it.
        let (mut lit, mut bars, mut all, mut wood) = (0u32, 0u32, 0u32, 0.0f32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let l = lum(px(x, y));
                all += 1;
                lit += (l > paper - 0.15) as u32;
                if l <= paper - 0.15 {
                    wood += l;
                }
            }
        }
        // The rest is the lattice: dark wood (a carpet's pattern between its pale parts is
        // coloured and lighter).
        let wood = wood / (all - lit).max(1) as f32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                bars += (lum(px(x, y)) < paper - 0.3) as u32;
            }
        }
        let (lit, bars) = (lit as f32 / all as f32, bars as f32 / all as f32);
        // (Paper and white paint are brighter than the pale parts of carvings and patterns.)
        if paper >= 0.86 && (0.3..=0.9).contains(&lit) && bars >= 0.08 && wood < 0.4 {
            panels.push(((x0, y0, x1, y1), paper - 0.15));
        }
    }
    // Windows: panels across a frame from each other.
    let frame = (w.min(h) / 24).max(6) as i64;
    let mut group: Vec<usize> = (0..panels.len()).collect();
    for a in 0..panels.len() {
        for b in a + 1..panels.len() {
            if apart(panels[a].0, panels[b].0) <= frame && (panels[a].1 - panels[b].1).abs() < 0.2 {
                let (x, y) = (root(&mut group, a), root(&mut group, b));
                group[x] = y;
            }
        }
    }
    let mut windows: std::collections::BTreeMap<usize, Vec<usize>> = std::collections::BTreeMap::new();
    for i in 0..panels.len() {
        let g = root(&mut group, i);
        windows.entry(g).or_default().push(i);
    }
    let mut out = vec![];
    for members in windows.into_values() {
        let (x0, y0) = members.iter().map(|&i| (panels[i].0 .0, panels[i].0 .1)).fold((u32::MAX, u32::MAX), |a, b| (a.0.min(b.0), a.1.min(b.1)));
        let (x1, y1) = members.iter().map(|&i| (panels[i].0 .2, panels[i].0 .3)).fold((0, 0), |a, b| (a.0.max(b.0), a.1.max(b.1)));
        let (gw, gh) = (x1 - x0 + 1, y1 - y0 + 1);
        // (Clear of the texture's edges: a window sits in its wall.)
        if x0 < 2 || y0 < 2 || x1 + 2 >= w || y1 + 2 >= h {
            continue;
        }
        // (A long strip of pale stones atop a wall is no window.)
        if gw.max(gh) > gw.min(gh) * 6 || gw.min(gh) < (w.min(h) / 24).max(10) || gw > w * 3 / 5 || gh > h * 3 / 5 || (gw * gh) as f32 > (w * h) as f32 * 0.3 {
            continue;
        }
        let light = members.iter().map(|&i| panels[i].1).sum::<f32>() / members.len() as f32;
        // Alone in its wall: in a band around it, little is as light as its paper.
        let margin = (gw.max(gh) / 4).max(3);
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
                alike += (lum(c) > light) as u32;
            }
        }
        if seen == 0 || alike as f32 > seen as f32 * 0.17 {
            continue;
        }
        let panels = members.iter().map(|&i| { let ((a, b, c, d), l) = panels[i]; (a, b, c, d, l) }).collect();
        out.push(Lattice { x0, y0, panels });
    }
    out
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
    fn a_pale_band_along_a_beam_is_no_window() {
        // A window, and below it a dark beam with a pale band along it, broken into lengths:
        // touching (one long strip), or apart (a row of them).
        for gap in [2, 7] {
            let mut img = wall();
            fill(&mut img, 40, 10, 80, 50, [240, 240, 240, 255]);
            fill(&mut img, 43, 13, 77, 47, [60, 70, 85, 255]);
            fill(&mut img, 4, 70, 124, 84, [35, 35, 40, 255]);
            let mut x = 8;
            while x + 14 <= 120 {
                fill(&mut img, x, 73, x + 14, 81, [120, 125, 135, 255]);
                x += 14 + gap;
            }
            let m = window_mask(&img).expect("the window");
            let at = |x: u32, y: u32| m.pixels[((y * m.width + x) * 4) as usize];
            assert_eq!(at(60, 30), 255, "glass");
            assert_eq!(at(12, 76), 0, "beam, gap {gap}");
            assert_eq!(at(64, 76), 0, "beam, gap {gap}");
        }
    }

    #[test]
    fn a_paper_lattice_window_lights_its_paper_not_its_lattice() {
        // Dark wood, a frame, and a 6 x 5 lattice of pale paper cells 6 texels across.
        let mut img = Rgba { width: 128, height: 128, pixels: [90u8, 55, 40, 255].repeat(128 * 128) };
        fill(&mut img, 36, 36, 96, 88, [60, 38, 28, 255]);
        for cy in 0..5 {
            for cx in 0..6 {
                let (x, y) = (40 + cx * 9, 40 + cy * 9);
                fill(&mut img, x, y, x + 7, y + 7, [240, 236, 222, 255]);
            }
        }
        let m = window_mask(&img).expect("a lattice window");
        let at = |x: u32, y: u32| m.pixels[((y * m.width + x) * 4) as usize];
        assert_eq!(at(43, 43), 255, "paper");
        assert_eq!(at(47, 43), 0, "lattice bar");
        assert_eq!(at(10, 10), 0, "wall");
    }

    #[test]
    fn a_whitewashed_wall_of_panels_is_no_lattice() {
        // Light panels over a whole half-timbered wall: not alone in it.
        let mut img = Rgba { width: 128, height: 128, pixels: [70u8, 45, 30, 255].repeat(128 * 128) };
        for ty in 0..14 {
            for tx in 0..14 {
                fill(&mut img, tx * 9 + 2, ty * 9 + 2, tx * 9 + 9, ty * 9 + 9, [230, 225, 210, 255]);
            }
        }
        assert!(window_mask(&img).is_none());
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
