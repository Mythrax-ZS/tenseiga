//! HD-2D post-processing. The world is drawn into an HDR target; this chain adds bloom,
//! tilt-shift depth of field (the "diorama" blur toward the top and bottom of the screen),
//! a soft highlight shoulder, colour grading and a vignette, then writes the result to the
//! swapchain format. The UI is drawn afterwards, untouched.

/// Scene colour format: linear, with headroom above 1.0 for bloom.
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Artist-facing look controls. `enabled = false` gives the classic, unprocessed look.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PostSettings {
    pub enabled: bool,
    /// Brightness above which things glow, and how much they glow.
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    /// 0..1: how blurred the top and bottom of the screen get.
    pub dof_strength: f32,
    /// Half-height (fraction of the screen) of the sharp band around the player.
    pub dof_band: f32,
    pub vignette: f32,
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
    /// -1 cool .. 1 warm.
    pub warmth: f32,
    /// 0..1: how dark sprite shadows on the ground are (0 turns them off).
    pub shadow_strength: f32,
    /// Shadow edge blur in shadow-map texels.
    pub shadow_softness: f32,
    /// Sun height for sprite shadows in degrees (lower = longer shadows); 0 uses the map's.
    pub sun_elevation: f32,
    /// Minutes for the sun to cross the sky outdoors, from the map's own bearing swung
    /// 70° either way (0 keeps it still). It sets with the server's night and rises at day.
    pub sun_cycle_minutes: f32,
    /// 0..1: how much map light, sun shadow and point lights tint sprites.
    pub sprite_light: f32,
    /// Sun-coloured rim on sprite edges facing the sun.
    pub rim_light: f32,
    /// Brightness of map light sources (torches, lamps).
    pub point_lights: f32,
    /// Distance haze toward the far parts of the view (0 off).
    pub fog: f32,
    /// Light shafts streaming from the sun.
    pub god_rays: f32,
    /// Ambient occlusion in creases and under overhangs.
    pub ambient_occlusion: f32,
    /// 0..1: how far exposure follows the scene's brightness, as eyes adapt (brighter in a
    /// dark dungeon, a little darker on bright snow). 0 is a fixed exposure.
    pub auto_exposure: f32,
    /// Depth in texels of the relief read from ground and model textures (0 flat).
    pub relief: f32,
    /// Outdoors, the sun's shadows from all map geometry are ray traced (GPUs with ray
    /// queries; ignored elsewhere), in place of the shadows baked into the lightmaps.
    pub ray_traced_shadows: bool,
    /// Outdoors, water reflects the map ray traced, off screen too (same GPUs).
    pub ray_traced_reflections: bool,
    /// Sprites snap to whole screen pixels (crisp pixel art).
    pub pixel_snap: bool,
}

impl Default for PostSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            bloom_threshold: 0.92,
            bloom_intensity: 0.22,
            dof_strength: 0.85,
            dof_band: 0.16,
            vignette: 0.45,
            exposure: 1.0,
            auto_exposure: 0.0,
            relief: 0.0,
            ray_traced_shadows: false,
            ray_traced_reflections: false,
            contrast: 1.12,
            saturation: 1.1,
            warmth: 0.35,
            shadow_strength: 0.7,
            shadow_softness: 1.5,
            sun_elevation: 35.0,
            sun_cycle_minutes: 60.0,
            sprite_light: 0.8,
            rim_light: 0.35,
            point_lights: 1.0,
            fog: 0.35,
            god_rays: 0.25,
            ambient_occlusion: 0.6,
            pixel_snap: true,
        }
    }
}


