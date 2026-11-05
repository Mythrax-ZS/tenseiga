//! Ray tracing (wgpu ray queries; GPUs with ray tracing): the map's static geometry in an
//! acceleration structure. The world and sprite shaders cast rays toward the sun for shadows,
//! the water shader casts its reflection. Cut-out texels (leaves, fences, railings) let the
//! light through.

use ro_formats::image::Rgba;
use wgpu::util::DeviceExt;

use crate::scene::Vertex;

/// Edge of a texture layer: textures are sampled down to this for the alpha test and for
/// shading what reflections hit.
const MASK: u32 = 128;

/// The device features and limits ray tracing needs, if the adapter has them.
pub fn device_needs(adapter: &wgpu::Adapter) -> Option<(wgpu::Features, wgpu::Limits)> {
    // Phones: not worth the battery, and the drivers are the least proven.
    let rq = wgpu::Features::EXPERIMENTAL_RAY_QUERY;
    (cfg!(not(target_os = "android")) && adapter.features().contains(rq)).then(|| {
        // Texture layers too: every map texture gets one (`split`).
        let limits = wgpu::Limits { max_texture_array_layers: adapter.limits().max_texture_array_layers, ..wgpu::Limits::default() };
        (rq, limits.using_acceleration_structure_values(adapter.limits()))
    })
}

/// `src` (scene, sprite or water shader) with rt.wgsl's functions in place of its stubs, its
/// bindings in bind group `group`.
pub fn join(src: &str, group: u32) -> String {
    let rt = include_str!("rt.wgsl").replace("@group(2)", &format!("@group({group})"));
    format!("{rt}
{}", &src[..src.find("// RT-STUB").expect("shader stub")])
}

pub fn layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    let entry = |binding, ty| wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::FRAGMENT, ty, count: None };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ray tracing"),
        entries: &[
            entry(0, wgpu::BindingType::AccelerationStructure { vertex_return: false }),
            entry(1, wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None }),
            entry(2, wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2Array, multisampled: false }),
            entry(3, wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)),
        ],
    })
}

/// The scene's acceleration structure and surface data, bound to the world, sprite and water
/// shaders. `textures[i]` is the image used by `ranges[i]` of `vertices`.
pub fn build(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, vertices: &[Vertex], ranges: &[std::ops::Range<u32>], textures: &[Option<&Rgba>]) -> wgpu::BindGroup {
    upload(device, queue, layout, split(vertices, ranges, textures, device.limits().max_texture_array_layers as usize))
}

/// The CPU side of `build` (see `split`), ready to upload.
pub struct Geometry {
    opaque: Vec<[f32; 3]>,
    opaque_n: u32,
    info: Vec<[f32; 4]>,
    layers: Vec<u8>,
}

/// vec4s per triangle in the info buffer: (uv0, uv1), (uv2, layer or -1, 0), (colour, 0),
/// (normal, 0). Entry 0 holds the number of opaque triangles.
const INFO: usize = 4;

