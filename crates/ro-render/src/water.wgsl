// Water surface. Every feature is a run-time bit in `w.flags.x` (see water.rs `flag`), so
// the quality presets switch without recompiling. Lighting is linear; the HDR target and
// post stack (bloom, fog, grading) take it from there.
//
// Inputs: group 0 the shared globals; group 1 the map's water (animation frame, uniform,
// detail texture, shore/depth map, sky map); group 2 copies of the opaque scene's colour
// and linear depth (refraction, absorption depth, screen-space reflections).

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    // xyz: direction the sun travels (y-up world)
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    // x: seconds since start
    params: vec4<f32>,
    light_vp: mat4x4<f32>,
    shadow: vec4<f32>,
    // w, h, 1/w, 1/h
    screen: vec4<f32>,
    sprite_light: vec4<f32>,
    lights: array<vec4<f32>, 64>,
    room: vec4<f32>,
    water: vec4<f32>,
    water_tint: vec4<f32>,
    upscale: vec4<f32>,
    // xyz: direction the sun's light travels, w: ray tracing on (bits: 1 shadows, 2 reflections)
    rt: vec4<f32>,
    // (Torches and cloud shadows, used by the scene.)
    torch_vp: array<mat4x4<f32>, 2>,
    torch: array<vec4<f32>, 2>,
    torch_k: vec4<f32>,
    clouds: vec4<f32>,
    // x how wet (rain on the water: rings where drops land), y how hard it rains now,
    // z how much of the water is frozen (snowy maps: 0 open .. 1 frozen over).
    wet: vec4<f32>,
    wet_sky: vec4<f32>,
};

struct Water {
    // level (y-up), RSW wave height, RSW wave speed (deg/tick), RSW wave pitch (deg/tile)
    level: vec4<f32>,
    // frames per texture step, map width, map height (world), world units per tile
    map: vec4<f32>,
    // feature bits, SSR steps
    flags: vec4<u32>,
    // absorption rgb (per world unit), depth scale
    absorb: vec4<f32>,
    // scatter rgb, classic texture mix
    scatter: vec4<f32>,
    // foam rgb, intensity
    foam_color: vec4<f32>,
    // falloff, speed, scale, crest
    foam: vec4<f32>,
    // refraction strength, reflection strength, SSR distance, shininess
    optics: vec4<f32>,
    // normal strength, tiling, speed
    normal: vec4<f32>,
    // amplitude, wavelength, steepness, speed
    waves: vec4<f32>,
    // mean sky radiance rgb, sky brightness now
    sky: vec4<f32>,
    // sun rgb, sun intensity
    sun: vec4<f32>,
    // x, z, birth time (s), strength; count in flags.z
    ripples: array<vec4<f32>, 48>,
    // x, z, depth at the feet, 1 when walking; count in flags.w
    waders: array<vec4<f32>, 16>,
};

const F_WAVES: u32 = 1u;
const F_NORMAL_MAP: u32 = 2u;
const F_REFRACTION: u32 = 4u;
const F_REFLECTION: u32 = 8u;
const F_SSR: u32 = 16u;
const F_ABSORPTION: u32 = 32u;
const F_FOAM: u32 = 64u;
const F_BICUBIC: u32 = 128u;
const F_CLASSIC: u32 = 256u;
const F_SPECULAR: u32 = 512u;
const F_CAUSTICS: u32 = 1024u;
const F_INTERACT: u32 = 2048u;
const F_LAVA: u32 = 4096u;
// Keep in sync with water.rs.
const RIPPLE_LIFE: f32 = 2.2;

// Keep in sync with water.rs.
const DEPTH_RANGE: f32 = 64.0;
const SHORE_RANGE: f32 = 128.0;
const PI: f32 = 3.14159265;
const TAU: f32 = 6.28318531;

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var frame_tex: texture_2d<f32>;
@group(1) @binding(1) var wrap: sampler;
@group(1) @binding(2) var<uniform> w: Water;
// RG normal, B height, A foam pattern (tileable).
@group(1) @binding(3) var detail: texture_2d<f32>;
// R depth / DEPTH_RANGE, G distance to shore / SHORE_RANGE, per GND corner.
@group(1) @binding(4) var shore_map: texture_2d<f32>;
// Equirectangular sky, sRGB.
@group(1) @binding(5) var sky_map: texture_2d<f32>;
@group(1) @binding(6) var clamp_samp: sampler;
@group(2) @binding(0) var scene_color: texture_2d<f32>;
// x: linear distance from the eye.
@group(2) @binding(1) var scene_depth: texture_2d<f32>;

