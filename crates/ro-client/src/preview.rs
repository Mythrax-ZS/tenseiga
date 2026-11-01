//! Character previews for the select and create screens: the body, head (dyed) and
//! headgear sprites composited on the CPU into one image, the way the renderer layers
//! them in game (head attached at the body's anchor).

use ro_formats::{act::Act, spr::Spr};

/// What a preview shows.
#[derive(Clone, PartialEq, Debug)]
pub struct Look {
    /// Job sprite name (Korean, as in the GRF), e.g. 초보자.
    pub job: String,
    pub male: bool,
    pub hair: u16,
    pub hair_color: u16,
    /// Body dye (jobs::body_palette).
    pub clothes_color: u16,
    /// Headgear sprite names (악세사리), already resolved.
    pub headgear: Vec<String>,
}

const SIZE: i32 = 140;

struct Layered {
    images: Vec<ro_formats::image::Rgba>,
    /// How many of `images` are palette-indexed (RGBA layers index after them).
    indexed_len: usize,
    act: Act,
}

fn load(vfs: &ro_vfs::Vfs, base: &str, palette: Option<String>) -> Option<Layered> {
    let spr = Spr::parse(&vfs.read(&format!("{base}.spr")).ok()??).ok()?;
    let act = Act::parse(&vfs.read(&format!("{base}.act")).ok()??).ok()?;
    let pal = palette.and_then(|p| vfs.read(&p).ok().flatten()).and_then(|d| ro_formats::spr::parse_palette(&d).ok());
    let (indexed, rgba) = spr.images(pal.as_ref());
    let indexed_len = indexed.len();
    let mut images = indexed;
    images.extend(rgba);
    Some(Layered { images, indexed_len, act })
}

/// Draws frame 0 of `action` (idle, facing the camera) with its layers at `shift`.
fn draw(out: &mut [u8], s: &Layered, action: usize, shift: (i32, i32)) {
    let Some(frame) = s.act.actions.get(action).and_then(|a| a.frames.first()) else { return };
    for l in &frame.layers {
        if l.sprite < 0 {
            continue;
        }
        let idx = if l.kind == 1 { s.indexed_len + l.sprite as usize } else { l.sprite as usize };
        let Some(img) = s.images.get(idx) else { continue };
        let (w, h) = (img.width as i32, img.height as i32);
        // Centre of the layer relative to the feet, then the image's top-left in the canvas.
        let (cx, cy) = (SIZE / 2 + l.x + shift.0, SIZE * 3 / 4 + l.y + shift.1);
        for y in 0..h {
            for x in 0..w {
                let sx = if l.mirror { w - 1 - x } else { x };
                let p = ((y * w + sx) * 4) as usize;
                let (r, g, b, a) = (img.pixels[p], img.pixels[p + 1], img.pixels[p + 2], img.pixels[p + 3]);
                if a == 0 {
                    continue;
                }
                let (dx, dy) = (cx - w / 2 + x, cy - h / 2 + y);
                if dx < 0 || dy < 0 || dx >= SIZE || dy >= SIZE {
                    continue;
                }
                let d = ((dy * SIZE + dx) * 4) as usize;
                // Images are premultiplied; composite "over".
                let inv = 255 - a as u32;
                out[d] = (r as u32 + out[d] as u32 * inv / 255) as u8;
                out[d + 1] = (g as u32 + out[d + 1] as u32 * inv / 255) as u8;
                out[d + 2] = (b as u32 + out[d + 2] as u32 * inv / 255) as u8;
                out[d + 3] = (a as u32 + out[d + 3] as u32 * inv / 255) as u8;
            }
        }
    }
}

/// The composited preview, `SIZE` square, premultiplied RGBA. None if the body is missing.
pub fn compose(vfs: &ro_vfs::Vfs, look: &Look) -> Option<egui::ColorImage> {
    let sex = if look.male { "남" } else { "여" };
    // Doram (Summoner) have their own sprite and palette folders.
    let race = crate::jobs::race_dir(&look.job);
    let pal_dir = if race == "도람족" { "palette\\도람족\\머리" } else { "palette\\머리" };
    let appearance = crate::jobs::appearance(&look.job, look.male)?;
    let body = load(vfs, &appearance.body, crate::jobs::body_palette(&look.job, look.male, look.clothes_color))?;
    let head = if appearance.attachments { load(
        vfs,
        &format!("data\\sprite\\{race}\\머리통\\{sex}\\{}_{sex}", look.hair),
        (look.hair_color > 0).then(|| format!("data\\{pal_dir}\\머리{}_{sex}_{}.pal", look.hair, look.hair_color)),
    ) } else { None };
    let gear: Vec<Layered> = look.headgear.iter().filter(|_| appearance.attachments)
        .filter_map(|n| load(vfs, &format!("data\\sprite\\악세사리\\{sex}\\{sex}{n}"), None)).collect();
    let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
    let anchor = |s: &Layered| s.act.actions.first().and_then(|a| a.frames.first()).and_then(|f| f.anchors.first()).copied();
    let body_anchor = anchor(&body);
    draw(&mut out, &body, 0, (0, 0));
    for s in head.iter().chain(gear.iter()) {
        let shift = match (body_anchor, anchor(s)) {
            (Some(b), Some(h)) => (b[0] - h[0], b[1] - h[1]),
            _ => (0, 0),
        };
        draw(&mut out, s, 0, shift);
    }
    Some(egui::ColorImage::from_rgba_premultiplied([SIZE as usize, SIZE as usize], &out))
}
