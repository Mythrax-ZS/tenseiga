// HD-2D post-processing: bloom (bright pass + blur), tilt-shift depth of field, tone
// shoulder, colour grading and vignette. Every pass is a fullscreen triangle.

struct Params {
    // xy: 1 / source size in texels, zw: blur direction (texels)
    texel: vec4<f32>,
    // x: threshold, y: soft knee, z: intensity, w: unused
    bloom: vec4<f32>,
    // x: strength 0..1, y: focus height (0 top .. 1 bottom), z: sharp band half-height, w: falloff
    dof: vec4<f32>,
    // x: exposure, y: contrast, z: saturation, w: warmth (-1 cool .. 1 warm)
    grade: vec4<f32>,
    // x: vignette strength, y: aspect ratio, z: shadow lift, w: sky height offset
    misc: vec4<f32>,
    // x: focus distance, y: depth blur range, z: fog start, w: fog end
    depth: vec4<f32>,
    // rgb: fog colour, a: fog strength
    fog: vec4<f32>,
    // rgb: sky zenith, a: sky gradient exponent (0 disables the outdoor sky path)
    sky: vec4<f32>,
    // xy: sun position (uv), z: ray intensity, w: 1 when the sun is in front
    rays: vec4<f32>,
    // x: AO strength, y: sample radius in pixels at the focus distance
    ao: vec4<f32>,
    // Underwater: x strength (0 off), y seconds, z aspect ratio; w heat haze (0 off)
    water: vec4<f32>,
    // Eye position (world)
    eye: vec4<f32>,
    // Auto exposure: x strength (0 off), y log2 neutral luminance, zw exposure range
    adapt: vec4<f32>,
    inv_vp: mat4x4<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var blurred: texture_2d<f32>;
@group(0) @binding(4) var bloom_tex: texture_2d<f32>;
@group(0) @binding(5) var info: texture_2d<f32>;
@group(0) @binding(6) var ao_tex: texture_2d<f32>;
@group(0) @binding(7) var rays_tex: texture_2d<f32>;
// The eye's adapted log2 luminance (1x1), for auto exposure.
@group(0) @binding(8) var adapted_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    // One triangle covering the screen.
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4(xy * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2(xy.x, 1.0 - xy.y);
    return o;
}

// 4-tap box downsample (bilinear taps cover a 4x4 area).
fn down4(uv: vec2<f32>) -> vec3<f32> {
    let o = p.texel.xy;
    return (textureSample(src, samp, uv + vec2(-o.x, -o.y)).rgb
          + textureSample(src, samp, uv + vec2( o.x, -o.y)).rgb
          + textureSample(src, samp, uv + vec2(-o.x,  o.y)).rgb
          + textureSample(src, samp, uv + vec2( o.x,  o.y)).rgb) * 0.25;
}

@fragment
fn fs_down(in: VsOut) -> @location(0) vec4<f32> {
    return vec4(down4(in.uv), 1.0);
}

// Downsample keeping only what is brighter than the threshold (soft knee).
fn bloom_sample(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSample(src, samp, uv).rgb;
    let depth = textureSample(info, samp, uv).x;
    // A bright cloud is a background, not a light source. Mask each downsample tap
    // before filtering so the sky cannot flood the scene's bloom and god-ray buffers.
    return c * select(1.0, 0.0, p.sky.w > 0.0 && depth >= 50000.0);
}

@fragment
fn fs_bright(in: VsOut) -> @location(0) vec4<f32> {
    let o = p.texel.xy;
    let c = (bloom_sample(in.uv + vec2(-o.x, -o.y))
          + bloom_sample(in.uv + vec2( o.x, -o.y))
          + bloom_sample(in.uv + vec2(-o.x,  o.y))
          + bloom_sample(in.uv + vec2( o.x,  o.y))) * 0.25;
    let l = max(c.r, max(c.g, c.b));
    let knee = p.bloom.x * p.bloom.y + 1e-4;
    let soft = clamp(l - p.bloom.x + knee, 0.0, 2.0 * knee);
    let w = max(soft * soft / (4.0 * knee), l - p.bloom.x) / max(l, 1e-4);
    return vec4(c * w, 1.0);
}

// 9-tap Gaussian along p.texel.zw.
@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let d = p.texel.zw;
    var c = textureSample(src, samp, in.uv).rgb * 0.2270270;
    c += (textureSample(src, samp, in.uv + d * 1.3846153).rgb + textureSample(src, samp, in.uv - d * 1.3846153).rgb) * 0.3162162;
    c += (textureSample(src, samp, in.uv + d * 3.2307692).rgb + textureSample(src, samp, in.uv - d * 3.2307692).rgb) * 0.0702702;
    return vec4(c, 1.0);
}