/// Per-pixel depth info written by the world and sprites: x linear distance from the eye,
/// y 1 on sprites. Resolved from MSAA and sampled by AO, fog and depth of field.
pub const INFO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    texel: [f32; 4],
    bloom: [f32; 4],
    dof: [f32; 4],
    grade: [f32; 4],
    misc: [f32; 4],
    /// x: focus distance, y: depth blur range, z: fog start, w: fog end
    depth: [f32; 4],
    /// rgb: fog colour, a: fog strength
    fog: [f32; 4],
    /// rgb: outdoor sky zenith, a: gradient exponent (0: use the uniform fog colour).
    sky: [f32; 4],
    /// xy: sun position (uv), z: ray intensity, w: 1 when the sun is in front of the camera
    rays: [f32; 4],
    /// x: AO strength, y: sample radius (pixels at the focus distance)
    ao: [f32; 4],
    /// Underwater: x strength (0 off), y seconds, z aspect ratio
    water: [f32; 4],
    /// Eye position (world), for rebuilding world positions from depth.
    eye: [f32; 4],
    /// Auto exposure: x strength (0 off), y log2 of the luminance the eye settles to as
    /// neutral, z / w the exposure's range.
    adapt: [f32; 4],
    /// Inverse view-projection.
    inv_vp: [[f32; 4]; 4],
}

/// The same vertical colour gradient used by the outdoor sky backdrop.
#[derive(Clone, Copy, Debug)]
pub struct SkyFog {
    pub zenith: [f32; 3],
    pub gradient: f32,
    pub height_offset: f32,
}

/// What the post chain needs from the frame besides the settings.
pub struct FrameInfo {
    /// Distance from the eye to the player (the focal plane).
    pub focus: f32,
    /// Haze colour (from the map's ambient light).
    pub fog_color: [f32; 3],
    /// Outdoors, match the sky at this screen height rather than fogging toward a flat colour.
    pub sky: Option<SkyFog>,
    /// Where the sun sits on screen (uv), if in front of the camera.
    pub sun_uv: Option<[f32; 2]>,
    /// 0..1: the underwater look (absorption, caustics, shafts, drifting particles).
    pub underwater: f32,
    /// Heat haze (0 none .. 1): the air shimmering over deserts and volcanoes by day.
    pub heat: f32,
    /// Seconds, for the underwater animation (and the eye's adaptation).
    pub time: f32,
    /// The map's light now against its daylight (below 1 at night), so auto exposure
    /// leaves night dark instead of adapting it away.
    pub light_ratio: f32,
    pub eye: glam::Vec3,
    pub inv_view_proj: glam::Mat4,
}

/// Intermediate textures, sized from the window.
pub struct PostTargets {
    /// The resolved scene (render target + sampled).
    pub hdr: wgpu::TextureView,
    /// Resolved depth info (see INFO_FORMAT).
    pub info: wgpu::TextureView,
    /// The textures behind `hdr` and `info`, copied for the water each frame.
    pub hdr_tex: wgpu::Texture,
    pub info_tex: wgpu::Texture,
    half_bloom: wgpu::TextureView,
    half_dof: wgpu::TextureView,
    ao: wgpu::TextureView,
    bloom: [wgpu::TextureView; 2],
    dof: [wgpu::TextureView; 2],
    rays: wgpu::TextureView,
    /// The graded frame at the scene's size, when the world renders smaller than the window:
    /// grading runs per scene pixel and one plain pass scales the result up.
    graded: wgpu::TextureView,
    size: (u32, u32),
    /// The bloom / depth-of-field / AO chain's half and quarter sizes: of the scene, or of
    /// the window when supersampling, so blur radii stay the same on screen.
    half: (u32, u32),
    quarter: (u32, u32),
}

/// A render target that can also be copied from (the water reads the opaque scene).
fn copyable(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = tex.create_view(&Default::default());
    (tex, view)
}

fn target(device: &wgpu::Device, label: &str, w: u32, h: u32, format: wgpu::TextureFormat) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

