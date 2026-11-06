// Map particle emitters (RSW EF_EMITTER). Straight (non-premultiplied) colour and alpha,
// so each pipeline's blend factors (the map's Direct3D source/destination modes) apply as
// the original client used them.

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
    // x: 1 when indoors (room mask active), y/z: mask size in cells, w: world units per cell
    room: vec4<f32>,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(3) var room_mask: texture_2d<f32>;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world: vec3<f32>,
};

@vertex
fn vs_main(@location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = g.view_proj * vec4(pos, 1.0);
    o.uv = uv;
    o.color = color;
    o.world = pos;
    return o;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3(2.4)), c / 12.92, c <= vec3(0.04045));
}

fn in_room(world: vec3<f32>) -> bool {
    if g.room.x < 0.5 {
        return true;
    }
    let cell = vec2<i32>(floor(world.xz / g.room.w));
    if any(cell < vec2(0)) || cell.x >= i32(g.room.y) || cell.y >= i32(g.room.z) {
        return false;
    }
    return textureLoad(room_mask, cell, 0).r > 0.5;
}

// The particle's texel, smoothly magnified. RO's particle textures are small (sparks are
// single bright texels on black); drawn many screen pixels to a texel, bilinear filtering
// spreads each into a soft square. A B-spline (bicubic) filter rounds them into soft dots,
// as they read at the original's resolution. Minified, the usual mipmapped sample.
fn particle_texel(uv: vec2<f32>) -> vec4<f32> {
    let plain = textureSample(tex, samp, uv);
    let size = vec2<f32>(textureDimensions(tex));
    let texels = max(length(dpdx(uv * size)), length(dpdy(uv * size)));
    let p = uv * size - 0.5;
    let f = fract(p);
    let i = p - f;
    let wx = bspline(f.x);
    let wy = bspline(f.y);
    let c = vec4(i.x - 0.5, i.x + 1.5, i.y - 0.5, i.y + 1.5);
    let s = vec4(wx.x + wx.y, wx.z + wx.w, wy.x + wy.y, wy.z + wy.w);
    let o = (c + vec4(wx.y, wx.w, wy.y, wy.w) / s) / vec4(size.x, size.x, size.y, size.y);
    let a = textureSampleLevel(tex, samp, vec2(o.x, o.z), 0.0);
    let b = textureSampleLevel(tex, samp, vec2(o.y, o.z), 0.0);
    let cc = textureSampleLevel(tex, samp, vec2(o.x, o.w), 0.0);
    let d = textureSampleLevel(tex, samp, vec2(o.y, o.w), 0.0);
    let sx = s.x / (s.x + s.y);
    let sy = s.z / (s.z + s.w);
    let soft = mix(mix(d, cc, sx), mix(b, a, sx), sy);
    return mix(plain, soft, 1.0 - smoothstep(0.7, 1.2, texels));
}

// Cubic B-spline weights for the four texels around `f` (0..1 between the middle two).
fn bspline(f: f32) -> vec4<f32> {
    let n = vec4(1.0, 2.0, 3.0, 4.0) - f;
    let s = n * n * n;
    let x = s.x;
    let y = s.y - 4.0 * s.x;
    let z = s.z - 4.0 * s.y + 6.0 * s.x;
    return vec4(x, y, z, 6.0 - x - y - z) / 6.0;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if !in_room(in.world) {
        discard;
    }
    let t = particle_texel(in.uv);
    return vec4(srgb_to_linear(t.rgb * in.color.rgb), t.a * in.color.a);
}

// Additive modes (destination factor One): the original adds in gamma space, where a faint
// colour still shows; converted to linear first it would all but vanish. Adding the gamma
// value as-is lands close to the original's result for the small values these use.
@fragment
fn fs_add(in: VsOut) -> @location(0) vec4<f32> {
    if !in_room(in.world) {
        discard;
    }
    let t = particle_texel(in.uv);
    return vec4(t.rgb * in.color.rgb, t.a * in.color.a);
}
