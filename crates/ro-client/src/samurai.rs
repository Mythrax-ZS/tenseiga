//! Samurai's shared appearance and client metadata. Combat rules live on the server.

use std::path::{Path, PathBuf};

pub const JOB: i16 = 4500;
pub const SPRITE: &str = "samurai";
pub const BODY: &str = r"data\sprite\custom\samurai\ember_ronin";
pub const KATANA: &str = r"data\sprite\custom\samurai\katana";
pub const KUSANAGI: &str = r"data\sprite\custom\samurai\kusanagi";
pub const MURAMASA: &str = r"data\sprite\custom\samurai\muramasa";
/// Two-handed: Kusanagi and Muramasa forged into one blade, so it has no off-hand pair.
pub const TENSEIGA: &str = r"data\sprite\custom\samurai\tenseiga";
pub const KATANA_OFFHAND: &str = r"data\sprite\custom\samurai\katana_offhand";
pub const KUSANAGI_OFFHAND: &str = r"data\sprite\custom\samurai\kusanagi_offhand";
pub const MURAMASA_OFFHAND: &str = r"data\sprite\custom\samurai\muramasa_offhand";

/// The supplied weapon ACT is aligned to this body; stock weapon ACTs are not.
pub fn weapon(view: u32) -> Option<&'static str> {
    match view {
        // 49 is the stock Slayer/Claymore/Muramasa sprite alias in current itemInfo.
        2 | 3 | 49 => Some(KATANA),
        9001 => Some(KUSANAGI),
        9002 => Some(MURAMASA),
        9003 => Some(TENSEIGA),
        _ => None,
    }
}

/// A katana in the left hand (dual wielding): the same blade on its own cross-slash poses,
/// sweeping opposite to the main-hand blade. The left hand arrives as an item id, and a view
/// can't tell a sword from a shield (a Buckler is view 2, like the katana), so match item ids.
pub fn offhand(item: u32) -> Option<&'static str> {
    match item {
        900000 => Some(KATANA_OFFHAND),
        900001 => Some(KUSANAGI_OFFHAND),
        900002 => Some(MURAMASA_OFFHAND),
        _ => None,
    }
}

/// Find the tracked asset overlay from a packaged executable or a repository checkout.
/// Existing local.toml files do not need editing when this content is installed.
pub fn overlay_path() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok();
    let working = std::env::current_dir().ok();
    let roots = executable.as_deref().and_then(Path::parent).into_iter()
        .chain(working.as_deref())
        .chain(std::iter::once(Path::new(env!("CARGO_MANIFEST_DIR"))));
    roots.flat_map(|p| p.ancestors().take(4))
        .map(|p| p.join("overlays/samurai"))
        .find(|p| p.join("data/sprite/custom/samurai/ember_ronin.spr").is_file())
}

pub struct Skill {
    pub id: u16,
    pub internal: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    /// An existing client icon; a custom icon with the skill's own name takes priority.
    pub icon: &'static str,
    pub slot: u32,
}

