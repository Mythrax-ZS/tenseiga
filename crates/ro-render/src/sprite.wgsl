// Screen-aligned sprite layers. Offsets are added in clip space around the actor's feet,
// so sprites keep their pixel-art proportions, scale with perspective, and share the
// feet's depth (they never cut into the ground or sort per-layer).
//
// HD-2D lighting: sprites take the map's light at their feet (ambient + sun, darkened by
// the sun shadow map), nearby point lights, and a sun-coloured rim on the edges that face
// the sun, so the flat pixel art sits in the lit diorama instead of floating on it.

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    // x: seconds, y/z: clip units per sprite pixel (x, y) at w = 1, w: 1 to snap to pixels
    params: vec4<f32>,
    light_vp: mat4x4<f32>,
    shadow: vec4<f32>,
    // w, h, 1/w, 1/h
    screen: vec4<f32>,
    // x: sprite lighting, y: rim, z: point light intensity, w: light count
    sprite_light: vec4<f32>,
    lights: array<vec4<f32>, 64>,
    // x: 1 when indoors (room mask active), y/z: mask size in cells, w: world units per cell
    room: vec4<f32>,
    // x: water level (y-up), y: 1 when sprites sink into water, z: world units per sprite pixel
    water: vec4<f32>,
    // rgb: the water's scatter colour
    water_tint: vec4<f32>,
    upscale: vec4<f32>,
    // xyz: direction the sun's light travels, w: ray tracing on (bits: 1 shadows, 2 reflections)
    rt: vec4<f32>,
    // (Torches, used by the scene.)
    torch_vp: array<mat4x4<f32>, 2>,
    torch: array<vec4<f32>, 2>,
    torch_k: vec4<f32>,
    // Cloud shadows (cloud_shade).
    clouds: vec4<f32>,
    // (Wet ground, used by the scene and water.)
    wet: vec4<f32>,
    wet_sky: vec4<f32>,
};

/// World units (about two cells) a sprite's depth is pulled toward the camera.
const DEPTH_PULL: f32 = 10.0;
/// Deepest the waterline climbs up a wading sprite (world units, about knee height).
const WADE_DEPTH: f32 = 2.6;

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_samp: sampler_comparison;
@group(0) @binding(3) var room_mask: texture_2d<f32>;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsIn {
    @location(0) anchor: vec3<f32>,
    @location(1) offset: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) @interpolate(flat) anchor: vec3<f32>,
    // Screen direction toward the sun (for the rim light).
    @location(3) @interpolate(flat) sun_dir: vec2<f32>,
    // World units above the feet (for the waterline).
    @location(4) height: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    return sprite_vertex(v, DEPTH_PULL);
}

// A sprite corner, its depth taken from a point `pull` world units toward the camera.
fn sprite_vertex(v: VsIn, pull: f32) -> VsOut {
    var clip = g.view_proj * vec4(v.anchor, 1.0);
    // Pixel stability: put the anchor on a whole screen pixel so sprites don't shimmer as
    // the camera moves.
    if g.params.w > 0.5 {
        let px = (clip.xy / clip.w * 0.5 + 0.5) * g.screen.xy;
        clip = vec4((floor(px) + 0.5) * g.screen.zw * 2.0 - 1.0, clip.z / clip.w, 1.0) * clip.w;
    }
    clip.x += v.offset.x * g.params.y;
    clip.y -= v.offset.y * g.params.z;
    // Depth comes from a point pulled toward the camera, so the shadow and feet are not
    // hidden by the ground just in front of where the actor stands.
    let toward_eye = normalize(g.eye.xyz - v.anchor) * pull;
    let d = g.view_proj * vec4(v.anchor + toward_eye, 1.0);
    clip.z = d.z / d.w * clip.w;
    let a = g.view_proj * vec4(v.anchor, 1.0);
    let s = g.view_proj * vec4(v.anchor - g.light_dir.xyz * 20.0, 1.0);
    var o: VsOut;
    o.pos = clip;
    o.uv = v.uv;
    o.color = v.color;
    o.anchor = v.anchor;
    o.sun_dir = normalize(s.xy / s.w - a.xy / a.w + vec2(1e-5, 0.0)) * vec2(1.0, -1.0);
    o.height = -v.offset.y * g.water.z;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

/// Sun visibility at a world point from the sprite shadow map (1 lit .. 0 shadowed).
fn sun_visible(world: vec3<f32>) -> f32 {
    let lp = g.light_vp * vec4(world, 1.0);
    let ndc = lp.xyz / lp.w;
    let uv = vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return 1.0;
    }
    var lit = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            lit += textureSampleCompareLevel(shadow_map, shadow_samp, uv + vec2(f32(x), f32(y)) * g.shadow.y * 2.0, ndc.z - 0.002);
        }
    }
    return lit / 9.0;
}

/// Map light-source contribution at a point; w: the flicker of flames near it (see scene.wgsl).
fn point_light(world: vec3<f32>) -> vec4<f32> {
    var sum = vec3(0.0);
    var flicker = 0.0;
    let count = i32(g.sprite_light.w);
    for (var i = 0; i < count; i++) {
        let l = g.lights[i * 2];
        let f = saturate(1.0 - distance(l.xyz, world) / max(l.w, 1.0));
        sum += g.lights[i * 2 + 1].rgb * f * f;
        flicker += g.lights[i * 2 + 1].w * f * f;
    }
    return vec4(sum, clamp(flicker * 0.6, -0.3, 0.3)) * g.sprite_light.z;
}