// Identity below 0.8, then rolls off toward 1 so bloom and bright effects don't clip.
fn shoulder(c: vec3<f32>) -> vec3<f32> {
    let k = 0.8;
    let over = max(c - vec3(k), vec3(0.0));
    return min(c, vec3(k)) + (1.0 - k) * (1.0 - exp(-over / (1.0 - k)));
}

// ---- Underwater (submerged maps) ----

fn hash2(p: vec2<f32>) -> vec2<f32> {
    let q = vec2(dot(p, vec2(127.1, 311.7)), dot(p, vec2(269.5, 183.3)));
    return fract(sin(q) * 43758.5453);
}

// Sunlight focused by the waves above: a few warped iterations whose fine bright lines
// form the shifting net seen on sea floors (after joltz0r / Dave Hoskins' "water caustic").
fn caustic(pos: vec2<f32>, t: f32) -> f32 {
    let p = (pos % vec2(6.2832)) - vec2(250.0);
    var q = p;
    var c = 1.0;
    let inten = 0.005;
    for (var n = 0; n < 4; n++) {
        let tt = t * (1.0 - 3.5 / f32(n + 1));
        q = p + vec2(cos(tt - q.x) + sin(tt + q.y), sin(tt - q.y) + cos(tt + q.x));
        c += 1.0 / length(vec2(p.x / (sin(q.x + tt) / inten), p.y / (cos(q.y + tt) / inten)));
    }
    c /= 4.0;
    c = 1.17 - pow(c, 1.4);
    return clamp(pow(abs(c), 8.0), 0.0, 3.0);
}

// Gradient noise (smooth, no square lattice showing through), 0..1.
fn noise2(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    // Quintic fade: continuous curvature across cell borders.
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let ga = normalize(hash2(i) * 2.0 - 1.0 + vec2(1e-4));
    let gb = normalize(hash2(i + vec2(1.0, 0.0)) * 2.0 - 1.0 + vec2(1e-4));
    let gc = normalize(hash2(i + vec2(0.0, 1.0)) * 2.0 - 1.0 + vec2(1e-4));
    let gd = normalize(hash2(i + vec2(1.0, 1.0)) * 2.0 - 1.0 + vec2(1e-4));
    let a = dot(ga, f);
    let b = dot(gb, f - vec2(1.0, 0.0));
    let c = dot(gc, f - vec2(0.0, 1.0));
    let d = dot(gd, f - vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y) * 0.75 + 0.5;
}

// Direction light travels down through the water (slanted, like a low sun above the waves).
const SHAFT_DIR: vec3<f32> = vec3<f32>(0.32, -1.0, 0.18);

