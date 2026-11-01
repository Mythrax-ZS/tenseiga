//! Illustrated skies for exterior maps. The renderer never guesses that an unknown
//! room is outdoors: the client can opt additional maps in with `outdoor` after
//! consulting its map metadata or a local override.

use crate::Lighting;

/// Linear RGB colours and art direction for one map's sky.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyProfile {
    pub name: &'static str,
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    pub cloud: [f32; 3],
    /// Fractional cloud cover, 0..1.
    pub cloud_cover: f32,
    pub cloud_scale: f32,
    /// Slow cloud movement in sky coordinates per second.
    pub wind_speed: f32,
    pub haze: f32,
    /// Visibility of stars once the sky transitions to night, 0..1.
    pub stars: f32,
    /// Stable per-map variation, 0..1. Does not change when revisiting a map.
    pub seed: f32,
    /// Open space instead of a sky: the Milky Way, nebulae and stars at any hour (Herosria).
    pub galaxy: bool,
}

#[derive(Clone, Copy)]
enum Climate {
    Temperate,
    Coast,
    Desert,
    Forest,
    Blossom,
    Highland,
    Snow,
    Gothic,
    Industrial,
    Faerie,
    Volcanic,
    Cosmos,
}

impl SkyProfile {
    /// Known exterior maps, including open-air areas named as dungeons. The client
    /// should also consult indoorRswTable and its underwater detection before using
    /// this profile. Unknown custom maps need an explicit outdoor declaration.
    pub fn for_map(name: &str, lighting: &Lighting) -> Option<Self> {
        let key = Self::map_key(name);
        known_outdoor(&key).then(|| Self::from_key(&key, lighting))
    }

    /// A map already verified as outdoors by the caller. Unknown regions receive
    /// a temperate palette, tinted by their own RSW lighting and a stable map seed.
    pub fn outdoor(name: &str, lighting: &Lighting) -> Self {
        Self::from_key(&Self::map_key(name), lighting)
    }

    /// Resource names may contain a VFS path, extension, mixed case, or a custom
    /// `instance-number#source-map` prefix. Stock `1@...` map names are identities,
    /// not prefixes: keep them so indoor and outdoor instance floors stay distinct.
    pub fn map_key(name: &str) -> String {
        let basename = name.trim().rsplit(['/', '\\']).next().unwrap_or(name).to_ascii_lowercase();
        let basename = basename.strip_suffix(".rsw").or_else(|| basename.strip_suffix(".gat")).or_else(|| basename.strip_suffix(".gnd")).unwrap_or(&basename);
        let basename = match basename.split_once('#') {
            Some((prefix, suffix)) if !prefix.is_empty() && prefix.bytes().all(|b| b.is_ascii_digit()) && suffix.bytes().any(|b| b.is_ascii_alphabetic()) => suffix,
            _ => basename,
        };
        basename.to_owned()
    }