fn on(f: u32) -> bool {
    return (w.flags.x & f) != 0u;
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) tile: vec2<f32>,
    // Analytic wave normal and the crest height above the rest level.
    @location(2) normal: vec3<f32>,
    @location(3) crest: f32,
};

fn shore_uv(xz: vec2<f32>) -> vec2<f32> {
    let corners = vec2<f32>(textureDimensions(shore_map));
    return (xz / w.map.w + 0.5) / corners;
}

// One Gerstner wave: adds displacement and the normal's slope terms.
fn gerstner(p: vec2<f32>, dir: vec2<f32>, len: f32, amp: f32, steep: f32, t: f32, disp: ptr<function, vec3<f32>>, nrm: ptr<function, vec3<f32>>) {
    let k = TAU / len;
    let c = sqrt(9.8 * len / TAU) * 0.35 * w.waves.w;
    let f = k * (dot(dir, p) - c * t);
    let q = steep / (k * amp * 3.0 + 1e-4);
    let cf = cos(f);
    let sf = sin(f);
    *disp += vec3(q * amp * dir.x * cf, amp * sf, q * amp * dir.y * cf);
    *nrm += vec3(-dir.x * k * amp * cf, -q * k * amp * sf, -dir.y * k * amp * cf);
}

@vertex
fn vs_main(@location(0) pos: vec2<f32>, @location(1) tile: vec2<f32>) -> VsOut {
    let t = g.params.x;
    // RO's own sine swell (RSW wave height / speed / pitch).
    let ticks = t * 60.0;
    let phase = radians(ticks * w.level.z + (tile.x + tile.y) * w.level.w * 0.5);
    var p = vec3(pos.x, w.level.x + sin(phase) * w.level.y, pos.y);
    var n = vec3(0.0, 1.0, 0.0);
    var crest = 0.0;
    if on(F_WAVES) {
        // Swell calms toward the shore so it never climbs the beach.
        let depth = textureSampleLevel(shore_map, clamp_samp, shore_uv(pos), 0.0).r * DEPTH_RANGE;
        let calm = smoothstep(0.5, 10.0, depth);
        let a = w.waves.x * calm;
        let l = w.waves.y;
        let s = w.waves.z;
        var disp = vec3(0.0);
        var slope = vec3(0.0, 1.0, 0.0);
        gerstner(pos, normalize(vec2(1.0, 0.35)), l, a, s, t, &disp, &slope);
        gerstner(pos, normalize(vec2(-0.45, 1.0)), l * 0.61, a * 0.55, s, t, &disp, &slope);
        gerstner(pos, normalize(vec2(0.7, -0.75)), l * 0.37, a * 0.32, s, t, &disp, &slope);
        gerstner(pos, normalize(vec2(-0.9, -0.2)), l * 0.23, a * 0.18, s, t, &disp, &slope);
        p += disp;
        n = normalize(slope);
        // 0 at rest .. 1 at the very top of all four waves stacked.
        crest = disp.y / max(a * 2.05, 1e-3);
    }
    var o: VsOut;
    o.pos = g.view_proj * vec4(p, 1.0);
    o.world = p;
    o.tile = tile;
    o.normal = n;
    o.crest = crest;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

// Cubic B-spline filtering from four bilinear taps (smooth, no grid artefacts when a
// small texture is stretched far). Gradients come from the caller so mips still apply.
fn bicubic(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(t));
    let p = uv * size - 0.5;
    let f = fract(p);
    let i = floor(p);
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 0.5 + w1 / g0) / size;
    let h1 = (i + 1.5 + w3 / g1) / size;
    let a = textureSampleGrad(t, s, vec2(h0.x, h0.y), ddx, ddy);
    let b = textureSampleGrad(t, s, vec2(h1.x, h0.y), ddx, ddy);
    let c = textureSampleGrad(t, s, vec2(h0.x, h1.y), ddx, ddy);
    let d = textureSampleGrad(t, s, vec2(h1.x, h1.y), ddx, ddy);
    return g0.y * (g0.x * a + g1.x * b) + g1.y * (g0.x * c + g1.x * d);
}

fn filtered(t: texture_2d<f32>, s: sampler, uv: vec2<f32>, ddx: vec2<f32>, ddy: vec2<f32>) -> vec4<f32> {
    if on(F_BICUBIC) {
        return bicubic(t, s, uv, ddx, ddy);
    }
    return textureSampleGrad(t, s, uv, ddx, ddy);
}

fn sky_uv(d: vec3<f32>) -> vec2<f32> {
    let u = atan2(d.z, d.x) / TAU;
    let v = acos(clamp(d.y, -1.0, 1.0)) / PI;
    return vec2(fract(u), v);
}

