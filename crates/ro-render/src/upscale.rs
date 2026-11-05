//! Texture upscaling for environment and interface art. See docs/upscaling.md.
//!
//! ┌──────────────────────────────────────────────────────────────────────────────────┐
//! │ NEVER CHARACTER OR EFFECT SPRITES.                                               │
//! │ Sprites (players, monsters, NPCs, pets, sprite effects: data\sprite\..., SPR/ACT)│
//! │ are uploaded by `sprite::Sheet::upload`, which never calls into this module.     │
//! │ `AssetClass` deliberately has no sprite variant, and `Upscaler::apply` refuses   │
//! │ any path under data\sprite\ even if a caller gets it wrong.                      │
//! └──────────────────────────────────────────────────────────────────────────────────┘
//!
//! Two stages, each optional:
//! - load time: the decoded image is resampled once on the CPU (Catmull-Rom or Lanczos-3,
//!   alpha-premultiplied so colour-keyed transparency never bleeds into edges), optionally
//!   cached on disk, then uploaded with its usual mip chain;
//! - run time: the world shader samples ground and model textures with a 9-tap Catmull-Rom
//!   filter wherever they are magnified (bilinear elsewhere), for every texture regardless
//!   of the load-time stage. Sprites use their own pipeline and sampler and never see it.

use std::path::PathBuf;

use ro_formats::image::Rgba;
use serde::{Deserialize, Serialize};

/// What a texture is, which decides whether it may be upscaled. There is no sprite class:
/// character and effect sprites are never passed here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssetClass {
    /// GND ground textures and RSW water frames.
    Ground,
    /// RSM model textures.
    Model,
    /// Item icons and collection pictures (data\texture\유저인터페이스\item, collection).
    ItemIcon,
    /// Other interface art: window skins, buttons, minimaps, status icons, loading screens.
    Interface,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    /// No upscaling at all; textures exactly as before.
    Off,
    /// Run-time Catmull-Rom filtering of world textures only: no load cost, no extra memory.
    Fast,
    /// Load-time Catmull-Rom upscale + run-time Catmull-Rom (default).
    Balanced,
    /// Load-time Lanczos-3 with anti-ringing + run-time Catmull-Rom.
    High,
}

/// `[upscale]` in settings.toml.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct UpscaleSettings {
    pub quality: Quality,
    /// Load-time scale factor: 1.5, 2 (default), 3 or 4.
    pub factor: f32,
    /// Upscaled textures are capped to this many texels on their longer side.
    pub max_size: u32,
    /// Per-category switches (sprites have none: they are never upscaled).
    pub ground: bool,
    pub models: bool,
    pub item_icons: bool,
    pub interface: bool,
    /// Texture paths containing any of these (case-insensitive) are left alone.
    pub exclude: Vec<String>,
    /// Keep load-time results in `cache/upscale` so later loads skip the resampling. Off by
    /// default: at 2x resampling is faster than the disk it would save (about 100 MB per
    /// town); worth it at 3x and 4x.
    pub cache: bool,
}

impl Default for UpscaleSettings {
    fn default() -> Self {
        Self { quality: Quality::Balanced, factor: 2.0, max_size: 2048, ground: true, models: true, item_icons: true, interface: true, exclude: vec![], cache: false }
    }
}

impl UpscaleSettings {
    pub const FACTORS: [f32; 4] = [1.5, 2.0, 3.0, 4.0];

    /// Whether the world shader filters magnified textures with Catmull-Rom.
    pub fn runtime_bicubic(&self) -> bool {
        self.quality != Quality::Off
    }

    fn kernel(&self) -> Option<Kernel> {
        match self.quality {
            Quality::Off | Quality::Fast => None,
            Quality::Balanced => Some(Kernel::CatmullRom),
            Quality::High => Some(Kernel::Lanczos3),
        }
    }

    fn class_on(&self, class: AssetClass) -> bool {
        match class {
            AssetClass::Ground => self.ground,
            AssetClass::Model => self.models,
            AssetClass::ItemIcon => self.item_icons,
            AssetClass::Interface => self.interface,
        }
    }
}

/// True for paths that are (or belong to) sprites. Upscaling refuses them outright.
pub fn is_sprite_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase().replace('/', "\\");
    p.contains("data\\sprite\\") || p.starts_with("sprite\\") || p.ends_with(".spr") || p.ends_with(".act")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kernel {
    CatmullRom,
    Lanczos3,
}

