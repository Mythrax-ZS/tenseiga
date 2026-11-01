//! Outdoor sky selection: regional defaults, the client's map metadata, then local art direction.

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use ro_formats::lub::{self, Chunk, Value};
use ro_render::{Lighting, SkyProfile};
use serde::Deserialize;

const INDOOR_TABLE: &str = r"data\indoorRswTable.txt";
const SKY_TABLE: &str = r"data\LuaFiles514\Lua Files\MapSkyData\MapSkyData.lub";

/// Called once per map entry, before water is uploaded. Overrides are reloaded here so
/// artists can change a map's look and re-enter it without rebuilding the client.
pub fn for_map(
    vfs: &ro_vfs::Vfs,
    name: &str,
    lighting: &Lighting,
    submerged: bool,
) -> Option<SkyProfile> {
    // An explicit local override must never put a sky behind an underwater map.
    if submerged {
        return None;
    }
    let key = map_key(name);
    let indoor = read(vfs, INDOOR_TABLE).is_some_and(|data| indoor_maps(&data).contains(&key));
    let overrides = load_overrides();
    let selected = overrides.selected(&key);
    let mut profile = base_profile(&key, lighting, submerged, indoor, selected)?;
    // Open space keeps its own colours and stars over the original's sky table.
    if let Some(data) = read(vfs, SKY_TABLE).filter(|_| !profile.galaxy) {
        match lub::eval(&data) {
            Ok(chunk) => apply_stock(&mut profile, &chunk, &key),
            Err(error) => tracing::warn!(%error, "could not read MapSkyData; using regional sky"),
        }
    }
    if let Some(value) = selected {
        value.apply(&mut profile);
    }
    Some(profile)
}

fn read(vfs: &ro_vfs::Vfs, path: &str) -> Option<Vec<u8>> {
    match vfs.read(path) {
        Ok(data) => data,
        Err(error) => {
            tracing::warn!(%path, %error, "could not read sky metadata");
            None
        }
    }
}

fn base_profile(
    name: &str,
    lighting: &Lighting,
    submerged: bool,
    indoor: bool,
    value: Option<&MapOverride>,
) -> Option<SkyProfile> {
    if submerged || value.and_then(|v| v.enabled) == Some(false) {
        return None;
    }
    if value.and_then(|v| v.enabled) == Some(true) {
        // Explicit art direction also supports custom outdoor maps unknown to the stock
        // regional classifier. This is the only way to override an indoor-table entry.
        return Some(SkyProfile::outdoor(name, lighting));
    }
    if indoor {
        return None;
    }
    SkyProfile::for_map(name, lighting)
}

fn map_key(name: &str) -> String {
    SkyProfile::map_key(name)
}

/// The stock file is a list of `map.rsw#` records, with EUC-KR comments. ASCII map
/// names survive decoding even when a translation pack changes the comments' encoding.
fn indoor_maps(data: &[u8]) -> HashSet<String> {
    let (text, _, _) =
        encoding_rs::EUC_KR.decode(data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(data));
    text.lines()
        .filter_map(|line| {
            let line = line
                .split("//")
                .next()?
                .trim()
                .trim_start_matches('\u{feff}');
            let record = line.split_once('#')?.0.trim();
            record
                .to_ascii_lowercase()
                .ends_with(".rsw")
                .then(|| map_key(record))
        })
        .collect()
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn color(chunk: &Chunk, value: &Value) -> Option<[f32; 3]> {
    let mut result = [0.0; 3];
    for (index, channel) in result.iter_mut().enumerate() {
        let n = chunk.field_index(value, index as u32 + 1)?.as_number()?;
        if !n.is_finite() || !(0.0..=255.0).contains(&n) {
            return None;
        }
        *channel = srgb_to_linear(n as f32 / 255.0);
    }
    Some(result)
}

fn mix(a: [f32; 3], b: [f32; 3], weight: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * weight)
}

