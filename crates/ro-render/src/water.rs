//! Water: RSW water cells (or any rectangle) rendered as a displaced, refracting,
//! reflecting surface with depth absorption and shore foam. See docs/water.md.
//!
//! Resources:
//! - per map: a tessellated surface mesh, a depth/shore map baked from the GND heights
//!   (R: water depth, G: distance to shore), the RSW animation frames (classic tint) and an
//!   equirectangular sky map generated from the map's lighting (or supplied by the caller);
//! - shared: a tileable detail texture (RG: normal, B: height, A: foam pattern);
//! - per window size: copies of the opaque scene's colour and depth (refraction, depth,
//!   screen-space reflections), bound as group 2.
//!
//! Every feature is a bit in `WaterSettings::flags()`, read by the shader at run time, so
//! quality changes need no pipeline rebuild (tessellation re-uploads the mesh).

use ro_formats::{gnd::Gnd, image::Rgba, rsw};
use wgpu::util::DeviceExt;

pub const FRAMES: usize = 32;

/// Depth / shore-distance ranges encoded in the shore map (world units; keep in sync with
/// water.wgsl `DEPTH_RANGE` / `SHORE_RANGE`).
pub const DEPTH_RANGE: f32 = 64.0;
pub const SHORE_RANGE: f32 = 128.0;
const DETAIL_SIZE: u32 = 256;
const SKY_SIZE: (u32, u32) = (256, 128);

/// Feature bits (water.wgsl `F_*`).
pub mod flag {
    pub const WAVES: u32 = 1 << 0;
    pub const NORMAL_MAP: u32 = 1 << 1;
    pub const REFRACTION: u32 = 1 << 2;
    pub const REFLECTION: u32 = 1 << 3;
    pub const SSR: u32 = 1 << 4;
    pub const ABSORPTION: u32 = 1 << 5;
    pub const FOAM: u32 = 1 << 6;
    pub const BICUBIC: u32 = 1 << 7;
    pub const CLASSIC: u32 = 1 << 8;
    pub const SPECULAR: u32 = 1 << 9;
    /// Moving light network on the refracted seabed.
    pub const CAUSTICS: u32 = 1 << 10;
    /// Actors standing in the water: contact foam, ripple rings and wakes.
    pub const INTERACT: u32 = 1 << 11;
    /// Molten rock instead of water (RSW water types 4 and 6), set per map by `Water::update`.
    pub const LAVA: u32 = 1 << 12;
}

/// RSW water types whose RO texture is lava: 4 molten, 6 cooling crust with glowing cracks.
pub fn is_lava(kind: i32) -> bool {
    matches!(kind, 4 | 6)
}

/// Ripple rings and wading actors the uniform carries (water.wgsl `MAX_RIPPLES` / `MAX_WADERS`).
pub const MAX_RIPPLES: usize = 48;
pub const MAX_WADERS: usize = 16;
/// Seconds a ripple ring lives (water.wgsl `RIPPLE_LIFE`).
const RIPPLE_LIFE: f32 = 2.2;
/// Deepest the waterline climbs up a wading sprite (sprite.wgsl `WADE_DEPTH`).
const WADE_DEPTH: f32 = 2.6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterQuality {
    /// The original look: RO's animated texture, alpha-blended, one sine wave.
    Classic,
    Low,
    Medium,
    High,
    Ultra,
}

impl WaterQuality {
    pub const ALL: [WaterQuality; 5] = [Self::Classic, Self::Low, Self::Medium, Self::High, Self::Ultra];

    pub fn name(self) -> &'static str {
        match self {
            Self::Classic => "Classic",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }
}

/// Shore foam look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Foam {
    /// 0..1 overall strength.
    pub intensity: f32,
    /// Distance from the shoreline (world units) over which shore foam fades out.
    pub falloff: f32,
    /// Scroll speed of the foam pattern.
    pub speed: f32,
    /// World units per foam pattern repeat.
    pub scale: f32,
    /// 0..1 foam on wave crests.
    pub crest: f32,
    /// Linear RGB.
    pub color: [f32; 3],
}

