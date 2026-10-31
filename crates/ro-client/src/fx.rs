//! Skill effects: which STR animation (data\texture\effect\*.str) plays for a skill, and a
//! cache so each one is parsed and uploaded once.

use std::collections::HashMap;

/// How a skill looks when it lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Visual {
    /// An STR animation (data\texture\effect\<name>.str).
    Str(&'static str),
    /// A code-drawn column of light textured with data\texture\effect\<tex>, tinted.
    Column(&'static str, [f32; 3]),
    /// A sprite effect (data\sprite\이팩트\<name>.spr/.act) played once.
    Sprite(&'static str),
    /// Drawn by the client's own code where the hit is handled (Grimtooth, Soul Breaker...).
    Code,
    /// A Mortal Flame (Tenseiga) skill's own code-drawn effect: see `tenseiga_layers`.
    Tenseiga,
}

/// A texture a code-drawn effect uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FxTexture {
    /// A soft line, bright along its middle (generated).
    Streak,
    /// A soft ring (generated).
    Ring,
    /// A soft round disc (generated): the aura's dark pool.
    Disc,
    /// effect\whitelight.tga: a soft round glow.
    Glow,
    /// effect\alpha_down.tga, drawn as the stock light column.
    Column,
}

/// The layers of a Mortal Flame skill's effect: (piece and whether it is the solid colour
/// layer, texture, duration ms, tint). A `None` piece is the stock light column. Crimson
/// pieces come as a solid layer with a glowing core over it, so they stay crimson on pale
/// ground instead of washing out to pink.
pub fn tenseiga_layers(skill: u16) -> &'static [(Option<(ro_render::TenseigaPart, bool)>, FxTexture, f32, [f32; 3])] {
    use ro_render::TenseigaPart::*;
    const CRIMSON: [f32; 3] = [0.8, 0.02, 0.06];
    const CYAN: [f32; 3] = [0.1, 0.75, 1.0];
    const GOLD: [f32; 3] = [1.0, 0.82, 0.45];
    const WHITE: [f32; 3] = [1.0, 0.92, 0.92];
    match skill {
        // Severing Heaven's Lies: a crimson-to-cyan crescent cut, and the halo it breaks.
        7010 => &[
            (Some((Arc, true)), FxTexture::Streak, 500.0, CRIMSON),
            (Some((Arc, false)), FxTexture::Streak, 500.0, CRIMSON),
            (Some((Halo, true)), FxTexture::Ring, 800.0, GOLD),
            (Some((Spark, false)), FxTexture::Glow, 500.0, WHITE),
        ],
        // Light and Shadow: a gold cut, a crimson cut across it, a flash where they meet.
        7011 => &[
            (Some((CutLight, true)), FxTexture::Streak, 650.0, GOLD),
            (Some((CutLight, false)), FxTexture::Streak, 650.0, GOLD),
            (Some((CutShadow, true)), FxTexture::Streak, 650.0, CRIMSON),
            (Some((CutShadow, false)), FxTexture::Streak, 650.0, CRIMSON),
            (Some((Spark, true)), FxTexture::Glow, 650.0, CRIMSON),
            (Some((Spark, false)), FxTexture::Glow, 650.0, WHITE),
        ],
        // Unchained Will: a cyan column, a ring bursting out, the chain's pieces flying off.
        7012 => &[
            (None, FxTexture::Column, 1200.0, CYAN),
            (Some((Ring, true)), FxTexture::Ring, 900.0, CYAN),
            (Some((Shards, false)), FxTexture::Glow, 900.0, CYAN),
        ],
        // Forge of Ten Souls: ten souls rising round the target over a crimson ring.
        7013 => &[
            (Some((Ring, true)), FxTexture::Ring, 1000.0, CRIMSON),
            (Some((Souls, true)), FxTexture::Glow, 1400.0, CRIMSON),
            (Some((Souls, false)), FxTexture::Glow, 1400.0, CRIMSON),
        ],
        // Destiny's Severance: a blade of light falls from the sky, flashes and sends a
        // crimson ring across the ground.
        7014 => &[
            (Some((Ring, true)), FxTexture::Ring, 1100.0, CRIMSON),
            (Some((Beam, true)), FxTexture::Streak, 900.0, CRIMSON),
            (Some((Beam, false)), FxTexture::Streak, 900.0, CRIMSON),
            (Some((Spark, false)), FxTexture::Glow, 900.0, WHITE),
        ],
        // Crescent Gale, on each enemy the crescent tears through (the crescent itself flies
        // from the caster: `App::crescent_gale`).
        7015 => &[
            (Some((Spark, true)), FxTexture::Glow, 500.0, CRIMSON),
            (Some((Spark, false)), FxTexture::Glow, 500.0, CYAN),
        ],
        _ => &[],
    }
}