// Sky radiance in a direction, dimmed with the night.
fn sky(d: vec3<f32>) -> vec3<f32> {
    let c = textureSampleLevel(sky_map, wrap, sky_uv(d), 0.0).rgb;
    // The sky texture is sRGB: sampling has already decoded it to linear light.
    return c * w.sky.w;
}

// Linear eye distance of the opaque scene at a screen uv.
fn scene_dist(uv: vec2<f32>) -> f32 {
    let px = vec2<i32>(clamp(uv, vec2(0.0), vec2(0.9999)) * g.screen.xy);
    return textureLoad(scene_depth, px, 0).x;
}

fn to_screen(p: vec3<f32>) -> vec3<f32> {
    let c = g.view_proj * vec4(p, 1.0);
    let n = c.xyz / c.w;
    return vec3(n.x * 0.5 + 0.5, 0.5 - n.y * 0.5, c.w);
}

// Short screen-space reflection: march the reflected ray and take the scene where it
// passes behind visible geometry. Returns colour and confidence (0 = use the sky).
fn ssr(origin: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    let steps = max(w.flags.y, 1u);
    let step_len = w.optics.z / f32(steps);
    var t = step_len * 0.5;
    for (var i = 0u; i < steps; i++) {
        let q = origin + dir * t;
        let s = to_screen(q);
        if s.z <= 0.0 || any(s.xy < vec2(0.0)) || any(s.xy > vec2(1.0)) {
            break;
        }
        let behind = distance(g.eye.xyz, q) - scene_dist(s.xy);
        if behind > 0.5 && behind < step_len * 2.0 {
            let edge = min(min(s.x, 1.0 - s.x), min(s.y, 1.0 - s.y));
            let fade = smoothstep(0.0, 0.08, edge) * (1.0 - f32(i) / f32(steps));
            return vec4(textureSampleLevel(scene_color, clamp_samp, s.xy, 0.0).rgb, fade);
        }
        t += step_len;
    }
    return vec4(0.0);
}

// Caustics on the refracted floor (F_CAUSTICS): two scrolling, rotated layers of the
// cellular pattern (bright cell borders), multiplied so only where both ridges cross
// stays bright, giving the shifting light net of sun focused through waves. Fades in
// over the first unit of depth and out with deep water.
fn caustics(floor_pos: vec3<f32>, depth: f32) -> f32 {
    let t = g.params.x;
    let q = floor_pos.xz;
    let r = mat2x2<f32>(0.6, 0.8, -0.8, 0.6);
    let a = textureSampleLevel(detail, wrap, q / 19.0 + vec2(0.031, 0.022) * t, 0.0).a;
    let b = textureSampleLevel(detail, wrap, r * q / 24.3 - vec2(0.019, -0.027) * t, 0.0).a;
    let net = pow(a * b, 1.4) * 4.0;
    let sun_up = smoothstep(0.05, 0.4, -normalize(g.light_dir.xyz).y);
    return net * smoothstep(0.2, 1.5, depth) * exp(-depth * 0.05) * sun_up;
}

// Actors in the water (F_INTERACT). Returns xy: surface slope to add to the normal, z: foam.
// Each ripple is a ring travelling outward from where it was shed, a little wave packet
// that widens and dies over RIPPLE_LIFE; fresh rings carry foam. Every wader also has a
// wobbling foam collar where the water meets the legs, stronger while walking.
fn disturb(p: vec2<f32>, t: f32) -> vec3<f32> {
    var slope = vec2(0.0);
    var foam = 0.0;
    for (var i = 0u; i < min(w.flags.z, 48u); i++) {
        let r = w.ripples[i];
        let age = t - r.z;
        if age < 0.0 || age > RIPPLE_LIFE {
            continue;
        }
        let d = p - r.xy;
        let dist = length(d);
        let life = 1.0 - age / RIPPLE_LIFE;
        let env = r.w * life * life;
        // The splash itself: a burst of churned water at the centre for the first moments.
        let burst = 1.0 - smoothstep(0.0, 0.45, age);
        foam += r.w * burst * (1.0 - smoothstep(0.5 + age * 6.0, 1.2 + age * 7.0, dist)) * 1.3;
        let width = 0.7 + age * 0.8;
        let x = (dist - (1.0 + age * 6.5)) / width;
        if abs(x) > 4.5 {
            continue;
        }
        let bell = exp(-x * x * 0.5);
        // A wave train of crests behind the leading one (sin(2.4x)·bell); d/dr is the ring's
        // slope, pointing away from its centre.
        let dh = (2.4 * cos(2.4 * x) - x * sin(2.4 * x)) * bell / width;
        slope += d / max(dist, 1e-3) * dh * env * 1.1;
        // Foam rides the leading crest and breaks up as the ring spreads.
        let lead = exp(-pow((x - 0.6) * 1.6, 2.0));
        foam += env * lead * (1.0 - smoothstep(0.3, 1.6, age)) * 1.1;
    }
    for (var i = 0u; i < min(w.flags.w, 16u); i++) {
        let a = w.waders[i];
        let d = p - a.xy;
        let dist = length(d);
        if dist > 7.0 {
            continue;
        }
        let ang = atan2(d.y, d.x);
        let edge = 3.4 + 0.3 * sin(ang * 5.0 + t * 2.3) + 0.14 * sin(ang * 3.0 - t * 3.1);
        let x = (dist - edge) / (0.6 + 0.5 * a.w);
        let shallow = smoothstep(0.3, 1.5, a.z);
        foam += exp(-x * x) * (0.8 + 0.5 * a.w) * shallow;
        // Walking churns the water inside the collar too.
        foam += a.w * (1.0 - smoothstep(edge * 0.4, edge, dist)) * 0.7 * shallow;
        // The water piles up against the legs.
        slope += d / max(dist, 1e-3) * -2.0 * x * exp(-x * x) * (0.25 + 0.2 * a.w) * shallow;
    }
    return vec3(slope, foam);
}

