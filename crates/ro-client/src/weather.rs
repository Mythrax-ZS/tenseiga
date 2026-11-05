//! Weather: what the server turns on (its snow, sakura, leaves, clouds and fog mapflags, and
//! GM @snow and the like, sent as special effects on entering a map) and, if Settings asks,
//! ambient weather by climate: snow on snowy maps, petals where cherry trees grow, leaves in
//! forests, fog in Niflheim, and now and then a shower of rain. Like the original, the
//! server's weather lasts until the map changes (turning a mapflag off sends nothing).

use std::collections::HashMap;

use ro_render::{WeatherKind, WeatherLayer};
use serde::{Deserialize, Serialize};

/// Settings > Graphics > Weather.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum WeatherMode {
    Off,
    /// The server's weather alone, as the original.
    Server,
    /// The server's, and the client's own by climate.
    #[default]
    Ambient,
}

impl WeatherMode {
    pub const ALL: [WeatherMode; 3] = [WeatherMode::Off, WeatherMode::Server, WeatherMode::Ambient];

    pub fn name(self) -> &'static str {
        match self {
            WeatherMode::Off => "Off",
            WeatherMode::Server => "Server's only",
            WeatherMode::Ambient => "Server's and ambient",
        }
    }
}

/// The weather a special effect turns on (rAthena's clif_weather_check, plus EF_RAIN).
pub fn from_effect(effect: u32) -> Option<WeatherKind> {
    Some(match effect {
        161 => WeatherKind::Rain,         // EF_RAIN
        162 => WeatherKind::Snow,         // EF_SNOW (snow)
        163 => WeatherKind::Petals,       // EF_SAKURA (sakura)
        333 => WeatherKind::Leaves,       // EF_MAPLE (leaves)
        233 | 516 => WeatherKind::Clouds, // EF_CLOUD3 (clouds), EF_CLOUD5 (clouds2)
        515 => WeatherKind::Fog,          // EF_CLOUD4 (fog)
        _ => return None,
    })
}

/// Ambient weather for a map's climate (its sky profile's name) at `unix_secs`: what is on
/// and how strongly.
pub fn ambient(climate: &str, map: &str, unix_secs: u64) -> Vec<(WeatherKind, f32)> {
    let mut out = match climate {
        "snow" => vec![(WeatherKind::Snow, 0.55)],
        "blossom" => vec![(WeatherKind::Petals, 0.6)],
        "forest" => vec![(WeatherKind::Leaves, 0.35)],
        "gothic" => vec![(WeatherKind::Fog, 0.7)],
        _ => vec![],
    };
    if matches!(climate, "temperate" | "coastal" | "forest" | "highland") && showering(map, unix_secs) {
        out.push((WeatherKind::Rain, 0.85));
    }
    out
}