impl PostTargets {
    /// Targets for a `w`x`h` scene shown in an `out`-sized window.
    pub fn new(device: &wgpu::Device, w: u32, h: u32, out: (u32, u32), output: wgpu::TextureFormat) -> Self {
        let (bw, bh) = (w.min(out.0), h.min(out.1));
        let (hw, hh, qw, qh) = ((bw / 2).max(1), (bh / 2).max(1), (bw / 4).max(1), (bh / 4).max(1));
        let hdr = |label, w, h| target(device, label, w, h, HDR_FORMAT);
        let (hdr_tex, hdr_view) = copyable(device, "hdr scene", w, h, HDR_FORMAT);
        let (info_tex, info_view) = copyable(device, "depth info", w, h, INFO_FORMAT);
        Self {
            hdr: hdr_view,
            info: info_view,
            hdr_tex,
            info_tex,
            half_bloom: hdr("bloom half", hw, hh),
            half_dof: hdr("dof half", hw, hh),
            ao: hdr("ambient occlusion", hw, hh),
            bloom: [hdr("bloom a", qw, qh), hdr("bloom b", qw, qh)],
            dof: [hdr("dof a", qw, qh), hdr("dof b", qw, qh)],
            rays: hdr("god rays", qw, qh),
            graded: target(device, "graded", w, h, output),
            size: (w.max(1), h.max(1)),
            half: (hw, hh),
            quarter: (qw, qh),
        }
    }

    /// The scene size these targets are for.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }
}

pub struct Post {
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    bright: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    ao: wgpu::RenderPipeline,
    rays: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    /// Plain copy to the output when post-processing is off.
    copy: wgpu::RenderPipeline,
    /// Auto exposure: the scene's log luminance at 64x64, averaged down to one texel (a
    /// geometric mean, which a few bright lights can't swing), then eased into one of
    /// `adapted` (alternating: one is read while the other is written).
    lum: wgpu::RenderPipeline,
    adapt: wgpu::RenderPipeline,
    meter: [wgpu::TextureView; 4],
    adapted: [wgpu::TextureView; 2],
    /// Which of `adapted` holds the eye's state, and when it was last updated (None:
    /// never, so the first frame takes the scene as it is).
    eye: std::cell::Cell<(usize, Option<f32>)>,
    /// One uniform buffer per pass (each has its own texel size / direction).
    uniforms: Vec<wgpu::Buffer>,
}

const PASSES: usize = 17;

/// The luminance the eye treats as neutral (exposure 1): a daylit town (Prontera's scene
/// averages about this, so towns keep their look and dark maps open up).
const NEUTRAL_LUMINANCE: f32 = 0.125;
/// Seconds for the eye to settle: opening up in the dark is slower than closing down.
const ADAPT_TO_DARK: f32 = 2.5;
const ADAPT_TO_BRIGHT: f32 = 0.8;
/// How far auto exposure may brighten and darken.
const EXPOSURE_RANGE: (f32, f32) = (0.6, 2.0);