// Light shafts in the world: where the surface above focuses light, a slanted column of
// brighter water runs down along SHAFT_DIR. A point is lit by the patch it traces back to
// (its projection along the light onto the y = 0 plane). The patches drift and swell, so
// shafts sway, brighten and fade over time; being in the world they hold still as the
// camera moves.
fn shaft_density(pos: vec3<f32>, t: f32) -> f32 {
    let q = pos.xz - SHAFT_DIR.xz / SHAFT_DIR.y * pos.y;
    // Each layer is rotated off the world axes, so no grid lines up anywhere.
    let r1 = mat2x2<f32>(0.819, 0.574, -0.574, 0.819);
    let r2 = mat2x2<f32>(0.423, -0.906, 0.906, 0.423);
    let a = noise2(r1 * q / 38.0 + vec2(t * 0.020, t * 0.013));
    let b = noise2(r2 * q / 17.0 - vec2(t * 0.031, -t * 0.017));
    let pulse = noise2(q / 90.0 + vec2(t * 0.05, 0.0));
    return smoothstep(0.55, 0.85, a * 0.65 + b * 0.35) * (0.3 + 1.1 * pulse * pulse);
}

// In-scattered shaft light along the view ray, through a fixed band of water in front of
// the player's floor (from `near` to `near + 200`), cut short only by nearer geometry. What
// lies deeper (chasm walls at any depth) doesn't change it, so it stays seamless.
fn shafts(eye: vec3<f32>, dir: vec3<f32>, near: f32, dist: f32, t: f32) -> f32 {
    let far = min(dist, near + 200.0);
    if far <= near {
        return 0.0;
    }
    let steps = 16;
    var sum = 0.0;
    for (var i = 0; i < steps; i++) {
        let s = eye + dir * (near + (far - near) * (f32(i) + 0.5) / f32(steps));
        sum += shaft_density(s, t);
    }
    return sum / f32(steps) * (far - near) / 200.0;
}

fn hash3(p: vec3<f32>) -> vec4<f32> {
    let q = vec4(dot(p, vec3(127.1, 311.7, 74.7)), dot(p, vec3(269.5, 183.3, 246.1)), dot(p, vec3(113.5, 271.9, 124.6)), dot(p, vec3(63.7, 197.3, 41.9)));
    return fract(sin(q) * 43758.5453);
}