/// Showers come and go in twenty-minute spells, about one spell in six, each map on its own
/// schedule (the same for everyone, as it follows the clock).
fn showering(map: &str, unix_secs: u64) -> bool {
    const SPELL_SECS: u64 = 20 * 60;
    let spell = unix_secs / SPELL_SECS;
    // FNV-1a over the map name and the spell.
    let h = map.bytes().chain(spell.to_le_bytes()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    h % 6 == 0
}

#[derive(Default)]
pub struct Weather {
    /// The server's weather on this map.
    server: Vec<WeatherKind>,
    /// `--weather snow,fog`: shown on every map (captures).
    forced: Vec<WeatherKind>,
    /// Each kind's strength as shown, eased toward what is wanted.
    shown: HashMap<WeatherKind, f32>,
    /// Each kind's texture once loaded (and how many pictures it holds side by side).
    textures: HashMap<WeatherKind, (ro_render::TexId, u32)>,
}

impl Weather {
    /// With `forced` weather on every map (`--weather`).
    pub fn with_forced(forced: Vec<WeatherKind>) -> Self {
        Self { forced, ..Default::default() }
    }

    /// A new map: the server says again what its weather is.
    pub fn clear_server(&mut self) {
        self.server.clear();
        self.shown.clear();
    }

    pub fn server_on(&mut self, kind: WeatherKind) {
        if !self.server.contains(&kind) {
            tracing::info!(?kind, "weather");
            self.server.push(kind);
        }
    }

    /// Eases the weather toward what is wanted and hands it to the renderer. `climate` is the
    /// map's sky profile (None indoors and underground, where no ambient weather falls).
    pub fn update(&mut self, dt_ms: f32, mode: WeatherMode, map: &str, climate: Option<&str>, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs) {
        let mut want: Vec<(WeatherKind, f32)> = vec![];
        if mode != WeatherMode::Off {
            want.extend(self.server.iter().chain(&self.forced).map(|k| (*k, 1.0)));
        }
        if let (WeatherMode::Ambient, Some(climate)) = (mode, climate) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            for (kind, s) in ambient(climate, map, now) {
                if !want.iter().any(|(k, _)| *k == kind) {
                    want.push((kind, s));
                }
            }
        }
        // About eight seconds to come in or clear.
        let step = dt_ms / 8000.0;
        for (kind, target) in &want {
            let s = self.shown.entry(*kind).or_insert(0.0);
            *s = if *s < *target { (*s + step).min(*target) } else { (*s - step).max(*target) };
        }
        for (kind, s) in self.shown.iter_mut() {
            if !want.iter().any(|(k, _)| k == kind) {
                *s -= step;
            }
        }
        self.shown.retain(|_, s| *s > 0.0);
        r.weather.clear();
        // A steady order, so layers don't swap places from frame to frame.
        let mut kinds: Vec<(WeatherKind, f32)> = self.shown.iter().map(|(k, s)| (*k, *s)).collect();
        kinds.sort_by_key(|(k, _)| *k as u8);
        for (kind, strength) in kinds {
            let (tex, frames) = *self.textures.entry(kind).or_insert_with(|| texture(kind, r, vfs));
            r.weather.push(WeatherLayer { kind, tex, frames, strength });
        }
    }
}

/// A kind's look: the original's art where the data has it (petals, maple leaves, fog and
/// cloud puffs), else drawn here; snow and rain are always drawn here (the data's snow is
/// sheets of flakes for scrolling, not single flakes).
fn texture(kind: WeatherKind, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs) -> (ro_render::TexId, u32) {
    let art = |paths: &[&str]| -> Option<ro_render::Rgba> {
        let imgs: Vec<ro_render::Rgba> = paths
            .iter()
            .filter_map(|p| vfs.read(&format!("data\\texture\\effect\\{p}")).ok().flatten().and_then(|d| ro_formats::image::decode(&d, p).ok()))
            .collect();
        side_by_side(imgs)
    };
    let (img, frames) = match kind {
        WeatherKind::Snow => (flake(), 1),
        WeatherKind::Rain => (streak(), 1),
        WeatherKind::Petals => art(&["efst_blossom_fluttering\\sakura1.bmp"]).map(black_to_alpha).map_or((flake(), 1), |i| (i, 1)),
        WeatherKind::Leaves => art(&["efst_autumn_full_moon\\maple1.tga", "efst_autumn_full_moon\\maple2.tga"]).map_or((flake(), 1), frames),
        WeatherKind::Fog => art(&["fog1.tga", "fog2.tga"]).map_or((puff(), 1), frames),
        WeatherKind::Clouds => art(&["cloud3.tga", "cloud5.tga"]).map_or((puff(), 1), frames),
    };
    (r.add_effect_texture(&img), frames)
}

/// Square pictures side by side: the picture and how many it holds.
fn frames(img: ro_render::Rgba) -> (ro_render::Rgba, u32) {
    let n = (img.width / img.height.max(1)).max(1);
    (img, n)
}

