//! Weather over the map (weather.wgsl): snow, rain, petals, leaves, fog banks, clouds. The
//! game says which kinds are on and how strongly (`Renderer::weather`); this turns each into
//! a layer of particles placed on the GPU from their index and the time. One tile's worth
//! repeats over the world, and as many tiles are drawn as cover what the camera sees (any
//! window shape, any zoom); lit by the map and hidden behind what stands in front of them.

use crate::effect::TexId;
use crate::CELL;

/// What falls (or drifts).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WeatherKind {
    Snow,
    Rain,
    /// Cherry blossom petals (EF_SAKURA).
    Petals,
    /// Maple leaves (EF_MAPLE).
    Leaves,
    /// Low banks of fog (EF_CLOUD4, the fog mapflag).
    Fog,
    /// Clouds drifting over (EF_CLOUD3 and EF_CLOUD5).
    Clouds,
}

/// One kind of weather on show: its texture (`frames` pictures side by side, one picked per
/// particle) and how strongly (0..1, thinning it out; the game eases it in and out).
#[derive(Clone, Copy, Debug)]
pub struct WeatherLayer {
    pub kind: WeatherKind,
    pub tex: TexId,
    pub frames: u32,
    pub strength: f32,
}

pub(crate) const MAX_LAYERS: usize = 8;
/// Instances a layer may draw (weather.wgsl's PER_LAYER): its are `index * PER_LAYER ..`.
pub(crate) const PER_LAYER: u32 = 1 << 19;
/// How far from the focus weather is drawn at most (cells), however far the camera sees.
const REACH: f32 = 140.0;

/// weather.wgsl's Layer.
#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Layer {
    a: [f32; 4],
    box_: [f32; 4],
    motion: [f32; 4],
    wind: [f32; 4],
    tint: [f32; 4],
    tiles: [f32; 4],
    more: [f32; 4],
}

/// How each kind looks and moves (sizes and speeds in cells, seconds).
struct Preset {
    /// Particles per tile.
    count: f32,
    look: f32,
    size: f32,
    spread: f32,
    /// Half a tile's width; its height, and how far its floor is below the focus.
    half: f32,
    height: f32,
    below: f32,
    fall: f32,
    sway: f32,
    sway_hz: f32,
    spin: f32,
    wind: [f32; 2],
    streak: f32,
    alpha: f32,
    tint: [f32; 3],
    lit: f32,
}

fn preset(kind: WeatherKind) -> Preset {
    match kind {
        WeatherKind::Snow => Preset { count: 4000.0, look: 0.0, size: 0.11, spread: 0.8, half: 26.0, height: 24.0, below: 3.0, fall: 1.4, sway: 0.35, sway_hz: 0.9, spin: 0.0, wind: [0.4, 0.15], streak: 0.0, alpha: 0.9, tint: [1.0, 1.0, 1.0], lit: 0.6 },
        WeatherKind::Rain => Preset { count: 6000.0, look: 1.0, size: 0.018, spread: 0.6, half: 24.0, height: 22.0, below: 3.0, fall: 26.0, sway: 0.0, sway_hz: 0.0, spin: 0.0, wind: [2.5, 0.8], streak: 0.55, alpha: 0.35, tint: [0.78, 0.84, 0.95], lit: 0.7 },
        WeatherKind::Petals => Preset { count: 700.0, look: 2.0, size: 0.16, spread: 0.6, half: 26.0, height: 18.0, below: 2.0, fall: 0.9, sway: 1.1, sway_hz: 0.6, spin: 0.35, wind: [0.8, 0.3], streak: 0.0, alpha: 1.0, tint: [1.0, 1.0, 1.0], lit: 0.7 },
        WeatherKind::Leaves => Preset { count: 320.0, look: 2.0, size: 0.22, spread: 0.5, half: 26.0, height: 18.0, below: 2.0, fall: 1.1, sway: 1.3, sway_hz: 0.5, spin: 0.3, wind: [0.9, 0.2], streak: 0.0, alpha: 1.0, tint: [1.0, 1.0, 1.0], lit: 0.75 },
        WeatherKind::Fog => Preset { count: 90.0, look: 3.0, size: 7.0, spread: 0.6, half: 30.0, height: 2.4, below: 0.4, fall: 0.0, sway: 0.6, sway_hz: 0.05, spin: 0.0, wind: [0.35, 0.12], streak: 0.0, alpha: 0.3, tint: [0.92, 0.94, 0.98], lit: 0.8 },
        WeatherKind::Clouds => Preset { count: 45.0, look: 3.0, size: 10.0, spread: 0.6, half: 34.0, height: 6.0, below: -14.0, fall: 0.0, sway: 0.0, sway_hz: 0.0, spin: 0.0, wind: [1.0, 0.35], streak: 0.0, alpha: 0.32, tint: [1.0, 1.0, 1.0], lit: 0.8 },
    }
}