// Marine snow: real points in the water. The view ray walks the cells of a 3D grid that
// sinks slowly; each cell may hold one mote, drawn where the ray passes close to it. Motes
// stay round from any angle, shrink with distance, keep their place as the camera moves,
// and hide behind geometry.
fn marine_snow(eye: vec3<f32>, dir: vec3<f32>, near: f32, far: f32, t: f32) -> f32 {
    let cell = 9.0;
    // The grid sinks and drifts with a slow current.
    let flow = vec3(t * 0.5, t * 1.3, t * 0.3);
    let o = (eye + dir * near + flow) / cell;
    var c = floor(o);
    let stp = sign(dir);
    let inv = 1.0 / max(abs(dir), vec3(1e-5));
    var tmax = (select(o - c, c + 1.0 - o, stp > vec3(0.0))) * inv;
    let span = (far - near) / cell;
    var s = 0.0;
    for (var i = 0; i < 40; i++) {
        let h = hash3(c);
        if h.w > 0.84 {
            let pt = (c + 0.15 + h.xyz * 0.7) * cell - flow;
            let rel = pt - eye;
            let along = dot(rel, dir);
            if along > near && along < far {
                let d = length(rel - dir * along);
                let r = 0.18 + 0.17 * h.x;
                s += smoothstep(r, r * 0.2, d) * (0.35 + 0.65 * h.y);
            }
        }
        let m = min(tmax.x, min(tmax.y, tmax.z));
        if m > span {
            break;
        }
        // Step into the neighbouring cell across the nearest face.
        let mask = vec3<f32>(tmax <= vec3(m));
        c += stp * mask;
        tmax += inv * mask;
    }
    return min(s, 1.5);
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4<f32> {
    let uw = p.water.x;
    let secs = p.water.y;
    // Underwater the whole view wavers a little, as if seen through moving water.
    var uv = in.uv;
    if uw > 0.0 {
        uv += vec2(sin(in.uv.y * 23.0 + secs * 1.6) + sin(in.uv.y * 41.0 - secs * 2.3) * 0.5, cos(in.uv.x * 19.0 + secs * 1.3)) * 0.0018 * uw;
    }
    // Heat haze (deserts and volcanoes by day): the air shimmers, rising, more the further off.
    let heat = p.water.w;
    if heat > 0.0 {
        let far = textureSample(info, samp, in.uv).x;
        let reach = 0.55 + 0.45 * smoothstep(p.depth.x * 0.7, p.depth.x * 1.8, far);
        let q = vec2(in.uv.x * p.water.z * 34.0, in.uv.y * 52.0 + secs * 2.2);
        let wob = vec2(noise2(q) - 0.5, noise2(q * 1.7 + vec2(13.1, secs * 1.1)) - 0.5);
        uv += wob * vec2(0.0042, 0.0068) * heat * reach;
    }
    let inf = textureSample(info, samp, uv);
    let depth = inf.x;
    // Ambient occlusion darkens the world (not sprites, which carry their own shading).
    let ao = textureSample(ao_tex, samp, uv).r;
    var sharp = textureSample(src, samp, uv).rgb * mix(1.0, ao, 1.0 - inf.y);
    // The view ray through this pixel (underwater effects live in the world).
    let ndc = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    let far = p.inv_vp * ndc;
    let dir = normalize(far.xyz / far.w - p.eye.xyz);
    // The void around rooms and chasms has no real distance: it, and anything deeper than
    // the water lets you see, is the same far murk, so the two blend without seams.
    let ray_len = min(depth, p.depth.x * 3.0);
    if uw > 0.0 && depth < 50000.0 && inf.y < 0.5 {
        // Caustics dance over the floor and walls (not sprites): rebuild the world point
        // from the linear depth and light it by a pattern fixed to the world.
        let world = p.eye.xyz + dir * depth;
        let k = caustic(world.xz / 9.0, secs * 0.55) * 0.8 + caustic(world.xz / 5.3 + vec2(1.7, 3.1), secs * 0.4) * 0.35;
        sharp += (sharp * 2.2 + vec3(0.025, 0.06, 0.065)) * vec3(0.6, 1.0, 1.0) * k * uw;
    }
    // Depth of field: the tilt-shift band around the player, combined with real distance
    // from the focal plane so tall foreground models and the far map blur too.
    let band = smoothstep(p.dof.z, p.dof.z + p.dof.w, abs(in.uv.y - p.dof.y));
    let away = smoothstep(0.0, p.depth.y, abs(depth - p.depth.x) - p.depth.x * 0.15);
    let t = max(band * 0.7, away) * p.dof.x;
    let blur_ao = mix(1.0, textureSample(ao_tex, samp, in.uv).r, 1.0 - inf.y);
    var c = mix(sharp, textureSample(blurred, samp, in.uv).rgb * blur_ao, t);
    // Distance haze matches the backdrop's vertical gradient outdoors; interiors keep
    // their map-light fog colour. Both are graded together later in this pass.
    // Nothing drawn there (the void around indoor rooms): leave it dark.
    // (MSAA edges average real depth with the void's 60000, so anything far past the fog
    // range counts as void too.)
    let haze = smoothstep(p.depth.z, p.depth.w, depth) * p.fog.a * select(1.0, 0.0, depth > p.depth.w * 3.0);
    var fog_color = p.fog.rgb;
    if p.sky.w > 0.0 {
        let sky_height = clamp(1.0 - uv.y + p.misc.w, 0.0, 1.0);
        fog_color = mix(fog_color, p.sky.rgb, pow(sky_height, p.sky.w));
    }
    c = mix(c, fog_color, haze);
    c += textureSample(bloom_tex, samp, in.uv).rgb * p.bloom.z;
    c += textureSample(rays_tex, samp, in.uv).rgb * p.rays.z;
    if uw > 0.0 {
        // Water swallows red first, then green, with distance; what's lost is replaced by
        // the deep blue-green the water scatters (Beer-Lambert per channel).
        let d = ray_len;
        let transmit = exp(-vec3(0.0034, 0.0012, 0.0009) * d * uw);
        let deep = vec3(0.008, 0.045, 0.065);
        c = c * transmit + deep * (1.0 - transmit);
        // The abyss: whatever lies well below the floor the player stands on (chasms, and the
        // void around the map) sinks into the deep colour, so drop-offs read as depth rather
        // than showing their stepped tile walls.
        let cfar = p.inv_vp * vec4(0.0, 0.0, 0.5, 1.0);
        let floor_y = p.eye.y + normalize(cfar.xyz / cfar.w - p.eye.xyz).y * p.depth.x;
        let below = floor_y - (p.eye.y + dir.y * ray_len);
        // Saturates within a few units below the floor, so walls at different depths don't
        // each get their own in-between shade.
        let abyss = select(smoothstep(4.0, 30.0, below), 1.0, depth > 50000.0);
        c = mix(c, deep * 0.8, abyss * uw);
        // In the abyss, shafts and motes run on into open water instead of stopping at
        // whichever wall polygon the ray happened to hit.
        let open_len = mix(ray_len, p.depth.x * 3.0, abyss);
        let light = vec3(0.30, 0.62, 0.70);
        c += light * shafts(p.eye.xyz, dir, p.depth.x * 0.6, open_len, secs) * 0.45 * uw;
        c += vec3(0.55, 0.75, 0.8) * marine_snow(p.eye.xyz, dir, p.depth.x * 0.35, min(open_len, p.depth.x * 1.3), secs) * 0.2 * uw;
    }

    c *= p.grade.x * auto_exposure();
    // Warm highlights / cool shadows, like a storybook print.
    let luma = dot(c, vec3(0.2126, 0.7152, 0.0722));
    let warm = vec3(1.0 + 0.08 * p.grade.w, 1.0 + 0.02 * p.grade.w, 1.0 - 0.10 * p.grade.w);
    c *= mix(vec3(1.0), warm, smoothstep(0.05, 0.6, luma));
    c += vec3(0.0, 0.004, 0.012) * p.misc.z * (1.0 - smoothstep(0.0, 0.25, luma));
    // Saturation and contrast (around mid grey).
    let l2 = dot(c, vec3(0.2126, 0.7152, 0.0722));
    c = mix(vec3(l2), c, p.grade.z);
    c = max(vec3(0.0), (c - 0.18) * p.grade.y + 0.18);
    c = shoulder(c);

    // Vignette.
    let v = (in.uv - 0.5) * vec2(p.misc.y, 1.0);
    c *= 1.0 - (p.misc.x + 0.25 * uw) * smoothstep(0.35, 1.05, length(v));
    // Dither by about one display step (in gamma space) so smooth dark gradients, like
    // underwater murk, do not show 8-bit bands.
    let n = hash2(in.pos.xy + vec2(secs * 7.0, 0.0)).x + hash2(in.pos.yx * 1.7).x - 1.0;
    c = pow(max(pow(c, vec3(1.0 / 2.2)) + n / 255.0, vec3(0.0)), vec3(2.2));
    return vec4(c, 1.0);
}

// Ambient occlusion from linear depth alone: neighbours clearly nearer the eye than this
// pixel (walls, ledges, props) occlude it. Sprites neither receive nor cast here.
@fragment
fn fs_ao(in: VsOut) -> @location(0) vec4<f32> {
    let c = textureSample(info, samp, in.uv);
    if c.x > 50000.0 || c.y > 0.5 || p.ao.x <= 0.0 {
        return vec4(1.0);
    }
    // A fixed spiral of 12 taps, sized so the radius shrinks with distance.
    let radius = p.ao.y * p.depth.x / max(c.x, 1.0);
    let range = c.x * 0.05;
    var occ = 0.0;
    for (var i = 0; i < 12; i++) {
        let a = f32(i) * 2.39996;
        let r = radius * (0.25 + 0.75 * f32(i) / 11.0);
        let o = vec2(cos(a), sin(a)) * r * p.texel.xy;
        let s = textureSampleLevel(info, samp, in.uv + o, 0.0);
        let dz = c.x - s.x;
        // Closer by a meaningful amount, but not so far it's a separate object.
        occ += select(0.0, smoothstep(range * 0.1, range, dz) * (1.0 - smoothstep(range * 3.0, range * 6.0, dz)), s.y < 0.5);
    }
    return vec4(vec3(1.0 - p.ao.x * occ / 12.0), 1.0);
}

// God rays: march from each pixel toward the sun through the bloom bright pass.
@fragment
fn fs_rays(in: VsOut) -> @location(0) vec4<f32> {
    if p.rays.w < 0.5 || p.rays.z <= 0.0 {
        return vec4(0.0);
    }
    let steps = 24;
    let delta = (p.rays.xy - in.uv) / f32(steps) * 0.85;
    // Each pixel starts its march a different fraction of a step along (interleaved
    // gradient noise). Without it a small bright spot (an aura, a spell) is caught at the
    // same 24 offsets by a line of pixels and shows as a trail of separate copies.
    let jitter = fract(52.9829189 * fract(dot(in.pos.xy, vec2(0.06711056, 0.00583715))));
    var uv = in.uv + delta * (jitter - 1.0);
    var decay = 1.0;
    var sum = vec3(0.0);
    for (var i = 0; i < steps; i++) {
        uv += delta;
        sum += textureSample(src, samp, clamp(uv, vec2(0.0), vec2(1.0))).rgb * decay;
        decay *= 0.93;
    }
    return vec4(sum / f32(steps), 1.0);
}

// Auto exposure. The meter: the scene's log2 luminance, four taps per texel of a 64x64
// target (the sky counts as neutral, so looking up at clouds doesn't darken the town; black
// counts as merely dark, so the void around a dungeon doesn't blow the exposure up).
@fragment
fn fs_lum(in: VsOut) -> @location(0) vec4<f32> {
    let o = p.texel.xy;
    var sum = 0.0;
    for (var i = 0; i < 4; i++) {
        let uv = in.uv + vec2(select(-o.x, o.x, (i & 1) == 1), select(-o.y, o.y, i >= 2));
        let c = textureSample(src, samp, uv).rgb;
        let sky = p.sky.w > 0.0 && textureSample(info, samp, uv).x >= 50000.0;
        sum += select(log2(max(dot(c, vec3(0.2126, 0.7152, 0.0722)), 0.01)), p.adapt.y, sky);
    }
    return vec4(sum * 0.25, 0.0, 0.0, 1.0);
}

// The eye: eased from what it was (`blurred`) toward the metered scene (`src`), slower when
// the scene went dark (texel.x) than when it went bright (texel.y); texel.z 1 snaps.
@fragment
fn fs_adapt(in: VsOut) -> @location(0) vec4<f32> {
    let now = textureSample(src, samp, vec2(0.5)).r;
    let before = textureSample(blurred, samp, vec2(0.5)).r;
    let rate = select(p.texel.x, p.texel.y, now > before);
    return vec4(select(mix(before, now, rate), now, p.texel.z > 0.5), 0.0, 0.0, 1.0);
}

// The exposure the eye's state asks for: toward neutral by the strength, within range.
fn auto_exposure() -> f32 {
    if p.adapt.x <= 0.0 {
        return 1.0;
    }
    let eye = textureSample(adapted_tex, samp, vec2(0.5)).r;
    return clamp(exp2((p.adapt.y - eye) * p.adapt.x), p.adapt.z, p.adapt.w);
}
