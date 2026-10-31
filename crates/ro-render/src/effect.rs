//! Code-drawn 3D effects: the warp portal (counter-rotating cylinders textured with
//! effect\ring_blue.tga) and skill effects the original client also builds in code — light
//! columns (Heal, Increase AGI, Blessing) and spinning ground circles (casting). All are
//! additively blended and rebuilt each frame (a few hundred vertices).

use glam::Vec3;

use crate::scene::Heights;
use crate::CELL;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// RGB tint and strength.
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32x4];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

const SEGMENTS: usize = 32;

/// An open cylinder (cone frustum) whose texture scrolls around it by `spin` turns.
fn cylinder(out: &mut Vec<Vertex>, base: Vec3, bottom: f32, top: f32, height: f32, spin: f32, color: [f32; 4]) {
    for i in 0..SEGMENTS {
        let (a0, a1) = (i as f32 / SEGMENTS as f32, (i + 1) as f32 / SEGMENTS as f32);
        let ring = |t: f32, r: f32, y: f32| {
            let ang = t * std::f32::consts::TAU;
            base + Vec3::new(ang.cos() * r, y, ang.sin() * r)
        };
        // The texture wraps twice around the cylinder.
        let (u0, u1) = (a0 * 2.0 + spin, a1 * 2.0 + spin);
        let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
        let (b0, b1, t0, t1) = (ring(a0, bottom, 0.0), ring(a1, bottom, 0.0), ring(a0, top, height), ring(a1, top, height));
        out.extend([v(b0, u0, 1.0), v(b1, u1, 1.0), v(t1, u1, 0.0), v(b0, u0, 1.0), v(t1, u1, 0.0), v(t0, u0, 0.0)]);
    }
}

/// Geometry for warp portals at `positions` (ground points) at time `t` seconds.
pub fn warp_portals(positions: &[Vec3], t: f32) -> Vec<Vertex> {
    let mut out = Vec::with_capacity(positions.len() * SEGMENTS * 12);
    for &p in positions {
        cylinder(&mut out, p, 1.7 * CELL, 2.2 * CELL, 1.4 * CELL, t * 0.25, [1.0, 1.0, 1.0, 0.9]);
        cylinder(&mut out, p, 1.1 * CELL, 1.5 * CELL, 2.6 * CELL, -t * 0.4, [1.0, 1.0, 1.0, 0.7]);
    }
    out
}

/// A texture uploaded for code-drawn effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexId(pub usize);

#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// A column of light that shoots up and fades (radius and full height in cells).
    Column { radius: f32, height: f32 },
    /// A flat circle on the ground, turning (radius in cells, turns per second).
    Disc { radius: f32, spin: f32 },
    /// Spikes erupting in a wave along `reach` (world offset from `pos`), like Grimtooth.
    Spikes { reach: Vec3, count: u32 },
    /// A shockwave wall expanding to `radius` cells.
    Wave { radius: f32 },
    /// A glowing ball flying along `reach` and bursting at its end.
    Orb { reach: Vec3 },
    /// A camera-facing crescent (effect\purpleslash.tga) flying along `travel`, its curved
    /// edge leading, growing from `size.0` to `size.1` cells (Soul Breaker, Meteor Assault).
    Slash { travel: Vec3, size: (f32, f32) },
    /// A camera-facing patch of motes (effect\pok1.bmp) drifting along `drift` and slowly
    /// turning, `size` cells across (map fireflies).
    Motes { drift: Vec3, size: f32 },
    /// A flat rectangle on the ground, `size` cells across (skill areas being cast).
    Area { size: (f32, f32) },
    /// One layer of the level 99 aura, held under its owner while it stands in view
    /// (`time_ms` runs from when it appeared; `seed` varies the sparkles between owners).
    Aura { layer: AuraLayer, seed: u32, style: AuraStyle },
    /// One layer of a Mortal Flame (Tenseiga) skill effect, over the effect's whole
    /// duration (layers that come in late wait inside it). Rings, sparks and shards take
    /// the effect's colour; slashes and souls carry Tenseiga's own. `solid` layers paint
    /// true colour (alpha-blended); the others add a narrower glowing core over them.
    Tenseiga { part: TenseigaPart, solid: bool },
    /// Crescent Gale: a crescent of mortal flame flying from `pos` along `travel`, then
    /// breaking up; `solid` as for `Tenseiga`.
    Crescent { travel: Vec3, solid: bool },
}