/// MapSkyData also names indoor and underground special-effect maps. It only refines
/// a sky already admitted by outdoor classification; its presence never enables one.
fn apply_stock(profile: &mut SkyProfile, chunk: &Chunk, name: &str) {
    let Some(table) = chunk.globals.get("MapSkyData") else {
        return;
    };
    let Some((_, record)) = chunk
        .entries(table)
        .iter()
        .find(|(key, _)| key.as_str().is_some_and(|key| map_key(key) == name))
    else {
        return;
    };
    if let Some(bg) = chunk
        .field(record, "BG_Color")
        .and_then(|v| color(chunk, v))
    {
        profile.zenith = mix(profile.zenith, bg, 0.65);
        profile.horizon = mix(profile.horizon, bg, 0.45);
    }
    if let Some(Value::Bool(stars)) = chunk.field(record, "Star_Effect") {
        profile.stars = if *stars { profile.stars.max(0.7) } else { 0.0 };
    }
    if matches!(chunk.field(record, "BG_Fog"), Some(Value::Bool(true))) {
        profile.haze = profile.haze.max(0.35);
    }
    if let Some(clouds) = chunk.field(record, "Cloud_Effect") {
        let mut sum = [0.0; 3];
        let mut weight = 0.0;
        for (_, cloud) in chunk.entries(clouds) {
            let Some(rgb) = chunk.field(cloud, "Color").and_then(|v| color(chunk, v)) else {
                continue;
            };
            let n = chunk
                .field(cloud, "Num")
                .and_then(Value::as_number)
                .unwrap_or(1.0);
            if !n.is_finite() || n <= 0.0 {
                continue;
            }
            let n = n.min(10_000.0) as f32;
            for i in 0..3 {
                sum[i] += rgb[i] * n;
            }
            weight += n;
        }
        if weight > 0.0 {
            profile.cloud = mix(profile.cloud, sum.map(|channel| channel / weight), 0.6);
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Overrides {
    maps: BTreeMap<String, MapOverride>,
}

impl Overrides {
    fn parse(text: &str) -> Result<Self, String> {
        let parsed = toml::from_str::<Self>(text).map_err(|error| error.to_string())?;
        let mut maps = BTreeMap::new();
        let mut originals = BTreeMap::new();
        for (name, value) in parsed.maps {
            let key = map_key(&name);
            if let Some(previous) = originals.insert(key.clone(), name.clone()) {
                return Err(format!(
                    "duplicate normalized map key {key:?}: {previous:?} and {name:?}"
                ));
            }
            maps.insert(key, value);
        }
        Ok(Self { maps })
    }

    fn selected(&self, name: &str) -> Option<&MapOverride> {
        self.maps.get(name).filter(|value| {
            // A disable is authoritative; unused art values must not accidentally
            // re-enable the map's default sky when they are invalid.
            if value.enabled == Some(false) {
                return true;
            }
            if let Err(message) = value.validate() {
                tracing::warn!(map = name, %message, "invalid sky override; using map defaults");
                false
            } else {
                true
            }
        })
    }
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct MapOverride {
    enabled: Option<bool>,
    zenith: Option<[f32; 3]>,
    horizon: Option<[f32; 3]>,
    cloud: Option<[f32; 3]>,
    cloud_cover: Option<f32>,
    cloud_scale: Option<f32>,
    wind_speed: Option<f32>,
    haze: Option<f32>,
    stars: Option<f32>,
}

impl MapOverride {
    fn validate(&self) -> Result<(), String> {
        for (name, rgb) in [
            ("zenith", self.zenith),
            ("horizon", self.horizon),
            ("cloud", self.cloud),
        ] {
            if rgb.is_some_and(|rgb| {
                rgb.iter()
                    .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            }) {
                return Err(format!(
                    "{name} must contain three finite sRGB values in 0..1"
                ));
            }
        }
        for (name, value, low, high) in [
            ("cloud_cover", self.cloud_cover, 0.0, 1.0),
            ("cloud_scale", self.cloud_scale, 0.25, 8.0),
            ("wind_speed", self.wind_speed, 0.0, 1.0),
            ("haze", self.haze, 0.0, 1.0),
            ("stars", self.stars, 0.0, 1.0),
        ] {
            if value.is_some_and(|value| !value.is_finite() || !(low..=high).contains(&value)) {
                return Err(format!("{name} must be finite and in {low}..{high}"));
            }
        }
        Ok(())
    }

    fn apply(&self, profile: &mut SkyProfile) {
        if let Some(rgb) = self.zenith {
            profile.zenith = rgb.map(srgb_to_linear);
        }
        if let Some(rgb) = self.horizon {
            profile.horizon = rgb.map(srgb_to_linear);
        }
        if let Some(rgb) = self.cloud {
            profile.cloud = rgb.map(srgb_to_linear);
        }
        if let Some(value) = self.cloud_cover {
            profile.cloud_cover = value;
        }
        if let Some(value) = self.cloud_scale {
            profile.cloud_scale = value;
        }
        if let Some(value) = self.wind_speed {
            profile.wind_speed = value;
        }
        if let Some(value) = self.haze {
            profile.haze = value;
        }
        if let Some(value) = self.stars {
            profile.stars = value;
        }
    }
}

fn load_overrides() -> Overrides {
    let config = std::env::var("ROCLIENT_CONFIG").unwrap_or_else(|_| "config/local.toml".into());
    let path = PathBuf::from(config).with_file_name("sky.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Overrides::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not read sky overrides");
            return Overrides::default();
        }
    };
    Overrides::parse(&text).unwrap_or_else(|error| {
        tracing::warn!(path = %path.display(), %error, "invalid sky overrides; using map defaults");
        Overrides::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indoor_table_parses_comments_and_normalizes_map_names() {
        let maps = indoor_maps(b"// ignored.rsw#\r\nPrt_Castle.rsw# // palace\n data\\Church.rsw#\ninvalid\n// another.rsw#\n");
        assert_eq!(
            maps,
            HashSet::from(["prt_castle".to_string(), "church".to_string()])
        );
        assert_eq!(map_key(r"data\00123#PRT_FILD08.RSW"), "prt_fild08");
        assert_eq!(map_key("1@mist.gat"), "1@mist");
        assert!(indoor_maps(b"\xef\xbb\xbfprt_castle.rsw#\n").contains("prt_castle"));
    }

    #[test]
    fn explicit_map_override_can_enable_custom_outdoors_but_never_underwater() {
        let lighting = Lighting::default();
        let enabled = MapOverride {
            enabled: Some(true),
            ..Default::default()
        };
        let disabled = MapOverride {
            enabled: Some(false),
            ..Default::default()
        };
        assert!(base_profile("custom_coast", &lighting, false, false, None).is_none());
        assert!(base_profile("custom_coast", &lighting, false, false, Some(&enabled)).is_some());
        assert!(base_profile("prontera", &lighting, false, true, None).is_none());
        assert!(base_profile("prontera", &lighting, false, true, Some(&enabled)).is_some());
        assert!(base_profile("prontera", &lighting, false, false, Some(&disabled)).is_none());
        assert!(base_profile("prontera", &lighting, true, false, Some(&enabled)).is_none());
    }

    #[test]
    fn disable_survives_invalid_unused_art_values() {
        let overrides = Overrides::parse(
            "[maps.Prontera]\nenabled = false\ncloud_cover = nan\nzenith = [2.0, 0.0, 0.0]\n",
        )
        .unwrap();
        let selected = overrides.selected("prontera");
        assert_eq!(selected.and_then(|value| value.enabled), Some(false));
        assert!(base_profile("prontera", &Lighting::default(), false, false, selected).is_none());
    }

    #[test]
    fn normalized_map_collisions_are_rejected_independently_of_source_order() {
        let entries = [
            "[maps.Prontera]\nenabled = true\n",
            "[maps.\"prontera.rsw\"]\nenabled = false\n",
        ];
        let first = Overrides::parse(&entries.concat()).err().unwrap();
        let second = Overrides::parse(&[entries[1], entries[0]].concat())
            .err()
            .unwrap();
        assert_eq!(first, second);
        assert!(first.contains("duplicate normalized map key"));
        assert!(first.contains("Prontera") && first.contains("prontera.rsw"));
        let parsed = Overrides::parse("[maps.\"42#Prontera.rsw\"]\nenabled = true\n").unwrap();
        assert!(parsed.selected("prontera").is_some());
    }

    #[test]
    fn stock_palette_is_refined_then_local_art_direction_wins() {
        let chunk = lub::eval(br#"MapSkyData = { ["prontera.rsw"] = { BG_Color = {255, 0, 0}, Star_Effect = false, BG_Fog = true, Cloud_Effect = {{Num = 10, Color = {64, 128, 255}}} } }"#).unwrap();
        let mut profile = SkyProfile::outdoor("prontera", &Lighting::default());
        let before = profile.zenith;
        apply_stock(&mut profile, &chunk, "prontera");
        assert!(profile.zenith[0] > before[0]);
        assert!(profile.zenith[2] < before[2]);
        assert_eq!(profile.stars, 0.0);
        let overrides: Overrides =
            toml::from_str("[maps.prontera]\nzenith = [0.0, 0.5, 1.0]\nstars = 0.8\n").unwrap();
        let selected = &overrides.maps["prontera"];
        selected.validate().unwrap();
        selected.apply(&mut profile);
        assert_eq!(profile.zenith, [0.0, srgb_to_linear(0.5), 1.0]);
        assert_eq!(profile.stars, 0.8);
    }

    #[test]
    fn invalid_art_values_and_typos_are_rejected() {
        for text in [
            "cloud_cover = nan",
            "haze = inf",
            "cloud_scale = 0.0",
            "zenith = [1.1, 0.0, 0.0]",
        ] {
            let value: MapOverride = toml::from_str(text).unwrap();
            assert!(value.validate().is_err(), "{text}");
        }
        assert!(toml::from_str::<Overrides>("[maps.prontera]\nclod_cover = 0.5").is_err());
        // A malformed stock record must not inject NaNs or partially replace the palette.
        let chunk = lub::eval(br#"MapSkyData = { ["prontera.rsw"] = { BG_Color = {999, 0, 0} } }"#)
            .unwrap();
        let mut profile = SkyProfile::outdoor("prontera", &Lighting::default());
        let before = profile.zenith;
        apply_stock(&mut profile, &chunk, "prontera");
        assert_eq!(profile.zenith, before);
    }
}
