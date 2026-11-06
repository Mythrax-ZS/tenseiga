//! wgpu renderer. Vulkan preferred; DX12 / GL selectable via config.

mod camera;
mod scene;
mod effect;
mod particle;
mod fx;
pub mod model3d;
mod post;
mod rt;
mod shadow;
mod sky;
mod sky_profile;
mod sprite;
mod upscale;
mod water;
mod weather;
mod windows;

use std::sync::Arc;

use wgpu::util::DeviceExt;

pub use camera::{Camera, CELL};
pub use ro_formats::image::Rgba;
pub use scene::{rsm_at, SceneBuilder, SceneMesh, Vertex as ModelVertex};
pub use upscale::{is_sprite_path, AssetClass, Quality as UpscaleQuality, UpscaleSettings, Upscaler};
pub use water::{flag as water_flag, Foam, WaterMesh, WaterQuality, WaterSettings};
pub use weather::{WeatherKind, WeatherLayer};
pub use windows::window_mask;
pub use effect::{AuraLayer, AuraStyle, FireworkPart, ProcFx, Shape, TenseigaPart, TexId, FIREWORK_BURST_MS, FIREWORK_MS};
pub use particle::{Emitter, EmitterDef, Glow, Waterfall, Wind};
pub use fx::{Fx, FxId};
pub use post::PostSettings;
pub use sky_profile::SkyProfile;
pub use sprite::{Actor, Dir, SheetId};
use winit::window::Window;

const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// sRGB (0.05, 0.06, 0.09) expressed in linear space.
const CLEAR: wgpu::Color = wgpu::Color { r: 0.0039, g: 0.0050, b: 0.0080, a: 1.0 };

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Globals {
    view_proj: [[f32; 4]; 4],
    focus: [f32; 4],
    eye: [f32; 4],
    light_dir: [f32; 4],
    ambient: [f32; 4],
    diffuse: [f32; 4],
    /// x: seconds since start
    params: [f32; 4],
    /// The sun's view-projection for the sprite shadow map.
    light_vp: [[f32; 4]; 4],
    /// x: shadow strength (0 off), y: shadow-map texel size, z: softness (texels)
    shadow: [f32; 4],
    /// Output size in pixels: w, h, 1/w, 1/h.
    screen: [f32; 4],
    /// x: sprite lighting strength, y: rim light, z: point light intensity, w: light count.
    sprite_light: [f32; 4],
    /// Point lights near the camera, as pairs: (position, range), (colour, 0).
    lights: [[f32; 4]; MAX_LIGHTS * 2],
    /// Room mask: x 1 when active, y/z mask size in cells.
    room: [f32; 4],
    /// Wading: x water level (y-up), y 1 when sprites sink into the water, z world units
    /// per sprite pixel.
    water: [f32; 4],
    /// Wading: the water's scatter colour (linear rgb).
    water_tint: [f32; 4],
    /// x: 1 to filter magnified world textures with Catmull-Rom (texture upscaling).
    upscale: [f32; 4],
    /// Ray tracing: xyz the direction sunlight travels, w what is on (bits: 1 shadows, 2 reflections).
    rt: [f32; 4],
    /// The torches whose light sprites shadow (`TORCHES`): view-projections of their shadow
    /// maps, (position, range), and x/y each one's shadow strength (0 none), z texel size.
    torch_vp: [[[f32; 4]; 4]; TORCHES],
    torch: [[f32; 4]; TORCHES],
    torch_k: [f32; 4],
    /// Cloud shadows: x how much of the sun a cloud takes (0 off), y how much of the sky is
    /// cloud (0..1), zw their drift (world units a second).
    clouds: [f32; 4],
    /// Wet ground: x how wet (0..1), y how hard it rains now (drop rings); the sky's colour.
    wet: [f32; 4],
    wet_sky: [f32; 4],
    /// Wind for swaying foliage: xy its direction (unit, world xz), z how strong (0 still).
    wind: [f32; 4],
    /// Night: x how much the windows glow (0 by day).
    night: [f32; 4],
}

/// Torches near the player that cast sprite shadows, each with a shadow map.
const TORCHES: usize = 2;
/// Highest a torch casts sprite shadows from, above the player (world units: 2.5 cells).
const TORCH_HEIGHT: f32 = 25.0;

/// One frame of tessellated egui output.
/// A registered 3D model texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModelTexId(pub usize);

/// Triangles of a posed 3D model sharing one texture (world space, y-up).
#[derive(Clone)]
pub struct ModelDraw {
    pub texture: ModelTexId,
    pub vertices: Vec<ModelVertex>,
}

pub struct UiFrame {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

/// Point lights sent to the shaders each frame (the nearest to the camera).
pub const MAX_LIGHTS: usize = 32;

/// A map light source (RSW): torches, lamps, glowing crystals.
#[derive(Clone, Copy, Debug)]
pub struct PointLight {
    pub pos: glam::Vec3,
    pub color: [f32; 3],
    pub range: f32,
}

impl PointLight {
    /// Brightness at `t` seconds: fire-coloured lights (red over green over blue: torches,
    /// braziers, lanterns) flicker by up to a quarter, each on its own beat (map lights are dim
    /// beside the lightmaps, so less hardly shows); others hold steady.
    fn flicker(&self, t: f32) -> f32 {
        let [r, g, b] = self.color;
        if !(r > g * 1.15 && g >= b) {
            return 1.0;
        }
        let phase = (self.pos.x * 0.37 + self.pos.z * 0.73).fract() * std::f32::consts::TAU;
        1.0 + 0.12 * (t * 7.3 + phase).sin() + 0.08 * (t * 13.1 + phase * 2.0).sin() + 0.05 * (t * 23.7 + phase * 3.0).sin()
    }
}

/// Map-wide sun lighting (from the RSW), in the renderer's y-up space.
#[derive(Clone, Copy, Debug)]
pub struct Lighting {
    /// Direction the light travels.
    pub direction: [f32; 3],
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
}

impl Default for Lighting {
    fn default() -> Self {
        Self { direction: [-0.5, -0.7, -0.5], ambient: [0.5; 3], diffuse: [1.0; 3] }
    }
}

impl Lighting {
    pub fn from_rsw(light: &ro_formats::rsw::Light) -> Self {
        let [x, y, z] = light.direction();
        Self { direction: [x, -y, z], ambient: light.ambient, diffuse: light.diffuse }
    }
}

/// Size-dependent render targets, rebuilt on resize.
struct Targets {
    depth: wgpu::TextureView,
    /// None when MSAA is off.
    msaa: Option<wgpu::TextureView>,
    /// Multisampled depth-info target (resolved into `post.info`); None when MSAA is off.
    msaa_info: Option<wgpu::TextureView>,
    /// HDR scene and post-processing intermediates.
    post: post::PostTargets,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    /// Kept to make a new surface when the old one is lost (Android, after a resume).
    instance: wgpu::Instance,
    /// Where frames are drawn when the surface can't take a second view format (see new).
    relay: Option<wgpu::Texture>,
    /// The GPU and graphics API in use ("Adreno (TM) 740 (Vulkan)"), for the performance overlay.
    adapter_name: String,
    /// The world's resolution as a share of the window's (0.5..2): below 1 the scene and its
    /// effects render smaller and the last pass scales them up; above 1 they render larger and
    /// it averages them down (supersampling). The interface stays at the window's resolution.
    render_scale: f32,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    samples: u32,
    targets: Targets,
    globals: wgpu::Buffer,
    globals_bind: wgpu::BindGroup,
    globals_layout: wgpu::BindGroupLayout,
    /// Indoor room mask size in cells (None: draw everything).
    room: Option<(u32, u32)>,
    grid: wgpu::RenderPipeline,
    scene_pipeline: wgpu::RenderPipeline,
    /// Ray-traced shadows (GPUs with ray queries): the world shader's group 2 layout, and the
    /// map's acceleration structure bound to it.
    rt_layout: Option<wgpu::BindGroupLayout>,
    /// The map's ground surface (for effects laid over it).
    ground: scene::Heights,
    /// Shadow maps of the torches nearest the player (`TORCHES`).
    torches: [shadow::Shadow; TORCHES],
    rt_bind: Option<wgpu::BindGroup>,
    sky: sky::Sky,
    sky_profile: Option<SkyProfile>,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    lightmap_sampler: wgpu::Sampler,
    water_pipeline: wgpu::RenderPipeline,
    water_layout: wgpu::BindGroupLayout,
    water_screen_layout: wgpu::BindGroupLayout,
    /// Opaque-scene copies for refraction / depth / reflections, sized with the window.
    water_screen: water::ScreenCopies,
    /// Shared ripple normal / height / foam texture.
    water_detail: wgpu::TextureView,
    clamp_sampler: wgpu::Sampler,
    water_settings: water::WaterSettings,
    /// A caller-supplied sky (equirectangular) and its mean radiance; None generates one.
    water_sky: Option<(Rgba, [f32; 3])>,
    repeat_sampler: wgpu::Sampler,
    water: Option<water::Water>,
    sprite_pipeline: wgpu::RenderPipeline,
    /// Actors with a silhouette, drawn again where the scene hides them.
    silhouette_pipeline: wgpu::RenderPipeline,
    sprite_layout: wgpu::BindGroupLayout,
    sheets: Vec<sprite::Sheet>,
    ui_renderer: egui_wgpu::Renderer,
    /// UI textures egui freed last frame: destroyed only once that frame was submitted,
    /// since its draw commands could still use them.
    ui_free: Vec<egui::TextureId>,
    /// UI to draw over the next frame (set by the caller each frame).
    ui: Option<UiFrame>,
    /// Actors drawn each frame; the caller updates positions, actions and times.
    pub actors: Vec<Actor>,
    /// 0..1: the underwater screen look for submerged maps (HD-2D only).
    pub underwater: f32,
    /// Run-time Catmull-Rom filtering of ground and model textures (never sprites).
    pub upscale_runtime: bool,
    /// Warp portal positions drawn each frame.
    pub warps: Vec<glam::Vec3>,
    warp_texture: Option<wgpu::BindGroup>,
    fx_pipeline: wgpu::RenderPipeline,
    fx_assets: Vec<fx::FxAsset>,
    effect_textures: Vec<wgpu::BindGroup>,
    /// Textures of the 3D monster models (Granny), in the scene's bind group layout, and the
    /// flat lightmap they share (models are lit by the sun and lamps, not the map's lightmap).
    model_textures: Vec<wgpu::BindGroup>,
    model_lightmap: wgpu::TextureView,
    /// Posed 3D model triangles to draw this frame (world space, y-up); the client rebuilds
    /// the list every frame.
    pub models: Vec<ModelDraw>,
    post: post::Post,
    shadow: shadow::Shadow,
    /// The HD-2D look (bloom, tilt-shift, grading); `enabled = false` for the classic look.
    pub post_settings: PostSettings,
    /// The map's light sources.
    pub point_lights: Vec<PointLight>,
    /// Night mode (the server's day/night cycle); the look fades over `night_t`.
    pub night: bool,
    night_t: f32,
    /// Direction sprite shadows are cast in this frame: the sun, bent away from a strong
    /// nearby light source (eased so it swings smoothly).
    cast_dir: glam::Vec3,
    /// Seconds since the sun last rose (day began), for the moving sun.
    day_clock: f32,
    /// Where in the day the clock started: mid-morning on login, sunrise after a night.
    day_start: f32,
    was_night: bool,
    last_frame: std::time::Instant,
    /// Code-drawn effects playing this frame; the client advances and removes them.
    pub procs: Vec<ProcFx>,
    /// Level auras under the units that carry them, drawn before the actors; the client
    /// rebuilds the list every frame.
    pub auras: Vec<ProcFx>,
    /// Map particle emitters (RSW EF_EMITTER).
    particles: particle::Particles,
    weather_pass: weather::WeatherPass,
    /// Weather on show (the game eases each layer's strength in and out).
    pub weather: Vec<WeatherLayer>,
    /// STR effects playing this frame; the client advances and removes them.
    pub effects: Vec<Fx>,
    /// Night life: windows glowing warm (and, in the game, fireflies).
    pub night_lights: bool,
    /// Heat haze over deserts and volcanoes by day (with the HD-2D look).
    pub heat_haze: bool,
    /// Foliage swaying in the wind (0 still .. 1 lively); the weather makes it blow harder.
    pub foliage_sway: f32,
    /// Ice on the water of snowy maps (0 open water .. 1 frozen over).
    pub icy_water: f32,
    /// A lightning flash lighting the map this frame (0 none .. about 1): the game sets it
    /// while a storm flashes. Outdoors only.
    pub lightning: f32,
    /// How wet the ground is (0 dry .. 1 soaked): it soaks in over the first half minute of
    /// rain and dries over a couple of minutes after.
    wetness: f32,
    /// Cloud shadows drifting over outdoor maps (0 off .. 1 deep): how much of the sun a
    /// passing cloud takes on a fair day; cloudy weather brings more of them, and deeper.
    pub cloud_shadows: f32,
    /// Draw skill effects (STR and code-drawn); the original's /effect turns them off.
    pub show_effects: bool,
    effect_pipeline: wgpu::RenderPipeline,
    /// The same geometry alpha-blended (effects that darken, see `ProcFx::shades`).
    shade_pipeline: wgpu::RenderPipeline,
    started: std::time::Instant,
    /// Photo mode: time stands still from this moment (and all the time it stood still
    /// before, which the clock leaves out).
    frozen_at: Option<std::time::Instant>,
    frozen_for: std::time::Duration,
    /// The grid is drawn instead while no map is loaded.
    scene: Option<scene::Scene>,
    /// While a new map's textures go up (Scene::pump): since when, over how many frames.
    uploading: Option<(std::time::Instant, u32)>,
    pub lighting: Lighting,
    pub camera: Camera,
    pub scale_factor: f64,
}

pub fn parse_backends(name: &str) -> wgpu::Backends {
    match name.to_ascii_lowercase().as_str() {
        "vulkan" => wgpu::Backends::VULKAN,
        "dx12" => wgpu::Backends::DX12,
        "gl" => wgpu::Backends::GL,
        "metal" => wgpu::Backends::METAL,
        _ => wgpu::Backends::all(),
    }
}

impl Renderer {
    pub async fn new(window: Arc<Window>, backends: wgpu::Backends, msaa: u32) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_without_display_handle_from_env()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let info = adapter.get_info();
        tracing::info!(backend = ?info.backend, name = %info.name, "adapter");