/// A layer's uniform, and how many instances to draw: `focus` is the camera's target (world
/// units), and `inv_view_proj` says what the camera sees, which the tiles cover.
pub(crate) fn layer(w: &WeatherLayer, focus: glam::Vec3, inv_view_proj: glam::Mat4) -> (Layer, u32) {
    let p = preset(w.kind);
    let tile = 2.0 * p.half * CELL;
    let floor = focus.y - p.below * CELL;
    let per = p.count.min(PER_LAYER as f32) as u32;
    let (x0, z0, nx, nz) = tiles(focus, inv_view_proj, tile, floor, floor + p.height * CELL, p.size * CELL * 1.5, PER_LAYER / per.max(1));
    let l = Layer {
        a: [per as f32, p.look, p.size * CELL, w.strength.clamp(0.0, 1.0)],
        box_: [tile, p.height * CELL, tile, floor],
        motion: [p.fall * CELL, p.sway * CELL, p.sway_hz * std::f32::consts::TAU, p.spin],
        wind: [p.wind[0] * CELL, p.wind[1] * CELL, p.streak * CELL, p.alpha],
        tint: [p.tint[0], p.tint[1], p.tint[2], p.lit],
        tiles: [x0 as f32, z0 as f32, nx as f32, w.frames.max(1) as f32],
        more: [p.spread, 0.0, 0.0, 0.0],
    };
    (l, per * nx * nz)
}

/// The tiles (`tile` wide, on a grid from the world's origin) covering what the camera sees
/// between heights `floor` and `top`, widened by `pad` (particles reach past their tile): the
/// first tile's x and z, and how many across and deep, at most `most` in all.
fn tiles(focus: glam::Vec3, inv_view_proj: glam::Mat4, tile: f32, floor: f32, top: f32, pad: f32, most: u32) -> (i32, i32, u32, u32) {
    let reach = REACH * CELL;
    let centre = glam::Vec2::new(focus.x, focus.z);
    let (mut lo, mut hi) = (centre, centre);
    let mut take = |p: glam::Vec3| {
        let q = centre + (glam::Vec2::new(p.x, p.z) - centre).clamp_length_max(reach);
        lo = lo.min(q);
        hi = hi.max(q);
    };
    for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        let near = inv_view_proj.project_point3(glam::Vec3::new(x, y, 0.0));
        let far = inv_view_proj.project_point3(glam::Vec3::new(x, y, 1.0));
        let dir = far - near;
        take(near);
        for h in [floor, top] {
            // Where this corner's ray crosses the height; else as far as it goes, if it heads
            // that way, or where it starts.
            let t = if dir.y.abs() > 1e-6 { (h - near.y) / dir.y } else { f32::INFINITY };
            take(if (0.0..=1.0).contains(&t) { near + dir * t } else if t > 1.0 { far } else { near });
        }
    }
    let first = ((lo - pad) / tile).floor();
    let last = ((hi + pad) / tile).floor();
    let mut n = last - first + glam::Vec2::ONE;
    // A camera seeing a great deal at once: the tiles nearest the focus.
    while (n.x * n.y) as u32 > most.max(1) {
        n = (n - glam::Vec2::ONE).max(glam::Vec2::ONE);
    }
    let home = (centre / tile).floor();
    let first = first.max(home - (n - glam::Vec2::ONE)).min(home);
    (first.x as i32, first.y as i32, n.x as u32, n.y as u32)
}

/// How overcast the weather makes the sky (0 clear .. 1): the strongest layer's say.
pub(crate) fn overcast(layers: &[WeatherLayer]) -> f32 {
    layers
        .iter()
        .map(|w| {
            let o = match w.kind {
                WeatherKind::Rain => 0.8,
                WeatherKind::Snow => 0.45,
                WeatherKind::Clouds | WeatherKind::Fog => 0.35,
                WeatherKind::Petals | WeatherKind::Leaves => 0.0,
            };
            o * w.strength.clamp(0.0, 1.0)
        })
        .fold(0.0, f32::max)
}

pub(crate) struct WeatherPass {
    pub pipeline: wgpu::RenderPipeline,
    pub uniform: wgpu::Buffer,
    pub bind: wgpu::BindGroup,
}