/// The pieces the Mortal Flame skills' effects are built from. Streaks and rings use
/// textures the client generates (a soft line, a soft ring); the rest a soft glow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TenseigaPart {
    /// Severing Heaven's Lies: a crescent cut swept across the target, crimson at its tail
    /// and cyan at its leading edge (streak).
    Arc,
    /// Severing Heaven's Lies: a golden halo over the target that breaks in two and falls
    /// away (ring).
    Halo,
    /// Light and Shadow: the cut of light, gold-white, top left to bottom right (streak).
    CutLight,
    /// Light and Shadow: the cut of shadow, crimson, crossing it a moment later (streak).
    CutShadow,
    /// A burst of light where cuts meet or a strike lands (glow).
    Spark,
    /// A ring racing outward along the ground (ring).
    Ring,
    /// Unchained Will: the broken chain's pieces flying off (glow).
    Shards,
    /// Forge of Ten Souls: ten souls circling up around the target (glow).
    Souls,
    /// Destiny's Severance: a blade of light falling from the sky onto the target, crimson
    /// at the ground and cyan above (streak).
    Beam,
}

/// Whose colours an aura wears.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuraStyle {
    /// The original's level 99 aura: blue throughout.
    Level,
    /// Tenseiga-no-Hayate's (a custom hat effect): its blade's crimson, with the cyan of its
    /// edge. Drawn with whitened textures, so the colours here are the whole of it.
    Tenseiga,
    /// Kagutsuchi-no-Yami, god of ashes and chains (Twin Flames Saga): embers over a black pool.
    Yami,
    /// Amaterasu-no-Mikazuki, the Moon-Sun Weaver: sun gold and moon silver, no shadow.
    Amaterasu,
}

// Tenseiga's colours. Crimson stays a deep red (no blue in it), so added light reads as
// crimson rather than drifting to pink; a darker crimson for the ends of ribbons.
const CRIMSON: [f32; 3] = [1.0, 0.03, 0.06];
const DEEP_CRIMSON: [f32; 3] = [0.55, 0.0, 0.03];
const CYAN: [f32; 3] = [0.1, 0.75, 1.0];
const GOLD: [f32; 3] = [1.0, 0.82, 0.45];
// Yami's embers and ash; Amaterasu's sun and moon.
const EMBER: [f32; 3] = [1.0, 0.32, 0.04];
const ASH: [f32; 3] = [0.35, 0.3, 0.3];
const SUN: [f32; 3] = [1.0, 0.78, 0.3];
const MOON: [f32; 3] = [0.78, 0.85, 1.0];

impl AuraStyle {
    /// Ribbon colour at its foot and at its top.
    fn ribbon(self) -> ([f32; 4], [f32; 4]) {
        match self {
            AuraStyle::Level => {
                let c = [100.0 / 255.0, 100.0 / 255.0, 1.0, 120.0 / 255.0];
                (c, c)
            }
            AuraStyle::Tenseiga => ([CRIMSON[0], CRIMSON[1], CRIMSON[2], 2.0], [DEEP_CRIMSON[0], DEEP_CRIMSON[1], DEEP_CRIMSON[2], 1.6]),
            AuraStyle::Yami => ([EMBER[0], EMBER[1], EMBER[2], 2.0], [ASH[0], ASH[1], ASH[2], 1.2]),
            AuraStyle::Amaterasu => ([SUN[0], SUN[1], SUN[2], 1.6], [MOON[0], MOON[1], MOON[2], 1.4]),
        }
    }

    /// The two ground stars' colours, and how fast they turn (turns per step).
    fn ground(self) -> ([[f32; 4]; 2], f32) {
        match self {
            AuraStyle::Level => ([[1.0, 1.0, 1.0, 0.8]; 2], 0.0),
            AuraStyle::Tenseiga => ([[CRIMSON[0], CRIMSON[1], CRIMSON[2], 0.7], [DEEP_CRIMSON[0], DEEP_CRIMSON[1], DEEP_CRIMSON[2], 1.0]], 1.0 / 800.0),
            AuraStyle::Yami => ([[EMBER[0], EMBER[1], EMBER[2], 0.8], [0.5, 0.08, 0.02, 1.0]], -1.0 / 600.0),
            AuraStyle::Amaterasu => ([[SUN[0], SUN[1], SUN[2], 0.8], [MOON[0], MOON[1], MOON[2], 0.8]], 1.0 / 1200.0),
        }
    }

    /// The dark pool under the aura, if it has one: crimson-black, so the light over it stays
    /// red on pale ground.
    fn shade(self) -> Option<[f32; 4]> {
        match self {
            AuraStyle::Level => None,
            AuraStyle::Tenseiga => Some([0.09, 0.0, 0.02, 0.92]),
            AuraStyle::Yami => Some([0.03, 0.01, 0.01, 0.95]),
            AuraStyle::Amaterasu => None,
        }
    }

    /// The `j`th sparkle's colour (alpha 1).
    fn sparkle(self, j: u32) -> [f32; 3] {
        match self {
            AuraStyle::Level => [80.0 / 255.0, 80.0 / 255.0, 1.0],
            // Mostly crimson embers, with the edge's cyan.
            AuraStyle::Tenseiga => [CRIMSON, CRIMSON, CYAN][j as usize % 3],
            // Embers rising with flakes of ash.
            AuraStyle::Yami => [EMBER, EMBER, ASH][j as usize % 3],
            AuraStyle::Amaterasu => [SUN, MOON, GOLD][j as usize % 3],
        }
    }
}