/// Everything the water draws with; build one from a preset and tweak fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterSettings {
    pub quality: WaterQuality,
    pub waves: bool,
    pub normal_map: bool,
    pub refraction: bool,
    pub reflection: bool,
    pub ssr: bool,
    pub absorption: bool,
    pub foam_enabled: bool,
    pub bicubic: bool,
    pub classic_texture: bool,
    pub specular: bool,
    /// Moving light network on the seabed (needs refraction).
    pub caustics: bool,
    /// Actors disturb the water (rings, wakes, contact foam) and sink into it.
    pub interaction: bool,
    /// Mesh subdivisions per water tile (1..8) for smoother wave displacement.
    pub tessellation: u32,
    /// Screen-space reflection march steps.
    pub ssr_steps: u32,
    /// World distance a reflected ray is marched.
    pub ssr_distance: f32,
    /// Screen-space refraction offset (fraction of the screen per unit normal tilt).
    pub refraction_strength: f32,
    /// 0..1 scale on the Fresnel reflection.
    pub reflection_strength: f32,
    /// Beer-Lambert extinction per world unit (linear RGB): red dies first.
    pub absorption_coeff: [f32; 3],
    /// Colour light takes on scattering inside deep water (linear RGB).
    pub scatter_color: [f32; 3],
    /// Multiplier on the depth used for absorption (stylises how fast water darkens).
    pub depth_scale: f32,
    /// 0..1 blend of RO's original animated texture into the water colour.
    pub classic_mix: f32,
    pub foam: Foam,
    /// Ripple normal strength, world units per detail repeat, and scroll speed.
    pub normal_strength: f32,
    pub normal_tiling: f32,
    pub normal_speed: f32,
    /// Gerstner swell: height (world units), wavelength, steepness 0..1, speed.
    pub wave_amplitude: f32,
    pub wave_length: f32,
    pub wave_steepness: f32,
    pub wave_speed: f32,
    /// Sun highlight sharpness and brightness.
    pub shininess: f32,
    pub sun_intensity: f32,
}

impl WaterSettings {
    pub fn preset(q: WaterQuality) -> Self {
        let base = Self {
            quality: q,
            waves: true,
            normal_map: true,
            refraction: true,
            reflection: true,
            ssr: true,
            absorption: true,
            foam_enabled: true,
            bicubic: true,
            classic_texture: true,
            specular: true,
            caustics: true,
            interaction: true,
            tessellation: 4,
            ssr_steps: 24,
            ssr_distance: 160.0,
            refraction_strength: 0.035,
            reflection_strength: 1.0,
            absorption_coeff: [0.060, 0.022, 0.014],
            scatter_color: [0.020, 0.090, 0.110],
            depth_scale: 1.0,
            classic_mix: 0.25,
            foam: Foam { intensity: 0.9, falloff: 14.0, speed: 0.6, scale: 28.0, crest: 0.25, color: [0.92, 0.95, 0.97] },
            normal_strength: 0.28,
            normal_tiling: 42.0,
            normal_speed: 0.9,
            wave_amplitude: 0.9,
            wave_length: 58.0,
            wave_steepness: 0.45,
            wave_speed: 1.0,
            shininess: 900.0,
            sun_intensity: 1.4,
        };
        match q {
            WaterQuality::Classic => Self {
                waves: false,
                normal_map: false,
                refraction: false,
                reflection: false,
                ssr: false,
                absorption: false,
                foam_enabled: false,
                bicubic: false,
                specular: false,
                caustics: false,
                interaction: false,
                classic_texture: true,
                classic_mix: 1.0,
                tessellation: 1,
                ..base
            },
            WaterQuality::Low => Self { refraction: false, caustics: false, ssr: false, bicubic: false, tessellation: 1, ssr_steps: 0, foam: Foam { crest: 0.0, ..base.foam }, ..base },
            WaterQuality::Medium => Self { ssr: false, bicubic: false, tessellation: 2, ..base },
            WaterQuality::High => Self { ssr_steps: 16, tessellation: 3, ..base },
            WaterQuality::Ultra => Self { ssr_steps: 32, ssr_distance: 220.0, tessellation: 6, ..base },
        }
    }

    pub fn flags(&self) -> u32 {
        let mut f = 0;
        for (on, bit) in [
            (self.waves, flag::WAVES),
            (self.normal_map, flag::NORMAL_MAP),
            (self.refraction, flag::REFRACTION),
            (self.reflection, flag::REFLECTION),
            (self.ssr && self.ssr_steps > 0, flag::SSR),
            (self.absorption, flag::ABSORPTION),
            (self.foam_enabled, flag::FOAM),
            (self.bicubic, flag::BICUBIC),
            (self.classic_texture, flag::CLASSIC),
            (self.specular, flag::SPECULAR),
            (self.caustics, flag::CAUSTICS),
            (self.interaction, flag::INTERACT),
        ] {
            if on {
                f |= bit;
            }
        }
        f
    }

    /// Whether the water reads the opaque scene (colour/depth copies are made each frame).
    pub fn uses_screen(&self) -> bool {
        self.refraction || (self.ssr && self.ssr_steps > 0)
    }
}

impl Default for WaterSettings {
    fn default() -> Self {
        Self::preset(WaterQuality::High)
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// x, z in world units.
    pub pos: [f32; 2],
    /// Tile coordinates (classic wave phase and texture).
    pub tile: [f32; 2],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Self>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &Self::ATTRS }
    }
}

