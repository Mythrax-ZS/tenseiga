//! Map particle emitters: RSW effect objects with id 974 (EF_EMITTER), defined per map in
//! `data\luafiles514\lua files\effecttool\<map>.lub` (`_<map>_emitterInfo`, the Nth 974
//! object takes entry N). Smoke, steam, sparks, fireflies, falling leaves, glowing dust.
//!
//! Simulation follows BrowEdit 3's reading of the format: particles spawn in a box of
//! `radius` around the emitter, fly with a velocity picked between `dir1` and `dir2`, fall
//! with `gravity` (both scaled by `speed`), live `life` seconds fading in and out, and are
//! drawn `size` across as camera-facing quads (or fixed quads turned by `rotate_angle` when
//! `billboard_off`), blended with the map's Direct3D source/destination modes.

use std::collections::HashMap;

use glam::{Mat3, Vec3};

use crate::effect::{TexId, Vertex};

/// One emitter's definition (lub units: RO y-down; colours 0..1).
#[derive(Clone, Debug)]
pub struct EmitterDef {
    pub dir1: [f32; 3],
    pub dir2: [f32; 3],
    pub gravity: [f32; 3],
    pub radius: [f32; 3],
    pub color: [f32; 4],
    /// Particles per second (min, max).
    pub rate: [f32; 2],
    pub size: [f32; 2],
    pub life: [f32; 2],
    pub speed: f32,
    /// Direct3D blend factors (D3DBLEND numbers).
    pub src: u8,
    pub dst: u8,
    pub max: u32,
    pub zenable: bool,
    /// Additive blends normally add in gamma space like the original (faint colours stay
    /// visible); true adds in linear light instead, so a near-black texture background (torch
    /// flame frames) stays invisible.
    pub linear: bool,
    /// Size at the end of a particle's life relative to its start (1 = constant).
    pub grow: f32,
    pub billboard: bool,
    /// Degrees, used when not a billboard.
    pub rotate: [f32; 3],
    pub tex: TexId,
    /// Animated emitters (EF_ANIMATED_EMITTER): frame sets; each particle picks one and
    /// steps a frame every 0.13 s. Empty: every particle shows `tex`.
    pub frames: Vec<Vec<TexId>>,
}

/// An emitter placed in the world (y-up render space).
#[derive(Clone, Debug)]
pub struct Emitter {
    pub pos: Vec3,
    pub def: EmitterDef,
}

/// A wind effect (RSW EF_WIND_EFFECT, 2343, `_<map>_windEffectInfo`): `count` ribbons, each
/// a band `thickness` wide swept over an arc of `full_angle` degrees around a circle of
/// `radius`, stacked up through `height`, turning (a revolution per 6 s at speed 1) and
/// fading in and out over 8 × alpha seconds, tilted by `rotate` (degrees about x, then z).
/// Follows BrowEdit's lubwind shader.
#[derive(Clone, Debug)]
pub struct Wind {
    pub pos: Vec3,
    pub count: u32,
    /// Linear 0..1 RGBA (the lub stores it ARGB; converted by the caller).
    pub color: [f32; 4],
    pub radius: f32,
    pub thickness: f32,
    pub height: f32,
    pub speed: f32,
    pub full_angle: f32,
    pub rotate: [f32; 2],
    pub src: u8,
    pub dst: u8,
    pub tex: TexId,
    /// Per-ribbon random seeds (phase of the fade and of the turn).
    pub seeds: Vec<(f32, f32)>,
}

/// A waterfall (RSW EF_WATERFALL family, 349-356): four layered sheets, each five 40-unit
/// segments of `effect\waterfall<set>1..3.tga` scrolling down at its own speed and cycling the
/// textures, 80/255 opaque, rising 160 units above the emitter (at the pool). Reconstructed after
/// roBrowserLegacy's WaterfallEffect (the spray puffs at the foot are not drawn yet).
#[derive(Clone, Debug)]
pub struct Waterfall {
    pub pos: Vec3,
    /// 36 for the large kinds, 18 for the small ones.
    pub width: f32,
    /// Turn about the vertical axis (radians): 0 spans x, 90° spans z.
    pub yaw: f32,
    pub textures: [TexId; 3],
}

