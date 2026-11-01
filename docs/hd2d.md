# HD-2D rendering

The goal is the *Octopath Traveler* / *Triangle Strategy* presentation: RO's 2D sprites
standing in a lit 3D diorama, with soft directional shadows, bloom, a tilt-shift blur and
storybook colour grading. The original look remains one toggle away.

## Frame pipeline

```
 sprites ──► sun shadow pass ──► shadow map (2048², depth)
                                      │
 ground, models, water, sprites, effects ──► HDR scene (Rgba16Float, MSAA resolved)
                                      │  (scene.wgsl samples the shadow map)
                                      ▼
 post chain:  bright pass ½ ─► ¼ ─► blur H ─► blur V ────────── bloom
              downsample ½ ─► ¼ ─► blur H ─► blur V ────────── DoF source
              composite: tilt-shift mix ─► + bloom ─► exposure / warmth /
                         saturation / contrast ─► highlight shoulder ─► vignette
                                      ▼
                                 swapchain ──► egui UI on top
```

| Piece | File | Notes |
|---|---|---|
| HDR target and post chain | `crates/ro-render/src/post.rs`, `post.wgsl` | Fullscreen-triangle passes; quarter-resolution blurs keep the cost low |
| Sun shadows for sprites | `shadow.rs`, `shadow.wgsl` | Sprites only. RO maps already bake building shadows into lightmaps, so static geometry does not cast |
| Shadow receiving | `scene.wgsl` (`sprite_shadow`) | 5×5 PCF with a comparison sampler; strength and softness from globals |
| Ray-traced sun shadows | `rt.rs`, `rt.wgsl` | Outdoors on GPUs with ray queries: all map geometry casts toward the sun, replacing the lightmaps' baked shadows |
| Look settings | `PostSettings` (renderer), `settings::Visual` (client) | Saved in `settings.toml`, edited live in Settings → Visual |

### Sprite shadows

Each sprite layer is drawn into the shadow map as an upright card at the actor's feet,
alpha-tested at 0.5, so the shadow keeps the sprite's silhouette. The card faces the sun,
not the camera. A camera-facing card seen from the side collapses into a thin line, while a
sun-facing card throws its full outline. That matches how HD-2D games cast character shadows.
The round blob shadow under each actor is skipped as a caster but still drawn, as a contact
shadow.

The sun keeps the map's bearing (RSW longitude) but its height comes from
`sun_elevation` (35° by default). RO's own light is often nearly vertical, which hides
shadows under the sprite.

### Torch shadows

The two torches nearest the player (RSW light sources, closest first, fading in within a
third of their range) each get a 1024² sprite shadow map (`Renderer.torches`, `TORCHES`).
They're drawn like the sun's, but in perspective from the torch (`Shadow::torch_vp`, 120°),
each card turned toward it. RO hangs its lights high, so a torch casts from at most
`TORCH_HEIGHT` (2.5 cells) above the player, and shadows stretch across the floor instead
of pooling at the feet. The world shader's `torch_shadows` darkens by Shadows × the light's
falloff there, so the shadow also shows on torchlight baked into the lightmaps. Night
doesn't weaken it. Works on every GPU.

### Ray-traced sun shadows

Baked lightmap shadows point one way, so they go wrong once the sun moves, and models never
receive them. With ray queries (`Features::EXPERIMENTAL_RAY_QUERY`, Vulkan or DX12; not
requested on Android), `rt.rs` puts the map's ground and models in one BLAS when it loads.
Opaque triangles form one geometry. Triangles with colour-keyed textures form a second,
non-opaque one, whose 256² alpha masks sit in a texture array, so leaves and railings let
light through. The world shader's `sun_ray` (from `rt.wgsl`, joined in place of the stub at the
end of `scene.wgsl`) casts four rays over a small sun disc (about 1.7°, turned from spot to spot) toward `shadow_dir()`, for soft edges,, the same sun the sprite shadows use.
Where it's on, `lm.a` is ignored and the ray's result joins the sprite shadow, so both get
the same darkness (Shadows). Sprites trace the same ray from their feet (`sprite.wgsl`), so
characters darken when they walk into a building's shadow.