/// The per-frame water uniform (water.wgsl `Water`).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Uniform {
    /// level (y-up), RSW wave height, RSW wave speed (deg/tick), RSW wave pitch (deg/tile)
    level: [f32; 4],
    /// frames per texture step, map width / height (world units), world units per tile
    map: [f32; 4],
    /// feature bits, SSR steps, 0, 0
    flags: [u32; 4],
    /// absorption rgb, depth scale
    absorb: [f32; 4],
    /// scatter rgb, classic mix
    scatter: [f32; 4],
    /// foam rgb, intensity
    foam_color: [f32; 4],
    /// falloff, speed, scale, crest
    foam: [f32; 4],
    /// refraction, reflection, SSR distance, shininess
    optics: [f32; 4],
    /// normal strength, tiling, speed, 0
    normal: [f32; 4],
    /// amplitude, wavelength, steepness, speed
    waves: [f32; 4],
    /// mean sky radiance rgb, sky brightness now (night dims it)
    sky: [f32; 4],
    /// sun rgb, sun intensity
    sun: [f32; 4],
    /// x, z, birth time (s), strength
    ripples: [[f32; 4]; MAX_RIPPLES],
    /// x, z, depth at the feet, 1 when walking
    waders: [[f32; 4]; MAX_WADERS],
}

/// A water surface ready to upload: tiles below the water level and the baked shore map.
pub struct WaterMesh {
    /// Water tiles (x, y) in GND cells.
    pub tiles: Vec<(u32, u32)>,
    pub water: rsw::Water,
    /// World units per tile.
    pub zoom: f32,
    /// Map size in tiles.
    pub size: (u32, u32),
    /// (size.0+1) x (size.1+1) corners: RGBA8 with R depth / DEPTH_RANGE, G distance to
    /// shore / SHORE_RANGE.
    pub shore: Rgba,
    /// `data\texture\` relative paths of the animation frames.
    pub textures: Vec<String>,
}

impl WaterMesh {
    /// Classic RSW water: every GND tile whose ground dips below the level.
    pub fn build(gnd: &Gnd, water: &rsw::Water) -> Self {
        let level = -water.level;
        let mut tiles = Vec::new();
        for y in 0..gnd.height {
            for x in 0..gnd.width {
                let c = gnd.cube(x, y).unwrap();
                if c.heights.iter().any(|&h| -h < level) {
                    tiles.push((x, y));
                }
            }
        }
        let shore = shore_map(gnd, level);
        let textures = (0..FRAMES).map(|i| format!("워터\\water{}{i:02}.jpg", water.kind)).collect();
        Self { tiles, water: *water, zoom: gnd.zoom, size: (gnd.width, gnd.height), shore, textures }
    }

    /// A large body of water over a tile rectangle (x0..x1, y0..y1), for custom maps: the
    /// GND still supplies depth and shore distances.
    pub fn rect(gnd: &Gnd, water: &rsw::Water, x0: u32, y0: u32, x1: u32, y1: u32) -> Self {
        let mut m = Self::build(gnd, water);
        m.tiles = (y0..y1.min(gnd.height)).flat_map(|y| (x0..x1.min(gnd.width)).map(move |x| (x, y))).collect();
        m
    }

    /// Triangles with each tile split `n` x `n` (more vertices for smoother swell).
    fn vertices(&self, n: u32) -> Vec<Vertex> {
        let n = n.clamp(1, 8);
        let step = 1.0 / n as f32;
        let mut out = Vec::with_capacity(self.tiles.len() * (n * n * 6) as usize);
        for &(x, y) in &self.tiles {
            for j in 0..n {
                for i in 0..n {
                    let v = |di: u32, dj: u32| {
                        let tx = x as f32 + (i + di) as f32 * step;
                        let ty = y as f32 + (j + dj) as f32 * step;
                        Vertex { pos: [tx * self.zoom, ty * self.zoom], tile: [tx, ty] }
                    };
                    out.extend([v(0, 0), v(1, 0), v(1, 1), v(0, 0), v(1, 1), v(0, 1)]);
                }
            }
        }
        out
    }
}

