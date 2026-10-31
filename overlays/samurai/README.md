# Samurai appearance assets

This overlay supplies the male Samurai body and its separately equipped katanas.
The game job is **Samurai** (4500); **Ember Ronin** remains the asset filename.

## Rendering contract

- The body includes the head and ponytail. Do not add stock heads, hair,
  headgear, garment or shield layers. Its colors are authored into the sprite.
- The body and each weapon ACT contain 104 actions. Directions are S, SW, W, NW, N, NE, E, SE;
  the action index is `group * 8 + direction`.
- Use human player actions: idle 0, walk 1, sit 2, pickup 3, ready 4,
  attack 5, hurt 6, freeze 7, death 8, freeze 9, attack 10/11, cast 12.
- Walking has eight frames per direction at 100 ms per frame. Continuous movement
  retains its animation phase across tiles and turns.
- NE/E/SE mirror the authored NW/W/SW views, including layer positions and weapon
  rotation. Mirroring also reverses costume details.
- Attack groups 5, 10 and 11 correct the cutting-edge side for SW/W/NW/N and
  their reflected directions, while S keeps its clockwise cutting orientation.
  Five winding/overhead poses also align the blade with the hand axis. The shared
  metadata applies to the starter katana, Kusanagi, Muramasa and Tenseiga-no-Hayate.
- The weapon follows the body's action, frame index and delay. Its offsets are
  relative to the actor's ground origin; do not apply a head-anchor offset.
- Katana layers are present in ready and attack groups 4, 5, 10 and 11.
  All other weapon frames have empty layers.
- The katana uses native scale 1.0 in every armed pose. Its curved blade and
  wrapped hilt occupy 58 pixels of the 72-by-10 source canvas, with a main blade
  about 5 pixels wide. The blade was shortened and widened for gameplay zoom; do not
  restore the old 0.75 scale, which made the thin blade hard to read in-game.
- Equipped swords with view 2, 3 or 49 use the starter katana. Custom views 9001
  and 9002 select Kusanagi and Muramasa, and 9003 Tenseiga-no-Hayate. Katar artwork is not supplied.
- Dual wielding draws the left-hand katana from its own `*_offhand` pair. It is the
  same blade on off-hand poses. In the ready stance it is held low behind; when
  attacking, it sweeps opposite to the main blade, a cross slash. The server sends
  the left hand as an item ID. The client matches 900000, 900001 and 900002 rather
  than view numbers, because a view can't tell a sword from a shield (a Buckler is
  also view 2). Other swords in the left hand are not drawn.
- All pairs use SPR 2.1 truecolor RGBA and ACT 2.5. Weapon pairs must match the
  body's action layout and authored grip positions.

## Reference design revision

The artwork matches the reference selected by the user on 2026-10-30: charcoal
wrap clothing with bright red piping and cord knots, a red rope belt, long red
split coat panels, black/red boots and a long dark ponytail. The exact selected
image supplies idle and ready poses; the other actions use that same design.
The previous corrected walk poses provide the movement reference.

Walk cycles use explicit pose selection and fixed anatomical ground origins.
One scale calibration applies to each complete source sheet. Individual frames
are never resized to their silhouette or centered on the leading foot.
The south walk combines selected contact and passing poses from two generated
sheets; its exact sequence is recorded in the build metadata.

The same design is used for idle, ready, walking, sitting, pickup, hurt, casting,
falling and attacking. The three attack groups share one six-frame swing per
authored direction. Female art, independently selectable heads/hair/headgear,
alternate weapon artwork and custom skill effects remain outside this package.

## Sources and rebuilding

The checked-in source artwork, placement metadata and exact generation prompts
are in [tools/samurai_assets/artwork](../../tools/samurai_assets/artwork/README.md).
The [compiler](../../tools/samurai_assets/README.md) samples and packs those
sources; it does not draw poses or infer anatomy.

From the client repository:

```powershell
py tools/samurai_assets/compile.py tools/samurai_assets/artwork/build.json --output target/samurai-review
py -m unittest discover -s tools/samurai_assets -p 'test_*.py'
```

Open `target/samurai-review/preview.html` to inspect all directions, step through
frames and toggle the katana. Contact sheets and the validation report are
decoded from the output binaries. Installation is a separate copy of the
SPR/ACT pairs (body, katana and katana_offhand) into `data/sprite/custom/samurai/`.

[provenance.json](provenance.json) records the runtime file hashes and source
locations. The artwork was generated with the built-in image-generation tool.
Source images retain their generated alpha; nearest-neighbor sampling and ACT
placement are reproducible from the metadata.

