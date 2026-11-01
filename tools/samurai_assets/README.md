# Samurai artwork compiler

This tool imports **already generated artwork** into SPR 2.1 + ACT 2.5. It does
not paint artwork, infer feet from silhouettes, normalize individual poses,
guess hands, or synthesize missing motion. Source pixels are sampled on a fixed
grid and RGBA values, including partial transparency, are encoded as truecolor.

Run from the client repository:

```powershell
py tools/samurai_assets/compile.py C:/path/to/build.json --output C:/path/to/compiled
py -m unittest discover -s tools/samurai_assets -p 'test_*.py'
```

Python 3.12+ and Pillow are required. The output is staged in the supplied
directory. Installing files into an overlay is a separate, deliberate step.

## Input metadata

All paths in the JSON resolve relative to that JSON. `downsample` is one positive
integer divisor for **every body frame**; 4 means a 256px source cell becomes a
64px game cell. Sampling happens before trimming, so cropped bounds cannot
change the pixel phase between frames. The trimmed output is padded to even dimensions for
integer ACT center positions. `origin` is the fixed ground position in the
source cell, and is rounded to a game pixel after division. Advancing feet must
move around this origin; do not move the origin to follow the leading foot.

A sheet may specify a single positive `source_scale` (default 1) to calibrate a
generator's differently sized atlas against the master. For example, 0.95
scales the entire sheet's artwork by 95% before the common divisor. Both steps
are applied in **one** nearest-neighbor affine sample with a fixed spacing, not
two successive resizes. The same scale applies to every pose in that sheet;
all rectangles and origins remain authored in original source coordinates.
This is explicit atlas calibration, never per-pose silhouette normalization.

The following outline uses three sheets. Add every group 0 through 12 exactly
once. Column selection explicitly controls timeline order and frame repetition.

```json
{
  "schema_version": 1,
  "name": "Samurai — RO revision",
  "body": "ember_ronin",
  "weapon_name": "katana",
  "downsample": 4,
  "head_anchor_y": -40,
  "mirror_directions": {"NE": "NW", "E": "W", "SE": "SW"},
  "sheets": {
    "states": {
      "path": "source/states.png",
      "cell_size": [192, 256],
      "rows": ["S", "SW", "W", "NW", "N"],
      "origin": [96, 232]
    },
    "walk": {
      "path": "source/walk.png",
      "cell_size": [192, 256],
      "rows": ["S", "SW", "W", "NW", "N"],
      "origin": [96, 232]
    }
  },
  "groups": [
    {"index": 0, "sheet": "states", "columns": [0], "interval_ms": 200, "loop": true},
    {"index": 1, "sheet": "walk", "columns": [0, 1, 2, 3, 4, 5], "interval_ms": 100, "loop": true}
  ]
}
```

The example above is an outline and intentionally omits 11 groups. The standard
groups are 0 idle, 1 walk, 2 sit, 3 pickup, 4 ready, 5 attack1, 6 hurt, 7 freeze,
8 die, 9 freeze2, 10 attack2, 11 attack3, 12 cast. ACT order is always
`group * 8 + direction`, with directions S, SW, W, NW, N, NE, E, SE. Mirroring
reflects the entire layer geometry, including x offset and rotation. Mirrored
costume asymmetries reverse; symmetric costumes avoid that discrepancy.

A sheet can have `offset: [left_margin, top_margin]`. For an explicitly measured
nonuniform grid, replace `cell_size` with `x_edges: [0, ...]` and
`y_edges: [0, ...]`. These are authored coordinates, never inferred gutters.

For a source cell that needs measured correction, use:

```json
"cell_overrides": {
  "walk:2:3": {"origin": [96, 232], "shift": [0, -1], "rect": [576, 512, 768, 768]}
}
```

Keys are `sheet:row:column`. `rect` uses absolute source image coordinates;
`origin` stays relative to that rectangle; `shift` uses game pixels. These are
placement metadata and do not redraw pixels. For mixed-sheet timelines, a group
may supply `frames: {"S": [{"sheet":"states","row":0,"column":3}, ...], ...}`
instead of `sheet`/`columns`, for each unmirrored direction. Each reference can
also override `rect`, `origin` and `shift`. A sheet may omit `rows` when every
group using that sheet supplies explicit frame references (for example, when
directions are columns rather than rows).

## Separate katana

Existing weapon art can be decoded from an SPR without changing its pixels:

```json
"weapon": {
  "spr": "../ember_ronin/katana.spr",
  "image": 0,
  "source_grip": [12.5, 6.0]
}
```

`source_grip` is the handle's authored grip position in that SPR image. Use
`png` instead of `spr` to import a generated RGBA source directly:

```json
"weapon": {
  "png": "source/katana.png",
  "rect": [0, 0, 1024, 256],
  "downsample": 16,
  "source_grip": [9, 8]
}
```

Exactly one of `spr` or `png` is required. For PNG, `rect` is an explicit
integer rectangle inside the source (default: the whole image); `downsample`
is a positive integer (default: 1). The compiler crops that rectangle and takes
one nearest-neighbor sample, preserving the sampled RGBA values. Output width
and height are the rectangle dimensions divided by `downsample`, rounded up.
Transparent margins remain; the compiler never crops to a detected silhouette.
`source_grip` is measured in the resulting sampled image, not the original PNG.
The source PNG hash is recorded, and the runtime weapon is still an SPR/ACT pair.