        let rt_needs = rt::device_needs(&adapter);
        let (device, queue) = adapter
            .request_device(&match rt_needs.clone() {
                // SAFETY: ray queries are experimental in wgpu; only the world shader's shadow
                // ray uses them, and only where the adapter reports support.
                Some((features, limits)) => wgpu::DeviceDescriptor { required_features: features, required_limits: limits, experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() }, ..Default::default() },
                None => wgpu::DeviceDescriptor::default(),
            })
            .await?;
        tracing::info!(ray_tracing = rt_needs.is_some(), "device");
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("surface not supported by adapter"))?;
        // Shaders output linear color; let the sRGB view encode it.
        let caps = surface.get_capabilities(&adapter);
        if let Some(srgb) = caps.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        // egui wants a non-sRGB view of the frame. Most drivers let the window surface have
        // both views; where one can't (some phone GPUs), frames are drawn into a texture that
        // can, and copied to the window.
        let views = adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::SURFACE_VIEW_FORMATS);
        let relayed = !views && config.format.is_srgb() && caps.usages.contains(wgpu::TextureUsages::COPY_DST);
        if !views && !relayed {
            // Neither: a plain (non-sRGB) surface. It runs, but the world shows darker.
            config.format = config.format.remove_srgb_suffix();
        }
        if relayed {
            config.usage |= wgpu::TextureUsages::COPY_DST;
            tracing::info!("the window can't take a second view format: drawing through a texture");
        } else if views {
            config.view_formats = vec![config.format.remove_srgb_suffix()];
        }
        surface.configure(&device, &config);
        let relay = relayed.then(|| Self::make_relay(&device, &config));

        // The world renders into an HDR target (post::HDR_FORMAT); MSAA must suit that format.
        let supported = adapter.get_texture_format_features(post::HDR_FORMAT).flags;
        let samples = [msaa, 4, 2, 1]
            .into_iter()
            .find(|&s| s == 1 || (s <= msaa && supported.sample_count_supported(s)))
            .unwrap_or(1);
        tracing::info!(format = ?config.format, samples, "surface");

        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Sun shadow map (sprites seen from the sun) and its comparison sampler.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                // Room mask (indoor maps): which cells are drawn, one texel per GAT cell.
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Torch shadow maps (sprites seen from the nearest torches).
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // The texture's windows (windows.rs): red glass, green a number per window.
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let lightmap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lightmap"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        let (water_layout, water_screen_layout) = water::layouts(&device);
        let clamp_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("water clamp"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let repeat_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("repeat"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        // `blend: Some(..)` makes a transparent pass: blended, depth-tested but not written.
        let pipeline = |label: &str,
                        src: &str,
                        layouts: &[Option<&wgpu::BindGroupLayout>],
                        buffers: &[Option<wgpu::VertexBufferLayout>],
                        blend: Option<wgpu::BlendState>| {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                ..Default::default()
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(blend.is_none()),
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
                        Some(wgpu::ColorTargetState { format: post::HDR_FORMAT, blend, write_mask: wgpu::ColorWrites::ALL }),
                        // Depth info for post-processing (AO, fog, depth of field): written by
                        // the opaque world and by sprites only.
                        Some(wgpu::ColorTargetState {
                            format: post::INFO_FORMAT,
                            blend: None,
                            write_mask: if matches!(label, "scene" | "sprite") { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() },
                        }),
                    ],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let grid = pipeline("grid", include_str!("grid.wgsl"), &[Some(&globals_layout)], &[], None);
        let rt_layout = rt_needs.is_some().then(|| rt::layout(&device));
        // With ray tracing, rt.wgsl's shadow and reflection rays replace the stubs at the end of
        // the world, sprite and water shaders (bound as `group`).
        let with_rt = |src: &str, group| match &rt_layout {
            Some(_) => rt::join(src, group),
            None => src.to_string(),
        };
        let scene_pipeline = pipeline(
            "scene",
            &with_rt(include_str!("scene.wgsl"), 2),
            &[Some(&globals_layout), Some(&texture_layout), rt_layout.as_ref()],
            &[Some(scene::Vertex::layout())],
            None,
        );
        let water_pipeline = pipeline(
            "water",
            &with_rt(include_str!("water.wgsl"), 3),
            &[Some(&globals_layout), Some(&water_layout), Some(&water_screen_layout), rt_layout.as_ref()],
            &[Some(water::Vertex::layout())],
            Some(wgpu::BlendState::ALPHA_BLENDING),
        );
        let sprite_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // The sun shadow map is bound with the globals so world shaders can sample it.
        let shadow = shadow::Shadow::new(&device, &sprite_layout, shadow::SIZE);
        let torches = [(); TORCHES].map(|_| shadow::Shadow::new(&device, &sprite_layout, shadow::TORCH_SIZE));
        let particles = particle::Particles::new(&device, &globals_layout, &sprite_layout, samples);
        let weather_pass = weather::WeatherPass::new(&device, &globals_layout, &sprite_layout, samples);
        let room_mask = room_mask_view(&device, &queue, 1, 1, &[255]);
        let globals_bind = globals_bind_group(&device, &globals_layout, &globals, &shadow, &torches, &room_mask);
        let effect_pipeline = pipeline(
            "effect",
            include_str!("effect.wgsl"),
            &[Some(&globals_layout), Some(&sprite_layout)],
            &[Some(effect::Vertex::layout())],
            Some(wgpu::BlendState {
                color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
                alpha: wgpu::BlendComponent::OVER,
            }),
        );
        let shade_pipeline = pipeline(
            "effect shade",
            &include_str!("effect.wgsl").replace("const SHADE: bool = false;", "const SHADE: bool = true;"),
            &[Some(&globals_layout), Some(&sprite_layout)],
            &[Some(effect::Vertex::layout())],
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sprite_pipeline = pipeline(
            "sprite",
            &with_rt(include_str!("sprite.wgsl"), 2),
            &[Some(&globals_layout), Some(&sprite_layout), rt_layout.as_ref()],
            &[Some(sprite::Vertex::layout())],
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let silhouette_pipeline = {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("silhouette"), source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()) });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("silhouette"), bind_group_layouts: &[Some(&globals_layout), Some(&sprite_layout)], ..Default::default() });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("silhouette"),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &shader, entry_point: Some("vs_silhouette"), compilation_options: Default::default(), buffers: &[Some(sprite::Vertex::layout())] },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    // Only where something stands in front of it.
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_silhouette"),
                    compilation_options: Default::default(),
                    targets: &[
                        Some(wgpu::ColorTargetState { format: post::HDR_FORMAT, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL }),
                        Some(wgpu::ColorTargetState { format: post::INFO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::empty() }),
                    ],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let fx_pipeline = pipeline(
            "str effects",
            include_str!("strfx.wgsl"),
            &[Some(&globals_layout), Some(&sprite_layout)],
            &[Some(sprite::Vertex::layout())],
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );

        let ui_renderer = egui_wgpu::Renderer::new(&device, config.format.remove_srgb_suffix(), Default::default());
        let targets = Self::make_targets(&device, (config.width, config.height), (config.width, config.height), samples, config.format);
        let water_screen = water::ScreenCopies::new(&device, &water_screen_layout, config.width, config.height);
        let water_detail = scene::upload_texture(&device, &queue, &water::detail_texture(), true);
        let post = post::Post::new(&device, config.format);
        let sky = sky::Sky::new(&device, samples);
        let model_lightmap = scene::upload_texture(&device, &queue, &Rgba { width: 1, height: 1, pixels: vec![0, 0, 0, 255] }, false);
        // Something to bind before the first map.
        let rt_bind = rt_layout.as_ref().map(|l| rt::build(&device, &queue, l, &[], &[], &[]));
        Ok(Self {
            surface,
            instance,
            relay,
            adapter_name: format!("{} ({:?})", info.name, info.backend),
            render_scale: 1.0,
            device,
            queue,
            config,
            samples,
            targets,
            globals,
            globals_bind,
            globals_layout,
            room: None,
            grid,
            scene_pipeline,
            rt_bind,
            ground: Default::default(),
            rt_layout,
            sky,
            sky_profile: None,
            texture_layout,
            sampler,
            lightmap_sampler,
            scene: None,
            uploading: None,
            water_pipeline,
            water_layout,
            water_screen_layout,
            water_screen,
            water_detail,
            clamp_sampler,
            water_settings: water::WaterSettings::default(),
            water_sky: None,
            repeat_sampler,
            water: None,
            sprite_pipeline,
            silhouette_pipeline,
            sprite_layout,
            sheets: vec![],
            ui_renderer,
            ui_free: Vec::new(),
            ui: None,
            actors: vec![],
            underwater: 0.0,
            upscale_runtime: false,
            warps: vec![],
            warp_texture: None,
            fx_pipeline,
            fx_assets: vec![],
            effect_textures: vec![],
            model_textures: vec![],
            model_lightmap,
            models: vec![],
            post,
            shadow,
            torches,
            post_settings: PostSettings::default(),
            point_lights: vec![],
            night: false,
            night_t: 0.0,
            last_frame: std::time::Instant::now(),
            cast_dir: glam::Vec3::new(-0.5, -0.7, -0.5).normalize(),
            day_clock: 0.0,
            day_start: 0.3,
            was_night: false,
            procs: vec![],
            auras: vec![],
            particles,
            weather_pass,
            weather: vec![],
            cloud_shadows: 0.0,
            wetness: 0.0,
            lightning: 0.0,
            foliage_sway: 0.0,
            icy_water: 0.0,
            heat_haze: true,
            night_lights: true,
            effects: vec![],
            show_effects: true,
            effect_pipeline,
            shade_pipeline,
            started: std::time::Instant::now(),
            frozen_at: None,
            frozen_for: std::time::Duration::ZERO,
            lighting: Lighting::default(),
            camera: Camera::default(),
            scale_factor: window.scale_factor(),
        })
    }

    /// Replaces the world. `images[i]` is `mesh.textures[i]` (None if missing).
    pub fn set_scene(&mut self, mesh: &SceneMesh, images: Vec<Option<Rgba>>, lighting: Lighting) {
        let prepared = prepare_scene(mesh, images, self.rt_layers());
        self.set_prepared_scene(mesh, prepared, lighting);
    }

    /// What `prepare_scene` needs to know of this device: its texture array layer limit when
    /// it ray traces, None when it doesn't.
    pub fn rt_layers(&self) -> Option<usize> {
        self.rt_layout.as_ref().map(|_| self.device.limits().max_texture_array_layers as usize)
    }

    /// `set_scene` with its CPU work done beforehand (`prepare_scene`, e.g. on a loader thread):
    /// what is left is the upload.
    pub fn set_prepared_scene(&mut self, mesh: &SceneMesh, prepared: PreparedScene, lighting: Lighting) {
        self.ground = mesh.ground.clone();
        if let Some(layout) = &self.rt_layout {
            // Prepared for a device without ray tracing: split from the full-size textures.
            let geometry = prepared.rt.unwrap_or_else(|| {
                let refs: Vec<Option<&Rgba>> = prepared.textures.iter().map(|levels| levels.first()).collect();
                rt::split(&mesh.vertices, &mesh.ranges, &refs, self.device.limits().max_texture_array_layers as usize)
            });
            self.rt_bind = Some(rt::upload(&self.device, &self.queue, layout, geometry));
        }
        self.scene = Some(scene::Scene::upload(
            &self.device,
            &self.queue,
            mesh,
            prepared.textures,
            prepared.windows,
        ));
        self.uploading = Some((std::time::Instant::now(), 0));
        self.lighting = lighting;
        self.water = None;
        self.sky_profile = None;
    }

    /// How much of the map's textures is on the GPU (0..1) while they go up over the frames
    /// after `set_prepared_scene`, or None once all are (keep the loading screen up till then).
    pub fn upload_progress(&self) -> Option<f32> {
        self.scene.as_ref().and_then(scene::Scene::progress)
    }

    /// Uploads the rest of the map's textures now (captures, which draw one frame).
    pub fn finish_uploads(&mut self) {
        self.pump_uploads(None);
    }

    /// Uploads waiting map textures for up to `budget` (None: all of them).
    fn pump_uploads(&mut self, budget: Option<std::time::Duration>) {
        let Some(s) = self.scene.as_mut() else { return };
        s.pump(&self.device, &self.queue, &self.texture_layout, &self.sampler, &self.lightmap_sampler, budget);
        if let Some((since, frames)) = self.uploading.as_mut() {
            *frames += 1;
            if s.progress().is_none() {
                tracing::info!(ms = since.elapsed().as_millis() as u64, frames = *frames, "map textures uploaded");
                self.uploading = None;
            }
        }
    }

    /// Sets the outdoor backdrop for this map. Call after `set_scene` and before
    /// `set_water` so the background, haze and generated reflections share a palette.
    /// None keeps the dark backdrop used by interiors and subterranean maps.
    pub fn set_sky_profile(&mut self, profile: Option<SkyProfile>) {
        self.sky_profile = profile;
    }

    /// Uploads a sprite sheet. `palette` overrides the SPR's own (hair / body dyes).
    /// Texture for warp portals (effect\ring_blue.tga).
    pub fn set_warp_texture(&mut self, img: &Rgba) {
        let view = scene::upload_texture(&self.device, &self.queue, img, true);
        self.warp_texture = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("warp texture"),
            layout: &self.sprite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.repeat_sampler) },
            ],
        }));
    }

    /// Direction sunlight travels for sprite shadows: the map's sun bearing, lowered to
    /// `sun_elevation` so shadows stretch out like HD-2D dioramas.
    fn shadow_dir(&self) -> glam::Vec3 {
        let d = glam::Vec3::from(self.lighting.direction);
        let mut elev = self.post_settings.sun_elevation;
        if elev <= 0.0 {
            return d;
        }
        let mut h = glam::Vec2::new(d.x, d.z).try_normalize().unwrap_or(glam::Vec2::new(0.6, 0.8));
        // Moving sun: the map's bearing is noon; morning and evening swing it 70° either way
        // and lower it, so shadows lengthen toward sunrise and sunset.
        if let Some(p) = self.day_progress() {
            let turn = glam::Vec2::from_angle(((p - 0.5) * 140.0).to_radians());
            h = turn.rotate(h);
            elev = (elev * (0.35 + 0.65 * (p * std::f32::consts::PI).sin())).max(10.0);
        }
        let (s, c) = elev.to_radians().sin_cos();
        glam::Vec3::new(h.x * c, -s, h.y * c)
    }

    /// 0 at sunrise .. 1 at sunset, when the sun moves (it repeats if day lasts longer).
    fn day_progress(&self) -> Option<f32> {
        let secs = self.post_settings.sun_cycle_minutes * 60.0;
        (secs > 0.0).then(|| (self.day_clock / secs + self.day_start).fract())
    }

    /// Where sprite shadows point: the sun's direction, turned away from the map lights near
    /// the player (torches, lamps), each weighted by how close and bright it is, so the
    /// shadow swings smoothly from one light to the next. Lights on opposite sides cancel
    /// and the shadow shortens under them.
    fn light_cast_dir(&self) -> glam::Vec3 {
        let sun = self.shadow_dir();
        let peak = self.point_lights.iter().map(|l| l.color[0].max(l.color[1]).max(l.color[2])).fold(0.0f32, f32::max).max(1e-3);
        let target = self.camera.target;
        let (mut pull, mut total) = (glam::Vec2::ZERO, 0.0f32);
        for l in &self.point_lights {
            let away = glam::Vec2::new(target.x - l.pos.x, target.z - l.pos.z);
            let d = away.length();
            // RSW colours are dim (~0.1-0.5); judge each against the brightest on the map.
            let bright = l.color[0].max(l.color[1]).max(l.color[2]) / peak;
            let w = (1.0 - d / l.range.max(1.0)).clamp(0.0, 1.0).powi(2) * bright * self.post_settings.point_lights.min(1.0);
            if w > 0.0 && d > 1e-3 {
                pull += away / d * w;
                total += w;
            }
        }
        if total <= 0.0 {
            return sun;
        }
        // Away from the lights along the ground, dipping down as from a lamp overhead; the
        // more the lights agree on a side, the longer the shadow.
        let reach = (pull.length() / total).clamp(0.0, 1.0);
        let away = pull.try_normalize().unwrap_or(glam::Vec2::ZERO) * 0.8 * reach;
        let lamp = glam::Vec3::new(away.x, -0.6 - 0.4 * (1.0 - reach), away.y).normalize();
        // Lamps bend the sun's shadow only partway (about 20° at most), so walking past
        // them sways it gently instead of spinning it.
        sun.lerp(lamp, (total * 1.5).min(0.3)).normalize()
    }

    /// Indoor maps: draw only cells set in `mask` (`w` x `h`, one byte per GAT cell, row 0
    /// = cell y 0). None draws everything.
    pub fn set_room_mask(&mut self, mask: Option<(u32, u32, &[u8])>) {
        let view = match mask {
            Some((w, h, cells)) => room_mask_view(&self.device, &self.queue, w, h, cells),
            None => room_mask_view(&self.device, &self.queue, 1, 1, &[255]),
        };
        self.room = mask.map(|(w, h, _)| (w, h));
        self.globals_bind = globals_bind_group(&self.device, &self.globals_layout, &self.globals, &self.shadow, &self.torches, &view);
    }

    /// Uploads a texture for code-drawn effects (light columns, ground circles).
    /// Replaces the map's particle emitters (textures from `add_effect_texture`).
    pub fn set_emitters(&mut self, emitters: Vec<Emitter>, winds: Vec<Wind>, waterfalls: Vec<Waterfall>) {
        let now = self.secs();
        self.particles.set(&self.device, emitters, winds, waterfalls, now);
    }

    pub fn set_glows(&mut self, glows: Vec<Glow>) {
        self.particles.set_glows(&self.device, glows);
    }

    /// Forgets the map: none of it is drawn until the next `set_scene` (a map whose files
    /// couldn't be loaded, so the last one doesn't stay on screen under the new map's units).
    pub fn clear_map(&mut self) {
        self.weather.clear();
        self.wetness = 0.0;
        self.scene = None;
        self.rt_bind = None;
        self.ground = Default::default();
        self.water = None;
        self.sky_profile = None;
        self.point_lights.clear();
        self.warps.clear();
        self.set_emitters(vec![], vec![], vec![]);
        self.set_glows(vec![]);
        self.set_room_mask(None);
    }

    /// Draws code-built effects in order (additive, or alpha-blended where they shade),
    /// batched into one buffer.
    fn draw_procs(&self, pass: &mut wgpu::RenderPass<'_>, procs: &[ProcFx], label: &str) {
        let mut vertices = vec![];
        let mut draws = vec![];
        for p in procs {
            if p.tex.0 >= self.effect_textures.len() {
                continue;
            }
            let start = vertices.len() as u32;
            p.vertices(self.camera.eye(), self.scene.as_ref().map(|_| &self.ground), &mut vertices);
            draws.push((p.tex.0, p.shades(), start..vertices.len() as u32));
        }
        if vertices.is_empty() {
            return;
        }
        let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        pass.set_vertex_buffer(0, buffer.slice(..));
        let mut current = None;
        for (tex, shades, range) in draws {
            if current != Some(shades) {
                pass.set_pipeline(if shades { &self.shade_pipeline } else { &self.effect_pipeline });
                current = Some(shades);
            }
            pass.set_bind_group(1, &self.effect_textures[tex], &[]);
            pass.draw(range, 0..1);
        }
    }

    pub fn add_effect_texture(&mut self, img: &Rgba) -> TexId {
        let view = scene::upload_texture(&self.device, &self.queue, img, true);
        self.effect_textures.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("effect texture"),
            layout: &self.sprite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.repeat_sampler) },
            ],
        }));
        TexId(self.effect_textures.len() - 1)
    }

    /// Registers a 3D model texture (see `models`).
    pub fn add_model_texture(&mut self, img: &Rgba) -> ModelTexId {
        let view = scene::upload_texture(&self.device, &self.queue, img, true);
        self.model_textures.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("model texture"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.repeat_sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.model_lightmap) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.lightmap_sampler) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.model_lightmap) },
            ],
        }));
        ModelTexId(self.model_textures.len() - 1)
    }

    /// Whether a sheet draws anything in action group `group` (facing south).
    pub fn action_visible(&self, sheet: SheetId, group: usize) -> bool {
        self.sheets.get(sheet.0).is_some_and(|s| s.has_layers(group * 8))
    }

    /// How long one pass of an actor action takes (sprite effects play it once).
    pub fn action_ms(&self, sheet: SheetId, action: usize) -> f32 {
        self.sheets.get(sheet.0).map_or(0.0, |s| s.action_ms(action))
    }

    pub fn add_sheet(&mut self, spr: &ro_formats::spr::Spr, act: ro_formats::act::Act, palette: Option<&[[u8; 4]; 256]>) -> SheetId {
        let sheet = sprite::Sheet::upload(&self.device, &self.queue, &self.sprite_layout, &self.sampler, spr, act, palette);
        self.sheets.push(sheet);
        SheetId(self.sheets.len() - 1)
    }

    /// Uploads an STR effect; `images[layer][texture]` as named in the STR.
    pub fn add_fx(&mut self, str: ro_formats::strm::Str, images: Vec<Vec<Option<Rgba>>>) -> FxId {
        let asset = fx::FxAsset::upload(&self.device, &self.queue, &self.sprite_layout, &self.sampler, str, images);
        self.fx_assets.push(asset);
        FxId(self.fx_assets.len() - 1)
    }

    /// How long an effect plays, in ms.
    pub fn fx_duration(&self, id: FxId) -> f32 {
        self.fx_assets.get(id.0).map_or(0.0, |a| a.duration_ms())
    }

    /// The sound (data\wav name) an actor's body frame asks for, if it entered a new frame
    /// since `from_ms` (negative: the action just started).
    pub fn actor_sound(&self, a: &Actor, from_ms: f32) -> Option<String> {
        let index = a.action * 8 + a.dir as usize;
        self.sheets.get(a.body.0)?.sound_between(index, from_ms, a.time_ms, a.once).map(str::to_string)
    }

    /// Adds the map's water. `images[i]` is `mesh.textures[i]`.
    /// Call after `set_scene` (the generated sky follows the map's lighting).
    pub fn set_water(&mut self, mesh: &WaterMesh, images: Vec<Option<Rgba>>) {
        let generated;
        let sky = match &self.water_sky {
            Some((img, mean)) => (img, *mean),
            None => {
                generated = water::sky_map(&self.lighting, self.sky_profile.as_ref());
                (&generated.0, generated.1)
            }
        };
        self.water = water::Water::upload(
            &self.device,
            &self.queue,
            &self.water_layout,
            &self.repeat_sampler,
            &self.clamp_sampler,
            &self.water_detail,
            mesh,
            images,
            sky,
            &self.water_settings,
        );
    }

    /// Water look and feature toggles; applies from the next frame.
    pub fn set_water_settings(&mut self, s: WaterSettings) {
        self.water_settings = s;
    }

    pub fn water_settings(&self) -> WaterSettings {
        self.water_settings
    }

    /// Supplies an equirectangular sky (sRGB, top half above the horizon) for water
    /// reflections from the next `set_water`; None returns to the generated sky.
    pub fn set_sky_map(&mut self, sky: Option<Rgba>) {
        self.water_sky = sky.map(|img| {
            let mean = water::mean_radiance(&img);
            (img, mean)
        });
    }

    /// Targets for a `width`x`height` scene shown in an `out`-sized window.
    fn make_targets(device: &wgpu::Device, (width, height): (u32, u32), out: (u32, u32), samples: u32, output: wgpu::TextureFormat) -> Targets {
        let size = wgpu::Extent3d { width: width.max(1), height: height.max(1), depth_or_array_layers: 1 };
        let tex = |label, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        Targets {
            depth: tex("depth", DEPTH_FORMAT),
            msaa: (samples > 1).then(|| tex("msaa", post::HDR_FORMAT)),
            msaa_info: (samples > 1).then(|| tex("msaa info", post::INFO_FORMAT)),
            post: post::PostTargets::new(device, width, height, out, output),
        }
    }

    /// A new surface for the window, on the same device: Android destroys the old one while the
    /// app is in the background. Everything loaded stays.
    pub fn recreate_surface(&mut self, window: Arc<Window>) -> anyhow::Result<()> {
        let size = window.inner_size();
        self.surface = self.instance.create_surface(window)?;
        self.config.width = size.width.max(1);
        self.config.height = size.height.max(1);
        self.surface.configure(&self.device, &self.config);
        if self.relay.is_some() {
            self.relay = Some(Self::make_relay(&self.device, &self.config));
        }
        self.targets = Self::make_targets(&self.device, self.scene_size(), self.size(), self.samples, self.config.format);
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return; // minimized
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        if self.relay.is_some() {
            self.relay = Some(Self::make_relay(&self.device, &self.config));
        }
        self.targets = Self::make_targets(&self.device, self.scene_size(), self.size(), self.samples, self.config.format);
    }

    /// The GPU and graphics API in use.
    pub fn adapter(&self) -> &str {
        &self.adapter_name
    }

    /// Whether ray-traced shadows and reflections can run here (rt::device_needs: a GPU with
    /// ray queries, and never on Android).
    pub fn ray_tracing(&self) -> bool {
        self.rt_layout.is_some()
    }

    /// GPU memory this device has allocated and reserved from the driver (bytes), on Vulkan
    /// and DX12. Not counted: the swapchain and the driver's own memory.
    pub fn vram(&self) -> Option<(u64, u64)> {
        self.device.generate_allocator_report().map(|r| (r.total_allocated_bytes, r.total_reserved_bytes))
    }

    /// Sets the world's resolution as a share of the window's (see `render_scale`).
    pub fn set_render_scale(&mut self, scale: f32) {
        let scale = (scale.clamp(0.5, 2.0) * 20.0).round() / 20.0;
        if scale != self.render_scale {
            self.render_scale = scale;
            self.targets = Self::make_targets(&self.device, self.scene_size(), self.size(), self.samples, self.config.format);
        }
    }

    /// The size the world renders at.
    pub fn scene_size(&self) -> (u32, u32) {
        self.scene_size_for(self.config.width, self.config.height)
    }

    /// The world's size for a `width`x`height` output. Supersampling stops at about a 4K
    /// frame's pixels (with MSAA a pixel costs ~100 bytes of targets) and the GPU's largest
    /// texture.
    fn scene_size_for(&self, width: u32, height: u32) -> (u32, u32) {
        const MAX_PIXELS: f32 = 3840.0 * 2160.0;
        let (w, h) = (width.max(1) as f32, height.max(1) as f32);
        let largest = self.device.limits().max_texture_dimension_2d as f32;
        let cap = (MAX_PIXELS / (w * h)).sqrt().min(largest / w).min(largest / h).max(1.0);
        let scale = self.render_scale.min(cap);
        (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1))
    }

    pub fn size(&self) -> (u32, u32) {
        (self.config.width, self.config.height)
    }

    /// The globals' wind for swaying foliage: a breeze (`foliage_sway`), stronger in the
    /// weather (rain and storms most), from the same quarter the clouds come from.
    fn wind_globals(&self) -> [f32; 4] {
        if self.foliage_sway <= 0.0 {
            return [0.0; 4];
        }
        let mut gale = 1.0;
        if self.show_effects {
            for w in &self.weather {
                let s = w.strength.clamp(0.0, 1.0);
                gale += s * match w.kind {
                    WeatherKind::Rain => 1.2,
                    WeatherKind::Clouds => 0.4,
                    WeatherKind::Snow => 0.3,
                    WeatherKind::Leaves | WeatherKind::Petals => 0.3,
                    WeatherKind::Fog => -0.3,
                };
            }
        }
        let dir = glam::Vec2::new(1.0, 0.35).normalize();
        [dir.x, dir.y, self.foliage_sway * gale, 0.0]
    }

    /// Seconds of the world's clock (shaders' time): it stands still in photo mode.
    fn secs(&self) -> f32 {
        let now = self.frozen_at.unwrap_or_else(std::time::Instant::now);
        (now.duration_since(self.started).saturating_sub(self.frozen_for)).as_secs_f32()
    }

    /// Photo mode: the world's time stands still (water, weather, swaying trees, the sun).
    pub fn set_frozen(&mut self, on: bool) {
        match (on, self.frozen_at) {
            (true, None) => self.frozen_at = Some(std::time::Instant::now()),
            (false, Some(at)) => {
                self.frozen_for += at.elapsed();
                self.frozen_at = None;
            }
            _ => {}
        }
    }

    /// How far into night it is (0 day .. 1 night), as the light eases between them.
    pub fn night_amount(&self) -> f32 {
        self.night_t
    }

    /// Heat haze now (0..1): over deserts and volcanoes, outdoors, by day; rain and snow cool
    /// it off.
    fn heat(&self) -> f32 {
        let Some(profile) = self.sky_profile.as_ref() else { return 0.0 };
        if !self.heat_haze || self.room.is_some() || self.underwater > 0.0 {
            return 0.0;
        }
        let base = match profile.name {
            "desert" => 1.0,
            "volcanic" => 1.2,
            _ => return 0.0,
        };
        let cooled = self.weather.iter().filter(|w| matches!(w.kind, WeatherKind::Rain | WeatherKind::Snow)).map(|w| w.strength).fold(0.0, f32::max);
        base * (1.0 - self.night_t * 0.85) * (1.0 - cooled.clamp(0.0, 1.0))
    }

    /// The map's sky profile under the weather now. Rain greys the sky over (dark, heavy,
    /// solid cloud, more haze, a stronger wind); snow greys it paler; drifting cloud and fog
    /// a little; a lightning flash lights the clouds up from within. Overcast hides the
    /// stars. Open space (the galaxy) and weather turned off keep the profile as it is.
    fn weathered_sky(&self) -> Option<SkyProfile> {
        let mut p = *self.sky_profile.as_ref()?;
        if p.galaxy || !self.show_effects {
            return Some(p);
        }
        let o = weather::overcast(&self.weather);
        let flash = self.lightning.clamp(0.0, 1.0);
        if o <= 0.0 && flash <= 0.0 {
            return Some(p);
        }
        let of = |kind: WeatherKind| self.weather.iter().filter(|w| w.kind == kind).map(|w| w.strength.clamp(0.0, 1.0)).fold(0.0, f32::max);
        let (rain, snow) = (of(WeatherKind::Rain), of(WeatherKind::Snow));
        // How dark the grey: rain clouds are dark, snow clouds pale, plain cloud between.
        let tone = if rain + snow > 0.0 { 0.92 - 0.56 * rain / (rain + snow) } else { 0.8 };
        let k = (o * 1.15).min(0.92);
        let grey = |c: [f32; 3], t: f32| {
            let c = glam::Vec3::from(c);
            c.lerp(glam::Vec3::splat(c.dot(glam::Vec3::new(0.3, 0.59, 0.11)) * t), k)
        };
        let mut zenith = grey(p.zenith, tone * 0.9);
        let mut horizon = grey(p.horizon, tone);
        let mut cloud = grey(p.cloud, tone * 0.85);
        // Lightning: the cloud deck lights up cold white from within.
        let cold = glam::Vec3::new(0.8, 0.86, 1.0);
        zenith += cold * flash * 0.5;
        horizon += cold * flash * 0.35;
        cloud += cold * flash * 1.4;
        p.zenith = zenith.to_array();
        p.horizon = horizon.to_array();
        p.cloud = cloud.to_array();
        p.cloud_cover += (1.0 - p.cloud_cover) * (o * 1.25).min(1.0);
        p.haze += (1.0 - p.haze) * o * 0.8;
        p.wind_speed *= 1.0 + o;
        p.stars *= 1.0 - (o * 1.3).min(1.0);
        Some(p)
    }

    /// How much of the water is frozen: snowy maps outdoors only.
    fn ice(&self) -> f32 {
        match self.sky_profile.as_ref() {
            Some(p) if p.name == "snow" && self.room.is_none() && self.underwater <= 0.0 => self.icy_water.clamp(0.0, 1.0),
            _ => 0.0,
        }
    }

    /// How hard it rains on this map now (0..1): outdoors, with effects shown.
    fn raining(&self) -> f32 {
        if !self.show_effects || self.room.is_some() || self.underwater > 0.0 || self.sky_profile.is_none() {
            return 0.0;
        }
        self.weather.iter().filter(|w| w.kind == WeatherKind::Rain).map(|w| w.strength.clamp(0.0, 1.0)).fold(0.0, f32::max)
    }

    /// The globals' cloud shadows: outdoors only (not indoors, underwater, or with the HD-2D
    /// look off), more and deeper under cloudy weather.
    fn cloud_globals(&self) -> [f32; 4] {
        let on = self.cloud_shadows > 0.0 && self.post_settings.enabled && self.room.is_none() && self.underwater <= 0.0 && self.sky_profile.is_some();
        if !on {
            return [0.0; 4];
        }
        let (mut cover, mut depth) = (0.3, 1.0);
        if self.show_effects {
            for w in &self.weather {
                let s = w.strength.clamp(0.0, 1.0);
                let (c, d) = match w.kind {
                    WeatherKind::Clouds => (0.35, 0.2),
                    WeatherKind::Fog => (0.2, 0.0),
                    WeatherKind::Rain => (0.45, 0.1),
                    WeatherKind::Snow => (0.25, 0.0),
                    WeatherKind::Petals | WeatherKind::Leaves => (0.0, 0.0),
                };
                cover += c * s;
                depth += d * s;
            }
        }
        // The clouds' wind (weather.rs): a cell a second east, a third of that north.
        [(self.cloud_shadows * 0.7 * depth).min(0.95), cover.min(0.9), 1.0 * CELL, 0.35 * CELL]
    }

    /// World-space ray (origin, unit direction) through a window pixel.
    pub fn ray(&self, px: f32, py: f32) -> (glam::Vec3, glam::Vec3) {
        let (w, h) = (self.config.width as f32, self.config.height as f32);
        let ndc = glam::Vec2::new(2.0 * px / w - 1.0, 1.0 - 2.0 * py / h);
        let inv = self.camera.view_proj(self.aspect()).inverse();
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    pub fn aspect(&self) -> f32 {
        self.config.width as f32 / self.config.height as f32
    }

    /// The texture frames are drawn into when the window surface can't take a second view.
    fn make_relay(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("frame relay"),
            size: wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[config.format.remove_srgb_suffix()],
        })
    }

    pub fn frame(&mut self) -> anyhow::Result<()> {
        // A new map's textures, a few milliseconds' worth a frame.
        self.pump_uploads(Some(UPLOAD_BUDGET));
        let tex = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            other => {
                tracing::warn!(?other, "skipping frame");
                return Ok(());
            }
        };

        // egui blends in gamma space: give it a non-sRGB view of the same frame.
        let target = self.relay.as_ref().unwrap_or(&tex.texture);
        let view = target.create_view(&Default::default());
        let ui_view = target.create_view(&wgpu::TextureViewDescriptor { format: Some(self.config.format.remove_srgb_suffix()), ..Default::default() });
        let cmd = self.encode(&view, &ui_view, None, self.size(), true);
        match &self.relay {
            Some(relay) => {
                let mut enc = self.device.create_command_encoder(&Default::default());
                enc.copy_texture_to_texture(relay.as_image_copy(), tex.texture.as_image_copy(), relay.size());
                self.queue.submit([cmd, enc.finish()]);
            }
            None => {
                self.queue.submit([cmd]);
            }
        }
        self.queue.present(tex);
        Ok(())
    }

    /// Renders one frame offscreen at `width`x`height` and returns it as RGBA8 (sRGB).
    pub fn capture(&mut self, width: u32, height: u32) -> anyhow::Result<Rgba> {
        self.capture_with(width, height, true)
    }

    /// As `capture`, with or without the UI drawn over the world.
    pub fn capture_with(&mut self, width: u32, height: u32, ui: bool) -> anyhow::Result<Rgba> {
        // One frame: the whole map, not the part of its textures up so far.
        self.finish_uploads();
        let mut config = self.config.clone();
        config.width = width;
        config.height = height;
        // The world at the render scale, as on screen.
        let targets = Self::make_targets(&self.device, self.scene_size_for(width, height), (width, height), self.samples, config.format);
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[config.format.remove_srgb_suffix()],
        });
        let view = texture.create_view(&Default::default());
        let ui_view = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(config.format.remove_srgb_suffix()), ..Default::default() });
        let cmd = self.encode(&view, &ui_view, Some(&targets), (width, height), ui);


        // Rows must be padded to 256 bytes for buffer copies.
        let row = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (row * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(height) },
            },
            wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        );
        self.queue.submit([cmd, enc.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        rx.recv()??;
        let data = slice.get_mapped_range()?;
        let bgra = matches!(config.format, wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm);
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height as usize {
            for px in data[y * row as usize..][..width as usize * 4].chunks_exact(4) {
                pixels.extend(if bgra { [px[2], px[1], px[0], 255] } else { [px[0], px[1], px[2], 255] });
            }
        }
        Ok(Rgba { width, height, pixels })
    }

    /// Records the whole frame into `view` using `targets` (depth / MSAA of the same size).
    /// `draw_ui` false leaves the UI out (its texture changes are still applied).
    fn encode(&mut self, view: &wgpu::TextureView, ui_view: &wgpu::TextureView, targets: Option<&Targets>, size: (u32, u32), draw_ui: bool) -> wgpu::CommandBuffer {
        // Night: blend the map's light and the look toward moonlight for this frame only.
        let now = std::time::Instant::now();
        // Uncapped: after a long gap (a capture, a stall) the fade simply completes.
        // Nothing moves while time stands still (photo mode).
        let dt = if self.frozen_at.is_some() { 0.0 } else { (now - self.last_frame).as_secs_f32() };
        self.last_frame = now;
        let goal = if self.night { 1.0 } else { 0.0 };
        self.night_t += (goal - self.night_t).clamp(-dt * 0.5, dt * 0.5);
        // The ground soaks in the rain, and dries after.
        let soak = self.raining();
        self.wetness += (soak - self.wetness).clamp(-dt / 120.0, dt / 30.0);
        // The sun rises when day returns and climbs while it lasts.
        if self.was_night && !self.night {
            self.day_clock = 0.0;
            self.day_start = 0.0;
        }
        self.was_night = self.night;
        if !self.night {
            self.day_clock += dt.min(1.0);
        }
        let (day_light, day_post) = (self.lighting, self.post_settings);
        if self.night_t > 0.0 {
            let n = self.night_t;
            let mix3 = |a: [f32; 3], b: [f32; 3]| [a[0] + (b[0] - a[0]) * n, a[1] + (b[1] - a[1]) * n, a[2] + (b[2] - a[2]) * n];
            let a = self.lighting.ambient;
            let d = self.lighting.diffuse;
            self.lighting.ambient = mix3(a, [a[0] * 0.62, a[1] * 0.68, a[2] * 0.9]);
            self.lighting.diffuse = mix3(d, [d[0] * 0.42, d[1] * 0.48, d[2] * 0.68]);
            let p = &mut self.post_settings;
            p.shadow_strength *= 1.0 - 0.6 * n;
            p.point_lights *= 1.0 + 1.5 * n;
            p.warmth += (-0.5 - p.warmth) * n;
            p.saturation *= 1.0 - 0.25 * n;
            p.god_rays *= 1.0 - n;
            p.bloom_threshold *= 1.0 - 0.35 * n;
            p.vignette += 0.2 * n;
            p.exposure *= 1.0 - 0.15 * n;
            if !p.enabled {
                // Classic look: no effects, but night still darkens and cools the screen.
                *p = PostSettings {
                    enabled: true,
                    bloom_intensity: 0.0,
                    dof_strength: 0.0,
                    vignette: 0.2 * n,
                    contrast: 1.0,
                    saturation: 1.0 - 0.25 * n,
                    warmth: -0.5 * n,
                    exposure: 1.0 - 0.25 * n,
                    shadow_strength: 0.0,
                    sprite_light: 0.0,
                    rim_light: 0.0,
                    fog: 0.0,
                    god_rays: 0.0,
                    ambient_occlusion: 0.0,
                    pixel_snap: false,
                    ..PostSettings::default()
                };
            }
        }
        // Lightning: a cold white light over everything for a moment, the sky and all.
        if self.lightning > 0.0 && self.show_effects && self.sky_profile.is_some() && self.room.is_none() && self.underwater <= 0.0 {
            let l = self.lightning.min(1.5);
            let flash = [0.45, 0.55, 0.95];
            for (i, f) in flash.iter().enumerate() {
                self.lighting.ambient[i] += f * l;
                self.lighting.diffuse[i] += f * l * 0.4;
            }
            let p = &mut self.post_settings;
            p.exposure *= 1.0 + 0.3 * l;
            p.saturation *= 1.0 - 0.35 * l.min(1.0);
            p.warmth -= 0.6 * l.min(1.0);
        }
        // Ease the shadow direction toward this frame's light (about a second).
        let mut goal = self.light_cast_dir();
        // Keep shadows on the viewer's side: one falling away from the viewer starts behind
        // the body, so its visible part looks detached from the feet. Mirror it toward the
        // default (north-up) view; using the live camera would turn shadows with it.
        let fwd = glam::Vec2::new(0.0, 1.0);
        let h = glam::Vec2::new(goal.x, goal.z);
        if h.dot(fwd) > 0.0 {
            let m = h - fwd * 2.0 * h.dot(fwd);
            goal = glam::Vec3::new(m.x, goal.y, m.y);
        }
        self.cast_dir = self.cast_dir.lerp(goal, (dt * 1.5).min(1.0)).try_normalize().unwrap_or(goal);
        // Shadows fade out as the sun sets and back in as it rises (and wraps).
        if let Some(p) = self.day_progress() {
            let edge = |x: f32| (x / 0.06).clamp(0.0, 1.0);
            self.post_settings.shadow_strength *= edge(p) * edge(1.0 - p);
        }
        let targets = targets.unwrap_or(&self.targets);
        let aspect = size.0 as f32 / size.1 as f32;
        // The world's own size (smaller than the output with a render scale below 1).
        let scene = targets.post.size();
        let cam = &self.camera;
        // The sun's view of the area around the player, for sprite shadows.
        let light_vp = shadow::Shadow::light_vp(cam.target, self.cast_dir, (cam.distance * 0.75).clamp(120.0, 600.0));
        // Weather clouds the sun over: softer shadows, dimmer direct light.
        let overcast = if self.show_effects { weather::overcast(&self.weather) } else { 0.0 };
        let shadow_strength = if self.post_settings.enabled { self.post_settings.shadow_strength * (1.0 - 0.7 * overcast) } else { 0.0 };
        // The strongest torches around the player, each with a shadow strength that fades in
        // as the player nears it. Not faded at night (`day_post`): that's when torches matter.
        let torches: Vec<(&PointLight, f32)> = {
            let strength = if day_post.enabled { day_post.shadow_strength * day_post.point_lights.min(1.0) } else { 0.0 };
            let mut near: Vec<_> = self
                .point_lights
                .iter()
                .map(|l| {
                    let reach = (1.0 - l.pos.distance(cam.target) / l.range.max(1.0)).clamp(0.0, 1.0);
                    // Nearest first: a dim torch beside the player shadows more than a bright
                    // one far off (closeness ranks, strength is the same for all).
                    (l, (reach * 3.0).min(1.0) * strength + reach * 1e-3)
                })
                .filter(|(_, k)| *k > 0.0)
                .collect();
            near.sort_by(|a, b| b.1.total_cmp(&a.1));
            near.truncate(TORCHES);
            near
        };
        // Cast from at most TORCH_HEIGHT above the player: RO hangs its lights high, which
        // would leave only a pool at the feet (the same reason the sun is lowered).
        let cast_from = |l: &PointLight| glam::Vec3::new(l.pos.x, l.pos.y.min(cam.target.y + TORCH_HEIGHT), l.pos.z);
        let torch_vps: Vec<glam::Mat4> = torches.iter().map(|(l, _)| shadow::Shadow::torch_vp(cast_from(l), cam.target, l.range)).collect();
        // The point lights nearest the camera target, flattened into (pos, range), (colour, flicker).
        let lights: Vec<[f32; 4]> = {
            let mut near: Vec<&PointLight> = self.point_lights.iter().collect();
            near.sort_by(|a, b| a.pos.distance_squared(cam.target).total_cmp(&b.pos.distance_squared(cam.target)));
            let t = self.secs();
            // The colour's w is the flicker's swing, for the light baked around the flame.
            near.iter().take(MAX_LIGHTS).flat_map(|l| {
                let f = l.flicker(t);
                [l.pos.extend(l.range).to_array(), (glam::Vec3::from(l.color) * f).extend(f - 1.0).to_array()]
            }).collect()
        };
        // The sky as the weather has it: grey and overcast in rain, pale in snow, lit up by
        // lightning. The sky itself, the haze far things fade into and what wet ground
        // mirrors all take it.
        let weathered = self.weathered_sky();
        let globals = Globals {
            view_proj: cam.view_proj(aspect).to_cols_array_2d(),
            focus: [cam.target.x, CELL, cam.target.z, cam.distance * 4.0],
            eye: cam.eye().extend(1.0).to_array(),
            light_dir: glam::Vec3::from(self.lighting.direction).normalize_or_zero().extend(0.0).to_array(),
            ambient: glam::Vec3::from(self.lighting.ambient).extend(0.0).to_array(),
            diffuse: (glam::Vec3::from(self.lighting.diffuse) * (1.0 - 0.45 * overcast)).extend(0.0).to_array(),
            params: {
                // Clip units per sprite pixel at w = 1 (horizontal, vertical).
                let f = 1.0 / (cam.fovy / 2.0).tan();
                let s = CELL / sprite::PIXELS_PER_CELL;
                let snap = self.post_settings.enabled && self.post_settings.pixel_snap;
                [self.secs(), s * f / aspect, s * f, if snap { 1.0 } else { 0.0 }]
            },
            light_vp: light_vp.to_cols_array_2d(),
            shadow: [shadow_strength, 1.0 / shadow::SIZE as f32, self.post_settings.shadow_softness, 0.0],
            screen: [scene.0 as f32, scene.1 as f32, 1.0 / scene.0 as f32, 1.0 / scene.1 as f32],
            sprite_light: {
                let p = &self.post_settings;
                let on = if p.enabled { 1.0 } else { 0.0 };
                [p.sprite_light * on, p.rim_light * on, p.point_lights * on, lights.len() as f32 / 2.0]
            },
            lights: {
                let mut out = [[0.0; 4]; MAX_LIGHTS * 2];
                out[..lights.len()].copy_from_slice(&lights);
                out
            },
            room: match self.room {
                Some((w, h)) => [1.0, w as f32, h as f32, CELL],
                None => [0.0; 4],
            },
            water: match &self.water {
                // Nothing wades into lava.
                Some(w) if self.water_settings.interaction && !w.lava() => [w.level(), 1.0, CELL / sprite::PIXELS_PER_CELL, 0.0],
                _ => [0.0; 4],
            },
            water_tint: {
                let c = self.water_settings.scatter_color;
                [c[0], c[1], c[2], 0.0]
            },
            torch_vp: std::array::from_fn(|i| torch_vps.get(i).map_or([[0.0; 4]; 4], |m| m.to_cols_array_2d())),
            torch: std::array::from_fn(|i| torches.get(i).map_or([0.0; 4], |(l, _)| l.pos.extend(l.range).to_array())),
            torch_k: [torches.first().map_or(0.0, |t| t.1), torches.get(1).map_or(0.0, |t| t.1), 1.0 / shadow::TORCH_SIZE as f32, 0.0],
            clouds: self.cloud_globals(),
            wind: self.wind_globals(),
            // Windows light up as night falls, outdoors (interiors keep their own light).
            night: [if self.night_lights && self.sky_profile.is_some() && self.room.is_none() { self.night_t * 1.6 } else { 0.0 }, 0.0, 0.0, 0.0],
            wet: [if self.post_settings.enabled { self.wetness } else { 0.0 }, self.raining(), self.ice(), 0.0],
            // What wet ground and puddles mirror: the sky as the weather has it (grey in rain).
            wet_sky: match weathered.as_ref() {
                Some(p) => {
                    let (h, z) = (glam::Vec3::from(sky::horizon_color(p, self.night_t)), glam::Vec3::from(sky::zenith_color(p, self.night_t)));
                    h.lerp(z, 0.4).extend(0.0).to_array()
                }
                None => [0.0; 4],
            },
            upscale: [if self.upscale_runtime { 1.0 } else { 0.0 }, if self.post_settings.enabled { self.post_settings.relief } else { 0.0 }, 0.0, 0.0],
            // Outdoors only: interiors and dungeons keep their hand-baked light and shadow.
            rt: {
                let p = &self.post_settings;
                let on = self.rt_bind.is_some() && p.enabled && self.room.is_none() && self.underwater <= 0.0 && self.sky_profile.is_some();
                let bits = (on && p.ray_traced_shadows) as u32 | ((on && p.ray_traced_reflections) as u32) << 1;
                self.shadow_dir().normalize_or_zero().extend(bits as f32).to_array()
            },
        };
        self.queue.write_buffer(&self.globals, 0, bytemuck::bytes_of(&globals));
        let outdoor_sky = self.scene.is_some() && self.room.is_none() && self.underwater <= 0.0 && self.sky_profile.is_some();
        if outdoor_sky {
            self.sky.update(&self.queue, weathered.as_ref().unwrap(), cam, aspect, self.secs(), self.night_t, self.shadow_dir().to_array(), self.raining());
        }

        // Sprites, back to front by distance from the eye so blended edges composite.
        let eye = cam.eye();
        let mut order: Vec<&Actor> = self.actors.iter().collect();
        order.sort_by(|a, b| b.pos.distance_squared(eye).total_cmp(&a.pos.distance_squared(eye)));
        let mut sprite_vertices = vec![];
        let mut sprite_draws = vec![];
        let blobs: std::collections::HashSet<usize> = self.actors.iter().filter_map(|a| a.shadow.map(|s| s.0)).collect();
        // Silhouettes: the same quads (not the blob shadows), coloured.
        let mut silhouette_vertices = vec![];
        let mut silhouette_draws = vec![];
        for a in order {
            for (sheet, v) in sprite::actor_quads(&self.sheets, a) {
                if let (Some(c), false) = (a.silhouette, blobs.contains(&sheet)) {
                    let start = silhouette_vertices.len() as u32;
                    silhouette_vertices.extend(v.iter().map(|q| sprite::Vertex { color: [c[0], c[1], c[2], q.color[3]], ..*q }));
                    silhouette_draws.push((sheet, start..silhouette_vertices.len() as u32));
                }
                let start = sprite_vertices.len() as u32;
                sprite_vertices.extend(v);
                sprite_draws.push((sheet, start..sprite_vertices.len() as u32));
            }
        }
        let silhouette_buffer = (!silhouette_vertices.is_empty()).then(|| sprite::vertex_buffer(&self.device, &silhouette_vertices));
        let sprite_buffer = (!sprite_vertices.is_empty()).then(|| sprite::vertex_buffer(&self.device, &sprite_vertices));
        let mut enc = self.device.create_command_encoder(&Default::default());
        // Sun shadows: every sprite layer except the round blob shadows under actors.
        if shadow_strength > 0.0 {
            // Casters face the light (not the camera) so they throw their full silhouette.
            let sun = self.cast_dir;
            let right = glam::Vec3::new(-sun.z, 0.0, sun.x).try_normalize().unwrap_or(glam::Vec3::X);
            let casters = sprite_draws.iter().filter(|(s, _)| !blobs.contains(s)).map(|(s, r)| (&self.sheets[*s].bind, r.clone()));
            self.shadow.record(&self.queue, &mut enc, light_vp, right, None, CELL / sprite::PIXELS_PER_CELL, sprite_buffer.as_ref(), casters);
        }
        // Torch shadows: the same casters, each card facing the torch.
        for (i, ((light, _), vp)) in torches.iter().zip(&torch_vps).enumerate() {
            let away = cam.target - light.pos;
            let right = glam::Vec3::new(-away.z, 0.0, away.x).try_normalize().unwrap_or(glam::Vec3::X);
            let casters = sprite_draws.iter().filter(|(s, _)| !blobs.contains(s)).map(|(s, r)| (&self.sheets[*s].bind, r.clone()));
            self.torches[i].record(&self.queue, &mut enc, *vp, right, Some(cast_from(light)), CELL / sprite::PIXELS_PER_CELL, sprite_buffer.as_ref(), casters);
        }

        // Water: this frame's settings, mesh density and screen-copy size.
        let secs = self.secs();
        if let Some(w) = self.water.as_mut() {
            w.retessellate(&self.device, self.water_settings.tessellation);
            let walkers = self.actors.iter().filter(|a| a.alpha > 0.1).map(|a| (a.pos, a.action == 1));
            w.wade(walkers, cam.target, cam.eye(), secs);
            let lum = |c: [f32; 3]| c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
            let sky_scale = (lum(self.lighting.diffuse) + lum(self.lighting.ambient)) / (lum(day_light.diffuse) + lum(day_light.ambient)).max(1e-3);
            let sky_scale = if outdoor_sky { sky_scale * (1.0 - 0.78 * self.night_t) } else { sky_scale };
            w.update(&self.queue, &self.water_settings, self.lighting.diffuse, sky_scale);
            let s = self.water_screen.color.size();
            if (s.width, s.height) != scene {
                self.water_screen = water::ScreenCopies::new(&self.device, &self.water_screen_layout, scene.0, scene.1);
            }
        }

        // The world goes to the HDR target; post-processing then writes `view`.
        let (color, resolve) = match &targets.msaa {
            Some(m) => (m, Some(&targets.post.hdr)),
            None => (&targets.post.hdr, None),
        };
        // The world pass. With water reading the scene it is split in two: the opaque world,
        // a copy of its colour and depth, then the water and everything drawn over it.
        let begin = |enc: &mut wgpu::CommandEncoder, first: bool| {
            let load = |c: wgpu::Color| if first { wgpu::LoadOp::Clear(c) } else { wgpu::LoadOp::Load };
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(if first { "main" } else { "main (over water)" }),
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: color,
                        depth_slice: None,
                        resolve_target: resolve,
                        ops: wgpu::Operations { load: load(CLEAR), store: wgpu::StoreOp::Store },
                    }),
                    // Depth info: x linear distance from the eye (far where nothing drew),
                    // y 1 on sprites.
                    Some(wgpu::RenderPassColorAttachment {
                        view: targets.msaa_info.as_ref().unwrap_or(&targets.post.info),
                        depth_slice: None,
                        resolve_target: targets.msaa_info.as_ref().map(|_| &targets.post.info),
                        ops: wgpu::Operations { load: load(wgpu::Color { r: 60000.0, g: 0.0, b: 0.0, a: 0.0 }), store: wgpu::StoreOp::Store },
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations { load: if first { wgpu::LoadOp::Clear(1.0) } else { wgpu::LoadOp::Load }, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                ..Default::default()
            })
            .forget_lifetime()
        };
        let water_reads_screen = self.water.is_some() && self.water_settings.uses_screen();
        {
            let mut pass = begin(&mut enc, true);
            if outdoor_sky {
                self.sky.draw(&mut pass);
            }
            pass.set_bind_group(0, &self.globals_bind, &[]);
            if let Some(rt) = &self.rt_bind {
                pass.set_bind_group(2, rt, &[]);
            }
            match &self.scene {
                Some(t) => {
                    pass.set_pipeline(&self.scene_pipeline);
                    pass.set_vertex_buffer(0, t.vertices.slice(..));
                    for (range, tex) in t.ranges.iter().zip(&t.textures) {
                        // (A texture still going up: its pieces wait; see Scene::pump.)
                        let Some(tex) = tex else { continue };
                        if !range.is_empty() {
                            pass.set_bind_group(1, tex, &[]);
                            pass.draw(range.clone(), 0..1);
                        }
                    }
                }
                None => {
                    pass.set_pipeline(&self.grid);
                    pass.draw(0..6, 0..1);
                }
            }
            // 3D monsters (opaque, lit like the map's models).
            if self.models.iter().any(|m| !m.vertices.is_empty()) {
                let mut verts = Vec::new();
                let mut draws = Vec::new();
                for m in &self.models {
                    let start = verts.len() as u32;
                    verts.extend_from_slice(&m.vertices);
                    draws.push((m.texture.0, start..verts.len() as u32));
                }
                let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("models"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.scene_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                for (tex, range) in draws {
                    if let Some(bind) = self.model_textures.get(tex) {
                        pass.set_bind_group(1, bind, &[]);
                        pass.draw(range, 0..1);
                    }
                }
            }
            // Transparent passes after all opaque geometry.
            if water_reads_screen {
                drop(pass);
                self.water_screen.capture(&mut enc, &targets.post.hdr_tex, &targets.post.info_tex);
                pass = begin(&mut enc, false);
                pass.set_bind_group(0, &self.globals_bind, &[]);
            }
            if let Some(w) = &self.water {
                pass.set_pipeline(&self.water_pipeline);
                pass.set_bind_group(1, &w.frames[w.frame(globals.params[0])], &[]);
                pass.set_bind_group(2, &self.water_screen.bind, &[]);
                if let Some(rt) = &self.rt_bind {
                    pass.set_bind_group(3, rt, &[]);
                }
                pass.set_vertex_buffer(0, w.vertices.slice(..));
                pass.draw(0..w.count, 0..1);
            }
            // Warp portals (additive), then actors.
            if let (false, Some(bind)) = (self.warps.is_empty(), self.warp_texture.as_ref()) {
                let verts = effect::warp_portals(&self.warps, globals.params[0]);
                let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("effects"),
                    contents: bytemuck::cast_slice(&verts),
                    usage: wgpu::BufferUsages::VERTEX,
                });
                pass.set_pipeline(&self.effect_pipeline);
                pass.set_bind_group(1, bind, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..verts.len() as u32, 0..1);
            }
            // Level auras, under their owners.
            self.draw_procs(&mut pass, &self.auras, "auras");
            // Actors back to front (built before the shadow pass, which shares them).
            if let Some(buffer) = &sprite_buffer {
                pass.set_pipeline(&self.sprite_pipeline);
                if let Some(rt) = &self.rt_bind {
                    pass.set_bind_group(2, rt, &[]);
                }
                pass.set_vertex_buffer(0, buffer.slice(..));
                for (sheet, range) in &sprite_draws {
                    pass.set_bind_group(1, &self.sheets[*sheet].bind, &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
            // Silhouettes of those hidden behind walls and buildings.
            if let Some(buffer) = &silhouette_buffer {
                pass.set_pipeline(&self.silhouette_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                for (sheet, range) in &silhouette_draws {
                    pass.set_bind_group(1, &self.sheets[*sheet].bind, &[]);
                    pass.draw(range.clone(), 0..1);
                }
            }
            // Code-drawn skill effects (additive, world space).
            if self.show_effects {
                self.draw_procs(&mut pass, &self.procs, "skill effects");
            }
            // Map particle emitters (smoke, sparks, dust...), each with its map-given blend
            // (RO_PARTICLES_OFF hides them, for comparing).
            if !self.particles.is_empty() && std::env::var_os("RO_PARTICLES_OFF").is_none() {
                let now = self.secs();
                let (eye, focus) = (self.camera.eye(), self.camera.target);
                let range = 60.0 * CELL;
                self.particles.update(now, focus, range);
                let groups = self.particles.vertices(now, eye, focus, range);
                for (key, tex, verts) in groups {
                    let (Some(pipe), Some(bind)) = (self.particles.pipeline_for(&key), self.effect_textures.get(tex.0)) else { continue };
                    let buffer = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("particles"),
                        contents: bytemuck::cast_slice(&verts),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                    pass.set_pipeline(pipe);
                    pass.set_bind_group(1, bind, &[]);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..verts.len() as u32, 0..1);
                }
            }
            // Weather (hidden with the other effects, as the original's /effect does).
            if self.show_effects && !self.weather.is_empty() {
                let shown = &self.weather[..self.weather.len().min(weather::MAX_LAYERS)];
                let mut data = [weather::Layer::default(); weather::MAX_LAYERS];
                let mut counts = [0u32; weather::MAX_LAYERS];
                let inv = self.camera.view_proj(aspect).inverse();
                for ((d, n), w) in data.iter_mut().zip(&mut counts).zip(shown) {
                    (*d, *n) = weather::layer(w, self.camera.target, inv);
                }
                self.queue.write_buffer(&self.weather_pass.uniform, 0, bytemuck::cast_slice(&data));
                pass.set_pipeline(&self.weather_pass.pipeline);
                pass.set_bind_group(2, &self.weather_pass.bind, &[]);
                for (i, w) in shown.iter().enumerate() {
                    let Some(bind) = self.effect_textures.get(w.tex.0) else { continue };
                    pass.set_bind_group(1, bind, &[]);
                    let first = i as u32 * weather::PER_LAYER;
                    pass.draw(0..6, first..first + counts[i]);
                }
            }
            // Skill effects over everything else in the world.
            if self.show_effects && !self.effects.is_empty() {
                let mut vertices = vec![];
                let mut draws = vec![];
                for e in &self.effects {
                    let Some(asset) = self.fx_assets.get(e.asset.0) else { continue };
                    let start = vertices.len() as u32;
                    asset.vertices(e, &mut vertices);
                    if vertices.len() as u32 > start {
                        draws.push((e.asset.0, start..vertices.len() as u32));
                    }
                }
                if !vertices.is_empty() {
                    let buffer = sprite::vertex_buffer(&self.device, &vertices);
                    pass.set_pipeline(&self.fx_pipeline);
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    for (asset, range) in draws {
                        pass.set_bind_group(1, &self.fx_assets[asset].bind, &[]);
                        pass.draw(range, 0..1);
                    }
                }
            }
        }

        // HD-2D post-processing: HDR scene -> bloom, tilt-shift, grading -> swapchain.
        let frame = {
            let vp = cam.view_proj(aspect);
            let sun = vp * (cam.target - self.shadow_dir() * cam.distance * 4.0).extend(1.0);
            let amb = glam::Vec3::from(self.lighting.ambient);
            // Haze: the map's ambient light lifted toward a pale sky, in linear space.
            let fog = if outdoor_sky {
                glam::Vec3::from(sky::horizon_color(weathered.as_ref().unwrap(), self.night_t))
            } else {
                amb.lerp(glam::Vec3::new(0.85, 0.88, 0.95), 0.6).powf(2.2)
            };
            post::FrameInfo {
                light_ratio: {
                    let lum = |c: [f32; 3]| c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
                    (lum(self.lighting.diffuse) + lum(self.lighting.ambient)) / (lum(day_light.diffuse) + lum(day_light.ambient)).max(1e-3)
                },
                focus: cam.distance,
                fog_color: fog.to_array(),
                sky: weathered.as_ref().filter(|_| outdoor_sky).map(|profile| post::SkyFog {
                    zenith: sky::zenith_color(profile, self.night_t),
                    gradient: 0.7 + profile.haze * 0.4,
                    height_offset: (50f32.to_radians() - cam.pitch) * 0.3 / 0.82,
                }),
                // No sky, no sun shafts: in caves and rooms the march would streak the
                // brightest thing on screen (an aura, a spell) down the walls.
                sun_uv: (outdoor_sky && sun.w > 0.0).then(|| [sun.x / sun.w * 0.5 + 0.5, 0.5 - sun.y / sun.w * 0.5]),
                underwater: self.underwater,
                heat: self.heat(),
                time: self.secs(),
                eye: cam.eye(),
                inv_view_proj: vp.inverse(),
            }
        };
        self.post.record(&self.device, &self.queue, &mut enc, &targets.post, view, &self.post_settings, &frame, size);

        // The previous frame has been submitted by now; its freed UI textures can go.
        for id in std::mem::take(&mut self.ui_free) {
            self.ui_renderer.free_texture(&id);
        }
        // UI on top of the resolved frame.
        if let Some(ui) = self.ui.take() {
            for (id, delta) in &ui.textures.set {
                for d in delta {
                    self.ui_renderer.update_texture(&self.device, &self.queue, *id, d);
                }
            }
            let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: ui.pixels_per_point };
            if draw_ui {
                self.ui_renderer.update_buffers(&self.device, &self.queue, &mut enc, &ui.primitives, &screen);
                let mut pass = enc
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("ui"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: ui_view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                        })],
                        ..Default::default()
                    })
                    .forget_lifetime();
                self.ui_renderer.render(&mut pass, &ui.primitives, &screen);
            }
            self.ui_free.extend(ui.textures.free.iter().copied());
        }
        let cmd = enc.finish();
        (self.lighting, self.post_settings) = (day_light, day_post);
        cmd
    }

    /// Supplies the UI drawn over the next frame.
    pub fn set_ui(&mut self, ui: UiFrame) {
        // Texture changes (e.g. the font atlas) from frames that were never drawn must still
        // reach the GPU, so pending ones are carried over.
        let mut ui = ui;
        if let Some(mut old) = self.ui.take() {
            old.textures.append(ui.textures);
            ui.textures = old.textures;
        }
        self.ui = Some(ui);
    }

    /// Window pixel position of a world point, or None when behind the camera.
    pub fn project(&self, p: glam::Vec3, size: (u32, u32)) -> Option<glam::Vec2> {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let clip = self.camera.view_proj(w / h) * p.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(glam::Vec2::new((ndc.x + 1.0) * 0.5 * w, (1.0 - ndc.y) * 0.5 * h))
    }

    /// Window pixels one sprite pixel covers at a world point (as sprite.wgsl sizes them:
    /// with perspective, but screen-aligned, so not foreshortened by the camera's tilt).
    /// HUD art that belongs with a sprite, such as spirit spheres, scales by this.
    pub fn sprite_pixel(&self, p: glam::Vec3, size: (u32, u32)) -> Option<f32> {
        let (w, h) = (size.0 as f32, size.1 as f32);
        let clip = self.camera.view_proj(w / h) * p.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let f = 1.0 / (self.camera.fovy / 2.0).tan();
        Some(CELL / sprite::PIXELS_PER_CELL * f / clip.w * 0.5 * h)
    }
}