pub const SKILLS: &[Skill] = &[
    Skill { id: 7001, internal: "SAM_HEAVENS_SEVERANCE", name: "Heaven's Severance", description: "Divinity / Kusanagi. Strike one enemy for 1700/2600/3500/4400/5300% ATK (x base level / 100), with 25/30/35/40/45 percentage points of additional critical chance.", icon: "kn_bowlingbash", slot: 0 },
    Skill { id: 7002, internal: "SAM_KUSANAGI_GUARD", name: "Kusanagi Guard", description: "Divinity / Kusanagi. Gain a 30/35/40/45/50% chance to parry physical attacks for 30 seconds.", icon: "lk_parrying", slot: 7 },
    Skill { id: 7003, internal: "SAM_MOONLIT_REFLECTION", name: "Moonlit Reflection", description: "Divinity / Kusanagi. Within 15 seconds, block the next physical skill, return 100/150/200/250/300% of its damage and recover 5/6/7/8/9% of maximum SP.", icon: "kn_autocounter", slot: 14 },
    Skill { id: 7004, internal: "SAM_BLADE_HARMONY", name: "Blade Harmony", description: "Divinity / Kusanagi. Gain 20/40/60/80/100 HIT, 1/2/3/4/5 ASPD and 4/8/12/16/20% ATK for 60 seconds.", icon: "kn_twohandquicken", slot: 21 },
    Skill { id: 7005, internal: "SAM_BLOOD_COVENANT", name: "Blood Covenant", description: "Cursed / Muramasa. Pay 10% of maximum HP. Your next 3 normal attack attempts gain 60/120/180/240/300% damage. Unused charges expire after 30 seconds.", icon: "lk_berserk", slot: 4 },
    Skill { id: 7006, internal: "SAM_MURAMASAS_CURSE", name: "Muramasa's Curse", description: "Cursed / Muramasa. Gain 12/24/36/48/60% attack for 30 seconds. Drains 1% of maximum HP each second and ends before the drain would kill you.", icon: "bs_overthrust", slot: 11 },
    Skill { id: 7007, internal: "SAM_ASHEN_WIND_SLASH", name: "Ashen Wind Slash", description: "Cursed / Muramasa. Strike enemies within 3 cells for 1300/2000/2700/3400/4100% ATK (x base level / 100). Has a 15/20/25/30/35% chance to inflict Bleeding for 10 seconds.", icon: "asc_meteorassault", slot: 18 },
    Skill { id: 7008, internal: "SAM_WRATH_UNBOUND", name: "Wrath Unbound", description: "Cursed / Muramasa. Strike for 7000/11000/15000/19000/23000% ATK (x base level / 100). Requires HP strictly below 30% of maximum.", icon: "as_sonicblow", slot: 25 },
    // The Mortal Flame line, Tenseiga's own techniques, in the middle columns between the two
    // branches. Mortal Flame is granted by the Tenseiga questline, not learned with points.
    Skill { id: 7009, internal: "SAM_MORTAL_FLAME", name: "Mortal Flame", description: "Mortal Flame / Tenseiga. Quest skill: granted by the Tenseiga questline (for now, Masaru in Prontera grants it to whoever forged Tenseiga). Tenseiga burns with mortal will, not a god's flame. While wielding Tenseiga: ATK +10%, and 15% less damage from Holy and Shadow attacks. Opens Tenseiga's techniques.", icon: "mo_explosionspirits", slot: 2 },
    Skill { id: 7010, internal: "SAM_SEVER_HEAVENS_LIES", name: "Severing Heaven's Lies", description: "Mortal Flame / Tenseiga. Its edge cuts through illusions and divine lies. Strike one enemy for 2000/3000/4000/5000/6000% ATK (x base level / 100), with a 30/40/50/60/70% chance to strip its buffs.", icon: "lk_spiralpierce", slot: 8 },
    Skill { id: 7011, internal: "SAM_LIGHT_AND_SHADOW", name: "Light and Shadow", description: "Mortal Flame / Tenseiga. A cut through both light and shadow: strike one enemy twice for 1900/2800/3700/4600/5500% ATK each (x base level / 100), as Holy or Shadow, whichever it takes more damage from.", icon: "kn_bowlingbash", slot: 15 },
    Skill { id: 7012, internal: "SAM_UNCHAINED_WILL", name: "Unchained Will", description: "Mortal Flame / Tenseiga. Balance is not obedience; balance is freedom. Cure Stun, Freeze, Stone, Sleep, Silence, Curse, Blind and Confusion, and ward them off for 40/50/60/70/80 seconds.", icon: "mo_steelbody", slot: 10 },
    Skill { id: 7013, internal: "SAM_FORGE_OF_TEN_SOULS", name: "Forge of Ten Souls", description: "Mortal Flame / Tenseiga. The will of every companion lit the First Forge. For 90 seconds, every party member within 7 cells gains 5/10/15/20/25% ATK and MATK, and 1/2/3/4/5% more for each party member there (counting at most 10).", icon: "pr_gloria", slot: 17 },
    Skill { id: 7014, internal: "SAM_DESTINYS_SEVERANCE", name: "Destiny's Severance", description: "Mortal Flame / Tenseiga. The strike that severed the bond between divinity and mortal destiny. Strike one enemy for 8000/12000/16000/20000/24000% ATK (x base level / 100), ignoring DEF, and 50% more against boss monsters.", icon: "asc_breaker", slot: 23 },
    Skill { id: 7015, internal: "SAM_CRESCENT_GALE", name: "Crescent Gale", description: "Mortal Flame / Tenseiga. Hayate's will, loosed from Tenseiga's edge as a crescent of mortal flame. It tears through every enemy on an 11-cell path, 3 cells wide, toward the target, for 5500/8000/10500/13000/15500% ATK (x base level / 100). Ranged.", icon: "kn_brandishspear", slot: 9 },
];