impl WeatherPass {
    pub fn new(device: &wgpu::Device, globals: &wgpu::BindGroupLayout, texture: &wgpu::BindGroupLayout, samples: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("weather"), source: wgpu::ShaderSource::Wgsl(include_str!("weather.wgsl").into()) });
        let size = (std::mem::size_of::<Layer>() * MAX_LAYERS) as u64;
        let uniform = device.create_buffer(&wgpu::BufferDescriptor { label: Some("weather layers"), size, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let layers_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("weather layers"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("weather layers"), layout: &layers_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }] });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("weather"), bind_group_layouts: &[Some(globals), Some(texture), Some(&layers_layout)], ..Default::default() });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("weather"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: crate::DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: crate::post::HDR_FORMAT, blend: Some(wgpu::BlendState::ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: crate::post::INFO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { pipeline, uniform, bind }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_fit_the_shader() {
        // weather.wgsl reads an array of 8 layers of seven vec4s.
        assert_eq!(std::mem::size_of::<Layer>(), 7 * 16);
        let mut cam = crate::Camera::default();
        cam.distance = crate::Camera::MAX_DISTANCE;
        let inv = cam.view_proj(32.0 / 9.0).inverse();
        for kind in [WeatherKind::Snow, WeatherKind::Rain, WeatherKind::Petals, WeatherKind::Leaves, WeatherKind::Fog, WeatherKind::Clouds] {
            let (_, n) = layer(&WeatherLayer { kind, tex: TexId(0), frames: 1, strength: 1.0 }, cam.target, inv);
            assert!(n > 0 && n <= PER_LAYER, "{kind:?}: {n}");
        }
    }

    /// Tiles cover what the camera sees on the ground: all of a wide window, not a box.
    #[test]
    fn the_tiles_cover_the_view() {
        let mut cam = crate::Camera::default();
        cam.look_at_cell(150, 150);
        cam.distance = crate::Camera::MAX_DISTANCE;
        for (aspect, yaw) in [(16.0 / 9.0, 0.0), (32.0 / 9.0, 0.0), (32.0 / 9.0, 0.8)] {
            cam.yaw = yaw;
            let inv = cam.view_proj(aspect).inverse();
            let tile = 52.0 * CELL;
            let (x0, z0, nx, nz) = tiles(cam.target, inv, tile, cam.target.y - 3.0 * CELL, cam.target.y + 21.0 * CELL, 0.0, u32::MAX);
            let lo = glam::Vec2::new(x0 as f32, z0 as f32) * tile;
            let hi = glam::Vec2::new((x0 + nx as i32) as f32, (z0 + nz as i32) as f32) * tile;
            // The screen's corners and edges, on the ground.
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0), (0.0, 1.0), (0.0, -1.0), (1.0, 0.0), (-1.0, 0.0)] {
                let near = inv.project_point3(glam::Vec3::new(sx, sy, 0.0));
                let far = inv.project_point3(glam::Vec3::new(sx, sy, 1.0));
                let g = near + (far - near) * ((cam.target.y - near.y) / (far.y - near.y));
                let g = glam::Vec2::new(g.x, g.z);
                if g.distance(glam::Vec2::new(cam.target.x, cam.target.z)) > REACH * CELL {
                    continue;
                }
                assert!(g.cmpge(lo).all() && g.cmple(hi).all(), "{aspect} {yaw}: {g} outside {lo}..{hi}");
            }
            assert!(nx * nz > 1, "a wide view needs more than one tile");
        }
    }

    #[test]
    fn too_many_tiles_keep_those_by_the_focus() {
        let mut cam = crate::Camera::default();
        cam.look_at_cell(150, 150);
        let (x0, z0, nx, nz) = tiles(cam.target, cam.view_proj(4.0).inverse(), 10.0, 0.0, 10.0, 0.0, 4);
        assert!(nx * nz <= 4);
        let c = (glam::Vec2::new(cam.target.x, cam.target.z) / 10.0).floor();
        assert!((x0 as f32..(x0 + nx as i32) as f32).contains(&c.x) && (z0 as f32..(z0 + nz as i32) as f32).contains(&c.y));
    }

    #[test]
    fn rain_clouds_the_sky_over_more_than_petals() {
        let at = |kind, strength| WeatherLayer { kind, tex: TexId(0), frames: 1, strength };
        assert_eq!(overcast(&[]), 0.0);
        assert_eq!(overcast(&[at(WeatherKind::Petals, 1.0)]), 0.0);
        assert!(overcast(&[at(WeatherKind::Rain, 1.0)]) > overcast(&[at(WeatherKind::Snow, 1.0)]));
        // Easing in: half a shower, half the gloom.
        assert!((overcast(&[at(WeatherKind::Rain, 0.5)]) - 0.4).abs() < 1e-6);
    }

    #[test]
    fn the_tiles_follow_the_focus_and_strength_is_kept_in_range() {
        let w = WeatherLayer { kind: WeatherKind::Snow, tex: TexId(0), frames: 1, strength: 3.0 };
        let mut cam = crate::Camera::default();
        cam.target = glam::Vec3::new(1000.0, 20.0, 500.0);
        let (l, _) = layer(&w, cam.target, cam.view_proj(16.0 / 9.0).inverse());
        let tile = l.box_[0];
        assert!(l.tiles[0] * tile <= 1000.0 && (l.tiles[0] + l.tiles[2]) * tile >= 1000.0);
        assert!(l.tiles[1] * tile <= 500.0);
        assert_eq!(l.a[3], 1.0);
        assert!(l.box_[3] < 20.0, "the box starts below the focus");
    }
}