/// A one-texel-per-cell mask (255 drawn, 0 hidden) for indoor maps.
fn room_mask_view(device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32, cells: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("room mask"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        tex.as_image_copy(),
        cells,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w.max(1)), rows_per_image: None },
        size,
    );
    tex.create_view(&Default::default())
}

fn globals_bind_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, globals: &wgpu::Buffer, shadow: &shadow::Shadow, torches: &[shadow::Shadow; TORCHES], room: &wgpu::TextureView) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("globals"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&shadow.view) },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&shadow.compare) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(room) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&torches[0].view) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&torches[1].view) },
        ],
    })
}

/// How long a frame spends uploading a new map's textures (Scene::pump): the loading screen
/// keeps moving, at about 50 fps or better, while a map's hundreds of megabytes go up.
const UPLOAD_BUDGET: std::time::Duration = std::time::Duration::from_millis(10);

/// A map's textures and ray tracing geometry readied for `Renderer::set_prepared_scene`.
pub struct PreparedScene {
    /// Each texture's mip chain (a missing one is a magenta pixel).
    textures: Vec<Vec<Rgba>>,
    /// Each texture's window mask's mip chain, if it has windows (windows.rs).
    windows: Vec<Option<Vec<Rgba>>>,
    rt: Option<rt::Geometry>,
}