/// What plays on the target (or cell) when a skill lands, by rAthena skill id.
pub fn skill_visual(skill: u16) -> Option<Visual> {
    Some(match skill {
        7001 => Visual::Str("sonicblow"),
        7002 | 7003 | 7004 => Visual::Column("alpha_down.tga", [0.4, 0.75, 1.0]),
        7005 | 7006 => Visual::Column("alpha_down.tga", [1.0, 0.2, 0.25]),
        7007 => Visual::Str("brandish"),
        7008 => Visual::Str("bowling"),
        // The Mortal Flame line, in Tenseiga's colours where they are tinted.
        7010..=7015 => Visual::Tenseiga,
        28 => Visual::Column("alpha_down.tga", [0.3, 0.55, 0.3]),
        29 => Visual::Column("agi_up.bmp", [0.7, 1.0, 0.8]),
        30 => Visual::Column("alpha_down.tga", [0.25, 0.25, 0.55]),
        34 => Visual::Column("alpha_down.tga", [0.55, 0.5, 0.25]),
        17 => Visual::Sprite("fireball"),
        86 => Visual::Sprite("waterball"),
        // Bolts: the closest STR hits the game ships.
        14 => Visual::Str("icecrash"),
        15 => Visual::Str("freezing"),
        19 => Visual::Str("firehit1"),
        20 | 84 => Visual::Str("lightning"),
        21 => Visual::Str("thunderstorm"),
        83 => Visual::Str("meteor1"),
        379 | 406 => Visual::Code,
        _ => Visual::Str(skill_str(skill)?),
    })
}

/// Lasting ground effects (ZC_SKILL_ENTRY unit types), looped until they end.
pub fn ground_str(kind: u32) -> Option<&'static str> {
    Some(match kind {
        0x7e => "safetywall",
        0x7f => "firewall",
        0x83 => "sanctuary",
        0x84 => "magnus",
        0x85 => "pneuma1",
        0x88 => "firepillar",
        0x8e => "quagmire",
        0x92 => "venomdust",
        _ => return None,
    })
}

/// The light pillar a drop falls with, by ZC_ITEM_FALL_ENTRY5's drop effect mode: the
/// pillar, its glow on the ground, and its sound. Mode 0 is the client's own choice, the
/// pink pillar of cards.
pub fn drop_pillar(mode: u16) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match mode {
        0 => ("new_dropitem\\dropitem_pink\\dropitem_pink\\dropitem_pink", "new_dropitem\\dropitem_pink\\dropitem_pink_bottom\\dropitem_pink_bottom", "effect\\drop_pink.wav"),
        1 => ("new_dropitem\\dropitem_blue\\dropitem_blue\\dropitem_blue", "new_dropitem\\dropitem_blue\\dropitem_blue_bottom\\dropitem_blue_bottom", "effect\\drop_blue.wav"),
        2 => ("new_dropitem\\dropitem_yellow\\dropitem_yellow\\dropitem_yellow", "new_dropitem\\dropitem_yellow\\dropitem_yellow_bottom\\dropitem_yellow_bottom", "effect\\drop_yellow.wav"),
        3 => ("new_dropitem\\dropitem_purple\\dropitem_purple\\dropitem_purple", "new_dropitem\\dropitem_purple\\dropitem_purple_bottom\\dropitem_purple_bottom", "effect\\drop_purple.wav"),
        4 => ("new_dropitem\\dropitem_green\\dropitem_green\\dropitem_green", "new_dropitem\\dropitem_green\\dropitem_green_bottom\\dropitem_green_bottom", "effect\\drop_green.wav"),
        5 => ("new_dropitem\\dropitem_red\\dropitem_red\\dropitem_red", "new_dropitem\\dropitem_red\\dropitem_red_bottom\\dropitem_red_bottom", "effect\\drop_red.wav"),
        _ => return None,
    })
}

/// Traps (ZC_SKILL_ENTRY unit types) are small models on their cell, not effects: the
/// original's trap RSMs, under data\model\.
pub fn trap_model(kind: u32) -> Option<&'static str> {
    Some(match kind {
        0x8f => "외부소품\\트랩03_3", // Blast Mine
        0x90 => "외부소품\\트랩02",   // Skid Trap
        0x91 => "외부소품\\트랩01",   // Ankle Snare
        0x93 => "외부소품\\트랩03",   // Land Mine
        0x94 => "외부소품\\트랩03_6", // Shockwave Trap
        0x95 => "외부소품\\트랩03_4", // Sandman
        0x96 => "외부소품\\트랩03_5", // Flasher
        0x97 => "외부소품\\트랩03_2", // Freezing Trap
        0x98 => "외부소품\\트랩04",   // Claymore Trap
        0x99 => "외부소품\\트랩05",   // Talkie Box
        0xd2 => "event\\3차트랩_변화01", // Magenta Trap
        0xd3 => "event\\3차트랩_변수01", // Cobalt Trap
        0xd4 => "event\\3차트랩_변지01", // Maize Trap
        0xd5 => "event\\3차트랩_변풍01", // Verdure Trap
        0xd6 => "event\\3차트랩_화01", // Firing Trap
        0xd7 => "event\\3차트랩_수01", // Icebound Trap
        0xd8 => "event\\3차트랩_풍01", // Electric Shocker
        0xd9 => "event\\3차트랩_지01", // Cluster Bomb
        0xe5 => "event\\3차트랩_가시01", // Thorn Trap
        _ => return None,
    })
}