All fifteen skills have their own 24x24 icons (`data/texture/유저인터페이스/item/sam_*.bmp`),
pixel art in the manner of the Guillotine Cross icons, drawn by
[skill_icons.py](../../tools/samurai_assets/skill_icons.py). The seven skills of the
Mortal Flame line (Tenseiga's techniques, 7009-7015) are drawn from shapes in Tenseiga's
colours (crimson hilt, magenta runes, violet body, cyan edge), on an indigo field. Their two
buffs (`efst_sam_unchained_will.tga`, `efst_sam_forge_of_ten_souls.tga`) use a matching
theme in [buff_icons.py](../../tools/samurai_assets/buff_icons.py). The line sits in the
skill tree's middle columns, between the Kusanagi and Muramasa branches.

The line's six active skills also have their own effects, drawn in code in Tenseiga's
crimson, cyan and gold (`TenseigaPart` in `crates/ro-render/src/effect.rs`, layered per
skill by `tenseiga_layers` in `crates/ro-client/src/fx.rs`): a crimson crescent cut that
breaks a golden halo (Severing Heaven's Lies), a gold and a crimson cut crossing (Light and
Shadow), a cyan burst of chain shards (Unchained Will), ten souls spiralling up from a
crimson ring (Forge of Ten Souls), a blade of light falling from the sky (Destiny's
Severance) and a crescent of flame flying from the caster through every enemy on its
path, leaving a scorched line behind it (Crescent Gale, `Shape::Crescent`). Crimson is
painted as solid colour with a glowing core over it, so it stays
crimson on pale ground instead of washing out to pink. `--fx skill:<id>` replays one on
the player every 1.5 seconds.

`System/samurai_iteminfo.lua` supplies item 900000, **Samurai Initiate's Katana**.
It references the installed game's stock Katana item/collection icons; those
icons are not redistributed in this overlay.

## Kusanagi and Muramasa

| Item ID | Name | Client view | Role |
| --- | --- | --- | --- |
| 900001 | Kusanagi | 9001 | Divinity: precision, critical strikes and efficient SP use |
| 900002 | Muramasa | 9002 | Cursed: stronger attacks and Bleeding, with reduced Max HP |

Both are Samurai-only one-handed swords, weapon level 4, one slot, required
base level 99, refineable and Neutral element. The server owns their stats and
effects; see [the server specification](../../../rathena/doc/custom_jobs/samurai.md).
Their gameplay design uses the Divinity/Cursed themes from the earlier shared
ChatGPT conversation. The private Discord message was not accessible; no extra
origin story is asserted. The user's gold/blue and red/black images define
their visual identities.

The overlay includes separate equipped SPR/ACT pairs, 32-pixel inventory icons,
128-pixel collection art and floor-drop sprites. Either weapon can be equipped
in either hand. When both are wielded, the client draws the left-hand one from
its off-hand pair (`kusanagi_offhand`, `muramasa_offhand`), crossing the main blade.
The existing stock Muramasa (1164) remains a separate item.

The selected weapon art is the `*-v4.png` pair in the source directory below,
cut out of the user's Kusanagi and Muramasa illustrations and scaled to 58 game
pixels from pommel to tip, with the blades thickened so they read at gameplay
zoom. The item description pictures are the cut-out illustrations themselves.
The v3, v2 and unversioned PNGs and prompts are retained only as historical
references. Weapon sizes and ACT scales remain independent of the
inventory/collection image sizes.

Rebuild their art from the client root:

```powershell
py tools/samurai_assets/compile_weapons.py tools/samurai_assets/artwork/build.json tools/samurai_assets/artwork/legendary_weapons.json --output target/legendary-weapons-v4
```

The staging overlay is `target/legendary-weapons-v4/overlay`; each weapon directory
also contains decoded contacts and an animated preview. The generated PNGs and
exact built-in imagegen prompts are in `tools/samurai_assets/artwork/source/legendary`.
[legendary_provenance.json](legendary_provenance.json) records their source and
runtime hashes and separates prior-revision checks from current validation.
Restart the client after installing updated sprite files. Item stats are
unchanged by this visual revision; `@item 900001 1` and `@item 900002 1` create
the existing weapons for testing.

## Tenseiga-no-Hayate

| Item ID | Name | Client view | Role |
| --- | --- | --- | --- |
| 900003 | Tenseiga-no-Hayate | 9003 | The Mortal Flame: Kusanagi and Muramasa reforged as one two-handed blade |

Tenseiga is a Samurai-only two-handed sword: weapon level 4, two slots, 420 ATK,
required base level 99, refineable, Neutral. In the user's *Twin Flames Saga*,
Hayate shatters Kusanagi and Muramasa at the First Forge and reforges them into a
blade that answers to no god; its item script follows the relic each companion
adds to the forge (see [the server specification](../../../rathena/doc/custom_jobs/samurai.md)).
Masaru, in Prontera next to the Samurai Mentor, forges it from a dual-wielded
Kusanagi and Muramasa, 10 Oridecon and 10 Burning Hearts; `@item 900003 1` also
creates it.

Its art is cut out of the user's Tenseiga illustration by the outline traced in
`tools/samurai_assets/illustration/tenseiga_outline.py` (the phoenix wings and
flames behind it share the guard's reds), scaled to 62 game pixels from pommel to
tip and brightened so the dark blade's cyan edge, red runes and white core read at
gameplay zoom. Being two-handed, it has no off-hand pair (`"offhand": false` in
`legendary_weapons.json`). It shares the katana ready and attack poses.

While it is equipped the server turns on custom hat effect 2000
(`HAT_EF_SAMURAI_TENSEIGA`), which the client draws in code as the Mortal Flame
aura (`AuraStyle::Tenseiga` in `crates/ro-render/src/effect.rs`). It reuses the
level aura's ribbons, ground stars and sparkles with whitened textures, tinted with
the blade's crimson (cyan in a few sparks), over a crimson-black pool so they read as
crimson rather than pink on bright ground. It
takes the place of the level aura, and `/aura` and `/aura2` leave it alone.

## The Twin Flames Saga

The Samurai's ten-chapter quest line lives on the server (rathena
`doc/custom_jobs/twin_flames.md`). The client side is:

- `System/samurai_questinfo.lua`: titles, summaries and descriptions of the saga's quests
  (900101-900192) for the quest log.
- `System/samurai_iteminfo.lua`: the companions' relics and quest items (900010-900021),
  shown with stock item icons.
- `crates/ro-client/src/saga.rs`: the saga's monsters (31900-31917). Their ids are the
  server's own, so each is drawn from a stock monster sprite, recoloured by palette, resized
  and tinted. Kagutsuchi-no-Yami and his servants carry an ember aura, the goddess and hers
  a sun-and-moon one (`AuraStyle::Yami` / `AuraStyle::Amaterasu`).
