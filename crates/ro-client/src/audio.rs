//! Map background music (mp3nametable.txt → BGM folder) and one-shot sound effects (data\wav).

use std::collections::HashMap;
use std::io::Cursor;

use rodio::Source;

pub struct Audio {
    sink: rodio::MixerDeviceSink,
    bgm: rodio::Player,
    /// Map name → BGM path, as listed in data\mp3nametable.txt.
    table: HashMap<String, String>,
    playing: String,
    pub bgm_volume: f32,
    pub sfx_volume: f32,
    /// Skill sounds (data/wav/effect/*.wav): lowercase file stem -> path below data/wav.
    effects: HashMap<String, String>,
}

impl Audio {
    /// None when there is no output device (the client runs silently).
    pub fn new(vfs: &ro_vfs::Vfs, bgm_volume: f32, sfx_volume: f32) -> Option<Self> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink().inspect_err(|e| tracing::warn!("no audio output: {e}")).ok()?;
        sink.log_on_drop(false);
        let bgm = rodio::Player::connect_new(sink.mixer());
        bgm.set_volume(bgm_volume);
        let table = vfs.read("data\\mp3nametable.txt").ok().flatten().map(|d| parse_table(&d)).unwrap_or_default();
        let effects = vfs
            .grfs
            .iter()
            .flat_map(|g| g.names())
            .filter_map(|n| {
                let rest = n.strip_prefix(r"data\wav\effect\")?;
                let stem = rest.strip_suffix(".wav")?;
                Some((stem.to_ascii_lowercase(), format!(r"effect\{rest}")))
            })
            .collect();
        Some(Self { sink, bgm, table, playing: String::new(), bgm_volume, sfx_volume, effects })
    }

    /// The title theme the original plays on the login and character screens.
    pub fn title(&mut self, vfs: &ro_vfs::Vfs) {
        self.play_track(vfs, "bgm\\01.mp3".into());
    }

    /// Switches to the map's track; keeps playing if it is already the current one.
    pub fn enter_map(&mut self, vfs: &ro_vfs::Vfs, map: &str) {
        let Some(path) = self.table.get(map).cloned() else { return };
        self.play_track(vfs, path);
    }

    fn play_track(&mut self, vfs: &ro_vfs::Vfs, path: String) {
        if path == self.playing {
            return;
        }
        self.bgm.stop();
        self.playing = path.clone();
        let file = path.rsplit(['\\', '/']).next().unwrap_or(&path);
        let data = [path.clone(), format!("BGM\\{file}")].into_iter().find_map(|p| vfs.read(&p).ok().flatten());
        let Some(data) = data else {
            tracing::warn!(%path, "BGM not found");
            return;
        };
        match rodio::Decoder::try_from(Cursor::new(data)) {
            Ok(src) => self.bgm.append(src.repeat_infinite()),
            Err(e) => tracing::warn!(%path, "BGM: {e}"),
        }
    }

    /// A script's playBGM: a track name like "01" or "bgm\01.mp3".
    pub fn play_bgm(&mut self, vfs: &ro_vfs::Vfs, name: &str) {
        let file = if name.contains('.') { name.to_string() } else { format!("{name}.mp3") };
        let path = if file.contains('\\') || file.contains('/') { file } else { format!("bgm\\{file}") };
        self.play_track(vfs, path);
    }

    pub fn set_bgm_volume(&mut self, v: f32) {
        self.bgm_volume = v;
        self.bgm.set_volume(v);
    }

    /// Plays data\wav\<name> once over whatever else is playing.
    pub fn play(&self, vfs: &ro_vfs::Vfs, name: &str) {
        if self.sfx_volume <= 0.0 || name.is_empty() {
            return;
        }
        let Some(data) = vfs.read(&format!("data\\wav\\{name}")).ok().flatten() else { return };
        if let Ok(src) = rodio::Decoder::try_from(Cursor::new(data)) {
            self.sink.mixer().add(src.amplify(self.sfx_volume));
        }
    }
}