/// The level 99 aura's layers. The original builds them in code; the shapes and numbers
/// follow roBrowser's reconstruction of its Level99 effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuraLayer {
    /// EF_LEVEL99: three ribbons (effect\ring_blue.tga) spiralling up and around.
    Swirl,
    /// EF_LEVEL99_2: two light stars (effect\pikapika2.bmp) on the ground, breathing.
    Ground,
    /// EF_LEVEL99_3: blue motes (effect\whitelight.tga) drifting up from the feet.
    Sparkles,
    /// A dark pool on the ground under the light layers (alpha-blended), so coloured auras
    /// read on bright ground. Custom auras only; the level aura has none.
    Shade,
}

impl ProcFx {
    /// Drawn alpha-blended (darkening) rather than added.
    pub fn shades(&self) -> bool {
        matches!(self.shape, Shape::Aura { layer: AuraLayer::Shade, .. } | Shape::Tenseiga { solid: true, .. } | Shape::Crescent { solid: true, .. })
    }
}

/// The original's effect clock: one step every 25 ms.
const AURA_TICK_MS: f32 = 25.0;
/// roBrowser's scale from the original's aura units to cells.
const AURA_UNIT: f32 = 0.22 * CELL;

fn aura(layer: AuraLayer, seed: u32, style: AuraStyle, pos: Vec3, time_ms: f32, eye: Vec3, ground: Option<&Heights>, out: &mut Vec<Vertex>) {
    let ticks = time_ms / AURA_TICK_MS;
    match layer {
        AuraLayer::Shade => {
            let Some(color) = style.shade() else { return };
            // Breathes with the ground stars.
            let half = 2.4 * CELL * (1.0 + 0.05 * (ticks / 120.0 * std::f32::consts::TAU).sin());
            let v = |x: f32, z: f32, u: f32, vv: f32| Vertex { pos: (pos + Vec3::new(x, 0.08, z)).into(), uv: [u, vv], color };
            drape(out, [v(-half, -half, 0.0, 0.0), v(half, -half, 1.0, 0.0), v(half, half, 1.0, 1.0), v(-half, half, 0.0, 1.0)], ground, 0.25);
        }
        AuraLayer::Swirl => {
            const DIVISIONS: usize = 21;
            // The ribbons grow to full height over the first 90 steps.
            let grow = ticks.min(90.0).to_radians().sin();
            let (foot, top_color) = style.ribbon();
            for band in 0..3 {
                let b = band as f32;
                let start = b * 90.0 + ticks * (b + 3.0);
                let radius = (3.9 + 0.2 * b) * AURA_UNIT * 0.6;
                let height = (15.0 - 2.0 * b) * AURA_UNIT;
                let (rise_sin, rise_cos) = (55.0 - 5.0 * b).to_radians().sin_cos();
                let edge = |k: usize| {
                    // Each ribbon spans 315°, tallest in its middle and flat at both ends.
                    let h = height * (90.0 + (k as f32 - 10.0) * 9.0).to_radians().sin() * grow;
                    let (s, c) = (start + k as f32 * 315.0 / 20.0).to_radians().sin_cos();
                    let base = pos + Vec3::new(radius * c, 0.0, radius * s);
                    let top = base + Vec3::new(rise_cos * h * c, rise_sin * h, rise_cos * h * s);
                    let u = k as f32 / 20.0;
                    (Vertex { pos: base.into(), uv: [u, 1.0], color: foot }, Vertex { pos: top.into(), uv: [u, 0.0], color: top_color })
                };
                for k in 0..DIVISIONS - 1 {
                    let ((b0, t0), (b1, t1)) = (edge(k), edge(k + 1));
                    out.extend([b0, t0, b1, t0, t1, b1]);
                }
            }
        }
        AuraLayer::Ground => {
            // Two stars, 23° apart, swelling and shrinking out of step over 120 steps
            // (Tenseiga's also turn slowly, against each other).
            let (colors, spin) = style.ground();
            for (i, base) in [(0, 115.0), (1, 130.0)] {
                let breathe = (ticks / 120.0 * std::f32::consts::TAU + i as f32 * std::f32::consts::PI).sin();
                let half = base / 35.0 * CELL * 0.5 * (1.0 + 0.07 * breathe);
                let turn = ticks * spin * 360.0 * if i == 0 { 1.0 } else { -1.0 };
                let (s, c) = (i as f32 * 23.0 + turn).to_radians().sin_cos();
                let corner = |x: f32, z: f32| pos + Vec3::new(x * c - z * s, 0.1, x * s + z * c);
                let color = colors[i];
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                drape(out, [v(corner(-half, -half), 0.0, 0.0), v(corner(half, -half), 1.0, 0.0), v(corner(half, half), 1.0, 1.0), v(corner(-half, half), 0.0, 1.0)], ground, 0.35);
            }
        }
        AuraLayer::Sparkles => {
            // 16 motes, each rising 30 units in 200 steps, then resting a while before
            // starting again from the feet. Drawn twice over in the original, so doubled here.
            const RISE: f32 = 200.0;
            let fwd = (pos - eye).normalize_or_zero();
            let right = fwd.cross(Vec3::Y).normalize_or_zero();
            let up = right.cross(fwd);
            let half = 2.4 * AURA_UNIT * 0.6;
            for j in 0..16u32 {
                let r = |n: u32| (hash(seed ^ j.wrapping_mul(0x9e37_79b9) ^ n.wrapping_mul(0x85eb_ca6b)) & 0xffff) as f32 / 65535.0;
                let rest = r(1) * 660.0;
                let cycle = RISE + rest;
                let t = (ticks + r(2) * cycle) % cycle;
                if t < rest {
                    continue;
                }
                let lift = (t - rest) * 0.15;
                // Full until 20 units up, gone by 28.
                let alpha = ((250.0 - 30.0 * (lift - 20.0)).clamp(0.0, 250.0)) / 255.0;
                if alpha <= 0.0 {
                    continue;
                }
                let (f1, f2, p1, p2) = (0.02 + r(3) * 0.03, 0.02 + r(4) * 0.03, r(5) * 6.3, r(6) * 6.3);
                let drift = |f: f32, p: f32| ((t - rest) * f + p).sin() - p.sin();
                let centre = pos + Vec3::new(drift(f1, p1) * 3.0 * AURA_UNIT, lift * AURA_UNIT, drift(f2, p2) * 3.0 * AURA_UNIT);
                let [r, g, b] = style.sparkle(j);
                let color = [r, g, b, alpha * 2.0];
                let (a, b) = (right * half, up * half);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                out.extend([p0, p1, p2, p0, p2, p3]);
            }
        }
    }
}