/// The CPU half of `Renderer::set_scene`, which needs no GPU and so can run on any thread:
/// mip chains, and with `rt_layers` (`Renderer::rt_layers`) the ray tracing geometry.
pub fn prepare_scene(mesh: &SceneMesh, images: Vec<Option<Rgba>>, rt_layers: Option<usize>) -> PreparedScene {
    prepare_scene_counted(mesh, images, rt_layers, &|| {})
}

/// `prepare_scene`, calling `each` as every texture's mip chain is done (for a progress bar).
pub fn prepare_scene_counted(mesh: &SceneMesh, images: Vec<Option<Rgba>>, rt_layers: Option<usize>, each: &dyn Fn()) -> PreparedScene {
    let rt = rt_layers.map(|max| {
        let refs: Vec<Option<&Rgba>> = images.iter().map(Option::as_ref).collect();
        rt::split(&mesh.vertices, &mesh.ranges, &refs, max)
    });
    // Windows on the models' textures (the ground has none).
    let windows = images.iter().enumerate().map(|(i, img)| img.as_ref().filter(|_| i >= mesh.ground_textures).and_then(window_mask).map(|m| scene::mip_chain(m, true))).collect::<Vec<_>>();
    PreparedScene { textures: mip_chains(images, each), windows, rt }
}