impl Kernel {
    fn radius(self) -> f32 {
        match self {
            Kernel::CatmullRom => 2.0,
            Kernel::Lanczos3 => 3.0,
        }
    }

    fn weight(self, x: f32) -> f32 {
        let x = x.abs();
        match self {
            // Keys cubic with a = -0.5 (Catmull-Rom).
            Kernel::CatmullRom if x < 1.0 => 1.5 * x * x * x - 2.5 * x * x + 1.0,
            Kernel::CatmullRom if x < 2.0 => -0.5 * x * x * x + 2.5 * x * x - 4.0 * x + 2.0,
            Kernel::Lanczos3 if x < 1e-6 => 1.0,
            Kernel::Lanczos3 if x < 3.0 => {
                let px = std::f32::consts::PI * x;
                3.0 * px.sin() * (px / 3.0).sin() / (px * px)
            }
            _ => 0.0,
        }
    }

    fn id(self) -> u8 {
        match self {
            Kernel::CatmullRom => 1,
            Kernel::Lanczos3 => 2,
        }
    }
}

/// Applies the settings to decoded textures. Cheap to clone.
#[derive(Clone, Debug, Default)]
pub struct Upscaler {
    pub settings: UpscaleSettings,
    cache_dir: Option<PathBuf>,
}

impl Upscaler {
    pub fn new(settings: UpscaleSettings) -> Self {
        let cache_dir = settings.cache.then(|| PathBuf::from("cache").join("upscale"));
        Self { settings, cache_dir }
    }

    /// The load-time factor for this texture, or None to leave it untouched.
    fn plan(&self, class: AssetClass, path: &str, img: &Rgba) -> Option<(Kernel, u32, u32)> {
        // Hard stop for sprites, whatever the caller passed as the class.
        if is_sprite_path(path) {
            debug_assert!(false, "sprite texture passed to the upscaler: {path}");
            tracing::error!(%path, "refusing to upscale a sprite texture");
            return None;
        }
        let s = &self.settings;
        let kernel = s.kernel()?;
        if !s.class_on(class) || img.width == 0 || img.height == 0 {
            return None;
        }
        let lower = path.to_ascii_lowercase();
        if s.exclude.iter().any(|e| !e.is_empty() && lower.contains(&e.to_ascii_lowercase())) {
            return None;
        }
        let longest = img.width.max(img.height) as f32;
        let factor = s.factor.clamp(1.0, 4.0).min(s.max_size as f32 / longest);
        if factor < 1.05 {
            return None;
        }
        Some((kernel, (img.width as f32 * factor).round() as u32, (img.height as f32 * factor).round() as u32))
    }

    /// The texture upscaled according to the settings (or unchanged).
    pub fn apply(&self, class: AssetClass, path: &str, img: Rgba) -> Rgba {
        let Some((kernel, w, h)) = self.plan(class, path, &img) else { return img };
        let key = self.cache_dir.as_ref().map(|dir| dir.join(format!("{:016x}.rup", cache_key(&img, kernel, w, h))));
        if let Some(hit) = key.as_ref().and_then(|k| read_cache(k, w, h)) {
            return hit;
        }
        let out = resample(&img, w, h, kernel);
        if let Some(k) = key {
            write_cache(&k, &out);
        }
        out
    }

    /// `apply` over many textures, spread across the CPU's cores.
    pub fn apply_all(&self, items: Vec<(AssetClass, String, Option<Rgba>)>) -> Vec<Option<Rgba>> {
        self.apply_all_counted(items, &|| {})
    }