    fn from_key(key: &str, lighting: &Lighting) -> Self {
        // Palettes are authored in sRGB for predictable art direction. Conversion
        // happens once here, before the sky enters the renderer's linear HDR pass.
        let climate = climate(key);
        let (name, zenith, horizon, cloud, cover, scale, wind, haze, stars) = match climate {
            Climate::Temperate => ("temperate", [0.24, 0.49, 0.77], [0.82, 0.89, 0.92], [1.0, 0.96, 0.87], 0.46, 1.65, 0.006, 0.22, 0.72),
            Climate::Coast => ("coastal", [0.12, 0.57, 0.76], [0.70, 0.91, 0.88], [1.0, 0.98, 0.88], 0.42, 1.35, 0.010, 0.20, 0.80),
            Climate::Desert => ("desert", [0.39, 0.56, 0.69], [0.96, 0.77, 0.50], [1.0, 0.90, 0.72], 0.16, 2.45, 0.004, 0.57, 0.95),
            Climate::Forest => ("forest", [0.29, 0.52, 0.62], [0.76, 0.86, 0.77], [0.95, 0.96, 0.84], 0.55, 1.85, 0.004, 0.39, 0.55),
            Climate::Blossom => ("blossom", [0.40, 0.56, 0.78], [0.94, 0.80, 0.81], [1.0, 0.92, 0.90], 0.42, 1.9, 0.004, 0.30, 0.70),
            Climate::Highland => ("highland", [0.24, 0.42, 0.73], [0.77, 0.85, 0.96], [0.96, 0.97, 1.0], 0.48, 1.35, 0.011, 0.18, 0.90),
            Climate::Snow => ("snow", [0.32, 0.48, 0.68], [0.82, 0.90, 0.96], [0.93, 0.97, 1.0], 0.65, 1.65, 0.005, 0.48, 0.85),
            Climate::Gothic => ("gothic", [0.18, 0.13, 0.30], [0.54, 0.40, 0.52], [0.59, 0.53, 0.64], 0.68, 1.25, 0.003, 0.46, 0.80),
            Climate::Industrial => ("industrial", [0.40, 0.46, 0.51], [0.80, 0.71, 0.57], [0.79, 0.77, 0.70], 0.72, 1.9, 0.005, 0.70, 0.22),
            Climate::Faerie => ("faerie", [0.36, 0.34, 0.67], [0.75, 0.84, 0.91], [0.96, 0.90, 1.0], 0.40, 1.75, 0.004, 0.24, 1.0),
            Climate::Volcanic => ("volcanic", [0.35, 0.23, 0.30], [0.88, 0.48, 0.29], [0.62, 0.49, 0.46], 0.75, 1.4, 0.006, 0.72, 0.12),
            // Deep space; `cloud` tints the nebulae.
            Climate::Cosmos => ("cosmos", [0.03, 0.03, 0.09], [0.13, 0.09, 0.24], [0.62, 0.42, 0.95], 0.0, 1.0, 0.0, 0.25, 1.0),
        };
        let hash = key.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
        let sample = |shift: u32| ((hash >> shift) & 255) as f32 / 255.0;
        let seed = (hash & 0x00ff_ffff) as f32 / 16_777_215.0;
        let tint = lighting.ambient.map(|v| if v.is_finite() { v.clamp(0.0, 1.5) } else { 0.5 });
        let tint_mean = ((tint[0] + tint[1] + tint[2]) / 3.0).max(0.10);
        let palette = |c: [f32; 3], strength: f32| {
            std::array::from_fn(|i| {
                // Light colour influences the palette; light intensity does not turn
                // blue daylight into a black void on maps with low ambient values.
                let light_tint = (tint[i] / tint_mean - 1.0).clamp(-0.5, 0.5) * 0.06;
                let variation = (sample((i * 8) as u32) - 0.5) * strength;
                srgb_to_linear((c[i] + light_tint + variation).clamp(0.0, 1.0))
            })
        };
        Self {
            name,
            zenith: palette(zenith, 0.055),
            horizon: palette(horizon, 0.035),
            cloud: palette(cloud, 0.020),
            cloud_cover: (cover + (sample(8) - 0.5) * 0.12).clamp(0.08, 0.90),
            cloud_scale: scale * (0.90 + sample(16) * 0.20),
            wind_speed: wind * (0.85 + sample(24) * 0.30),
            haze,
            stars,
            seed,
            galaxy: matches!(climate, Climate::Cosmos),
        }
    }
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn known_outdoor(n: &str) -> bool {
    // Explicit exterior exceptions come first. A '_dun' substring alone says
    // nothing about a roof: Turtle Island's beach and Moscovia's forests are open.
    if matches!(n,
        "tur_dun01" | "mosk_dun01" | "mosk_dun02" | "mosk_dun03" |
        "gon_dun02" | "dew_dun01" |
        "1@mist" | "1@gef" | "1@ge_st" | "1@ge_sn" | "1@sara" | "1@glast" |
        "1@odin" | "1@os_a" | "1@os_b" | "1@cor" | "1@herbs" |
        "1@def01" | "1@def02" | "1@tnm2"
    ) {
        return true;
    }
    // Interiors and underwater/cave maps must not inherit a nearby city's sky.
    // Comodo is a cave town; only its coastal fields use the tropical palette.
    if n.contains("_in") || n.starts_with("in_") || n.contains("_dun") || n.contains("@") ||
        matches!(n, "comodo" | "airport" | "y_airport" | "lhz_airport" | "airplane" | "airplane_01" | "airplane2") {
        return false;
    }
    // Field families in the installed world's map index are outside. Exclude
    // obvious custom interior suffixes above before applying this convention.
    if n.contains("_fild") || n.contains("_field") {
        return true;
    }
    if pvp_city(n).is_some() {
        return true;
    }
    if ["mjolnir_", "prt_maze", "odin_tem", "hero_out", "jor_back", "jor_root", "jor_safty"].iter().any(|prefix| n.starts_with(prefix)) {
        return true;
    }
    matches!(n,
        "prontera" | "pprontera" | "prt_q" | "x_prt" |
        "alberta" | "alb2trea" | "aldebaran" | "geffen" |
        "izlude" | "izlude_a" | "izlude_b" | "izlude_c" | "izlude_d" | "izlu2dun" |
        "morocc" | "moc_ruins" | "payon" | "pay_arche" | "prt_monk" | "prt_are01" |
        "xmas" | "niflxmas" | "yuno" | "amatsu" | "gonryun" | "umbala" | "niflheim" |
        "louyang" | "jawaii" | "ayothaya" | "einbech" | "einbroch" | "lighthalzen" | "x_lhz" |
        "hugel" | "e_hugel" | "rachel" | "x_ra" | "ra_temple" | "ra_temsky" | "veins" |
        "nameless_i" | "nameless_n" | "moscovia" | "mid_camp" | "manuk" | "splendide" |
        "brasilis" | "dicastes01" | "dicastes02" | "mora" | "dewata" | "malangdo" | "malaya" |
        "eclage" | "ecl_hub01" | "dali" | "dali02" | "lasagna" | "harboro1" | "harboro2" |
        "rockrdg1" | "rockrdg2" | "verus01" | "verus02" | "verus02_a" | "verus02_b" | "verus03" | "verus04" |
        "sp_cor" | "sp_os" | "wolfvill" | "icecastle" | "herosria" |
        "glast_01" | "gef_fild13" | "alde_gld" | "pay_gld" | "prt_gld" | "sch_gld" | "aru_gld" |
        "te_prt_gld" | "te_alde_gld" | "odin_past" | "yggdrasil01" | "ygg_edge" |
        "star_frst" | "mjo_wst01" | "ra_pol01" | "thor_camp" | "moro_vol" |
        "jor_tail" | "jor_nest" | "jor_que" | "jor_twig" | "jor_sanct" | "jor_twice" |
        "bl_grass" | "bl_lava" | "bl_ice" | "bl_death" | "bl_soul" | "bl_temple" | "bl_venom" |
        "tha_scene01" | "p_track01" | "p_track02" | "e_tower" | "mosk_ship" | "que_temsky" |
        "que_dan01" | "schg_que01" | "arug_que01" | "bat_c01" | "bat_c02" | "bat_c03" |
        "que_qsch01" | "que_qsch02" | "que_qsch03" | "que_qsch04" | "que_qsch05" |
        "que_qaru01" | "que_qaru02" | "que_qaru03" | "que_qaru04" | "que_qaru05"
    )
}

/// The `pvp_y` arenas are copies of outdoor cities. `pvp_n` uses entirely
/// different arena geometry and must not inherit the same assumption.
fn pvp_city(n: &str) -> Option<&'static str> {
    let (room, city) = n.strip_prefix("pvp_y_")?.split_once('-')?;
    if !matches!(room, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8") {
        return None;
    }
    match city {
        "1" => Some("prontera"),
        "2" => Some("izlude"),
        "3" => Some("payon"),
        "4" => Some("alberta"),
        "5" => Some("morocc"),
        _ => None,
    }
}