/// Opaque triangles first, then cut-out ones, so each is one BLAS geometry and only the
/// cut-outs run the alpha test. Returns the positions, how many are opaque, each triangle's
/// info (for the alpha test and for shading what a reflection hits) and the textures sampled
/// down to MASK² RGBA layers; none is empty.
pub fn split(vertices: &[Vertex], ranges: &[std::ops::Range<u32>], textures: &[Option<&Rgba>], max_layers: usize) -> Geometry {
    let layer_bytes = (MASK * MASK * 4) as usize;
    let (mut opaque, mut cut, mut opaque_info, mut cut_info, mut layers) = (vec![], vec![], vec![], vec![], vec![]);
    for (range, img) in ranges.iter().zip(textures) {
        let tris = &vertices[range.start as usize..range.end as usize];
        // shortcut: past the GPU's layer limit (256+) textures shade grey and count as opaque.
        let img = img.filter(|_| layers.len() / layer_bytes < max_layers);
        let layer = img.map_or(-1.0, |img| {
            layers.extend(shrink(img));
            (layers.len() / layer_bytes - 1) as f32
        });
        let keyed = img.is_some_and(|i| i.pixels.chunks_exact(4).any(|p| p[3] < 128));
        for t in tris.chunks_exact(3) {
            let avg = |f: fn(&Vertex) -> [f32; 3]| [0, 1, 2].map(|k| t.iter().map(|v| f(v)[k]).sum::<f32>() / 3.0);
            let [r, g, b] = avg(|v| v.color);
            let [x, y, z] = avg(|v| v.normal);
            let rec = [[t[0].uv[0], t[0].uv[1], t[1].uv[0], t[1].uv[1]], [t[2].uv[0], t[2].uv[1], layer, 0.0], [r, g, b, 0.0], [x, y, z, 0.0]];
            let (pos, info) = if keyed { (&mut cut, &mut cut_info) } else { (&mut opaque, &mut opaque_info) };
            pos.extend(t.iter().map(|v| v.pos));
            info.extend(rec);
        }
    }
    // Something to trace even on an empty map: a triangle far below it.
    if opaque.is_empty() && cut.is_empty() {
        opaque = vec![[0.0, -1e6, 0.0], [1.0, -1e6, 0.0], [0.0, -1e6, 1.0]];
        opaque_info = vec![[0.0; 4]; INFO];
    }
    if layers.is_empty() {
        layers.resize(layer_bytes, 255);
    }
    let opaque_n = opaque.len() as u32;
    opaque.extend(cut);
    let mut info = vec![[(opaque_n / 3) as f32, 0.0, 0.0, 0.0]];
    info.extend(opaque_info);
    info.extend(cut_info);
    Geometry { opaque, opaque_n, info, layers }
}