// Molten rock (F_LAVA, RSW water types 4 and 6): opaque and lit from within. RO's own lava
// frames creep under a slow domain warp; dark crust plates (the cellular pattern's cells)
// drift on the melt with hot seams between them; a band of cooled crust and a glowing rim
// form where it meets the rock; and the heat swells and ebbs. It runs over the HDR range,
// so bloom makes it glow.
fn lava(in: VsOut, shore_dist: f32, wet: f32, screen_uv: vec2<f32>) -> vec4<f32> {
    let t = g.params.x;
    let p = in.world.xz;
    // The flow: a large, slowly moving warp bends everything below it. Taken from a coarse
    // mip, so only the broad swells of the ripple pattern bend it (no fine streaks).
    let wuv = p / 67.0 + vec2(0.0035, 0.0022) * t;
    let warp = textureSampleLevel(detail, wrap, wuv, 4.0).rg * 2.0 - 1.0;
    let muv = in.tile / 4.0 + warp * 0.10 + vec2(0.005, 0.003) * t;
    let melt = srgb_to_linear(textureSampleGrad(frame_tex, wrap, muv, dpdx(muv), dpdy(muv)).rgb);
    // Heat, 0..1: the melt's own brightness, a slow swell across it, a hot seam between
    // the crust plates, less under the plates (more of them toward the shore, where it has
    // cooled). The colour comes from heat along a glowing-metal ramp, so it never clips to
    // flat red.
    let heat_uv = p / 151.0 - vec2(0.002, 0.0031) * t;
    let swell = textureSampleLevel(detail, wrap, heat_uv, 3.0).b;
    let pulse = 0.88 + 0.12 * sin(t * 0.9 + swell * TAU);
    let lum = dot(melt, vec3(0.55, 0.35, 0.10));
    let c1uv = p / 21.0 + warp * 0.35 + vec2(0.012, 0.007) * t;
    let c2uv = p / 34.0 - warp * 0.25 + vec2(-0.005, 0.009) * t;
    let c1 = textureSampleGrad(detail, wrap, c1uv, dpdx(c1uv), dpdy(c1uv)).a;
    let c2 = textureSampleGrad(detail, wrap, c2uv, dpdx(c2uv), dpdy(c2uv)).a;
    let seam = max(c1, c2 * 0.8);
    let cooled = 1.0 - smoothstep(0.0, 10.0, shore_dist);
    let cover = mix(0.2, 0.8, cooled) * smoothstep(0.3, 0.7, swell);
    let plate = (1.0 - smoothstep(0.1, 0.5, seam)) * cover;
    // RO's molten frames sit at 0.45 (median) to 0.6 in linear light; the crust frames are
    // mostly dark with cracks up to 0.8.
    var heat = pow(clamp(lum / 0.62, 0.0, 1.0), 1.8) * (0.75 + 0.4 * swell) * pulse;
    heat = mix(heat, 0.05 + 0.85 * smoothstep(0.5, 0.95, seam) * pulse, plate);
    // A thin hot line right against the rock, flickering a little.
    heat = max(heat, exp(-shore_dist * 2.2) * (0.7 + 0.2 * sin(t * 2.3 + p.x * 0.31 + p.y * 0.27)));
    // (Tone mapping turns anything much past 1 yellow, so orange stays near it.)
    var color = mix(vec3(0.025, 0.004, 0.002), vec3(0.42, 0.035, 0.004), smoothstep(0.0, 0.4, heat));
    color = mix(color, vec3(0.95, 0.20, 0.015), smoothstep(0.35, 0.75, heat));
    color = mix(color, vec3(1.9, 0.95, 0.28), smoothstep(0.8, 1.0, heat));
    // The crust is rock: it takes the dungeon's light a little.
    color += vec3(0.05, 0.035, 0.03) * plate * (1.0 - heat) * (0.6 + 0.8 * c2) * (g.ambient.rgb + g.diffuse.rgb * 0.5);
    // Feather the edge like the water: barely-covered ground is just ground.
    let edge = smoothstep(0.0, 1.2, wet);
    if on(F_REFRACTION) {
        return vec4(mix(textureSampleLevel(scene_color, clamp_samp, screen_uv, 0.0).rgb, color, edge), 1.0);
    }
    return vec4(color, edge);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let t = g.params.x;
    let p = in.world;
    let to_eye = g.eye.xyz - p;
    let eye_dist = length(to_eye);
    let v = to_eye / eye_dist;
    let screen_uv = in.pos.xy * g.screen.zw;

    // Derivatives up front (uniform control flow) for the gradient samples below.
    let tiling = w.normal.y;
    // Three ripple layers, each rotated and at unrelated scales so their tiling never
    // lines up (no visible repeat), plus a slow macro field that varies the surface.
    let rot_b = mat2x2<f32>(0.799, 0.602, -0.602, 0.799); // 37°
    let rot_c = mat2x2<f32>(0.276, -0.961, 0.961, 0.276); // -74°
    let uv_a = p.xz / tiling + vec2(0.021, 0.013) * t * w.normal.z;
    let uv_b = rot_b * p.xz / (tiling * 0.437) + vec2(-0.017, 0.024) * t * w.normal.z;
    let uv_c = rot_c * p.xz / (tiling * 2.91) + vec2(0.006, -0.009) * t * w.normal.z;
    let uv_m = p.xz / (tiling * 13.7) + vec2(0.0013, 0.0021) * t;
    let da = mat2x2<f32>(dpdx(uv_a), dpdy(uv_a));
    let db = mat2x2<f32>(dpdx(uv_b), dpdy(uv_b));
    let dcc = mat2x2<f32>(dpdx(uv_c), dpdy(uv_c));
    let dm = mat2x2<f32>(dpdx(uv_m), dpdy(uv_m));
    // 0..1 large patches: calm/choppy areas, tint shifts, where the sun glitters.
    let macro_raw = textureSampleGrad(detail, wrap, uv_m, dm[0], dm[1]).b;
    let macro_v = smoothstep(0.2, 0.8, macro_raw);
    let suv = shore_uv(p.xz);
    let ds = mat2x2<f32>(dpdx(suv), dpdy(suv));
    let foam_scale = w.foam.z;
    let uv_f1 = p.xz / foam_scale + vec2(0.03, 0.02) * t * w.foam.y;
    let uv_f2 = p.xz / (foam_scale * 0.71) - vec2(0.025, -0.018) * t * w.foam.y;
    let df1 = mat2x2<f32>(dpdx(uv_f1), dpdy(uv_f1));
    let df2 = mat2x2<f32>(dpdx(uv_f2), dpdy(uv_f2));
    let classic_uv = in.tile / 4.0;
    let dc = mat2x2<f32>(dpdx(classic_uv), dpdy(classic_uv));

    // Surface normal: swell + two scrolling ripple layers, flattened with distance.
    var n = normalize(in.normal);
    if on(F_NORMAL_MAP) {
        let a = filtered(detail, wrap, uv_a, da[0], da[1]).rg * 2.0 - 1.0;
        // Layer b is rotated: bring its slopes back into world space.
        let b = transpose(rot_b) * (filtered(detail, wrap, uv_b, db[0], db[1]).rg * 2.0 - 1.0);
        let c = transpose(rot_c) * (textureSampleGrad(detail, wrap, uv_c, dcc[0], dcc[1]).rg * 2.0 - 1.0);
        let fade = 1.0 / (1.0 + eye_dist / 900.0);
        // Choppy and glassy patches drift across the surface.
        let chop = mix(0.35, 1.35, macro_v);
        let ripple = (a * 0.8 + b * 0.55 + c * 0.7) * w.normal.x * fade * chop;
        n = normalize(vec3(n.x + ripple.x, n.y, n.z + ripple.y));
    }
    // Rings, wakes and foam collars around actors standing in the water.
    var stir = vec3(0.0);
    let stir_pattern = textureSampleGrad(detail, wrap, uv_f1 * 1.7, df1[0] * 1.7, df1[1] * 1.7).a;
    if on(F_INTERACT) {
        stir = disturb(p.xz, t);
        n = normalize(vec3(n.x + stir.x, n.y, n.z + stir.y));
    }
    // Rain: rings spreading where the drops land.
    if g.wet.y > 0.0 {
        let ring = drops(p.xz, g.params.x) * 0.9 * g.wet.y;
        n = normalize(vec3(n.x + ring.x, n.y, n.z + ring.y));
    }

    // Depth: the baked vertical depth everywhere, and the view-ray thickness from the
    // opaque scene where the screen copy is available.
    let shore = filtered(shore_map, clamp_samp, suv, ds[0], ds[1]);
    let vertical = shore.r * DEPTH_RANGE;
    let screen_ok = on(F_REFRACTION) || on(F_SSR);
    var thickness = vertical / max(v.y, 0.15);
    var refr_uv = screen_uv;
    if on(F_REFRACTION) {
        thickness = max(scene_dist(screen_uv) - eye_dist, 0.0);
        // Bend the view by the normal tilt, more where the water is deeper; reject
        // samples that land on things in front of the water.
        let bend = n.xz * w.optics.x * clamp(thickness / 12.0, 0.3, 1.0);
        let cand = clamp(screen_uv + bend, vec2(0.001), vec2(0.999));
        let cand_thick = scene_dist(cand) - eye_dist;
        if cand_thick > 0.0 {
            refr_uv = cand;
            thickness = cand_thick;
        }
    }
    let depth = thickness * w.absorb.w;
    // True vertical water depth at this pixel (scene depth when available), for the waterline.
    var wet = vertical;
    if on(F_REFRACTION) {
        wet = max(scene_dist(screen_uv) - eye_dist, 0.0) * max(v.y, 0.05);
    }

    if on(F_LAVA) {
        return lava(in, shore.g * SHORE_RANGE, wet, screen_uv);
    }

    // Lighting terms.
    let sun_dir = -normalize(g.light_dir.xyz);
    let sun = w.sun.rgb;
    let mean_sky = w.sky.rgb * w.sky.w;
    let ambient = g.ambient.rgb + mean_sky * 0.5;

    // In-scattered colour of the water body (lit by sky + sun), optionally tinted by RO's
    // own animated texture.
    // The water's own colour shifts across the surface: greener in some patches, deeper
    // blue in others, so a bay is never one flat tint.
    let tint = mix(vec3(1.25, 0.85, 0.80), vec3(0.75, 1.15, 1.05), macro_v);
    var body = w.scatter.rgb * tint * (ambient + sun * max(sun_dir.y, 0.0) * 0.6);
    if on(F_CLASSIC) {
        let bent = classic_uv + n.xz * 0.02;
        let classic = srgb_to_linear(filtered(frame_tex, wrap, bent, dc[0], dc[1]).rgb) * (g.ambient.rgb + g.diffuse.rgb * 0.6);
        body = mix(body, classic, w.scatter.w);
    }

    // Beer-Lambert: what survives the trip through `depth` world units of water.
    var transmit = vec3(1.0);
    if on(F_ABSORPTION) {
        // Clarity drifts across the surface: murkier and clearer patches.
        transmit = exp(-w.absorb.rgb * depth * mix(0.55, 1.9, 1.0 - macro_v));
    } else {
        transmit = vec3(exp(-depth * 0.02));
    }

    var color: vec3<f32>;
    var alpha = 1.0;
    if on(F_REFRACTION) {
        let below = textureSampleLevel(scene_color, clamp_samp, refr_uv, 0.0).rgb;
        var floor_light = below;
        if on(F_CAUSTICS) {
            floor_light += below * caustics(p - v * thickness, depth);
        }
        // A little suspended haze even in the shallows, thicker in murky patches.
        let haze = mix(0.05, 0.22, 1.0 - macro_v) * smoothstep(0.0, 3.0, depth);
        color = mix(floor_light * transmit + body * (1.0 - transmit), body, haze);
    } else {
        // No scene copy: blend over the ground, more opaque where deeper.
        color = body;
        alpha = clamp(1.0 - dot(transmit, vec3(0.333)), 0.35, 0.95);
        if !on(F_ABSORPTION) {
            alpha = 0.56;
        }
    }

    // Reflection: Schlick Fresnel over the sky (or the scene, via SSR).
    let ndv = clamp(dot(n, v), 0.0, 1.0);
    let fresnel = (0.02 + 0.98 * pow(1.0 - ndv, 5.0)) * w.optics.y;
    if on(F_REFLECTION) {
        let r = reflect(-v, n);
        var refl = sky(vec3(r.x, max(r.y, 0.02), r.z));
        // Ray traced (outdoors, ray-tracing GPUs): the map's buildings and trees, even off screen.
        let traced = rt_reflect(p + n * 0.5, normalize(vec3(r.x, max(r.y, 0.02), r.z)));
        refl = mix(refl, traced.rgb, traced.a);
        if on(F_SSR) && screen_ok {
            let hit = ssr(p + n * 0.5, r);
            refl = mix(refl, hit.rgb, hit.a);
        }
        color = mix(color, refl, fresnel);
        alpha = max(alpha, fresnel);
    } else {
        // Mean sky radiance stands in for the sky map.
        color = mix(color, mean_sky, fresnel);
    }

    // Sun glitter.
    if on(F_SPECULAR) {
        let h = normalize(sun_dir + v);
        // Glitter gathers in the choppy patches and fades out of the glassy ones.
        let glitter = smoothstep(0.35, 0.9, macro_v);
        let spec = pow(max(dot(n, h), 0.0), w.optics.w) * w.sun.w * smoothstep(0.0, 0.1, sun_dir.y) * glitter;
        color += sun * spec;
        alpha = max(alpha, clamp(spec, 0.0, 1.0));
    }

    // Shore foam: where the water thins out (baked depth, or the true view thickness),
    // broken up by two scrolling foam patterns; a little on wave crests too.
    if on(F_FOAM) {
        let f1 = filtered(detail, wrap, uv_f1, df1[0], df1[1]).a;
        let f2 = filtered(detail, wrap, uv_f2, df2[0], df2[1]).a;
        let pattern = clamp(f1 * 0.65 + f2 * 0.55, 0.0, 1.0);
        // RO seabeds sit only a little under the surface almost everywhere, so foam keys off
        // the distance to the shoreline (baked) rather than depth, plus a thin contact line
        // where the water meets piers and walls (from the scene depth).
        var near_shore = 1.0 - smoothstep(0.0, w.foam.x, shore.g * SHORE_RANGE);
        if on(F_REFRACTION) {
            near_shore = max(near_shore, 1.0 - smoothstep(0.0, 1.6, thickness * max(v.y, 0.15)));
        }
        let lapping = 0.5 + 0.5 * sin(t * 1.7 * w.foam.y + shore.g * SHORE_RANGE * 0.35);
        var foam = smoothstep(1.0 - near_shore, 1.15 - near_shore * 0.5, pattern * (0.75 + 0.25 * lapping));
        if on(F_REFRACTION) {
            // A solid line only where the water truly meets the ground. The baked shore
            // distance is per tile corner, so on a lone tile over barely-submerged sand it
            // reads "shore" everywhere and filled the whole square white.
            // It is thin and broken by the pattern, so a flat, barely-flooded tile does not
            // light up as one pale square either.
            let line = smoothstep(0.0, 0.06, wet) * (1.0 - smoothstep(0.12, 0.45, wet));
            foam = max(foam, line * smoothstep(0.25, 0.6, pattern) * 0.85);
        } else {
            foam = max(foam, smoothstep(0.75, 1.0, near_shore) * 0.85);
        }
        // Crests: only the tallest peaks, as thin streaks (the pattern's brightest lines),
        // thinned further by a slow large-scale mask so open water stays mostly clear.
        let mask = filtered(detail, wrap, p.xz / (foam_scale * 9.0) + vec2(0.004, 0.003) * t, df1[0] / 9.0, df1[1] / 9.0).b;
        let crest = smoothstep(0.6, 0.9, in.crest) * smoothstep(0.6, 0.85, mask);
        foam = max(foam, crest * w.foam.w * smoothstep(0.6, 0.95, pattern));
        foam *= w.foam_color.a;
        let lit = w.foam_color.rgb * (g.ambient.rgb + g.diffuse.rgb * max(sun_dir.y, 0.2));
        color = mix(color, lit, foam);
        alpha = max(alpha, foam);
    }
    if stir.z > 0.0 {
        let foam = clamp(stir.z * (0.45 + 0.75 * stir_pattern), 0.0, 1.0) * w.foam_color.a;
        let lit = w.foam_color.rgb * (g.ambient.rgb + g.diffuse.rgb * max(sun_dir.y, 0.2));
        color = mix(color, lit, foam);
        // Spray: the brightest flecks of churned water flash above white (bloom catches them).
        color += lit * pow(stir_pattern, 3.0) * clamp(stir.z - 0.3, 0.0, 1.0) * 1.8;
        alpha = max(alpha, foam);
    }
    // Ice on snowy maps: a shelf frozen out from the banks, floes drifting on the open water.
    if g.wet.z > 0.0 {
        let ice = ice_cover(p, v, shore.g * SHORE_RANGE, g.wet.z, t, sun_dir);
        color = mix(color, ice.rgb, ice.a);
        alpha = max(alpha, ice.a);
    }
    // Feather the waterline: water a few hundredths of a unit deep is just wet ground, so
    // the surface fades out there instead of ending in a hard tile edge.
    let edge = smoothstep(0.0, 1.2, wet);
    if on(F_REFRACTION) {
        color = mix(textureSampleLevel(scene_color, clamp_samp, screen_uv, 0.0).rgb, color, edge);
    } else if on(F_NORMAL_MAP) {
        // (Classic keeps the original hard-edged tiles.)
        alpha *= edge;
    }
    return vec4(color, alpha);
}