/// What the skill window says about each skill's use. Matches db/custom/samurai/skill_db.yml
/// and skill_tree.yml on the server.
struct Use {
    max_level: u8,
    /// SP cost, cooldown and after-cast delay (seconds); None for a passive.
    cost: Option<(u32, f32, f32)>,
    target: &'static str,
    weapon: &'static str,
    prerequisite: Option<&'static str>,
    /// Cast time, seconds.
    cast: Option<f32>,
}

const ANY_BLADE: &str = "Requires an equipped dagger, one-handed sword, two-handed sword or katar.";
const TENSEIGA_ONLY: &str = "Requires Tenseiga-no-Hayate to be wielded.";

fn usage(id: u16) -> Use {
    let (cost, target, prerequisite) = match id {
        7001 => ((60, 0.7, 0.3), "One enemy, range 2 cells", None),
        7002 => ((40, 30.0, 0.3), "Self", Some("Heaven's Severance Lv 3")),
        7003 => ((45, 15.0, 0.3), "Self", Some("Kusanagi Guard Lv 3")),
        7004 => ((50, 60.0, 0.3), "Self", Some("Moonlit Reflection Lv 3")),
        7005 => ((30, 12.0, 0.3), "Self", None),
        7006 => ((50, 30.0, 0.3), "Self", Some("Blood Covenant Lv 3")),
        7007 => ((70, 1.0, 0.3), "Self-centered attack, radius 3 cells", Some("Muramasa's Curse Lv 3")),
        7008 => ((100, 15.0, 0.3), "One enemy, range 2 cells", Some("Ashen Wind Slash Lv 3")),
        7009 => return Use { max_level: 1, cost: None, target: "Passive", weapon: "", prerequisite: None, cast: None },
        7010 => ((70, 1.0, 0.3), "One enemy, range 2 cells", Some("Mortal Flame Lv 1")),
        7011 => ((80, 1.5, 0.5), "One enemy, range 2 cells", Some("Severing Heaven's Lies Lv 3")),
        7012 => ((60, 60.0, 0.3), "Self", Some("Mortal Flame Lv 1")),
        7013 => ((100, 120.0, 1.0), "Party members within 7 cells", Some("Unchained Will Lv 3")),
        7014 => ((150, 20.0, 1.0), "One enemy, range 2 cells", Some("Light and Shadow Lv 5, Forge of Ten Souls Lv 3")),
        7015 => ((120, 6.0, 0.5), "Enemies on a path toward one enemy, range 9 cells", Some("Severing Heaven's Lies Lv 5")),
        _ => unreachable!("Samurai skill metadata"),
    };
    let weapon = if id >= 7010 { TENSEIGA_ONLY } else { ANY_BLADE };
    Use { max_level: 5, cost: Some(cost), target, weapon, prerequisite, cast: match id { 7014 => Some(1.0), 7015 => Some(0.5), _ => None } }
}