/// A map glow (EF_GLOW1..12, EF_MAP_GREENLIGHT): an additive camera-facing disc of
/// `effect\glowNN.bmp` that breathes slowly; the ray burst (EF_GLOW4) also turns.
#[derive(Clone, Debug)]
pub struct Glow {
    pub pos: Vec3,
    pub radius: f32,
    pub color: [f32; 4],
    pub tex: TexId,
    /// Turn speed (radians per second).
    pub spin: f32,
    /// Phase of the breathing, so neighbours don't pulse in step.
    pub phase: f32,
}

struct Particle {
    start: Vec3,
    vel: Vec3,
    born: f32,
    life: f32,
    size: f32,
    /// Frame set and start offset (s) for animated emitters.
    set: usize,
    phase: f32,
}

struct Live {
    e: Emitter,
    particles: Vec<Particle>,
    next: f32,
}

/// (source blend, destination blend, depth test, linear add).
type Key = (u8, u8, bool, bool);

pub(crate) struct Particles {
    shader: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    samples: u32,
    pipelines: HashMap<Key, wgpu::RenderPipeline>,
    live: Vec<Live>,
    winds: Vec<Wind>,
    waterfalls: Vec<Waterfall>,
    glows: Vec<Glow>,
    rng: u64,
}

/// D3DBLEND -> wgpu (BrowEdit's table; 12/13 differ between source and destination).
fn factor(v: u8, dst: bool) -> wgpu::BlendFactor {
    use wgpu::BlendFactor as F;
    match v {
        1 => F::Zero,
        2 => F::One,
        3 => F::Src,
        4 => F::OneMinusSrc,
        5 => F::SrcAlpha,
        6 => F::OneMinusSrcAlpha,
        7 => F::DstAlpha,
        8 => F::OneMinusDstAlpha,
        9 => F::Dst,
        10 => F::OneMinusDst,
        11 => F::SrcAlphaSaturated,
        12 if dst => F::OneMinusSrcAlpha,
        12 => F::SrcAlpha,
        13 if dst => F::SrcAlpha,
        13 => F::OneMinusSrcAlpha,
        _ => F::Zero,
    }
}

