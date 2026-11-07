// Additive effect geometry (warp portals, light columns, ground circles), tinted per vertex.

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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    if !in_room(in.world) {
        discard;
    }
    let t = textureSample(tex, samp, in.uv);
    if SOLID {
        // Alpha-blended (premultiplied) and textured: the texture's own colours tinted by the
        // vertex colour, like a model's (rock, ice, a ring drawn in colour).
        let a = t.a * in.color.a;
        return vec4(srgb_to_linear(t.rgb) * in.color.rgb * a, a);
    }
    if SHADE {
        // Alpha-blended (premultiplied): the vertex colour itself, shaped by the texture's
        // alpha. Paints true colour (crimson stays crimson) and darkens pale ground, where
        // added light would only wash out.
        let a = t.a * in.color.a;
        return vec4(srgb_to_linear(in.color.rgb) * a, a);
    }
    // Additive: brightness carries the effect; alpha only scales it.
    return vec4(srgb_to_linear(t.rgb) * in.color.rgb * t.a * in.color.a, 1.0);
}

// The renderer builds this shader three times: additive, with SHADE set for the shade
// pipeline, and with SOLID set for the solid one.
const SHADE: bool = false;
const SOLID: bool = false;
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