/// STR for skills whose effect is a plain STR animation.
fn skill_str(skill: u16) -> Option<&'static str> {
    Some(match skill {
        6 => "provoke",
        12 => "safetywall",
        16 => "stonecurse",
        18 => "firewall",
        25 => "pneuma1",
        33 => "angelus",
        35 => "cure",
        45 => "concentration",
        50 => "steal_coin",
        52 => "invenom",
        53 => "detoxication",
        56 => "pierce",
        57 => "brandish",
        58 => "spearstab",
        59 => "spearboomerang",
        60 => "twohand",
        61 => "autocounter",
        62 => "bowling",
        66 => "impositio",
        67 => "suffragium",
        68 => "aspersio",
        69 => "benedictio",
        70 => "sanctuary",
        71 => "slowp",
        72 => "recovery",
        73 => "kyrie",
        74 => "magnificat",
        75 => "gloria",
        76 => "lexdivina",
        78 => "lexaeterna",
        79 => "magnus",
        80 => "firepillar",
        89 => "stormgust",
        92 => "quagmire",
        111 | 459 => "adrenaline",
        112 => "weaponperfection",
        114 => "maximizepower",
        115 => "skidtrap",
        116 => "landmine",
        119 => "sandman",
        121 => "freezing",
        122 => "blastmine",
        123 => "claymore",
        136 => "sonicblow",
        139 => "poisonreact",
        140 => "venomdust",
        141 => "venomsplasher",
        153 => "cartrevolution",
        // NPC_SELFDESTRUCTION (Marine Sphere, @nuke): the blast, not suicide.str's 3 s charge.
        173 => "sui_explosion",
        215 => "strip_weapon",
        216 => "strip_shield",
        217 => "strip_armor",
        218 => "strip_helm",
        250 => "shield_charge",
        255 => "devotion",
        _ => return None,
    })
}

/// Effect for a ZC_NOTIFY_EFFECT id.
pub fn misc_effect(effect: u32) -> Option<&'static str> {
    match effect {
        0 => Some("h_levelup"),
        1 => Some("joblvup"),
        _ => None,
    }
}

/// How an RSW ambient effect (its id is an EF_* id) looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MapVisual {
    /// One of these STRs, chosen at random, fired once per emit.
    Str(&'static [&'static str]),
    /// A sprite effect looping in place for as long as the map is loaded, raised this many
    /// world units above its RSW position.
    Sprite(&'static str, f32),
    /// Code-drawn drifting motes (fireflies).
    Motes,
}

pub fn map_effect(id: i32) -> Option<MapVisual> {
    Some(match id {
        45 => MapVisual::Motes,
        // EF_TORCH: the original raises it 10 units (m_deltaPos2.y = -10), onto the brazier
        // it burns in (Payon Cave's stand is 8.7 tall).
        47 => MapVisual::Sprite("torch_01", 10.0),
        109 => MapVisual::Str(&["bubble1", "bubble2", "bubble3", "bubble4"]),
        _ => return None,
    })
}

/// Emitters re-fire only while the player is this close (cells), as the original client.
pub const MAP_FX_RANGE: f32 = 25.0;

/// An RSW effect emitter on the current map.
pub struct MapEmitter {
    pub visual: Option<MapVisual>,
    pub pos: glam::Vec3,
    /// Milliseconds between emits (the RSW value × 10).
    pub every_ms: f32,
    /// Countdown to the next emit; sprites use it as their animation clock.
    pub clock_ms: f32,
    /// Emits so far (varies which STR plays).
    pub fired: u32,
}

impl MapEmitter {
    /// `index` staggers emitters so a field of them doesn't fire in step.
    pub fn new(id: i32, pos: glam::Vec3, delay: f32, index: usize) -> Self {
        let every_ms = (delay * 10.0).max(200.0);
        Self { visual: map_effect(id), pos, every_ms, clock_ms: (index as f32 * 397.0) % every_ms, fired: index as u32 }
    }
}

/// The map's particle emitters: RSW effect objects with id 974 (EF_EMITTER, `positions` in
/// RSW order) paired with the entries of `effecttool\<map>.lub` (the Nth object takes entry
/// N). Missing fields take BrowEdit's defaults. `animated`: the id 1073 objects instead
/// (EF_ANIMATED_EMITTER, `_<map>_animatedEmitterInfo`), whose particles play built-in frame
/// sets rather than one texture.
pub fn map_emitters(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, map: &str, positions: &[glam::Vec3], animated: bool) -> Vec<ro_render::Emitter> {
    if positions.is_empty() {
        return vec![];
    }
    let data = [format!("data\\luafiles514\\lua files\\effecttool\\{map}.lub"), format!("data\\lua files\\effecttool\\{map}.lub")]
        .iter()
        .find_map(|p| vfs.read(p).ok().flatten());
    let Some(data) = data else { return vec![] };
    let chunk = match ro_formats::lub::eval(&data) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(map, "effecttool lub: {e:#}");
            return vec![];
        }
    };
    let kind = if animated { "animatedEmitterInfo" } else { "emitterInfo" };
    let Some(table) = chunk.globals.get(&format!("_{}_{kind}", map.replace('@', ""))) else { return vec![] };
    let frames: Vec<Vec<ro_render::TexId>> = if animated {
        [&["shockwave_b", "shockwave_c", "shockwave_d", "shockwave_e"][..], &["plazma_a", "plazma_b", "plazma_c"][..]]
            .iter()
            .map(|set| set.iter().filter_map(|n| fx.texture_dyn(r, vfs, &format!("effect\\{n}.bmp"))).collect())
            .collect()
    } else {
        vec![]
    };
    let num = |v: &ro_formats::lub::Value| if let ro_formats::lub::Value::Number(n) = v { Some(*n as f32) } else { None };
    let mut out = vec![];
    for (i, &pos) in positions.iter().enumerate() {
        let Some(e) = chunk.field_index(table, i as u32) else { continue };
        let list = |key: &str| -> Vec<f32> { chunk.field(e, key).map(|t| chunk.entries(t).iter().filter_map(|(_, v)| num(v)).collect()).unwrap_or_default() };
        let v3 = |key: &str, d: [f32; 3]| {
            let l = list(key);
            if l.len() >= 3 { [l[0], l[1], l[2]] } else { d }
        };
        let v2 = |key: &str, d: [f32; 2]| {
            let l = list(key);
            match l.len() {
                0 => d,
                1 => [l[0], l[0]],
                _ => [l[0], l[1]],
            }
        };
        let one = |key: &str, d: f32| list(key).first().copied().unwrap_or(d);
        let color = list("color");
        let color = if color.len() >= 4 { [color[0] / 255.0, color[1] / 255.0, color[2] / 255.0, color[3] / 255.0] } else { [1.0; 4] };
        let texture = match chunk.field(e, "texture") {
            Some(ro_formats::lub::Value::Str(s)) => s.replace("\\\\", "\\"),
            _ => "smoke2.bmp".to_string(),
        };
        let Some(tex) = fx.texture_dyn(r, vfs, &texture) else { continue };
        out.push(ro_render::Emitter {
            pos,
            def: ro_render::EmitterDef {
                dir1: v3("dir1", [0.0; 3]),
                dir2: v3("dir2", [0.0; 3]),
                gravity: v3("gravity", [0.0; 3]),
                radius: v3("radius", [0.0; 3]),
                color,
                rate: v2("rate", [5.0, 15.0]),
                size: v2("size", [3.0, 8.0]),
                life: v2("life", [1.0, 5.0]),
                speed: one("speed", 0.5),
                src: one("srcmode", 10.0) as u8,
                dst: one("destmode", 2.0) as u8,
                max: one("maxcount", 30.0).clamp(0.0, 500.0) as u32,
                zenable: one("zenable", 1.0) != 0.0,
                linear: false,
                grow: 1.0,
                billboard: one("billboard_off", 0.0) == 0.0,
                rotate: v3("rotate_angle", [0.0; 3]),
                tex,
                frames: frames.clone(),
            },
        });
    }
    out
}