/// Bakes water depth at each GND corner and the distance from each corner to dry land
/// (two-pass chamfer transform), so the shader knows true depth everywhere.
fn shore_map(gnd: &Gnd, level: f32) -> Rgba {
    let (w, h) = (gnd.width as usize + 1, gnd.height as usize + 1);
    let mut depth = vec![0.0f32; w * h];
    let mut seen = vec![0u8; w * h];
    // Corner order follows Surface::u: (x, y), (x+1, y), (x, y+1), (x+1, y+1).
    for y in 0..gnd.height as usize {
        for x in 0..gnd.width as usize {
            let c = gnd.cube(x as u32, y as u32).unwrap();
            for (k, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                let i = (y + dy) * w + x + dx;
                let d = level + c.heights[k]; // y-up level minus y-up ground (-h)
                depth[i] = if seen[i] == 0 { d } else { depth[i].min(d) };
                seen[i] = 1;
            }
        }
    }
    let cell = gnd.zoom;
    let mut dist: Vec<f32> = depth.iter().map(|&d| if d <= 0.0 { 0.0 } else { f32::MAX / 4.0 }).collect();
    let diag = cell * std::f32::consts::SQRT_2;
    let at = |x: isize, y: isize| -> Option<usize> { (x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h).then(|| y as usize * w + x as usize) };
    for y in 0..h as isize {
        for x in 0..w as isize {
            let i = at(x, y).unwrap();
            for (dx, dy, c) in [(-1, 0, cell), (0, -1, cell), (-1, -1, diag), (1, -1, diag)] {
                if let Some(j) = at(x + dx, y + dy) {
                    dist[i] = dist[i].min(dist[j] + c);
                }
            }
        }
    }
    for y in (0..h as isize).rev() {
        for x in (0..w as isize).rev() {
            let i = at(x, y).unwrap();
            for (dx, dy, c) in [(1, 0, cell), (0, 1, cell), (1, 1, diag), (-1, 1, diag)] {
                if let Some(j) = at(x + dx, y + dy) {
                    dist[i] = dist[i].min(dist[j] + c);
                }
            }
        }
    }
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let mut pixels = Vec::with_capacity(w * h * 4);
    for i in 0..w * h {
        pixels.extend([byte(depth[i] / DEPTH_RANGE), byte(dist[i] / SHORE_RANGE), 0, 255]);
    }
    Rgba { width: w as u32, height: h as u32, pixels }
}

/// Debugging aid: `RO_WATER_OFF=<bits>` (decimal or 0x hex) clears feature bits, e.g.
/// `RO_WATER_OFF=0x200` hides the sun highlight. See `flag`.
fn debug_mask() -> u32 {
    static MASK: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MASK.get_or_init(|| {
        let v = std::env::var("RO_WATER_OFF").unwrap_or_default();
        v.strip_prefix("0x").map_or_else(|| v.parse().ok(), |h| u32::from_str_radix(h, 16).ok()).unwrap_or(0)
    })
}