pub fn register(names: &mut ro_game::skills::SkillNames, tree: &mut crate::skilltree::SkillTree) {
    for s in SKILLS {
        let u = usage(s.id);
        let mut description = vec![s.description.into(), format!("Maximum level: {}", u.max_level), format!("Target: {}", u.target)];
        if let Some((sp, cooldown, delay)) = u.cost {
            let cast = u.cast.map(|c| format!("Cast time: {c} second. ")).unwrap_or_default();
            description.push(format!("SP cost: {sp}. {cast}Cooldown: {cooldown} seconds. After-cast delay: {delay} seconds."));
        }
        if !u.weapon.is_empty() {
            description.push(u.weapon.into());
        }
        if let Some(prerequisite) = u.prerequisite {
            description.push(format!("Prerequisite: {prerequisite}"));
        }
        names.insert(s.id, s.internal, s.name, description);
    }
    tree.insert(JOB, Some(4013), SKILLS.iter().map(|s| (s.slot, s.internal.to_string())).collect());
}

pub fn icon(id: u16) -> Option<&'static str> {
    SKILLS.iter().find(|s| s.id == id).map(|s| s.icon)
}

/// (EFST id, skill id): the server's temporary Samurai effects.
pub const BUFFS: &[(u16, u16)] = &[(2000, 7002), (2001, 7003), (2002, 7004), (2003, 7005), (2004, 7006), (2005, 7012), (2006, 7013)];

