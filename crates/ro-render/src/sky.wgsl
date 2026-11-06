// A painted sky panorama for RO's tilted diorama camera. The real camera looks down,
// so a physical horizon would be above the screen: we deliberately show a broader
// band of sky behind the map instead. Rotation changes the vista; walking and zooming
// do not. All colour inputs and outputs are linear.

struct Sky {
    zenith: vec4<f32>,
    horizon: vec4<f32>,
    cloud: vec4<f32>,
    // Cloud cover, scale, wind speed, haze.
    clouds: vec4<f32>,
    // Seconds, night blend, star strength, map seed.
    time: vec4<f32>,
    // Camera yaw, pitch, aspect, vertical field of view.
    camera: vec4<f32>,
    sun: vec4<f32>,
};

@group(0) @binding(0) var<uniform> s: Sky;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let xy = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var out: VsOut;
    out.pos = vec4(xy * 2.0 - 1.0, 1.0, 1.0);
    out.uv = vec2(xy.x, 1.0 - xy.y);
    return out;
}

// Arithmetic hashes avoid the precision-dependent streaks of sin-based noise.
fn hash(p: vec3<f32>) -> f32 {
    var q = fract(p * 0.1031);
    q += dot(q, q.yzx + vec3(33.33));
    return fract((q.x + q.y) * q.z);
}

fn noise(p: vec3<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let t = f * f * (3.0 - 2.0 * f);
    let z0 = mix(
        mix(hash(cell), hash(cell + vec3(1.0, 0.0, 0.0)), t.x),
        mix(hash(cell + vec3(0.0, 1.0, 0.0)), hash(cell + vec3(1.0, 1.0, 0.0)), t.x), t.y);
    let z1 = mix(
        mix(hash(cell + vec3(0.0, 0.0, 1.0)), hash(cell + vec3(1.0, 0.0, 1.0)), t.x),
        mix(hash(cell + vec3(0.0, 1.0, 1.0)), hash(cell + vec3(1.0, 1.0, 1.0)), t.x), t.y);
    return mix(z0, z1, t.z);
}

// A lattice point's gradient direction (one hash).
fn grad(q: vec2<f32>) -> vec2<f32> {
    let a = hash(vec3(q, 3.7)) * 6.2831853;
    return vec2(cos(a), sin(a));
}