/// Small deterministic hash for the procedural textures.
fn hash(x: u32, y: u32, s: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ s.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

/// The shared detail texture: a tileable ripple field built from integer-frequency waves
/// (RG: normal, B: height) and a tileable cellular foam pattern (A).
pub fn detail_texture() -> Rgba {
    let n = DETAIL_SIZE as usize;
    let tau = std::f32::consts::TAU;
    // Integer frequencies keep every wave periodic over the tile.
    let mut waves = vec![];
    for k in 0..28u32 {
        let fx = (hash(k, 1, 7) * 13.0) as i32 - 6;
        let fy = (hash(k, 2, 7) * 13.0) as i32 - 6;
        if fx == 0 && fy == 0 {
            continue;
        }
        let f = ((fx * fx + fy * fy) as f32).sqrt();
        waves.push((fx as f32, fy as f32, 1.0 / f.powf(1.4), hash(k, 3, 7) * tau));
    }
    let cells = 8usize;
    let points: Vec<(f32, f32)> = (0..cells * cells).map(|i| (hash(i as u32, 4, 9), hash(i as u32, 5, 9))).collect();
    let mut height = vec![0.0f32; n * n];
    let mut grad = vec![(0.0f32, 0.0f32); n * n];
    let mut foam = vec![0.0f32; n * n];
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            let (mut hgt, mut gx, mut gy) = (0.0, 0.0, 0.0);
            for &(fx, fy, a, p) in &waves {
                let ph = tau * (fx * u + fy * v) + p;
                hgt += a * ph.sin();
                gx += a * tau * fx * ph.cos();
                gy += a * tau * fy * ph.cos();
            }
            let i = y * n + x;
            height[i] = hgt;
            grad[i] = (gx, gy);
            lo = lo.min(hgt);
            hi = hi.max(hgt);
            // Cellular: distance to the nearest two feature points (wrapping).
            let (cu, cv) = (u * cells as f32, v * cells as f32);
            let (mut d1, mut d2) = (9.0f32, 9.0f32);
            for oy in -1..=1i32 {
                for ox in -1..=1i32 {
                    let (gxc, gyc) = (cu.floor() as i32 + ox, cv.floor() as i32 + oy);
                    let (wx, wy) = (gxc.rem_euclid(cells as i32) as usize, gyc.rem_euclid(cells as i32) as usize);
                    let (px, py) = points[wy * cells + wx];
                    let d = ((gxc as f32 + px - cu).powi(2) + (gyc as f32 + py - cv).powi(2)).sqrt();
                    if d < d1 {
                        d2 = d1;
                        d1 = d;
                    } else if d < d2 {
                        d2 = d;
                    }
                }
            }
            // Bright lines along cell borders, broken up a little.
            foam[i] = (1.0 - ((d2 - d1) * 3.2).clamp(0.0, 1.0)).powf(1.6);
        }
    }
    let gscale = grad.iter().map(|g| g.0.abs().max(g.1.abs())).fold(0.0f32, f32::max).max(1e-3);
    let mut pixels = Vec::with_capacity(n * n * 4);
    for i in 0..n * n {
        let (gx, gy) = grad[i];
        let enc = |v: f32| ((v * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        let h = (height[i] - lo) / (hi - lo).max(1e-3);
        pixels.extend([enc(-gx / gscale), enc(-gy / gscale), (h * 255.0) as u8, (foam[i] * 255.0) as u8]);
    }
    Rgba { width: DETAIL_SIZE, height: DETAIL_SIZE, pixels }
}

/// An equirectangular sky (u: azimuth, v: 0 zenith .. 1 nadir) from the map's lighting, and
/// its cosine-weighted mean radiance over the upper hemisphere (linear RGB).
pub fn sky_map(lighting: &crate::Lighting, profile: Option<&crate::SkyProfile>) -> (Rgba, [f32; 3]) {
    let (w, h) = SKY_SIZE;
    let sun = -glam::Vec3::from(lighting.direction).normalize_or(glam::Vec3::new(0.5, -0.7, 0.5));
    let amb = glam::Vec3::from(lighting.ambient);
    let dif = glam::Vec3::from(lighting.diffuse);
    let zenith = profile.map_or_else(|| glam::Vec3::new(0.16, 0.30, 0.58).lerp(amb * 0.8, 0.25), |p| p.zenith.into());
    let horizon = profile.map_or_else(|| glam::Vec3::new(0.62, 0.72, 0.82).lerp(dif, 0.2), |p| p.horizon.into());
    let cloud_color = profile.map_or_else(|| glam::Vec3::splat(0.85) * (amb + dif * 0.5).min(glam::Vec3::ONE), |p| p.cloud.into());
    let cloud_scale = profile.map_or(2.5, |p| p.cloud_scale * 2.5);
    let cloud_cover = profile.map_or(0.45, |p| p.cloud_cover);
    let seed = profile.map_or(0.0, |p| p.seed);
    let ground = amb * 0.22 + glam::Vec3::new(0.02, 0.03, 0.03);
    let mut pixels = Vec::with_capacity((w * h * 4) as usize);
    let (mut sum, mut wsum) = (glam::Vec3::ZERO, 0.0f32);
    let pi = std::f32::consts::PI;
    for y in 0..h {
        let theta = (y as f32 + 0.5) / h as f32 * pi; // 0 zenith
        for x in 0..w {
            let phi = (x as f32 + 0.5) / w as f32 * std::f32::consts::TAU;
            let d = glam::Vec3::new(theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin());
            let up = d.y;
            let mut c = if up >= 0.0 { horizon.lerp(zenith, up.powf(0.6)) } else { horizon.lerp(ground, (-up).powf(0.4)) };
            if up > 0.0 {
                // Soft clouds: smooth noise over the projected sky plane.
                let (px, pz) = (d.x / (up + 0.15) * cloud_scale + seed * 0.13, d.z / (up + 0.15) * cloud_scale + seed * 0.21);
                let cloud = value_noise(px, pz) * 0.6 + value_noise(px * 2.3, pz * 2.3) * 0.4;
                let cover = ((cloud - (1.0 - cloud_cover)) * 3.0).clamp(0.0, 1.0) * up.powf(0.3);
                c = c.lerp(cloud_color, cover * 0.7);
                let s = d.dot(sun).max(0.0);
                c += dif * (s.powf(1200.0) * 30.0 + s.powf(24.0) * 0.35);
                sum += c * up * theta.sin();
                wsum += up * theta.sin();
            }
            let enc = |v: f32| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8;
            pixels.extend([enc(c.x), enc(c.y), enc(c.z), 255]);
        }
    }
    let mean = sum / wsum.max(1e-3);
    (Rgba { width: w, height: h, pixels }, [mean.x, mean.y, mean.z])
}

/// Mean radiance of a caller-supplied equirectangular sky (sRGB, top half = sky).
pub fn mean_radiance(img: &Rgba) -> [f32; 3] {
    let (mut sum, mut wsum) = ([0.0f32; 3], 0.0f32);
    let pi = std::f32::consts::PI;
    for y in 0..img.height / 2 {
        let theta = (y as f32 + 0.5) / img.height as f32 * pi;
        let wgt = theta.cos() * theta.sin();
        for x in 0..img.width {
            let p = ((y * img.width + x) * 4) as usize;
            for c in 0..3 {
                sum[c] += srgb_to_linear(img.pixels[p + c] as f32 / 255.0) * wgt;
            }
            wsum += wgt;
        }
    }
    sum.map(|s| s / wsum.max(1e-3))
}

fn value_noise(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let s = |a: f32| a * a * (3.0 - 2.0 * a);
    let h = |dx: f32, dy: f32| hash((ix + dx) as i32 as u32, (iy + dy) as i32 as u32, 11);
    let a = h(0.0, 0.0) + (h(1.0, 0.0) - h(0.0, 0.0)) * s(fx);
    let b = h(0.0, 1.0) + (h(1.0, 1.0) - h(0.0, 1.0)) * s(fx);
    a + (b - a) * s(fy)
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Uploads an image as a (mipmapped) texture in the given format (RGBA8 data).
fn texture(device: &wgpu::Device, queue: &wgpu::Queue, img: &Rgba, format: wgpu::TextureFormat, mips: bool) -> wgpu::TextureView {
    let mut levels = vec![];
    let mut cur = Some(Rgba { width: img.width, height: img.height, pixels: img.pixels.clone() });
    while let Some(l) = cur {
        cur = if mips { l.downsample() } else { None };
        levels.push(l);
    }
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("water texture"),
        size: wgpu::Extent3d { width: img.width, height: img.height, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (mip, l) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: mip as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &l.pixels,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.width * 4), rows_per_image: Some(l.height) },
            wgpu::Extent3d { width: l.width, height: l.height, depth_or_array_layers: 1 },
        );
    }
    tex.create_view(&Default::default())
}