Add `weapon_poses` to each armed group. It maps each **unmirrored direction** to
exactly one entry per body timeline frame:

```json
"weapon_poses": {
  "S": [
    {"grip": [4, -25], "rotation": 30, "scale": [0.7, 0.7]},
    null
  ]
}
```

`grip` is measured in game pixels relative to the shared ground origin;
`rotation` is clockwise screen degrees. `null` means no weapon layer. Directions
without weapon poses are unarmed. The compiler computes layer center from the
authored grip; it never detects a hand by color. Reflection automatically mirrors
the weapon geometry alongside the body. Body and katana always get identical
104 action counts, frame counts and delays, including unarmed empty layers.

`offhand_poses` has the same shape and describes a second, left-hand copy of the
weapon for dual wielding. When any group has them, the compiler also writes
`<weapon_name>_offhand.spr/.act`: the same weapon image on those poses, with the
same timeline as the body. The client draws it just before the main weapon.

## Review outputs

`validation_report.json` checks strict SPR/ACT parsing, all image references,
104 actions, paired weapon timelines and the client atlas dimensions. It records
source/config checksums and every source cell's fixed origin and edge contacts.
It records all nonzero-alpha contacts separately from visible contacts (alpha
128 or greater), so nearly invisible generator residue is distinguishable from
likely clipped anatomy. Neither measurement modifies the source alpha.
An edge contact signals possible clipping and requires visual inspection.

The thirteen `contact_*.png` sheets show all eight directions and every frame.
`preview.html` plays all directions simultaneously with speed, stepping and
weapon controls. **Both are built from decoded output binaries**, not from
uncompiled source imagery. The exported PNGs preserve decoded RGBA values.
Preview browser/Pillow rotation sampling may differ slightly from the GPU.

Structural validation cannot prove art style, correct direction, foot alternation,
weapon grip anatomy or convincing animation. Inspect those, the end-to-start walk
transition, source clipping flags, and the live client before release.

## Separating the head

The generated frames draw the head and ponytail into the body. `separate_head.py`
turns the compiled body into an official-style one, after `compile.py` and
`compile_weapons.py`:

```powershell
py tools/samurai_assets/separate_head.py fit target/samurai-review --reference-head <folder>
py tools/samurai_assets/separate_head.py build target/samurai-review --output target/samurai-head
py tools/samurai_assets/separate_head.py install target/samurai-head
```

`fit` lays an official male head over each frame's drawn head to find the head
anchor (then lowers it onto the shoulders, as official bodies seat their heads),
and records the official hair-colour shades and head-turn offsets. Its folder
holds `2_남.spr/.act` and `머리2_남_1..8.pal` extracted from the game data; they
are never committed. The result, with any hand corrections, is
`artwork/head_cut.json`: per frame `centre_window` bounds the fit, `cut` and
`keep` rectangles add to or protect from the cut, and `anchor_override` replaces
the anchor. `build` cuts the head and hair out, fills the coat they covered,
writes an indexed body with one palette range per material, the clothes dyes,
hairstyle 43 and its hair colours, and `review_frames.png` (original, body,
body with hairstyle 43, body with the reference head). `install` copies it into
`overlays/samurai` and pads the weapon timelines to the body's frame counts.

## Format implementation provenance

`ro_formats.py` is copied from the workspace's original
`rathena/tools/custom_job_sprite/ro_formats.py`, authored for the first Samurai
package. It includes links to primary format implementations consulted by that
writer. No game artwork or stock extracted reference sprites are included in
this tooling directory. The package's generated artwork provenance belongs with
the runtime overlay and its source build metadata.

## Skill icons

`skill_icons.py` draws the eight Samurai skill icons as 24x24 pixel art in the
manner of the Guillotine Cross icons: a dark field inside a wine-and-rose fringe
frame, cream blades and gold sparks. Divinity (Kusanagi) skills use pale blades
and gold light. Cursed (Muramasa) skills use crimson blades and wine-dark auras.
Each icon is a character grid in the script, so a pixel can be changed in place.

```powershell
py tools/samurai_assets/skill_icons.py --output overlays/samurai --preview target/skill-icons.png
```

It writes `data/texture/유저인터페이스/item/sam_<skill>.bmp` (24-bit, magenta
transparent). The client uses these before the stock stand-in icons, in the skill
window, hotkeys and the buff column.

## Buff icons

`buff_icons.py` draws the five Samurai status icons (the buffs of Kusanagi Guard,
Moonlit Reflection, Blade Harmony, Blood Covenant and Muramasa's Curse) the way the
stock 32x32 status TGAs are built. The glyph and frame are one-pixel bright lines.
Every other pixel is coloured by its distance from them: a glow, a mid-tone, then a
translucent fill. Kusanagi buffs use the classic blue theme (as Cloaking and
Two-Hand Quicken do); Muramasa buffs use a crimson one.

```powershell
py tools/samurai_assets/buff_icons.py --output overlays/samurai --preview target/buff-icons.png
```

It writes `data/texture/effect/efst_sam_<skill>.tga`, the name the client looks up
for the status `EFST_SAM_<skill>`.