/// A flat quad (corners in order around it) laid over the ground as a 12x12 grid, `lift`
/// above it, so bumps don't poke through it and dips don't leave it hanging. Without a map
/// it stays flat.
fn drape(out: &mut Vec<Vertex>, q: [Vertex; 4], ground: Option<&Heights>, lift: f32) {
    let Some(ground) = ground else {
        out.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
        return;
    };
    const N: usize = 12;
    let at = |i: usize, j: usize| {
        let (s, t) = (i as f32 / N as f32, j as f32 / N as f32);
        let w = [(1.0 - s) * (1.0 - t), s * (1.0 - t), s * t, (1.0 - s) * t];
        let mix = |f: fn(&Vertex) -> [f32; 3]| [0, 1, 2].map(|k| (0..4).map(|n| f(&q[n])[k] * w[n]).sum::<f32>());
        let [x, _, z] = mix(|v| v.pos);
        let [u, vv, _] = mix(|v| [v.uv[0], v.uv[1], 0.0]);
        Vertex { pos: [x, ground.at(x, z) + lift, z], uv: [u, vv], color: q[0].color }
    };
    for j in 0..N {
        for i in 0..N {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            out.extend([a, b, c, a, c, d]);
        }
    }
}

/// A camera-facing frame at `centre`: (right, up).
fn facing(centre: Vec3, eye: Vec3) -> (Vec3, Vec3) {
    let fwd = (centre - eye).normalize_or_zero();
    let right = fwd.cross(Vec3::Y).normalize_or_zero();
    (right, right.cross(fwd))
}

/// A quad around `c` spanning `a` and `b` (half extents), with the texture's `u0..u1` on it.
fn quad_u(out: &mut Vec<Vertex>, c: Vec3, a: Vec3, b: Vec3, u0: f32, u1: f32, color: [f32; 4]) {
    let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
    let (p0, p1, p2, p3) = (v(c - a - b, u0, 1.0), v(c + a - b, u1, 1.0), v(c + a + b, u1, 0.0), v(c - a + b, u0, 0.0));
    out.extend([p0, p1, p2, p0, p2, p3]);
}