fn climate(n: &str) -> Climate {
    let n = pvp_city(n).unwrap_or(n);
    if n == "herosria" {
        Climate::Cosmos
    } else if n == "niflheim" || n == "niflxmas" || n.starts_with("nif_") || n.starts_with("nameless") || n.starts_with("glast") || n.starts_with("gl_") || matches!(n, "1@glast" | "bl_death" | "bl_soul") {
        Climate::Gothic
    } else if n == "xmas" || n.starts_with("xmas_") || n == "manuk" || n.starts_with("man_") || n.starts_with("dicastes") || n.starts_with("dic_") || n.starts_with("que_qsch") || n.starts_with("que_qaru") || matches!(n, "icecastle" | "bl_ice") {
        Climate::Snow
    } else if n.starts_with("ein") || n.starts_with("verus") || matches!(n, "sp_cor" | "sp_os" | "1@cor" | "1@os_a" | "1@os_b") {
        Climate::Industrial
    } else if n.starts_with("morocc") || n.starts_with("moc_") || n.starts_with("ve_") || n == "veins" || n.starts_with("ra_") || matches!(n, "rachel" | "x_ra" | "harboro1" | "harboro2" | "rockrdg1" | "rockrdg2" | "aru_gld") {
        Climate::Desert
    } else if n.starts_with("thor_") || matches!(n, "moro_vol" | "bl_lava" | "dew_dun01") {
        Climate::Volcanic
    } else if n.starts_with("amatsu") || n.starts_with("ama_") || n == "louyang" || n.starts_with("lou_") {
        Climate::Blossom
    } else if n == "geffen" || n.starts_with("gef_") || n == "splendide" || n.starts_with("spl_") || n == "mora" || n.starts_with("bif_") || n.starts_with("ecl") || n.starts_with("ygg") || matches!(n, "1@gef" | "1@ge_st" | "1@ge_sn" | "1@mist" | "star_frst" | "dali" | "dali02") {
        Climate::Faerie
    } else if n == "yuno" || n.starts_with("yuno_") || n.starts_with("mjolnir_") || n.starts_with("mjo_wst") || n == "gonryun" || n.starts_with("gon_") || n == "hugel" || n == "e_hugel" || n.starts_with("hu_") || n.starts_with("odin_") || matches!(n, "1@odin" | "tha_scene01" | "que_temsky" | "que_dan01" | "1@def02") {
        Climate::Highland
    } else if n.starts_with("alberta") || n == "alb2trea" || n.starts_with("izlude") || n == "izlu2dun" || n.starts_with("cmd_") || n == "comodo" || n == "jawaii" || n == "brasilis" || n.starts_with("bra_") || n == "dewata" || n.starts_with("dew_") || n == "malangdo" || n == "malaya" || n.starts_with("ma_fild") || n == "lasagna" || n.starts_with("lasa_") || n == "tur_dun01" || n == "ayothaya" || n.starts_with("ayo_") || n == "e_tower" {
        Climate::Coast
    } else if n == "payon" || n.starts_with("pay_") || n == "umbala" || n.starts_with("um_") || n.starts_with("mosk") || n == "moscovia" || n.starts_with("prt_maze") || n.starts_with("jor_") || matches!(n, "wolfvill" | "bl_grass" | "bl_venom" | "1@sara" | "1@herbs") {
        Climate::Forest
    } else {
        Climate::Temperate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cities_fields_and_open_air_dungeons_have_skies() {
        for name in ["prontera", "prt_fild08", "izlude_d", "geffen", "morocc", "payon", "niflheim", "einbroch", "amatsu", "xmas", "tur_dun01", "mosk_dun02", "gon_dun02", "dew_dun01", "odin_tem03", "1@mist", "1@gef", "1@tnm2", "jor_back5", "custom_fild01", "pvp_y_8-5", "tha_scene01", "e_tower"] {
            assert!(SkyProfile::for_map(name, &Lighting::default()).is_some(), "missing exterior: {name}");
        }
    }

    #[test]
    fn interiors_caves_and_unknown_maps_stay_dark() {
        for name in ["prt_in", "prt_castle", "prt_church", "comodo", "in_sphinx1", "gef_tower", "c_tower1", "anthell01", "iz_dun00", "iz_dun05", "tur_dun02", "beach_dun", "pay_dun04", "abbey01", "gl_chyard", "dew_dun02", "1@tnm3", "1@gef_in", "1@tower", "airplane_01", "new_custom_room", "prontera_in", "prt_fild_room_in", "00123#00456", "pvp_n_8-5", "pvp_y_room"] {
            assert!(SkyProfile::for_map(name, &Lighting::default()).is_none(), "sky leaked into {name}");
        }
    }

    #[test]
    fn aliases_resolve_to_the_same_stable_sky() {
        let light = Lighting::default();
        let p = SkyProfile::for_map("prontera", &light);
        assert_eq!(p, SkyProfile::for_map("DATA\\Prontera.RSW", &light));
        assert_eq!(p, SkyProfile::for_map("00123#prontera.gat", &light));
        assert_eq!(SkyProfile::map_key("1@mist.rsw"), "1@mist");
        assert_eq!(SkyProfile::map_key("00123#00456"), "00123#00456");
    }

    #[test]
    fn maps_have_distinct_stable_art_direction() {
        let light = Lighting::default();
        let prontera = SkyProfile::for_map("prontera", &light).unwrap();
        let field = SkyProfile::for_map("prt_fild08", &light).unwrap();
        let desert = SkyProfile::for_map("morocc", &light).unwrap();
        assert_eq!(prontera, SkyProfile::for_map("prontera", &light).unwrap());
        assert_eq!(prontera.name, field.name);
        assert_ne!(prontera.seed, field.seed);
        assert_ne!(prontera.zenith, field.zenith);
        assert_ne!(prontera.name, desert.name);
        assert!(desert.haze > prontera.haze);
        assert!(desert.cloud_cover < prontera.cloud_cover);
        assert_eq!(SkyProfile::for_map("pvp_y_8-5", &light).unwrap().name, desert.name);
        assert!(SkyProfile::for_map("herosria", &light).unwrap().galaxy && !prontera.galaxy);
    }

    #[test]
    fn lighting_tints_but_cannot_black_out_daylight() {
        let daylight = SkyProfile::outdoor("custom_exterior", &Lighting::default());
        let dark = SkyProfile::outdoor("custom_exterior", &Lighting { ambient: [0.0; 3], ..Lighting::default() });
        let invalid = SkyProfile::outdoor("custom_exterior", &Lighting { ambient: [f32::NAN, f32::INFINITY, -3.0], ..Lighting::default() });
        for p in [daylight, dark, invalid] {
            for c in p.zenith.into_iter().chain(p.horizon).chain(p.cloud) {
                assert!(c.is_finite() && (0.0..=1.0).contains(&c));
            }
            assert!(p.horizon.iter().all(|v| *v > 0.1));
        }
    }
}