Water reflections trace too (`rt_reflect`, the water shader's group 3). The reflected ray
finds what it would see of the map, off screen included, and shades it from each triangle's
info: a 128² copy of its texture (the same array the cut-out test reads), its vertex colour
and the sun, with a one-ray shadow. Screen-space reflection still lies on top where it finds
the scene, which keeps on-screen detail and sprites. The ray tracing has no lightmaps, so
off-screen surfaces show no baked colour light. Setting: Ray-traced reflections (on by default except on Android). It runs outdoors only. Interiors, dungeons and underwater keep
their hand-baked light. Setting: Ray-traced shadows (on by default except on Android).

### Tilt-shift depth of field

There is no depth read-back, because MSAA depth is not sampleable. Instead the composite
blends toward a blurred copy by screen height, sharp in a band around the player (the camera
keeps them centred). For a top-down diorama this is the miniature look itself, and it costs
two quarter-resolution blurs.

### Tone

Shaders still produce the client's familiar gamma-tuned colours, so with everything neutral
the image matches the classic look. Grading is applied on top. A highlight shoulder rolls
off values above 0.8, so bloom and bright effects don't clip.

## Controls

Settings → Visual. The same fields are in `settings.toml` under `[visual]`.

| Control | Default | Effect |
|---|---|---|
| HD-2D lighting and effects | on | Off gives the classic look: plain copy, no shadows |
| Bloom | 0.22 | Glow strength (threshold 0.92 in `PostSettings`) |
| Depth of field | 0.85 | Tilt-shift blur toward the top and bottom of the screen |
| Shadows | 0.70 | How dark sprite shadows get |
| Sun height | 35° | Lower values give longer shadows |
| Vignette | 0.45 | Darkened corners |
| Warmth | 0.35 | Warm highlights, cool shadows (−1 to 1) |
| Saturation / Contrast | 1.10 / 1.12 | Grading around mid grey |

`PostSettings` also holds `bloom_threshold`, `dof_band`, `exposure` and
`shadow_softness` for finer tuning.

## Lighting, atmosphere and stability

All six roadmap items are implemented:

1. **Sprite lighting** (`sprite.wgsl`). Each sprite takes the map light at its feet
   relative to full sun, so it keeps its colours in the open and darkens in shadow. Feet
   shadow comes from the shadow map, sampled a little toward the sun so a sprite never shades
   itself. Nearby light sources add to it, and a sun-coloured rim lights the edges whose
   neighbour toward the sun is transparent.
2. **Point lights**. RSW light sources (colour floats, range; position centred like models,
   y flipped) go to the renderer. The 32 nearest the camera ride in the globals and light
   ground and models (half-wrapped, smooth falloff) and sprites. Fire-coloured ones (red over
   green over blue) flicker by up to a quarter on three sine beats of their own, and so does
   the torch light baked into the lightmaps around them (most of a torch's light is baked; the
   live light alone is too dim for a flicker to show).
3. **Fog and god rays** (`post.wgsl`). Fog: linear distance from the depth-info target
   blends toward a pale sky tinted by the map ambient, from 1.15x to 2.6x the camera
   distance. God rays: the bloom bright pass is marched toward the sun's screen
   position (24 taps with decay) and added in the composite.
4. **Ambient occlusion** (`fs_ao`). A 12-tap spiral over linear depth at half
   resolution; nearby neighbours closer to the eye occlude, with range limits against
   halos. Sprites neither cast nor receive it.
5. **Depth-aware DoF**. Every world and sprite pixel writes `(distance, is_sprite)` to an
   `Rg16Float` target alongside colour, resolved from MSAA. The composite blur is the
   larger of the tilt-shift band and real distance from the focal plane (the player).
6. **Pixel stability**. Sprite anchors snap to whole screen pixels (`params.w`), so
   sprites don't shimmer as the camera glides.

Settings → Visual adds sliders for sprite lighting, rim light, light sources, fog,
god rays and ambient occlusion, plus a pixel-snapping toggle (`PostSettings` fields of
the same names).

### Ideas beyond this

- Small additive glow sprites on light positions.
- Baked per-vertex AO for models (cheaper and steadier than screen-space AO).
- Nearest-neighbour sprite sampling at high zoom.