fn quad(out: &mut Vec<Vertex>, c: Vec3, a: Vec3, b: Vec3, color: [f32; 4]) {
    quad_u(out, c, a, b, 0.0, 1.0, color);
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn rgba(c: [f32; 3], k: f32, a: f32) -> [f32; 4] {
    [c[0] * k, c[1] * k, c[2] * k, a]
}

/// A straight streak from `from` to `to`, `width` across, turned to face the camera.
fn streak(out: &mut Vec<Vertex>, from: Vec3, to: Vec3, width: f32, eye: Vec3, color: [f32; 4]) {
    let along = to - from;
    let side = along.cross(((from + to) * 0.5 - eye).normalize_or_zero()).normalize_or_zero() * width * 0.5;
    let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
    let (p0, p1, p2, p3) = (v(from - side, 0.0, 0.0), v(to - side, 1.0, 0.0), v(to + side, 1.0, 1.0), v(from + side, 0.0, 1.0));
    out.extend([p0, p1, p2, p0, p2, p3]);
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// One layer of a Mortal Flame skill effect at progress `t` (0..1). `tint` colours rings,
/// sparks and shards. A `solid` layer paints the colour itself; the glow layer drawn over it
/// is a narrower, paler core (added light), so crimson reads as crimson even on pale ground.
fn tenseiga(part: TenseigaPart, solid: bool, pos: Vec3, t: f32, tint: [f32; 3], eye: Vec3, out: &mut Vec<Vertex>) {
    use std::f32::consts::PI;
    let chest = pos + Vec3::Y * CELL * 1.1;
    let (right, up) = facing(chest, eye);
    let fade = |from: f32| 1.0 - ((t - from) / (1.0 - from)).clamp(0.0, 1.0);
    // Solid: the colour at `opacity`. Glow: a whiter core of it, `k` bright.
    let paint = |c: [f32; 3], opacity: f32, k: f32| if solid { rgba(c, 1.0, opacity) } else { rgba(mix(c, [1.0, 0.95, 0.95], 0.55), k, opacity) };
    let core = if solid { 1.0 } else { 0.35 };
    match part {
        TenseigaPart::Arc => {
            // The head sweeps a 190-degree arc over the first half; the tail follows.
            let (a0, a1) = (150f32.to_radians(), (-40f32).to_radians());
            let head = a0 + (a1 - a0) * ease_out(t / 0.45);
            let tail = a0 + (a1 - a0) * ease_out((t - 0.2) / 0.6);
            let (radius, width) = (CELL * 1.5, CELL * 0.34 * core);
            const N: usize = 24;
            let alpha = fade(0.55);
            let edge = |k: usize| {
                let s = k as f32 / N as f32;
                let a = tail + (head - tail) * s;
                let dir = right * a.cos() + up * a.sin();
                let w = width * (PI * s).sin().max(0.15);
                let color = paint(mix(CRIMSON, CYAN, s * s * s), alpha * (0.5 + 0.5 * s), 1.4);
                let inner = Vertex { pos: (chest + dir * (radius - w)).into(), uv: [s, 0.0], color };
                let outer = Vertex { pos: (chest + dir * (radius + w)).into(), uv: [s, 1.0], color };
                (inner, outer)
            };
            for k in 0..N {
                let ((i0, o0), (i1, o1)) = (edge(k), edge(k + 1));
                out.extend([i0, o0, o1, i0, o1, i1]);
            }
        }
        TenseigaPart::Halo => {
            // Appears as the cut passes over, then splits and drops away to either side.
            let show = ((t - 0.25) / 0.1).clamp(0.0, 1.0) * fade(0.5);
            if show <= 0.0 {
                return;
            }
            let split = ease_out((t - 0.35) / 0.65);
            let centre = pos + Vec3::Y * (CELL * 2.6 - split * CELL * 1.2);
            let r = CELL * 0.75;
            let depth = right.cross(Vec3::Y).normalize_or_zero() * r;
            for (side, u0) in [(-1.0f32, 0.0f32), (1.0, 0.5)] {
                let c = centre + right * side * (split * CELL * 0.8 + r * 0.5);
                quad_u(out, c, right * r * 0.5, depth, u0, u0 + 0.5, paint(GOLD, show * 0.9, 1.2));
            }
        }
        TenseigaPart::CutLight | TenseigaPart::CutShadow => {
            let (start, dir, color) = match part {
                TenseigaPart::CutLight => (0.0, (up - right).normalize_or_zero(), GOLD),
                _ => (0.28, (up + right).normalize_or_zero(), CRIMSON),
            };
            let local = ((t - start) / (1.0 - start)).clamp(0.0, 1.0);
            if local <= 0.0 {
                return;
            }
            let half = CELL * 1.8;
            let from = chest + dir * half;
            let to = from - dir * (2.0 * half * ease_out(local / 0.3));
            let width = CELL * 0.32 * core * (1.0 - ((local - 0.3) / 0.7).clamp(0.0, 1.0));
            streak(out, from, to, width, eye, paint(color, 0.95, 1.6));
        }
        TenseigaPart::Spark => {
            let local = ((t - 0.3) / 0.7).clamp(0.0, 1.0);
            if local <= 0.0 {
                return;
            }
            let size = CELL * 1.6 * core.max(0.5) * ease_out(local / 0.4);
            quad(out, chest, right * size, up * size, paint(tint, (1.0 - local) * 0.7, 1.6));
        }
        TenseigaPart::Ring => {
            let r = CELL * (0.4 + 3.2 * ease_out(t));
            quad(out, pos + Vec3::Y * 0.2, Vec3::X * r, Vec3::Z * r, paint(tint, (1.0 - t).powf(1.5) * 0.85, 1.2));
        }
        TenseigaPart::Shards => {
            for i in 0..10 {
                let a = i as f32 * 36f32.to_radians() + 0.3;
                let c = pos + Vec3::new(a.cos(), 0.0, a.sin()) * ease_out(t) * CELL * 2.6 + Vec3::Y * (CELL * (1.0 + (PI * t).sin() * 1.3));
                let colour = if i % 2 == 0 { tint } else { [0.85, 0.85, 1.0] };
                let s = CELL * 0.3 * core.max(0.6);
                quad(out, c, right * s, up * s, paint(colour, 1.0 - t, 2.0));
            }
        }
        TenseigaPart::Souls => {
            let colours = [CRIMSON, CRIMSON, CYAN, GOLD, CRIMSON];
            for i in 0..10 {
                let a = i as f32 * 36f32.to_radians() + t * 3.0 * PI;
                let r = CELL * 1.3 * (1.0 - 0.35 * t);
                let c = pos + Vec3::new(a.cos() * r, CELL * (0.3 + 3.4 * t), a.sin() * r);
                let s = CELL * 0.45 * core.max(0.5);
                quad(out, c, right * s, up * s, paint(colours[i % colours.len()], (PI * t).sin(), 2.0));
            }
        }
        TenseigaPart::Beam => {
            // A blade of light: falls over the first fifth, stands, then narrows away.
            let fall = ease_out(t / 0.2);
            let width = CELL * 0.9 * core * if t < 0.3 { fall } else { 1.0 - ((t - 0.3) / 0.7).clamp(0.0, 1.0) };
            if width <= 0.0 {
                return;
            }
            let top = pos + Vec3::Y * CELL * 14.0;
            let bottom = top + (pos - top) * fall;
            const N: usize = 8;
            for k in 0..N {
                let (s0, s1) = (k as f32 / N as f32, (k + 1) as f32 / N as f32);
                let (p0, p1) = (bottom + (top - bottom) * s0, bottom + (top - bottom) * s1);
                streak(out, p0, p1, width, eye, paint(mix(CRIMSON, CYAN, (s0 + s1) * 0.5), 0.95, 1.8));
            }
        }
    }
}

/// Crescent Gale at progress `t`: the crescent flies the first 70%, two fading afterimages
/// behind it and a scorched line left on the ground, then swells and breaks up. Crimson, its
/// leading edge cyan; it faces the camera, its bulge leading along the travel on screen.
fn crescent(solid: bool, pos: Vec3, travel: Vec3, t: f32, eye: Vec3, ground: Option<&Heights>, out: &mut Vec<Vertex>) {
    use std::f32::consts::PI;
    const FLY: f32 = 0.7;
    let paint = |c: [f32; 3], opacity: f32, k: f32| if solid { rgba(c, 1.0, opacity) } else { rgba(mix(c, [1.0, 0.95, 0.95], 0.55), k, opacity) };
    let flown = |t: f32| (t / FLY).clamp(0.0, 1.0);
    let burst = ((t - FLY) / (1.0 - FLY)).clamp(0.0, 1.0);
    if solid {
        let side = Vec3::new(-travel.z, 0.0, travel.x).normalize_or_zero() * CELL * 0.6;
        let head = pos + travel * flown(t);
        let c = rgba(DEEP_CRIMSON, 1.0, 0.7 * (1.0 - t));
        let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color: c };
        drape(out, [v(pos - side, 0.0, 0.0), v(head - side, 1.0, 0.0), v(head + side, 1.0, 1.0), v(pos + side, 0.0, 1.0)], ground, 0.5);
    }
    for (k, (lag, strength)) in [(0.0, 1.0), (0.06, 0.45), (0.12, 0.2)].into_iter().enumerate() {
        let local = t - lag;
        let alpha = strength * (1.0 - burst) * (local / 0.05).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            continue;
        }
        let s = flown(local);
        let centre = pos + travel * s + Vec3::Y * CELL * 1.5;
        let (right, up) = facing(centre, eye);
        let (dx, dy) = (travel.dot(right), travel.dot(up));
        let lead = if dx * dx + dy * dy > 1e-6 { (right * dx + up * dy).normalize() } else { up };
        let side = right * -lead.dot(up) + up * lead.dot(right);
        let r = CELL * (2.0 + 0.5 * s + 1.5 * burst) * (1.0 - 0.1 * k as f32);
        let width = CELL * 0.8 * if solid { 1.0 } else { 0.4 } * (1.0 - 0.5 * burst);
        let hub = centre - lead * r * 0.55;
        let span = 75f32.to_radians();
        const N: usize = 24;
        // Thick in the middle, sharp at the tips; the glow layer hugs the leading edge.
        let edge = |i: usize| {
            let f = i as f32 / N as f32;
            let a = -span + 2.0 * span * f;
            let radial = lead * a.cos() + side * a.sin();
            let outer = hub + radial * r;
            let inner = outer - radial * width * (PI * f).sin().powf(0.7);
            (
                Vertex { pos: inner.into(), uv: [f, 0.0], color: paint(CRIMSON, alpha * 0.9, 1.2) },
                Vertex { pos: outer.into(), uv: [f, 1.0], color: paint(mix(CRIMSON, CYAN, 0.85), alpha, 1.8) },
            )
        };
        for i in 0..N {
            let ((i0, o0), (i1, o1)) = (edge(i), edge(i + 1));
            out.extend([i0, o0, o1, i0, o1, i1]);
        }
    }
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// A playing code-drawn effect.
#[derive(Clone, Debug)]
pub struct ProcFx {
    pub tex: TexId,
    pub shape: Shape,
    pub pos: Vec3,
    pub color: [f32; 3],
    pub time_ms: f32,
    pub duration_ms: f32,
}

impl ProcFx {
    /// `ground`: the map's surface, which ground-hugging layers are laid over.
    pub fn vertices(&self, eye: Vec3, ground: Option<&Heights>, out: &mut Vec<Vertex>) {
        let t = (self.time_ms / self.duration_ms.max(1.0)).clamp(0.0, 1.0);
        // Quick fade in, long fade out.
        let fade = (t / 0.1).min(1.0) * ((1.0 - t) / 0.5).min(1.0);
        let color = [self.color[0], self.color[1], self.color[2], fade];
        match self.shape {
            Shape::Aura { layer, seed, style } => aura(layer, seed, style, self.pos, self.time_ms, eye, ground, out),
            Shape::Tenseiga { part, solid } => tenseiga(part, solid, self.pos, t, self.color, eye, out),
            Shape::Crescent { travel, solid } => crescent(solid, self.pos, travel, t, eye, ground, out),
            Shape::Column { radius, height } => {
                let h = height * CELL * (t / 0.3).min(1.0);
                let r = radius * CELL;
                cylinder(out, self.pos, r, r * 0.8, h, t * 0.5, color);
                cylinder(out, self.pos, r * 0.6, r * 0.5, h * 1.2, -t * 0.7, color);
            }
            Shape::Spikes { reach, count } => {
                // Each spike rises and sinks in turn, the wave running from pos to pos + reach.
                let side = Vec3::new(-reach.z, 0.0, reach.x).normalize_or_zero();
                for i in 0..count {
                    let f = (i as f32 + 1.0) / count as f32;
                    let local = (t * 1.6 - f * 0.9).clamp(0.0, 1.0);
                    if local <= 0.0 || local >= 1.0 {
                        continue;
                    }
                    let rise = (local * std::f32::consts::PI).sin();
                    // Alternate sides a little so the line looks jagged.
                    let jitter = side * if i % 2 == 0 { 0.35 } else { -0.35 } * CELL;
                    let base = self.pos + reach * f + jitter;
                    let h = CELL * (2.2 + 1.0 * ((i * 7 % 5) as f32 / 4.0)) * rise;
                    let r = CELL * 0.4;
                    let c = [color[0], color[1], color[2], color[3] * rise.min(1.0)];
                    let apex = base + Vec3::new(0.0, h, 0.0);
                    for k in 0..5 {
                        let a0 = k as f32 / 5.0 * std::f32::consts::TAU;
                        let a1 = (k + 1) as f32 / 5.0 * std::f32::consts::TAU;
                        let p0 = base + Vec3::new(a0.cos() * r, 0.0, a0.sin() * r);
                        let p1 = base + Vec3::new(a1.cos() * r, 0.0, a1.sin() * r);
                        let (u0, u1) = (k as f32 / 5.0, (k + 1) as f32 / 5.0);
                        out.extend([
                            Vertex { pos: p0.into(), uv: [u0, 1.0], color: c },
                            Vertex { pos: p1.into(), uv: [u1, 1.0], color: c },
                            Vertex { pos: apex.into(), uv: [(u0 + u1) * 0.5, 0.0], color: c },
                        ]);
                    }
                }
            }
            Shape::Wave { radius } => {
                // A low wall rushing outward and thinning (Meteor Assault's shockwave).
                let ease = 1.0 - (1.0 - t).powi(3);
                let r = radius * CELL * ease.max(0.05);
                let h = CELL * 1.2 * (1.0 - t * 0.6);
                cylinder(out, self.pos, r, r * 1.15, h, t * 0.3, color);
                cylinder(out, self.pos, r * 0.7, r * 0.85, h * 0.6, -t * 0.4, color);
            }
            Shape::Orb { reach } => {
                // Flies to pos + reach over the first half, then swells and fades there.
                let fly = (t / 0.5).min(1.0);
                let swell = ((t - 0.5) / 0.5).max(0.0);
                let centre = self.pos + reach * fly + Vec3::new(0.0, CELL * (1.2 - 0.6 * swell), 0.0);
                let s = CELL * (0.7 + 2.3 * swell);
                let c = [color[0], color[1], color[2], (1.0 - swell).min(fade * 2.0).max(0.0)];
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color: c };
                // Three crossed quads read as a ball from any camera angle.
                for (a, b) in [(Vec3::X, Vec3::Y), (Vec3::Z, Vec3::Y), (Vec3::X, Vec3::Z)] {
                    let (a, b) = (a * s, b * s);
                    let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                    out.extend([p0, p1, p2, p0, p2, p3]);
                }
            }
            Shape::Slash { travel, size } => {
                let centre = self.pos + travel * t + Vec3::new(0.0, CELL, 0.0);
                let fwd = (centre - eye).normalize_or_zero();
                let right = fwd.cross(Vec3::Y).normalize_or_zero();
                let up = right.cross(fwd);
                // The crescent bulges toward -u: point that side along the on-screen travel.
                let (dx, dy) = (travel.dot(right), travel.dot(up));
                let lead = if dx * dx + dy * dy > 1e-6 { (right * dx + up * dy).normalize() } else { -right };
                let side = lead.cross(fwd);
                let s = CELL * (size.0 + (size.1 - size.0) * t);
                let (a, b) = (-lead * s, side * s);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                out.extend([p0, p1, p2, p0, p2, p3]);
            }
            Shape::Motes { drift, size } => {
                let secs = self.time_ms / 1000.0;
                let wobble = Vec3::new((secs * 1.3).sin(), (secs * 0.9).sin() * 0.5, (secs * 1.1).cos()) * CELL * 0.3;
                let centre = self.pos + drift * t + wobble;
                let fwd = (centre - eye).normalize_or_zero();
                let right = fwd.cross(Vec3::Y).normalize_or_zero();
                let up = right.cross(fwd);
                let (s, c) = (secs * 0.4).sin_cos();
                let half = CELL * size * 0.5;
                let (a, b) = ((right * c + up * s) * half, (up * c - right * s) * half);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
                out.extend([p0, p1, p2, p0, p2, p3]);
            }
            Shape::Disc { radius, spin } => {
                let r = radius * CELL;
                let ang = self.time_ms / 1000.0 * spin * std::f32::consts::TAU;
                let (s, c) = ang.sin_cos();
                let corner = |x: f32, z: f32| self.pos + Vec3::new(x * c - z * s, 0.15, x * s + z * c);
                let v = |p: Vec3, u: f32, vv: f32| Vertex { pos: p.into(), uv: [u, vv], color };
                let (a, b, cc, d) = (v(corner(-r, -r), 0.0, 0.0), v(corner(r, -r), 1.0, 0.0), v(corner(r, r), 1.0, 1.0), v(corner(-r, r), 0.0, 1.0));
                out.extend([a, b, cc, a, cc, d]);
            }
            Shape::Area { size } => {
                let (hx, hz) = (size.0 * CELL / 2.0, size.1 * CELL / 2.0);
                let v = |x: f32, z: f32, u: f32, vv: f32| Vertex { pos: (self.pos + Vec3::new(x, 0.2, z)).into(), uv: [u, vv], color };
                let (a, b, cc, d) = (v(-hx, -hz, 0.0, 0.0), v(hx, -hz, 1.0, 0.0), v(hx, hz, 1.0, 1.0), v(-hx, hz, 0.0, 1.0));
                out.extend([a, b, cc, a, cc, d]);
            }
        }
    }
}