struct FsOut {
    @location(0) color: vec4<f32>,
    // x: distance from the eye, y: 1 (sprite)
    @location(1) info: vec4<f32>,
};

@fragment
fn fs_main(in: VsOut) -> FsOut {
    if !in_room(in.anchor) {
        discard;
    }
    // Premultiplied texture: tint colour, scale everything by the layer alpha.
    let s = textureSample(tex, samp, in.uv);
    // Rim: texels whose neighbour toward the sun is transparent are lit edges.
    let texel = 1.0 / vec2<f32>(textureDimensions(tex));
    let edge = s.a * (1.0 - textureSample(tex, samp, in.uv + in.sun_dir * texel * 1.5).a);
    let t = vec4(s.rgb * in.color.rgb * in.color.a, s.a * in.color.a);
    if t.a < 0.02 {
        discard;
    }
    var rgb = srgb_to_linear(t.rgb / t.a);

    let k = g.sprite_light.x;
    if k > 0.0 {
        // The ground light at the feet relative to full sun, so sprites in the open keep
        // their original colours and darken in shadow. The sample point sits a little
        // toward the sun so a sprite does not shade itself.
        let up = max(dot(vec3(0.0, 1.0, 0.0), -g.light_dir.xyz), 0.0);
        let full = g.ambient.rgb + g.diffuse.rgb * up;
        let feet = in.anchor - normalize(vec3(g.light_dir.x, 0.0, g.light_dir.z) + vec3(1e-4, 0.0, 0.0)) * 6.0 + vec3(0.0, 1.0, 0.0);
        // Buildings and trees shade sprites too where shadows are ray traced (from the feet,
        // like the ground the sprite stands on).
        let geometry = sun_ray(in.anchor + vec3(0.0, 1.0, 0.0), vec3(0.0, 1.0, 0.0), 1u);
        let here = g.ambient.rgb + g.diffuse.rgb * up * mix(1.0, min(sun_visible(feet), geometry), g.shadow.x) * cloud_shade(in.anchor);
        let shade = here / max(full, vec3(0.05));
        let pl = point_light(in.anchor + vec3(0.0, 8.0, 0.0));
        rgb = rgb * mix(vec3(1.0), shade, k) * (1.0 + pl.w * k) + rgb * pl.rgb * k;
        rgb += srgb_to_linear(g.diffuse.rgb) * edge * g.sprite_light.y * up;
    }
    // Wading: below the waterline the sprite is seen through the water, tinted and faded
    // with depth, with a bright wet line where it breaks the surface. RO seabeds are often
    // far below the surface (actors walk them as if dry), so the line is capped at shin
    // height: actors wade rather than vanish.
    var alpha = t.a;
    let depth = g.water.x - in.anchor.y;
    if g.water.y > 0.5 && depth > 0.3 {
        let wobble = 0.2 * sin(in.pos.x * 0.09 + g.params.x * 3.1) + 0.12 * sin(in.pos.x * 0.21 - g.params.x * 4.3);
        let line = min(depth, WADE_DEPTH) + wobble;
        let under = 1.0 - smoothstep(line - 0.2, line + 0.2, in.height);
        let sunk = saturate((line - in.height) / 6.0);
        let water = g.water_tint.rgb * (g.ambient.rgb + g.diffuse.rgb) * 4.0;
        rgb = mix(rgb, mix(rgb * vec3(0.55, 0.8, 0.85), water, 0.3 + 0.5 * sunk), under);
        alpha *= 1.0 - under * (0.3 + 0.45 * sunk);
        let wet = exp(-pow((in.height - line) / 0.35, 2.0));
        rgb += vec3(0.6, 0.7, 0.75) * (g.ambient.rgb + g.diffuse.rgb) * wet * 0.5;
    }
    return FsOut(vec4(rgb * alpha, alpha), vec4(distance(in.anchor, g.eye.xyz), 1.0, 0.0, 0.0));
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

// Silhouettes: an actor hidden behind a wall or building shows through it as a soft shape
// of its colour (the vertex colour). Drawn where the scene is nearer than the sprite: its
// depth is pulled 3 cells further toward the camera than a sprite's, so only something
// truly in front reveals it (not the ground at its feet).
@vertex
fn vs_silhouette(v: VsIn) -> VsOut {
    return sprite_vertex(v, DEPTH_PULL + 15.0);
}

@fragment
fn fs_silhouette(in: VsOut) -> @location(0) vec4<f32> {
    if !in_room(in.anchor) {
        discard;
    }
    let a = textureSample(tex, samp, in.uv).a * in.color.a;
    if a < 0.3 {
        discard;
    }
    // Brighter toward the shape's edge, as if lit from behind.
    let texel = 1.0 / vec2<f32>(textureDimensions(tex));
    let edge = 1.0 - min(min(textureSample(tex, samp, in.uv + vec2(texel.x * 2.0, 0.0)).a, textureSample(tex, samp, in.uv - vec2(texel.x * 2.0, 0.0)).a), min(textureSample(tex, samp, in.uv + vec2(0.0, texel.y * 2.0)).a, textureSample(tex, samp, in.uv - vec2(0.0, texel.y * 2.0)).a));
    let k = 0.38 + 0.5 * edge;
    let c = srgb_to_linear(in.color.rgb);
    return vec4(c * k, k);
}

// RT-STUB: replaced by rt.wgsl where the GPU traces rays.
fn sun_ray(world: vec3<f32>, n: vec3<f32>, rays: u32) -> f32 {
    return 1.0;
}