impl Post {
    pub fn new(device: &wgpu::Device, output: wgpu::TextureFormat) -> Self {
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                tex(0),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                tex(3),
                tex(4),
                tex(5),
                tex(6),
                tex(7),
                tex(8),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("post"), source: wgpu::ShaderSource::Wgsl(include_str!("post.wgsl").into()) });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("post"), bind_group_layouts: &[Some(&layout)], ..Default::default() });
        let pipe = |entry: &str, format| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("post"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        let uniforms = (0..PASSES)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("post params"),
                    size: std::mem::size_of::<Params>() as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let meter = [64, 16, 4, 1].map(|n| target(device, "exposure meter", n, n, HDR_FORMAT));
        let adapted = [0, 1].map(|_| target(device, "adapted exposure", 1, 1, HDR_FORMAT));
        Self {
            lum: pipe("fs_lum", HDR_FORMAT),
            adapt: pipe("fs_adapt", HDR_FORMAT),
            meter,
            adapted,
            eye: std::cell::Cell::new((0, None)),
            down: pipe("fs_down", HDR_FORMAT),
            bright: pipe("fs_bright", HDR_FORMAT),
            blur: pipe("fs_blur", HDR_FORMAT),
            ao: pipe("fs_ao", HDR_FORMAT),
            rays: pipe("fs_rays", HDR_FORMAT),
            composite: pipe("fs_composite", output),
            copy: pipe("fs_down", output),
            layout,
            sampler,
            uniforms,
        }
    }

    /// Runs the chain from `t.hdr` (and `t.info`) into `out`, `out_size` pixels: when that
    /// differs from the scene (a render scale), grading runs at the scene's size and a last
    /// pass scales it into `out`.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        enc: &mut wgpu::CommandEncoder,
        t: &PostTargets,
        out: &wgpu::TextureView,
        s: &PostSettings,
        frame: &FrameInfo,
        out_size: (u32, u32),
    ) {
        let (w, h) = (t.size.0 as f32, t.size.1 as f32);
        let base = Params {
            bloom: [s.bloom_threshold, 0.5, s.bloom_intensity, 0.0],
            // The camera keeps the player at the centre; their body sits a little above it.
            dof: [s.dof_strength, 0.47, s.dof_band, 0.22],
            grade: [s.exposure, s.contrast, s.saturation, s.warmth],
            misc: [s.vignette, w / h, 1.0, frame.sky.map_or(0.0, |sky| sky.height_offset)],
            depth: [frame.focus, frame.focus * 0.6, frame.focus * 1.15, frame.focus * 2.6],
            fog: [frame.fog_color[0], frame.fog_color[1], frame.fog_color[2], s.fog],
            sky: frame.sky.map_or([0.0; 4], |sky| [sky.zenith[0], sky.zenith[1], sky.zenith[2], sky.gradient.max(0.01)]),
            rays: match frame.sun_uv {
                Some(uv) => [uv[0], uv[1], s.god_rays, 1.0],
                None => [0.5, -0.5, 0.0, 0.0],
            },
            ao: [s.ambient_occlusion, 14.0 * h / 1080.0, 0.0, 0.0],
            water: [if s.enabled { frame.underwater } else { 0.0 }, frame.time, w / h, if s.enabled { frame.heat } else { 0.0 }],
            eye: frame.eye.extend(1.0).to_array(),
            adapt: [s.auto_exposure, (NEUTRAL_LUMINANCE * frame.light_ratio.max(0.05)).log2(), EXPOSURE_RANGE.0, EXPOSURE_RANGE.1],
            inv_vp: frame.inv_view_proj.to_cols_array_2d(),
            ..Default::default()
        };
        let mut n = 0;
        let mut pass = |enc: &mut wgpu::CommandEncoder, pipe: &wgpu::RenderPipeline, src: &wgpu::TextureView, dst: &wgpu::TextureView, texel: [f32; 4], extra: [&wgpu::TextureView; 6]| {
            let buf = &self.uniforms[n];
            n += 1;
            queue.write_buffer(buf, 0, bytemuck::bytes_of(&Params { texel, ..base }));
            let view = |v| wgpu::BindingResource::TextureView(v);
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("post"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: view(src) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: view(extra[0]) },
                    wgpu::BindGroupEntry { binding: 4, resource: view(extra[1]) },
                    wgpu::BindGroupEntry { binding: 5, resource: view(extra[2]) },
                    wgpu::BindGroupEntry { binding: 6, resource: view(extra[3]) },
                    wgpu::BindGroupEntry { binding: 7, resource: view(extra[4]) },
                    wgpu::BindGroupEntry { binding: 8, resource: view(extra[5]) },
                ],
            });
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                ..Default::default()
            });
            rp.set_pipeline(pipe);
            rp.set_bind_group(0, &bind, &[]);
            rp.draw(0..3, 0..1);
        };
        // Textures a pass does not read: any valid view of the right kind.
        let none = [&t.hdr, &t.hdr, &t.info, &t.hdr, &t.hdr, &self.adapted[0]];
        // Into the window: a plain bilinear sample when scaling up; when supersampling, four
        // taps a quarter of a window pixel out, which average each pixel's block of scene.
        let scaled = t.size != out_size;
        let resample = if t.size.0 > out_size.0 { [0.25 / out_size.0 as f32, 0.25 / out_size.1 as f32, 0.0, 0.0] } else { [0.0; 4] };
        if !s.enabled {
            pass(enc, &self.copy, &t.hdr, out, resample, none);
            return;
        }
        let full = [1.0 / w, 1.0 / h, 0.0, 0.0];
        let (hw, hh) = (1.0 / t.half.0 as f32, 1.0 / t.half.1 as f32);
        let (qw, qh) = (1.0 / t.quarter.0 as f32, 1.0 / t.quarter.1 as f32);
        // Bloom: bright pass to half, down to quarter, blur twice.
        pass(enc, &self.bright, &t.hdr, &t.half_bloom, full, none);
        pass(enc, &self.down, &t.half_bloom, &t.bloom[0], [hw, hh, 0.0, 0.0], none);
        // God rays: march the bright pass toward the sun.
        pass(enc, &self.rays, &t.bloom[0], &t.rays, [qw, qh, 0.0, 0.0], none);
        pass(enc, &self.blur, &t.bloom[0], &t.bloom[1], [qw, qh, qw, 0.0], none);
        pass(enc, &self.blur, &t.bloom[1], &t.bloom[0], [qw, qh, 0.0, qh], none);
        // Depth of field source: the whole scene, softly blurred at quarter size.
        pass(enc, &self.down, &t.hdr, &t.half_dof, full, none);
        pass(enc, &self.down, &t.half_dof, &t.dof[0], [hw, hh, 0.0, 0.0], none);
        pass(enc, &self.blur, &t.dof[0], &t.dof[1], [qw, qh, qw, 0.0], none);
        pass(enc, &self.blur, &t.dof[1], &t.dof[0], [qw, qh, 0.0, qh], none);
        // Ambient occlusion from the depth info, at half size.
        pass(enc, &self.ao, &t.hdr, &t.ao, full, none);
        // Auto exposure: meter the scene, then ease the eye toward it.
        let (now, then) = self.eye.get();
        let next = 1 - now;
        if s.auto_exposure > 0.0 {
            pass(enc, &self.lum, &t.hdr, &self.meter[0], [0.25 / 64.0, 0.25 / 64.0, 0.0, 0.0], none);
            for (i, n) in [64.0, 16.0, 4.0].into_iter().enumerate() {
                pass(enc, &self.down, &self.meter[i], &self.meter[i + 1], [1.0 / n, 1.0 / n, 0.0, 0.0], none);
            }
            let dt = then.map_or(0.0, |t| (frame.time - t).max(0.0));
            let ease = |secs: f32| 1.0 - (-dt / secs).exp();
            let snap = if then.is_none() { 1.0 } else { 0.0 };
            // The eye before (read) and after (written): neither slot may hold the one written.
            let mut prev = none;
            prev[0] = &self.adapted[now];
            prev[5] = &self.adapted[now];
            pass(enc, &self.adapt, &self.meter[3], &self.adapted[next], [ease(ADAPT_TO_DARK), ease(ADAPT_TO_BRIGHT), snap, 0.0], prev);
            self.eye.set((next, Some(frame.time)));
        } else {
            self.eye.set((now, None));
        }
        let graded = if scaled { &t.graded } else { out };
        pass(enc, &self.composite, &t.hdr, graded, full, [&t.dof[0], &t.bloom[0], &t.info, &t.ao, &t.rays, &self.adapted[self.eye.get().0]]);
        if scaled {
            pass(enc, &self.copy, &t.graded, out, resample, none);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        let source = include_str!("post.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("post.wgsl validates");
    }
}