#[cfg(test)]
mod drape_tests {
    use super::*;

    #[test]
    fn drapes_over_the_drawn_ground() {
        // One 10-unit tile: corners 0, 10 (x), 20 (z), 30 (both), split along 0 to 3.
        let ground = Heights::from_corners(1, 1, 10.0, vec![[0.0, 10.0, 20.0, 30.0]]);
        assert_eq!(ground.at(5.0, 0.0), 5.0);
        assert_eq!(ground.at(5.0, 5.0), 15.0);
        // Off the diagonal each half is its own plane: (8, 2) lies on corners 0, 1 and 3.
        assert!((ground.at(8.0, 2.0) - (8.0 + 2.0 * 2.0)).abs() < 1e-4);
        let v = |x: f32, z: f32| Vertex { pos: [x, 99.0, z], uv: [x / 10.0, z / 10.0], color: [1.0; 4] };
        let mut out = vec![];
        drape(&mut out, [v(0.0, 0.0), v(10.0, 0.0), v(10.0, 10.0), v(0.0, 10.0)], Some(&ground), 0.5);
        assert_eq!(out.len(), 12 * 12 * 6);
        assert!(out.iter().all(|p| (p.pos[1] - ground.at(p.pos[0], p.pos[2]) - 0.5).abs() < 1e-4));
    }
}
