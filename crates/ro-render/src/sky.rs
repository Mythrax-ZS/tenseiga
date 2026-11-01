//! A map-styled sky behind the world. It has its own camera-relative panorama, so it
//! follows camera rotation but never moves with the player or changes with camera distance.

use crate::{post, sky_profile::SkyProfile, Camera, DEPTH_FORMAT};

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniform {
    zenith: [f32; 4],
    horizon: [f32; 4],
    cloud: [f32; 4],
    /// Cloud cover, scale, wind speed, haze.
    clouds: [f32; 4],
    /// Seconds, night blend, star strength, stable map seed.
    time: [f32; 4],
    /// Camera yaw, pitch, aspect, vertical field of view.
    camera: [f32; 4],
    /// Direction toward the sun (opposite the direction light travels).
    /// w: 1 for open space (the galaxy) instead of a sky.
    sun: [f32; 4],
}

pub(crate) struct Sky {
    uniform: wgpu::Buffer,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

/// Match the horizon in sky.wgsl so distant geometry disappears into the same haze.
pub(crate) fn horizon_color(profile: &SkyProfile, night: f32) -> [f32; 3] {
    if profile.galaxy {
        return profile.horizon;
    }
    let day = glam::Vec3::from(profile.horizon);
    let moonlight = day * glam::Vec3::new(0.08, 0.10, 0.18) + glam::Vec3::new(0.008, 0.012, 0.027);
    day.lerp(moonlight, night.clamp(0.0, 1.0)).to_array()
}

/// Match the upper end of the sky gradient when shading distant outdoor geometry.
pub(crate) fn zenith_color(profile: &SkyProfile, night: f32) -> [f32; 3] {
    if profile.galaxy {
        return profile.zenith;
    }
    let day = glam::Vec3::from(profile.zenith);
    let moonlight = day * glam::Vec3::new(0.06, 0.08, 0.17) + glam::Vec3::new(0.004, 0.007, 0.018);
    day.lerp(moonlight, night.clamp(0.0, 1.0)).to_array()
}

impl Sky {
    pub fn new(device: &wgpu::Device, samples: u32) -> Self {
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sky"),
            size: std::mem::size_of::<Uniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sky"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sky"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sky"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sky.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sky"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[
                    Some(wgpu::ColorTargetState { format: post::HDR_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                    Some(wgpu::ColorTargetState { format: post::INFO_FORMAT, blend: None, write_mask: wgpu::ColorWrites::ALL }),
                ],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self { uniform, bind, pipeline }
    }

    pub fn update(
        &self,
        queue: &wgpu::Queue,
        profile: &SkyProfile,
        camera: &Camera,
        aspect: f32,
        seconds: f32,
        night: f32,
        sun_direction: [f32; 3],
    ) {
        let sun = -glam::Vec3::from(sun_direction).try_normalize().unwrap_or(glam::Vec3::NEG_Y);
        let uniform = Uniform {
            zenith: [profile.zenith[0], profile.zenith[1], profile.zenith[2], 0.0],
            horizon: [profile.horizon[0], profile.horizon[1], profile.horizon[2], 0.0],
            cloud: [profile.cloud[0], profile.cloud[1], profile.cloud[2], 0.0],
            clouds: [profile.cloud_cover, profile.cloud_scale, profile.wind_speed, profile.haze],
            time: [seconds, night.clamp(0.0, 1.0), profile.stars, profile.seed],
            camera: [camera.yaw, camera.pitch, aspect.max(0.01), camera.fovy],
            sun: sun.extend(if profile.galaxy { 1.0 } else { 0.0 }).to_array(),
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
    }

    /// Draw before geometry. This replaces bind group 0; the world must restore its globals.
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        let source = include_str!("sky.wgsl");
        let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
            .validate(&module)
            .expect("sky.wgsl validates");
    }
}
