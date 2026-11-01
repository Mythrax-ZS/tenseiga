# Water

The water renderer replaces RO's flat, alpha-blended animated texture with a displaced,
refracting, reflecting surface that absorbs light with depth and foams at the shore. It
reads the same RSW water and GND heights as the original, so every classic map gets it with
no new assets, and it keeps the original texture as an optional tint (the Classic preset is
the original look).

Code: `crates/ro-render/src/water.rs` (resources, settings, presets),
`crates/ro-render/src/water.wgsl` (shader), wiring in `crates/ro-render/src/lib.rs`.

## 1. How it plugs into the renderer

```
main pass (HDR + depth info + depth buffer)
 ├─ opaque world (GND + RSM models)            ─┐
 │                                              │  only when refraction or SSR is on:
 ├─ [end pass] copy HDR colour + depth info  ◄──┘  2 GPU copies, then the pass resumes
 ├─ water  (group 0 globals, 1 map water, 2 screen copies)
 ├─ warp portals, sprites, skill effects
post: AO, fog, DoF, bloom, god rays, grading  (the water is lit in linear HDR, so bloom
                                              picks up sun glints and fog/DoF apply)
```

* The opaque world already writes linear eye distance to the `info` target (used by AO, fog
  and DoF). The water reuses it as its depth buffer: no extra depth pre-pass.
* When neither refraction nor SSR is enabled (Classic, Low) the pass is not split and nothing
  is copied; the water alpha-blends over the ground like the original.
* Water writes no depth info, so post effects treat the bottom as the surface distance.
  That keeps fog and DoF matching the original's look of the seabed.

## 2. Data and resources

| Map | Format / size | Source | Use |
|---|---|---|---|
| Height + normal | RGBA8 256², tileable, mipmapped | generated (`detail_texture`) | RG: ripple normal, B: height (crest mask) |
| Foam | A channel of the same texture | generated (cellular) | shore + crest foam pattern |
| Shore / depth | RGBA8, one texel per GND corner | baked per map (`shore_map`) | R: water depth / 64, G: distance to dry land / 128 |
| Sky | RGBA8 sRGB 256×128 equirect, mipmapped | generated from the map's RSW light (`sky_map`) or supplied (`set_sky_map`) | reflections; its cosine-weighted mean is the mean sky radiance |
| Classic frames | RO `워터\waterNNN.jpg` ×32 | the client's own data | optional tint (`classic_mix`) |
| Scene colour + depth | HDR + RG16F, screen size | copied each frame | refraction, Beer-Lambert thickness, SSR |

* **Shore map:** for each GND corner, `depth = level − ground` (y-up), then a two-pass
  chamfer distance transform gives the distance to the nearest corner at or above the water
  line. It's computed once on map load, in O(cells).
* **Detail texture:** a sum of 28 sine waves with integer frequencies, so it is exactly
  periodic and tiles with no seam. It uses a power-law spectrum like real ripples, and the
  normals are analytic. Foam is the border field of a wrapping 8×8 Worley grid.
* **Authoring your own:** to use hand-painted or photo-sourced maps, replace
  `detail_texture()` (keep the channel layout) or call `set_sky_map(Some(img))` with any
  equirectangular sky (top half above the horizon) before the map loads. The mean radiance
  is integrated automatically.

## 3. Shader (`water.wgsl`)

All features are bits in `Water.flags.x`, tested with uniform branches, so presets switch
instantly. There's one pipeline and no shader permutations.

| Bit | Feature | What it does |
|---|---|---|
| `F_WAVES` | Vertex displacement | 4 summed Gerstner waves on top of RO's own sine swell; amplitude fades to 0 in shallow water so waves never climb the beach. Analytic normals and a crest height go to the fragment stage. |
| `F_NORMAL_MAP` | Ripples | two scrolling detail layers at different scales and directions, faded with distance to avoid shimmer |
| `F_REFRACTION` | Refraction | samples the scene copy offset by the normal tilt × depth; offsets landing in front of the water are rejected (no "ghosting" of piers into the water) |
| `F_ABSORPTION` | Beer-Lambert | `transmit = exp(-σ · thickness)` per channel, with thickness taken along the view ray from the depth copy; mixed with the water's in-scattered colour |
| `F_REFLECTION` | Sky reflection | Schlick Fresnel on the sky map; when off, the mean sky radiance stands in |
| `F_SSR` | Screen-space reflection | short ray-march of the reflected ray against the depth copy; fades at screen edges and with distance into the sky reflection |
| `F_FOAM` | Shore foam | distance-to-shore (baked) plus a contact line from the true view thickness (piers, walls), shaped by two scrolling foam layers and a lapping pulse; sparse crest foam on the tallest waves |
| `F_BICUBIC` | Bicubic filtering | 4-tap B-spline (Sigg & Hadwiger) on the detail, shore and classic textures; gradients are passed explicitly so mips still work |
| `F_CLASSIC` | RO texture | the original animated frame blended into the water body colour |
| `F_SPECULAR` | Sun glint | Blinn-Phong against the map's sun, gated by sun height |
| `F_CAUSTICS` | Caustics | two scrolling, rotated layers of the cellular pattern multiplied into a moving light net on the refracted seabed; fades in over the first unit of depth, out with deep water and a low sun (Medium and up) |
| `F_INTERACT` | Wading | actors standing in water get a wobbling foam collar and shed ripple rings (a trail of them while walking, the wake; a slow pulse while standing), up to 48 rings and 16 waders. Sprites sink to the waterline (capped at knee height, since RO seabeds are often far below the surface), tinted and faded below it with a wet line where they break the surface. Rings are centred where the eye ray through the waterline meets the surface, so they sit at the sprite on screen (Low and up) |
| (always) | Variation | a slow, large-scale field varies ripple strength (glassy vs choppy patches), the water tint, clarity (murky vs clear patches), haze and where the sun glitters, so a bay never reads as one flat repeat; ripple layers are rotated and at unrelated scales so their tiling never lines up |