/// Coloured map torches (EF_TORCH_RED 690, EF_TORCH_GREEN 691, EF_TORCH_PURPLE 696): the
/// original draws them in code as a camera-facing flame cycling `effect\torch_<colour>01..13`
/// (black-keyed, added). Each becomes one never-ending particle on the shared particle system,
/// its flame's foot on the RSW position like the plain torch sprite.
pub fn map_torches(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, effects: &[(i32, glam::Vec3)]) -> Vec<ro_render::Emitter> {
    const HALF: f32 = 4.0;
    let mut sets: HashMap<i32, Option<Vec<ro_render::TexId>>> = HashMap::new();
    let mut out = vec![];
    for &(id, pos) in effects {
        let colour = match id {
            690 => "red",
            691 => "green",
            696 => "violet",
            _ => continue,
        };
        let frames = sets.entry(id).or_insert_with(|| {
            let f: Vec<_> = (1..=13).filter_map(|n| fx.texture_dyn(r, vfs, &format!("effect\\torch_{colour}{n:02}.bmp"))).collect();
            (!f.is_empty()).then_some(f)
        });
        let Some(frames) = frames.clone() else { continue };
        out.push(ro_render::Emitter {
            // The particle system lifts emitters 10 units; set the flame's foot on the spot.
            pos: pos + glam::Vec3::new(0.0, HALF - 10.0, 0.0),
            def: ro_render::EmitterDef {
                dir1: [0.0; 3],
                dir2: [0.0; 3],
                gravity: [0.0; 3],
                radius: [0.0; 3],
                color: [1.0; 4],
                rate: [0.001, 0.001],
                size: [HALF, HALF],
                life: [1.0e6, 1.0e6],
                speed: 0.0,
                src: 2,
                dst: 2,
                max: 1,
                zenable: true,
                linear: true,
                grow: 1.0,
                billboard: true,
                rotate: [0.0; 3],
                tex: frames[0],
                frames: vec![frames],
            },
        });
    }
    out
}

/// Chimney smoke (EF_SMOKE, 44): the original takes the single image of sprite 굴뚝연기 and
/// animates it in code. Following roBrowser's table, a puff leaves every second, drifts up
/// about 20 units and a little sideways, grows to four times its size and fades over 10 s
/// (at most 0.8 opaque).
pub fn map_smoke(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, effects: &[(i32, glam::Vec3)]) -> Vec<ro_render::Emitter> {
    let spots: Vec<glam::Vec3> = effects.iter().filter(|e| e.0 == 44).map(|e| e.1).collect();
    if spots.is_empty() {
        return vec![];
    }
    let Some(tex) = fx.sprite_texture(r, vfs, "굴뚝연기") else { return vec![] };
    spots
        .into_iter()
        .map(|pos| ro_render::Emitter {
            // The particle system lifts emitters 10 units; start at the chimney itself.
            pos: pos - glam::Vec3::new(0.0, 10.0, 0.0),
            def: ro_render::EmitterDef {
                // RO y-down: negative y rises.
                dir1: [0.0, -2.0, 0.0],
                dir2: [0.3, -2.0, 0.3],
                gravity: [0.0; 3],
                radius: [0.3, 0.0, 0.3],
                color: [1.0, 1.0, 1.0, 0.8],
                rate: [1.0, 1.0],
                size: [2.5, 3.0],
                life: [10.0, 10.0],
                speed: 1.0,
                src: 5,
                dst: 6,
                max: 10,
                zenable: true,
                billboard: true,
                rotate: [0.0; 3],
                tex,
                frames: vec![],
                linear: false,
                grow: 4.0,
            },
        })
        .collect()
}

