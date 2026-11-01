//! Text Lua data files a server ships for its custom content, read the way the original
//! client reads them: run the chunk, then take the table it builds. Currently
//! System\itemInfo.lua (`tbl = { [id] = { identifiedDisplayName = ..., ... } }`), whose
//! entries override the stock compiled iteminfo.

use mlua::{Lua, Table, Value};
use ro_game::items::ItemInfo;

/// Text (not compiled) item info files, in the order they override each other.
/// The first is where translation packs keep the full table (their System\itemInfo.lua is
/// only a loader that runs it).
pub const ITEMINFO_SOURCES: [&str; 4] = [r"System\LuaFiles514\itemInfo.lua", r"System\itemInfo.lua", r"System\samurai_iteminfo.lua", r"System\iteminfo_custom.lua"];

fn lines(t: &Table, k: &str) -> Vec<String> {
    match t.get::<Value>(k) {
        Ok(Value::Table(d)) => d.sequence_values::<mlua::LuaString>().filter_map(Result::ok).map(|s| encoding_rs::EUC_KR.decode(&s.as_bytes()).0.into_owned()).collect(),
        _ => vec![],
    }
}

/// Runs a text itemInfo and returns its entries (empty when it is compiled or fails).
pub fn iteminfo(src: &[u8], name: &str) -> Vec<(u32, ItemInfo)> {
    if src.starts_with(b"\x1bLua") {
        return vec![];
    }
    // A translation pack's System\itemInfo.lua only loads other files (read separately).
    if src.windows(7).any(|w| w == b"dofile(") {
        tracing::debug!("{name}: a loader, skipped");
        return vec![];
    }
    let lua = Lua::new();
    // The files are EUC-KR/CP949 text; Lua strings are bytes, so decode after reading.
    if let Err(e) = lua.load(src).set_name(name).exec() {
        tracing::warn!("{name}: {e}");
        return vec![];
    }
    let Ok(tbl) = lua.globals().get::<Table>("tbl") else { return vec![] };
    let text = |t: &Table, k: &str| -> String {
        match t.get::<Value>(k) {
            Ok(Value::String(s)) => encoding_rs::EUC_KR.decode(&s.as_bytes()).0.into_owned(),
            _ => String::new(),
        }
    };
    let num = |t: &Table, k: &str| t.get::<f64>(k).unwrap_or(0.0);
    let mut out = vec![];
    for pair in tbl.pairs::<i64, Table>() {
        let Ok((id, t)) = pair else { continue };
        out.push((
            id as u32,
            ItemInfo {
                name: text(&t, "identifiedDisplayName"),
                resource: text(&t, "identifiedResourceName"),
                description: lines(&t, "identifiedDescriptionName"),
                unidentified_name: text(&t, "unidentifiedDisplayName"),
                unidentified_resource: text(&t, "unidentifiedResourceName"),
                slots: num(&t, "slotCount") as u8,
                view: num(&t, "ClassNum") as u32,
            },
        ));
    }
    tracing::info!(items = out.len(), "{name}");
    out
}

/// A quest's log text, and where its NPC stands (NpcNavi, NpcPosX, NpcPosY), as the
/// original's quest info files give them.
pub struct QuestText {
    pub title: String,
    pub summary: String,
    pub description: Vec<String>,
    pub npc: Option<(String, u16, u16)>,
}

/// Text quest info files (`QuestInfoList = { [id] = { Title = ..., Summary = ..., Description = {...} } }`).
pub const QUESTINFO_SOURCES: [&str; 3] = [r"System\OngoingQuestInfoList_True.lua", r"System\samurai_questinfo.lua", r"System\questinfo_custom.lua"];

/// Runs a text quest info file.
pub fn questinfo(src: &[u8], name: &str) -> Vec<(u32, QuestText)> {
    if src.starts_with(b"\x1bLua") {
        return vec![];
    }
    let lua = Lua::new();
    if let Err(e) = lua.load(src).set_name(name).exec() {
        tracing::warn!("{name}: {e}");
        return vec![];
    }
    let Ok(list) = lua.globals().get::<Table>("QuestInfoList") else { return vec![] };
    let text = |t: &Table, k: &str| -> String {
        match t.get::<Value>(k) {
            Ok(Value::String(s)) => encoding_rs::EUC_KR.decode(&s.as_bytes()).0.into_owned(),
            _ => String::new(),
        }
    };
    let npc = |t: &Table| {
        let map = text(t, "NpcNavi");
        let pos = |k: &str| t.get::<f64>(k).unwrap_or(0.0) as u16;
        (!map.is_empty()).then(|| (map, pos("NpcPosX"), pos("NpcPosY")))
    };
    let out: Vec<_> = list
        .pairs::<i64, Table>()
        .filter_map(Result::ok)
        .map(|(id, t)| (id as u32, QuestText { title: text(&t, "Title"), summary: text(&t, "Summary"), description: lines(&t, "Description"), npc: npc(&t) }))
        .collect();
    tracing::info!(quests = out.len(), "{name}");
    out
}

/// A server's custom jobs (job ids in `jobs::CUSTOM`).
pub const CUSTOMJOBS_SOURCE: &str = r"System\customjobs.lua";

/// Runs a custom jobs file (`CustomJobs = { [id] = { sprite = "...", name = "..." } }`): each
/// job's sprite name (its body, weapon and palette files are named after it, as for stock
/// jobs) and display name.
pub fn custom_jobs(src: &[u8], name: &str) -> Vec<(i16, String, String)> {
    let lua = Lua::new();
    if let Err(e) = lua.load(src).set_name(name).exec() {
        tracing::warn!("{name}: {e}");
        return vec![];
    }
    let Ok(list) = lua.globals().get::<Table>("CustomJobs") else { return vec![] };
    let text = |t: &Table, k: &str| match t.get::<Value>(k) {
        Ok(Value::String(s)) => encoding_rs::EUC_KR.decode(&s.as_bytes()).0.into_owned(),
        _ => String::new(),
    };
    let out: Vec<_> = list
        .pairs::<i64, Table>()
        .filter_map(Result::ok)
        .filter(|(id, _)| crate::jobs::CUSTOM.contains(&(*id as i16)))
        .map(|(id, t)| (id as i16, text(&t, "sprite"), text(&t, "name")))
        .filter(|(_, sprite, _)| !sprite.is_empty())
        .collect();
    tracing::info!(jobs = out.len(), "{name}");
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn custom_jobs_need_a_sprite_and_an_id_in_the_range() {
        let src = "CustomJobs = { [4501] = { sprite = \"ronin\", name = \"Ronin\" }, [4502] = { name = \"No sprite\" }, [4001] = { sprite = \"x\" } }";
        assert_eq!(super::custom_jobs(src.as_bytes(), "test"), [(4501, "ronin".to_string(), "Ronin".to_string())]);
    }

    #[test]
    fn saga_quests_know_where_to_go() {
        let src = include_bytes!("../../../overlays/samurai/System/samurai_questinfo.lua");
        let quests = super::questinfo(src, "samurai_questinfo.lua");
        assert_eq!(quests.len(), 23);
        assert!(quests.iter().all(|(_, q)| q.npc.is_some()));
        let raider = quests.iter().find(|(id, _)| *id == 900112).map(|(_, q)| q.npc.clone());
        assert_eq!(raider, Some(Some(("amatsu".to_string(), 187, 158))));
    }
}