/// Bind group layouts: group 1 per map, group 2 the screen copies.
pub(crate) fn layouts(device: &wgpu::Device) -> (wgpu::BindGroupLayout, wgpu::BindGroupLayout) {
    let tex = |binding, filterable, vis| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: vis,
        ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
        count: None,
    };
    let samp = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    };
    let f = wgpu::ShaderStages::FRAGMENT;
    let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
    let map = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("water map"),
        entries: &[
            tex(0, true, f),
            samp(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: vf,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            },
            tex(3, true, f),
            tex(4, true, vf),
            tex(5, true, f),
            samp(6),
        ],
    });
    let screen = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("water screen"),
        entries: &[tex(0, true, f), tex(1, true, f)],
    });
    (map, screen)
}

/// Opaque-scene copies the water samples, sized with the window.
pub(crate) struct ScreenCopies {
    pub color: wgpu::Texture,
    pub depth: wgpu::Texture,
    pub bind: wgpu::BindGroup,
}

impl ScreenCopies {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, w: u32, h: u32) -> Self {
        let make = |label, format| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let color = make("water scene colour", crate::post::HDR_FORMAT);
        let depth = make("water scene depth", crate::post::INFO_FORMAT);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water screen"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&color.create_view(&Default::default())) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth.create_view(&Default::default())) },
            ],
        });
        Self { color, depth, bind }
    }

    /// Copies the resolved opaque scene (colour + linear depth) for the water to sample.
    pub fn capture(&self, enc: &mut wgpu::CommandEncoder, hdr: &wgpu::Texture, info: &wgpu::Texture) {
        let size = self.color.size();
        enc.copy_texture_to_texture(hdr.as_image_copy(), self.color.as_image_copy(), size);
        enc.copy_texture_to_texture(info.as_image_copy(), self.depth.as_image_copy(), size);
    }
}

pub(crate) struct Water {
    mesh: WaterMesh,
    pub vertices: wgpu::Buffer,
    pub count: u32,
    tessellation: u32,
    uniform: wgpu::Buffer,
    pub frames: Vec<wgpu::BindGroup>,
    pub anim_speed: f32,
    mean_sky: [f32; 3],
    ripples: Vec<[f32; 4]>,
    waders: Vec<[f32; 4]>,
}

