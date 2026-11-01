# Outdoor skies

Outdoor maps draw a procedural sky behind the map geometry. Regional palettes distinguish
temperate towns and fields, deserts, coasts, snow, industrial haze, and other environments.
Each map has a stable seed for its cloud arrangement and subtle color variation; the RSW
lighting contributes to the palette. Clouds drift slowly, and night blends the background
toward moonlight and stars. The sky is an illustrated backdrop suited to RO's downward
camera, rather than a physical horizon that would sit above the screen.

Herosria (and the maps drawn from its files, such as Twin Flames' `tf_goddess`) floats in
open space instead: the Milky Way arching across with its warm core and dark dust lanes,
coloured nebulae, and four layers of twinkling stars, the same at any hour. MapSkyData's
cloudy sky for Herosria is not applied over it.

The sky draws before ground, models, water, sprites and effects in the HDR world pass.
It does not write hardware depth. Its post-process depth is the same far value as empty
space, so it cannot occlude the world. Water receives the map sky palette for reflections.

## Which maps get a sky

`ro-render/src/sky_profile.rs` classifies known exterior towns and map families, including
explicit outdoor exceptions to dungeon naming. Unknown maps stay dark until classified;
the absence of `_in` alone is not evidence that a map is outdoors. Interior room masking
continues to work independently.

The client also reads the installed `data/indoorRswTable.txt` through the VFS and excludes
its entries. `data/LuaFiles514/Lua Files/MapSkyData/MapSkyData.lub` refines an admitted sky's
background and cloud colors, stars and haze. That stock table also contains underground
special effects, so an entry there does not classify a map as outdoors. Submerged maps
always retain the underwater background.

## Per-map art direction

The optional `sky.toml` lives beside the active client configuration (`config/local.toml`
by default, or the path named by `ROCLIENT_CONFIG`). Start from
`config/sky.example.toml`. It is read on every map entry; return to the map to see edits.

```toml
[maps.my_outdoor_map]
enabled = true
zenith = [0.24, 0.52, 0.80]
horizon = [0.82, 0.90, 0.96]
cloud = [0.98, 0.97, 0.93]
cloud_cover = 0.5
cloud_scale = 1.0
wind_speed = 0.025
haze = 0.2
stars = 0.7
```

Every field is optional. Colors are three sRGB channels from 0 to 1, converted to linear
light by the client. `cloud_cover`, `haze` and `stars` range from 0 to 1; `cloud_scale`
ranges from 0.25 to 8, and `wind_speed` from 0 to 1 (0 holds clouds still).

`enabled = false` suppresses the sky even when unused art values in that entry are invalid.
`enabled = true` explicitly treats a map as outdoors,
including a custom map or a correction to the indoor table, using regional or general
defaults for omitted fields. It cannot override submerged-map detection. Without an
`enabled` value, colors only modify maps already classified as outdoors.

Local overrides take precedence over stock metadata and regional defaults. Map keys are
case insensitive; `.rsw`, `.gat`, `.gnd` extensions and dynamic numeric instance prefixes
such as `123#prontera` are normalized. Invalid TOML, unknown fields, nonfinite numbers
and values outside the documented ranges produce a warning and fall back to defaults.
Two keys that normalize to the same map (for example `Prontera` and `prontera.rsw`)
reject the overrides file with a warning, so precedence never depends on key ordering.

The renderer and client tests cover outdoor/interior classification, stable map variation,
stock metadata parsing, override precedence and shader validation. Existing `--offline`,
`--map`, `--at` and `--capture` options can preview skies without a server connection.
Use `--yaw` and `--pitch` (degrees) to frame the map edge, `--distance` to set the zoom,
and `--capture-after 600` to let the first UI frame settle. `--offline --night` previews
the night sky without changing the server's day/night state.

```powershell
.\target\release\tenseiga.exe --offline --map prontera --at 156,34 --yaw 165 --pitch 30 --distance 650 --capture-after 600 --capture target/prontera-sky.png
```