### Lava

RSW water types 4 (molten) and 6 (a cooling crust with glowing cracks) are RO's lava, as in
the Magma Dungeon. On every preset but Classic they get `F_LAVA` (set per map in
`Water::update`) and their own shading in `lava()` instead of the water's:

* opaque and lit from within: no reflection, refraction tint, foam, glitter, caustics or
  wading (sprites don't sink into it either);
* RO's own lava frames creep under a slow, coarse domain warp, and their brightness is the
  heat, mapped along a dark red → orange → yellow-white ramp kept near 1 so tone mapping
  doesn't flatten it to yellow;
* dark crust plates (cells of the cellular pattern, two layers drifting apart) float on the
  melt with hot seams; more of them toward the shore, where it has cooled;
* a slow swell of heat across the surface, a thin flickering hot line against the rock, and
  a thick, slow swell (a quarter of the water's height, a fifth of its speed).

`cargo run -p ro-formats --example waterkinds -- map.gnd map.rsw` prints a map's water type
and the ground textures under it.

### HLSL / GLSL / SPIR-V

WGSL is the only source. wgpu translates it for every backend at run time (Vulkan, DX12,
Metal, GL). For engines, tools or shader debuggers that need the other languages, naga (the
same compiler) emits them:

```
cargo run -p ro-render --example shaderxlate -- crates/ro-render/src/water.wgsl target/shaders
# -> water.spv, water.vs_main.vert, water.fs_main.frag (GLSL 450), water.hlsl (SM 5.1)
```

`cargo test -p ro-render water` validates the WGSL with naga, so a shader that won't compile
fails the build before it reaches a GPU.

## 4. Rust API

```rust
// Map load (after set_scene, so the generated sky follows the map's light):
let mesh = ro_render::WaterMesh::build(&gnd, &rsw.water);   // classic RSW cells
// or WaterMesh::rect(&gnd, &water, x0, y0, x1, y1)           // a large custom body
renderer.set_water(&mesh, frames);

// Settings, any time (applied next frame; tessellation re-uploads the mesh):
let mut s = ro_render::WaterSettings::preset(ro_render::WaterQuality::High);
s.foam.intensity = 0.6;
s.refraction = false;
renderer.set_water_settings(s);

// Optional: your own sky for reflections (next set_water).
renderer.set_sky_map(Some(sky_rgba));
```

The client maps Settings → HD-2D → **Water** (Classic / Low / Medium / High / Ultra) to these
presets. Turning HD-2D off forces Classic. For debugging, `RO_WATER_OFF=<bits>` (decimal or
`0x` hex) clears feature bits at run time, e.g. `RO_WATER_OFF=0x200` hides the sun glint.

## 5. Presets

| | Classic | Low | Medium | High (default) | Ultra |
|---|---|---|---|---|---|
| Waves (Gerstner) | – | ✓ | ✓ | ✓ | ✓ |
| Tessellation / tile | 1 | 1 | 2 | 3 | 6 |
| Ripple normals | – | ✓ | ✓ | ✓ | ✓ |
| Bicubic | – | – | – | ✓ | ✓ |
| Refraction + depth absorption | – | blend only | ✓ | ✓ | ✓ |
| Sky reflection | – | ✓ | ✓ | ✓ | ✓ |
| SSR (steps / distance) | – | – | – | 16 / 160 | 32 / 220 |
| Shore foam | – | ✓ (no crests) | ✓ | ✓ | ✓ |
| Screen copies per frame | 0 | 0 | 2 | 2 | 2 |

Example `config/settings.toml` (the in-game Settings window writes this):

```toml
[visual]
hd2d = true
water = 3          # 0 Classic, 1 Low, 2 Medium, 3 High, 4 Ultra
```

Tunables per preset (in code, `WaterSettings`): foam intensity / falloff / speed / scale /
crest / colour, absorption σ (RGB), scatter colour, depth scale, refraction and reflection
strength, SSR steps / distance, ripple strength / tiling / speed, wave amplitude / length /
steepness / speed, sun shininess / intensity, classic texture mix. All of them are plain
fields, ready to put behind a debug UI.

## 6. Performance notes

* **Vertices:** water tiles × tessellation² × 6. Alberta's harbour is about 4k tiles: 24k
  vertices at Low and 864k at Ultra (6×6). That's trivial for the vertex stage, and the
  displacement is 4 sines per vertex.
* **Fragment cost** (per water pixel, High): 2 ripple + 1 shore + 2 foam + 1 mask sample,
  each ×4 for bicubic. That's 24 taps, plus 1 sky, up to 2 scene-depth loads, 1 refraction
  sample and SSR (up to 16 depth loads, early-out). On a mid-range GPU at 1080p with water
  filling half the screen, this is well under 1 ms. Medium drops bicubic and SSR, which
  roughly halves it.
* **Bandwidth:** two full-screen copies per frame on Medium and above (RGBA16F + RG16F, about
  12 MB at 1080p). Classic and Low skip them entirely.
* **Texture sizes:** detail 256² is enough because it tiles every 42 world units and is
  filtered bicubically. Use 512² only for very close cameras. A 256×128 sky is plenty for a
  glossy surface; reflections are blurred by the ripples anyway. The shore map is fixed by
  the map size (≤ 401² for the largest maps, under 1 MB).
* **Mobile / integrated GPUs:** use Low. It's single-pass, has no copies, and costs about as
  much as the original water plus 3 texture samples.