/// EF_WATERFALL_ANI (821): spray churning at the foot of a falls, drifting up as mist. The
/// first parameter is the width of the churn (70 or 100).
pub fn map_spray(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, effects: &[(i32, glam::Vec3, [f32; 4])]) -> Vec<ro_render::Emitter> {
    let spots: Vec<(glam::Vec3, f32)> = effects.iter().filter(|e| e.0 == 821).map(|e| (e.1, e.2[0].max(20.0))).collect();
    if spots.is_empty() {
        return vec![];
    }
    let Some(tex) = fx.sprite_texture(r, vfs, "굴뚝연기") else { return vec![] };
    spots
        .into_iter()
        .map(|(pos, width)| ro_render::Emitter {
            pos: pos - glam::Vec3::new(0.0, 10.0, 0.0),
            def: ro_render::EmitterDef {
                dir1: [-0.6, -1.2, -0.6],
                dir2: [0.6, -2.4, 0.6],
                gravity: [0.0; 3],
                radius: [width * 0.2, 1.0, width * 0.2],
                color: [0.95, 0.98, 1.0, 0.35],
                rate: [4.0, 6.0],
                size: [6.0, 9.0],
                life: [2.5, 4.0],
                speed: 4.0,
                src: 5,
                dst: 6,
                max: 24,
                zenable: true,
                billboard: true,
                rotate: [0.0; 3],
                tex,
                frames: vec![],
                linear: false,
                grow: 2.5,
            },
        })
        .collect()
}

/// Waterfalls (EF_WATERFALL 349 .. EF_WATERFALL_SMALL_T2_90 356): large or small, plain
/// (`waterfall1x.tga`) or dark (`waterfall3x.tga`), along x or turned 90°.
pub fn map_waterfalls(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, effects: &[(i32, glam::Vec3, [f32; 4])]) -> Vec<ro_render::Waterfall> {
    let mut out = vec![];
    for &(id, pos, _) in effects {
        let (width, yaw, dark) = match id {
            349..=356 => {
                let k = id - 349;
                (if k & 2 != 0 { 18.0 } else { 36.0 }, if k & 1 != 0 { 90f32.to_radians() } else { 0.0 }, k >= 4)
            }
            _ => continue,
        };
        let set = if dark { 3 } else { 1 };
        let tex: Vec<_> = (1..=3).filter_map(|n| fx.texture_dyn(r, vfs, &format!("effect\\waterfall{set}{n}.tga"))).collect();
        let Ok(textures) = <[ro_render::TexId; 3]>::try_from(tex) else { continue };
        out.push(ro_render::Waterfall { pos, width, yaw, textures });
    }
    out
}

/// Map glows: EF_GLOW1 (693) yellow, EF_GLOW2 (694) green, EF_GLOW4 (695) a turning green
/// ray burst, EF_GLOW11 (701) a large blue sphere, EF_GLOW12 (702) a small violet one and
/// EF_MAP_GREENLIGHT (710) a larger green sphere, each `effect\glowNN.bmp` drawn additively.
/// The first parameter is the diameter.
pub fn map_glows(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, effects: &[(i32, glam::Vec3, [f32; 4])]) -> Vec<ro_render::Glow> {
    let mut out = vec![];
    for (i, &(id, pos, params)) in effects.iter().enumerate() {
        let (file, color, scale, spin) = match id {
            693 => ("glow01", [1.0, 1.0, 1.0, 0.85], 1.0, 0.0),
            694 => ("glow02", [1.0, 1.0, 1.0, 0.85], 1.0, 0.0),
            695 => ("glow04", [1.0, 1.0, 1.0, 0.9], 1.2, 0.6),
            701 => ("glow11", [0.55, 0.75, 1.0, 0.9], 1.6, 0.0),
            702 => ("glow12", [1.0, 1.0, 1.0, 0.85], 1.0, 0.0),
            710 => ("glow11", [0.5, 1.0, 0.6, 0.9], 1.8, 0.0),
            _ => continue,
        };
        let Some(tex) = fx.texture_dyn(r, vfs, &format!("effect\\{file}.bmp")) else { continue };
        let size = if params[0] > 0.0 { params[0] } else { 10.0 };
        let phase = (i as f32 * 2.399).rem_euclid(std::f32::consts::TAU);
        out.push(ro_render::Glow { pos, radius: size * 0.5 * scale, color, tex, spin, phase });
    }
    out
}