// Gradient noise for the clouds: smooth, without value noise's blocky cells. About -0.7..0.7.
fn gnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(grad(i), f);
    let b = dot(grad(i + vec2(1.0, 0.0)), f - vec2(1.0, 0.0));
    let c = dot(grad(i + vec2(0.0, 1.0)), f - vec2(0.0, 1.0));
    let d = dot(grad(i + vec2(1.0, 1.0)), f - vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// `octaves` of gradient noise, each turned and doubled (no grid lines); `detail` (0..1)
// fades out all past the third. About -0.5..0.5.
fn gfbm(p: vec2<f32>, octaves: i32, detail: f32) -> f32 {
    let turn = mat2x2<f32>(0.8, 0.6, -0.6, 0.8);
    var q = p;
    var sum = 0.0;
    var amp = 0.5;
    for (var o = 0; o < octaves; o++) {
        sum += gnoise(q) * amp * select(1.0, detail, o >= 3);
        q = turn * q * 2.02 + vec2(3.1, 1.7);
        amp *= 0.5;
    }
    return sum;
}


fn fbm(p: vec3<f32>) -> f32 {
    return noise(p) * 0.5
        + noise(p * 2.02 + vec3(5.3, 1.7, 9.2)) * 0.25
        + noise(p * 4.07 + vec3(2.9, 8.1, 4.4)) * 0.125
        + noise(p * 8.13 + vec3(7.7, 3.6, 1.2)) * 0.0625;
}

// One layer of stars on the bearing / elevation grid, `cells` to a full turn, a star in
// `chance` of them (`mask` thins them per pixel). Blue-white to amber, twinkling; `glow`
// adds a soft halo and a faint four-point cross for the brightest.
fn star_layer(bearing: f32, elevation: f32, cells: f32, chance: f32, mask: f32, size: f32, bright: f32, glow: f32, layer: f32, t: f32) -> vec3<f32> {
    let p = vec2(bearing, elevation) * (cells / 6.2831853);
    // Derivatives first, while control flow is uniform.
    let aa = max(length(fwidth(p)) * 0.5, 0.004);
    let c = floor(p);
    let id = vec3(c.x - floor(c.x / cells) * cells, c.y, layer);
    let h = hash(id);
    let offset = vec2(hash(id + vec3(17.0)), hash(id + vec3(39.0))) * 0.6 + 0.2;
    let d = fract(p) - offset;
    let r = size * (0.5 + hash(id + vec3(71.0)));
    let core = 1.0 - smoothstep(r, r + aa, length(d));
    let halo = glow * (exp(-dot(d, d) / (r * r * 9.0)) * 0.35
        + exp(-abs(d.x) / (r * 0.25)) * exp(-abs(d.y) / (r * 5.0)) * 0.25
        + exp(-abs(d.y) / (r * 0.25)) * exp(-abs(d.x) / (r * 5.0)) * 0.25);
    let warmth = hash(id + vec3(91.0));
    let tint = mix(vec3(0.62, 0.74, 1.0), vec3(1.0, 0.82, 0.58), warmth * warmth);
    let twinkle = 0.72 + 0.28 * sin(t * (0.8 + hash(id + vec3(53.0)) * 2.4) + h * 118.0);
    let lit = step(1.0 - chance * mask, h);
    return tint * (core + halo) * bright * (0.4 + 0.6 * hash(id + vec3(29.0))) * twinkle * lit;
}

// Open space: the Milky Way arching across, its bright core, dark dust lanes, coloured
// nebulae and several layers of stars. `s.cloud` tints the nebulae.
fn galaxy(direction: vec3<f32>, bearing: f32, elevation: f32, height: f32) -> vec3<f32> {
    let t = s.time.x;
    var color = mix(s.horizon.rgb, s.zenith.rgb, pow(height, 0.6));

    // Galactic frame: the core sits ahead at the default camera, 30 degrees up, and the
    // band climbs from lower left to upper right through it.
    let core_dir = normalize(vec3(0.0, 0.5, 0.87));
    let pole = normalize(vec3(-0.52, 0.74, -0.425));
    let across = cross(pole, core_dir);
    let lat = dot(direction, pole);
    let lon = atan2(dot(direction, across), dot(direction, core_dir));
    let band = exp(-pow(lat / 0.17, 2.0));
    let bulge = exp(-pow(lon / 0.45, 2.0)) * exp(-pow(lat / 0.11, 2.0));

    let q = direction * 4.0;
    let warp = vec3(fbm(q + vec3(3.1)), fbm(q + vec3(8.7, 1.3, 5.9)), 0.0);
    let clumps = fbm(direction * 7.0 + warp * 1.6);
    let lanes = smoothstep(0.4, 0.62, fbm(direction * 11.0 + warp * 2.2)) * exp(-pow((lat - 0.015) / 0.08, 2.0));
    let arm = mix(vec3(0.26, 0.34, 0.80), vec3(0.75, 0.70, 0.92), clumps);
    let glowing = arm * band * (0.08 + 0.42 * clumps * clumps) + vec3(1.0, 0.62, 0.36) * bulge * (0.25 + 0.35 * clumps);
    color += glowing * (1.0 - 0.92 * lanes);

    // Nebulae: warped clouds, thickest near the band, magenta and violet with teal veils.
    let n1 = fbm(q * 0.8 + warp * 2.5 + vec3(11.0));
    let n2 = fbm(q * 1.6 + vec3(4.0, 9.0, 2.0));
    let wisps = 0.45 + 0.55 * fbm(direction * 22.0 + warp * 3.0);
    let cloud = smoothstep(0.48, 0.8, n1) * wisps * (0.3 + 0.7 * exp(-pow(lat / 0.4, 2.0)));
    let hue = mix(mix(vec3(0.85, 0.22, 0.55), s.cloud.rgb, smoothstep(0.35, 0.65, n2)), vec3(0.12, 0.62, 0.75), smoothstep(0.6, 0.78, n2));
    color += hue * cloud * 0.22 * (1.0 - 0.6 * lanes);

    // Stars: a fine dusting, thicker in the band; ordinary stars; a few bright ones.
    color += star_layer(bearing, elevation, 1400.0, 0.55, band, 0.09, 0.32, 0.0, 1.0, t);
    color += star_layer(bearing, elevation, 900.0, 0.14, 1.0, 0.08, 0.45, 0.0, 2.0, t);
    color += star_layer(bearing, elevation, 420.0, 0.07, 1.0, 0.06, 0.9, 0.0, 3.0, t);
    color += star_layer(bearing, elevation, 150.0, 0.05, 1.0, 0.035, 3.0, 1.0, 4.0, t);

    // A little haze where the sky meets the map's edge.
    color = mix(color, s.horizon.rgb, (1.0 - smoothstep(0.0, 0.2, height)) * s.clouds.w);
    return max(color, vec3(0.0));
}

struct FsOut {
    @location(0) color: vec4<f32>,
    @location(1) info: vec2<f32>,
};

@fragment
fn fs_main(input: VsOut) -> FsOut {
    let uv = input.uv;
    let night = s.time.y;
    let seed = s.time.w;
    let haze = s.clouds.w;
    // A wider lens only for the backdrop prevents cloud shapes stretching at ultrawide
    // aspect ratios. Yaw wraps seamlessly because noise is sampled on a cylinder.
    let lens = max(tan(s.camera.w * 0.5), 0.62);
    let bearing = -s.camera.x + atan((uv.x - 0.5) * 2.0 * lens * s.camera.z);
    let elevation = 0.08 + (1.0 - uv.y) * 0.82 + (0.872665 - s.camera.y) * 0.3;
    let direction = vec3(sin(bearing) * cos(elevation), sin(elevation), cos(bearing) * cos(elevation));

    let height = clamp((elevation - 0.08) / 0.82, 0.0, 1.0);
    if s.sun.w > 0.5 {
        var space: FsOut;
        space.color = vec4(galaxy(direction, bearing, elevation, height), 1.0);
        space.info = vec2(60000.0, 0.0);
        return space;
    }
    let gradient = pow(height, 0.7 + haze * 0.4);
    let day_color = mix(s.horizon.rgb, s.zenith.rgb, gradient);
    // Keep each map's hue in moonlight rather than replacing every sky with the same blue.
    let night_top = s.zenith.rgb * vec3(0.06, 0.08, 0.17) + vec3(0.004, 0.007, 0.018);
    let night_bottom = s.horizon.rgb * vec3(0.08, 0.10, 0.18) + vec3(0.008, 0.012, 0.027);
    var color = mix(day_color, mix(night_bottom, night_top, gradient), night);

    let sun_alignment = max(dot(direction, s.sun.xyz), 0.0);
    let sun_glow = pow(sun_alignment, 28.0) * (1.0 - haze * 0.65);
    color += s.cloud.rgb * sun_glow * 0.13 * (1.0 - night);

    let drift = s.time.x * s.clouds.z;
    // The cloud layer overhead, seen in perspective: shapes shrink toward the horizon
    // instead of stretching across a wide screen, and the finest detail fades out far off
    // (where it would only shimmer).
    let plane = direction.xz / max(direction.y, 0.06);
    let far = length(plane);
    let detail = 1.0 - smoothstep(3.0, 11.0, far);
    let wind = vec2(drift * 1.3, drift * 0.45);
    let seeded = vec2(seed * 47.3, seed * 83.1);

    // Cirrus: thin wisps far above the cumulus, streaked along the wind.
    let hp = plane * s.clouds.y * 0.2 + wind * 0.35 + seeded * 0.31;
    let streak = 0.5 + gfbm(vec2(hp.x * 0.5 + hp.y * 0.18, hp.y * 2.6 - hp.x * 0.9), 5, detail) * 1.3;
    let cirrus = smoothstep(0.6, 0.84, streak) * (0.2 + 0.35 * s.clouds.x) * smoothstep(0.08, 0.45, height) * (1.0 - smoothstep(10.0, 16.0, far));
    let cirrus_color = mix(s.cloud.rgb * 1.04 + s.cloud.rgb * sun_glow * 0.2, night_top * 1.8, night);
    color = mix(color, cirrus_color, cirrus * 0.3);

    // Cumulus: broad masses, domain-warped so they billow, with puffy detail on top.
    let cp = plane * s.clouds.y * 0.95 + wind + seeded;
    let warp = vec2(gfbm(cp * 0.6 + vec2(5.2, 1.3), 3, 1.0), gfbm(cp * 0.6 + vec2(8.3, 2.8), 3, 1.0));
    let wp = cp + warp * 0.35;
    // Cauliflower tops: ridged mid-size noise adds rounded puffs to each mass.
    let puffs = (0.5 - abs(gnoise(wp * 2.7 + vec2(1.9, 7.4)))) * 0.22 + (0.5 - abs(gnoise(wp * 5.9 + vec2(6.1, 2.2)))) * 0.1 * detail;
    let mass = 0.5 + gfbm(wp, 6, detail) * 1.15 + puffs - 0.12;
    let threshold = 0.64 - s.clouds.x * 0.36;
    let density = smoothstep(threshold, threshold + 0.14, mass) * smoothstep(0.0, 0.12, s.clouds.x);
    // Light through the cloud: how much of it lies between here and the sun (two steps
    // toward it), so sunlit sides are bright and the far sides and bellies darker.
    let sun_flat = s.sun.xz / max(length(s.sun.xz), 1e-3);
    let toward = sun_flat * 0.09;
    let near_sun = 0.5 + gfbm(wp + toward, 3, 1.0) * 1.25;
    let far_sun = 0.5 + gfbm(wp + toward * 2.5, 3, 1.0) * 1.25;
    let thick = max(near_sun - threshold, 0.0) * 1.4 + max(far_sun - threshold, 0.0) * 0.9 + max(mass - threshold, 0.0) * 0.6;
    let lit = exp(-thick * 4.0);
    // Looking toward the sun, the thin edges glow (a silver lining).
    let rim = pow(sun_alignment, 5.0) * density * (1.0 - smoothstep(0.0, 0.55, density)) * 3.0;
    let shadow = mix(s.horizon.rgb, s.cloud.rgb, 0.45) * 0.8;
    let day_cloud = mix(shadow, s.cloud.rgb * 1.08, lit) + s.cloud.rgb * (rim + sun_glow * 0.15);
    let night_cloud = mix(night_bottom, s.cloud.rgb * vec3(0.08, 0.10, 0.17), lit);
    // Clouds settle into the horizon haze, thin out far off, and soften at night.
    let opacity = density * mix(0.6, 0.97, smoothstep(0.0, 0.3, height)) * (1.0 - night * 0.15) * (1.0 - 0.75 * smoothstep(6.0, 15.0, far));
    color = mix(color, mix(day_cloud, night_cloud, night), opacity);

    // A sparse, antialiased star field, tied to map bearing rather than screen pixels.
    // Wrapping the longitude cell keeps the same stars when rotating across north.
    let star_pos = vec2(bearing / 6.2831853 * 384.0, elevation * 140.0);
    let star_cell = floor(star_pos);
    let wrapped_x = star_cell.x - floor(star_cell.x / 384.0) * 384.0;
    let star_id = vec3(wrapped_x, star_cell.y, floor(seed * 997.0));
    let star_hash = hash(star_id);
    let star_offset = vec2(hash(star_id + vec3(17.0)), hash(star_id + vec3(39.0))) * 0.6 + 0.2;
    let radius = 0.045 + hash(star_id + vec3(71.0)) * 0.055;
    let aa = max(length(fwidth(star_pos)) * 0.5, 0.005);
    let dot_shape = 1.0 - smoothstep(radius, radius + aa, length(fract(star_pos) - star_offset));
    let twinkle = 0.86 + 0.14 * sin(s.time.x * 0.7 + star_hash * 125.0);
    let stars = step(0.991, star_hash) * dot_shape * twinkle * s.time.z * night
        * (1.0 - opacity) * smoothstep(0.04, 0.35, height) * (1.0 - haze * 0.6);
    color += vec3(0.62, 0.73, 0.95) * stars;

    var out: FsOut;
    out.color = vec4(max(color, vec3(0.0)), 1.0);
    // Same void distance as the renderer clear: AO and geometry fog ignore the sky.
    // Depth writes are disabled, so geometry, sprites and transparent effects cover it.
    out.info = vec2(60000.0, 0.0);
    return out;
}