/// Lines look like `prontera.rsw#bgm\\08.mp3#`; `//` comments the line out.
fn parse_table(data: &[u8]) -> HashMap<String, String> {
    let (text, _, _) = encoding_rs::EUC_KR.decode(data);
    text.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .filter_map(|l| {
            let mut parts = l.split('#');
            let map = parts.next()?.trim().strip_suffix(".rsw")?.to_ascii_lowercase();
            let path = parts.next()?.trim().replace("\\\\", "\\");
            Some((map, path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_bgm_table() {
        let t = super::parse_table(br"prontera.rsw#bgm\\08.mp3#
//prontera.rsw#bgm\\55.mp3#
Prt_Fild08.rsw#bgm\\12.mp3#
");
        assert_eq!(t["prontera"], r"bgm\08.mp3");
        assert_eq!(t["prt_fild08"], r"bgm\12.mp3");
        assert_eq!(t.len(), 2);
    }
}

/// rAthena skill prefix -> the word the client's sound files use for that class.
fn class_word(prefix: &str) -> Option<&'static str> {
    Some(match prefix {
        "SM" => "swordman",
        "MG" => "magician",
        "AL" => "acolyte",
        "AC" => "archer",
        "MC" => "merchant",
        "TF" => "thief",
        "KN" => "knight",
        "PR" => "priest",
        "WZ" => "wizard",
        "BS" => "black",
        "HT" => "hunter",
        "AS" => "assasin",
        "CR" => "cru",
        "MO" => "monk",
        "SA" => "sage",
        "RG" => "rogue",
        "AM" => "alche",
        "BA" => "bard",
        "DC" => "dancer",
        _ => return None,
    })
}

impl Audio {
    /// The sound file for a skill (by internal name, e.g. AS_SONICBLOW), if the client has
    /// one: tries the full name, the class word + skill (assasin_sonicblow) and the bare
    /// skill name, with and without underscores.
    pub fn skill_sound(&self, internal: &str) -> Option<String> {
        let lower = internal.to_ascii_lowercase();
        let (prefix, short) = internal.split_once('_').map_or(("", lower.as_str()), |(p, _)| (p, &lower[p.len() + 1..]));
        if let Some(file) = SKILL_SOUNDS.iter().find(|(k, _)| *k == internal).map(|(_, f)| *f) {
            // "_name" files sit in datawav itself, the rest in datawavffect.
            return if file.starts_with('_') { Some(format!("{file}.wav")) } else { self.effects.get(file).cloned() };
        }
        let flat = short.replace('_', "");
        let mut tries = vec![lower.clone(), short.to_string(), flat.clone(), format!("ef_{short}"), format!("ef_{flat}")];
        if let Some(w) = class_word(prefix) {
            for s in [short.to_string(), flat.clone()] {
                tries.push(format!("{w}_{s}"));
                tries.push(format!("{w}_{s}_a"));
            }
        }
        tries.iter().find_map(|t| self.effects.get(t)).cloned()
    }
}

/// Weapon sound family for a weapon sprite name (as in the look tables, e.g. "_단검").
pub fn weapon_kind(sprite: Option<&str>) -> &'static str {
    let s = sprite.unwrap_or("");
    for (k, v) in [("카타르", "katar"), ("단검", "dagger"), ("도끼", "axe"), ("창", "spear"), ("활", "bow"), ("책", "book"), ("지팡이", "rod"), ("롯드", "rod"), ("메이스", "mace"), ("둔기", "mace"), ("검", "sword")] {
        if s.contains(k) {
            return v;
        }
    }
    if s.is_empty() { "fist" } else { "sword" }
}

/// Skills whose sound file does not follow a naming pattern (file stems in wav\effect).
const SKILL_SOUNDS: &[(&str, &str)] = &[
    // Samurai (custom): the game ships no sounds of their own, so each borrows a fitting one.
    ("SAM_HEAVENS_SEVERANCE", "assasin_sonicblow"),
    ("SAM_KUSANAGI_GUARD", "ef_endure"),
    ("SAM_MOONLIT_REFLECTION", "knight_autocounter"),
    ("SAM_BLADE_HARMONY", "knight_twohandquicken"),
    ("SAM_BLOOD_COVENANT", "black_overthrust"),
    ("SAM_MURAMASAS_CURSE", "ef_devil"),
    ("SAM_ASHEN_WIND_SLASH", "knight_brandish_spear"),
    ("SAM_WRATH_UNBOUND", "knight_bowling_bash"),
    ("SAM_SEVER_HEAVENS_LIES", "ef_bash"),
    ("SAM_LIGHT_AND_SHADOW", "assasin_sonicblow"),
    ("SAM_UNCHAINED_WILL", "ef_detoxication"),
    ("SAM_FORGE_OF_TEN_SOULS", "priest_gloria"),
    ("SAM_DESTINYS_SEVERANCE", "ef_magnumbreak"),
    ("SAM_CRESCENT_GALE", "knight_brandish_spear"),
    ("SM_BASH", "ef_bash"),
    ("SM_MAGNUM", "ef_magnumbreak"),
    ("SM_ENDURE", "ef_endure"),
    ("SM_PROVOKE", "swordman_provoke"),
    ("MG_NAPALMBEAT", "ef_napalmbeat"),
    ("MG_SAFETYWALL", "ef_glasswall"),
    ("MG_SOULSTRIKE", "ef_soulstrike"),
    ("MG_COLDBOLT", "ef_icearrow"),
    ("MG_FROSTDIVER", "ef_frostdiver"),
    ("MG_STONECURSE", "ef_stonecurse"),
    ("MG_FIREBALL", "ef_fireball"),
    ("MG_FIREWALL", "ef_firewall"),
    ("MG_FIREBOLT", "ef_firearrow"),
    ("MG_LIGHTNINGBOLT", "ef_lightbolt"),
    ("MG_THUNDERSTORM", "ef_thunderstorm"),
    ("MG_SIGHT", "ef_sight"),
    ("AL_RUWACH", "ef_ruwach"),
    ("AL_PNEUMA", "ef_signum"),
    ("AL_TELEPORT", "ef_teleportation"),
    ("AL_WARP", "ef_readyportal"),
    ("AL_HEAL", "_heal_effect"),
    ("AL_INCAGI", "ef_incagility"),
    ("AL_DECAGI", "ef_decagility"),
    ("AL_HOLYWATER", "ef_aqua"),
    ("AL_CRUCIS", "ef_signum"),
    ("AL_ANGELUS", "ef_angelus"),
    ("AL_BLESSING", "ef_blessing"),
    ("AL_CURE", "acolyte_cure"),
    ("MC_MAMMONITE", "ef_coin"),
    ("AC_DOUBLE", "ef_hit2"),
    ("AC_SHOWER", "ef_hit3"),
    ("TF_STEAL", "ef_steal"),
    ("TF_HIDING", "ef_hiding"),
    ("TF_POISON", "ef_poisonattack"),
    ("TF_DETOXIFY", "ef_detoxication"),
    ("KN_PIERCE", "ef_hit4"),
    ("KN_BRANDISHSPEAR", "knight_brandish_spear"),
    ("KN_SPEARSTAB", "ef_hit5"),
    ("KN_SPEARBOOMERANG", "knight_spear_boomerang"),
    ("KN_BOWLINGBASH", "knight_bowling_bash"),
    ("KN_AUTOCOUNTER", "knight_autocounter"),
    ("KN_TWOHANDQUICKEN", "knight_twohandquicken"),
    ("PR_KYRIE", "priest_kyrie_eleison_a"),
    ("PR_TURNUNDEAD", "priest_turn_undead"),
    ("PR_STRECOVERY", "priest_recovery"),
    ("PR_SLOWPOISON", "priest_slowpoison"),
    ("WZ_FIREPILLAR", "wizard_fire_pillar_a"),
    ("WZ_FIREIVY", "wizard_fire_ivy"),
    ("WZ_JUPITEL", "magician_thunderstorm"),
    ("WZ_VERMILION", "magician_thunderstorm"),
    ("WZ_WATERBALL", "wizard_waterball_chulung"),
    ("WZ_FROSTNOVA", "ef_frostdiver2"),
    ("WZ_HEAVENDRIVE", "wizard_earthspike"),
    ("WZ_EARTHSPIKE", "wizard_earthspike"),
    ("WZ_METEOR", "wizard_meteor"),
    ("WZ_STORMGUST", "wizard_stormgust"),
    ("WZ_ICEWALL", "wizard_icewall"),
    ("WZ_QUAGMIRE", "wizard_quagmire"),
    ("WZ_SIGHTRASHER", "wizard_sightrasher"),
    ("BS_ADRENALINE", "black_adrenalinerush"),
    ("BS_ADRENALINE2", "black_adrenalinerush"),
    ("BS_WEAPONPERFECT", "black_weapon_perfection"),
    ("BS_MAXIMIZE", "black_maximize_power_sword"),
    ("BS_OVERTHRUST", "black_overthrust"),
    ("BS_HAMMERFALL", "black_hammerfall"),
    ("BS_REPAIRWEAPON", "black_weapon_repair"),
    ("HT_SHOCKWAVE", "hunter_shockwavetrap"),
    ("HT_TALKIEBOX", "hunter_talkiebox_a"),
    ("AS_GRIMTOOTH", "ef_hit6"),
    ("AS_SPLASHER", "assasin_venomsplasher"),
    ("AS_ENCHANTPOISON", "assasin_enchantpoison"),
    ("AS_VENOMKNIFE", "thief_invenom"),
    ("MC_CARTREVOLUTION", "ef_hit3"),
    ("AL_HOLYLIGHT", "ef_lightbolt"),
    ("RG_STEALCOIN", "ef_coin"),
    ("RG_INTIMIDATE", "rog_intimidate"),
    ("RG_RAID", "ef_hit5"),
    ("RG_BACKSTAP", "ef_hit6"),
    ("CR_SHIELDCHARGE", "ef_hit4"),
    ("CR_HOLYCROSS", "ef_hit5"),
    ("CR_REFLECTSHIELD", "ef_endure"),
    ("CR_AUTOGUARD", "ef_endure"),
    ("CR_DEFENDER", "ef_endure"),
    ("CR_PROVIDENCE", "ef_angel"),
    ("CR_DEVOTION", "ef_angel"),
    ("MO_TRIPLEATTACK", "ef_hit4"),
    ("MO_CHAINCOMBO", "ef_hit5"),
    ("MO_COMBOFINISH", "ef_hit6"),
    ("MO_FINGEROFFENSIVE", "ef_hit4"),
    ("MO_EXTREMITYFIST", "ef_hit6"),
    ("MO_INVESTIGATE", "ef_hit5"),
    ("AM_ACIDTERROR", "ef_poisonattack"),
    ("AM_DEMONSTRATION", "ef_fireball"),
    ("AM_POTIONPITCHER", "_heal_effect"),
    ("SA_DISPELL", "ef_devil"),
    ("SA_LANDPROTECTOR", "ef_signum"),
    ("SA_VOLCANO", "ef_firewall"),
    ("SA_DELUGE", "ef_aqua"),
    ("SA_VIOLENTGALE", "ef_thunderstorm"),
    ("SA_FLAMELAUNCHER", "ef_firearrow"),
    ("SA_FROSTWEAPON", "ef_icearrow"),
    ("SA_LIGHTNINGLOADER", "ef_lightbolt"),
    ("SA_SEISMICWEAPON", "wizard_earthspike"),
    ("SA_MAGICROD", "sage_magic"),
];
