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
    // Cloud shadows (cloud_shade).
    clouds: vec4<f32>,
    // Wet ground (wet_surface): x how wet, y how hard it rains now; the sky's colour (linear).
    wet: vec4<f32>,
    wet_sky: vec4<f32>,
    // Wind for swaying foliage: xy its direction (unit, world xz), z how strong (0 still).
    wind: vec4<f32>,
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
    // How far it sways in the wind at most (world units; foliage only).
    @location(5) sway: f32,
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
    let pos = v.pos + sway(v.pos, v.sway);
    o.pos = g.view_proj * vec4(pos, 1.0);
    o.normal = v.normal;
    o.uv = v.uv;
    o.lightmap_uv = v.lightmap_uv;
    o.color = v.color;
    o.world = pos;
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
    // The ground's lightmap coordinates run across it; every model's are one white texel.
    let ground = select(0.0, 1.0, any(fwidth(in.lightmap_uv) > vec2(0.0)));
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
    // A cloud passing over takes some of the sun.
    let ndl = max(dot(flat, -g.light_dir.xyz), 0.0) * cloud_shade(in.world);
    // Ray-traced shadows replace the lightmap's baked ones (which can't follow the sun).
    let rt = (u32(g.rt.w) & 1u) != 0u;
    let light = clamp(g.ambient.rgb + g.diffuse.rgb * ndl, vec3(0.0), vec3(1.0)) * select(lm.a, 1.0, rt);
    // Relief as a ratio of the tilted surface's light to the flat one's, so it shows on maps
    // whose light already saturates (the clamp above would swallow it).
    let tilted = g.ambient.rgb + g.diffuse.rgb * max(dot(n, -g.light_dir.xyz), 0.0) * cloud_shade(in.world);
    let shade = clamp(tilted / max(g.ambient.rgb + g.diffuse.rgb * ndl, vec3(0.05)), vec3(0.5), vec3(1.5));
    let rgb = clamp(t.rgb * in.color * light * shade + lm.rgb, vec3(0.0), vec3(1.0));
    let pl = point_light(in.world, n);
    // The torch light baked into the lightmap wavers with the flame.
    let lit = (srgb_to_linear(rgb) * sprite_shadow(in.world, sun_ray(in.world, flat, 4u)) * (1.0 + pl.w) + srgb_to_linear(t.rgb * in.color) * pl.rgb) * torch_shadows(in.world);
    return FsOut(vec4(wet_surface(lit, in.world, flat, ground), 1.0), vec4(distance(in.world, g.eye.xyz), 0.0, 0.0, 0.0));
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

// Cloud shadows: patches of shade drifting over the map, cast along the sun from a layer of
// cloud overhead (value noise, four octaves). g.clouds: x how much of the sun a cloud takes
// (0 off), y how much of the sky is cloud (0..1), zw the drift (world units a second).
// 1 in the open, less under a cloud; only the sun's share of the light is for it to take.
fn cloud_shade(world: vec3<f32>) -> f32 {
    if g.clouds.x <= 0.0 || g.room.x > 0.5 {
        return 1.0;
    }
    let d = g.light_dir.xyz;
    // Back along the sunlight to the cloud layer, 600 units (120 cells) up.
    let up = max(-d.y, 0.2);
    let q = world.xz + d.xz / up * (world.y - 600.0) - g.clouds.zw * g.params.x;
    var p = q / 260.0;
    var n = 0.0;
    var amp = 0.5;
    for (var o = 0; o < 4; o++) {
        n += cloud_noise(p) * amp;
        p = p * 2.03 + vec2(17.3, 41.9);
        amp *= 0.5;
    }
    let edge = mix(0.68, 0.36, g.clouds.y);
    return 1.0 - g.clouds.x * smoothstep(edge - 0.07, edge + 0.07, n / 0.9375);
}

fn cloud_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = cloud_hash(i);
    let b = cloud_hash(i + vec2(1.0, 0.0));
    let c = cloud_hash(i + vec2(0.0, 1.0));
    let e = cloud_hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, e, u.x), u.y);
}

fn cloud_hash(i: vec2<f32>) -> f32 {
    let v = vec2<u32>(vec2<i32>(i) + vec2(65536));
    var h = v.x * 747796405u + v.y * 2891336453u + 12345u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    return f32(h & 0xffffu) / 65535.0;
}

