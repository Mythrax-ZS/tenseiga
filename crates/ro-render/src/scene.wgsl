// World geometry (ground + models). Lighting is computed in gamma space like the
// original client, then converted for the sRGB target.

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    // xyz: direction the sun travels (y-up world)
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    params: vec4<f32>,
    // The sun's view of the sprite shadow casters.
    light_vp: mat4x4<f32>,
    // x: strength (0 off), y: shadow-map texel size, z: softness in texels
    shadow: vec4<f32>,
    screen: vec4<f32>,
    // x: sprite lighting, y: rim, z: point light intensity, w: light count
    sprite_light: vec4<f32>,
    // Pairs: (position, range), (colour, 0).
    lights: array<vec4<f32>, 64>,
    // x: 1 when indoors (room mask active), y/z: mask size in cells, w: world units per cell
    room: vec4<f32>,
    // (Wading, used by sprites.)
    water: vec4<f32>,
    water_tint: vec4<f32>,
    // x: 1 to filter magnified textures with Catmull-Rom (texture upscaling)
    upscale: vec4<f32>,
    // xyz: direction the sun's light travels, w: ray tracing on (bits: 1 shadows, 2 reflections)
    rt: vec4<f32>,
    // Torches whose light sprites shadow: shadow-map view-projections, (position, range), and
    // x/y each one's strength (0 none), z texel size.
    torch_vp: array<mat4x4<f32>, 2>,
    torch: array<vec4<f32>, 2>,
    torch_k: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
@group(0) @binding(3) var room_mask: texture_2d<f32>;
@group(0) @binding(4) var torch_map0: texture_depth_2d;
@group(0) @binding(5) var torch_map1: texture_depth_2d;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var lightmap: texture_2d<f32>;
@group(1) @binding(3) var lightmap_samp: sampler;

struct VsIn {
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) lightmap_uv: vec2<f32>,
    @location(4) color: vec3<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) lightmap_uv: vec2<f32>,
    @location(3) color: vec3<f32>,
    @location(4) world: vec3<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = g.view_proj * vec4(v.pos, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.lightmap_uv = v.lightmap_uv;
    o.color = v.color;
    o.world = v.pos;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

struct FsOut {
    @location(0) color: vec4<f32>,
    // x: distance from the eye, y: 0 (not a sprite)
    @location(1) info: vec4<f32>,
};

// Catmull-Rom (bicubic, B = 0, C = 0.5) from 9 bilinear taps of the top mip: the middle
// pair of each axis is merged into one tap (Jimenez / MJP). Used only where the texture is
// magnified. World textures only; sprites have their own shader and are never filtered here.
fn catmull_rom(uv: vec2<f32>, size: vec2<f32>) -> vec4<f32> {
    let pos = uv * size;
    let p1 = floor(pos - 0.5) + 0.5;
    let f = pos - p1;
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);
    let w12 = w1 + w2;
    let t0 = (p1 - 1.0) / size;
    let t3 = (p1 + 2.0) / size;
    let t12 = (p1 + w2 / w12) / size;
    var c = vec4(0.0);
    c += textureSampleLevel(tex, samp, vec2(t0.x, t0.y), 0.0) * w0.x * w0.y;
    c += textureSampleLevel(tex, samp, vec2(t12.x, t0.y), 0.0) * w12.x * w0.y;
    c += textureSampleLevel(tex, samp, vec2(t3.x, t0.y), 0.0) * w3.x * w0.y;
    c += textureSampleLevel(tex, samp, vec2(t0.x, t12.y), 0.0) * w0.x * w12.y;
    c += textureSampleLevel(tex, samp, vec2(t12.x, t12.y), 0.0) * w12.x * w12.y;
    c += textureSampleLevel(tex, samp, vec2(t3.x, t12.y), 0.0) * w3.x * w12.y;
    c += textureSampleLevel(tex, samp, vec2(t0.x, t3.y), 0.0) * w0.x * w3.y;
    c += textureSampleLevel(tex, samp, vec2(t12.x, t3.y), 0.0) * w12.x * w3.y;
    c += textureSampleLevel(tex, samp, vec2(t3.x, t3.y), 0.0) * w3.x * w3.y;
    // Catmull-Rom overshoots slightly at hard edges; keep it in range.
    return clamp(c, vec4(0.0), vec4(1.0));
}