    /// `apply_all`, calling `each` as every item is done (from the worker threads; for a
    /// progress bar).
    pub fn apply_all_counted(&self, items: Vec<(AssetClass, String, Option<Rgba>)>, each: &(dyn Fn() + Sync)) -> Vec<Option<Rgba>> {
        if self.settings.kernel().is_none() {
            return items.into_iter().map(|(_, _, img)| img).collect();
        }
        let started = std::time::Instant::now();
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
        let mut slots: Vec<Option<(AssetClass, String, Option<Rgba>)>> = items.into_iter().map(Some).collect();
        let n = slots.len();
        let chunk = n.div_ceil(threads).max(1);
        let mut out: Vec<Option<Rgba>> = Vec::with_capacity(n);
        std::thread::scope(|scope| {
            let handles: Vec<_> = slots
                .chunks_mut(chunk)
                .map(|part| {
                    scope.spawn(move || {
                        part.iter_mut()
                            .map(|s| {
                                let out = s.take().and_then(|(class, path, img)| img.map(|i| self.apply(class, &path, i)));
                                each();
                                out
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            for h in handles {
                out.extend(h.join().unwrap_or_default());
            }
        });
        tracing::info!(textures = n, ms = started.elapsed().as_millis() as u64, quality = ?self.settings.quality, factor = self.settings.factor, "upscaled textures");
        out
    }
}

/// Separable resampling in premultiplied alpha (colour-keyed texels are transparent black,
/// so straight filtering would darken every cut-out edge), clamped at the borders.
fn resample(img: &Rgba, w: u32, h: u32, kernel: Kernel) -> Rgba {
    let (sw, sh) = (img.width as usize, img.height as usize);
    let (w, h) = (w as usize, h as usize);
    let src: Vec<[f32; 4]> = img
        .pixels
        .chunks_exact(4)
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            [p[0] as f32 / 255.0 * a, p[1] as f32 / 255.0 * a, p[2] as f32 / 255.0 * a, a]
        })
        .collect();
    let cols = taps(sw, w, kernel);
    let rows = taps(sh, h, kernel);
    // Horizontal pass: sh rows of w.
    let mut mid = vec![[0.0f32; 4]; sh * w];
    for y in 0..sh {
        let row = &src[y * sw..(y + 1) * sw];
        for (x, t) in cols.iter().enumerate() {
            mid[y * w + x] = t.apply(|i| row[i]);
        }
    }
    // Vertical pass.
    let mut pixels = vec![0u8; w * h * 4];
    for (y, t) in rows.iter().enumerate() {
        for x in 0..w {
            let [r, g, b, a] = t.apply(|i| mid[i * w + x]);
            let a = a.clamp(0.0, 1.0);
            let un = |c: f32| if a > 1e-4 { (c / a).clamp(0.0, 1.0) } else { 0.0 };
            let o = (y * w + x) * 4;
            pixels[o..o + 4].copy_from_slice(&[(un(r) * 255.0 + 0.5) as u8, (un(g) * 255.0 + 0.5) as u8, (un(b) * 255.0 + 0.5) as u8, (a * 255.0 + 0.5) as u8]);
        }
    }
    Rgba { width: w as u32, height: h as u32, pixels }
}

/// One output texel's source taps and weights along an axis.
struct Taps {
    first: usize,
    weights: Vec<f32>,
    len: usize,
    /// Anti-ringing (Lanczos): the result is clamped to the two nearest source texels.
    antiring: Option<(usize, usize)>,
}

impl Taps {
    fn apply(&self, at: impl Fn(usize) -> [f32; 4]) -> [f32; 4] {
        let mut sum = [0.0f32; 4];
        for (k, w) in self.weights.iter().enumerate() {
            let p = at((self.first + k).min(self.len - 1));
            for c in 0..4 {
                sum[c] += p[c] * w;
            }
        }
        if let Some((a, b)) = self.antiring {
            let (pa, pb) = (at(a), at(b));
            for c in 0..4 {
                sum[c] = sum[c].clamp(pa[c].min(pb[c]), pa[c].max(pb[c]));
            }
        }
        sum
    }
}

fn taps(src: usize, dst: usize, kernel: Kernel) -> Vec<Taps> {
    let scale = dst as f32 / src as f32;
    let r = kernel.radius();
    (0..dst)
        .map(|i| {
            let center = (i as f32 + 0.5) / scale - 0.5;
            let lo = (center - r).floor() as isize + 1;
            let hi = (center + r).floor() as isize;
            let mut weights: Vec<f32> = (lo..=hi).map(|j| kernel.weight(center - j as f32)).collect();
            let total: f32 = weights.iter().sum();
            weights.iter_mut().for_each(|w| *w /= total);
            // Taps left of the image repeat its first texel (clamp to edge).
            let lead = (-lo).max(0) as usize;
            let mut folded = vec![0.0; weights.len() - lead.min(weights.len())];
            for (k, w) in weights.iter().enumerate() {
                folded[k.saturating_sub(lead)] += w;
            }
            let near = center.floor().clamp(0.0, (src - 1) as f32) as usize;
            Taps { first: lo.max(0) as usize, weights: folded, len: src, antiring: (kernel == Kernel::Lanczos3).then_some((near, (near + 1).min(src - 1))) }
        })
        .collect()
}

fn cache_key(img: &Rgba, kernel: Kernel, w: u32, h: u32) -> u64 {
    // FNV-1a over the source texels and everything that changes the result.
    let mut hash = 0xcbf29ce484222325u64;
    let mut eat = |b: u8| {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    };
    for b in [1u8, kernel.id()].into_iter().chain(img.width.to_le_bytes()).chain(img.height.to_le_bytes()).chain(w.to_le_bytes()).chain(h.to_le_bytes()) {
        eat(b);
    }
    for &b in &img.pixels {
        eat(b);
    }
    hash
}

const MAGIC: &[u8; 4] = b"RUP1";

fn read_cache(path: &std::path::Path, w: u32, h: u32) -> Option<Rgba> {
    use std::io::Read;
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 || &data[..4] != MAGIC || data[4..8] != w.to_le_bytes() || data[8..12] != h.to_le_bytes() {
        return None;
    }
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    flate2::read::DeflateDecoder::new(&data[12..]).read_to_end(&mut pixels).ok()?;
    (pixels.len() == (w * h * 4) as usize).then_some(Rgba { width: w, height: h, pixels })
}

fn write_cache(path: &std::path::Path, img: &Rgba) {
    use std::io::Write;
    let mut enc = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
    if enc.write_all(&img.pixels).is_err() {
        return;
    }
    let Ok(body) = enc.finish() else { return };
    let mut data = Vec::with_capacity(body.len() + 12);
    data.extend_from_slice(MAGIC);
    data.extend_from_slice(&img.width.to_le_bytes());
    data.extend_from_slice(&img.height.to_le_bytes());
    data.extend_from_slice(&body);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Write then rename, so a crash never leaves a half-written entry.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, data).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(n: u32) -> Rgba {
        let pixels = (0..n * n).flat_map(|i| if (i % n + i / n) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 0] }).collect();
        Rgba { width: n, height: n, pixels }
    }

    #[test]
    fn sprites_are_never_upscaled() {
        let up = Upscaler::new(UpscaleSettings { cache: false, ..Default::default() });
        for path in [r"data\sprite\몬스터\poring.spr", r"data\sprite\인간족\몸통\남\초보자_남.act", "data/sprite/이팩트/torch_01.spr"] {
            assert!(is_sprite_path(path));
            // (In debug builds `apply` asserts; check the plan, which is what gates it.)
            let img = checker(8);
            assert!(std::panic::catch_unwind(|| up.plan(AssetClass::Ground, path, &img)).map_or(true, |p| p.is_none()));
        }
        assert!(!is_sprite_path(r"data\texture\유저인터페이스\item\red_potion.bmp"));
    }

    #[test]
    fn upscales_by_factor_and_cap() {
        let up = Upscaler::new(UpscaleSettings { cache: false, max_size: 24, ..Default::default() });
        let out = up.apply(AssetClass::Model, r"model\x.bmp", checker(8));
        assert_eq!((out.width, out.height), (16, 16));
        let out = up.apply(AssetClass::Model, r"model\x.bmp", checker(16));
        assert_eq!((out.width, out.height), (24, 24));
    }

    #[test]
    fn transparency_does_not_darken_edges() {
        // Opaque white next to colour-keyed (0,0,0,0): every visible texel stays white.
        let up = Upscaler::new(UpscaleSettings { cache: false, quality: Quality::High, ..Default::default() });
        let out = up.apply(AssetClass::Model, "m.bmp", checker(8));
        for p in out.pixels.chunks_exact(4).filter(|p| p[3] > 8) {
            assert!(p[0] > 240 && p[1] > 240 && p[2] > 240, "dark fringe {p:?}");
        }
    }

    #[test]
    fn off_and_fast_leave_textures_alone() {
        for quality in [Quality::Off, Quality::Fast] {
            let up = Upscaler::new(UpscaleSettings { cache: false, quality, ..Default::default() });
            let out = up.apply(AssetClass::Ground, "g.bmp", checker(8));
            assert_eq!(out.width, 8);
        }
    }
}