/// The map's wind effects: RSW objects with id 2343 (EF_WIND_EFFECT, `positions` in RSW
/// order) paired with `_<map>_windEffectInfo` entries the same way as emitters.
pub fn map_winds(fx: &mut FxLib, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, map: &str, positions: &[glam::Vec3]) -> Vec<ro_render::Wind> {
    if positions.is_empty() {
        return vec![];
    }
    let Some(data) = [format!("data\\luafiles514\\lua files\\effecttool\\{map}.lub"), format!("data\\lua files\\effecttool\\{map}.lub")].iter().find_map(|p| vfs.read(p).ok().flatten()) else {
        return vec![];
    };
    let Ok(chunk) = ro_formats::lub::eval(&data) else { return vec![] };
    let Some(table) = chunk.globals.get(&format!("_{}_windEffectInfo", map.replace('@', ""))) else { return vec![] };
    use ro_formats::lub::Value;
    let num = |v: Option<&Value>| if let Some(Value::Number(n)) = v { Some(*n as f32) } else { None };
    let mut out = vec![];
    for (i, &pos) in positions.iter().enumerate() {
        let Some(e) = chunk.field_index(table, i as u32) else { continue };
        let list = |key: &str| -> Vec<f32> { chunk.field(e, key).map(|t| chunk.entries(t).iter().filter_map(|(_, v)| num(Some(v))).collect()).unwrap_or_default() };
        let one = |key: &str, d: f32| num(chunk.field(e, key)).unwrap_or(d);
        let texture = match chunk.field(e, "texture") {
            Some(Value::Str(s)) => s.replace("\\\\", "\\"),
            _ => continue,
        };
        let Some(tex) = fx.texture_dyn(r, vfs, &texture) else { continue };
        // Stored ARGB (BrowEdit: "the lub color format is argb").
        let c = list("color");
        let color = if c.len() >= 4 { [c[1] / 255.0, c[2] / 255.0, c[3] / 255.0, c[0] / 255.0] } else { [1.0; 4] };
        let rot = list("rotateVector");
        let count = one("particleNum", 0.0).max(0.0) as u32;
        let seeds = (0..count).map(|k| {
            let h = |g: f32| ((k as f32 * 12.9898 + g * 78.233 + i as f32 * 3.7).sin() * 43758.547).fract().abs();
            (h(1.0), h(2.0))
        }).collect();
        out.push(ro_render::Wind {
            pos,
            count,
            color,
            radius: one("radius", 5.0),
            thickness: one("thickness", 1.0),
            height: one("height", 0.0),
            speed: one("speed", 1.0),
            full_angle: one("fullAngle", 360.0),
            rotate: [rot.first().copied().unwrap_or(0.0), rot.get(1).copied().unwrap_or(0.0)],
            src: one("srcMode", 5.0) as u8,
            dst: one("destMode", 2.0) as u8,
            tex,
            seeds,
        });
    }
    out
}

#[derive(Default)]
pub struct FxLib {
    /// Textures by path under data\texture\ (particle emitters name them at run time).
    textures_dyn: HashMap<String, Option<ro_render::TexId>>,
    cache: HashMap<String, Option<ro_render::FxId>>,
    textures: HashMap<&'static str, Option<ro_render::TexId>>,
    /// Effect textures turned to white light (see `texture_white`).
    textures_white: HashMap<&'static str, Option<ro_render::TexId>>,
    /// Textures made in code (`FxTexture::Streak`, `Ring`).
    generated: HashMap<FxTexture, ro_render::TexId>,
    sprites: HashMap<&'static str, Option<ro_render::SheetId>>,
    /// STR paths under effect that sit in subfolders (see `index`).
    strs: Vec<String>,
}

impl FxLib {
    /// Indexes the STR effects kept in per-skill folders (effect\savage_impact\...\x.str),
    /// so skills without a hand-mapped effect can find their own by name.
    pub fn index(&mut self, vfs: &ro_vfs::Vfs) {
        const ROOT: &str = r"data\texture\effect\";
        self.strs = vfs
            .names()
            .filter_map(|n| n.to_ascii_lowercase().strip_prefix(ROOT)?.strip_suffix(".str").map(str::to_string))
            .filter(|n| n.contains('\\') && !n.starts_with(r"mineffect\"))
            .collect();
        self.strs.sort();
        tracing::info!(count = self.strs.len(), "indexed skill effect folders");
    }

    /// The best STR (path under effect\, no extension) for a skill's short name, e.g.
    /// "savage_impact" -> savage_impact\savage_impact\savage_impact. Cast, ground and
    /// counter pieces are skipped in favour of the main or hit animation.
    pub fn find(&self, short: &str) -> Option<&str> {
        let joined = short.replace('_', "");
        let keys = [short.to_string(), joined.clone(), format!("new_{short}"), format!("new_{joined}")];
        let score = |p: &str| -> Option<u32> {
            let parts: Vec<&str> = p.split('\\').collect();
            let stem = parts.last()?.trim_start_matches("min_");
            let key = keys.iter().find(|k| parts.iter().any(|c| c == k))?;
            let mut s = 100;
            if stem == key {
                s = 0;
            } else if stem.ends_with("_hit") || stem.ends_with("_hit_01") {
                s = 10;
            }
            if ["cast", "bottom", "count", "clock", "ground", "_end", "_loop"].iter().any(|w| stem.contains(w)) {
                s += 50;
            }
            Some(s + parts.len() as u32)
        };
        self.strs.iter().filter_map(|p| Some((score(p)?, p.as_str()))).min().map(|(_, p)| p)
    }

    /// A texture from data\texture\effect\ for code-drawn effects.
    pub fn texture(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &'static str) -> Option<ro_render::TexId> {
        *self.textures.entry(name).or_insert_with(|| {
            let path = format!("data\\texture\\effect\\{name}");
            let img = vfs.read(&path).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &path).ok());
            if img.is_none() {
                tracing::warn!(%path, "effect texture missing");
            }
            Some(r.add_effect_texture(&img?))
        })
    }