@fragment
fn fs_main(in: VsOut) -> FsOut {
    if !in_room(in.world) {
        discard;
    }
    // Gradients up front (uniform control flow), so both paths below can use them.
    let ddx = dpdx(in.uv);
    let ddy = dpdy(in.uv);
    let dwx = dpdx(in.world);
    let dwy = dpdy(in.world);
    var t: vec4<f32>;
    let size = vec2<f32>(textureDimensions(tex));
    let footprint = max(dot(ddx * size, ddx * size), dot(ddy * size, ddy * size));
    if g.upscale.x > 0.5 && footprint < 1.0 {
        // Magnified (a texel covers more than a pixel): Catmull-Rom instead of bilinear,
        // so close-up ground and models stay sharp without blocky or blurry texels.
        t = catmull_rom(in.uv, size);
    } else {
        t = textureSampleGrad(tex, samp, in.uv, ddx, ddy);
    }
    if t.a < 0.5 {
        discard;
    }
    let lm = textureSample(lightmap, lightmap_samp, in.lightmap_uv);
    let flat = normalize(in.normal);
    let n = relief(flat, in.uv, ddx, ddy, dwx, dwy, size);
    let ndl = max(dot(flat, -g.light_dir.xyz), 0.0);
    // Ray-traced shadows replace the lightmap's baked ones (which can't follow the sun).
    let rt = (u32(g.rt.w) & 1u) != 0u;
    let light = clamp(g.ambient.rgb + g.diffuse.rgb * ndl, vec3(0.0), vec3(1.0)) * select(lm.a, 1.0, rt);
    // Relief as a ratio of the tilted surface's light to the flat one's, so it shows on maps
    // whose light already saturates (the clamp above would swallow it).
    let tilted = g.ambient.rgb + g.diffuse.rgb * max(dot(n, -g.light_dir.xyz), 0.0);
    let shade = clamp(tilted / max(g.ambient.rgb + g.diffuse.rgb * ndl, vec3(0.05)), vec3(0.5), vec3(1.5));
    let rgb = clamp(t.rgb * in.color * light * shade + lm.rgb, vec3(0.0), vec3(1.0));
    let pl = point_light(in.world, n);
    // The torch light baked into the lightmap wavers with the flame.
    let lit = (srgb_to_linear(rgb) * sprite_shadow(in.world, sun_ray(in.world, flat, 4u)) * (1.0 + pl.w) + srgb_to_linear(t.rgb * in.color) * pl.rgb) * torch_shadows(in.world);
    return FsOut(vec4(lit, 1.0), vec4(distance(in.world, g.eye.xyz), 0.0, 0.0, 0.0));
}

// Surface relief: the texture's own brightness read as height (pale stone stands out of dark
// mortar) and turned into a tilted normal for the sun and point lights. g.upscale.y is the
// depth in texels from black to white (0 off). Bump mapping without tangents (Mikkelsen 2010).
fn relief(n: vec3<f32>, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>, dwx: vec3<f32>, dwy: vec3<f32>, size: vec2<f32>) -> vec3<f32> {
    let depth = g.upscale.y;
    if depth <= 0.0 {
        return n;
    }
    let w = vec3(0.299, 0.587, 0.114);
    let h = dot(textureSampleGrad(tex, samp, uv, ddx, ddy).rgb, w);
    let hu = dot(textureSampleGrad(tex, samp, uv + vec2(1.0 / size.x, 0.0), ddx, ddy).rgb, w);
    let hv = dot(textureSampleGrad(tex, samp, uv + vec2(0.0, 1.0 / size.y), ddx, ddy).rgb, w);
    // Height change per unit of uv, in world units (a texel's world size times the depth).
    let texel = length(dwx) / max(length(ddx * size), 1e-4);
    let dh = vec2(hu - h, hv - h) * size * depth * texel;
    let r1 = cross(dwy, n);
    let r2 = cross(n, dwx);
    let det = dot(dwx, r1);
    let grad = sign(det) * (dot(dh, ddx) * r1 + dot(dh, ddy) * r2);
    return normalize(abs(det) * n - grad);
}