/// Each texture's mip chain; a missing texture is a magenta pixel.
fn mip_chains(images: Vec<Option<Rgba>>, each: &dyn Fn()) -> Vec<Vec<Rgba>> {
    let fallback = Rgba { width: 1, height: 1, pixels: vec![255, 0, 255, 255] };
    images
        .into_iter()
        .map(|img| {
            let chain = scene::mip_chain(img.unwrap_or_else(|| fallback.clone()), true);
            each();
            chain
        })
        .collect()
}

#[cfg(test)]
mod flicker_tests {
    use super::PointLight;

    #[test]
    fn fires_flicker_and_others_hold_steady() {
        let light = |color| PointLight { pos: glam::Vec3::new(12.0, 0.0, 30.0), color, range: 50.0 };
        let (torch, crystal) = (light([0.5, 0.3, 0.1]), light([0.2, 0.4, 0.6]));
        let samples: Vec<f32> = (0..50).map(|i| torch.flicker(i as f32 * 0.07)).collect();
        assert!(samples.iter().all(|f| (0.75..=1.25).contains(f)));
        assert!(samples.iter().any(|f| (f - 1.0).abs() > 0.1));
        assert!((0..50).all(|i| crystal.flicker(i as f32 * 0.07) == 1.0));
    }
}

#[cfg(test)]
mod prepare_tests {
    use super::*;

    #[test]
    fn textures_get_their_mip_chains_and_missing_ones_a_stand_in() {
        let img = Rgba { width: 4, height: 2, pixels: vec![200; 4 * 2 * 4] };
        let counted = std::cell::Cell::new(0);
        let chains = mip_chains(vec![Some(img), None], &|| counted.set(counted.get() + 1));
        assert_eq!(counted.get(), 2);
        let sizes: Vec<Vec<(u32, u32)>> = chains.iter().map(|l| l.iter().map(|m| (m.width, m.height)).collect()).collect();
        assert_eq!(sizes, [vec![(4, 2), (2, 1), (1, 1)], vec![(1, 1)]]);
        assert_eq!(chains[1][0].pixels, [255, 0, 255, 255]);
    }
}