    /// An effect texture with its colour taken out (each pixel as bright as its brightest
    /// channel), for code-drawn effects that tint it themselves.
    pub fn texture_white(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &'static str) -> Option<ro_render::TexId> {
        *self.textures_white.entry(name).or_insert_with(|| {
            let path = format!("data\\texture\\effect\\{name}");
            let mut img = vfs.read(&path).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &path).ok())?;
            for p in img.pixels.chunks_exact_mut(4) {
                let v = p[0].max(p[1]).max(p[2]);
                (p[0], p[1], p[2]) = (v, v, v);
            }
            Some(r.add_effect_texture(&img))
        })
    }

    /// A code-drawn effect's texture: generated, or an effect texture from the game.
    pub fn fx_texture(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, which: FxTexture) -> Option<ro_render::TexId> {
        let (size, alpha): (u32, fn(f32, f32) -> f32) = match which {
            FxTexture::Glow => return self.texture(r, vfs, "whitelight.tga"),
            FxTexture::Column => return self.texture(r, vfs, "alpha_down.tga"),
            // Bright along v = 0.5, fading to nothing at both edges.
            FxTexture::Streak => (64, |_, v| (-((v - 0.5) / 0.2).powi(2)).exp()),
            // Solid in the middle, fading out over the outer three quarters.
            FxTexture::Disc => (64, |u, v| {
                let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
                1.0 - ((r - 0.25) / 0.75).clamp(0.0, 1.0).powi(2) * (3.0 - 2.0 * ((r - 0.25) / 0.75).clamp(0.0, 1.0))
            }),
            // A ring at 78% of the radius with a faint wider glow.
            FxTexture::Ring => (128, |u, v| {
                let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
                ((-((r - 0.78) / 0.07).powi(2)).exp() + 0.25 * (-((r - 0.78) / 0.25).powi(2)).exp()).min(1.0)
            }),
        };
        if let Some(t) = self.generated.get(&which) {
            return Some(*t);
        }
        let mut pixels = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let a = alpha((x as f32 + 0.5) / size as f32, (y as f32 + 0.5) / size as f32);
                pixels.extend([255, 255, 255, (a * 255.0).round() as u8]);
            }
        }
        let t = r.add_effect_texture(&ro_formats::image::Rgba { width: size, height: size, pixels });
        self.generated.insert(which, t);
        Some(t)
    }

    /// A texture by path under data\texture\ (or data\texture\effect\ when not found there).
    pub fn texture_dyn(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &str) -> Option<ro_render::TexId> {
        if let Some(t) = self.textures_dyn.get(&name.to_ascii_lowercase()) {
            return *t;
        }
        let img = [format!("data\\texture\\{name}"), format!("data\\texture\\effect\\{name}")]
            .into_iter()
            .find_map(|p| vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok()));
        if img.is_none() {
            tracing::warn!(%name, "particle texture missing");
        }
        let t = img.map(|i| r.add_effect_texture(&i));
        self.textures_dyn.insert(name.to_ascii_lowercase(), t);
        t
    }

    /// The first image of sprite effect `name` (data\sprite\이팩트\), as an effect texture.
    pub fn sprite_texture(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &str) -> Option<ro_render::TexId> {
        let key = format!("spr:{name}");
        if let Some(t) = self.textures_dyn.get(&key) {
            return *t;
        }
        let spr = vfs.read(&format!("data\\sprite\\이팩트\\{name}.spr")).ok().flatten().and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
        // Straight alpha (the particle blend applies it); images() would premultiply.
        let t = spr.and_then(|s| s.rgba.first().cloned().or_else(|| s.images(None).0.into_iter().next()));
        let t = t.map(|img| r.add_effect_texture(&img));
        self.textures_dyn.insert(key, t);
        t
    }

    /// A sprite effect from data\sprite\이팩트\.
    pub fn sprite(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &'static str) -> Option<ro_render::SheetId> {
        *self.sprites.entry(name).or_insert_with(|| {
            let base = format!("data\\sprite\\이팩트\\{name}");
            let spr = vfs.read(&format!("{base}.spr")).ok().flatten().and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
            let act = vfs.read(&format!("{base}.act")).ok().flatten().and_then(|d| ro_formats::act::Act::parse(&d).ok());
            match (spr, act) {
                (Some(spr), Some(act)) => Some(r.add_sheet(&spr, act, None)),
                _ => {
                    tracing::warn!(%base, "sprite effect missing");
                    None
                }
            }
        })
    }

    /// The uploaded effect, loading it on first use (None if missing or unreadable).
    pub fn get(&mut self, r: &mut ro_render::Renderer, vfs: &ro_vfs::Vfs, name: &str) -> Option<ro_render::FxId> {
        *self.cache.entry(name.to_string()).or_insert_with(|| {
            let path = format!("data\\texture\\effect\\{name}.str");
            let str = vfs
                .read(&path)
                .ok()
                .flatten()
                .and_then(|d| ro_formats::strm::Str::parse(&d).inspect_err(|e| tracing::warn!(%path, "{e:#}")).ok())?;
            let images = str
                .layers
                .iter()
                .map(|l| {
                    l.textures
                        .iter()
                        .map(|t| {
                            // Effects in subfolders name textures beside themselves.
                            let dir = name.rsplit_once('\\').map_or("", |(d, _)| d);
                            [format!("data\\texture\\effect\\{dir}\\{t}"), format!("data\\texture\\effect\\{t}")]
                                .into_iter()
                                .find_map(|p| vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok()))
                        })
                        .collect()
                })
                .collect();
            tracing::debug!(%path, layers = str.layers.len(), "loaded effect");
            Some(r.add_fx(str, images))
        })
    }
}

/// rAthena `EF_*` effects the original draws in code, shown with the matching skill's effect.
pub fn ef_skill(effect: u32) -> Option<u16> {
    Some(match effect {
        312 | 313 => 28,   // HEAL, HEAL2: Heal
        226 => 254,        // GRANDCROSS
        18 => 50,          // STEAL
        16 => 5,           // BASH
        53 => 52,          // POISONHIT: Envenom
        125 => 138,        // ENCHANTPOISON
        215 => 196,        // SUMMONSLAVE
        106 => 122,        // BLASTMINEBOMB
        72 => 261,         // SPHERE: Call Spirits
        99 => 120,         // FLASHER
        496 => 249,        // GUARD2: Auto Guard
        9 | 60 => 45,      // ENHANCE, LOCKON: Improve Concentration
        71 => 29,          // CONE: Increase AGI
        _ => return None,
    })
}