impl Water {
    #[allow(clippy::too_many_arguments)]
    pub fn upload(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        repeat: &wgpu::Sampler,
        clamp: &wgpu::Sampler,
        detail: &wgpu::TextureView,
        mesh: &WaterMesh,
        images: Vec<Option<Rgba>>,
        sky: (&Rgba, [f32; 3]),
        settings: &WaterSettings,
    ) -> Option<Self> {
        if mesh.tiles.is_empty() {
            return None;
        }
        let shore = texture(device, queue, &mesh.shore, wgpu::TextureFormat::Rgba8Unorm, false);
        // Kept for re-tessellation; the shore image is already on the GPU.
        let mesh = WaterMesh { tiles: mesh.tiles.clone(), water: mesh.water, zoom: mesh.zoom, size: mesh.size, shore: Rgba { width: 1, height: 1, pixels: vec![0; 4] }, textures: vec![] };
        let tessellation = settings.tessellation.clamp(1, 8);
        let verts = mesh.vertices(tessellation);
        let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("water"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water uniform"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sky_view = texture(device, queue, sky.0, wgpu::TextureFormat::Rgba8UnormSrgb, true);
        let fallback = Rgba { width: 1, height: 1, pixels: vec![40, 80, 140, 255] };
        let frames = images
            .into_iter()
            .map(|img| {
                let view = crate::scene::upload_texture(device, queue, img.as_ref().unwrap_or(&fallback), true);
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("water frame"),
                    layout,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(repeat) },
                        wgpu::BindGroupEntry { binding: 2, resource: uniform.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(detail) },
                        wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&shore) },
                        wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&sky_view) },
                        wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::Sampler(clamp) },
                    ],
                })
            })
            .collect();
        let anim_speed = (mesh.water.anim_speed.max(1)) as f32;
        Some(Self { mesh, vertices, count: verts.len() as u32, tessellation, uniform, frames, anim_speed, mean_sky: sky.1, ripples: vec![], waders: vec![] })
    }

    /// Rebuilds the mesh when the tessellation setting changes.
    pub fn retessellate(&mut self, device: &wgpu::Device, n: u32) {
        let n = n.clamp(1, 8);
        if n == self.tessellation {
            return;
        }
        let verts = self.mesh.vertices(n);
        self.vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("water"), contents: bytemuck::cast_slice(&verts), usage: wgpu::BufferUsages::VERTEX });
        self.count = verts.len() as u32;
        self.tessellation = n;
    }

    /// The water surface height (y-up).
    pub fn level(&self) -> f32 {
        -self.mesh.water.level
    }

    /// Tracks actors standing in the water (feet, walking) at `now` on the shader clock:
    /// the nearest to `focus` become waders (contact foam), and each sheds ripple rings, a
    /// trail of them while walking (the wake) and a slow pulse while standing.
    ///
    /// Seabeds often lie far below the surface, and sprites only sink to `WADE_DEPTH`, so
    /// each actor's disturbance is centred where the line from the eye through its visible
    /// waterline meets the surface: on screen the rings sit at the sprite, not above it.
    pub fn wade(&mut self, actors: impl Iterator<Item = (glam::Vec3, bool)>, focus: glam::Vec3, eye: glam::Vec3, now: f32) {
        let level = self.level();
        self.ripples.retain(|r| now - r[2] < RIPPLE_LIFE);
        let mut wet: Vec<(glam::Vec3, bool)> = actors.filter(|(p, _)| level - p.y > 0.3).collect();
        wet.sort_by(|a, b| a.0.distance_squared(focus).total_cmp(&b.0.distance_squared(focus)));
        wet.truncate(MAX_WADERS);
        let on_surface = |p: glam::Vec3| {
            let line = p + glam::Vec3::Y * (level - p.y).min(WADE_DEPTH);
            let ray = eye - line;
            if ray.y > 0.1 { line + ray * ((level - line.y) / ray.y) } else { p }
        };
        let wet: Vec<(glam::Vec3, bool, f32)> = wet.into_iter().map(|(p, walking)| (on_surface(p), walking, level - p.y)).collect();
        self.waders = wet.iter().map(|(p, walking, depth)| [p.x, p.z, *depth, if *walking { 1.0 } else { 0.0 }]).collect();
        for (p, walking, _) in wet {
            let near = |radius: f32, age: f32| self.ripples.iter().any(|r| now - r[2] < age && glam::Vec2::new(r[0] - p.x, r[1] - p.z).length() < radius);
            if if walking { near(2.8, RIPPLE_LIFE) } else { near(1.0, 1.1) } {
                continue;
            }
            if self.ripples.len() >= MAX_RIPPLES {
                self.ripples.remove(0);
            }
            self.ripples.push([p.x, p.z, now, if walking { 1.0 } else { 0.6 }]);
        }
    }

    /// Molten rock: lit from within, with no reflections, foam or wading (the Classic preset
    /// keeps RO's own texture).
    pub fn lava(&self) -> bool {
        is_lava(self.mesh.water.kind)
    }

    /// Writes this frame's uniform. `sky_scale` dims reflections with the night.
    pub fn update(&self, queue: &wgpu::Queue, s: &WaterSettings, sun: [f32; 3], sky_scale: f32) {
        let w = &self.mesh.water;
        let mut flags = s.flags() & !debug_mask();
        let mut waves = [s.wave_amplitude, s.wave_length.max(4.0), s.wave_steepness.clamp(0.0, 1.0), s.wave_speed];
        if self.lava() && s.normal_map {
            use flag::*;
            flags = (flags & !(REFLECTION | SSR | ABSORPTION | FOAM | SPECULAR | CAUSTICS | INTERACT)) | LAVA;
            // A thick, slow swell.
            waves = [waves[0] * 0.25, waves[1] * 1.6, waves[2] * 0.3, waves[3] * 0.2];
        }
        let u = Uniform {
            level: [-w.level, w.wave_height, w.wave_speed, w.wave_pitch],
            map: [self.anim_speed, self.mesh.size.0 as f32 * self.mesh.zoom, self.mesh.size.1 as f32 * self.mesh.zoom, self.mesh.zoom],
            flags: [flags, s.ssr_steps.min(64), self.ripples.len() as u32, self.waders.len() as u32],
            absorb: [s.absorption_coeff[0], s.absorption_coeff[1], s.absorption_coeff[2], s.depth_scale],
            scatter: [s.scatter_color[0], s.scatter_color[1], s.scatter_color[2], s.classic_mix],
            foam_color: [s.foam.color[0], s.foam.color[1], s.foam.color[2], s.foam.intensity],
            foam: [s.foam.falloff.max(0.01), s.foam.speed, s.foam.scale.max(1.0), s.foam.crest],
            optics: [s.refraction_strength, s.reflection_strength, s.ssr_distance, s.shininess],
            normal: [s.normal_strength, s.normal_tiling.max(1.0), s.normal_speed, 0.0],
            waves,
            sky: [self.mean_sky[0], self.mean_sky[1], self.mean_sky[2], sky_scale],
            sun: [sun[0], sun[1], sun[2], s.sun_intensity],
            ripples: {
                let mut out = [[0.0; 4]; MAX_RIPPLES];
                out[..self.ripples.len()].copy_from_slice(&self.ripples);
                out
            },
            waders: {
                let mut out = [[0.0; 4]; MAX_WADERS];
                out[..self.waders.len()].copy_from_slice(&self.waders);
                out
            },
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&u));
    }

    /// Texture frame for a time in seconds (the original animates at 60 ticks/s).
    pub fn frame(&self, time: f32) -> usize {
        ((time * 60.0 / self.anim_speed) as usize) % self.frames.len().max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("water.wgsl")).unwrap_or_else(|e| panic!("{}", e.emit_to_string(include_str!("water.wgsl"))));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).expect("water.wgsl validates");
    }

    #[test]
    fn uniform_matches_shader_size() {
        // 11 vec4<f32> + 1 vec4<u32> + the ripple and wader arrays, as in water.wgsl `Water`.
        assert_eq!(std::mem::size_of::<Uniform>(), (12 + MAX_RIPPLES + MAX_WADERS) * 16);
    }

    #[test]
    fn presets_scale_up() {
        let bits = |q| WaterSettings::preset(q).flags().count_ones();
        assert!(bits(WaterQuality::Classic) < bits(WaterQuality::Low));
        assert!(bits(WaterQuality::Low) < bits(WaterQuality::High));
        assert!(!WaterSettings::preset(WaterQuality::Low).uses_screen());
        assert!(WaterSettings::preset(WaterQuality::Ultra).uses_screen());
    }

    #[test]
    fn reflection_map_uses_the_outdoor_palette_in_linear_light() {
        // Remove sunlight/cloud highlights so the test measures the supplied palette.
        let light = crate::Lighting { ambient: [0.0; 3], diffuse: [0.0; 3], ..Default::default() };
        let mut red = crate::SkyProfile::outdoor("custom_sky", &light);
        red.zenith = [0.6, 0.03, 0.01];
        red.horizon = red.zenith;
        red.cloud_cover = 0.0;
        let (image, mean) = sky_map(&light, Some(&red));
        assert!(mean[0] > mean[2] * 20.0);
        let decoded = mean_radiance(&image);
        for channel in 0..3 {
            assert!((decoded[channel] - mean[channel]).abs() < 0.01, "sky texture and radiance must agree");
        }
        let mut blue = red;
        blue.zenith.reverse();
        blue.horizon = blue.zenith;
        let (_, mean) = sky_map(&light, Some(&blue));
        assert!(mean[2] > mean[0] * 20.0);
    }

    #[test]
    fn detail_texture_tiles() {
        let t = detail_texture();
        let n = t.width as usize;
        // Opposite edges match closely (it wraps).
        let px = |x: usize, y: usize| t.pixels[(y * n + x) * 4 + 2] as i32;
        let worst = (0..n).map(|y| (px(0, y) - px(n - 1, y)).abs()).max().unwrap();
        assert!(worst < 40, "edge mismatch {worst}");
    }
}