/// `img` box-filtered to MASK² RGBA.
fn shrink(img: &Rgba) -> Vec<u8> {
    let mut out = Vec::with_capacity((MASK * MASK * 4) as usize);
    for y in 0..MASK {
        for x in 0..MASK {
            let (x0, y0) = (x * img.width / MASK, y * img.height / MASK);
            let (x1, y1) = (((x + 1) * img.width / MASK).max(x0 + 1), ((y + 1) * img.height / MASK).max(y0 + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = ((sy * img.width + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += img.pixels[p + c] as u32;
                    }
                }
            }
            let n = (x1 - x0) * (y1 - y0);
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    out
}

pub fn upload(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, geometry: Geometry) -> wgpu::BindGroup {
    let Geometry { opaque, opaque_n, info, layers } = geometry;
    let positions = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("rt positions"),
        contents: bytemuck::cast_slice(&opaque),
        usage: wgpu::BufferUsages::BLAS_INPUT,
    });
    let sizes: Vec<_> = [(opaque_n, wgpu::AccelerationStructureGeometryFlags::OPAQUE), (opaque.len() as u32 - opaque_n, wgpu::AccelerationStructureGeometryFlags::empty())]
        .into_iter()
        .scan(0, |first, (count, flags)| {
            let at = *first;
            *first += count;
            Some((at, wgpu::BlasTriangleGeometrySizeDescriptor { vertex_format: wgpu::VertexFormat::Float32x3, vertex_count: count, index_format: None, index_count: None, flags }))
        })
        .filter(|(_, s)| s.vertex_count > 0)
        .collect();
    let blas = device.create_blas(
        &wgpu::CreateBlasDescriptor { label: Some("map"), flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE, update_mode: wgpu::AccelerationStructureUpdateMode::Build },
        wgpu::BlasGeometrySizeDescriptors::Triangles { descriptors: sizes.iter().map(|(_, s)| s.clone()).collect() },
    );
    let mut tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
        label: Some("map"),
        max_instances: 1,
        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
    });
    tlas[0] = Some(wgpu::TlasInstance::new(&blas, [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0], 0, 0xFF));
    let geometry = sizes
        .iter()
        .map(|(first, size)| wgpu::BlasTriangleGeometry {
            size,
            vertex_buffer: &positions,
            first_vertex: *first,
            vertex_stride: 12,
            index_buffer: None,
            first_index: None,
            transform_buffer: None,
            transform_buffer_offset: None,
        })
        .collect();
    let mut enc = device.create_command_encoder(&Default::default());
    enc.build_acceleration_structures([&wgpu::BlasBuildEntry { blas: &blas, geometry: wgpu::BlasGeometries::TriangleGeometries(geometry) }], [&tlas]);
    queue.submit([enc.finish()]);

    let info = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("rt surfaces"), contents: bytemuck::cast_slice(&info), usage: wgpu::BufferUsages::STORAGE });
    let count = (layers.len() / (MASK * MASK * 4) as usize) as u32;
    let textures = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some("rt textures"),
            size: wgpu::Extent3d { width: MASK, height: MASK, depth_or_array_layers: count },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &layers,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::Repeat,
        address_mode_v: wgpu::AddressMode::Repeat,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ray tracing"),
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::AccelerationStructure(&tlas) },
            wgpu::BindGroupEntry { binding: 1, resource: info.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&textures.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })) },
            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri(z: f32) -> [Vertex; 3] {
        [0.0, 1.0, 2.0].map(|u| Vertex { pos: [u, 0.0, z], normal: [0.0; 3], uv: [u, z], lightmap_uv: [0.0; 2], color: [1.0; 3] })
    }

    #[test]
    fn joined_shaders_validate() {
        let check = |src: &str, caps| {
            let module = naga::front::wgsl::parse_str(src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(src)));
            naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps).validate(&module).unwrap_or_else(|e| panic!("{e:?}"));
        };
        for (src, group) in [(include_str!("scene.wgsl"), 2), (include_str!("sprite.wgsl"), 2), (include_str!("water.wgsl"), 3)] {
            check(src, naga::valid::Capabilities::empty());
            check(&join(src, group), naga::valid::Capabilities::RAY_QUERY);
        }
    }

    #[test]
    fn cut_outs_go_last_and_every_triangle_has_its_surface() {
        let solid = Rgba { width: 2, height: 2, pixels: vec![255; 16] };
        // Left column see-through.
        let keyed = Rgba { width: 2, height: 2, pixels: (0..4).flat_map(|i| [9, 9, 9, if i % 2 == 0 { 0 } else { 255 }]).collect() };
        let vertices: Vec<Vertex> = [tri(1.0), tri(2.0), tri(3.0)].concat();
        let Geometry { opaque: pos, opaque_n: opaque, info, layers } = split(&vertices, &[0..3, 3..6, 6..9], &[Some(&keyed), Some(&solid), None], 8);
        assert_eq!(opaque, 6);
        assert_eq!(pos.len(), 9);
        assert_eq!(pos[6][2], 1.0, "the keyed triangle comes after the opaque ones");
        // Header, then the solid one (layer 1), the untextured one (-1), the keyed one (layer 0).
        assert_eq!(info.len(), 1 + 3 * INFO);
        assert_eq!(info[0][0], 2.0);
        assert_eq!((info[2][2], info[6][2], info[10][2]), (1.0, -1.0, 0.0));
        assert_eq!(&info[9..11], &[[0.0, 1.0, 1.0, 1.0], [2.0, 1.0, 0.0, 0.0]]);
        assert_eq!(layers.len(), 2 * (MASK * MASK * 4) as usize);
        assert_eq!((layers[3], layers[((MASK - 1) * 4 + 3) as usize]), (0, 255));
        // Past the layer limit a texture shades grey and counts as opaque.
        assert_eq!(split(&vertices, &[0..3, 3..6, 6..9], &[Some(&keyed), Some(&solid), None], 0).opaque_n, 9);
    }
}