/// Pictures of one size, left to right (the shader picks one per particle). None if none.
fn side_by_side(imgs: Vec<ro_render::Rgba>) -> Option<ro_render::Rgba> {
    let first = imgs.first()?;
    let (w, h) = (first.width, first.height);
    let imgs: Vec<_> = imgs.iter().filter(|i| (i.width, i.height) == (w, h)).collect();
    let n = imgs.len() as u32;
    let mut pixels = vec![0u8; (w * n * h * 4) as usize];
    for (k, img) in imgs.iter().enumerate() {
        for y in 0..h {
            let src = &img.pixels[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
            let at = ((y * w * n + k as u32 * w) * 4) as usize;
            pixels[at..at + src.len()].copy_from_slice(src);
        }
    }
    Some(ro_render::Rgba { width: w * n, height: h, pixels })
}

/// Art on black with no alpha (the sakura petal): black is see-through, the rest solid.
fn black_to_alpha(mut img: ro_render::Rgba) -> ro_render::Rgba {
    for p in img.pixels.chunks_exact_mut(4) {
        let v = p[0].max(p[1]).max(p[2]);
        p[3] = if v < 12 { 0 } else { p[3].min(((v as u32 - 12) * 255 / 40).min(255) as u8) };
    }
    img
}

/// A round flake, soft at its edge.
fn flake() -> ro_render::Rgba {
    soft_disc(32, 2.2)
}

/// A puff for fog and clouds when the data has none: very soft.
fn puff() -> ro_render::Rgba {
    soft_disc(64, 1.2)
}

fn soft_disc(n: u32, sharp: f32) -> ro_render::Rgba {
    let mut pixels = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = ((x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            let a = (1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0).powf(1.0 / sharp);
            pixels.extend([255, 255, 255, (a * 255.0) as u8]);
        }
    }
    ro_render::Rgba { width: n, height: n, pixels }
}

/// A raindrop's streak: brightest in the middle, fading to both ends and its sides.
fn streak() -> ro_render::Rgba {
    let (w, h) = (8u32, 64u32);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let across = 1.0 - ((x as f32 + 0.5) / w as f32 * 2.0 - 1.0).abs();
            let along = (std::f32::consts::PI * (y as f32 + 0.5) / h as f32).sin();
            pixels.extend([255, 255, 255, (across * along * 255.0) as u8]);
        }
    }
    ro_render::Rgba { width: w, height: h, pixels }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_servers_weather_effects() {
        assert_eq!(from_effect(162), Some(WeatherKind::Snow));
        assert_eq!(from_effect(163), Some(WeatherKind::Petals));
        assert_eq!(from_effect(333), Some(WeatherKind::Leaves));
        assert_eq!(from_effect(233), Some(WeatherKind::Clouds));
        assert_eq!(from_effect(516), Some(WeatherKind::Clouds));
        assert_eq!(from_effect(515), Some(WeatherKind::Fog));
        assert_eq!(from_effect(297), None, "fireworks stay effects");
    }

    #[test]
    fn ambient_weather_follows_the_climate() {
        assert_eq!(ambient("snow", "xmas", 0)[0].0, WeatherKind::Snow);
        assert_eq!(ambient("blossom", "amatsu", 0)[0].0, WeatherKind::Petals);
        assert!(ambient("desert", "morocc", 0).is_empty());
        assert!(ambient("volcanic", "thor_v01", 0).is_empty());
        // Showers: some spells but not most, steady through a spell, each map its own.
        let spells: Vec<bool> = (0..600u64).map(|s| showering("prt_fild08", s * 1200)).collect();
        let wet = spells.iter().filter(|w| **w).count();
        assert!((60..150).contains(&wet), "{wet} of 600 spells rain");
        assert_eq!(showering("prt_fild08", 1200 * 7), showering("prt_fild08", 1200 * 7 + 1199));
        assert_ne!((0..600u64).map(|s| showering("payon", s * 1200)).collect::<Vec<_>>(), spells);
    }

    #[test]
    fn pictures_go_side_by_side() {
        let img = |v: u8| ro_render::Rgba { width: 2, height: 1, pixels: vec![v; 8] };
        let both = side_by_side(vec![img(1), img(2)]).unwrap();
        assert_eq!((both.width, both.height), (4, 1));
        assert_eq!(both.pixels, [1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2]);
        assert!(side_by_side(vec![]).is_none());
    }
}