impl Particles {
    pub fn new(device: &wgpu::Device, globals: &wgpu::BindGroupLayout, texture: &wgpu::BindGroupLayout, samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("particles"), source: wgpu::ShaderSource::Wgsl(include_str!("particle.wgsl").into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("particles"), bind_group_layouts: &[Some(globals), Some(texture)], ..Default::default() });
        Self { shader, layout, samples, pipelines: HashMap::new(), live: vec![], winds: vec![], waterfalls: vec![], glows: vec![], rng: 0x9e3779b97f4a7c15 }
    }

    fn pipeline(&self, device: &wgpu::Device, (src, dst, z, linear): Key) -> wgpu::RenderPipeline {
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: factor(src, false), dst_factor: factor(dst, true), operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::OVER,
        };
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("particles"),
            layout: Some(&self.layout),
            // `zenable` 0: still hidden by what stands in front, but tested a little nearer
            // the camera so the particles don't cut into the surface they rise from.
            vertex: wgpu::VertexState { module: &self.shader, entry_point: Some(if z { "vs_main" } else { "vs_pulled" }), compilation_options: Default::default(), buffers: &[Some(Vertex::layout())] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: self.samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some(if dst == 2 && !linear { "fs_add" } else { "fs_main" }),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: crate::post::HDR_FORMAT, blend: Some(blend), write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: crate::post::INFO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        })
    }

    /// Replaces the map's emitters (pre-warmed, so a map never starts empty) and builds a
    /// pipeline for each blend they use.
    pub fn set(&mut self, device: &wgpu::Device, emitters: Vec<Emitter>, winds: Vec<Wind>, waterfalls: Vec<Waterfall>, now: f32) {
        if !waterfalls.is_empty() && !self.pipelines.contains_key(&(5, 6, true, false)) {
            let p = self.pipeline(device, (5, 6, true, false));
            self.pipelines.insert((5, 6, true, false), p);
        }
        self.waterfalls = waterfalls;
        for w in &winds {
            let key = (w.src, w.dst, true, false);
            if !self.pipelines.contains_key(&key) {
                let p = self.pipeline(device, key);
                self.pipelines.insert(key, p);
            }
        }
        self.winds = winds;
        self.live = emitters.into_iter().map(|e| Live { e, particles: vec![], next: now }).collect();
        for i in 0..self.live.len() {
            let d = &self.live[i].e.def;
            let key = (d.src, d.dst, d.zenable, d.linear);
            if !self.pipelines.contains_key(&key) {
                let p = self.pipeline(device, key);
                self.pipelines.insert(key, p);
            }
            self.warm(i, now);
        }
    }

    /// Fills emitter `i` with the particles it would have in steady state (random ages), for
    /// map entry and after any gap in updates (minimised window, far away, a slow frame).
    fn warm(&mut self, i: usize, now: f32) {
        self.live[i].particles.clear();
        let d = &self.live[i].e.def;
        let count = (d.max as f32).min(avg(d.rate) * avg(d.life)).ceil() as usize;
        for _ in 0..count {
            let age = self.rand() * avg(self.live[i].e.def.life);
            let p = self.spawn(i, now - age);
            if now - p.born < p.life {
                self.live[i].particles.push(p);
            }
        }
        self.live[i].next = now;
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545f4914f6cdd1d) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn spawn(&mut self, i: usize, born: f32) -> Particle {
        let d = self.live[i].e.def.clone();
        let mut lerp = |a: f32, b: f32| a + self.rand() * (b - a);
        // Lub vectors are RO y-down; render space is y-up.
        let start = Vec3::new(lerp(-d.radius[0], d.radius[0]), lerp(-d.radius[1], d.radius[1]), lerp(-d.radius[2], d.radius[2]));
        // BrowEdit mirrors z relative to RO render space, so z flips too.
        let vel = Vec3::new(lerp(d.dir1[0], d.dir2[0]), -lerp(d.dir1[1], d.dir2[1]), -lerp(d.dir1[2], d.dir2[2]));
        let life = lerp(d.life[0], d.life[1]).max(0.05);
        let size = lerp(d.size[0], d.size[1]);
        let set = if d.frames.is_empty() { 0 } else { ((self.rand() * d.frames.len() as f32) as usize).min(d.frames.len() - 1) };
        let phase = self.rand() * 2.0;
        Particle { start, vel, born, life, size, set, phase }
    }

    /// Ages particles and emits new ones near `focus` (render space), within `range`.
    pub fn update(&mut self, now: f32, focus: Vec3, range: f32) {
        for i in 0..self.live.len() {
            let near = self.live[i].e.pos.distance(focus) < range;
            self.live[i].particles.retain(|p| now - p.born < p.life);
            if !near {
                self.live[i].next = now;
                continue;
            }
            if now - self.live[i].next > 1.0 {
                self.warm(i, now);
                continue;
            }
            let mut budget = 4;
            while now >= self.live[i].next && budget > 0 && (self.live[i].particles.len() as u32) < self.live[i].e.def.max {
                let p = self.spawn(i, self.live[i].next);
                self.live[i].particles.push(p);
                let rate = self.live[i].e.def.rate;
                let r = (rate[0] + self.rand() * (rate[1] - rate[0])).max(0.05);
                self.live[i].next += 1.0 / r;
                budget -= 1;
            }
        }
    }

    /// Quads for this frame, grouped by (blend, texture).
    pub fn vertices(&self, now: f32, eye: Vec3, focus: Vec3, range: f32) -> Vec<(Key, TexId, Vec<Vertex>)> {
        let mut groups: HashMap<(Key, usize), Vec<Vertex>> = HashMap::new();
        for l in &self.live {
            if l.particles.is_empty() || l.e.pos.distance(focus) > range {
                continue;
            }
            let d = &l.e.def;
            let base = l.e.pos + Vec3::new(0.0, 10.0, 0.0);
            let grav = Vec3::new(d.gravity[0], -d.gravity[1], -d.gravity[2]);
            let fixed = Mat3::from_rotation_z(d.rotate[2].to_radians()) * Mat3::from_rotation_y(-d.rotate[1].to_radians()) * Mat3::from_rotation_x(d.rotate[0].to_radians());
            for p in &l.particles {
                let t = now - p.born;
                let pos = base + p.start + p.vel * d.speed * t + grav * d.speed * t * t * 0.5;
                // Fade in over the first second and out over the last (or a share of a
                // short life), as the original.
                let edge = (p.life * 0.5).min(1.0);
                let fade = (t / edge).min((p.life - t) / edge).clamp(0.0, 1.0);
                let color = [d.color[0], d.color[1], d.color[2], d.color[3] * fade];
                let (right, up) = if d.billboard {
                    let fwd = (pos - eye).normalize_or_zero();
                    let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
                    (right, right.cross(fwd))
                } else {
                    (fixed * Vec3::X, fixed * Vec3::Y)
                };
                let size = p.size * (1.0 + (d.grow - 1.0) * (t / p.life).clamp(0.0, 1.0));
                let (a, b) = (right * size, up * size);
                let v = |q: Vec3, u: f32, vv: f32| Vertex { pos: q.into(), uv: [u, vv], color };
                let (p0, p1, p2, p3) = (v(pos - a - b, 0.0, 1.0), v(pos + a - b, 1.0, 1.0), v(pos + a + b, 1.0, 0.0), v(pos - a + b, 0.0, 0.0));
                let tex = match d.frames.get(p.set) {
                    Some(f) if !f.is_empty() => f[((t + p.phase) / 0.13) as usize % f.len()],
                    _ => d.tex,
                };
                groups.entry(((d.src, d.dst, d.zenable, d.linear), tex.0)).or_default().extend([p0, p1, p2, p0, p2, p3]);
            }
        }
        for w in &self.winds {
            if w.pos.distance(focus) > range || w.count < 2 {
                continue;
            }
            let duration = 8.0 * w.color[3];
            if duration <= 0.0 {
                continue;
            }
            let (rx, rz) = (w.rotate[0].to_radians(), -w.rotate[1].to_radians());
            let out = groups.entry(((w.src, w.dst, true, false), w.tex.0)).or_default();
            const SEGMENTS: usize = 20;
            for (i, &(s1, s2)) in w.seeds.iter().enumerate() {
                let t = (now + duration * s1).rem_euclid(duration);
                let half = duration * 0.5;
                let alpha = if t < half { t / half } else { (duration - t) / half };
                let turn = (s2 + now / 6.0 * w.speed).rem_euclid(1.0);
                let lift = i as f32 / (w.count as f32 - 1.0) * w.height;
                let color = [w.color[0], w.color[1], w.color[2], w.color[3] * alpha];
                let point = |along: f32, edge: f32| -> Vec3 {
                    let angle = along * w.full_angle.to_radians() + turn * std::f32::consts::TAU;
                    let r = w.radius + if edge < 0.0 { w.thickness } else { 0.0 };
                    let (mut x, mut y, mut z) = (angle.cos() * r, edge * w.thickness + lift, angle.sin() * r);
                    (y, z) = (y * rx.cos() - z * rx.sin(), y * rx.sin() + z * rx.cos());
                    (x, y) = (x * rz.cos() - y * rz.sin(), x * rz.sin() + y * rz.cos());
                    // BrowEdit's world mirrors z relative to ours.
                    w.pos + Vec3::new(x, y, -z)
                };
                for k in 0..SEGMENTS {
                    let (a, b) = (k as f32 / SEGMENTS as f32, (k + 1) as f32 / SEGMENTS as f32);
                    let v = |along: f32, edge: f32| Vertex { pos: point(along, edge).into(), uv: [1.0 - along, if edge < 0.0 { 0.0 } else { 1.0 }], color };
                    let (bl, br, tl, tr) = (v(a, 0.0), v(b, 0.0), v(a, -1.0), v(b, -1.0));
                    out.extend([bl, br, tl, tl, br, tr]);
                }
            }
        }
        // Waterfalls: process ticks every 24 ms; layer l scrolls a segment per 80 - 13l ticks.
        let process = (now * 1000.0 / 24.0) as i64;
        for w in &self.waterfalls {
            if w.pos.distance(focus) > range {
                continue;
            }
            const SEG: f32 = 40.0;
            let color = [1.0, 1.0, 1.0, 80.0 / 255.0];
            let turn = Mat3::from_rotation_y(-w.yaw);
            let (across, depth_axis) = (turn * Vec3::X, turn * Vec3::Z);
            // The emitter sits at the pool: the sheets rise 160 units to the lip, their texture
            // scrolling down (roBrowser builds them in RO's y-down space).
            let base = w.pos;
            for layer in 0..4i64 {
                let speed = 80 - layer * 13;
                let scroll = (process % speed) as f32 * SEG / speed as f32;
                let crop = scroll / SEG;
                let phase = ((process % (3 * speed)) / speed) as usize;
                let half = (w.width + layer as f32) / 2.0;
                let depth = depth_axis * (layer as f32 - 1.0);
                for seg in 0..5usize {
                    let seg_top = scroll - seg as f32 * SEG;
                    let seg_bottom = seg_top - SEG;
                    let (mut vis_top, mut vis_bottom, mut v_top, mut v_bottom) = (seg_top, seg_bottom, 0.0, 1.0);
                    if seg == 0 {
                        vis_top = seg_top + (seg_bottom - seg_top) * crop;
                        v_top = crop;
                    } else if seg == 4 {
                        vis_bottom = seg_top + (seg_bottom - seg_top) * crop;
                        v_bottom = crop;
                    }
                    let tex = w.textures[(seg + phase) % 3];
                    let v = |side: f32, y: f32, vv: f32| Vertex { pos: (base + across * side * half - Vec3::Y * y + depth).into(), uv: [if side < 0.0 { 0.0 } else { 1.0 }, vv], color };
                    let (bl, br, tl, tr) = (v(-1.0, vis_bottom, v_bottom), v(1.0, vis_bottom, v_bottom), v(-1.0, vis_top, v_top), v(1.0, vis_top, v_top));
                    groups.entry(((5, 6, true, false), tex.0)).or_default().extend([bl, br, tl, tl, br, tr]);
                }
            }
        }
        for g in &self.glows {
            if g.pos.distance(focus) > range {
                continue;
            }
            let fwd = (g.pos - eye).normalize_or_zero();
            let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
            let up = right.cross(fwd);
            let turn = g.spin * now;
            let (s, c) = turn.sin_cos();
            let (right, up) = (right * c + up * s, up * c - right * s);
            let breathe = 0.8 + 0.2 * (now * 1.3 + g.phase).sin();
            let color = [g.color[0], g.color[1], g.color[2], g.color[3] * breathe];
            let (a, b) = (right * g.radius, up * g.radius);
            // Drawn a radius toward the eye so the model it lights (a crystal, a lamp) doesn't
            // swallow it, while walls in front still hide it.
            let centre = g.pos - fwd * g.radius;
            let v = |q: Vec3, u: f32, vv: f32| Vertex { pos: q.into(), uv: [u, vv], color };
            let (p0, p1, p2, p3) = (v(centre - a - b, 0.0, 1.0), v(centre + a - b, 1.0, 1.0), v(centre + a + b, 1.0, 0.0), v(centre - a + b, 0.0, 0.0));
            groups.entry(((5, 2, true, false), g.tex.0)).or_default().extend([p0, p1, p2, p0, p2, p3]);
        }
        groups.into_iter().map(|((k, t), v)| (k, TexId(t), v)).collect()
    }

    /// Replaces the map's glows.
    pub fn set_glows(&mut self, device: &wgpu::Device, glows: Vec<Glow>) {
        if !glows.is_empty() && !self.pipelines.contains_key(&(5, 2, true, false)) {
            let p = self.pipeline(device, (5, 2, true, false));
            self.pipelines.insert((5, 2, true, false), p);
        }
        self.glows = glows;
    }

    pub fn pipeline_for(&self, key: &Key) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty() && self.winds.is_empty() && self.waterfalls.is_empty() && self.glows.is_empty()
    }
}

fn avg(v: [f32; 2]) -> f32 {
    (v[0] + v[1]) * 0.5
}