pub fn buff_skill(id: u16) -> Option<&'static Skill> {
    let skill = BUFFS.iter().find(|(efst, _)| *efst == id)?.1;
    SKILLS.iter().find(|s| s.id == skill)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skills_resolve_in_both_tree_branches_and_preserve_inherited_tabs() {
        let mut names = ro_game::skills::SkillNames::default();
        let mut tree = crate::skilltree::SkillTree::default();
        tree.insert(4013, None, vec![(0, "ASC_METEORASSAULT".into())]);
        register(&mut names, &mut tree);
        let tabs = tree.tabs(JOB);
        assert_eq!(tabs.iter().map(|t| t.0).collect::<Vec<_>>(), [4013, JOB]);
        assert_eq!(tabs[1].1.len(), 15);
        let mut slots: Vec<_> = tabs[1].1.iter().map(|(slot, _)| *slot).collect();
        slots.sort();
        slots.dedup();
        assert_eq!(slots.len(), 15, "no two skills share a slot");
        for (slot, name) in tabs[1].1 {
            // Kusanagi in column 0, Muramasa in column 4, the Mortal Flame line between them.
            assert!(slot % crate::skilltree::COLUMNS <= 4);
            let id = names.id_of(name).expect("tree skill has a network ID");
            assert_eq!(names.internal(id), Some(name.as_str()));
            assert!(names.description(name).is_some());
            assert!(icon(id).is_some());
        }
    }

    #[test]
    fn full_body_preview_needs_no_stock_hair_or_headgear_assets() {
        assert!(crate::jobs::is_player(JOB));
        assert_eq!(crate::jobs::sprite(JOB), SPRITE);
        let appearance = crate::jobs::appearance(SPRITE, true).unwrap();
        assert_eq!(appearance.body, BODY);
        assert!(!appearance.attachments);
        assert!(crate::jobs::appearance(SPRITE, false).is_none());
        let vfs = ro_vfs::Vfs::new(vec![overlay_path().unwrap()], None, vec![]);
        let look = crate::preview::Look { job: SPRITE.into(), male: true, hair: 99, hair_color: 17, headgear: vec!["missing_hat".into()] };
        let portrait = crate::preview::compose(&vfs, &look).expect("full-body preview without GRFs");
        assert!(portrait.pixels.iter().any(|p| p.a() != 0));
        let plain = crate::preview::compose(&vfs, &crate::preview::Look { hair: 1, hair_color: 0, headgear: vec![], ..look.clone() }).unwrap();
        assert_eq!(portrait.pixels, plain.pixels);
        assert!(crate::preview::compose(&vfs, &crate::preview::Look { male: false, ..look }).is_none());
    }

    #[test]
    fn starter_item_metadata_resolves_to_the_matching_katana_view() {
        let path = overlay_path().unwrap().join("System/samurai_iteminfo.lua");
        let entries = crate::customdata::iteminfo(&std::fs::read(&path).unwrap(), "samurai_iteminfo.lua");
        let mut db = ro_game::items::ItemDb::default();
        db.extend(entries);
        let info = db.get(900000).expect("starter katana metadata");
        assert_eq!(info.name, "Samurai Initiate's Katana");
        assert_eq!(info.resource, "카타나");
        assert_eq!(weapon(info.view), Some(KATANA));
        assert_eq!(weapon(2), Some(KATANA));
        assert_eq!(weapon(3), Some(KATANA));
        assert_eq!(weapon(49), Some(KATANA)); // Slayer 1151 / Claymore 1163 / Muramasa 1164.
        assert_eq!(weapon(0), None);
        assert_eq!(weapon(16), None); // No matching custom katar art.
    }

    #[test]
    fn legendary_weapon_views_resolve_to_distinct_body_aligned_sprites() {
        assert_eq!(weapon(9001), Some(KUSANAGI));
        assert_eq!(weapon(9002), Some(MURAMASA));
        assert_eq!(weapon(9003), Some(TENSEIGA));
        assert_ne!(weapon(9001), weapon(9002));
        assert_ne!(weapon(9003), weapon(9001));
        assert_ne!(weapon(9003), weapon(9002));
        assert_ne!(weapon(9001), weapon(2));
        assert_ne!(weapon(9002), weapon(49)); // The stock Muramasa remains the generic katana.
        assert_eq!(weapon(900000), None); // Item IDs must first resolve through itemInfo.
        assert_eq!(weapon(900001), None);
        assert_eq!(weapon(900002), None);
        assert_eq!(weapon(900003), None);
    }

    #[test]
    fn every_skill_has_a_bundled_icon() {
        let root = overlay_path().expect("bundled Samurai overlay");
        for s in SKILLS {
            let path = format!("data/texture/유저인터페이스/item/{}.bmp", s.internal.to_ascii_lowercase());
            let bytes = std::fs::read(root.join(&path)).expect("bundled skill icon");
            let image = ro_formats::image::decode(&bytes, &path).unwrap();
            assert_eq!((image.width, image.height), (24, 24), "{path}");
            assert!(image.pixels.chunks_exact(4).any(|p| p[3] == 0), "{path}: magenta corners must be transparent");
        }
    }

    #[test]
    fn every_buff_has_a_bundled_status_icon() {
        let root = overlay_path().expect("bundled Samurai overlay");
        for &(efst, _) in BUFFS {
            let skill = buff_skill(efst).unwrap();
            // The client looks status icons up as efst_<name>.tga; ours are EFST_<internal>.
            let path = format!("data/texture/effect/efst_{}.tga", skill.internal.to_ascii_lowercase());
            let bytes = std::fs::read(root.join(&path)).expect("bundled status icon");
            let image = ro_formats::image::decode(&bytes, &path).unwrap();
            assert_eq!((image.width, image.height), (32, 32), "{path}");
            // Like the stock icons: an opaque line art over a translucent fill.
            assert!(image.pixels.chunks_exact(4).any(|p| p[3] == 255), "{path}");
            assert!(image.pixels.chunks_exact(4).any(|p| (100..=160).contains(&p[3])), "{path}");
        }
    }

    #[test]
    fn dual_wielded_katanas_resolve_to_their_offhand_sprites() {
        assert_eq!(offhand(900000), Some(KATANA_OFFHAND));
        assert_eq!(offhand(900001), Some(KUSANAGI_OFFHAND));
        assert_eq!(offhand(900002), Some(MURAMASA_OFFHAND));
        assert_eq!(offhand(900003), None); // Tenseiga is two-handed.
        assert_eq!(offhand(0), None); // Empty left hand.
        assert_eq!(offhand(2104), None); // Buckler: a shield, though its view (2) is the katana's.
        assert_eq!(offhand(9001), None); // Views are not item ids.
    }

    #[test]
    fn legendary_item_metadata_resolves_bundled_icons_and_visible_floor_sprites() {
        let root = overlay_path().expect("bundled Samurai overlay");
        let entries = crate::customdata::iteminfo(
            &std::fs::read(root.join("System/samurai_iteminfo.lua")).unwrap(), "samurai_iteminfo.lua");
        let mut db = ro_game::items::ItemDb::default();
        db.extend(entries);
        assert!(db.get(1164).is_none()); // Do not replace the stock two-handed Muramasa.
        for (id, name, resource, slots, view, sprite) in [
            (900001, "Kusanagi", "samurai_kusanagi", 1, 9001, KUSANAGI),
            (900002, "Muramasa", "samurai_muramasa", 1, 9002, MURAMASA),
            (900003, "Tenseiga-no-Hayate", "samurai_tenseiga", 2, 9003, TENSEIGA),
        ] {
            let info = db.get(id).expect("legendary weapon metadata");
            assert_eq!((info.name.as_str(), info.resource.as_str()), (name, resource));
            assert_eq!((info.slots, info.view), (slots, view));
            assert_eq!(weapon(info.view), Some(sprite));
            for (folder, size) in [("item", 32), ("collection", 128)] {
                let path = format!("data/texture/유저인터페이스/{folder}/{resource}.bmp");
                let bytes = std::fs::read(root.join(&path)).expect("bundled item picture");
                assert!(bytes.starts_with(b"BM"), "{path} must be a real BMP");
                let image = ro_formats::image::decode(&bytes, &path).unwrap();
                assert_eq!((image.width, image.height), (size, size));
                assert!(image.pixels.chunks_exact(4).any(|p| p[3] == 0), "{path} needs transparency");
                assert!(image.pixels.chunks_exact(4).any(|p| p[3] != 0), "{path} must be visible");
            }
            let path = root.join(format!("data/sprite/아이템/{resource}"));
            let spr = ro_formats::spr::Spr::parse(&std::fs::read(path.with_extension("spr")).unwrap()).unwrap();
            let act = ro_formats::act::Act::parse(&std::fs::read(path.with_extension("act")).unwrap()).unwrap();
            let (indexed, rgba) = spr.images(None);
            let frames = &act.actions.first().expect("floor action 0").frames;
            assert!(!frames.is_empty());
            for frame in frames {
                let layers: Vec<_> = frame.layers.iter().filter(|l| l.sprite >= 0).collect();
                assert!(!layers.is_empty(), "floor item must have a visible layer");
                for layer in layers {
                    let images = if layer.kind == 1 { &rgba } else { &indexed };
                    let image = images.get(layer.sprite as usize).expect("valid floor sprite reference");
                    assert!(image.pixels.chunks_exact(4).any(|p| p[3] != 0));
                }
            }
        }
    }

    #[test]
    fn shipped_body_and_weapon_match_all_player_actions() {
        let root = overlay_path().expect("bundled Samurai overlay");
        let mut parsed = Vec::new();
        for base in [BODY, KATANA, KUSANAGI, MURAMASA, TENSEIGA, KATANA_OFFHAND, KUSANAGI_OFFHAND, MURAMASA_OFFHAND] {
            let path = root.join(base.replace('\\', "/"));
            let spr = ro_formats::spr::Spr::parse(&std::fs::read(path.with_extension("spr")).unwrap()).unwrap();
            let act = ro_formats::act::Act::parse(&std::fs::read(path.with_extension("act")).unwrap()).unwrap();
            let (indexed, rgba) = spr.images(None);
            assert_eq!(act.actions.len(), 104);
            for a in &act.actions {
                assert!(!a.frames.is_empty());
                for l in a.frames.iter().flat_map(|f| &f.layers).filter(|l| l.sprite >= 0) {
                    assert!((l.sprite as usize) < if l.kind == 1 { rgba.len() } else { indexed.len() });
                }
            }
            parsed.push(act);
        }
        for weapon in &parsed[1..] {
            for (body, weapon) in parsed[0].actions.iter().zip(&weapon.actions) {
                assert_eq!(body.frames.len(), weapon.frames.len());
                assert_eq!(body.delay_ms, weapon.delay_ms);
            }
        }
    }
}