// How lit a point is by the sun: 1 lit, 1 - strength fully shadowed. Sprites cast through the
// shadow map, where a 5x5 PCF kernel (scaled by softness) gives soft, painterly edges; `sun`
// is what the map's own geometry lets through (ray traced, else 1).
fn sprite_shadow(world: vec3<f32>, sun: f32) -> f32 {
    if g.shadow.x <= 0.0 {
        return 1.0;
    }
    let lp = g.light_vp * vec4(world, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) || ndc.z > 1.0 {
        return 1.0 - g.shadow.x * (1.0 - sun);
    }
    let step = g.shadow.y * max(g.shadow.z, 0.5);
    var lit = 0.0;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            // A hair of bias (~0.4 world units over the 4000-unit depth range) so the
            // shadow meets the feet instead of starting a cell away.
            lit += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2(f32(x), f32(y)) * step, ndc.z - 0.0001);
        }
    }
    lit = min(lit / 25.0, sun);
    return 1.0 - g.shadow.x * (1.0 - lit);
}

// How much the nearest torches still light `world` past the sprites standing in their way
// (1 unshadowed): each shadow is as strong as that torch's light is here, so it shows on
// baked torchlight too.
fn torch_shadows(world: vec3<f32>) -> f32 {
    return torch_shadow(0, torch_map0, world) * torch_shadow(1, torch_map1, world);
}

fn torch_shadow(i: i32, map: texture_depth_2d, world: vec3<f32>) -> f32 {
    let k = select(g.torch_k.y, g.torch_k.x, i == 0);
    let l = g.torch[i];
    let near = saturate(1.0 - distance(world, l.xyz) / max(l.w, 1.0));
    let lp = g.torch_vp[i] * vec4(world, 1.0);
    if k <= 0.0 || near <= 0.0 || lp.w <= 0.0 {
        return 1.0;
    }
    let ndc = lp.xyz / lp.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) || ndc.z > 1.0 {
        return 1.0;
    }
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            lit += textureSampleCompareLevel(map, shadow_samp, uv + vec2(f32(x), f32(y)) * g.torch_k.z * 1.5, ndc.z);
        }
    }
    return 1.0 - k * sqrt(near) * (1.0 - lit / 9.0);
}

// Light from the map's light sources (torches, lamps): smooth falloff to their range,
// half-wrapped around surfaces so lamps also light what is beside them. w: how much brighter
// or darker flickering flames make it here (the colour's w is each light's swing).
fn point_light(world: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    var sum = vec3(0.0);
    var flicker = 0.0;
    let count = i32(g.sprite_light.w);
    for (var i = 0; i < count; i++) {
        let l = g.lights[i * 2];
        let to = l.xyz - world;
        let d = length(to);
        let f = saturate(1.0 - d / max(l.w, 1.0));
        let wrap = saturate(dot(n, to / max(d, 1e-3)) * 0.5 + 0.5);
        sum += g.lights[i * 2 + 1].rgb * f * f * wrap;
        flicker += g.lights[i * 2 + 1].w * f * f;
    }
    return vec4(sum, clamp(flicker * 0.6, -0.3, 0.3)) * g.sprite_light.z;
}

/// Indoors, only the room the player is in is drawn (other rooms share the map).
fn in_room(world: vec3<f32>) -> bool {
    if g.room.x < 0.5 {
        return true;
    }
    let cell = vec2<i32>(floor(world.xz / g.room.w));
    if cell.x < 0 || cell.y < 0 || cell.x >= i32(g.room.y) || cell.y >= i32(g.room.z) {
        return false;
    }
    return textureLoad(room_mask, cell, 0).r > 0.5;
}

// RT-STUB: replaced by rt.wgsl where the GPU traces rays.
fn sun_ray(world: vec3<f32>, n: vec3<f32>, rays: u32) -> f32 {
    return 1.0;
}
