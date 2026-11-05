// Weather: snow, rain, petals, leaves, fog banks and clouds. Nothing is simulated on the CPU:
// each particle's place follows from its index and the time, wrapped into a box around the
// camera's focus. The box moves with the camera but the particles keep still in the world,
// so walking through snow passes between the flakes instead of dragging them along.

struct Globals {
    view_proj: mat4x4<f32>,
    focus: vec4<f32>,
    eye: vec4<f32>,
    light_dir: vec4<f32>,
    ambient: vec4<f32>,
    diffuse: vec4<f32>,
    params: vec4<f32>,
    light_vp: mat4x4<f32>,
    shadow: vec4<f32>,
    screen: vec4<f32>,
    sprite_light: vec4<f32>,
    lights: array<vec4<f32>, 64>,
    room: vec4<f32>,
};

// One layer (see weather.rs, Layer): world units throughout.
struct Layer {
    // count, look (0 billboard, 1 streak, 2 flutter, 3 flat), size, strength (0..1 thins it)
    a: vec4<f32>,
    // box half width, height, half depth; its floor's height
    box_: vec4<f32>,
    // fall speed, sway amplitude, sway frequency, spin (turns per second)
    motion: vec4<f32>,
    // wind x, wind z, streak length, opacity
    wind: vec4<f32>,
    // colour, how much the map's light shades it (0..1)
    tint: vec4<f32>,
    // focus x, z, frames across the texture, size spread (0..1)
    focus: vec4<f32>,
};

const MAX_LAYERS: u32 = 8u;
// Instances per layer: a layer's are `index * PER_LAYER ..`.
const PER_LAYER: u32 = 16384u;

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(2) @binding(0) var<uniform> layers: array<Layer, MAX_LAYERS>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

fn pcg(n: u32) -> u32 {
    var h = n * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return (h >> 22u) ^ h;
}

// A steady random number in 0..1 for particle `i`, stream `k`.
fn rnd(i: u32, k: u32) -> f32 {
    return f32(pcg(i * 16u + k) & 0xffffffu) / 16777216.0;
}

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    var o: VsOut;
    let l = layers[ii / PER_LAYER];
    let i = ii % PER_LAYER;
    let corner = array<vec2<f32>, 6>(vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, -1.0), vec2(1.0, 1.0), vec2(-1.0, 1.0))[vi];
    // Thinned out (a shower starting, a layer fading): this particle sits it out.
    if f32(i) >= l.a.x || rnd(i, 7u) > l.a.w {
        o.pos = vec4(0.0, 0.0, 2.0, 1.0);
        return o;
    }
    let t = g.params.x;
    let size = vec3(l.box_.x * 2.0, l.box_.y, l.box_.z * 2.0);
    let origin = vec3(l.focus.x, l.box_.w, l.focus.y) - vec3(l.box_.x, 0.0, l.box_.z);
    // Each particle falls at its own pace and sways on its own beat.
    let pace = 0.7 + 0.6 * rnd(i, 3u);
    let vel = vec3(l.wind.x, -l.motion.x * pace, l.wind.y);
    let phase = rnd(i, 4u) * 6.2832;
    let sway = l.motion.y * vec3(sin(t * l.motion.z + phase), 0.0, cos(t * l.motion.z * 0.8 + phase * 1.7));
    let q = vec3(rnd(i, 0u), rnd(i, 1u), rnd(i, 2u)) * size + vel * t;
    let local = fract((q - origin) / size) * size;
    let p = origin + local + sway;
    // Faded where the box wraps (its sides, its top and floor) and right by the eye.
    let edge = max(abs(local.x / size.x - 0.5), abs(local.z / size.z - 0.5)) * 2.0;
    let h = local.y / size.y;
    var fade = (1.0 - smoothstep(0.7, 1.0, edge)) * smoothstep(0.0, 0.08, h) * (1.0 - smoothstep(0.85, 1.0, h));
    let s = l.a.z * (1.0 - l.focus.w * 0.5 + l.focus.w * rnd(i, 5u));
    let to_eye = g.eye.xyz - p;
    let near = length(to_eye);
    // Gone before it fills the screen.
    fade *= smoothstep(0.75 * s, 2.0 * s, near);
    let view = to_eye / max(near, 0.001);
    var right = normalize(cross(view, vec3(0.0, 1.0, 0.0)));
    var up = cross(right, view);
    var offset: vec3<f32>;
    let look = u32(l.a.y + 0.5);
    if look == 1u {
        // Rain: a thin streak along its fall.
        let along = normalize(vel);
        let side = normalize(cross(along, view));
        offset = side * corner.x * s + along * corner.y * l.wind.z;
    } else if look == 2u {
        // Petals and leaves: turning, and tumbling (narrowing as they turn edge-on).
        let a = t * l.motion.w * 6.2832 * (0.6 + 0.8 * rnd(i, 6u)) + phase;
        let tumble = 0.25 + 0.75 * abs(cos(t * l.motion.w * 4.0 + phase * 2.3));
        let c = vec2(corner.x * tumble, corner.y);
        let r = vec2(c.x * cos(a) - c.y * sin(a), c.x * sin(a) + c.y * cos(a));
        offset = (right * r.x + up * r.y) * s;
    } else if look == 3u {
        // Fog and clouds: lying flat, each turned its own way.
        let a = phase;
        let r = vec2(corner.x * cos(a) - corner.y * sin(a), corner.x * sin(a) + corner.y * cos(a));
        offset = vec3(r.x, 0.0, r.y) * s;
    } else {
        offset = (right * corner.x + up * corner.y) * s;
    }
    o.pos = g.view_proj * vec4(p + offset, 1.0);
    let frames = max(l.focus.z, 1.0);
    let frame = floor(rnd(i, 8u) * frames);
    o.uv = vec2((frame + corner.x * 0.5 + 0.5) / frames, 0.5 - corner.y * 0.5);
    // Lit like the map: dim at night and in dark places, never black.
    let light = clamp(g.ambient.rgb + g.diffuse.rgb * 0.6, vec3(0.25), vec3(1.3));
    o.color = vec4(l.tint.rgb * mix(vec3(1.0), light, l.tint.w), l.wind.w * fade);
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Indoors (a room mask is on): no weather under the roof.
    if g.room.x > 0.5 {
        discard;
    }
    let t = textureSample(tex, samp, in.uv);
    let a = t.a * in.color.a;
    if a < 0.004 {
        discard;
    }
    return vec4(srgb_to_linear(t.rgb) * in.color.rgb, a);
}