pub fn script_effect(effect: u32) -> Option<&'static str> {
    ef_str(effect)
}

/// rAthena `EF_*` effect ids whose name matches an STR in the client data (generated from
/// src/map/script.hpp against data\texture\effect).
fn ef_str(effect: u32) -> Option<&'static str> {
    Some(match effect {
        21 => "detoxication",
        23 => "stonecurse",
        25 => "firewall",
        30 => "thunderstorm",
        41 => "angelus",
        49 => "firehit",
        52 => "windhit",
        64 => "arrowshot",
        65 => "invenom",
        66 => "cure",
        67 => "provoke",
        68 => "mvp",
        69 => "skidtrap",
        75 => "gloria",
        76 => "magnificat",
        77 => "resurrection",
        78 => "recovery",
        81 => "pierce",
        83 => "sanctuary",
        84 => "impositio",
        85 => "lexaeterna",
        86 => "aspersio",
        87 => "lexdivina",
        88 => "suffragium",
        89 => "stormgust",
        90 => "lord",
        91 => "benedictio",
        95 => "quagmire",
        96 => "firepillar",
        97 => "firepillarbomb",
        101 => "repairweapon",
        102 => "crashearth",
        105 => "blastmine",
        107 => "claymore",
        108 => "freezing",
        109 => "bubble",
        110 => "gaspush",
        112 => "kyrie",
        113 => "magnus",
        121 => "sonicblow",
        124 => "venomdust",
        126 => "poisonreact",
        131 => "autocounter",
        133 => "freeze",
        134 => "freezed",
        135 => "icecrash",
        139 => "sandman",
        144 => "brandish2",
        145 => "shockwave",
        146 => "shockwavehit",
        147 => "earthhit",
        152 => "holyhit",
        153 => "concentration",
        158 => "joblvup",
        163 => "sakura",
        169 => "energycoat",
        170 => "cartrevolution",
        181 => "mentalbreak",
        183 => "sui_explosion",
        185 => "suicide",
        199 => "pong",
        213 => "deffender",
        214 => "keeping",
        248 => "providence",
        251 => "devotion",
        311 => "loud",
        335 => "poison",
        371 => "angel",
        372 => "devil",
        507 => "mapae",
        508 => "itempokjuk",
        603 => "firehit2",
        689 => "dust",
        708 => "storm_min",
        813 => "enervation",
        920 => "firewall2",
        959 => "poison_mist",
        960 => "eraser_cutter",
        964 => "lava_slide",
        965 => "sonic_claw",
        998 => "red_cross",
        1031 => "invincibleoff2",
        1034 => "moonstar",
        1035 => "strangelights",
        1040 => "gc_darkcrow",
        1042 => "all_full_throttle",
        1043 => "sr_flashcombo",
        1044 => "rk_luxanima",
        1046 => "so_elemental_shield",
        1047 => "ab_offertorium",
        1048 => "wl_telekinesis_intense",
        1049 => "gn_illusiondoping",
        1050 => "nc_magma_eruption",
        1057 => "ab_offertorium_ring",
        1064 => "fire_rain",
        1078 => "s_storm",
        1084 => "fstone",
        1085 => "qscaraba",
        1086 => "ljosalfar",
        1089 => "maple_falls",
        1091 => "magical_feather",
        1093 => "gift_of_snow",
        1095 => "time_accessory",
        1123 => "su_grooming",
        1124 => "su_chattering",
        1133 => "firedance",
        1219 => "sj_document",
        1220 => "falling_star",
        1229 => "newmoon_kick",
        1230 => "fullmoon_kick",
        _ => return None,
    })
}

/// Hat effects (ZC_EQUIPMENT_EFFECT ids) -> STR under data\texture\effect, from
/// hateffectinfo.lub's hatEffectTable. Its entries are keyed by id (`[HatEFID.X]`, as the
/// English translation's copy has them, defining HatEFID itself) or by name, looked up in
/// hateffectids.lub.
pub fn hat_effects(vfs: &ro_vfs::Vfs) -> HashMap<u16, &'static str> {
    const DIR: &str = r"data\luafiles514\lua files\hateffectinfo";
    let load = |f: &str| vfs.read(&format!(r"{DIR}\{f}.lub")).ok().flatten().and_then(|d| ro_formats::lub::eval(&d).ok());
    let Some(info) = load("hateffectinfo") else { return HashMap::new() };
    let Some(info_tbl) = info.globals.get("hatEffectTable") else { return HashMap::new() };
    let ids = load("hateffectids");
    let id_of = |name: &str| -> Option<f64> {
        let ids = ids.as_ref()?;
        ids.entries(ids.globals.get("HatEFID")?).iter().find(|(k, _)| k.as_str() == Some(name)).and_then(|(_, v)| v.as_number())
    };
    let mut out = HashMap::new();
    for (key, t) in info.entries(info_tbl) {
        let Some(id) = key.as_number().or_else(|| key.as_str().and_then(id_of)) else { continue };
        // Entries with only a hatEffectID are drawn by the original in code.
        let Some(file) = info.field(t, "resourceFileName").and_then(|v| v.as_str()) else { continue };
        let short: &'static str = Box::leak(file.trim_end_matches(".str").trim_end_matches(".STR").to_string().into_boxed_str());
        out.insert(id as u16, short);
    }
    tracing::info!(count = out.len(), "hat effects");
    out
}