// Ice (snowy maps), `amount` 0 open water .. 1 frozen over. A shelf of ice reaches out from
// the banks, ragged at its edge, wider the colder; past it floes (the cellular pattern's
// cells) drift slowly on the current, more and larger with `amount`, with dark water
// between. The ice is pale blue, dusted with snow in patches, crazed with cracks, its edges
// a little brighter, and gives back a faint sheen of the sky. Colour and coverage (alpha).
fn ice_cover(p: vec3<f32>, v: vec3<f32>, shore_dist: f32, amount: f32, t: f32, sun_dir: vec3<f32>) -> vec4<f32> {
    let drift = vec2(0.21, 0.13) * t;
    // The shelf, from the shore, its edge ragged.
    let juv = p.xz / 31.0;
    let jag = textureSampleGrad(detail, wrap, juv, dpdx(juv), dpdy(juv)).b;
    let reach = mix(3.0, 30.0, amount) * (0.55 + 0.9 * jag);
    let shelf = 1.0 - smoothstep(reach - 1.0, reach + 1.0, shore_dist);
    // Floes: cell interiors, floating with the current; crowded where a slow field says so.
    let fuv = (p.xz + drift) / 115.0;
    let cell = textureSampleGrad(detail, wrap, fuv, dpdx(fuv), dpdy(fuv)).a;
    let size = mix(0.14, 0.6, amount * amount);
    let plate = 1.0 - smoothstep(size - 0.04, size + 0.04, cell);
    let cuv = (p.xz + drift) / 260.0;
    let crowd_n = textureSampleGrad(detail, wrap, cuv, dpdx(cuv), dpdy(cuv)).b;
    let crowd = smoothstep(0.8 - amount * 0.75, 0.92 - amount * 0.75, crowd_n);
    let floe = plate * crowd;
    // Frozen over at the top of the range.
    let cover = max(max(shelf, floe), smoothstep(0.9, 1.0, amount));
    // The surface: the shelf stays put, the floes carry their texture with them.
    let moving = select(0.0, 1.0, floe > shelf);
    // (The dusting is taken blurred: the pattern's fine detail reads as stamped flakes.)
    let tuv = (p.xz + drift * moving) / 37.0;
    let grain = textureSampleGrad(detail, wrap, tuv, dpdx(tuv) * 6.0, dpdy(tuv) * 6.0).b;
    let kuv = (p.xz + drift * moving) / 47.0 + vec2(0.37, 0.71);
    let crack = textureSampleGrad(detail, wrap, kuv, dpdx(kuv), dpdy(kuv)).a;
    var albedo = mix(vec3(0.55, 0.72, 0.84), vec3(0.92, 0.95, 1.0), 0.3 + 0.45 * smoothstep(0.2, 0.95, grain));
    albedo *= 1.0 - 0.25 * smoothstep(0.9, 0.98, crack);
    // Edges catch the light: a thin bright rim round each floe and along the shelf's edge.
    let rim = max(plate * (1.0 - plate), shelf * (1.0 - shelf)) * 4.0 * (1.0 - smoothstep(0.9, 1.0, amount));
    albedo = mix(albedo, vec3(0.97, 0.99, 1.0), clamp(rim, 0.0, 1.0) * 0.6);
    var lit = albedo * (g.ambient.rgb + g.diffuse.rgb * max(sun_dir.y, 0.25));
    // A faint sheen of the sky, and a soft glint of the sun.
    let up = vec3(0.0, 1.0, 0.0);
    let fr = 0.02 + 0.98 * pow(1.0 - clamp(v.y, 0.0, 1.0), 5.0);
    lit += sky(reflect(-v, up)) * fr * 0.35;
    let h = normalize(sun_dir + v);
    lit += w.sun.rgb * pow(max(h.y, 0.0), 90.0) * 0.25 * smoothstep(0.0, 0.1, sun_dir.y);
    return vec4(lit, cover);
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

// RT-STUB: replaced by rt.wgsl where the GPU traces rays.
fn rt_reflect(origin: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    return vec4(0.0);
}