// Rain-soaked ground (g.wet.x: 0 dry .. 1 soaked; it soaks in and dries out slowly): darker
// and glossy, the sun glinting off it, and on the ground (`ground` 1) puddles in the low
// spots of a slow noise field (more as it gets wetter) holding the sky, with rings where the
// drops land. Surfaces facing up get wet; walls stay as they are.
fn wet_surface(lit: vec3<f32>, world: vec3<f32>, n: vec3<f32>, ground: f32) -> vec3<f32> {
    let w = g.wet.x * smoothstep(0.35, 0.85, n.y);
    if w <= 0.001 || g.room.x > 0.5 {
        return lit;
    }
    let field = cloud_noise(world.xz / 34.0) * 0.65 + cloud_noise(world.xz / 11.0 + vec2(7.1, 3.3)) * 0.35;
    let fill = 0.68 - 0.07 * g.wet.x;
    let puddle = smoothstep(fill, fill + 0.04, field) * w * smoothstep(0.9, 0.97, n.y) * ground;
    let ring = drops(world.xz, g.params.x) * (0.08 + 0.45 * puddle) * g.wet.y * ground;
    let nr = normalize(vec3(n.x + ring.x, n.y, n.z + ring.y));
    let v = normalize(g.eye.xyz - world);
    let fres = pow(1.0 - max(dot(nr, v), 0.0), 5.0);
    // Soaked: darker (a little more on the ground), darker still under a puddle's water.
    let base = lit * (1.0 - (0.22 + 0.13 * ground) * w) * (1.0 - 0.6 * puddle);
    // The drops' rings catch the sky as they spread across a puddle.
    let mirror = g.wet_sky.rgb * ((0.04 + fres) * 0.25 * w * ground + (0.12 + 0.5 * fres + length(ring) * 0.5) * puddle);
    let h = normalize(v - g.light_dir.xyz);
    let glint = pow(max(dot(nr, h), 0.0), 120.0) * (0.25 * w + 1.0 * puddle) * cloud_shade(world);
    return base + mirror + srgb_to_linear(g.diffuse.rgb) * glint;
}

// Raindrops landing: a ring spreading from each, one drop in every 5-unit cell about once a
// second, each cell at its own place and beat. The slope of the rings (for a normal).
fn drops(p: vec2<f32>, t: f32) -> vec2<f32> {
    let size = 5.0;
    let i0 = floor(p / size);
    var slope = vec2(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let c = i0 + vec2(f32(x), f32(y));
            let h1 = drop_hash(c);
            let h2 = drop_hash(c + vec2(31.0, 17.0));
            let age = fract(t * 1.1 + h1 * 7.0 + h2 * 3.0);
            let d = p - (c + vec2(h1, h2)) * size;
            let r = length(d);
            let off = r - age * size * 0.75;
            let ring = (1.0 - age) * exp(-off * off / 0.4) * cos(off * 3.5);
            slope += d / max(r, 1e-3) * ring;
        }
    }
    return slope * 0.5;
}

fn drop_hash(i: vec2<f32>) -> f32 {
    let v = vec2<u32>(vec2<i32>(i) + vec2(65536));
    var h = v.x * 747796405u + v.y * 2891336453u + 99991u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    h = (h >> 22u) ^ h;
    return f32(h & 0xffffu) / 65535.0;
}

// Foliage in the wind (`reach`: how far this vertex sways at most, world units): leaning
// downwind and swaying back, in slow gusts that roll across the map (nearby trees move
// together, far ones out of step), with a quick flutter of the leaves on top.
fn sway(p: vec3<f32>, reach: f32) -> vec3<f32> {
    if reach <= 0.0 || g.wind.z <= 0.0 {
        return vec3(0.0);
    }
    let t = g.params.x;
    let dir = vec3(g.wind.x, 0.0, g.wind.y);
    // Gusts travel downwind: a wave over the map along the wind's direction.
    let along = dot(p.xz, g.wind.xy);
    let phase = along * 0.012 + dot(p.xz, vec2(0.0031, -0.0047));
    let gust = 0.55 + 0.45 * sin(t * 0.45 - along * 0.004);
    let bend = (0.45 + sin(t * 1.3 + phase) * 0.4 + sin(t * 2.2 + phase * 1.7) * 0.15) * gust;
    let flutter = sin(t * 6.5 + dot(p, vec3(0.9, 1.3, 0.7))) * 0.12 + sin(t * 9.1 + dot(p, vec3(-1.1, 0.4, 1.7))) * 0.06;
    let side = vec3(-g.wind.y, 0.0, g.wind.x);
    let k = reach * min(g.wind.z, 2.5);
    // Bending over, the top dips a little.
    return (dir * bend + side * flutter * 0.6 + vec3(0.0, flutter * 0.3 - bend * bend * 0.15, 0.0)) * k;
}

// RT-STUB: replaced by rt.wgsl where the GPU traces rays.
fn sun_ray(world: vec3<f32>, n: vec3<f32>, rays: u32) -> f32 {
    return 1.0;
}

