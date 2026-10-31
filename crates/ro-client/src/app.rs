// Tenseiga: the client (included by main.rs for the desktop exe and lib.rs for Android).

mod audio;
mod book;
mod captcha;
mod memorial;
mod roulette;
mod sale;
mod petevo;
mod skillscale;
mod sense;
mod pettalk;
mod fx;
mod gamepad;
mod guild;
mod hud;
mod iteminfo;
mod itemlink;
mod jobs;
mod saga;
mod samurai;
mod mail;
mod msgstr;
mod merchant;
mod preview;
mod sky;
mod progress;
mod quests;
mod refine;
mod screens;
mod ai;
mod agency;
mod booking;
mod battle;
mod clan;
mod merge;
mod reputation;
mod gameopts;
mod ranking;
mod storesearch;
mod stylist;
mod lapine;
mod mapfix;
mod models3d;
mod reform;
mod enchant;
mod market;
mod customdata;
mod navigation;
mod npcmarkup;
mod grade;
mod buffs;
mod chatrooms;
mod classic;
mod companions;
mod emblem;
mod emotes;
mod settings;
mod padglyph;
mod perf;
mod haptics;
mod hidpad;
mod hudlayout;
mod shortcuts;
mod skilltree;
mod status_icons;
mod stores;
mod splash;
mod social;
mod uitex;
mod windows;

use uitex::UiSize;

use std::{path::PathBuf, sync::Arc};

use anyhow::Context;
use serde::Deserialize;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    window::{Window, WindowId},
};

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_login_addr")]
    login_addr: String,
    username: String,
    password: String,
    /// PIN code for auto-login on servers with pincode_enabled (4 digits).
    #[serde(default)]
    pincode: String,
    #[serde(default)]
    char_slot: u8,
    #[serde(default = "default_backend")]
    backend: String,
    /// --headless: how long to stay on the map after entering.
    #[serde(default = "default_soak")]
    soak_secs: u64,
    /// UI size multiplier on top of the display's DPI scale.
    #[serde(default = "default_ui_scale")]
    ui_scale: f32,
    /// Skip the login screen and log in with username / password / char_slot.
    #[serde(default)]
    auto_login: bool,
    /// MSAA sample count (1, 2, 4); falls back to what the GPU supports.
    #[serde(default = "default_msaa")]
    msaa: u32,
    /// Music and sound effect volume, 0..1 (0 mutes).
    #[serde(default = "default_bgm_volume")]
    bgm_volume: f32,
    #[serde(default = "default_sfx_volume")]
    sfx_volume: f32,
    /// Installed RO client folder; its `data/` is checked before the GRFs.
    client_dir: Option<PathBuf>,
    /// GRFs inside `client_dir`, highest priority first.
    #[serde(default = "default_grfs")]
    grfs: Vec<String>,
    /// Folders laid over the client data, first wins: an unpacked translation pack such as
    /// ROenglishRE's Translation/Renewal (its SystemEN stands in for System).
    #[serde(default)]
    overlays: Vec<PathBuf>,
    /// Map shown in the window until it follows the network session.
    #[serde(default = "default_map")]
    map: String,
    /// rAthena's web server (Adventurer's Agency, …); defaults to the login host on port 8888.
    #[serde(default)]
    web_addr: Option<String>,
    /// Game Master: adds the GM actions (kick, mute, recall, …) to the player right-click menu.
    /// The GM slash commands work regardless; the server checks permissions either way.
    #[serde(default)]
    gm: bool,
    /// As the original's clientinfo.xml <aid>: accounts drawn with the GM sprite, unless their
    /// GM turned it off with @gmlook (see `gm_sprite`).
    #[serde(default)]
    gm_aids: Vec<u32>,
    /// As clientinfo.xml <yellow>: accounts whose names show in yellow.
    #[serde(default)]
    yellow_aids: Vec<u32>,
}

fn default_grfs() -> Vec<String> {
    vec!["data.grf".into()]
}

fn default_map() -> String {
    "prontera".into()
}

fn open_vfs(cfg: &Config) -> anyhow::Result<Option<ro_vfs::Vfs>> {
    let Some(dir) = &cfg.client_dir else { return Ok(None) };
    let mut overlays: Vec<PathBuf> = cfg.overlays.iter().filter(|p| p.is_dir() || { tracing::warn!(path = %p.display(), "overlay folder not found, skipping"); false }).cloned().collect();
    if let Some(path) = samurai::overlay_path() {
        if !overlays.contains(&path) {
            tracing::info!(path = %path.display(), "bundled Samurai assets");
            overlays.push(path);
        }
    }
    let mut vfs = ro_vfs::Vfs::new(overlays, Some(dir.clone()), vec![]);
    for name in &cfg.grfs {
        let path = dir.join(name);
        if path.is_file() {
            vfs.grfs.push(ro_vfs::Grf::open(&path)?);
        } else {
            tracing::warn!(path = %path.display(), "GRF not found, skipping");
        }
    }
    Ok(Some(vfs))
}

/// Character appearance, as the char server describes it.
#[derive(Clone)]
struct Look {
    /// Job sprite name (Korean, as in the GRF), e.g. 초보자 for Novice.
    job: String,
    male: bool,
    hair: u16,
    hair_color: u16,
    gear: ro_net::Look,
}

/// Loads body, head (with hair dye), worn headgear and weapon, and shadow sheets.
fn load_character(
    vfs: &ro_vfs::Vfs,
    r: &mut ro_render::Renderer,
    look: &Look,
    tables: &ro_game::looks::LookTables,
) -> anyhow::Result<ro_render::Actor> {
    let sex = if look.male { "남" } else { "여" };
    let mut sheet = |base: &str, palette: Option<String>| -> anyhow::Result<ro_render::SheetId> {
        let read = |p: String| vfs.read(&p)?.with_context(|| format!("{p} not found"));
        let spr = ro_formats::spr::Spr::parse(&read(format!("{base}.spr"))?)?;
        let act = ro_formats::act::Act::parse(&read(format!("{base}.act"))?)?;
        // A missing dye palette falls back to the sprite's own colors.
        let pal = palette.and_then(|p| vfs.read(&p).ok().flatten()).and_then(|d| ro_formats::spr::parse_palette(&d).ok());
        Ok(r.add_sheet(&spr, act, pal.as_ref()))
    };
    // Doram jobs have their own sprite and hair palette folders.
    let race = jobs::race_dir(&look.job);
    let pal_dir = if race == "도람족" { "palette\\도람족\\머리" } else { "palette\\머리" };
    let appearance = jobs::appearance(&look.job, look.male)
        .with_context(|| format!("{} has no {} sprite package", look.job, if look.male { "male" } else { "female" }))?;
    let body = sheet(&appearance.body, None)?;
    let head = if appearance.attachments { Some(sheet(
        &format!("data\\sprite\\{race}\\머리통\\{sex}\\{}_{sex}", look.hair),
        (look.hair_color > 0).then(|| format!("data\\{pal_dir}\\머리{}_{sex}_{}.pal", look.hair, look.hair_color)),
    )?) } else { None };
    let shadow = sheet("data\\sprite\\shadow", None).inspect_err(|e| tracing::warn!("shadow sprite: {e:#}")).ok();
    // Equipment is optional: a missing sprite just isn't drawn.
    let g = look.gear;
    let mut headgear = vec![];
    for view in [g.head_bottom, g.head_mid, g.head_top] {
        if view == 0 || !appearance.attachments {
            continue;
        }
        // A headgear on several slots (e.g. top+mid) is drawn once.
        if let Some(name) = tables.accessory(view as u32) {
            match sheet(&format!("data\\sprite\\악세사리\\{sex}\\{sex}{name}"), None) {
                Ok(id) if !headgear.contains(&id) => headgear.push(id),
                Ok(_) => {}
                Err(e) => tracing::debug!(view, "headgear sprite: {e:#}"),
            }
        }
    }
    // Weapon, then its slash trail (_검광). Two-handed katars are named twice
    // (어세신_남_카타르_카타르), as both hands hold one.
    let mut weapon = vec![];
    if look.job == samurai::SPRITE {
        // Dual wielding: the left-hand katana (an item id) is drawn just before (under) the main blade.
        if let Some(base) = samurai::offhand(g.shield) {
            tracing::debug!(item = g.shield, base, "offhand sprite");
            weapon.extend(sheet(base, None).inspect_err(|e| tracing::warn!("offhand sprite: {e:#}")).ok());
        }
        if let Some(base) = samurai::weapon(g.weapon) {
            weapon.push(sheet(base, None)?);
        }
    } else if let Some(name) = (g.weapon != 0).then(|| tables.weapon(g.weapon)).flatten() {
        let wj = jobs::weapon_dir(&look.job);
        let dir = format!("data\\sprite\\{race}\\{wj}\\{wj}_{sex}");
        let found = [format!("{dir}{name}"), format!("{dir}{name}{name}")].into_iter().find_map(|base| sheet(&base, None).ok().map(|id| (id, base)));
        tracing::debug!(%dir, %name, found = found.is_some(), "weapon sprite");
        match found {
            Some((id, base)) => {
                weapon.push(id);
                if let Ok(trail) = sheet(&format!("{base}_검광"), None) {
                    weapon.push(trail);
                }
            }
            None => tracing::debug!(weapon = g.weapon, %name, "no weapon sprite"),
        }
    }
    Ok(ro_render::Actor {
        pos: Default::default(),
        body,
        head,
        headgear,
        weapon,
        shadow,
        action: 0,
        dir: ro_render::Dir::S,
        time_ms: 0.0,
        once: false,
        alpha: 1.0, scale: 1.0, tint: [1.0; 3],
    })
}

struct LoadedMap {
    mesh: ro_render::SceneMesh,
    images: Vec<Option<ro_render::Rgba>>,
    lighting: ro_render::Lighting,
    /// Light sources (torches, lamps) in world space.
    lights: Vec<ro_render::PointLight>,
    water: Option<(ro_render::WaterMesh, Vec<Option<ro_render::Rgba>>)>,
    /// Ambient effect emitters (EF_* id, world position): torches, bubbles, fireflies.
    effects: Vec<(i32, glam::Vec3, f32, [f32; 4])>,
}

/// `up` upscales ground, water and model textures (never sprites, which load elsewhere).
/// Server maps that are copies of a stock map (its cells under a new name in the server's map
/// cache, rathena db/import/map_cache.dat): the client draws the original's files.
const MAP_COPIES: [(&str, &str); 1] = [
    // Twin Flames: the battle against the Goddess.
    ("tf_goddess", "herosria"),
];

/// The map whose files draw `name` (itself, unless it is a copy).
fn map_files(name: &str) -> &str {
    MAP_COPIES.iter().find(|(copy, _)| name.eq_ignore_ascii_case(copy)).map_or(name, |(_, original)| original)
}

fn load_map(vfs: &ro_vfs::Vfs, name: &str, up: &ro_render::Upscaler) -> anyhow::Result<LoadedMap> {
    let started = std::time::Instant::now();
    let read = |path: String| vfs.read(&path)?.with_context(|| format!("{path} not found"));
    let rsw = ro_formats::rsw::Rsw::parse(&read(format!("data\\{name}.rsw"))?)?;
    let gnd_name = if rsw.gnd.is_empty() { format!("{name}.gnd") } else { rsw.gnd.clone() };
    let mut gnd = ro_formats::gnd::Gnd::parse(&read(format!("data\\{gnd_name}"))?)?;
    let fixed = mapfix::ground(name, &mut gnd);
    if fixed > 0 {
        tracing::info!(map = name, tiles = fixed, "ground fixes");
    }

    // Each model file is parsed once and shared by all its placements.
    let mut models: std::collections::HashMap<String, Option<ro_formats::rsm::Rsm>> = Default::default();
    let mut builder = ro_render::SceneBuilder::new(&gnd);
    let mut failed = 0;
    for inst in &rsw.models {
        let rsm = models.entry(inst.file.to_ascii_lowercase()).or_insert_with(|| {
            let path = format!("data\\model\\{}", inst.file);
            match vfs.read(&path).map(|d| d.map(|d| ro_formats::rsm::Rsm::parse(&d))) {
                Ok(Some(Ok(m))) => Some(m),
                Ok(Some(Err(e))) => {
                    tracing::warn!(%path, "model unreadable: {e:#}");
                    None
                }
                Ok(None) => {
                    tracing::warn!(%path, "model missing");
                    None
                }
                Err(e) => {
                    tracing::warn!(%path, "model unreadable: {e:#}");
                    None
                }
            }
        });
        match rsm {
            Some(m) => builder.add_model(m, inst),
            None => failed += 1,
        }
    }
    let mesh = builder.finish();

    let load_textures = |names: &[String]| -> Vec<Option<ro_render::Rgba>> {
        names
            .iter()
            .map(|t| {
                let path = format!("data\\texture\\{t}");
                match vfs.read(&path).and_then(|d| d.map(|d| ro_formats::image::decode(&d, &path)).transpose()) {
                    Ok(Some(img)) => Some(img),
                    Ok(None) => {
                        tracing::warn!(%path, "texture missing");
                        None
                    }
                    Err(e) => {
                        tracing::warn!(%path, "texture unreadable: {e:#}");
                        None
                    }
                }
            })
            .collect()
    };
    // Ground textures come first in the mesh's list, then the models'.
    let classes = (0..mesh.textures.len()).map(|i| if i < mesh.ground_textures { ro_render::AssetClass::Ground } else { ro_render::AssetClass::Model });
    let images = up.apply_all(classes.zip(mesh.textures.iter().cloned()).zip(load_textures(&mesh.textures)).map(|((c, n), img)| (c, n, img)).collect());
    // RSW 2.6+ moved the water into the GND (1.8+).
    let water = rsw.water.or(gnd.water).as_ref().map(|w| {
        let mesh = ro_render::WaterMesh::build(&gnd, w);
        let images = up.apply_all(mesh.textures.iter().cloned().zip(load_textures(&mesh.textures)).map(|(n, img)| (ro_render::AssetClass::Ground, n, img)).collect());
        (mesh, images)
    });
    tracing::info!(
        map = name,
        size = format_args!("{}x{}", gnd.width, gnd.height),
        models = rsw.models.len() - failed,
        failed,
        textures = mesh.textures.len(),
        triangles = mesh.vertices.len() / 3,
        ms = started.elapsed().as_millis() as u64,
        "loaded map"
    );
    // RSW light positions are centred on the map with y pointing down, like model placements.
    let lights = rsw
        .lights
        .iter()
        .map(|l| ro_render::PointLight {
            pos: glam::Vec3::new(l.pos[0] + gnd.width as f32 * gnd.zoom / 2.0, -l.pos[1], l.pos[2] + gnd.height as f32 * gnd.zoom / 2.0),
            color: l.color,
            range: l.range,
        })
        .collect();
    let effects = rsw.effects.iter().map(|e| (e.id, glam::Vec3::new(e.pos[0] + gnd.width as f32 * gnd.zoom / 2.0, -e.pos[1], e.pos[2] + gnd.height as f32 * gnd.zoom / 2.0), e.delay, e.params)).collect();
    Ok(LoadedMap { mesh, images, water, lighting: ro_render::Lighting::from_rsw(&rsw.light), lights, effects })
}

fn default_ui_scale() -> f32 {
    1.0
}

fn default_msaa() -> u32 {
    4
}

fn default_bgm_volume() -> f32 {
    0.5
}

fn default_sfx_volume() -> f32 {
    0.7
}

fn default_soak() -> u64 {
    30
}

fn default_backend() -> String {
    "vulkan".into()
}

fn default_login_addr() -> String {
    "127.0.0.1:6900".into()
}

/// Accepts `host`, `host:port`, `[v6]` or `[v6]:port`; appends the default login port when absent.
fn normalize_login_addr(s: &str) -> String {
    let s = s.trim();
    let has_port = match s.strip_prefix('[') {
        Some(rest) => rest.split_once(']').is_some_and(|(_, tail)| tail.starts_with(':')),
        None => s.matches(':').count() == 1,
    };
    if has_port {
        s.into()
    } else if s.contains(':') && !s.starts_with('[') {
        format!("[{s}]:6900") // bare IPv6
    } else {
        format!("{s}:6900")
    }
}

/// The web server's base URL: `web_addr` as given (scheme optional), else the login host on 8888.
fn web_url(web_addr: Option<&str>, login_addr: &str) -> String {
    let addr = match web_addr.map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => a.to_string(),
        None => {
            let host = login_addr.rsplit_once(':').map_or(login_addr, |(h, _)| h);
            format!("{host}:8888")
        }
    };
    let addr = addr.trim_end_matches('/');
    if addr.contains("://") { addr.to_string() } else { format!("http://{addr}") }
}

fn load_config() -> anyhow::Result<Config> {
    let path = std::env::var("ROCLIENT_CONFIG").unwrap_or_else(|_| "config/local.toml".into());
    let text = std::fs::read_to_string(&path).with_context(|| format!("read {path} (copy config/example.toml)"))?;
    Ok(toml::from_str(&text)?)
}

async fn headless(cfg: &Config) -> anyhow::Result<()> {
    let auth = ro_net::login(&cfg.login_addr, &cfg.username, &cfg.password).await?;
    tracing::info!(account_id = auth.account_id, servers = auth.servers.len(), "logged in");
    let server = auth.servers.first().context("no char servers offered")?;
    tracing::info!(name = %server.name, addr = %server.addr, users = server.users, "char server");

    let chars = ro_net::enter_char(server, &auth).await?;
    for c in &chars.characters {
        tracing::info!(slot = c.slot, name = %c.name, job = c.job, lv = c.base_level, map = %c.map, "character");
    }
    let zone = chars.select(cfg.char_slot).await?;
    tracing::info!(map = %zone.map, addr = %zone.addr, "zone server");

    let mut map = ro_net::enter_map(&zone, &auth).await?;
    tracing::info!(x = map.spawn.x, y = map.spawn.y, dir = map.spawn.dir, "entered map");

    let seen = map.soak(std::time::Duration::from_secs(cfg.soak_secs)).await?;
    let total: usize = seen.values().sum();
    tracing::info!(packets = total, kinds = seen.len(), secs = cfg.soak_secs, "stayed connected");
    for (op, n) in seen {
        tracing::info!("  {op:#06x} x{n}");
    }
    Ok(())
}

/// Sprite name (as in the GRF) for a job id. Covers Novice through the 2-1/2-2 classes.
/// Display name for a job id (Novice through the 2-1/2-2 classes).
fn job_title(job: i16) -> &'static str {
    jobs::title(job)
}

fn job_sprite(job: i16) -> &'static str {
    jobs::sprite(job)
}

const DIRS: [ro_render::Dir; 8] = {
    use ro_render::Dir::*;
    [S, SW, W, NW, N, NE, E, SE]
};

/// The map currently shown: its geometry (for picking/heights) and walkability.
struct World {
    name: String,
    /// Confirmed exterior (including an explicit sky override for a custom map).
    outdoors: bool,
    ground: ro_render::SceneMesh,
    gat: Option<ro_formats::gat::Gat>,
    /// Indoor maps: the cells of the room we are in, plus its walls (one byte per cell).
    room: Option<Vec<u8>>,
    /// A sunken map (dense bubble emitters, like Byalan's lower levels): drawn underwater.
    submerged: bool,
}

/// Cells around a room's floor still drawn, so its walls and furniture show.
const ROOM_MARGIN: i32 = 5;

impl World {
    /// Interior maps pack many rooms (linked by warps) into one map.
    /// Height (y-up) of what actors stand on at a world point: the GAT surface, which
    /// includes stairs, bridges and platforms above the drawn ground, else the ground.
    fn floor_at(&self, x: f32, z: f32) -> f32 {
        self.gat.as_ref().and_then(|g| g.height_at(x / ro_render::CELL, z / ro_render::CELL)).map_or_else(|| self.ground.height_at(x, z), |h| -h)
    }

    fn indoor(&self) -> bool {
        !self.outdoors && self.name.contains("_in")
    }

    /// Whether a cell is drawn (always, outdoors).
    fn shows_cell(&self, x: i32, y: i32) -> bool {
        match (&self.room, &self.gat) {
            (Some(room), Some(g)) => x >= 0 && y >= 0 && (x as u32) < g.width && (y as u32) < g.height && room[(y as u32 * g.width + x as u32) as usize] != 0,
            _ => true,
        }
    }

    /// The room containing `start`: walkable cells reachable from it, grown by ROOM_MARGIN.
    fn room_of(&self, start: (i32, i32)) -> Option<Vec<u8>> {
        let g = self.gat.as_ref()?;
        let (w, h) = (g.width as i32, g.height as i32);
        if !g.walkable(start.0, start.1) {
            return None;
        }
        let mut floor = vec![false; (w * h) as usize];
        let mut stack = vec![start];
        floor[(start.1 * w + start.0) as usize] = true;
        while let Some((x, y)) = stack.pop() {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < w && ny < h && !floor[(ny * w + nx) as usize] && g.walkable(nx, ny) {
                    floor[(ny * w + nx) as usize] = true;
                    stack.push((nx, ny));
                }
            }
        }
        // Grow by the margin (separable: rows, then columns).
        let mut rows = vec![false; floor.len()];
        for y in 0..h {
            for x in 0..w {
                rows[(y * w + x) as usize] = (x - ROOM_MARGIN..=x + ROOM_MARGIN).any(|sx| sx >= 0 && sx < w && floor[(y * w + sx) as usize]);
            }
        }
        let mut out = vec![0u8; floor.len()];
        for y in 0..h {
            for x in 0..w {
                if (y - ROOM_MARGIN..=y + ROOM_MARGIN).any(|sy| sy >= 0 && sy < h && rows[(sy * w + x) as usize]) {
                    out[(y * w + x) as usize] = 255;
                }
            }
        }
        Some(out)
    }
}

/// Fractional cell (centre plus the server's twelfth-of-a-cell offset) of a floor item.
fn floor_cell(f: &ro_net::FloorItem) -> (f32, f32) {
    (f.x as f32 + f.sub.0 as f32 / 12.0, f.y as f32 + f.sub.1 as f32 / 12.0)
}

fn lapine_command(a: lapine::LapineAction) -> ro_net::Command {
    ro_net::Command::Lapine(match a {
        lapine::LapineAction::Synthesize { trigger, items } => ro_net::LapineRequest::Synthesize { trigger, items },
        lapine::LapineAction::Upgrade { trigger, index } => ro_net::LapineRequest::Upgrade { trigger, index },
        lapine::LapineAction::Close { upgrade: false } => ro_net::LapineRequest::SynthesisClose,
        lapine::LapineAction::Close { upgrade: true } => ro_net::LapineRequest::UpgradeClose,
    })
}

fn inv_item(d: ro_net::ItemData) -> ro_game::items::InvItem {
    ro_game::items::InvItem {
        index: d.index,
        id: d.id,
        kind: d.kind,
        amount: d.amount,
        location: d.location,
        equipped: d.equipped,
        refine: d.refine,
        grade: d.grade,
        switch: 0,
        identified: d.identified,
        cards: d.cards,
        look: d.look,
        favorite: d.favorite,
    }
}

/// Korean text from the game data (some status descriptions, names) is drawn with a system
/// font after egui's own, as the original client draws it with Windows' fonts; without one
/// it shows as empty boxes.
fn install_fallback_fonts(ctx: &egui::Context) {
    let dir = std::path::PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into())).join("Fonts");
    // Windows, then Android's and the usual Linux (SteamOS) CJK fonts.
    let candidates = [
        dir.join("malgun.ttf"),
        dir.join("gulim.ttc"),
        "/system/fonts/NotoSansCJK-Regular.ttc".into(),
        "/system/fonts/NotoSansKR-Regular.otf".into(),
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc".into(),
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc".into(),
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc".into(),
    ];
    let Some(data) = candidates.iter().find_map(|f: &std::path::PathBuf| std::fs::read(f).ok()) else {
        tracing::warn!("no Korean system font found; Korean text will show as boxes");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("korean".into(), egui::FontData::from_owned(data).into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts.families.entry(family).or_default().push("korean".into());
    }
    ctx.set_fonts(fonts);
}

/// Decodes an image from the VFS into an egui texture.
/// An interface texture (item icons, window art...), upscaled per the settings. Size it with
/// `ui_size()`, never `size_vec2()`. Not for sprites: they never load through here.
fn load_texture(ctx: &egui::Context, vfs: &ro_vfs::Vfs, path: &str) -> Option<egui::TextureHandle> {
    uitex::load(ctx, vfs, path, egui::TextureOptions::NEAREST)
}

/// What `showdigit` shows `t` ms after it arrived: 0 the value for 5 s, 1 counting up, 2
/// counting down (past zero), 3 a two-digit countdown at 2 ticks/s that stops at zero. All
/// but type 3 are seconds, shown as days/hours/minutes/seconds.
fn digit_text(kind: u8, value: i32, t: f32) -> String {
    let secs = (t / 1000.0) as i64;
    let v = match kind {
        // The server sends counters' start negated (as the original client expects).
        1 => (value as i64).abs() + secs,
        2 => (value as i64).abs() - secs,
        3 => return format!("{:02}", (value as i64 - (t / 500.0) as i64).clamp(0, 99)),
        _ => value as i64,
    };
    let (sign, v) = if v < 0 { ("-", -v) } else { ("", v) };
    let (d, h, m, s) = (v / 86400, v / 3600 % 24, v / 60 % 60, v % 60);
    if d > 0 { format!("{sign}{d}d {h:02}:{m:02}:{s:02}") } else { format!("{sign}{h:02}:{m:02}:{s:02}") }
}

/// Expanded-cell budget when planning a long (multi-hop) walk.
const LONG_WALK_NODES: usize = 200_000;
/// Steps per server walk request. rAthena refuses walks longer than `max_walk_path`
/// (conf/battle/client.conf, 17 by default); stay well under it, since the server's
/// position can trail the client's by a cell or two when the next hop is sent.
const WALK_HOP: usize = 12;

/// Controller: how far (cells) targets are picked up, and how far ahead the stick walks.
const PAD_REACH: i32 = 14;
const PAD_STEP: i32 = 8;

use windows::{HOTKEYS_PER_ROW, HOTKEY_ROWS};

/// F1..F9 -> slot within the visible shortcut row.
/// The skill rAthena keeps using while its key is held (CZ_USE_SKILL_START): Rolling Cutter.
const HOLD_SKILL: u16 = 2036;

fn hotkey_slot(k: winit::keyboard::NamedKey) -> Option<usize> {
    use winit::keyboard::NamedKey::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9].iter().position(|&f| f == k)
}

/// The next free ScreenShot\screenNNNN.png in the working directory (the original numbers
/// its shots in a ScreenShot folder beside the client too).
fn screenshot_path() -> PathBuf {
    let dir = std::env::current_dir().unwrap_or_default().join("ScreenShot");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(dir = %dir.display(), "screenshot folder: {e}");
    }
    (0..).map(|n| dir.join(format!("screen{n:04}.png"))).find(|p| !p.exists()).unwrap_or_default()
}

/// Battle mode: the shortcut slot a key uses. `rows` holds the keys of the row after the one
/// on show, the next and the one after (Q ~ O, A ~ L, Z ~ > by default); rows wrap around.
fn battle_slot(key: &str, page: u8, rows: &[String]) -> Option<usize> {
    let mut chars = key.chars().map(|c| c.to_ascii_lowercase());
    let c = match (chars.next()?, chars.next()) {
        // Shift held: < and > are still the , and . keys.
        ('<', None) => ',',
        ('>', None) => '.',
        (c, None) => c,
        _ => return None,
    };
    rows.iter().take(HOTKEY_ROWS as usize - 1).enumerate().find_map(|(i, row)| {
        let col = row.chars().take(HOTKEYS_PER_ROW).position(|k| k.to_ascii_lowercase() == c)?;
        Some((page as usize + i + 1) % HOTKEY_ROWS as usize * HOTKEYS_PER_ROW + col)
    })
}

/// A clicked destination being reached through one or more server walks.
struct WalkGoal {
    cell: (u16, u16),
    sent_at: Option<std::time::Instant>,
    /// Hops sent without the previous one moving us; gives up after a few.
    tries: u32,
}

/// Anything that stands and walks on the map: the player, other players, NPCs, monsters.
struct Mob {
    actor: ro_render::Actor,
    walker: ro_game::Walker,
    /// Name shown under the actor (empty: none).
    name: String,
    /// Unit object type (0 player, 1/6 NPC, 5 monster, ...).
    kind: u8,
    /// Uses the player body/head sprites (whose action groups differ from monsters').
    human: bool,
    /// A timed pose (attack, hurt) overriding idle: (pose, remaining ms).
    pose: Option<(Pose, f32)>,
    sitting: bool,
    /// Playing the death animation; removed when it runs out (None for the player).
    dying: Option<f32>,
    hp: Option<(i32, i32)>,
    /// Player-sprite appearance (None for NPCs and monsters).
    look: Option<Look>,
    /// Players: (party, guild, guild rank) from ZC_ACK_REQNAMEALL, shown under the name.
    titles: Option<(String, String, String)>,
    /// Unit class (job / monster id).
    class: i16,
    /// Guild id (0: none), for the emblem beside the name.
    guild: u32,
    /// Drawn position (cells), eased toward the walker's.
    smooth: Option<(f32, f32)>,
    /// Player attack motion: 10, or 11 for weapons drawn only there (katars).
    attack_group: usize,
    /// Length of the current timed pose, and how fast its animation must play to fit.
    pose_len: f32,
    anim_rate: f32,
    /// Base level as the server reports it (players at 99+ carry the level aura).
    level: u16,
    /// How long the aura has been showing (its ribbons grow in), ms.
    aura_ms: f32,
    /// Name font (players), which carries the GM look; and whether the name shows in yellow.
    font: u16,
    yellow: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Pose {
    Attack,
    Hurt,
    PickUp,
    Dead,
    /// Chanting a skill (players only; monsters stand still).
    Cast,
}

/// @gmlook (npc/custom/gm_look.txt) keeps a GM's choice in their name font, which the server
/// sends to everyone who sees them: on, or off even where the viewer's lists name them.
const GM_LOOK_ON: u16 = 200;
const GM_LOOK_OFF: u16 = 201;

/// Whether a unit object type is an NPC: 1 / 6, or 12 for an NPC drawn with a monster
/// sprite (rAthena sends that type since 2017-07-26 clients).
fn is_npc(kind: u8) -> bool {
    matches!(kind, 1 | 6 | 12)
}

/// Keep one clock for the action actually drawn. Movement can temporarily override a
/// timed pose, so its fitted attack rate must not accelerate the walking frames.
fn animation_time(previous_action: usize, action: (usize, bool), time_ms: f32, dt: f32, pose_rate: f32) -> f32 {
    if previous_action != action.0 {
        0.0
    } else {
        time_ms + dt * if action.1 { pose_rate } else { 1.0 }
    }
}

impl Mob {
    fn new(actor: ro_render::Actor, cell: (u16, u16), dir: usize, speed: i16) -> Self {
        let speed = if speed > 0 { speed as f32 } else { 150.0 };
        Self {
            actor,
            walker: ro_game::Walker::new((cell.0 as i32, cell.1 as i32), dir, speed),
            name: String::new(),
            kind: 0,
            human: true,
            pose: None,
            sitting: false,
            dying: None,
            hp: None,
            look: None,
            titles: None,
            class: 0,
            guild: 0,
            smooth: None,
            attack_group: 10,
            pose_len: 0.0,
            anim_rate: 1.0,
            level: 0,
            aura_ms: 0.0,
            font: 0,
            yellow: false,
        }
    }

    /// Action group for a pose: player sprites and monster sprites order them differently.
    fn action_for(&self, pose: Pose) -> usize {
        match (self.human, pose) {
            (true, Pose::Attack) => self.attack_group,
            (true, Pose::Hurt) => 6,
            (true, Pose::Dead) => 8,
            (true, Pose::Cast) => 12,
            (false, Pose::Cast) => 0,
            (false, Pose::Attack) => 2,
            (false, Pose::Hurt) => 3,
            (false, Pose::Dead) => 4,
            (_, Pose::PickUp) => 3,
        }
    }

    /// Speeds a timed pose's animation up so all its frames show before the pose ends, as
    /// the original client does (attack motions follow attack speed; katar blades appear
    /// only in the later frames). Death keeps its own pace.
    fn fit_pose(&mut self, r: &ro_render::Renderer) {
        let Some((pose, _)) = self.pose else { return };
        if pose == Pose::Dead || self.pose_len <= 0.0 {
            return;
        }
        let index = self.action_for(pose) * 8 + self.actor.dir as usize;
        let full = r.action_ms(self.actor.body, index);
        self.anim_rate = (full / self.pose_len).clamp(1.0, 8.0);
    }

    fn set_pose(&mut self, pose: Pose, ms: f32) {
        self.pose = Some((pose, ms.max(200.0)));
        self.pose_len = ms.max(200.0);
        self.anim_rate = 1.0;
        if !self.walker.is_walking() || pose == Pose::Dead {
            self.actor.time_ms = 0.0;
        }
    }

    /// Turns to face a cell.
    fn face(&mut self, cell: (i32, i32)) {
        let c = self.walker.cell;
        if c != cell {
            self.walker.dir = ro_game::dir_from_step(cell.0 - c.0, cell.1 - c.1);
        }
    }

    /// Name label colour, or None when the original client would not show one
    /// (a monster's name shows only while the cursor is over it).
    fn label_color(&self, hovered: bool) -> Option<egui::Color32> {
        if self.name.is_empty() {
            return None;
        }
        match self.kind {
            0 if self.yellow => Some(egui::Color32::from_rgb(255, 255, 0)),
            0 => Some(egui::Color32::WHITE),
            k if is_npc(k) => Some(egui::Color32::from_rgb(160, 225, 255)),
            5 if hovered && self.dying.is_none() => Some(egui::Color32::WHITE),
            _ => None,
        }
    }

    /// Starts walking from -> to along the client-side path (as the server will).
    fn walk(&mut self, gat: Option<&ro_formats::gat::Gat>, from: (u16, u16), to: (u16, u16)) {
        let from = (from.0 as i32, from.1 as i32);
        let to = (to.0 as i32, to.1 as i32);
        // A re-aimed walk whose start is close to where we are (the cell we are leaving, the
        // one we are stepping into, or one behind when the server took the new target as we
        // crossed a cell) goes on from our own position: finish the step in progress, then
        // path from its end. Starting over from the server's cell would jump us back. The
        // walk ends in the same place either way.
        // It rejoins the server's own route (the path it walks from its cell) where we
        // already are on it, or at the furthest of its first cells next to us, so the two
        // keep to the same cells rather than drifting apart over re-aims.
        let here = self.walker.next().unwrap_or(self.walker.cell);
        let dist = |a: (i32, i32), b: (i32, i32)| (a.0 - b.0).abs().max((a.1 - b.1).abs());
        if let (true, Some(g)) = (self.walker.is_walking() && (dist(from, self.walker.cell) <= 2 || dist(from, here) <= 2), gat) {
            if let Some(server) = ro_game::path::find(g, from, to) {
                let route: Vec<(i32, i32)> = std::iter::once(from).chain(server).collect();
                if let Some(j) = route.iter().position(|&c| c == here) {
                    self.walker.continue_with(route[j + 1..].to_vec());
                    return;
                }
                if let Some(j) = route.iter().take(4).rposition(|&c| dist(c, here) == 1) {
                    if let Some(link) = ro_game::path::find(g, here, route[j]).filter(|l| l.len() == 1) {
                        self.walker.continue_with(link.into_iter().chain(route[j + 1..].iter().copied()).collect());
                        return;
                    }
                }
            }
            if let Some(path) = ro_game::path::find(g, here, to) {
                self.walker.continue_with(path);
                return;
            }
        }
        match gat.and_then(|g| ro_game::path::find(g, from, to)) {
            // Continue smoothly when the new walk starts where we are (re-aimed walks).
            Some(path) => {
                if !self.walker.redirect(from, path.clone()) {
                    self.walker.stop_at(from);
                    self.walker.walk(path);
                }
            }
            None => self.walker.stop_at(to),
        }
    }

    /// Advances movement and returns the actor to draw. `idle_action` is the pose when
    /// standing; facing is drawn relative to the camera's rotation (`octant`).
    fn update(&mut self, dt: f32, world: &World, octant: i32, idle_action: usize) -> ro_render::Actor {
        let (wx, wy) = self.walker.update(dt);
        // Ease the drawn position toward the logical one (exponential smoothing, ~50 ms), so
        // corrections and re-aimed walks glide; big jumps (warps, knockback) snap.
        let (fx, fy) = match self.smooth {
            Some((sx, sy)) if (sx - wx).abs() + (sy - wy).abs() < 3.0 => {
                let k = 1.0 - (-dt / 50.0).exp();
                (sx + (wx - sx) * k, sy + (wy - sy) * k)
            }
            _ => (wx, wy),
        };
        self.smooth = Some((fx, fy));
        let walking = self.walker.is_walking();
        let (x, z) = ((fx + 0.5) * ro_render::CELL, (fy + 0.5) * ro_render::CELL);
        let y = world.floor_at(x, z);
        self.actor.pos = glam::Vec3::new(x, y, z);
        if let Some((_, left)) = self.pose.as_mut() {
            *left -= dt;
            if *left <= 0.0 && self.pose.is_some_and(|(p, _)| p != Pose::Dead) {
                self.pose = None;
            }
        }
        if let Some(left) = self.dying.as_mut() {
            *left -= dt;
        }
        let (action, once) = match self.pose {
            Some((Pose::Dead, _)) => (self.action_for(Pose::Dead), true),
            Some((p, _)) if !walking => (self.action_for(p), true),
            _ if walking => (1, false),
            _ if self.sitting => (2, false),
            _ => (idle_action, false),
        };
        // A newly queued walk already reports is_walking() before this update. Compare
        // rendered actions to catch its start, and retain the phase across cells/turns.
        self.actor.time_ms = animation_time(self.actor.action, (action, once), self.actor.time_ms, dt, self.anim_rate);
        self.actor.action = action;
        self.actor.once = once;
        self.actor.dir = DIRS[(self.walker.dir as i32 + octant).rem_euclid(8) as usize];
        self.actor.clone()
    }
}

/// Class id -> sprite name, from the client's npcidentity + jobname tables.
fn load_class_names(vfs: &ro_vfs::Vfs) -> std::collections::HashMap<i16, String> {
    use ro_formats::lub::assignments;
    let dir = "data\\luafiles514\\lua files\\datainfo";
    let read = |name: &str| -> anyhow::Result<Vec<(ro_formats::lub::Value, ro_formats::lub::Value)>> {
        let path = format!("{dir}\\{name}.lub");
        assignments(&vfs.read(&path)?.with_context(|| format!("{path} not found"))?)
    };
    let (ids, names) = match (read("npcidentity"), read("jobname")) {
        (Ok(i), Ok(n)) => (i, n),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!("class tables: {e:#}");
            return Default::default();
        }
    };
    let ids: std::collections::HashMap<String, i16> =
        ids.iter().filter_map(|(k, v)| Some((k.as_str()?.to_string(), v.as_number()? as i16))).collect();
    let map: std::collections::HashMap<i16, String> = names
        .iter()
        .filter_map(|(k, v)| Some((*ids.get(k.as_str()?)?, v.as_str()?.to_string())))
        .collect();
    tracing::info!(classes = map.len(), "loaded class sprite names");
    map
}

/// Player jobs use the human body/head sprites; everything else has one sprite per class.
fn is_player_class(job: i16) -> bool {
    jobs::is_player(job)
}

struct App {
    backend: String,
    msaa: u32,
    /// How to (re)start the network session, and the login / character screens while they show.
    session: ro_net::SessionConfig,
    front: Option<screens::Front>,
    /// Exit requested from the login screen.
    quit: bool,
    /// Exit was asked of the map server at this time; we leave on its answer, or after a
    /// few seconds without one.
    quit_asked: Option<std::time::Instant>,
    /// A skill kept going while its shortcut key is held (CZ_USE_SKILL_START), and that key.
    held_skill: Option<(u16, winit::keyboard::Key)>,
    /// The Esc game menu (Character Select, Options, Exit).
    esc_menu: bool,
    /// Start-up splash, title art and loading screens (made once the window exists).
    splash: Option<splash::Splash>,
    vfs: Option<ro_vfs::Vfs>,
    /// Offline preview map and cell (ignored once the server places us).
    map: String,
    at: (u16, u16),
    /// `--still`: ignore camera input (for reproducible captures).
    still: bool,
    /// `--capture file.png [--size WxH] [--capture-after ms]`: render offscreen, save, exit.
    /// Online, the delay counts from map entry.
    capture: Option<(PathBuf, u32, u32)>,
    capture_after: f32,
    /// `--walk x,y`: request a walk right after entering (scripted test).
    walk_on_enter: Option<(u16, u16)>,
    /// `--say text`: send a chat line right after entering (scripted test).
    say_on_enter: Option<String>,
    /// `--distance`: initial camera distance.
    distance: Option<f32>,
    /// `--yaw degrees`: initial camera rotation (0 looks north).
    yaw: Option<f32>,
    pitch: Option<f32>,
    /// Offline preview appearance and pose (`--job`, `--sex`, `--action n`, `--dir 0..7`).
    look: Look,
    action: usize,
    dir: usize,
    net: Option<(tokio::sync::mpsc::UnboundedSender<ro_net::Command>, std::sync::mpsc::Receiver<ro_net::Event>)>,
    world: Option<World>,
    player: Option<Mob>,
    account_id: u32,
    char_id: u32,
    guild: guild::Guild,
    /// Connection problem shown in the title bar.
    status: Option<String>,
    walk_goal: Option<WalkGoal>,
    /// The NPC conversation in progress, and its illustration.
    dialog: Option<hud::Dialog>,
    cutin: Option<hud::Cutin>,
    minimap: Option<hud::Minimap>,
    settings: settings::Settings,
    audio: Option<audio::Audio>,
    /// NPC to talk to once we have walked into range.
    pending_npc: Option<u32>,
    /// A skill waiting until we have walked into range: (skill, level, target, tries).
    pending_skill: Option<(u16, u16, u32, u8)>,
    /// The monster we keep attacking, and our reach (cells) once the server tells us.
    attack: Option<(u32, i16)>,
    /// Walking toward the attack target after the server said it was out of reach.
    chasing: bool,
    /// Floating damage numbers: (world position, text, colour, age ms).
    floaters: Vec<(glam::Vec3, String, egui::Color32, f32)>,
    item_db: Option<ro_game::items::ItemDb>,
    skill_names: ro_game::skills::SkillNames,
    skill_tree: skilltree::SkillTree,
    inventory: ro_game::items::Inventory,
    /// Items on the ground, with their sprite.
    floor: std::collections::HashMap<u32, (ro_net::FloorItem, Option<ro_render::Actor>)>,
    stats: Option<ro_net::Stats>,
    skills: Vec<ro_net::Skill>,
    windows: windows::Windows,
    icons: std::collections::HashMap<u32, egui::TextureHandle>,
    icons_tried: std::collections::HashSet<u32>,
    /// Unidentified items' icons (their own look until identified), by item id.
    unid_icons: std::collections::HashMap<u32, egui::TextureHandle>,
    unid_tried: std::collections::HashSet<u32>,
    skill_icons: std::collections::HashMap<u16, egui::TextureHandle>,
    /// Floor item to pick up once we have walked next to it.
    pending_pickup: Option<u32>,
    /// A targeted skill waiting for the player to click its target.
    aiming: Option<(u16, u16)>,
    /// Shortcut bar slots (tab 0, HOTKEY_ROWS rows) and the row on show.
    hotkeys: Vec<ro_net::Hotkey>,
    hotkey_row: u8,
    /// Casts in progress: caster -> (elapsed ms, total ms).
    casts: std::collections::HashMap<u32, (f32, f32)>,
    fx: fx::FxLib,
    /// The current map's ambient effect emitters (RSW torches, bubbles, fireflies).
    map_fx: Vec<fx::MapEmitter>,
    /// Script `progressbar` over our head: (when it started, total ms).
    script_progress: Option<(std::time::Instant, f32)>,
    /// `progressbar_npc` over other units: id -> (elapsed ms, total ms).
    unit_progress: std::collections::HashMap<u32, (f32, f32)>,
    /// `showscript` call-outs over units: id -> (text, age ms).
    shouts: std::collections::HashMap<u32, (String, f32)>,
    /// Devotion links: Crusader id -> the units it protects.
    devotions: std::collections::HashMap<u32, Vec<u32>>,
    /// Ground skill units' cells (for Talkie Box messages), and Graffiti texts on them.
    ground_units: std::collections::HashMap<u32, (u16, u16)>,
    /// Traps on the ground (their models, placed), by unit id; and the trap models loaded,
    /// by path.
    traps: std::collections::HashMap<u32, Vec<ro_render::ModelDraw>>,
    trap_models: std::collections::HashMap<&'static str, Option<std::rc::Rc<(ro_formats::rsm::Rsm, Vec<Option<ro_render::ModelTexId>>)>>>,
    graffiti: std::collections::HashMap<u32, String>,
    /// `showdigit`: (type, value, ms since shown).
    digit: Option<(u8, i32, f32)>,
    /// Our PvP rank on this map: (rank, players).
    pvp_rank: Option<(u32, u32)>,
    /// Countdown to the player's next breath of bubbles on submerged maps.
    breath_ms: f32,
    /// Sprite effects playing once: (actor, ms left).
    sprite_fx: Vec<(ro_render::Actor, f32)>,
    /// When each unit last showed an area burst (one per cast, not per monster hit).
    burst_at: std::collections::HashMap<u32, std::time::Instant>,
    /// Emotes showing over units: id -> (ET_* id, ms shown).
    emotes: std::collections::HashMap<u32, (u8, f32)>,
    /// Who last whispered us, for /r.
    last_whisper_from: Option<String>,
    /// Chat lines typed this frame, sent at the start of the next.
    queued_chat: Vec<String>,
    /// Our buffs and debuffs (the icon column).
    buffs: buffs::Buffs,
    /// The quest log and NPC quest marks.
    quests: quests::QuestLog,
    /// Cart, vending and shops.
    merchant: merchant::Merchant,
    /// Buying stores and the cash shop.
    stores: stores::Stores,
    /// Achievements, titles and attendance.
    progress: progress::Progress,
    /// Pet, homunculus and mercenary.
    companions: companions::Companions,
    pet_evo: petevo::PetEvolution,
    skill_scales: skillscale::SkillScales,
    /// The skill-area texture, made on first use.
    area_tex: Option<ro_render::TexId>,
    /// RODEX mail.
    mailbox: mail::Mailbox,
    clan: clan::Clan,
    merge: merge::Merge,
    reputation: reputation::Reputation,
    game_opts: gameopts::GameOptions,
    ranking: ranking::Ranking,
    store_search: storesearch::StoreSearch,
    stylist: stylist::Stylist,
    reform: reform::Reform,
    lapine: lapine::Lapine,
    enchant: enchant::Enchant,
    market: market::Market,
    /// Test hook: a market purchase to make once a shop opens.
    market_buy: Option<(usize, u32)>,
    battle: battle::Battle,
    navi: navigation::Navigation,
    /// A walk the navigation window asked for (done outside the UI pass).
    navi_walk: Option<(u16, u16)>,
    grade: grade::Grade,
    /// Chat rooms.
    rooms: chatrooms::ChatRooms,
    /// Party booking.
    booking: booking::Booking,
    /// Adventurer's Agency, and the web server it talks to.
    agency: agency::Agency,
    web_url: String,
    /// Show the GM actions in the player menu (config `gm`).
    gm: bool,
    /// The clientinfo-style GM lists (see `Config::gm_aids`, `yellow_aids`).
    gm_aids: std::collections::HashSet<u32>,
    yellow_aids: std::collections::HashSet<u32>,
    /// The last /ex or /in sent: (name, block).
    ignore_pending: Option<(String, bool)>,
    /// A companion window request, handled after the UI pass.
    companion_action: Option<companions::CompanionAction>,
    /// When the companion AI next checks distances.
    follow_at: std::time::Instant,
    /// Homunculus / mercenary classes by unit id, for their AI.
    unit_jobs: std::collections::HashMap<u32, i16>,
    /// Who each unit last attacked (monster aggro, for the AI scripts).
    aggro: std::collections::HashMap<u32, u32>,
    /// The companion AI scripts (AI\AI.lua, AI\AI_M.lua), loaded when first needed.
    homun_ai: Option<ai::Brain>,
    merc_ai: Option<ai::Brain>,
    client_dir: Option<std::path::PathBuf>,
    /// The Classic interface art, loaded once the GRF is open.
    skin: Option<classic::Skin>,
    /// The item information / card illustration window.
    item_info: iteminfo::ItemInfo,
    refine: refine::Refine,
    /// Whether others may view our equipment (as the server last said).
    show_equip: Option<bool>,
    /// Our character drawn for the equipment window, keyed by the look it shows.
    portrait: Option<(preview::Look, Option<egui::TextureHandle>)>,
    /// Skills cooling down: id -> ready again at.
    cooldowns: std::collections::HashMap<u16, std::time::Instant>,
    /// When a paused (`wait:`) test script carries on.
    script_resume: Option<std::time::Instant>,
    pad: gamepad::Pad,
    /// The shortcut row a held controller trigger reaches (0 the row on show, 1 the next,
    /// 2 the one after), while one is held.
    pad_row: Option<u8>,
    /// Test runs: draw the controller UI in this style (and as if this row's trigger were
    /// held) without a controller (`pad:style[:row]`).
    pad_preview: Option<(padglyph::Style, Option<u8>)>,
    /// Test runs: hold-to-walk toward a fixed offset (cells) from the player, for this many
    /// ms (`holdwalk:dx:dy:ms`), as a mouse held still on screen.
    hold_walk: Option<((i32, i32), f32)>,
    /// Our walk speed as the server last sent it (SP_SPEED), kept for when we respawn.
    walk_speed: Option<f32>,
    /// Test runs: the left stick held at this position for this many ms (`stick:x:y:ms`).
    test_stick: Option<(glam::Vec2, f32)>,
    /// A Sony or Nintendo controller opened directly (touchpad, rumble, light bar), and when
    /// to look for one again.
    hid: Option<hidpad::HidPad>,
    hid_retry_ms: f32,
    /// A DualSense on USB: its haptic actuators played as audio.
    audio_haptics: Option<haptics::AudioHaptics>,
    haptics: haptics::Haptics,
    heartbeat_ms: f32,
    /// A vibration test plays for this long, controller in use or not.
    haptics_test_ms: f32,
    /// The route rumble last went by, for the log.
    haptics_route: &'static str,
    /// The DualSense HD haptics setting as last applied.
    haptics_hd: bool,
    /// The touchpad as last read, where it is steering the pointer (window pixels), the
    /// two-finger slide so far, and the mouse button a touchpad press is holding.
    touch_prev: hidpad::Touch,
    touch_pos: Option<(f64, f64)>,
    touch_slide: glam::Vec2,
    touch_button: Option<MouseButton>,
    /// Set while the touchpad's own clicks and pointer moves are fed in, which leave the
    /// controller in use; and how far the real mouse has moved while the controller was.
    touch_feeding: bool,
    mouse_travel: f64,
    /// The unit the controller acts on (highlighted), the walk direction being held and
    /// time until the walk is re-aimed.
    pad_target: Option<u32>,
    pad_dir: Option<glam::Vec2>,
    /// Where the stick's current heading was set: walks follow the line from here.
    pad_line: Option<glam::Vec2>,
    pad_walk_ms: f32,
    /// We died and wait to return to the save point (or be resurrected).
    dead: bool,
    /// `--fx name`: loop one STR effect on the player (effect previews).
    fx_test: Option<&'static str>,
    /// `--fx skill:<id>`: when the effect plays next.
    fx_test_next: Option<std::time::Instant>,
    alt: bool,
    ctrl: bool,
    /// Print Screen was pressed: the next frame is saved instead of shown.
    screenshot: bool,
    /// The Emotion List (Alt+L) and the macro window (Alt+M).
    emote_win: emotes::EmoteWindow,
    /// Shortcut Information: every key and what it does (from the Esc menu).
    shortcut_win: shortcuts::ShortcutWindow,
    queued_action: Option<windows::UiAction>,
    /// A pointer move the test script queued (UI points); egui keeps the position.
    fake_pointer: Option<egui::Pos2>,
    /// A click the test script queued there; pressed one frame, released the next.
    fake_click: Option<(egui::Pos2, bool)>,
    /// Test drags: pointer events played one frame at a time.
    fake_frames: std::collections::VecDeque<Vec<egui::Event>>,
    social: social::Social,
    /// The book reader (readbook scripts, book items).
    books: book::Books,
    memorial: memorial::Memorial,
    roulette: roulette::Roulette,
    sales: sale::Sales,
    reporter: captcha::Reporter,
    captcha: captcha::Captcha,
    /// The player we asked to trade, until they answer.
    trade_partner: Option<String>,
    /// Clicked player: (id, name, menu position in UI points).
    player_menu: Option<(u32, String, egui::Pos2)>,
    /// `--deal buy|sell` / `--sell item_id`: answer shops automatically (scripted tests).
    auto_deal: Option<bool>,
    auto_sell: Option<u32>,
    auto_buy: Option<u32>,
    /// `--accept`: accept every request; `--auto-trade item_id|0`: put one item up, lock, commit.
    auto_accept: bool,
    auto_trade: Option<u32>,
    trade_stage: u8,
    trade_offers: std::collections::HashMap<u16, i32>,
    /// Window commands sent next tick (the renderer is borrowed while the UI runs).
    queued_commands: Vec<ro_net::Command>,
    /// `--windows`: open all game windows (for captures).
    open_windows: bool,
    script: Vec<String>,
    /// Pick up the nearest floor item once one appears (scripted tests).
    auto_pickup: bool,
    /// `--talk name`: talk to the first NPC with this name once in view (scripted test).
    talk_to: Option<String>,
    /// --front-rename / --front-move: the character they act on, once the list arrives.
    front_hook: Option<u32>,
    /// --front-start: the slot to play once the PIN code (if any) is through.
    front_start: Option<u8>,
    /// Script minimap marks (viewpoint): (npc, mark id) -> (cell, colour, blinking until).
    viewpoints: std::collections::HashMap<(u32, u8), ((f32, f32), egui::Color32, Option<std::time::Instant>)>,
    /// Party and guild members elsewhere on the map: id -> (cell, guild?).
    member_pos: std::collections::HashMap<u32, ((f32, f32), bool)>,
    /// Spirit spheres per unit (ZC_SPIRITS).
    spirits: std::collections::HashMap<u32, (u16, u8)>,
    /// Our normal attack reach (ZC_ATTACK_RANGE).
    attack_range: i32,
    /// A taming item is waiting for a monster to be clicked.
    capturing: bool,
    /// ZC_MAPPROPERTY_R2 flags of the current map.
    map_flags: u32,
    /// Hat effect files by id (hateffectinfo), and the ones playing: (unit, effect) -> fx tag.
    hat_files: std::collections::HashMap<u16, &'static str>,
    hat_fx: std::collections::HashMap<(u32, u16), u32>,
    /// Units wearing a custom code-drawn aura (custom hat effect ids, see `custom_hat_aura`).
    hat_auras: std::collections::HashMap<u32, ro_render::AuraStyle>,
    /// Guild emblems by guild id (None while being fetched).
    emblems: std::collections::HashMap<u32, Option<egui::TextureHandle>>,
    /// The emblem version last asked for, by guild id.
    emblem_versions: std::collections::HashMap<u32, u32>,
    /// Emblem uploads and downloads on the web server.
    emblem_web: emblem::Web,
    /// Test hook (--npc-answers): replies to NPC dialog steps, in order.
    npc_answers: std::collections::VecDeque<String>,
    /// `--attack`: attack the nearest monster once one is in view (scripted test).
    auto_attack: bool,
    /// Other units in view, by id.
    units: std::collections::HashMap<u32, Mob>,
    /// Warp portals in view (drawn as an effect, not a sprite), by unit id.
    warps: std::collections::HashMap<u32, (u16, u16)>,
    class_names: std::collections::HashMap<i16, String>,
    pet_talk: pettalk::PetTalk,
    sense: sense::Sense,
    look_tables: ro_game::looks::LookTables,
    /// Loaded actor templates by sprite key (None: not available, don't retry).
    sprites: std::collections::HashMap<String, Option<ro_render::Actor>>,
    /// Classes drawn as 3D models (the original's Granny monsters), loaded once each.
    models3d: std::collections::HashMap<i16, Option<std::rc::Rc<models3d::Loaded>>>,
    /// When the current map became ready (for --capture-after).
    ready_at: Option<std::time::Instant>,
    last_tick: std::time::Instant,
    window: Option<Arc<Window>>,
    renderer: Option<ro_render::Renderer>,
    /// In the background (Android): its window surface is gone, so nothing draws until resumed.
    paused: bool,
    /// Right mouse button held: dragging rotates the camera.
    rotating: bool,
    /// Fingers on a touch screen working the world (not the UI), by touch id, oldest first.
    fingers: Vec<(u64, (f64, f64))>,
    /// How far the cursor moved while the right button was held: a click (barely moved)
    /// opens a player's menu, a drag only turns the camera.
    right_drag: f64,
    cursor_sprite: Option<hud::CursorSprite>,
    /// Which cursor to show (hud::cursor), from what is under the mouse.
    cursor_action: usize,
    /// Left button held after clicking the ground: keep walking toward the cursor.
    mouse_walk: bool,
    mouse_walk_ms: f32,
    /// Left mouse button held (from window events).
    left_down: bool,
    cursor: Option<(f64, f64)>,
    fps: FpsCounter,
    /// The performance overlay's measurements (F10), and the map server's last round trip.
    perf: perf::Perf,
    /// The interface layout editor (Settings > Game > Interface > Arrange).
    arrange: hudlayout::Editor,
    ping: Option<f32>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    hud: hud::Hud,
    /// UI size multiplier on top of the display's DPI scale (`ui_scale` in the config).
    ui_scale: f32,
}

impl App {
    /// Opens the roulette window (the server answers with points and the prize table).
    fn open_roulette(&mut self) {
        self.send(ro_net::Command::Roulette(ro_net::RouletteRequest::Open));
        self.send(ro_net::Command::Roulette(ro_net::RouletteRequest::Info));
    }

    /// Opens book `id` (its item id) at `page`, titled with the item's name.
    fn open_book(&mut self, id: u32, page: u32) {
        let title = self.item_db.as_ref().map_or_else(|| format!("Book {id}"), |db| db.name(id, true));
        if let Some(vfs) = self.vfs.as_ref() {
            if !self.books.open(vfs, id, page, title) {
                self.hud.push_chat(ro_net::ChatKind::System, "This book cannot be read.".into());
            }
        }
    }

    /// Loads a map into the renderer and remembers its ground for picking.
    fn enter_map(&mut self, name: &str) {
        // A loading screen covers the map load (not behind the login screens or in captures).
        if self.front.is_none() && (self.capture.is_none() || std::env::args().any(|a| a == "--front-start")) {
            if let Some(s) = self.splash.as_mut() {
                s.begin_loading(&self.egui_ctx, self.vfs.as_ref());
            }
        }
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        // The server's name for the map stays the world's; its files may be another map's.
        let server_name = name;
        let name = map_files(name);
        match load_map(vfs, name, &ro_render::Upscaler::new(self.settings.upscale.clone())) {
            Ok(map) => {
                r.set_scene(&map.mesh, map.images, map.lighting);
                let submerged = map.effects.iter().filter(|e| e.0 == 109).count() >= 40;
                let sky = sky::for_map(vfs, name, &map.lighting, submerged);
                let outdoors = sky.is_some();
                tracing::info!(map = name, sky = sky.as_ref().map(|s| s.name), "map sky");
                r.set_sky_profile(sky);
                tracing::info!(count = map.lights.len(), first = ?map.lights.first(), "map light sources");
                r.point_lights = map.lights;
                if let Some((mesh, images)) = map.water {
                    r.set_water(&mesh, images);
                }
                self.unit_progress.clear();
                self.shouts.clear();
                self.digit = None;
                self.pvp_rank = None;
                // Ambient map effects play from the frame loop.
                // EF_EMITTER objects are particle systems defined by the map's effecttool lub.
                let emitter_spots: Vec<glam::Vec3> = map.effects.iter().filter(|e| e.0 == 974).map(|e| e.1).collect();
                let mut emitters = fx::map_emitters(&mut self.fx, r, vfs, name, &emitter_spots, false);
                let animated_spots: Vec<glam::Vec3> = map.effects.iter().filter(|e| e.0 == 1073).map(|e| e.1).collect();
                emitters.extend(fx::map_emitters(&mut self.fx, r, vfs, name, &animated_spots, true));
                let spots: Vec<(i32, glam::Vec3)> = map.effects.iter().map(|e| (e.0, e.1)).collect();
                let placed: Vec<(i32, glam::Vec3, [f32; 4])> = map.effects.iter().map(|e| (e.0, e.1, e.3)).collect();
                emitters.extend(fx::map_torches(&mut self.fx, r, vfs, &spots));
                emitters.extend(fx::map_smoke(&mut self.fx, r, vfs, &spots));
                tracing::info!(objects = emitter_spots.len(), emitters = emitters.len(), "map particle emitters");
                let wind_spots: Vec<glam::Vec3> = map.effects.iter().filter(|e| e.0 == 2343).map(|e| e.1).collect();
                let winds = fx::map_winds(&mut self.fx, r, vfs, name, &wind_spots);
                tracing::info!(objects = wind_spots.len(), winds = winds.len(), "map wind effects");
                let waterfalls = fx::map_waterfalls(&mut self.fx, r, vfs, &placed);
                emitters.extend(fx::map_spray(&mut self.fx, r, vfs, &placed));
                let glows = fx::map_glows(&mut self.fx, r, vfs, &placed);
                tracing::info!(glows = glows.len(), "map glows");
                r.set_glows(glows);
                r.set_emitters(emitters, winds, waterfalls);
                self.map_fx = map.effects.into_iter().enumerate().map(|(i, (id, pos, delay, _))| fx::MapEmitter::new(id, pos, delay, i)).collect();
                tracing::info!(emitters = self.map_fx.len(), drawn = self.map_fx.iter().filter(|e| e.visual.is_some()).count(), first = ?self.map_fx.first().map(|e| (e.pos, e.every_ms)), "map effects");
                let gat = vfs
                    .read(&format!("data\\{name}.gat"))
                    .ok()
                    .flatten()
                    .and_then(|d| ro_formats::gat::Gat::parse(&d).inspect_err(|e| tracing::warn!("{name}.gat: {e:#}")).ok());
                let path = format!("data\\texture\\유저인터페이스\\map\\{name}.bmp");
                self.minimap = match (load_texture(&self.egui_ctx, vfs, &path), gat.as_ref()) {
                    (Some(texture), Some(g)) => Some(hud::Minimap { texture, cells: (g.width as f32, g.height as f32) }),
                    _ => None,
                };
                if let Some(a) = self.audio.as_mut() {
                    a.enter_map(vfs, name);
                }
                r.set_room_mask(None);
                self.world = Some(World { name: server_name.to_string(), outdoors, ground: map.mesh, gat, room: None, submerged });
                self.ready_at = Some(std::time::Instant::now());
            }
            Err(e) => tracing::error!("map {name}: {e:#}"),
        }
    }

    /// Actor template for a player look, cached so sheets upload once per look.
    fn player_actor(&mut self, look: &Look) -> Option<ro_render::Actor> {
        let key = format!("player:{}:{}:{}:{}:{:?}", look.job, look.male, look.hair, look.hair_color, look.gear);
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        // Weapons arrive as item ids; their sprite class is the item's ClassNum (iteminfo).
        let mut resolved = look.clone();
        if let Some(info) = self.item_db.as_ref().and_then(|d| d.get(look.gear.weapon)) {
            resolved.gear.weapon = info.view;
            tracing::debug!(item = look.gear.weapon, view = info.view, name = ?self.look_tables.weapon(info.view), "weapon view");
        }
        let actor = load_character(vfs, r, &resolved, &self.look_tables).inspect_err(|e| tracing::warn!("player sprites {key}: {e:#}")).ok();
        self.sprites.insert(key, actor.clone());
        actor
    }

    /// Actor template for an NPC / monster class: data\sprite\npc\ or data\sprite\몬스터\.
    fn class_actor(&mut self, job: i16) -> Option<ro_render::Actor> {
        // Newer NPC classes (10000+) this client has no name or sprite for still need a
        // body to be clicked (e.g. 10243, the Zonda teleporters): a stock townsman stands in.
        // Twin Flames Saga monsters: a stock sprite, recoloured and resized.
        let saga = saga::look(job);
        let name = match self.class_names.get(&job) {
            _ if saga.is_some() => saga.map(|l| l.base.to_string()).unwrap_or_default(),
            Some(n) => n.clone(),
            None if (10000..20000).contains(&(job as i32)) => {
                tracing::debug!(job, "unknown NPC class, using a stand-in sprite");
                "4_M_01".to_string()
            }
            None => return None,
        };
        let key = if saga.is_some() { format!("class#{job}") } else { format!("class:{name}") };
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        // Classes the original draws as 3D models: the model replaces the body sprite.
        if models3d::is_model(&name) {
            let loaded = models3d::load(vfs, r, &name).inspect_err(|e| tracing::warn!(job, "3D model {name}: {e:#}")).ok().map(std::rc::Rc::new);
            let actor = loaded.is_some().then(|| {
                let shadow = self.sprites.get("shadow").cloned().flatten().and_then(|a| a.shadow);
                ro_render::Actor { pos: Default::default(), body: ro_render::SheetId::NONE, head: None, headgear: vec![], weapon: vec![], shadow, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false, alpha: 1.0, scale: 1.0, tint: [1.0; 3] }
            });
            self.models3d.insert(job, loaded);
            self.sprites.insert(key, actor.clone());
            return actor;
        }
        let read = |base: &str| -> Option<(ro_formats::spr::Spr, ro_formats::act::Act)> {
            let spr = vfs.read(&format!("{base}.spr")).ok().flatten()?;
            let act = vfs.read(&format!("{base}.act")).ok().flatten()?;
            Some((ro_formats::spr::Spr::parse(&spr).ok()?, ro_formats::act::Act::parse(&act).ok()?))
        };
        // Homunculi are in homun\; mercenaries name a human body (e.g. 여\활용병) and carry
        // a weapon from 인간족\용병\.
        // Monster classes look in 몬스터 first: some names (poring) are also NPC sprites.
        let folders: &[&str] = if (1000..4000).contains(&(job as i32)) || job as i32 >= 20000 || job < 0 { &["몬스터", "npc", "homun", "인간족\\몸통"] } else { &["npc", "몬스터", "homun", "인간족\\몸통"] };
        let found = folders.iter().find_map(|folder| read(&format!("data\\sprite\\{folder}\\{name}"))).or_else(|| {
            // Its sprite is missing from this client's data (e.g. 10243 4_m_drzonda01).
            (10000..20000).contains(&(job as i32)).then(|| tracing::debug!(job, %name, "missing NPC sprite, using a stand-in"))?;
            read("data\\sprite\\npc\\4_m_01")
        });
        let merc_weapon = name.rsplit('\\').next().and_then(|n| ["활", "창", "검"].iter().find_map(|w| read(&format!("data\\sprite\\인간족\\용병\\{n}_{w}"))));
        // Mercenary bodies are headless; the original adds a player head, its style set by
        // the mercenary's grade (classes 6017.. come in tens: archers, lancers, swordsmen).
        let merc_head = merc_weapon.as_ref().and(name.split_once('\\')).and_then(|(sex, _)| read(&format!("data\\sprite\\인간족\\머리통\\{sex}\\{}_{sex}", (job - 6017).rem_euclid(10) + 1)));
        let actor = match found {
            Some((spr, act)) => {
                let palette = saga.filter(|l| l.recoloured()).map(|l| l.recolour(&spr.palette));
                let body = r.add_sheet(&spr, act, palette.as_ref());
                let weapon = merc_weapon.map(|(s, a)| vec![r.add_sheet(&s, a, None)]).unwrap_or_default();
                let head = merc_head.map(|(s, a)| r.add_sheet(&s, a, None));
                let shadow = self.sprites.get("shadow").cloned().flatten().and_then(|a| a.shadow);
                let (scale, tint) = saga.map_or((1.0, [1.0; 3]), |l| (l.scale, l.tint));
                Some(ro_render::Actor { pos: Default::default(), body, head, headgear: vec![], weapon, shadow, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false, alpha: 1.0, scale, tint })
            }
            None => {
                tracing::debug!(job, %name, "no sprite for class");
                None
            }
        };
        self.sprites.insert(key, actor.clone());
        actor
    }

    /// Creates (or re-creates) the local player at a cell.
    fn spawn_player(&mut self, look: &Look, cell: (u16, u16), dir: usize, speed: i16) {
        if let Some(actor) = self.player_actor(look) {
            // NPC/monster actors borrow the player's shadow sheet.
            self.sprites.insert("shadow".into(), Some(actor.clone()));
            let mut mob = Mob::new(actor, cell, dir, speed);
            if let Some(s) = self.walk_speed {
                mob.walker.speed = s;
            }
            mob.name = self.hud.status.name.clone();
            mob.look = Some(look.clone());
            self.player = Some(mob);
        }
    }

    fn market_test_buy(&mut self) {
        tracing::info!(pending = ?self.market_buy, "market test buy");
        if let Some(r) = self.market_buy.take().and_then(|(row, n)| self.market.test_buy(row, n)) {
            self.send(ro_net::Command::Market(r));
        }
    }

    /// Redraws the equipment window's portrait when our look has changed.
    fn refresh_portrait(&mut self) {
        let Some(look) = self.player.as_ref().and_then(|p| p.look.as_ref()) else { return };
        let mut headgear = vec![];
        for v in [look.gear.head_bottom, look.gear.head_mid, look.gear.head_top] {
            if let Some(n) = (v != 0).then(|| self.look_tables.accessory(v as u32).map(str::to_string)).flatten() {
                if !headgear.contains(&n) {
                    headgear.push(n);
                }
            }
        }
        let want = preview::Look { job: look.job.clone(), male: look.male, hair: look.hair.max(1), hair_color: look.hair_color, headgear };
        if self.portrait.as_ref().is_some_and(|p| p.0 == want) {
            return;
        }
        let tex = self.vfs.as_ref().and_then(|v| preview::compose(v, &want)).map(|img| self.egui_ctx.load_texture("portrait", img, egui::TextureOptions::NEAREST));
        self.portrait = Some((want, tex));
    }

    /// Applies a ZC_SPRITE_CHANGE to a player-sprite unit (us or another player).
    /// Whether a character is drawn with the GM sprite: its GM's @gmlook choice (name font 200
    /// on, 201 off), else the clientinfo-style `gm_aids` list.
    fn gm_sprite(&self, aid: u32, font: u16) -> bool {
        match font {
            GM_LOOK_ON => true,
            GM_LOOK_OFF => false,
            _ => self.gm_aids.contains(&aid),
        }
    }

    /// Whether a character's name shows in yellow: with the GM look, else the `yellow_aids` list.
    fn yellow_name(&self, aid: u32, font: u16) -> bool {
        match font {
            GM_LOOK_ON => true,
            GM_LOOK_OFF => false,
            _ => self.yellow_aids.contains(&aid),
        }
    }

    /// A character's name font changed (ZC_NOTIFY_FONT): redraw it with or without the GM look.
    fn font_change(&mut self, id: u32, font: u16) {
        let (gm, yellow) = (self.gm_sprite(id, font), self.yellow_name(id, font));
        tracing::debug!(id, font, gm, yellow, "name font");
        let class = if id == self.account_id { self.hud.status.job_id } else { self.units.get(&id).map_or(0, |m| m.class) };
        let Some(mut look) = self.mob_mut(id).map(|m| {
            (m.font, m.yellow) = (font, yellow);
            m.look.clone()
        }).flatten() else { return };
        look.job = if gm { jobs::GM_SPRITE.to_string() } else { job_sprite(class).to_string() };
        let Some(fresh) = self.player_actor(&look) else { return };
        if let Some(m) = self.mob_mut(id) {
            (m.actor.body, m.actor.head, m.actor.headgear, m.actor.weapon) = (fresh.body, fresh.head, fresh.headgear, fresh.weapon);
            m.look = Some(look);
        }
    }

    fn look_change(&mut self, id: u32, kind: u8, value: u32, value2: u32) {
        let Some(mut look) = self.mob_mut(id).and_then(|m| m.look.clone()) else { return };
        match kind {
            0 => {
                let font = self.mob_mut(id).map_or(0, |m| m.font);
                look.job = if self.gm_sprite(id, font) { jobs::GM_SPRITE.to_string() } else { job_sprite(value as i16).to_string() };
                if id == self.account_id {
                    self.hud.status.job = job_title(value as i16).to_string();
                    self.hud.status.job_id = value as i16;
                    self.load_skill_icons();
                }
            }
            1 => look.hair = value.max(1) as u16,
            2 => (look.gear.weapon, look.gear.shield) = (value, value2),
            3 => look.gear.head_bottom = value as u16,
            4 => look.gear.head_top = value as u16,
            5 => look.gear.head_mid = value as u16,
            6 => look.hair_color = value as u16,
            _ => return,
        }
        let Some(fresh) = self.player_actor(&look) else { return };
        if let Some(m) = self.mob_mut(id) {
            // Keep position, pose and timing; swap the layers.
            (m.actor.body, m.actor.head, m.actor.headgear, m.actor.weapon) = (fresh.body, fresh.head, fresh.headgear, fresh.weapon);
            m.look = Some(look);
        }
    }

    /// A unit came into view or started walking.
    fn unit(&mut self, info: ro_net::UnitInfo) {
        if info.id == self.account_id {
            return;
        }
        if (6001..=6099).contains(&info.job) {
            self.unit_jobs.insert(info.id, info.job);
        }
        let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
        if let Some(m) = self.units.get_mut(&info.id) {
            m.level = info.level;
            match info.pos {
                ro_net::UnitPos::Walking { from, to } => m.walk(gat, from, to),
                ro_net::UnitPos::At { x, y, dir } => {
                    m.walker.stop_at((x as i32, y as i32));
                    m.walker.dir = dir as usize;
                }
            }
            return;
        }
        // Warps are an effect; hidden NPCs are not drawn at all.
        match info.job {
            45 => {
                if let ro_net::UnitPos::At { x, y, .. } = info.pos {
                    self.warps.insert(info.id, (x, y));
                }
                return;
            }
            111 | 139 | 32767 => return,
            _ => {}
        }
        let gm = self.gm_sprite(info.id, info.font);
        let look = is_player_class(info.job).then(|| Look {
            job: if gm { jobs::GM_SPRITE.to_string() } else { job_sprite(info.job).to_string() },
            male: info.sex == 1,
            hair: info.hair.max(1),
            hair_color: info.hair_color.max(0) as u16,
            gear: info.look,
        });
        let actor = match &look {
            Some(l) => self.player_actor(l),
            None => self.class_actor(info.job),
        };
        let Some(actor) = actor else { return };
        let (cell, dir) = match info.pos {
            ro_net::UnitPos::At { x, y, dir } => ((x, y), dir as usize),
            ro_net::UnitPos::Walking { from, .. } => (from, 0),
        };
        let mut mob = Mob::new(actor, cell, dir, info.speed);
        // NPC names may carry a "#unique-suffix" that the client never shows.
        mob.name = info.name.split('#').next().unwrap_or_default().trim().to_string();
        mob.kind = info.object_type;
        mob.class = info.job;
        mob.level = info.level;
        mob.actor.alpha = option_alpha(info.option, false);
        mob.human = is_player_class(info.job);
        mob.look = look;
        mob.font = info.font;
        mob.yellow = self.yellow_name(info.id, info.font);
        if let ro_net::UnitPos::Walking { from, to } = info.pos {
            let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
            mob.walk(gat, from, to);
        }
        // Players: ask for their party and guild names, shown under the name.
        if info.object_type == 0 {
            self.send(ro_net::Command::ReqName(info.id));
        }
        mob.guild = info.guild;
        self.want_emblem(info.guild, info.emblem as u32);
        tracing::debug!(id = info.id, job = info.job, name = %info.name, ?cell, "unit");
        self.units.insert(info.id, mob);
    }

    fn lapine_action(&mut self, a: lapine::LapineAction) {
        self.queued_commands.push(lapine_command(a));
    }

    /// Asks to walk to a cell: through the server online, locally offline.
    fn request_walk(&mut self, cell: (u16, u16)) {
        if self.net.is_none() {
            let (Some(p), Some(w)) = (self.player.as_mut(), self.world.as_ref()) else { return };
            if let Some(gat) = &w.gat {
                if let Some(path) = ro_game::path::find_long(gat, p.walker.cell, (cell.0 as i32, cell.1 as i32), LONG_WALK_NODES) {
                    p.walker.walk(path);
                }
            }
            return;
        }
        self.walk_goal = Some(WalkGoal { cell, sent_at: None, tries: 0 });
        self.next_hop();
    }

    /// Online walks longer than the server's MAX_WALKPATH go as a chain of hops along the
    /// full client-side path; the next hop is sent when the previous one finishes.
    fn next_hop(&mut self) {
        let (Some(goal), Some(p), Some(w), Some((tx, _))) = (self.walk_goal.as_mut(), self.player.as_ref(), self.world.as_ref(), self.net.as_ref())
        else {
            return;
        };
        let target = (goal.cell.0 as i32, goal.cell.1 as i32);
        if p.walker.cell == target || goal.tries >= 5 {
            self.walk_goal = None; // arrived, or the server keeps refusing
            return;
        }
        let path = w.gat.as_ref().and_then(|g| ro_game::path::find_long(g, p.walker.cell, target, LONG_WALK_NODES));
        let Some(path) = path.filter(|p| !p.is_empty()) else {
            tracing::debug!(from = ?p.walker.cell, ?target, "no path to walk target");
            self.walk_goal = None;
            return;
        };
        // A refused hop is retried shorter: rAthena's A* uses a small fixed node table and
        // fails on detours (around fountains, walls) even under max_walk_path.
        let hop_len = (WALK_HOP >> goal.tries.min(2)).max(1);
        let hop = path[path.len().min(hop_len) - 1];
        tracing::debug!(from = ?p.walker.cell, ?target, steps = path.len(), ?hop, "walk hop");
        let _ = tx.send(ro_net::Command::Move { x: hop.0 as u16, y: hop.1 as u16 });
        goal.sent_at = Some(std::time::Instant::now());
        goal.tries += 1;
    }

    fn handle(&mut self, event: ro_net::Event) {
        use ro_net::Event::*;
        // Login and character-select replies belong to those screens.
        if let Some(front) = self.front.as_mut() {
            if front.event(&event) {
                if let (Characters { chars, .. }, true) = (&event, std::env::args().any(|a| a == "--front-start")) {
                    if let Some(c) = chars.first() {
                        self.front_start = Some(c.slot);
                    }
                }
                // --front-pin 1234 answers PIN prompts (entering, making or renewing it).
                let pin_arg = std::env::args().skip_while(|a| a != "--front-pin").nth(1);
                if let (Pin(state @ 1..=4), Some(pin)) = (&event, pin_arg) {
                    let req = match state {
                        1 => ro_net::PinRequest::Check(pin),
                        3 => ro_net::PinRequest::Change { old: pin.clone(), new: pin },
                        _ => ro_net::PinRequest::Make(pin),
                    };
                    self.queued_commands.push(ro_net::Command::Pin(req));
                }
                // --front-start waits for the PIN code to be accepted.
                if !front.pin_pending() {
                    if let Some(slot) = self.front_start.take() {
                        self.queued_commands.push(ro_net::Command::SelectChar(slot));
                    }
                }
                if let (Characters { chars, .. }, true) = (&event, std::env::args().any(|a| a == "--front-make")) {
                    if !chars.iter().any(|c| c.name == "Preview") {
                        let slot = (0..9u8).find(|s| !chars.iter().any(|c| c.slot == *s)).unwrap_or(1);
                        self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::Make { name: "Preview".into(), slot, hair_color: 4, hair_style: 9, job: 0, sex: 0 }));
                    }
                }
                if matches!(event, Characters { .. }) && std::env::args().any(|a| a == "--front-create") {
                    front.open_create();
                }
                // --front-rename NAME / --front-move SLOT: rename or move the last character once.
                let args: Vec<String> = std::env::args().collect();
                let after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1).cloned());
                match &event {
                    Characters { chars, .. } if self.front_hook.is_none() => {
                        if let Some(c) = chars.iter().max_by_key(|c| c.slot) {
                            self.front_hook = Some(c.char_id);
                            if let Some(name) = after("--front-rename") {
                                self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::CheckName { account_id: 0, char_id: c.char_id, name }));
                            }
                            if let Some(to) = after("--front-move").and_then(|s| s.parse().ok()) {
                                self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::MoveSlot { from: c.slot as u16, to }));
                            }
                        }
                    }
                    CharNameOk(true) => {
                        if let (Some(char_id), Some(name)) = (self.front_hook, after("--front-rename")) {
                            self.queued_commands.push(ro_net::Command::Char(ro_net::CharRequest::Rename { char_id, name }));
                        }
                    }
                    _ => {}
                }
                return;
            }
            match &event {
                Entered { .. } => {
                    // Remember the ID for next time (Remember ID).
                    self.settings.last_id = if front.remember { front.user().to_string() } else { String::new() };
                    self.settings.save();
                    self.front = None;
                }
                Disconnected(reason) => {
                    // Back to a fresh login screen with the reason.
                    let user = front.user().to_string();
                    let mut f = screens::Front::new(user, front.remember);
                    f.event(&LoginFailed(format!("Disconnected: {reason}")));
                    self.front = Some(f);
                    if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                        a.title(v);
                    }
                    self.net = Some(ro_net::spawn_session(self.session.clone()));
                    return;
                }
                _ => {}
            }
        }
        match event {
            Entered { map, x, y, dir, character, account_id, font } => {
                tracing::info!(%map, x, y, name = %character.name, job = character.job, "entered");
                self.account_id = account_id;
                self.char_id = character.char_id;
                self.units.clear();
                self.devotions.clear();
                self.ground_units.clear();
                self.traps.clear();
                self.graffiti.clear();
                self.floor.clear();
                self.warps.clear();
                self.enter_map(&map);
                let look = Look {
                    job: if self.gm_sprite(account_id, font) { jobs::GM_SPRITE.to_string() } else { job_sprite(character.job).to_string() },
                    male: character.sex == 1,
                    hair: character.head.max(1) as u16,
                    hair_color: character.hair_color as u16,
                    gear: character.look,
                };
                let s = &mut self.hud.status;
                s.name = character.name.clone();
                s.job = job_title(character.job).to_string();
                s.job_id = character.job;
                s.base_level = character.base_level as i64;
                s.job_level = character.job_level as i64;
                (s.hp, s.max_hp, s.sp, s.max_sp) = (character.hp, character.max_hp, character.sp, character.max_sp);
                s.base_exp = character.base_exp;
                s.zeny = character.zeny as i64;
                self.spawn_player(&look, (x, y), dir as usize, character.speed);
                let yellow = self.yellow_name(account_id, font);
                if let Some(p) = self.player.as_mut() {
                    (p.font, p.yellow) = (font, yellow);
                }
                if let Some(text) = self.say_on_enter.take() {
                    // Several lines separated by "|" (e.g. a batch of GM commands).
                    for line in text.split('|') {
                        self.queued_chat.push(line.trim().to_string());
                    }
                }
                if let Some(cell) = self.walk_on_enter.take() {
                    self.request_walk(cell);
                }
            }
            Walk { from, to } => {
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                if let Some(p) = self.player.as_mut() {
                    tracing::debug!(?from, ?to, cell = ?p.walker.cell, next = ?p.walker.next(), "walk");
                    if let Some(g) = self.walk_goal.as_mut() {
                        g.tries = 0; // the server accepted this hop
                    }
                    p.walk(gat, from, to);
                }
            }
            Stop { id, x, y } => {
                let mob = if id == self.account_id { self.player.as_mut() } else { self.units.get_mut(&id) };
                if let Some(m) = mob {
                    m.walker.stop_at((x as i32, y as i32));
                }
            }
            Unit(info) => self.unit(info),
            Stat { id, value } => {
                self.hud.status.set(id, value);
                // SP_SPEED: our walk speed (ms per cell) as the server moves us. Walking at any
                // other pace runs ahead of (or behind) the server, and every re-aimed walk
                // then pulls us back to where it has us.
                if id == 0 && value > 0 {
                    tracing::debug!(speed = value, "walk speed");
                    self.walk_speed = Some(value as f32);
                    if let Some(p) = self.player.as_mut() {
                        p.walker.speed = value as f32;
                    }
                }
                // Keep the Status window live: points and the derived values (SP_ATK1..SP_ASPD).
                if let Some(s) = self.stats.as_mut() {
                    let v = value.clamp(i16::MIN as i64, i16::MAX as i64) as i16;
                    match id {
                        9 => s.points = value.max(0) as u32,
                        41 => s.atk.0 = v,
                        42 => s.atk.1 = v,
                        43 => s.matk.1 = v,
                        44 => s.matk.0 = v,
                        45 => s.def.0 = v,
                        46 => s.def.1 = v,
                        47 => s.mdef.0 = v,
                        48 => s.mdef.1 = v,
                        49 => s.hit = v,
                        50 => s.flee.0 = v,
                        51 => s.flee.1 = v,
                        52 => s.crit = v,
                        53 => s.aspd = (v, 0),
                        225..=230 => s.advanced[(id - 225) as usize] = v,
                        231 => s.trait_points = value.max(0) as u32,
                        _ => {}
                    }
                }
            }
            Chat { kind, text, .. } => {
                if let (ro_net::ChatKind::Whisper, Some(name)) = (kind, text.strip_prefix("(From ").and_then(|t| t.split_once(") : ")).map(|(n, _)| n.to_string())) {
                    self.last_whisper_from = Some(name);
                    self.play_sound("_chat_whisper.wav");
                }
                self.hud.push_chat(kind, text);
            }
            Menu(menu) => {
                // Icons for items that may not be in the inventory (poisons, arrows to make).
                let ids: Vec<u32> = match &menu {
                    ro_net::Menu::Items(ids) | ro_net::Menu::Make { items: ids, .. } => ids.clone(),
                    ro_net::Menu::Produce(list) => list.iter().flat_map(|(id, m)| std::iter::once(*id).chain(m.iter().copied())).collect(),
                    _ => vec![],
                };
                self.load_icons_for(&ids);
                self.windows.menu = Some(menu);
            }
            MakeResult { ok, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("item {item}"), |d| d.name(item, true));
                let msg = if ok { format!("You made {name}.") } else { format!("Making {name} failed.") };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            Quests(list) => self.quests.quests = list.into_iter().map(|q| (q.id, q)).collect(),
            QuestAdded(q) => {
                self.hud.push_chat(ro_net::ChatKind::System, format!("Quest accepted: {}", self.quests.title(q.id)));
                self.quests.quests.insert(q.id, q);
            }
            QuestRemoved(id) => {
                self.quests.quests.remove(&id);
            }
            QuestProgress(updates) => self.quests.progress(&updates),
            QuestActiveChanged { quest, active } => {
                if let Some(q) = self.quests.quests.get_mut(&quest) {
                    q.active = active;
                }
            }
            QuestMark { npc, mark, color } => {
                if mark == 9999 {
                    self.quests.marks.remove(&npc);
                } else {
                    self.quests.marks.insert(npc, (mark, color));
                }
            }
            InventoryExtra(n) => self.windows.inv_extra = n,
            ItemFavorite { index, favorite } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.favorite = favorite;
                }
            }
            ExpandAsk { result, item } => match result {
                0 => self.windows.expand_ask = Some(item),
                _ => self.hud.push_chat(ro_net::ChatKind::System, match result {
                    2 => "You can't expand your inventory right now.",
                    3 => "You need an inventory expansion item.",
                    4 => "Your inventory can't be expanded any further.",
                    _ => "The inventory can't be expanded.",
                }.into()),
            },
            CartOff => {
                self.merchant.cart.clear();
                self.merchant.cart_count = Default::default();
                self.merchant.cart_open = false;
            }
            PartyPickup { aid, item, refine } => {
                let who = self.units.get(&aid).map(|m| m.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "A party member".into());
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let refine = if refine > 0 { format!("+{refine} ") } else { String::new() };
                self.hud.push_chat(ro_net::ChatKind::Party, format!("{who} got {refine}{name}."));
            }
            CartStart => {
                tracing::debug!("cart list");
                self.merchant.cart.clear();
            }
            CartItems(items) => {
                for d in items {
                    let it = inv_item(d);
                    self.merchant.cart.insert(it.index, it);
                }
                let ids = self.merchant.item_ids();
                self.load_icons_for(&ids);
            }
            CartCount { items, max_items, weight, max_weight } => {
                tracing::debug!(items, max_items, weight, max_weight, "cart");
                self.merchant.cart_count = (items, max_items, weight, max_weight);
            }
            CartAdded(d) => {
                let it = inv_item(d);
                self.load_icons_for(&[it.id]);
                match self.merchant.cart.get_mut(&it.index) {
                    Some(e) if e.id == it.id => e.amount += it.amount,
                    _ => {
                        self.merchant.cart.insert(it.index, it);
                    }
                }
            }
            CartRemoved { index, amount } => {
                if let Some(e) = self.merchant.cart.get_mut(&index) {
                    e.amount = e.amount.saturating_sub(amount as u16);
                    if e.amount == 0 {
                        self.merchant.cart.remove(&index);
                    }
                }
            }
            ShopSign { aid, title } => {
                if title.is_empty() {
                    self.merchant.signs.remove(&aid);
                } else {
                    self.merchant.signs.insert(aid, title);
                }
            }
            Shop { seller, shop, items } => {
                let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
                self.load_icons_for(&ids);
                let amounts = vec![0; items.len()];
                self.merchant.browsing = Some((seller, shop, items, amounts));
            }
            ShopSetup(slots) => {
                self.merchant.setup = Some((slots, format!("{}'s shop", self.hud.status.name), Default::default()));
            }
            MyShop(stock) => self.merchant.my_shop = Some(stock),
            Sold { index, amount, zeny } => {
                let what = self.merchant.my_shop.as_ref().and_then(|s| s.iter().find(|i| i.index == index)).map(|i| i.id);
                let name = what.and_then(|id| self.item_db.as_ref().map(|d| d.name(id, true))).unwrap_or_else(|| "an item".into());
                self.hud.push_chat(ro_net::ChatKind::System, format!("Sold {amount} x {name} for {zeny} z."));
                if let Some(stock) = self.merchant.my_shop.as_mut() {
                    if let Some(i) = stock.iter_mut().find(|i| i.index == index) {
                        i.amount = i.amount.saturating_sub(amount);
                    }
                    stock.retain(|i| i.amount > 0);
                }
            }
            BuySign { aid, title } => {
                if title.is_empty() {
                    self.stores.signs.remove(&aid);
                } else {
                    self.stores.signs.insert(aid, title);
                }
            }
            BuyStoreSetup(slots) => {
                let limit = self.hud.status.zeny.max(0).to_string();
                self.stores.setup = Some((slots, format!("{} is buying", self.hud.status.name), limit, Default::default()));
            }
            MyBuyStore { limit, items } => {
                self.stores.mine = Some((limit, items));
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            BuyStore { buyer, store, limit, items } => {
                let amounts = vec![0; items.len()];
                self.stores.browsing = Some((buyer, store, limit, items, amounts));
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            BoughtThroughStore { id, amount, zeny, limit } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
                self.hud.push_chat(ro_net::ChatKind::System, format!("Your buying store bought {amount} x {name} for {zeny} z."));
                if let Some((l, items)) = self.stores.mine.as_mut() {
                    *l = limit;
                    if let Some(i) = items.iter_mut().find(|i| i.id == id) {
                        i.amount = i.amount.saturating_sub(amount);
                    }
                }
            }
            SoldToStore { index, amount, price } => {
                let name = self.inventory.items.get(&index).and_then(|i| self.item_db.as_ref().map(|d| d.name(i.id, true))).unwrap_or_else(|| "an item".into());
                self.hud.push_chat(ro_net::ChatKind::System, format!("Sold {amount} x {name} for {} z.", amount as u64 * price as u64));
                self.inventory.remove(index, amount);
            }
            BuyStoreNotice(msg) => {
                if msg.contains("closed") {
                    self.stores.mine = None;
                }
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            Inbox(mut list) => {
                for h in &mut list {
                    h.title = msgstr::resolve(&h.title, "(no title)");
                    h.sender = msgstr::resolve(&h.sender, "System");
                }
                self.mailbox.inbox = list;
            }
            Mail { id, text, zeny, items, .. } => {
                let text = msgstr::resolve(&text, "");
                if let Some(h) = self.mailbox.inbox.iter_mut().find(|h| h.id == id) {
                    h.read = true;
                    self.mailbox.reading = Some((h.clone(), text, zeny, items));
                }
                let ids = self.mailbox.item_ids();
                self.load_icons_for(&ids);
            }
            MailTaken { items, id, result } => {
                if result == 0 {
                    if let Some(r) = self.mailbox.reading.as_mut().filter(|r| r.0.id == id) {
                        if items {
                            r.3.clear();
                        } else {
                            r.2 = 0;
                        }
                    }
                    self.mailbox.status = if items { "Items taken.".into() } else { "Zeny taken.".into() };
                } else {
                    self.mailbox.status = "Couldn't take that (weight or inventory full?).".into();
                }
            }
            MailDeleted(id) => {
                self.mailbox.inbox.retain(|h| h.id != id);
                if self.mailbox.reading.as_ref().is_some_and(|r| r.0.id == id) {
                    self.mailbox.reading = None;
                }
            }
            MailUnread(new) => {
                if new && !self.mailbox.open {
                    self.mailbox.unread = true;
                    self.hud.push_chat(ro_net::ChatKind::System, "You have new mail (Alt+X).".into());
                }
            }
            MailReceiver(id, _) => {
                let me = self.hud.status.name.clone();
                if let Some(req) = self.mailbox.receiver_found(id, &me) {
                    self.send(ro_net::Command::Mail(req));
                }
            }
            ClanInfo(info) => self.clan.info = Some(info),
            ClanOnline(on, max) => self.clan.online = Some((on, max)),
            Navigate { map, x, y } => {
                self.navi.ensure_loaded(self.vfs.as_ref());
                let cell = (x > 0 && y > 0).then_some((x, y));
                let label = match cell { Some((x, y)) => format!("{map} ({x}, {y})"), None => map.clone() };
                self.navi.go(&map, cell, label);
            }
            Bg(e) => {
                let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                if let Some(text) = self.battle.event(e, &map) {
                    self.hud.push_chat(ro_net::ChatKind::Battle, text);
                }
                if self.battle.auto_accept {
                    if let Some((name, lobby)) = self.battle.ready.take() {
                        self.send(ro_net::Command::Bg(ro_net::BgRequest::Reply { accept: true, name, lobby }));
                    }
                }
            }
            Castles(list) => self.battle.castles = list,
            CastleInfo { id, economy, defense } => self.battle.castle_info(id, economy, defense),
            CastleTeleport(r) => {
                let why = if r == 1 { "You don't have enough zeny to teleport to the castle." } else { "You can't teleport to the castle right now." };
                self.hud.push_chat(ro_net::ChatKind::System, why.into());
            }
            Market(items) => {
                self.market.open(market::Shop::Market(items));
                self.market_test_buy();
            }
            MarketResult { ok, bought } => {
                self.market.status = if ok { "Thank you for your purchase.".into() } else { "The purchase failed.".into() };
                if let Some(market::Shop::Market(items)) = self.market.shop.as_mut() {
                    for (item, n, _) in bought {
                        if let Some(it) = items.iter_mut().find(|i| i.item == item) {
                            it.stock = it.stock.saturating_sub(n as u32);
                        }
                    }
                }
            }
            Barter { items, expanded } => {
                self.market.open(market::Shop::Barter { items, expanded });
                self.market_test_buy();
            }
            ClanLeft => {
                self.clan.left();
                self.hud.push_chat(ro_net::ChatKind::System, "You left your clan.".into());
            }
            MailItemAdded { result, index, amount, id } => self.mailbox.item_added(result, index, amount, id),
            MailItemRemoved { ok, index, amount } => self.mailbox.item_removed(ok, index, amount),
            MailSent(result) => {
                self.mailbox.sent(result);
                if result == 0 {
                    self.send(ro_net::Command::Mail(ro_net::MailRequest::Open { refresh: true }));
                }
            }
            IgnoreResult { all, block, result } => {
                let who = if all { "everyone".to_string() } else { self.ignore_pending.take().map_or_else(|| "that player".into(), |(n, _)| n) };
                let msg = match (result, block) {
                    (0, true) => format!("Whispers from {who} are now blocked."),
                    (0, false) => format!("Whispers from {who} are allowed again."),
                    (2, _) => "Your block list is full.".into(),
                    (_, true) => format!("Whispers from {who} were already blocked."),
                    (_, false) => format!("{who} wasn't blocked."),
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            IgnoreList(names) => {
                let msg = if names.is_empty() { "You aren't blocking anyone's whispers.".into() } else { format!("Blocked: {}", names.join(", ")) };
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            BookingResult { delete, result } => {
                self.booking.status = match (delete, result) {
                    (false, 0) => {
                        self.booking.posted = true;
                        "Your ad is posted.".into()
                    }
                    (false, 2) => {
                        self.booking.posted = true;
                        "You already have an ad posted.".into()
                    }
                    (false, _) => "Couldn't post the ad.".into(),
                    (true, 0) => {
                        self.booking.posted = false;
                        "Your ad was removed.".into()
                    }
                    (true, _) => "Couldn't remove the ad.".into(),
                };
                let level = self.hud.status.base_level.clamp(1, 999) as u16;
                let _ = level;
                    self.send(ro_net::Command::Booking(ro_net::BookingRequest::Search { level: 0, map: 0, job: 0xffff }));
            }
            BookingList(ads) => {
                let me = self.hud.status.name.clone();
                self.booking.posted = ads.iter().any(|a| a.leader == me) || self.booking.posted;
                self.booking.ads = ads;
            }
            RoomSign(room) => {
                if room.owner == self.account_id {
                    if let Some(inside) = self.rooms.inside.as_mut().filter(|r| r.0 == 0) {
                        inside.0 = room.id;
                    }
                }
                self.rooms.signs.insert(room.owner, room);
            }
            RoomGone(id) => self.rooms.signs.retain(|_, r| r.id != id),
            RoomCreated(code) => {
                if code == 0 {
                    // The creator gets no member list: the room is just us, as owner.
                    let me = self.hud.status.name.clone();
                    self.rooms.inside = Some((0, vec![(true, me)]));
                }
                let msg = match code {
                    0 => None,
                    1 => Some("Too many chat rooms here."),
                    _ => Some("You already have a chat room."),
                };
                if let Some(m) = msg {
                    self.hud.push_chat(ro_net::ChatKind::System, m.into());
                }
            }
            RoomRefused(code) => {
                let msg = match code {
                    0 => "The room is full.",
                    1 => "Wrong password.",
                    2 => "You were kicked from that room.",
                    4 => "You don't have enough zeny to join.",
                    5 => "Your level is too low for that room.",
                    6 => "Your level is too high for that room.",
                    7 => "Your job can't join that room.",
                    _ => "",
                };
                if !msg.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, msg.into());
                }
            }
            RoomEntered { id, members } => self.rooms.inside = Some((id, members)),
            RoomMember { joined, name, kicked } => {
                let me = self.hud.status.name.clone();
                if !joined && name == me {
                    self.rooms.inside = None;
                    if kicked {
                        self.hud.push_chat(ro_net::ChatKind::System, "You were kicked from the chat room.".into());
                    }
                } else if let Some((_, members)) = self.rooms.inside.as_mut() {
                    if joined {
                        members.push((false, name.clone()));
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} joined the chat room."));
                    } else {
                        members.retain(|(_, n)| *n != name);
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} left the chat room."));
                    }
                }
            }
            RoomOwner(name) => {
                if let Some((_, members)) = self.rooms.inside.as_mut() {
                    for m in members.iter_mut() {
                        m.0 = m.1 == name;
                    }
                }
            }
            Pet(info) => self.companions.pet = Some(info),
            CompanionState { homun, state, id, value } => {
                if homun {
                    if state == 0 {
                        self.companions.homun_id = id;
                    } else if let Some(h) = self.companions.homun.as_mut() {
                        match state {
                            1 => h.intimacy = value as u16,
                            2 => h.hunger = value as u16,
                            _ => {}
                        }
                    }
                } else if state == 0 {
                    self.companions.pet_id = id;
                    // The server sends the pet's details only when asked.
                    self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(0)));
                } else if let Some(p) = self.companions.pet.as_mut() {
                    match state {
                        1 => p.intimacy = value as u16,
                        2 => p.hunger = value as u16,
                        3 => p.accessory = value as u16,
                        _ => {}
                    }
                }
            }
            Homun(info) => self.companions.homun = Some(info),
            CompanionSkills { merc, skills } => {
                if merc {
                    self.companions.merc_skills = skills;
                } else {
                    self.companions.homun_skills = skills;
                }
            }
            Merc(info) => self.companions.merc = Some(info),
            MsgId(id @ (1266 | 1267)) => {
                // The mercenary is gone: killed, or its contract ran out.
                self.companions.merc = None;
                self.companions.merc_skills.clear();
                self.merc_ai = None;
                tracing::info!(id, "mercenary gone");
                let msg = if id == 1267 { "Your mercenary was killed." } else { "Your mercenary's contract has ended." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            MsgId(id) => match msgstr::get(id as usize, &[]) {
                Some(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
                None => tracing::debug!(id, "server message"),
            },
            Elemental { id, hp, max_hp, sp, max_sp } => {
                let name = self.units.get(&id).map(|m| m.name.clone()).unwrap_or_default();
                self.companions.elem = Some(companions::Elemental { id, name, hp, max_hp, sp, max_sp });
            }
            ElemParam(k, v) => {
                if let Some(e) = self.companions.elem.as_mut() {
                    let u = v.max(0) as u32;
                    match k {
                        5 => e.hp = u,
                        6 => e.max_hp = u,
                        7 => e.sp = u,
                        8 => e.max_sp = u,
                        _ => {}
                    }
                    let id = e.id;
                    let (hp, max) = (e.hp as i32, e.max_hp as i32);
                    if let Some(m) = self.units.get_mut(&id) {
                        m.hp = Some((hp, max));
                    }
                }
            }
            HomunParam(k, v) => {
                if let Some(h) = self.companions.homun.as_mut() {
                    let u = v.clamp(0, u32::MAX as i64) as u32;
                    match k {
                        1 => h.exp = v,
                        5 => h.hp = u,
                        6 => h.max_hp = u,
                        7 => h.sp = u,
                        8 => h.max_sp = u,
                        _ => {}
                    }
                }
            }
            HomunSkill { id, level, sp, range, upgradable } => {
                if let Some(s) = self.companions.homun_skills.iter_mut().find(|s| s.id == id) {
                    s.level = level;
                    s.sp = sp;
                    s.range = range;
                    s.upgradable = upgradable;
                }
            }
            SummonHp { id, hp, max } => {
                if let Some(m) = self.units.get_mut(&id) {
                    let max = max.map(|x| x as i32).or(m.hp.map(|h| h.1)).unwrap_or(hp as i32);
                    m.hp = Some((hp as i32, max));
                }
            }
            MercParam(k, v) => {
                if let Some(m) = self.companions.merc.as_mut() {
                    match k {
                        5 => m.hp = v.max(0) as u32,
                        6 => m.max_hp = v.max(0) as u32,
                        7 => m.sp = v.max(0) as u32,
                        8 => m.max_sp = v.max(0) as u32,
                        _ => {}
                    }
                }
            }
            Achievements { rank, list } => {
                self.progress.set_all(rank, list);
                let ids = self.progress.item_ids();
                self.load_icons_for(&ids);
            }
            AchievementUpdate { rank, ach } => {
                if let Some(name) = self.progress.update(rank, ach) {
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Achievement complete: {name}"));
                    self.play_sound("ui_achievement_complete.wav");
                }
            }
            AchievementReward { failed, id } => {
                if failed {
                    self.hud.push_chat(ro_net::ChatKind::System, "The achievement reward couldn't be given.".into());
                } else {
                    self.progress.rewarded(id);
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Reward collected: {}", self.progress.name(id)));
                }
            }
            TitleChanged { failed, id } => {
                if !failed {
                    self.progress.title = id;
                    let msg = if id == 0 { "Title removed.".into() } else { format!("Title: {}", self.progress.title_name(id)) };
                    self.hud.push_chat(ro_net::ChatKind::System, msg);
                }
            }
            UiOpen { kind: 7, data } => {
                if let Some(vfs) = self.vfs.as_ref() {
                    self.progress.load_art(&self.egui_ctx, vfs);
                }
                self.progress.attendance = Some(data);
            }
            UiOpen { kind: 1, .. } => {
                let (hair, color) = self.player.as_ref().and_then(|p| p.look.as_ref()).map_or((1, 0), |l| (l.hair, l.hair_color));
                self.stylist.open(hair, color, 0);
            }
            UiOpen { kind: 8, .. } => {
                self.grade.open = true;
            }
            GradeOptions(o) => self.grade.options = Some(o),
            GradeResult { index, grade, result } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.grade = grade as u8;
                    // A grade raised starts again from +0.
                    if result == 0 {
                        it.refine = 0;
                    }
                }
                if let Some(r) = self.grade.result(grade, result) {
                    self.send(ro_net::Command::Grade(r));
                }
            }
            GradeBroadcast { name, item, grade, ok } => {
                let item = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let letter = grade::LETTERS.get(grade as usize).copied().unwrap_or("?");
                let text = if ok { format!("{name} raised {item} to grade {letter}!") } else { format!("{name} failed to raise the grade of {item}.") };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            UiOpen { .. } => {}
            Attendance(data) => {
                self.progress.attendance = Some(data);
                self.hud.push_chat(ro_net::ChatKind::System, format!("Attendance checked: day {}. Your reward was sent by mail.", data / 10));
            }
            CashShop { cash, kafra } => {
                self.stores.cash = Some((cash, kafra));
                if let Some(vfs) = self.vfs.as_ref() {
                    self.stores.load_cash_art(&self.egui_ctx, vfs);
                }
                // The server sends the item list once per login.
                if self.stores.cash_tabs.is_empty() {
                    self.send(ro_net::Command::CashShop(ro_net::CashShopRequest::List));
                }
            }
            CashTab { tab, items } => {
                self.stores.cash_tabs.insert(tab, items);
                let ids = self.stores.item_ids();
                self.load_icons_for(&ids);
            }
            CashResult { result, cash, kafra } => {
                if let Some(c) = self.stores.cash.as_mut() {
                    *c = (cash, kafra);
                }
                let msg = match result {
                    0 => "Purchase complete.",
                    2 => "You don't have enough points.",
                    3 => "That item isn't for sale.",
                    4 => "You are carrying too much weight.",
                    5 => "You can't carry that many items.",
                    6 => "You can't buy anything right now.",
                    _ => "The purchase failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            ShopBuyResult(code) => {
                let msg = match code {
                    0 => "Purchase complete.",
                    1 => "You don't have enough zeny.",
                    2 => "You are carrying too much weight.",
                    4 => "The shop is out of that item.",
                    5 => "The shop has changed; look again.",
                    6 => "The seller cannot take more zeny.",
                    7 => "You cannot carry that many items.",
                    _ => "The purchase failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Bank { balance, reply } => {
                if let Some((b, _)) = self.merchant.bank.as_mut() {
                    *b = Some(balance);
                }
                if let Some((deposit, reason)) = reply.filter(|(_, r)| *r != 0) {
                    let why = match (deposit, reason) {
                        (true, 1) | (false, 1) => "The bank is busy; try again.",
                        (true, 2) => "You don't have that much zeny.",
                        (false, 2) => "The bank doesn't hold that much.",
                        (_, 3) => "That would exceed the limit.",
                        _ => "The bank couldn't do that.",
                    };
                    self.hud.push_chat(ro_net::ChatKind::System, why.into());
                }
            }
            ViewEquip { name, job, items } => {
                let items: Vec<ro_game::items::InvItem> = items.into_iter().map(inv_item).collect();
                let ids: Vec<u32> = items.iter().map(|i| i.id).collect();
                self.load_icons_for(&ids);
                self.item_info.viewing = Some((name, job_title(job).to_string(), items));
            }
            ShowEquip(on) => self.show_equip = Some(on),
            RefineOpen => {
                self.refine.open = true;
                self.refine.selected = None;
                self.windows.inventory = true;
            }
            RefineInfo { index, blessing, options } => {
                let ids: Vec<u32> = options.iter().map(|o| o.material).collect();
                self.load_icons_for(&ids);
                self.refine.selected = Some((index, blessing, options));
            }
            RefineResult { result, index, refine } => {
                let name = self.inventory.items.get(&index).and_then(|i| self.item_db.as_ref().map(|d| d.name(i.id, true))).unwrap_or_else(|| "The item".into());
                let (msg, sound) = match result {
                    0 => (format!("Refine succeeded: +{refine} {name}!"), "bs_refinesuccess.wav"),
                    1 => (format!("Refine failed: {name} broke."), "bs_refinefailed.wav"),
                    2 => (format!("Refine failed: {name} went down to +{refine}."), "bs_refinefailed.wav"),
                    _ => (format!("Refine failed: {name} stays +{refine}."), "bs_refinefailed.wav"),
                };
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.refine = refine as u8;
                }
                self.play_sound(sound);
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            SkillCooldown { skill, ms } => {
                self.cooldowns.insert(skill, std::time::Instant::now() + std::time::Duration::from_millis(ms as u64));
            }
            Emotion { id, kind } => {
                self.emotes.insert(id, (kind, 0.0));
            }
            WhisperResult(code) => {
                let why = match code {
                    0 => return,
                    1 => "That character is not online.",
                    2 => "Your whisper was ignored.",
                    _ => "That character is ignoring all whispers.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, why.into());
            }
            Npc(d) => self.npc_dialog(d),
            Vanish { id, reason } => {
                if self.companions.elem.as_ref().is_some_and(|e| e.id == id) && reason != 0 {
                    self.companions.elem = None;
                }
                self.warps.remove(&id);
                if self.attack.is_some_and(|(t, _)| t == id) {
                    self.attack = None;
                }
                if id == self.account_id {
                    if reason == 1 {
                        if let Some(p) = self.player.as_mut() {
                            p.set_pose(Pose::Dead, f32::MAX);
                        }
                        self.hud.push_chat(ro_net::ChatKind::System, "You have died.".into());
                        self.feel(haptics::Cue::Death);
                        self.dead = true;
                        self.attack = None;
                        self.walk_goal = None;
                    }
                } else if reason == 1 {
                    // Died: play the death animation, then remove.
                    let death_3d = self.units.get(&id).and_then(|m| self.models3d.get(&m.class)).and_then(|l| l.as_ref()).map(|l| l.death_ms());
                    if let Some(m) = self.units.get_mut(&id) {
                        m.walker.stop_at(m.walker.cell);
                        m.set_pose(Pose::Dead, f32::MAX);
                        m.dying = Some(death_3d.map_or(1500.0, |ms| ms.max(500.0)));
                    }
                } else {
                    self.units.remove(&id);
                }
            }
            Act(act) => {
                if act.target != 0 && act.src != act.target {
                    self.aggro.insert(act.src, act.target);
                }
                self.feel_act(&act);
                self.act(act)
            }
            OutOfRange { target, target_cell, range } => {
                // Walk toward the target; the attack is re-sent once in reach.
                self.chasing = true;
                self.attack = Some((target, range.max(1)));
                self.request_walk(target_cell);
            }
            LookChange { id, kind, value, value2 } => self.look_change(id, kind, value, value2),
            StorageOpen => {
                self.social.storage.items.clear();
                self.social.storage.open = true;
                // Talking to the Kafra ends the dialog the storage was opened from.
                self.dialog = None;
                self.cutin = None;
            }
            StorageItems(list) => {
                for it in list {
                    self.social.storage.items.insert(it.index, inv_item(it));
                }
                self.load_icons();
            }
            StorageAdded(it) => {
                let it = inv_item(it);
                match self.social.storage.items.get_mut(&it.index) {
                    Some(s) if s.id == it.id => s.amount += it.amount,
                    _ => {
                        self.social.storage.items.insert(it.index, it);
                    }
                }
                self.load_icons();
            }
            StorageRemoved { index, amount } => {
                if let Some(s) = self.social.storage.items.get_mut(&index) {
                    s.amount = s.amount.saturating_sub(amount);
                    if s.amount == 0 {
                        self.social.storage.items.remove(&index);
                    }
                }
            }
            StorageCount { used, capacity } => self.social.storage.count = (used, capacity),
            StorageClosed => self.social.storage.open = false,
            ShopDeal(npc) => {
                self.dialog = None;
                self.social.shop = Some(social::Shop::Choose { npc });
                if let Some(buy) = self.auto_deal.take() {
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Deal { npc, buy }));
                }
            }
            ShopBuyList(items) => {
                if let Some(id) = self.auto_buy.take() {
                    tracing::info!(id, "buying");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Buy(vec![(id, 1)])));
                }
                self.social.shop = Some(social::Shop::Buy { items, cart: Default::default() });
                self.load_shop_icons();
            }
            RankPoints { kind, gained, total } => {
                let (id, en) = match kind {
                    0 => (901, "Blacksmith rank"),
                    1 => (902, "Alchemist rank"),
                    2 => (926, "Tae-Kwon Mission rank"),
                    3 => (953, "Slayer rank"),
                    _ => (1209, "Ranking"),
                };
                let (g, t) = (gained.to_string(), total.to_string());
                let text = msgstr::get(id, &[&g, &t]).unwrap_or_else(|| format!("[Point] You have been rewarded with {g} {en} points. Your point total is {t}."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            Ping(ms) => self.ping = Some(ms),
            MannerResult(result) => {
                let text = match result {
                    0 => Some(msgstr::get(854, &[]).unwrap_or_else(|| "A manner point has been successfully aligned.".into())),
                    3 => Some("Chat Block has been applied by GM due to your ill-mannerous action.".into()),
                    4 => Some("Automated Chat Block has been applied due to Anti-Spam System.".into()),
                    _ => None,
                };
                if let Some(text) = text {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            Muted { muted, by } => {
                let (id, en) = if muted { (388, "negative") } else { (387, "positive") };
                let text = msgstr::get(id, &[&by]).unwrap_or_else(|| format!("Received {en} manner point from {by}."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PointShop { cash, kafra, items } => {
                self.dialog = None;
                if let Some(id) = self.auto_buy.take() {
                    tracing::info!(id, "buying with points");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::CashBuy { kafra: 0, items: vec![(id, 1)] }));
                }
                self.social.shop = Some(social::Shop::Cash { items, cart: Default::default(), cash, kafra, use_kafra: 0 });
                self.load_shop_icons();
            }
            PointShopResult { cash: c, kafra: k, error } => {
                if let Some(social::Shop::Cash { cash, kafra, cart, use_kafra, .. }) = self.social.shop.as_mut() {
                    (*cash, *kafra) = (c, k);
                    if error == 0 {
                        cart.clear();
                        *use_kafra = 0;
                    }
                }
                let text = match error {
                    0 => "The deal has successfully completed.",
                    1 => "The purchase has failed because the NPC does not exist.",
                    3 => "You are over your Weight Limit.",
                    4 => "You cannot purchase items while you are in a trade.",
                    5 => "The purchase has failed because the item information was incorrect.",
                    6 => "You do not have enough points.",
                    7 => "You can purchase up to 10 items.",
                    8 => "Some items could not be purchased.",
                    _ => "The purchase has failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            ShopSellList(offers) => {
                if let Some(id) = self.auto_sell.take() {
                    let list: Vec<(u16, u16)> = offers
                        .iter()
                        .filter_map(|o| self.inventory.items.get(&o.index).filter(|i| i.id == id).map(|i| (i.index, i.amount)))
                        .collect();
                    tracing::info!(?list, "selling");
                    self.send(ro_net::Command::Shop(ro_net::ShopRequest::Sell(list)));
                }
                self.social.shop = Some(social::Shop::Sell { offers, cart: Default::default() });
            }
            ShopResult { buy, ok } => {
                let msg = match (buy, ok) {
                    (true, true) => "Purchase complete.",
                    (true, false) => "You could not buy that (not enough zeny, weight or space).",
                    (false, true) => "Sale complete.",
                    (false, false) => "You could not sell that.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            TradeRequested { name, .. } => self.social.prompts.push(social::Prompt::Trade { name }),
            TradeAnswer(result) => match result {
                3 => {
                    let partner = self.trade_partner.take().unwrap_or_default();
                    self.social.trade = Some(social::Trade { partner, ..Default::default() });
                }
                r => {
                    let why = match r {
                        0 => "too far away",
                        1 => "that character is not available",
                        2 => "they are already trading",
                        4 => "they declined",
                        _ => "they are busy",
                    };
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Trade failed: {why}."));
                }
            },
            TradeItem { id, amount, .. } => {
                if let Some(t) = self.social.trade.as_mut() {
                    if id == 0 {
                        t.zeny_theirs = amount;
                    } else {
                        t.theirs.push((id, amount));
                    }
                }
                self.load_icons_for(&[id]);
            }
            TradeAdded { index, ok } => {
                if let (true, Some(t)) = (ok, self.social.trade.as_mut()) {
                    if let Some(it) = self.inventory.items.get(&index) {
                        let amount = self.trade_offers.remove(&index).unwrap_or(it.amount as i32);
                        t.ours.push((it.id, amount));
                    }
                }
            }
            TradeLocked { partner } => {
                if let Some(t) = self.social.trade.as_mut() {
                    if partner {
                        t.they_locked = true;
                    } else {
                        t.we_locked = true;
                    }
                }
            }
            TradeEnded { completed } => {
                self.trade_stage = 0;
                tracing::info!(completed, "trade ended");
                self.social.trade = None;
                let msg = if completed { "Trade completed." } else { "Trade cancelled." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Party { name, members } => {
                let hp = self.social.party.take().map(|p| p.hp).unwrap_or_default();
                self.social.party = Some(social::Party { name, members, hp });
            }
            PartyMember { name, member } => {
                let p = self.social.party.get_or_insert_with(|| social::Party { name: name.clone(), ..Default::default() });
                p.members.retain(|m| m.account_id != member.account_id);
                p.members.push(member);
            }
            PartyLeft { account_id, name } => {
                if account_id == self.account_id {
                    self.social.party = None;
                    self.hud.push_chat(ro_net::ChatKind::System, "You left the party.".into());
                } else if let Some(p) = self.social.party.as_mut() {
                    p.members.retain(|m| m.account_id != account_id);
                    self.hud.push_chat(ro_net::ChatKind::Party, format!("{name} left the party."));
                }
            }
            PartyHp { account_id, hp, max_hp } => {
                if let Some(p) = self.social.party.as_mut() {
                    p.hp.insert(account_id, (hp, max_hp));
                }
            }
            PartyInvite { party, name } => self.social.prompts.push(social::Prompt::Party { party, name }),
            WebAuth { account_id, token, world } => {
                let login = agency::Login { url: self.web_url.clone(), aid: account_id, token, world };
                self.emblem_web.set_login(login.clone());
                self.agency.set_login(login);
            }
            AgencyAsk(ask) => self.social.prompts.push(social::Prompt::Agency(ask)),
            AgencyResult { leader, refused, .. } => {
                let (id, en) = if refused { (3499, "rejected") } else { (3498, "approved") };
                let text = msgstr::get(id, &[&leader]).unwrap_or_else(|| format!("[{leader}]'s party leader has {en} your join request."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PartyCreated { ok } => {
                let msg = if ok { "Party created." } else { "Could not create the party (name taken, or already in one)." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Guild { id, name } => self.social.guild = (id != 0 && !name.is_empty()).then_some(name),
            GuildBelong { id, master, name } => {
                (self.guild.id, self.guild.master) = (id, master);
                if id == 0 {
                    (self.guild.info, self.guild.members) = (None, vec![]);
                } else {
                    self.guild.name = name.clone();
                    self.guild.needs_refresh = true;
                }
                self.social.guild = (id != 0 && !name.is_empty()).then_some(name);
            }
            GuildCreated(result) => {
                let text = match result {
                    0 => "Guild created.",
                    1 => "You are already in a guild.",
                    2 => "That guild name is already taken.",
                    3 => "You need an Emperium to found a guild.",
                    _ => "Could not create the guild.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
                if result == 0 {
                    self.guild.open = true;
                    self.guild.needs_refresh = true;
                }
            }
            GuildInfo(info) => {
                self.guild.name = info.name.clone();
                if let Some(p) = self.player.as_mut() {
                    p.guild = info.id;
                }
                // Fetch the emblem when it is new or changed.
                if info.emblem != 0 && self.guild.emblem.as_ref().is_none_or(|e| e.0 != info.emblem) {
                    // The guild window needs it even if name labels already have it.
                    self.emblem_versions.remove(&info.id);
                    self.want_emblem(info.id, info.emblem);
                }
                self.guild.info = Some(info);
            }
            GuildEmblem { guild, version, data } => {
                if let Some(raw) = guild::inflate_emblem(&data) {
                    self.show_emblem(guild, version, &raw);
                }
            }
            GuildMembers(m) => self.guild.members = m,
            GuildRankNames(names) => {
                for (id, name) in names {
                    match self.guild.ranks.iter_mut().find(|r| r.id == id) {
                        Some(r) => r.name = name,
                        None => self.guild.ranks.push(ro_net::Position { id, name, ..Default::default() }),
                    }
                }
                self.guild.ranks.sort_by_key(|r| r.id);
            }
            GuildRankInfo(info) => {
                for (id, mode, tax) in info {
                    if let Some(r) = self.guild.ranks.iter_mut().find(|r| r.id == id) {
                        (r.mode, r.tax) = (mode, tax);
                    }
                }
            }
            GuildRanksChanged(list) => {
                for p in list {
                    match self.guild.ranks.iter_mut().find(|r| r.id == p.id) {
                        Some(r) => *r = p,
                        None => self.guild.ranks.push(p),
                    }
                }
                self.guild.ranks.sort_by_key(|r| r.id);
            }
            GuildRankMoves(moves) => {
                for (_, char_id, position) in moves {
                    if let Some(m) = self.guild.members.iter_mut().find(|m| m.char_id == char_id) {
                        m.position = position;
                    }
                }
            }
            GuildSkills { points, skills } => (self.guild.skill_points, self.guild.skills) = (points, skills),
            GuildNotice { subject, text } => self.guild.notice = (subject, text),
            GuildInvited { guild, name } => self.social.prompts.push(social::Prompt::Guild { guild, name }),
            GuildInviteResult(r) => {
                let text = match r {
                    0 => "That player is already in a guild.",
                    1 => "Your guild invitation was declined.",
                    2 => "A new member joined the guild.",
                    3 => "The guild is full.",
                    _ => "Guild invitation failed.",
                };
                self.hud.push_chat(ro_net::ChatKind::Guild, text.into());
                if r == 2 {
                    self.guild.needs_refresh = true;
                }
            }
            GuildMemberState { char_id, online, .. } => {
                if let Some(m) = self.guild.members.iter_mut().find(|m| m.char_id == char_id) {
                    m.online = online;
                }
            }
            GuildMemberGone { char_id, reason, expelled } => {
                let who = self.guild.members.iter().find(|m| m.char_id == char_id).map_or_else(|| "A member".to_string(), |m| m.name.clone());
                let verb = if expelled { "was expelled from" } else { "left" };
                let why = if reason.trim().is_empty() { String::new() } else { format!(" ({})", reason.trim()) };
                self.hud.push_chat(ro_net::ChatKind::Guild, format!("{who} {verb} the guild{why}."));
                self.guild.members.retain(|m| m.char_id != char_id);
            }
            Friends(list) => self.social.friends = list,
            FriendOnline { account_id, online, .. } => {
                if let Some(f) = self.social.friends.iter_mut().find(|f| f.account_id == account_id) {
                    f.online = online;
                }
            }
            FriendRequested { account_id, char_id, name } => self.social.prompts.push(social::Prompt::Friend { account_id, char_id, name }),
            FriendAdded { ok, friend } => {
                if ok {
                    self.hud.push_chat(ro_net::ChatKind::System, format!("{} is now your friend.", friend.name));
                    self.social.friends.retain(|f| f.account_id != friend.account_id);
                    self.social.friends.push(friend);
                }
            }
            FriendRemoved { account_id, .. } => self.social.friends.retain(|f| f.account_id != account_id),
            InventoryStart => self.inventory.items.clear(),
            Items(list) => {
                for it in list {
                    self.inventory.set(inv_item(it));
                }
                self.load_icons();
            }
            ItemAdded(it) => {
                tracing::debug!(id = it.id, amount = it.amount, "item added");
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, it.identified));
                self.hud.push_chat(ro_net::ChatKind::System, format!("You got {name} ({}).", it.amount));
                self.feel(haptics::Cue::Pickup);
                self.inventory.add(inv_item(it));
                self.load_icons();
            }
            ItemRemoved { index, amount } => self.inventory.remove(index, amount),
            ItemUsed { index, amount, user, ok } => {
                if ok && user == self.account_id {
                    self.inventory.set_amount(index, amount);
                }
            }
            Equipped { index, slots, ok, .. } => {
                tracing::debug!(index, slots, ok, "equip");
                if ok {
                    self.inventory.set_equipped(index, slots);
                } else {
                    self.hud.push_chat(ro_net::ChatKind::System, "You cannot equip this item.".into());
                }
            }
            Unequipped { index, ok, .. } => {
                if ok {
                    self.inventory.set_equipped(index, 0);
                }
            }
            FloorItem(f) => {
                tracing::debug!(id = f.id, item = f.item, x = f.x, y = f.y, effect = ?f.drop_effect, "floor item");
                // Cards and rare drops fall with a light pillar that stays until picked up.
                if let Some((pillar, glow, sound)) = f.drop_effect.and_then(fx::drop_pillar) {
                    let (fx, fy) = floor_cell(&f);
                    let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                    if let Some(w) = self.world.as_ref() {
                        let pos = glam::Vec3::new(x, w.floor_at(x, z), z);
                        self.play_fx_tagged(glow, pos, f.id);
                        self.play_fx_tagged(pillar, pos, f.id);
                        if let (Some(audio), Some(vfs)) = (self.audio.as_ref(), self.vfs.as_ref()) {
                            audio.play(vfs, sound);
                        }
                    }
                }
                let actor = self.item_actor(f.item, f.identified);
                self.floor.insert(f.id, (f, actor));
            }
            FloorItemGone(id) => {
                self.floor.remove(&id);
                self.stop_fx(id);
                if self.pending_pickup == Some(id) {
                    self.pending_pickup = None;
                }
            }
            CardTargets(equips) => {
                if let Some((_, list)) = self.windows.card_pick.as_mut() {
                    *list = equips;
                }
            }
            CardInserted { equip, card, ok } => {
                // The server takes the card without telling us (pc_insert_card deletes it
                // silently), as the original client expects: on success, one card leaves the
                // stack and goes into the equipment's first free slot.
                let card_id = self.inventory.items.get(&card).map(|c| c.id);
                if let (true, Some(id)) = (ok, card_id) {
                    if let Some(slot) = self.inventory.items.get_mut(&equip).and_then(|e| e.cards.iter_mut().find(|c| **c == 0)) {
                        *slot = id;
                    }
                    self.inventory.remove(card, 1);
                }
                let msg = if ok { "The card was inserted." } else { "The card could not be inserted." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.to_string());
            }
            Stats(s) => self.stats = Some(s),
            StatRaised { stat, ok, value } => {
                // The ack carries one byte; past 255 the full value follows in 0x0141.
                if let (true, Some(s), true) = (ok, self.stats.as_mut(), value < 255) {
                    match stat {
                        13..=18 => s.base[(stat - 13) as usize] = value as u16,
                        219..=224 => s.traits[(stat - 219) as usize] = value as u16,
                        _ => {}
                    }
                }
            }
            StatBonus { stat, base, plus } => {
                if let Some(s) = self.stats.as_mut() {
                    let base = base.clamp(0, u16::MAX as i32) as u16;
                    match stat {
                        13..=18 => (s.base[(stat - 13) as usize], s.plus[(stat - 13) as usize]) = (base, plus as i16),
                        219..=224 => (s.traits[(stat - 219) as usize], s.trait_plus[(stat - 219) as usize]) = (base, plus as i16),
                        _ => {}
                    }
                }
            }
            StatCost { stat, cost } => {
                if let Some(s) = self.stats.as_mut() {
                    match stat {
                        32..=37 => s.cost[(stat - 32) as usize] = cost,
                        247..=252 => s.trait_cost[(stat - 247) as usize] = cost,
                        _ => {}
                    }
                }
            }
            Skills(list) => {
                self.skills = list;
                self.load_skill_icons();
            }
            SkillUpdate { id, level, sp, range, upgradable } => {
                if let Some(s) = self.skills.iter_mut().find(|s| s.id == id) {
                    (s.level, s.sp, s.range, s.upgradable) = (level, sp, range, upgradable);
                }
            }
            Hotkeys { row, keys } => {
                let n = self.hotkeys.len();
                self.hotkeys = keys;
                self.hotkeys.resize(n.max(self.hotkeys.len()), Default::default());
                self.hotkey_row = row % HOTKEY_ROWS;
            }
            Casting(c) => {
                let cell = (c.cell.0 as i32, c.cell.1 as i32);
                let target_cell = self.mob_mut(c.target).map(|m| m.walker.cell).or((c.target == 0).then_some(cell));
                if let Some(m) = self.mob_mut(c.src) {
                    if let Some(t) = target_cell {
                        m.face(t);
                    }
                    if c.ms > 0 {
                        m.set_pose(Pose::Cast, c.ms as f32);
                    }
                }
                if c.ms > 0 && c.src == self.account_id {
                    self.feel(haptics::Cue::Charge(c.ms));
                }
                if c.ms > 0 {
                    self.casts.insert(c.src, (0.0, c.ms as f32));
                    // The magic circle under the caster while chanting.
                    if let (Some(pos), Some(r), Some(vfs)) = (self.unit_pos(c.src), self.renderer.as_mut(), self.vfs.as_ref()) {
                        if let Some(tex) = self.fx.texture(r, vfs, "magic_target.tga") {
                            let shape = ro_render::Shape::Disc { radius: 1.6, spin: 0.25 };
                            r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [1.0, 1.0, 1.0], time_ms: 0.0, duration_ms: c.ms as f32 });
                        }
                    }
                }
            }
            Fed { homun, ok, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item as u32, true));
                let who = if homun { "homunculus" } else { "pet" };
                let text = if ok { format!("Your {who} ate {name}.") } else { format!("You don't have any {name} to feed your {who}.") };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PetAct { id, value } => {
                if value < 10_000 {
                    self.handle(Emotion { id, kind: value as u8 });
                } else if let Some(name) = self.units.get(&id).map(|m| m.name.clone()) {
                    let names = &self.class_names;
                    if let Some(line) = self.pet_talk.line(value, |c| names.get(&c).cloned()) {
                        self.hud.push_chat(ro_net::ChatKind::Public, format!("{name} : {line}"));
                    }
                }
            }
            Progress { secs } => {
                tracing::debug!(secs, "script progress bar");
                self.script_progress = Some((std::time::Instant::now(), secs.max(1) as f32 * 1000.0));
            }
            ProgressCancel => self.script_progress = None,
            ReadBook { id, page } => self.open_book(id, page),
            CaptchaImage(zipped) => {
                if self.captcha.set_image(&self.egui_ctx, &zipped) {
                    self.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Downloaded));
                }
            }
            CaptchaAsk { tries, timeout_ms } => self.captcha.ask(tries, timeout_ms),
            CaptchaDone(status) => {
                let text = self.captcha.done(status);
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            CaptchaApplied(status) => {
                let text = match status {
                    0 => "The player is now being checked by the macro detector.",
                    1 => "There are no captchas registered.",
                    _ => "That player is already being checked.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            Sale(note) => {
                let found = matches!(note, ro_net::SaleNote::Found(Some(_)) | ro_net::SaleNote::Selling { .. });
                let db = self.item_db.as_ref();
                if let Some(line) = self.sales.apply(note, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true))) {
                    self.hud.push_chat(ro_net::ChatKind::Broadcast, line);
                }
                if found {
                    let ids: Vec<u32> = self.sales.on_sale.keys().copied().collect();
                    self.load_icons_for(&ids);
                }
            }
            Roulette(note) => {
                let db = self.item_db.as_ref();
                self.roulette.apply(note, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true)));
                let ids = self.roulette.items();
                self.load_icons_for(&ids);
            }
            UnitProgress { id, secs } => {
                self.unit_progress.insert(id, (0.0, secs.max(1) as f32 * 1000.0));
            }
            ShowScript { id, text } => {
                self.shouts.insert(id, (text, 0.0));
            }
            ShowDigit { kind, value } => self.digit = Some((kind, value, 0.0)),
            Devotion { src, targets } if targets.is_empty() => {
                self.devotions.remove(&src);
            }
            Devotion { src, targets } => {
                self.devotions.insert(src, targets);
            }
            Instance(note) => {
                if let Some(text) = self.memorial.apply(note) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            // As the original: only with its "Experience message" chat option, except quest
            // rewards. Losses (death) aren't shown.
            ExpGain { amount, kind, quest } => {
                if amount > 0 && (quest || self.settings.exp_messages) {
                    let n = amount.to_string();
                    let (id, what) = if kind == 2 { (488, "Job") } else { (487, "Base") };
                    let text = msgstr::get(id, &[&n]).unwrap_or_else(|| format!("You got {n} {what} EXP."));
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            Recovery { kind, amount } => {
                // HP in green, SP in blue, rising over our head like the original.
                let color = if kind == 7 { egui::Color32::from_rgb(110, 160, 255) } else { egui::Color32::from_rgb(110, 255, 110) };
                if let Some(p) = self.player.as_ref() {
                    self.floaters.push((p.actor.pos, amount.to_string(), color, 0.0));
                }
            }
            UserCount(n) => {
                let text = msgstr::get(178, &[&n.to_string()]).unwrap_or_else(|| format!("There are {n} Players Currently Connected."));
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            PvpRank { id, rank, total } => {
                if id == self.account_id {
                    // PvP switched off sends 0 of 0: hide the rank.
                    self.pvp_rank = (total > 0).then_some((rank, total));
                }
            }
            PvpInfo { won, lost, points } => {
                self.hud.push_chat(ro_net::ChatKind::System, format!("You have won {won} times and have lost {lost} times in PVP. Current points {points}."));
            }
            PartyLeader { old, new } => {
                for m in self.social.party.iter_mut().flat_map(|p| p.members.iter_mut()) {
                    m.leader = m.account_id == new || (m.leader && m.account_id != old);
                }
            }
            PartyDead { id, dead } => {
                if dead {
                    self.social.party_dead.insert(id);
                } else {
                    self.social.party_dead.remove(&id);
                }
            }
            ItemObtained { who, item, from_box, source } => {
                let name = |id: u32| self.item_db.as_ref().map_or_else(|| format!("item {id}"), |db| db.name(id, true));
                let text = match from_box {
                    Some(b) => format!("[{who}] has obtained [{}] from [{}].", name(item), name(b)),
                    None if !source.is_empty() => format!("[{who}] has obtained [{}] from [{source}].", name(item)),
                    None => format!("[{who}] has obtained [{}].", name(item)),
                };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            RefineBroadcast { who, item, level, ok } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("item {item}"), |db| db.name(item, true));
                let text = if ok { format!("[{who}] successfully upgrades an equipment, [+{level} {name}] obtained.") } else { format!("[{who}] fails to upgrade [+{level} {name}].") };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, text);
            }
            CastCancel(id) => {
                if id == self.account_id {
                    self.haptics.end_charge();
                }
                self.casts.remove(&id);
                if let Some(m) = self.mob_mut(id) {
                    if matches!(m.pose, Some((Pose::Cast, _))) {
                        m.pose = None;
                    }
                }
            }
            SkillFailed { skill, cause, amount, item } => {
                self.haptics.end_charge();
                self.feel(haptics::Cue::Fail);
                let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_else(|| "Skill".into());
                let why = match cause {
                    1 => "not enough SP",
                    2 => "not enough HP",
                    3 => "missing required items",
                    4 => "still on cooldown",
                    5 => "not enough zeny",
                    6 => "wrong weapon equipped",
                    7 => "requires a Red Gemstone",
                    8 => "requires a Blue Gemstone",
                    9 => "carrying too much weight",
                    11 => "invalid target",
                    13 => "requires Holy Water",
                    15 | 22 => "already in effect",
                    16 => "requires another skill first",
                    23 => "conditions not met",
                    26 => "can't be used here",
                    28 => "too close to a wall",
                    32 => "no poison applied to the weapon",
                    33 | 50 => "requires a Mado Gear",
                    34 => "requires Kunai",
                    35 => "can only target players",
                    43 => "requires a Guillotine poison",
                    57 => "requires a cart",
                    71 => "requires an item",
                    72 | 95 => "requires a weapon",
                    73 => "must follow a combo",
                    74 | 96 => "not enough spirit spheres",
                    83 => "there are NPCs too close",
                    84 | 90 => "not enough bullets",
                    87 => "can't be used on this map",
                    89 => "a shop is already open",
                    91 => "requires arrows",
                    92 => "requires a party",
                    93 => "requires party leader",
                    97 => "requires Weapon Blocking",
                    98 => "too many traps",
                    100 => "not enough AP",
                    110 => "requires a shield",
                    _ => "failed",
                };
                self.casts.remove(&self.account_id);
                if let Some(p) = self.player.as_mut() {
                    p.pose = None;
                }
                // Basic Skill refusals name what was refused (trade, emotions, sit, chat room,
                // party, shout, PK, aligning); rAthena also sends "emotions" when emotes come
                // faster than one a second or two.
                let basic = match (skill, cause, amount) {
                    (1, 0, n @ 0..=6) => msgstr::get(159 + n as usize, &[]),
                    (1, 0, 7) => msgstr::get(383, &[]),
                    _ => None,
                };
                let why = if cause == 71 && item != 0 {
                    let item_name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                    format!("requires {amount} {item_name}")
                } else {
                    why.to_string()
                };
                self.hud.push_chat(ro_net::ChatKind::System, basic.unwrap_or_else(|| format!("{name}: {why}.")));
            }
            SkillUsed { skill, src, target } => {
                self.feel_skill_used(src, target);
                self.casts.remove(&src);
                let target_cell = self.mob_mut(target).map(|m| m.walker.cell);
                if let Some(m) = self.mob_mut(src) {
                    if let (Some(t), true) = (target_cell, src != target) {
                        m.face(t);
                    }
                    m.set_pose(Pose::Attack, 400.0);
                }
                if skill == 406 {
                    self.meteor_assault(src);
                }
                self.skill_fx(skill, self.unit_pos(target), false);
            }
            SkillHit { skill, act } => {
                self.feel_act(&act);
                // Our skill landed: a pulse per hit.
                if act.src == self.account_id {
                    self.haptics.end_charge();
                    if act.damage > 0 && act.target != self.account_id {
                        self.feel(haptics::Cue::Strike(act.hits.max(1) as u16));
                    }
                }
                let target = act.target;
                // Grimtooth: a line of spikes erupting from the caster to the target.
                if skill == 137 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        self.spike_line(from, to);
                    }
                }
                // Soul Breaker: a ball of energy hurled at the target.
                if skill == 379 {
                    if let (Some(from), Some(to)) = (self.unit_pos(act.src), self.unit_pos(target)) {
                        let shape = ro_render::Shape::Slash { travel: to - from, size: (1.0, 2.0) };
                        self.proc_fx("purpleslash.tga", shape, from, [1.0, 1.0, 1.0], 500.0);
                    }
                }
                if skill == 406 {
                    self.meteor_assault(act.src);
                }
                if let (7015, Some(from), Some(to)) = (skill, self.unit_pos(act.src), self.unit_pos(target)) {
                    self.crescent_gale(act.src, from, to);
                }
                self.act(act);
                self.skill_fx(skill, self.unit_pos(target), true);
            }
            SkillAt { skill, src, cell } => {
                if src == self.account_id {
                    self.haptics.end_charge();
                    self.feel(haptics::Cue::Ground);
                }
                self.casts.remove(&src);
                if let Some(m) = self.mob_mut(src) {
                    m.face((cell.0 as i32, cell.1 as i32));
                    m.set_pose(Pose::Attack, 400.0);
                }
                self.skill_fx(skill, self.cell_pos(cell), true);
            }
            Font { id, font } => self.font_change(id, font),
            UnitOption { id, option } => {
                let me = id == self.account_id;
                if let Some(m) = self.mob_mut(id) {
                    m.actor.alpha = option_alpha(option, me);
                }
            }
            GroundEffect { id, cell, kind } => {
                tracing::debug!(id, ?cell, kind = format_args!("{kind:#x}"), "ground effect");
                self.ground_units.insert(id, cell);
                if let (Some(path), Some(pos)) = (fx::trap_model(kind), self.cell_pos(cell)) {
                    let draws = self.trap_draws(path, pos);
                    self.traps.insert(id, draws);
                } else if let (0x87, Some(pos)) = (kind, self.cell_pos(cell)) {
                    // A Fire Pillar waiting to go off (EF_FIREPILLARON): a red column for 5 s.
                    self.proc_fx("magic_red.tga", ro_render::Shape::Column { radius: 1.0, height: 3.0 }, pos, [1.0; 3], 5000.0);
                } else if let (Some(name), Some(pos)) = (fx::ground_str(kind), self.cell_pos(cell)) {
                    self.stop_fx(id);
                    self.play_fx_tagged(name, pos, id);
                } else {
                    tracing::debug!(kind = format_args!("{kind:#x}"), "no visual for ground effect");
                }
            }
            GroundEffectGone(id) => {
                self.stop_fx(id);
                self.traps.remove(&id);
                self.ground_units.remove(&id);
                self.graffiti.remove(&id);
            }
            Graffiti { id, cell, text } => {
                tracing::debug!(id, ?cell, %text, "graffiti");
                self.ground_units.insert(id, cell);
                self.graffiti.insert(id, text);
            }
            TalkieBox { id, text } => {
                self.shouts.insert(id, (text, 0.0));
            }
            RentalTime { item, secs } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                let name = name.as_str();
                let text = match secs {
                    120.. => msgstr::get(1256, &[&name, &(secs / 60).to_string()]),
                    60.. => msgstr::get(1257, &[&name]),
                    _ => msgstr::get(1270, &[&name, &secs.to_string()]),
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.map_or_else(|| format!("The [{name}] item will disappear in {} minutes.", secs / 60), |t| t.trim().to_string()));
            }
            StarSkill { name, id, star, result } => {
                let name = name.trim_end_matches(".gat").to_string();
                let body = ["Sun", "Moon", "Stars"].get(star as usize).copied().unwrap_or("Stars");
                let text = match result {
                    0 => format!("{name} has been designated as your Place of the {body}."),
                    1 => format!("Your Place of the {body}: {name}"),
                    10 => format!("{name} has been designated as your Target of the {body}."),
                    11 => format!("Your Target of the {body}: {name}"),
                    20 => msgstr::get(927, &[&name, &star.to_string()]).unwrap_or_else(|| format!("[Taekwon Mission] Target Monster: {name} ({star}%)")),
                    21 => msgstr::get(1190, &[&name]).unwrap_or_else(|| format!("[Taming Mission] Target Monster: {name}")),
                    22 => format!("[Collector Rank] Target Item: {}", self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true))),
                    30 => "[Sun, Moon and Stars Angel] Designated places and monsters have been reset.".into(),
                    40 => format!("Target HP: {id}"),
                    _ => String::new(),
                };
                if !text.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            StarPlaceAsk(which) => self.windows.star_ask = Some(which),
            AdoptAsk { father, mother, name } => self.windows.adopt_ask = Some((father, mother, name)),
            MacroScan(ids) => self.reporter.found(ids),
            NpcName { id, name, title } => {
                if let Some(m) = self.units.get_mut(&id) {
                    if !name.is_empty() {
                        m.name = name;
                    }
                    // A title shows on the line under the name.
                    if !title.is_empty() {
                        m.titles = Some((String::new(), title, String::new()));
                    }
                }
            }
            MacroApplied(status) => self.reporter.applied(status),
            GuildStorageLog(result, entries) => match result {
                1 => self.guild.storage_log = Some(entries),
                2 => self.hud.push_chat(ro_net::ChatKind::System, "The guild storage log is empty.".into()),
                _ => self.hud.push_chat(ro_net::ChatKind::System, "The guild storage log could not be read.".into()),
            },
            AdoptFailed(why) => {
                let text = match why {
                    0 => "That couple already has a child.",
                    1 => "Both parents must be at least base level 70 to adopt.",
                    _ => "A married character cannot be adopted.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            DynamicNpc(result) => {
                let text = match result {
                    0 => return,
                    3 => "You already have this NPC.",
                    4 => "This NPC is no longer available.",
                    _ => "The NPC could not be created.",
                };
                self.hud.push_chat(ro_net::ChatKind::System, text.into());
            }
            // The area a monster's big spell will cover, on the ground for the cast (at least a
            // moment, so an instant cast still flashes it).
            SkillScale { skill, level, x, y, cast_ms, .. } => {
                if let (Some(size), Some(pos), Some(r)) = (self.skill_scales.size(skill, level), self.cell_pos((x, y)), self.renderer.as_mut()) {
                    let tex = *self.area_tex.get_or_insert_with(|| r.add_effect_texture(&skillscale::area_texture()));
                    let shape = ro_render::Shape::Area { size };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [1.0, 0.18, 0.12], time_ms: 0.0, duration_ms: cast_ms.max(800) as f32 });
                }
            }
            PetEvolved(result) => {
                // msgstringtable 2571..2576 explain the failures.
                let text = match result {
                    6 => "Your pet has evolved.".to_string(),
                    r => msgstr::get(2571 + r.min(5) as usize, &[]).unwrap_or_else(|| "Pet evolution failed.".into()),
                };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            Camera { show: true, .. } => {
                if let Some(r) = self.renderer.as_ref() {
                    let c = &r.camera;
                    let text = format!("Camera: range {:.1}, rotation {:.1}, latitude {:.1}", c.distance, c.yaw.to_degrees(), -c.pitch.to_degrees());
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            Camera { range, rotation, latitude, .. } => {
                if let Some(r) = self.renderer.as_mut() {
                    if range > 0.0 {
                        r.camera.distance = range.clamp(ro_render::Camera::MIN_DISTANCE, ro_render::Camera::MAX_DISTANCE);
                    }
                    r.camera.yaw = rotation.to_radians();
                    r.camera.pitch = (-latitude).to_radians().clamp(25f32.to_radians(), 75f32.to_radians());
                }
            }
            RentalExpired { index, item } => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                self.inventory.remove(index, u16::MAX);
                let text = msgstr::get(1258, &[&name.as_str()]).map_or_else(|| format!("[{name}] item has been deleted from the Inventory."), |t| t.trim().to_string());
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            // EFST_SKE on ourselves: the server's night (on) or day (off).
            StatusIcon { id, kind: 160, on, .. } if id == self.account_id => {
                tracing::info!(night = on, "day/night");
                if let Some(r) = self.renderer.as_mut() {
                    r.night = on;
                }
            }
            StatusIcon { id, kind, on, remain_ms } if id == self.account_id => self.buffs.set(kind, on, remain_ms),
            StatusIcon { .. } => {}
            Resurrected(id) if id == self.account_id => self.revive(),
            Resurrected(id) => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.pose = None;
                    m.dying = None;
                }
            }
            Sound { name, act } => {
                // 0 plays once, 1 loops (played once here), 2 stops.
                if act != 2 {
                    if let (Some(a), Some(v)) = (self.audio.as_ref(), self.vfs.as_ref()) {
                        a.play(v, &name);
                    }
                }
            }
            Bgm(name) => {
                if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                    a.play_bgm(v, &name);
                }
            }
            Viewpoint { npc, kind, x, y, id, color } => {
                if kind == 2 {
                    self.viewpoints.remove(&(npc, id));
                } else {
                    let rgb = egui::Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
                    // Kind 0 marks blink for a while; 1 stays until removed or the map changes.
                    let until = (kind == 0).then(|| std::time::Instant::now() + std::time::Duration::from_secs(15));
                    self.viewpoints.insert((npc, id), ((x as f32, y as f32), rgb, until));
                }
            }
            NpcSprite { id, class } => {
                if let Some(m) = self.units.remove(&id) {
                    let (x, y) = (m.walker.cell.0 as u16, m.walker.cell.1 as u16);
                    let info = ro_net::UnitInfo {
                        object_type: m.kind,
                        id,
                        speed: m.walker.speed as i16,
                        option: 0,
                        job: class as i16,
                        hair: 0,
                        hair_color: 0,
                        look: Default::default(),
                        sex: 0,
                        name: m.name.clone(),
                        pos: ro_net::UnitPos::At { x, y, dir: m.walker.dir as u8 },
                        guild: m.guild,
                        emblem: 0,
                        level: m.level,
                        font: m.font,
                    };
                    self.unit(info);
                }
            }
            MsgValue { msg, value } => {
                let v = value.to_string();
                if let Some(text) = msgstr::get(msg as usize, &[&v]) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            MsgColor { msg, .. } => {
                if let Some(text) = msgstr::get(msg as usize, &[]) {
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            MsgSkill { skill, msg } => {
                if let Some(text) = msgstr::get(msg as usize, &[]) {
                    let name = self.skill_names.internal(skill).map(|n| self.skill_names.display(n)).unwrap_or_else(|| format!("Skill #{skill}"));
                    self.hud.push_chat(ro_net::ChatKind::System, format!("[{name}] {text}"));
                }
            }
            RemoveEffect { .. } => {}
            Notice(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
            StyleResult(failed) => self.stylist.result(failed),
            MergeOpen(list) => self.merge.open = Some(list),
            Merged { reason, .. } => {
                let msg = match reason { 0 => "The items were merged.", 1 => "Those items can't be merged.", _ => "You have too many of that item to merge." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            GuildRelations(list) => self.guild.relations = list,
            GuildBans(list) => self.guild.bans = list,
            GuildBroken(r) => {
                let text = match r {
                    0 => {
                        self.guild.id = 0;
                        self.guild.master = false;
                        self.guild.name.clear();
                        self.guild.info = None;
                        self.guild.members.clear();
                        self.guild.bans.clear();
                        self.guild.relations.clear();
                        if let Some(p) = self.player.as_mut() {
                            p.guild = 0;
                            p.titles = None;
                        }
                        "The guild has been disbanded."
                    }
                    1 => "The guild name you entered does not match.",
                    _ => "You cannot disband the guild while it still has members.",
                };
                self.hud.push_chat(ro_net::ChatKind::Guild, text.into());
            }
            GmKicked(ok) => {
                let text = if ok { msgstr::get(151, &[]).unwrap_or_else(|| "Kick/Disconnected User.".into()) } else { msgstr::get(152, &[]).unwrap_or_else(|| "Kick has failed.".into()) };
                self.hud.push_chat(ro_net::ChatKind::System, text);
            }
            GmStatus(s) => {
                const NAMES: [&str; 6] = ["Str", "Agi", "Vit", "Int", "Dex", "Luk"];
                let stats: Vec<String> = s.stats.iter().zip(NAMES).map(|((v, need), n)| format!("{n} {v} ({need})")).collect();
                for line in [
                    stats.join("  "),
                    format!("Atk {} + {}  Matk {} ~ {}  Def {} + {}  Mdef {} + {}", s.atk.0, s.atk.1, s.matk.1, s.matk.0, s.def.0, s.def.1, s.mdef.0, s.mdef.1),
                    format!("Hit {}  Flee {} + {}  Critical {}  Aspd {}", s.hit, s.flee.0, s.flee.1, s.crit, s.aspd),
                ] {
                    self.hud.push_chat(ro_net::ChatKind::System, line);
                }
            }
            DisconnectAck(result) => {
                if result == 0 {
                    self.quit = self.quit_asked.take().is_some() || self.quit;
                } else {
                    self.quit_asked = None;
                    let text = msgstr::get(502, &[]).unwrap_or_else(|| "Please wait 10 seconds before trying to log out.".into());
                    self.hud.push_chat(ro_net::ChatKind::System, text);
                }
            }
            UnitGuild { id, guild, emblem } => {
                if let Some(m) = self.mob_mut(id) {
                    m.guild = guild;
                }
                self.want_emblem(guild, emblem);
            }
            AllyRequest { inviter, guild } => self.guild.ally_offer = Some((inviter, guild)),
            RelationNotice(text) => self.hud.push_chat(ro_net::ChatKind::Guild, text),
            RelationRemoved { guild, relation } => self.guild.relations.retain(|r| !(r.0 == relation && r.1 == guild)),
            StartCapture => {
                self.capturing = true;
                self.hud.push_chat(ro_net::ChatKind::System, "Click the monster you want to tame.".into());
            }
            Captured(ok) => {
                let msg = if ok { "You tamed it! Look for its egg in your inventory." } else { "The taming failed." };
                self.hud.push_chat(ro_net::ChatKind::System, msg.into());
            }
            Turned { id, dir } => {
                if let Some(m) = self.mob_mut(id) {
                    if !m.walker.is_walking() {
                        m.walker.dir = (dir % 8) as usize;
                    }
                }
            }
            MonsterInfo { class, level, size, hp, def, race, mdef, element, mods } => {
                let name = self.units.values().find(|m| m.class == class as i16).map(|m| m.name.clone()).or_else(|| self.class_names.get(&(class as i16)).cloned()).unwrap_or_else(|| format!("Monster #{class}"));
                self.sense.info = Some(sense::MonsterInfo { name, level, size, hp, def, race, mdef, element, mods });
            }
            CoupleName(name) => {
                let line = if name.is_empty() { "Your partner can't be called right now.".to_string() } else { format!("Calling your partner, {name}.") };
                self.hud.push_chat(ro_net::ChatKind::System, line);
            }
            BladeStop { src, target, active } => {
                // Both stand locked in place while it lasts.
                if active {
                    for id in [src, target] {
                        if let Some(m) = self.mob_mut(id) {
                            let cell = m.walker.cell;
                            m.walker.stop_at(cell);
                        }
                    }
                }
            }
            ComboDelay { id, ms } => tracing::debug!(id, ms, "combo window"),
            GuildTitle { id, position } => {
                if let Some(t) = self.mob_mut(id).and_then(|m| m.titles.as_mut()) {
                    t.2 = position;
                }
            }
            CellType { x, y, kind, map } => {
                if let Some(w) = self.world.as_mut().filter(|w| map.trim_end_matches(".gat") == w.name) {
                    if let Some(g) = w.gat.as_mut() {
                        g.set_cell(x as i32, y as i32, kind as u32);
                    }
                }
            }
            PartyMemberInfo { account_id, job, level } => {
                if let Some(m) = self.social.party.iter_mut().flat_map(|p| p.members.iter_mut()).find(|m| m.account_id == account_id) {
                    m.job = job;
                    m.level = level;
                }
            }
            Spirits { id, count, kind } => {
                if count == 0 {
                    self.spirits.remove(&id);
                } else {
                    self.spirits.insert(id, (count, kind));
                }
            }
            SkillAdded(mut s) => {
                tracing::debug!(id = s.id, level = s.level, "skill added");
                match self.skills.iter_mut().find(|k| k.id == s.id) {
                    Some(k) => {
                        if s.name.is_empty() {
                            s.name = std::mem::take(&mut k.name);
                        }
                        *k = s;
                    }
                    None => self.skills.push(s),
                }
                self.load_skill_icons();
            }
            SkillRemoved(id) => self.skills.retain(|k| k.id != id),
            SwitchList(list) => {
                for it in self.inventory.items.values_mut() {
                    it.switch = 0;
                }
                for (index, pos) in list {
                    if let Some(it) = self.inventory.items.get_mut(&index) {
                        it.switch = pos;
                    }
                }
            }
            SwitchChanged { index, location, ok } => {
                if ok {
                    if let Some(it) = self.inventory.items.get_mut(&index) {
                        it.switch = location;
                    }
                } else {
                    self.hud.push_chat(ro_net::ChatKind::System, "That item can't go in the switch set.".into());
                }
            }
            SwitchDone(ok) => {
                if !ok {
                    self.hud.push_chat(ro_net::ChatKind::System, "The equipment switch failed.".into());
                }
            }
            Reputation(list) => self.reputation.points.extend(list),
            ReputationOpen => {
                self.reputation.load(self.vfs.as_ref());
                self.reputation.open = true;
            }
            Config { kind, on } => self.game_opts.config(kind, on),
            DenyPartyInvites(d) => self.game_opts.deny_invites = d,
            PartyOptions { exp_share, pickup_share, item_share } => self.game_opts.party = Some((exp_share, pickup_share, item_share)),
            AttackRange(r) => self.attack_range = r.max(1) as i32,
            MapProperty { flags, .. } => {
                self.map_flags = flags;
            }
            OverweightPercent(p) => self.hud.status.overweight_pct = p,
            Ranking { kind, list, mine } => {
                self.ranking.open = true;
                (self.ranking.kind, self.ranking.list, self.ranking.mine) = (kind, list, mine);
                for cid in self.ranking.missing() {
                    self.send(ro_net::Command::ReqName(cid));
                }
            }
            CharName { cid, name } => {
                self.ranking.names.insert(cid, name);
            }
            SearchOpen(uses) => self.store_search.opened(uses),
            SearchResults { first, more, uses, found } => self.store_search.results(first, more, uses, found),
            SearchFailed(r) => self.store_search.failed(r),
            SearchAt(x, y) => {
                self.viewpoints.insert((0, 250), ((x as f32, y as f32), egui::Color32::from_rgb(255, 200, 60), Some(std::time::Instant::now() + std::time::Duration::from_secs(30))));
                self.hud.push_chat(ro_net::ChatKind::System, format!("The shop is at ({x}, {y}), marked on the minimap."));
            }
            BossInfo { kind, x, y, hours, minutes, name } => {
                let gold = egui::Color32::from_rgb(255, 60, 60);
                match kind {
                    0 => {
                        self.viewpoints.remove(&(0, 251));
                        self.hud.push_chat(ro_net::ChatKind::System, "You cannot find any trace of a Boss Monster in this area.".into());
                    }
                    1 | 2 => {
                        self.viewpoints.insert((0, 251), ((x as f32, y as f32), gold, None));
                        if kind == 2 {
                            self.hud.push_chat(ro_net::ChatKind::System, format!("{name} is present in this area."));
                        }
                    }
                    _ => {
                        self.viewpoints.remove(&(0, 251));
                        self.hud.push_chat(ro_net::ChatKind::System, format!("{name} will appear in approximately {hours} hours {minutes} minutes."));
                    }
                }
            }
            DressRoom => {
                let (hair, color) = self.player.as_ref().and_then(|p| p.look.as_ref()).map_or((1, 0), |l| (l.hair, l.hair_color));
                self.stylist.open(hair, color, 0);
                self.stylist.dressing = true;
            }
            ReformOpen(tool) => self.reform.open(self.vfs.as_ref(), tool),
            LapineOpen { upgrade, trigger } => self.lapine.open(self.vfs.as_ref(), upgrade, trigger),
            LapineResult { upgrade, result } => {
                if let Some(a) = self.lapine.result(upgrade, result) {
                    self.lapine_action(a);
                }
                let msg = self.lapine.status.clone();
                self.hud.push_chat(ro_net::ChatKind::System, msg);
            }
            ItemChanged { index, cards, refine, grade } => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    (it.cards, it.refine, it.grade) = (cards, refine, grade);
                }
            }
            ReformResult { result, .. } => self.reform.result(result == 0),
            UiOpenV3 { kind: 10, data } => self.enchant.open(self.vfs.as_ref(), data),
            UiOpenV3 { .. } => {}
            EnchantResult { msg, item } => {
                if msg == 3857 && item != 0 {
                    if let Some((index, slot)) = self.enchant.filled_slot(&self.inventory.items) {
                        if let Some(it) = self.inventory.items.get_mut(&index) {
                            it.cards[slot] = item;
                        }
                    }
                }
                self.enchant.result(msg == 3857, item, self.item_db.as_ref());
            }
            HatEffect { id, on, effects } => {
                for e in effects {
                    if let Some(style) = custom_hat_aura(e) {
                        if on {
                            self.hat_auras.insert(id, style);
                        } else if self.hat_auras.get(&id) == Some(&style) {
                            self.hat_auras.remove(&id);
                        }
                        continue;
                    }
                    let key = (id, e);
                    if on && !self.hat_fx.contains_key(&key) {
                        let Some(name) = self.hat_files.get(&e).copied() else { continue };
                        let tag = 0x7000_0000 + self.hat_fx.len() as u32 + 1 + (id & 0xffff) * 64;
                        if let Some(pos) = self.unit_pos(id) {
                            self.play_fx_tagged(name, pos, tag);
                            self.hat_fx.insert(key, tag);
                        }
                    } else if !on {
                        if let Some(tag) = self.hat_fx.remove(&key) {
                            self.stop_fx(tag);
                        }
                    }
                }
            }
            NameAll { id, party, guild, rank } => {
                let titles = (party, guild, rank);
                if id == self.account_id {
                    if let Some(p) = self.player.as_mut() {
                        p.titles = Some(titles);
                    }
                } else if let Some(m) = self.units.get_mut(&id) {
                    m.titles = Some(titles);
                }
            }
            AutorunSkill { id, target, level } => {
                // Self (and passive-flagged) skills go off at once; others wait for a target click.
                if target & 4 != 0 || target & (1 | 2 | 16 | 32) == 0 {
                    self.use_skill_on(id, level, self.account_id);
                } else {
                    self.aiming = Some((id, level));
                }
            }
            Identified(index) => {
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.identified = true;
                    let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, true));
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Identified: {name}."));
                }
            }
            ArrowEquipped(index) => {
                for it in self.inventory.items.values_mut() {
                    it.equipped &= !0x8000;
                }
                if let Some(it) = self.inventory.items.get_mut(&index) {
                    it.equipped |= 0x8000;
                }
            }
            Mvp(id) => {
                if let Some(pos) = self.unit_pos(id) {
                    self.play_fx("mvp", pos);
                }
                let who = if id == self.account_id { self.hud.status.name.clone() } else { self.units.get(&id).map_or_else(|| "Someone".into(), |m| m.name.clone()) };
                self.hud.push_chat(ro_net::ChatKind::Broadcast, format!("{who} is the MVP!"));
            }
            MvpItem(item) => {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
                self.hud.push_chat(ro_net::ChatKind::System, format!("MVP reward: {name}."));
            }
            MvpExp(exp) => self.hud.push_chat(ro_net::ChatKind::System, format!("MVP reward: {exp} experience.")),
            MvpDropped => self.hud.push_chat(ro_net::ChatKind::System, "You are the MVP, but the reward dropped to the floor because you are overweight.".into()),
            MemberPos { id, x, y, guild } => {
                if x == 0xffff || y == 0xffff {
                    self.member_pos.remove(&id);
                } else {
                    self.member_pos.insert(id, ((x as f32, y as f32), guild));
                }
            }
            EffectValue { id, effect, num } => {
                /// rAthena's EF_GREEN_NUMBER: the number is a heal.
                const EF_GREEN_NUMBER: u32 = 657;
                if effect == EF_GREEN_NUMBER {
                    if let Some(pos) = self.unit_pos(id) {
                        self.floaters.push((pos, num.to_string(), egui::Color32::from_rgb(110, 255, 110), 0.0));
                    }
                } else {
                    self.handle(ScriptEffect { id, effect });
                }
            }
            SpecialPopup(id) => {
                let popup = self.vfs.as_ref().and_then(|v| v.read(r"System\spopup.lub").ok().flatten()).and_then(|d| ro_formats::lub::eval(&d).ok()).and_then(|c| {
                    let t = c.globals.get("popuptbl")?.clone();
                    let e = &c.entries(&t).iter().find(|(k, _)| k.as_number() == Some(id as f64))?.1;
                    let ui = c.field(e, "uitype").and_then(|v| v.as_number()).unwrap_or(0.0) as u8;
                    let rgb = c.field(e, "textcolor").and_then(|v| v.as_number()).unwrap_or(16763904.0) as u32;
                    let lines: Vec<String> = c.field(e, "Description").map_or(vec![], |d| c.entries(d).iter().filter_map(|(_, v)| v.as_str().map(str::to_string)).collect());
                    Some((ui, egui::Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8), lines))
                });
                match popup {
                    Some((ui, color, lines)) => {
                        // uitype: 0 chat, 1 floating, 2 both.
                        if ui != 1 {
                            for l in &lines {
                                self.hud.push_chat(ro_net::ChatKind::System, l.clone());
                            }
                        }
                        if ui != 0 {
                            self.hud.banner = Some((lines, color, std::time::Instant::now()));
                        }
                    }
                    None => tracing::debug!(id, "special popup not in spopup.lub"),
                }
            }
            SkillMsg(id) => {
                let text = match id {
                    0x15 => "All abnormal status effects have been removed.",
                    0x16 => "You will be immune to abnormal status effects for the next minute.",
                    0x17 => "Your Max HP will stay increased for the next minute.",
                    0x18 => "Your Max SP will stay increased for the next minute.",
                    0x19 => "All of your stats will stay increased for the next minute.",
                    0x1c => "Your weapon will remain blessed with Holy power for the next minute.",
                    0x1d => "Your armor will remain blessed with Holy power for the next minute.",
                    0x1e => "Your Defense will stay increased for the next 10 seconds.",
                    0x1f => "Your Attack strength will stay increased for the next minute.",
                    0x20 => "Your Accuracy and Flee Rate will stay increased for the next minute.",
                    0x28 => "Full Strip failed because of coating.",
                    _ => "",
                };
                if !text.is_empty() {
                    self.hud.push_chat(ro_net::ChatKind::System, text.into());
                }
            }
            ScriptEffect { id, effect } => {
                if let Some(pos) = self.unit_pos(id) {
                    match (fx::script_effect(effect), fx::ef_skill(effect)) {
                        (Some(name), _) => self.play_fx(name, pos),
                        (None, Some(skill)) => {
                            tracing::debug!(effect, skill, "script effect via skill");
                            self.skill_fx(skill, Some(pos), false);
                        }
                        _ => tracing::debug!(effect, "no visual for script effect"),
                    }
                }
            }
            UnitEffect { id, effect } => {
                // Base or job level up.
                if id == self.account_id && effect <= 1 {
                    self.feel(haptics::Cue::LevelUp);
                }
                if let (Some(name), Some(pos)) = (fx::misc_effect(effect), self.unit_pos(id)) {
                    self.play_fx(name, pos);
                }
            }
            MonsterHp { id, hp, max_hp } => {
                if let Some(m) = self.units.get_mut(&id) {
                    m.hp = Some((hp, max_hp));
                }
            }
            MapMove { map, x, y } => {
                // Respawning at the save point arrives as a map move.
                self.viewpoints.clear();
                self.member_pos.clear();
                self.revive();
                self.units.clear();
                self.devotions.clear();
                self.ground_units.clear();
                self.traps.clear();
                self.graffiti.clear();
                self.floor.clear();
                self.warps.clear();
                if self.world.as_ref().is_none_or(|w| w.name != map) {
                    self.enter_map(&map);
                }
                if let Some(p) = self.player.as_mut() {
                    p.walker.stop_at((x as i32, y as i32));
                }
            }
            // Back at character select: clear the world; the screens take over again.
            LeftGame => {
                self.leave_world();
                let user = self.settings.last_id.clone();
                let mut front = screens::Front::new(user, true);
                front.screen = screens::Screen::Loading;
                self.front = Some(front);
            }
            // Login and character-select replies (handled by the screens above).
            LoginFailed(_) | Servers(_) | Characters { .. } | CharCreated(_) | CharCreateFailed(_) | CharDeleteStarted { .. } | CharDeleteCanceled { .. } | CharDeleted { .. } | CharNameOk(_) | CharRenamed(_) | CharMoved { .. } | CharsBlocked(_) | Pin(_) => {}
            // Leaving the game: the connection closing is expected.
            Disconnected(reason) if self.quit_asked.is_some() => tracing::info!("disconnected: {reason}"),
            // Lost in game (server restart, dropped network): as the original, "Disconnected
            // from Server." and back to the login screen.
            Disconnected(reason) => {
                tracing::error!("disconnected: {reason}");
                self.leave_world();
                let user = self.settings.last_id.clone();
                let mut front = screens::Front::new(user.clone(), !user.is_empty());
                front.event(&LoginFailed(msgstr::get(2, &[]).unwrap_or_else(|| "Disconnected from Server.".into())));
                self.front = Some(front);
                self.net = Some(ro_net::spawn_session(self.session.clone()));
            }
        }
    }

    /// Out of the game world (to character select or the login screen): what belonged to the
    /// map and character goes, and the title music plays.
    fn leave_world(&mut self) {
        self.leave_character();
        self.ping = None;
        self.units.clear();
        self.devotions.clear();
        self.ground_units.clear();
        self.traps.clear();
        self.graffiti.clear();
        self.floor.clear();
        self.warps.clear();
        self.player = None;
        self.walk_speed = None;
        self.world = None;
        self.walk_goal = None;
        self.attack = None;
        self.dialog = None;
        self.esc_menu = false;
        if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
            a.title(v);
        }
    }

    /// Back at character select: what belonged to the character goes, so the next one starts
    /// clean. The server sends a character's guild, party, clan, pets and the like only when
    /// it has one; kept over, the last character's would show as this one's. Loaded game
    /// tables and the player's settings and window layout stay.
    fn leave_character(&mut self) {
        self.guild = Default::default();
        self.social = Default::default();
        self.companions = Default::default();
        self.merchant = Default::default();
        self.mailbox = Default::default();
        self.clan = Default::default();
        self.memorial = Default::default();
        self.quests.leave_character();
        self.progress.leave_character();
        self.buffs.active.clear();
        self.reputation.points.clear();
        self.inventory = Default::default();
        self.stats = None;
        self.skills.clear();
        self.hotkeys.clear();
        self.hotkey_row = 0;
        self.cooldowns.clear();
        self.casts.clear();
        self.unit_progress.clear();
        self.script_progress = None;
        self.member_pos.clear();
        self.spirits.clear();
        self.viewpoints.clear();
        self.aggro.clear();
        self.homun_ai = None;
        self.merc_ai = None;
        self.pad_target = None;
        self.pending_npc = None;
        self.pending_skill = None;
        self.pending_pickup = None;
        self.aiming = None;
        self.cutin = None;
        self.pvp_rank = None;
        self.dead = false;
        self.trade_partner = None;
        self.trade_stage = 0;
        self.trade_offers.clear();
        self.player_menu = None;
        self.attack_range = 1;
    }

    fn send(&mut self, cmd: ro_net::Command) {
        // Remember how much of each item we offer in a trade (the ack only names the slot).
        if let ro_net::Command::Trade(ro_net::TradeRequest::Add { index, amount }) = &cmd {
            self.trade_offers.insert(*index, *amount);
        }
        // The server answers an invitation only when it reaches someone; to a name that is
        // not online it says nothing, so say what was sent.
        if let ro_net::Command::Guild(ro_net::GuildRequest::Invite { name }) = &cmd {
            self.hud.push_chat(ro_net::ChatKind::Guild, format!("Guild invitation sent to {name}. They need to be online to answer it."));
        }
        if let Some((tx, _)) = self.net.as_ref() {
            let _ = tx.send(cmd);
        }
    }

    /// Turns a window action into a server request.
    /// A key pressed over the game (not taken by the UI).
    fn key_pressed(&mut self, logical_key: winit::keyboard::Key) {
        use winit::keyboard::{Key, NamedKey};
        match &logical_key {
            // Esc cancels aiming; otherwise it opens the game menu, as in the original.
            Key::Named(NamedKey::Escape) => {
                if self.aiming.take().is_none() && self.dialog.is_none() && !self.dead {
                    self.esc_menu = !self.esc_menu;
                }
            }
            // Shortcut bar: F1-F9 use the first row on show; F12 shows one to four
            // rows or hides the bar.
            // F11: full screen on or off (borderless, at the desktop's resolution).
            // F10: the performance overlay on or off.
            Key::Named(NamedKey::F10) => {
                self.settings.perf_overlay = !self.settings.perf_overlay;
                self.settings.save();
            }
            Key::Named(NamedKey::F11) => {
                self.settings.fullscreen = !self.settings.fullscreen;
                self.settings.save();
            }
            Key::Named(NamedKey::F12) => {
                self.settings.hotkey_rows = (self.settings.hotkey_rows + 1) % (HOTKEY_ROWS + 1);
                self.settings.save();
            }
            Key::Named(k) if hotkey_slot(*k).is_some() => {
                let slot = self.hotkey_row as usize * HOTKEYS_PER_ROW + hotkey_slot(*k).unwrap_or(0);
                self.press_hotkey(slot, logical_key.clone());
            }
            Key::Character(c) if self.alt => self.alt_key(&c.to_ascii_lowercase()),
            // Insert: sit down or stand up, as in the original.
            Key::Named(NamedKey::Insert) => {
                let sitting = self.player.as_ref().is_some_and(|p| p.sitting);
                self.send(ro_net::Command::Sit(!sitting));
            }
            // Scroll Lock also takes a screenshot, as in the original.
            Key::Named(NamedKey::ScrollLock) => self.screenshot = true,
            // Ctrl+Tab: the minimap on or off.
            Key::Named(NamedKey::Tab) if self.ctrl => {
                self.settings.minimap = !self.settings.minimap;
                self.settings.save();
            }
            Key::Character(_) if self.ctrl => {}
            // Battle mode: Space opens the chat, letters use the shortcut rows.
            Key::Named(NamedKey::Space) if self.settings.battle_mode && self.settings.battle_space_chat => self.hud.focus_chat(),
            Key::Named(NamedKey::Space) if self.settings.battle_mode => {
                if let Some(slot) = battle_slot(" ", self.hotkey_row, &self.settings.battle_keys) {
                    self.press_hotkey(slot, logical_key.clone());
                }
            }
            Key::Character(c) if self.settings.battle_mode => {
                if let Some(slot) = battle_slot(c, self.hotkey_row, &self.settings.battle_keys) {
                    self.press_hotkey(slot, logical_key.clone());
                }
            }
            // Otherwise typing goes to the chat, as in the original.
            Key::Character(c) => self.hud.type_into_chat(c),
            _ => {}
        }
    }

    /// A shortcut key pressed: uses its slot. Rolling Cutter repeats for as long as the key
    /// is held, as in the original.
    fn press_hotkey(&mut self, slot: usize, held: winit::keyboard::Key) {
        let key = self.hotkeys.get(slot).copied().filter(|h| h.skill && h.id as u16 == HOLD_SKILL);
        let learned = key.and_then(|h| self.skills.iter().find(|s| s.id == HOLD_SKILL).map(|s| (h.count.max(1) as u16).min(s.level.max(1))));
        match learned {
            Some(level) if self.held_skill.is_none() => {
                self.send(ro_net::Command::Stat(ro_net::StatRequest::HoldSkill { skill: HOLD_SKILL, level, target: self.account_id }));
                self.held_skill = Some((HOLD_SKILL, held));
            }
            Some(_) => {}
            None => self.queued_action = Some(windows::UiAction::UseHotkey(slot)),
        }
    }

    /// Lets go of a held skill (CZ_USE_SKILL_END).
    fn release_held_skill(&mut self) {
        if let Some((skill, _)) = self.held_skill.take() {
            self.send(ro_net::Command::Stat(ro_net::StatRequest::ReleaseSkill(skill)));
        }
    }

    /// Leaves the game, asking the map server first as the original does: it refuses within
    /// 10 seconds of combat (prevent_logout).
    fn request_exit(&mut self) {
        if self.net.is_some() {
            self.queued_commands.push(ro_net::Command::Quit);
            self.quit_asked = Some(std::time::Instant::now());
        } else {
            self.quit = true;
        }
    }

    /// Fetches a guild's emblem unless this version was already asked for: from the web
    /// server, or the map server when there is no web login.
    fn want_emblem(&mut self, guild: u32, version: u32) {
        if guild == 0 || (self.emblem_versions.get(&guild) == Some(&version) && self.emblems.contains_key(&guild)) {
            return;
        }
        self.emblem_versions.insert(guild, version);
        self.emblems.entry(guild).or_insert(None);
        if !self.emblem_web.download(guild, version) {
            self.send(ro_net::Command::Guild(ro_net::GuildRequest::Emblem(guild)));
        }
    }

    /// An emblem image file arrived: textures for name labels and, for ours, the guild window.
    fn show_emblem(&mut self, guild: u32, version: u32, raw: &[u8]) {
        let Some(img) = emblem::decode(raw) else {
            tracing::debug!(guild, version, "undecodable emblem");
            return;
        };
        let ctx = self.egui_ctx.clone();
        // Labels draw it at 18 points; twice that keeps it sharp on high-DPI screens.
        self.emblems.insert(guild, Some(emblem::texture(&ctx, format!("emblem label {guild} {version}"), &img, 36)));
        self.guild.set_emblem(&ctx, guild, version, &img);
    }

    /// Web emblem results, and the guild master's queued upload.
    fn update_emblems(&mut self) {
        for done in self.emblem_web.poll() {
            match done {
                emblem::Done::Downloaded { guild, version, data } => self.show_emblem(guild, version, &data),
                // Emblems from before the web server, or no web server: the map server has them.
                emblem::Done::Missing { guild } => self.send(ro_net::Command::Guild(ro_net::GuildRequest::Emblem(guild))),
                // The map server tells everyone nearby the new version (ZC_CHANGE_GUILD).
                emblem::Done::Uploaded { guild, version } => {
                    self.guild.emblem_status = "Emblem uploaded.".into();
                    self.send(ro_net::Command::Guild(ro_net::GuildRequest::NewEmblem { guild, version }));
                }
                emblem::Done::UploadFailed(e) => self.guild.emblem_status = e,
            }
        }
        if let Some((data, kind)) = self.guild.pending_upload.take() {
            if !self.emblem_web.upload(self.guild.id, data.clone(), kind) {
                match guild::packet_emblem(&data) {
                    Some(c) => self.send(c),
                    None => self.guild.emblem_status = "Without the web server, the emblem must be a 24x24 BMP.".into(),
                }
            }
        }
    }

    fn ui_action(&mut self, a: windows::UiAction) {
        use ro_net::{Command, ItemRequest, StatRequest};
        use windows::UiAction::*;
        match a {
            UseItem(index) => {
                // Book items open locally, as the original (no use request is sent).
                match self.inventory.items.get(&index).map(|i| i.id).filter(|id| self.books.is_book(*id)) {
                    Some(id) => self.open_book(id, 1),
                    None => self.send(Command::Item(ItemRequest::Use { index, account_id: self.account_id })),
                }
            }
            Equip { index, location } => self.send(Command::Item(ItemRequest::Equip { index, location })),
            Unequip(index) => self.send(Command::Item(ItemRequest::Unequip { index })),
            UnequipAll(location) => self.send(Command::Item(ItemRequest::UnequipAll { location })),
            Favorite { index, on } => self.send(Command::Item(ItemRequest::Favorite { index, on })),
            SwitchAdd { index, location } => self.send(Command::Switch(ro_net::SwitchRequest::Add { index, location })),
            SwitchRemove(index) => self.send(Command::Switch(ro_net::SwitchRequest::Remove(index))),
            SwitchAll => self.send(Command::Switch(ro_net::SwitchRequest::All)),
            Drop { index, amount } => self.send(Command::Item(ItemRequest::Drop { index, amount })),
            Identify(index) => self.send(Command::Item(ItemRequest::Identify { index })),
            RaiseStat(i) => self.send(Command::Stat(StatRequest::Raise(13 + i as u16))),
            RaiseTrait(i) => self.send(Command::Stat(StatRequest::RaiseTrait(219 + i as u16))),
            CardTargets(card) => {
                self.windows.card_pick = Some((card, vec![]));
                self.send(Command::Item(ItemRequest::CardTargets { card }));
            }
            InsertCard { card, equip } => self.send(Command::Item(ItemRequest::InsertCard { card, equip })),
            Menu(reply) => self.send(Command::Menu(reply)),
            ShowEquip(on) => self.send(Command::ShowEquip(on)),
            StarPlace(which) => self.send(Command::StarPlace(which)),
            Adopt(r) => self.send(Command::Adopt(r)),
            SkillText { skill, level, cell, text } => {
                self.send(Command::Stat(ro_net::StatRequest::UseSkillAtWithText { skill, level, x: cell.0, y: cell.1, text }));
            }
            ExpandInventory =>
 self.send(Command::Item(ItemRequest::ExpandAsk)),
            ExpandConfirm(yes) => {
                self.windows.expand_ask = None;
                self.send(Command::Item(ItemRequest::ExpandConfirm(yes)));
            }
            OpenBank => {
                if self.merchant.bank.is_none() {
                    self.toggle_bank();
                }
            }
            ItemInfo { id, refine, cards } => {
                if let Some(vfs) = self.vfs.as_ref() {
                    self.item_info.show_item(&self.egui_ctx, vfs, self.item_db.as_ref(), id, refine, cards);
                }
            }
            CartPut { index, amount } => self.send(Command::Merchant(ro_net::MerchantRequest::CartPut { index, amount: amount as u32 })),
            SkillUp(id) => self.send(Command::Stat(StatRequest::SkillUp(id))),
            UseSkill(id) => {
                let level = self.skills.iter().find(|s| s.id == id).map_or(0, |s| s.level);
                self.start_skill(id, level);
            }
            UseHotkey(slot) => {
                let Some(key) = self.hotkeys.get(slot).copied().filter(|k| k.id != 0) else { return };
                if key.skill {
                    self.start_skill(key.id as u16, key.count.max(1) as u16);
                } else if let Some(it) = self.inventory.items.values().find(|it| it.id == key.id && it.equipped == 0) {
                    let (index, equip, location) = (it.index, it.is_equipment(), it.location);
                    self.ui_action(if equip { Equip { index, location } } else { UseItem(index) });
                }
            }
            SetHotkey { index, key } => {
                if let Some(slot) = self.hotkeys.get_mut(index as usize) {
                    *slot = key;
                }
                self.send(Command::Stat(StatRequest::SetHotkey { index, key }));
            }
            HotkeyRow(row) => {
                self.hotkey_row = row;
                self.send(Command::Stat(StatRequest::HotkeyRow(row)));
            }
            HotkeyRows(rows) => {
                self.settings.hotkey_rows = rows.min(HOTKEY_ROWS);
                self.settings.save();
            }
        }
    }

    /// Uses a skill at `level`: self skills go off at once, targeted ones wait for a click.
    fn start_skill(&mut self, id: u16, level: u16) {
        let Some(s) = self.skills.iter().find(|s| s.id == id) else { return };
        let level = level.clamp(1, s.level.max(1));
        if s.target & 4 != 0 {
            self.use_skill_on(id, level, self.account_id);
        } else if s.target & (1 | 2 | 16 | 32) != 0 {
            self.aiming = Some((id, level));
        }
    }

    fn use_skill_on(&mut self, skill: u16, level: u16, target: u32) {
        self.attack = None;
        // Out of reach: walk into range first, then cast (as the original client does).
        let range = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| (s.range as i32).max(1));
        let cells = self.player.as_ref().map(|p| p.walker.cell).zip(self.units.get(&target).map(|m| m.walker.cell));
        if let (Some((a, b)), true) = (cells, target != self.account_id) {
            if (a.0 - b.0).abs().max((a.1 - b.1).abs()) > range {
                let tries = self.pending_skill.filter(|p| p.2 == target).map_or(0, |p| p.3 + 1);
                if tries > 3 {
                    self.pending_skill = None;
                    return;
                }
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                let reach = range.min(3);
                let spot = (-reach..=reach)
                    .flat_map(|dy| (-reach..=reach).map(move |dx| (b.0 + dx, b.1 + dy)))
                    .filter(|&(x, y)| gat.is_some_and(|g| g.walkable(x, y)))
                    .min_by_key(|&(x, y)| (x - a.0).abs().max((y - a.1).abs()));
                if let Some((x, y)) = spot {
                    self.pending_skill = Some((skill, level, target, tries));
                    self.request_walk((x as u16, y as u16));
                }
                return;
            }
        }
        self.pending_skill = None;
        self.walk_goal = None;
        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkill { skill, level, target }));
    }

    fn use_skill_at(&mut self, skill: u16, level: u16, cell: (u16, u16)) {
        self.walk_goal = None;
        self.attack = None;
        // RG_GRAFFITI and HT_TALKIEBOX ask for their message first.
        if matches!(skill, 125 | 220) {
            self.windows.skill_text = Some((skill, level, cell, String::new()));
            return;
        }
        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkillAt { skill, level, x: cell.0, y: cell.1 }));
    }

    /// Picks an item up, walking next to it first when it is out of reach.
    fn pick_up(&mut self, id: u32) {
        let (Some(p), Some((f, _))) = (self.player.as_ref(), self.floor.get(&id)) else { return };
        let (a, b) = (p.walker.cell, (f.x as i32, f.y as i32));
        self.attack = None;
        if (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= 1 {
            self.pending_pickup = None;
            tracing::debug!(id, "pickup request");
            self.send(ro_net::Command::Item(ro_net::ItemRequest::PickUp { floor_id: id }));
        } else {
            self.pending_pickup = Some(id);
            self.request_walk((f.x, f.y));
        }
    }

    /// The floor item under the cursor.
    fn pick_floor_item(&self) -> Option<u32> {
        let (r, (cx, cy)) = (self.renderer.as_ref()?, self.cursor?);
        let w = self.world.as_ref()?;
        let size = r.size();
        self.floor
            .iter()
            .filter_map(|(&id, (f, _))| {
                let (fx, fy) = floor_cell(f);
                let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                let p = r.project(glam::Vec3::new(x, w.floor_at(x, z), z), size)?;
                let d = (p.x - cx as f32).hypot(p.y - 8.0 - cy as f32);
                (d < 22.0).then_some((id, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Ground sprite for an item (sprite\아이템\{resource}).
    fn item_actor(&mut self, item: u32, identified: bool) -> Option<ro_render::Actor> {
        let info = self.item_db.as_ref()?.get(item)?;
        let res = if identified || info.unidentified_resource.is_empty() { info.resource.clone() } else { info.unidentified_resource.clone() };
        let key = format!("item:{res}");
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_ref()?);
        let base = format!("data\\sprite\\아이템\\{res}");
        let spr = vfs.read(&format!("{base}.spr")).ok().flatten().and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
        let act = vfs.read(&format!("{base}.act")).ok().flatten().and_then(|d| ro_formats::act::Act::parse(&d).ok());
        let actor = match (spr, act) {
            (Some(spr), Some(act)) => {
                let body = r.add_sheet(&spr, act, None);
                Some(ro_render::Actor { pos: Default::default(), body, head: None, headgear: vec![], weapon: vec![], shadow: None, action: 0, dir: ro_render::Dir::S, time_ms: 0.0, once: false, alpha: 1.0, scale: 1.0, tint: [1.0; 3] })
            }
            _ => None,
        };
        self.sprites.insert(key, actor.clone());
        actor
    }

    fn load_shop_icons(&mut self) {
        let ids: Vec<u32> = match &self.social.shop {
            Some(social::Shop::Buy { items, .. } | social::Shop::Cash { items, .. }) => items.iter().map(|i| i.id).collect(),
            _ => vec![],
        };
        self.load_icons_for(&ids);
    }

    /// Loads icons for specific item ids (shops, trades, storage).
    fn load_icons_for(&mut self, ids: &[u32]) {
        let (Some(db), Some(vfs)) = (self.item_db.as_ref(), self.vfs.as_ref()) else { return };
        for &id in ids {
            if self.icons.contains_key(&id) || !self.icons_tried.insert(id) {
                continue;
            }
            let Some(info) = db.get(id) else { continue };
            let path = format!("data\\texture\\유저인터페이스\\item\\{}.bmp", info.resource);
            if let Some(tex) = load_texture(&self.egui_ctx, vfs, &path) {
                self.icons.insert(id, tex);
            }
        }
    }

    /// Loads inventory icons not yet loaded into egui textures: an item's own icon, and for
    /// unidentified ones their unidentified look as well (kept apart, so identifying one swaps
    /// the icon).
    fn load_icons(&mut self) {
        let ids: Vec<u32> = self.inventory.items.values().map(|i| i.id).collect();
        self.load_icons_for(&ids);
        let (Some(db), Some(vfs)) = (self.item_db.as_ref(), self.vfs.as_ref()) else { return };
        let unid: Vec<u32> = self.inventory.items.values().filter(|i| !i.identified).map(|i| i.id).collect();
        for id in unid {
            if self.unid_icons.contains_key(&id) || !self.unid_tried.insert(id) {
                continue;
            }
            let Some(res) = db.get(id).map(|i| &i.unidentified_resource).filter(|r| !r.is_empty()) else { continue };
            let path = format!("data\\texture\\유저인터페이스\\item\\{res}.bmp");
            if let Some(tex) = load_texture(&self.egui_ctx, vfs, &path) {
                self.unid_icons.insert(id, tex);
            }
        }
    }

    fn load_skill_icons(&mut self) {
        let Some(vfs) = self.vfs.as_ref() else { return };
        // Learned skills, and the rest of the job's tree (drawn greyed out).
        let mut wanted: Vec<(u16, String)> = self.skills.iter().map(|s| (s.id, s.name.clone())).collect();
        for (_, grid) in self.skill_tree.tabs(self.hud.status.job_id) {
            wanted.extend(grid.iter().filter_map(|(_, n)| Some((self.skill_names.id_of(n)?, n.clone()))));
        }
        for (id, name) in wanted {
            if self.skill_icons.contains_key(&id) {
                continue;
            }
            let path = format!("data\\texture\\유저인터페이스\\item\\{}.bmp", name.to_ascii_lowercase());
            let tex = load_texture(&self.egui_ctx, vfs, &path).or_else(|| {
                let alias = samurai::icon(id)?;
                load_texture(&self.egui_ctx, vfs, &format!("data\\texture\\유저인터페이스\\item\\{alias}.bmp"))
            });
            if let Some(tex) = tex {
                self.skill_icons.insert(id, tex);
            }
        }
    }

    /// The player or a unit by id.
    /// Left button held on the ground: re-aim the walk at the cursor every few frames, as
    /// the original client does when you hold the button and point.
    fn steer_to_cursor(&mut self, dt: f32) {
        if let Some((off, left)) = self.hold_walk.as_mut() {
            *left -= dt;
            let (off, done) = (*off, *left <= 0.0);
            if done {
                self.hold_walk = None;
                return;
            }
            self.mouse_walk_ms -= dt;
            if self.mouse_walk_ms <= 0.0 {
                self.mouse_walk_ms = 150.0;
                if let Some(c) = self.player.as_ref().map(|p| p.walker.cell) {
                    self.steer_toward(((c.0 + off.0).max(0) as u16, (c.1 + off.1).max(0) as u16));
                }
            }
            return;
        }
        if !self.mouse_walk {
            return;
        }
        if !self.left_down || self.dead || self.aiming.is_some() {
            self.mouse_walk = false;
            return;
        }
        self.mouse_walk_ms -= dt;
        if self.mouse_walk_ms > 0.0 {
            return;
        }
        self.mouse_walk_ms = 150.0;
        if self.egui_ctx.is_pointer_over_egui() {
            return;
        }
        if let Some(cell) = self.pick() {
            self.steer_toward(cell);
        }
    }

    /// Hold-to-walk toward a cell. Re-planning every time the aimed-at cell moves (it moves
    /// with us, the camera following) makes each new path start with the same step, so a
    /// heading between straight and diagonal drifts to the diagonal, and every re-plan risks
    /// a stutter. So the walk is re-aimed only when it is nearly done or when the aim turns
    /// (the mouse moves, the camera turns): paths run their length, straight and diagonal
    /// steps both, and the heading follows the cursor.
    fn steer_toward(&mut self, cell: (u16, u16)) {
        let Some(p) = self.player.as_ref() else { return };
        if self.walk_goal.as_ref().is_some_and(|g| g.cell == cell) {
            return;
        }
        let here = p.walker.cell;
        let goal = self.walk_goal.as_ref().map_or_else(|| p.walker.goal_or_cell(), |g| (g.cell.0 as i32, g.cell.1 as i32));
        let aim = glam::Vec2::new((cell.0 as i32 - here.0) as f32, (cell.1 as i32 - here.1) as f32);
        let going = glam::Vec2::new((goal.0 - here.0) as f32, (goal.1 - here.1) as f32);
        let turned = aim.length() > 0.5 && going.length() > 0.5 && aim.angle_to(going).abs() > 25f32.to_radians();
        if !p.walker.is_walking() || p.walker.remaining() <= 2 || turned {
            self.request_walk(cell);
        }
    }

    /// Indoors: recompute which room we are in when we step outside the current one (map
    /// entry, warps within the map) and hand the renderer its mask.
    fn update_room(&mut self) {
        let (Some(w), Some(p)) = (self.world.as_mut(), self.player.as_ref()) else { return };
        if !w.indoor() || (w.room.is_some() && w.shows_cell(p.walker.cell.0, p.walker.cell.1)) {
            return;
        }
        let Some(room) = w.room_of(p.walker.cell) else { return };
        if let (Some(r), Some(g)) = (self.renderer.as_mut(), w.gat.as_ref()) {
            r.set_room_mask(Some((g.width, g.height, &room)));
        }
        w.room = Some(room);
    }

    /// Touch screens (Android): one finger is the left mouse button on the world (tap to walk,
    /// talk or attack; hold to steer), two fingers turn the camera and pinch to zoom. Touches
    /// the UI takes never get here.
    fn touch_input(&mut self, el: &ActiveEventLoop, id: WindowId, t: winit::event::Touch) {
        use winit::event::TouchPhase;
        let pos = (t.location.x, t.location.y);
        let device_id = winit::event::DeviceId::dummy();
        let button = |pressed: bool| WindowEvent::MouseInput { device_id, state: if pressed { ElementState::Pressed } else { ElementState::Released }, button: MouseButton::Left };
        let moved = WindowEvent::CursorMoved { device_id, position: t.location };
        let before = self.fingers.clone();
        match t.phase {
            TouchPhase::Started => {
                self.fingers.push((t.id, pos));
                match self.fingers.len() {
                    1 => {
                        self.window_event(el, id, moved);
                        self.window_event(el, id, button(true));
                    }
                    // A second finger: a camera gesture, so the first stops steering.
                    2 => self.window_event(el, id, button(false)),
                    _ => {}
                }
            }
            TouchPhase::Moved => {
                if let Some(f) = self.fingers.iter_mut().find(|f| f.0 == t.id) {
                    f.1 = pos;
                }
                match (&before[..], &self.fingers[..]) {
                    ([one], _) if one.0 == t.id => self.window_event(el, id, moved),
                    ([a0, b0], [a1, b1]) => {
                        let mid = |a: (f64, f64), b: (f64, f64)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                        let span = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
                        let (m0, m1) = (mid(a0.1, b0.1), mid(a1.1, b1.1));
                        // Arranging the interface: the pinch resizes the selected element.
                        if self.arrange.open {
                            let (s0, s1) = (span(a0.1, b0.1), span(a1.1, b1.1));
                            if s0 > 1.0 {
                                self.arrange.pinch *= (s1 / s0) as f32;
                            }
                        } else if let Some(r) = self.renderer.as_mut() {
                            let s = 0.005 / r.scale_factor as f32;
                            r.camera.rotate((m1.0 - m0.0) as f32 * s, (m1.1 - m0.1) as f32 * s);
                            r.camera.zoom(((span(a1.1, b1.1) - span(a0.1, b0.1)) / (60.0 * r.scale_factor)) as f32);
                        }
                    }
                    _ => {}
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if !self.fingers.iter().any(|f| f.0 == t.id) {
                    return;
                }
                let last = self.fingers.len() == 1;
                self.fingers.retain(|f| f.0 != t.id);
                if last {
                    self.window_event(el, id, moved);
                    self.window_event(el, id, button(false));
                }
            }
        }
    }

    /// Right button: held and dragged it turns the camera; clicked on another player it
    /// opens their menu, as in the original.
    fn right_button(&mut self, pressed: bool) {
        let released = self.rotating && !pressed;
        self.rotating = pressed;
        if pressed {
            self.right_drag = 0.0;
        }
        if released && self.right_drag < 6.0 {
            if let Some(id) = self.pick_unit().filter(|id| self.units.get(id).is_some_and(|m| m.kind == 0)) {
                self.open_player_menu(id);
            }
        }
    }

    /// Opens the menu for another player (trade, party, friend; GM actions) at the cursor.
    fn open_player_menu(&mut self, id: u32) {
        let name = self.units.get(&id).map(|m| m.name.clone()).unwrap_or_default();
        let ppp = self.egui_ctx.pixels_per_point();
        let pos = self.cursor.map_or(egui::Pos2::ZERO, |(x, y)| egui::pos2(x as f32 / ppp, y as f32 / ppp));
        self.player_menu = Some((id, name, pos));
    }

    /// The cursor for what is under the mouse, as the original client shows it.
    fn hover_cursor(&self) -> usize {
        use hud::cursor::*;
        if self.rotating {
            return ROTATE;
        }
        if self.aiming.is_some() || self.capturing {
            return TARGET;
        }
        if self.cursor.is_none() || self.egui_ctx.is_pointer_over_egui() {
            return ARROW;
        }
        match self.pick_unit().and_then(|id| self.units.get(&id)).map(|m| m.kind) {
            Some(5) => ATTACK,
            Some(0) if self.pick_unit().is_some_and(|id| self.is_enemy_player(id)) => ATTACK,
            Some(k) if is_npc(k) => TALK,
            _ if self.pick_floor_item().is_some() => PICK_UP,
            _ => ARROW,
        }
    }

    /// On a PvP / GvG map, a player outside our party (PvP) or guild (GvG).
    fn is_enemy_player(&self, id: u32) -> bool {
        let Some(m) = self.units.get(&id) else { return false };
        let pvp = self.map_flags & 1 != 0;
        let gvg = self.map_flags & 2 != 0;
        let in_party = self.social.party.iter().flat_map(|p| p.members.iter()).any(|pm| pm.name == m.name);
        let in_guild = self.guild.id != 0 && m.guild == self.guild.id;
        // DISABLE_LOCKON: players are only attacked while holding Shift.
        let lock_ok = self.map_flags & 16 == 0 || self.egui_ctx.input(|i| i.modifiers.shift);
        lock_ok && ((pvp && !in_party) || (gvg && !in_guild))
    }

    /// Back on our feet: respawned at the save point or resurrected.
    fn revive(&mut self) {
        if !std::mem::take(&mut self.dead) {
            return;
        }
        if let Some(p) = self.player.as_mut() {
            p.pose = None;
            p.actor.time_ms = 0.0;
        }
    }

    /// Where a unit stands (its feet), if we can see it.
    fn unit_pos(&self, id: u32) -> Option<glam::Vec3> {
        if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }.map(|m| m.actor.pos)
    }

    /// The ground point at the centre of a cell.
    fn cell_pos(&self, cell: (u16, u16)) -> Option<glam::Vec3> {
        cell_pos_in(self.world.as_ref()?, cell)
    }

    /// Plays a skill's effect at `pos`, if it has one.
    /// Plays a skill's effect at `pos`: its known visual, else an STR named after the skill
    /// (AS_SONICBLOW -> sonicblow.str), else a generic hit (damage) or light (support).
    /// Weapon sound family of a player unit (None for monsters and NPCs).
    fn weapon_sound(&self, id: u32) -> Option<&'static str> {
        let m = if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }?;
        let look = m.look.as_ref()?;
        let view = self.item_db.as_ref().and_then(|d| d.get(look.gear.weapon)).map_or(look.gear.weapon, |i| i.view);
        Some(audio::weapon_kind(self.look_tables.weapon(view).as_deref().filter(|_| look.gear.weapon != 0)))
    }

    fn play_sound(&self, name: &str) {
        if let (Some(a), Some(vfs)) = (self.audio.as_ref(), self.vfs.as_ref()) {
            a.play(vfs, name);
        }
    }

    fn skill_fx(&mut self, skill: u16, pos: Option<glam::Vec3>, damaging: bool) {
        // The skill's own sound, when the client has one.
        if let (Some(a), Some(vfs), Some(name)) = (self.audio.as_ref(), self.vfs.as_ref(), self.skill_names.internal(skill)) {
            match a.skill_sound(name) {
                Some(file) => a.play(vfs, &file),
                None => tracing::debug!(skill, name, "no sound for skill"),
            }
        }
        let Some(pos) = pos else { return };
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        match fx::skill_visual(skill) {
            Some(fx::Visual::Str(name)) => {
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            Some(fx::Visual::Column(tex, color)) => {
                if let Some(tex) = self.fx.texture(r, vfs, tex) {
                    let shape = ro_render::Shape::Column { radius: 0.9, height: 3.5 };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms: 1200.0 });
                }
            }
            Some(fx::Visual::Sprite(name)) => {
                if let Some(body) = self.fx.sprite(r, vfs, name) {
                    let actor = ro_render::Actor {
                        pos,
                        body,
                        head: None,
                        headgear: vec![],
                        weapon: vec![],
                        shadow: None,
                        action: 0,
                        dir: ro_render::Dir::S,
                        time_ms: 0.0,
                        once: true,
                        alpha: 1.0, scale: 1.0, tint: [1.0; 3],
                    };
                    let ms = r.action_ms(body, 0).max(200.0);
                    self.sprite_fx.push((actor, ms));
                }
            }
            Some(fx::Visual::Code) => {}
            Some(fx::Visual::Tenseiga) => {
                for &(part, texture, duration_ms, color) in fx::tenseiga_layers(skill) {
                    let Some(tex) = self.fx.fx_texture(r, vfs, texture) else { continue };
                    let shape = match part {
                        Some((part, solid)) => ro_render::Shape::Tenseiga { part, solid },
                        None => ro_render::Shape::Column { radius: 0.9, height: 3.5 },
                    };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms });
                }
            }
            None => {
                let by_name = self.skill_names.internal(skill).map(|n| {
                    let short = n.split_once('_').map_or(n, |(_, rest)| rest).to_ascii_lowercase();
                    [short.clone(), short.replace('_', "")]
                });
                let folder = by_name.as_ref().and_then(|n| self.fx.find(&n[0])).map(str::to_string);
                let asset = by_name.iter().flatten().find_map(|n| self.fx.get(r, vfs, n))
                    .or_else(|| folder.and_then(|p| self.fx.get(r, vfs, &p))).or_else(|| damaging.then(|| self.fx.get(r, vfs, "hit")).flatten());
                if let Some(asset) = asset {
                    r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: false, tag: 0 });
                } else if let Some(tex) = self.fx.texture(r, vfs, "alpha_down.tga") {
                    // Buffs and other support skills without their own effect: a soft light.
                    let shape = ro_render::Shape::Column { radius: 0.8, height: 3.0 };
                    r.procs.push(ro_render::ProcFx { tex, shape, pos, color: [0.45, 0.45, 0.5], time_ms: 0.0, duration_ms: 900.0 });
                }
                tracing::debug!(skill, name = ?self.skill_names.internal(skill), "no mapped effect; used fallback");
            }
        }
    }

    /// Earth spikes erupting in a wave from `from` toward `to` (Grimtooth).
    fn spike_line(&mut self, from: glam::Vec3, to: glam::Vec3) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        let Some(tex) = self.fx.texture(r, vfs, "dustblast_stick.bmp") else { return };
        let reach = to - from;
        let count = ((reach.length() / ro_render::CELL) as u32).clamp(3, 12);
        let shape = ro_render::Shape::Spikes { reach, count };
        r.procs.push(ro_render::ProcFx { tex, shape, pos: from, color: [0.85, 0.7, 0.5], time_ms: 0.0, duration_ms: 650.0 });
    }

    /// Alt+letter window shortcuts (also the Classic Basic Info buttons).
    fn alt_key(&mut self, key: &str) {
        match key {
            "e" => self.windows.toggle_inventory(),
            "q" => self.windows.toggle_equipment(),
            "a" => self.windows.toggle_stats(),
            "s" => self.windows.toggle_skills(),
            "z" => self.windows.party = !self.windows.party,
            "h" => self.windows.friends = !self.windows.friends,
            "g" => self.windows.guild = !self.windows.guild,
            "l" => self.emote_win.open = !self.emote_win.open,
            "m" => self.emote_win.macros_open = !self.emote_win.macros_open,
            "o" => self.windows.settings = !self.windows.settings,
            "u" => self.quests.open = !self.quests.open,
            "w" => self.merchant.cart_open = !self.merchant.cart_open,
            "b" => self.toggle_bank(),
            "c" => self.toggle_cash_shop(),
            "y" => self.progress.open = !self.progress.open,
            "clan" => self.clan.open = !self.clan.open,
            "repute" => {
                self.reputation.load(self.vfs.as_ref());
                self.reputation.open = !self.reputation.open;
            }
            "opts" => self.game_opts.open = !self.game_opts.open,
            "keys" => self.shortcut_win.open = !self.shortcut_win.open,
            "rank" => self.send(ro_net::Command::Ranking(self.ranking.kind)),
            "roulette" => self.open_roulette(),
            "battle" => {
                self.battle.ensure_loaded(self.vfs.as_ref());
                self.battle.open = !self.battle.open;
            }
            "navi" => {
                self.navi.ensure_loaded(self.vfs.as_ref());
                self.navi.open = !self.navi.open;
            }
            "n" => {
                self.booking.open = !self.booking.open;
                if self.booking.open {
                    self.send(ro_net::Command::Booking(ro_net::BookingRequest::Search { level: 0, map: 0, job: 0xffff }));
                }
            }
            "k" => {
                let me = self.hud.status.name.clone();
                self.rooms.open_create(&me);
            }
            "x" => {
                self.mailbox.open = !self.mailbox.open;
                if self.mailbox.open {
                    self.mailbox.unread = false;
                    self.send(ro_net::Command::Mail(ro_net::MailRequest::Open { refresh: false }));
                }
            }
            "r" => {
                self.companions.open = !self.companions.open;
                if self.companions.open {
                    self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(0)));
                }
            }
            "j" => {
                tracing::info!(open = self.progress.attendance.is_some(), "attendance");
                if self.progress.attendance.take().is_none() {
                    self.queued_commands.push(ro_net::Command::Progress(ro_net::ProgressRequest::OpenAttendance));
                }
            }
            // Alt+1..Alt+0 say the macros (emotes by default), as in the original.
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                let line = self.settings.macros.get(emotes::macro_slot(d.as_bytes()[0] - b'0')).map(|m| m.trim().to_string()).unwrap_or_default();
                if !line.is_empty() {
                    self.queued_chat.push(line);
                }
            }
            _ => {}
        }
    }

    /// The companion AI. Like the original, the homunculus runs AI\AI.lua and the
    /// mercenary AI\AI_M.lua (USER_AI\ when present there); without the scripts they just
    /// follow us and join our fights.
    fn companion_follow(&mut self, now: std::time::Instant) {
        if now < self.follow_at {
            return;
        }
        self.follow_at = now + std::time::Duration::from_millis(250);
        let homun = self.companions.homun_id;
        let merc = self.companions.merc.as_ref().map_or(0, |m| m.id);
        let mut scripted = false;
        for (id, file, homun_brain) in [(homun, "AI.lua", true), (merc, "AI_M.lua", false)] {
            if id == 0 || !self.units.contains_key(&id) {
                continue;
            }
            let slot = if homun_brain { &mut self.homun_ai } else { &mut self.merc_ai };
            if slot.is_none() {
                let Some(dir) = self.client_dir.as_deref() else { continue };
                let user = dir.join("AI").join("USER_AI").join(file).exists();
                match ai::load(dir, file, user) {
                    Ok(b) => {
                        tracing::info!(file, user, "companion AI loaded");
                        *slot = Some(b);
                    }
                    Err(e) => {
                        tracing::warn!("companion AI {file}: {e}");
                        self.client_dir = None;
                        continue;
                    }
                }
            }
            let snapshot = self.ai_snapshot();
            let Some(brain) = (if homun_brain { self.homun_ai.as_ref() } else { self.merc_ai.as_ref() }) else { continue };
            {
                let mut w = brain.world.borrow_mut();
                w.owner = snapshot.0;
                w.units = snapshot.1;
            }
            scripted = true;
            for order in brain.tick(id) {
                tracing::debug!(id, ?order, "AI order");
                let cmd = match order {
                    // The original's MoveToOwner asks the server to bring it beside us.
                    ai::AiOrder::Move { id, x, y } if self.player.as_ref().is_some_and(|p| p.walker.goal_or_cell() == (x, y)) => ro_net::CompanionRequest::Return(id),
                    ai::AiOrder::Move { id, x, y } => ro_net::CompanionRequest::Move { id, x: x as u16, y: y as u16 },
                    ai::AiOrder::Attack { id, target } => ro_net::CompanionRequest::Attack { id, target },
                    ai::AiOrder::Skill { level, skill, target, .. } => {
                        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkill { skill, level, target }));
                        continue;
                    }
                    ai::AiOrder::SkillGround { level, skill, x, y, .. } => {
                        self.send(ro_net::Command::Stat(ro_net::StatRequest::UseSkillAt { skill, level, x: x as u16, y: y as u16 }));
                        continue;
                    }
                };
                self.send(ro_net::Command::Companion(cmd));
            }
        }
        if scripted {
            return;
        }
        self.follow_at = now + std::time::Duration::from_millis(400);
        let Some((me, facing)) = self.player.as_ref().map(|p| (p.walker.goal_or_cell(), p.walker.dir)) else { return };
        // Behind us relative to our facing (dir 0 = south ... clockwise), one on each side.
        const STEP: [(i32, i32); 8] = [(0, -1), (-1, -1), (-1, 0), (-1, 1), (0, 1), (1, 1), (1, 0), (1, -1)];
        let behind = |turn: usize| {
            let (x, y) = STEP[(facing + 4 + turn) % 8];
            (x * 2, y * 2)
        };
        let ids = [(self.companions.homun_id, behind(1)), (self.companions.merc.as_ref().map_or(0, |m| m.id), behind(7))];
        // While we fight, they attack our target too (the original AI's default).
        if let Some(target) = self.attack.map(|a| a.0).filter(|t| self.units.get(t).is_some_and(|m| m.dying.is_none())) {
            let present: Vec<u32> = ids.iter().map(|(id, _)| *id).filter(|id| *id != 0 && self.units.contains_key(id)).collect();
            for id in present {
                self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Attack { id, target }));
            }
            self.follow_at = now + std::time::Duration::from_millis(1000);
            return;
        }
        for (id, (dx, dy)) in ids {
            let Some(unit) = self.units.get(&id).filter(|_| id != 0) else { continue };
            let at = unit.walker.goal_or_cell();
            if (at.0 - me.0).abs().max((at.1 - me.1).abs()) <= 3 {
                continue;
            }
            let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
            let spot = [(dx, dy), (dx, 0), (0, dy), (-dx, dy), (0, 0)]
                .iter()
                .map(|(ox, oy)| (me.0 + ox, me.1 + oy))
                .find(|&(x, y)| gat.is_some_and(|g| g.walkable(x, y)));
            if let Some((x, y)) = spot {
                self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Move { id, x: x as u16, y: y as u16 }));
            }
        }
    }

    /// The units in sight as the AI scripts see them: (our account id, units).
    fn ai_snapshot(&self) -> (u32, std::collections::HashMap<u32, ai::AiUnit>) {
        let motion = |m: &Mob| {
            if m.dying.is_some() || matches!(m.pose, Some((Pose::Dead, _))) {
                3
            } else if matches!(m.pose, Some((Pose::Attack, _))) {
                2
            } else if m.walker.is_walking() {
                1
            } else {
                0
            }
        };
        let mut units = std::collections::HashMap::new();
        if let Some(p) = self.player.as_ref() {
            let s = &self.hud.status;
            units.insert(
                self.account_id,
                ai::AiUnit { cell: p.walker.cell, motion: motion(p), target: self.attack.map_or(0, |a| a.0), hp: (s.hp as i32, s.max_hp as i32), sp: (s.sp as i32, s.max_sp as i32), range: self.attack_range, ..Default::default() },
            );
        }
        for (id, m) in &self.units {
            let job = self.unit_jobs.get(id).copied().unwrap_or(0) as i32;
            let mut u = ai::AiUnit { cell: m.walker.cell, motion: motion(m), target: self.aggro.get(id).copied().unwrap_or(0), monster: m.kind == 5, range: 1, ..Default::default() };
            if let Some(hp) = m.hp {
                u.hp = hp;
            }
            if *id == self.companions.homun_id {
                u.kind = job - 6000;
                if let Some(h) = &self.companions.homun {
                    u.hp = (h.hp as i32, h.max_hp as i32);
                    u.sp = (h.sp as i32, h.max_sp as i32);
                }
            } else if self.companions.merc.as_ref().is_some_and(|mc| mc.id == *id) {
                u.kind = job - 6016;
                // Archer mercenaries (types 1-10) shoot from range.
                u.range = if (1..=10).contains(&u.kind) { 9 } else { 1 };
                if let Some(mc) = &self.companions.merc {
                    u.hp = (mc.hp as i32, mc.max_hp as i32);
                    u.sp = (mc.sp as i32, mc.max_sp as i32);
                }
            }
            units.insert(*id, u);
        }
        (self.account_id, units)
    }

    /// Opens the bank (asking for the balance) or closes it.
    fn toggle_bank(&mut self) {
        let aid = self.account_id;
        if self.merchant.bank.take().is_some() {
            self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Close(aid)));
        } else {
            self.merchant.bank = Some((None, String::new()));
            self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Open(aid)));
            self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Check(aid)));
        }
    }

    /// Opens the cash shop (the server answers with our points) or closes it.
    fn toggle_cash_shop(&mut self) {
        let req = if self.stores.cash.take().is_some() { ro_net::CashShopRequest::Close } else { ro_net::CashShopRequest::Open };
        self.queued_commands.push(ro_net::Command::CashShop(req));
    }

    fn proc_fx(&mut self, tex: &'static str, shape: ro_render::Shape, pos: glam::Vec3, color: [f32; 3], duration_ms: f32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        if let Some(tex) = self.fx.texture(r, vfs, tex) {
            r.procs.push(ro_render::ProcFx { tex, shape, pos, color, time_ms: 0.0, duration_ms });
        }
    }

    /// Meteor Assault: eight violet slashes flying outward from the caster.
    /// The cast and every monster hit all announce it, so it plays once per cast.
    fn meteor_assault(&mut self, src: u32) {
        use ro_render::{Shape, CELL};
        let now = std::time::Instant::now();
        if self.burst_at.get(&src).is_some_and(|t| now.duration_since(*t).as_millis() < 400) {
            return;
        }
        let Some(at) = self.unit_pos(src) else { return };
        self.burst_at.insert(src, now);
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let dir = glam::Vec3::new(a.cos(), 0.0, a.sin()) * CELL;
            let shape = Shape::Slash { travel: dir * 4.0, size: (1.0, 2.0) };
            self.proc_fx("purpleslash.tga", shape, at + dir, [1.0, 1.0, 1.0], 500.0);
        }
    }

    /// Crescent Gale: a crescent of mortal flame flying from the caster past the target, the
    /// server's 11-cell path. Every enemy on the path reports a hit, so it plays once per cast.
    fn crescent_gale(&mut self, src: u32, from: glam::Vec3, to: glam::Vec3) {
        let now = std::time::Instant::now();
        if self.burst_at.get(&src).is_some_and(|t| now.duration_since(*t).as_millis() < 400) {
            return;
        }
        self.burst_at.insert(src, now);
        let travel = (to - from).normalize_or_zero() * ro_render::CELL * 11.0;
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        let Some(tex) = self.fx.fx_texture(r, vfs, fx::FxTexture::Streak) else { return };
        for solid in [true, false] {
            let shape = ro_render::Shape::Crescent { travel, solid };
            r.procs.push(ro_render::ProcFx { tex, shape, pos: from, color: [1.0; 3], time_ms: 0.0, duration_ms: 800.0 });
        }
    }

    fn play_fx(&mut self, name: &'static str, pos: glam::Vec3) {
        self.play_fx_tagged(name, pos, 0);
    }

    /// Plays an STR effect; a non-zero `tag` loops it until `stop_fx(tag)`.
    /// A trap's model standing at `pos` (loaded once per model).
    fn trap_draws(&mut self, path: &'static str, pos: glam::Vec3) -> Vec<ro_render::ModelDraw> {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return vec![] };
        let model = self
            .trap_models
            .entry(path)
            .or_insert_with(|| {
                let file = format!("data\\model\\{path}.rsm");
                let rsm = vfs.read(&file).ok().flatten().and_then(|d| ro_formats::rsm::Rsm::parse(&d).inspect_err(|e| tracing::warn!(%file, "{e:#}")).ok())?;
                let textures = rsm
                    .textures
                    .iter()
                    .map(|t| {
                        let p = format!("data\\texture\\{t}");
                        let img = vfs.read(&p).ok().flatten().and_then(|d| ro_formats::image::decode(&d, &p).ok())?;
                        Some(r.add_model_texture(&img))
                    })
                    .collect();
                Some(std::rc::Rc::new((rsm, textures)))
            })
            .clone();
        let Some(model) = model else { return vec![] };
        ro_render::rsm_at(&model.0, pos)
            .into_iter()
            .filter_map(|(t, vertices)| Some(ro_render::ModelDraw { texture: (*model.1.get(t)?)?, vertices }))
            .collect()
    }

    fn play_fx_tagged(&mut self, name: &'static str, pos: glam::Vec3, tag: u32) {
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) else { return };
        if let Some(asset) = self.fx.get(r, vfs, name) {
            r.effects.push(ro_render::Fx { asset, pos, time_ms: 0.0, looping: tag != 0, tag });
        }
    }

    /// The original client's slash commands for parties, options and help (msgstringtable
    /// wording). Returns false for anything else.
    fn client_command(&mut self, text: &str) -> bool {
        let t = text.trim();
        let (head, arg) = t.split_once(' ').map_or((t, ""), |(h, a)| (h, a.trim().trim_matches('"')));
        let say = |id: usize, en: &str| msgstr::get(id, &[]).unwrap_or_else(|| en.to_string());
        let mut notes = vec![];
        match head {
            "/h" | "/help" => notes.extend((33..=46).chain(132..=142).chain([157, 211]).filter_map(|id| msgstr::get(id, &[]))),
            "/who" | "/w" | "/player" if arg.is_empty() => self.send(ro_net::Command::Who),
            "/organize" if !arg.is_empty() => self.send(ro_net::Command::Party(ro_net::PartyRequest::Create { name: arg.to_string() })),
            "/invite" if !arg.is_empty() => self.send(ro_net::Command::Party(ro_net::PartyRequest::Invite { name: arg.to_string() })),
            "/leave" => self.send(ro_net::Command::Party(ro_net::PartyRequest::Leave)),
            "/expel" if !arg.is_empty() => {
                let member = self.social.party.as_ref().and_then(|p| p.members.iter().find(|m| m.name.eq_ignore_ascii_case(arg)));
                match member.map(|m| (m.account_id, m.name.clone())) {
                    Some((account_id, name)) => self.send(ro_net::Command::Party(ro_net::PartyRequest::Expel { account_id, name })),
                    None => notes.push(say(148, "There is no such character name or the user is offline.")),
                }
            }
            "/accept" | "/refuse" => {
                self.game_opts.deny_invites = head == "/refuse";
                self.send(ro_net::Command::Option(ro_net::OptionRequest::DenyPartyInvites(head == "/refuse")));
            }
            "/call" => {
                let on = !self.game_opts.call;
                self.game_opts.call = on;
                self.send(ro_net::Command::Option(ro_net::OptionRequest::Config { kind: 1, on }));
                notes.push(if on { say(2978, "You can be called with various skills (Emergency Call,...). (/call)") } else { say(2979, "You cannot be called with various skills (Emergency Call,...). (/call)") });
            }
            "/memo" => self.send(ro_net::Command::Memo),
            // GM commands, as the original client sends them; the server checks permissions.
            "/b" | "/nb" | "/lb" | "/nlb" if !arg.is_empty() => {
                // /b and /lb sign the message with our name; /nb and /nlb don't.
                let rest = t.split_once(' ').map_or("", |(_, r)| r.trim());
                let text = if head.starts_with("/n") { rest.to_string() } else { format!("{}: {rest}", self.hud.status.name) };
                self.send(ro_net::Command::Gm(if head.ends_with("lb") { ro_net::GmRequest::LocalBroadcast(text) } else { ro_net::GmRequest::Broadcast(text) }));
            }
            "/hide" => self.send(ro_net::Command::Gm(ro_net::GmRequest::Hide)),
            "/mm" | "/mapmove" if !arg.is_empty() => {
                let mut parts = t.split_whitespace().skip(1);
                let map = parts.next().unwrap_or_default().to_string();
                let (x, y) = (parts.next().and_then(|v| v.parse().ok()).unwrap_or(0), parts.next().and_then(|v| v.parse().ok()).unwrap_or(0));
                self.send(ro_net::Command::Gm(ro_net::GmRequest::MapMove { map, x, y }));
            }
            "/recall" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Recall(arg.to_string()))),
            "/shift" | "/remove" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Shift(arg.to_string()))),
            "/item" | "/monster" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Create(arg.to_string()))),
            "/rc" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Mute(arg.to_string()))),
            "/check" if !arg.is_empty() => self.send(ro_net::Command::Gm(ro_net::GmRequest::Check(arg.to_string()))),
            "/killall" => self.send(ro_net::Command::Gm(ro_net::GmRequest::KickAll)),
            "/resetstate" => self.send(ro_net::Command::Gm(ro_net::GmRequest::ResetStats)),
            "/resetcooltime" => self.send(ro_net::Command::Gm(ro_net::GmRequest::ResetCooltime)),
            "/changemaptype" => {
                let v: Vec<u16> = arg.split_whitespace().filter_map(|v| v.parse().ok()).collect();
                match v[..] {
                    [x, y, kind] => self.send(ro_net::Command::Gm(ro_net::GmRequest::ChangeMapType { x, y, kind })),
                    _ => notes.push("Usage: /changemaptype <x> <y> <type>".into()),
                }
            }
            "/deal" if !arg.is_empty() => match self.units.iter().find(|(_, u)| u.name.eq_ignore_ascii_case(arg)).map(|(id, _)| *id) {
                Some(target) => self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ask { target })),
                None => notes.push(say(148, "There is no such character name or the user is offline.")),
            },
            "/chat" => {
                let me = self.hud.status.name.clone();
                self.rooms.open_create(&me);
            }
            "/q" => self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Leave)),
            "/bgm" | "/music" => {
                self.settings.bgm_off = !self.settings.bgm_off;
                notes.push(if self.settings.bgm_off { say(32, "BGM Off") } else { say(31, "BGM On") });
            }
            "/sound" => {
                self.settings.sfx_off = !self.settings.sfx_off;
                notes.push(if self.settings.sfx_off { say(28, "Sound Effects Off") } else { say(27, "Sound Effects On") });
            }
            "/effect" => {
                self.settings.effects_off = !self.settings.effects_off;
                notes.push(if self.settings.effects_off { say(24, "Effects Off") } else { say(23, "Effects On") });
            }
            "/aura" if self.settings.aura_off => notes.push(say(2996, "Aura effect is OFF. Please turn it ON and try again")),
            "/aura" => {
                self.settings.aura_simple = !self.settings.aura_simple;
                notes.push(if self.settings.aura_simple { say(712, "Simplified Aura [ON]") } else { say(711, "Simplified Aura [OFF]") });
            }
            "/bm" | "/battlemode" => {
                self.settings.battle_mode = !self.settings.battle_mode;
                notes.push(if self.settings.battle_mode { say(785, "Skills assigned to shortcut windows 1, 2, 3 are Enabled. [/bm ON]") } else { say(786, "Skills assigned to shortcut windows 1, 2, 3 are Disabled. [/bm OFF]") });
                if self.settings.battle_mode {
                    notes.push(say(796, "Press the space bar to Chat when in Battle mode [/battlemode | /bm]."));
                }
            }
            "/emo" | "/emotion" => self.emote_win.open = !self.emote_win.open,
            "/aura2" => {
                self.settings.aura_off = !self.settings.aura_off;
                notes.push(if self.settings.aura_off { say(2995, "Aura effect is OFF") } else { say(2994, "Aura effect is ON") });
            }
            "/v" | "/bv" if !arg.is_empty() => {
                let Ok(v) = arg.parse::<u8>() else { return false };
                let v = v.min(127) as f32 / 127.0;
                if head == "/v" {
                    self.settings.sfx_volume = v;
                } else {
                    self.settings.bgm_volume = v;
                }
            }
            _ => return false,
        }
        if matches!(head, "/bgm" | "/music" | "/sound" | "/effect" | "/aura" | "/aura2" | "/v" | "/bv" | "/bm" | "/battlemode") {
            self.apply_sound_and_effects();
            self.settings.save();
        }
        for n in notes {
            self.hud.push_chat(ro_net::ChatKind::System, n);
        }
        true
    }

    /// Volumes and the effect switch, as the settings and /bgm, /sound, /effect leave them.
    fn apply_sound_and_effects(&mut self) {
        let s = &self.settings;
        if let Some(a) = self.audio.as_mut() {
            a.set_bgm_volume(if s.bgm_off { 0.0 } else { s.bgm_volume });
            a.sfx_volume = if s.sfx_off { 0.0 } else { s.sfx_volume };
        }
        if let Some(r) = self.renderer.as_mut() {
            r.show_effects = !s.effects_off;
        }
    }

    /// Sends a chat line, handling the client's own commands (whispers, emotes, /where...).
    fn submit_chat(&mut self, text: String) {
        if self.client_command(&text) {
            return;
        }
        let Some((tx, _)) = self.net.as_ref() else { return };
            // "/p text" goes to the party, "/g text" to the guild.
            let speaker = self.hud.status.name.clone();
            let mut note = None;
            // Whisper blocking, as the original: /ex name, /in name, /exall, /inall, /ex (list).
            let ignore = match text.trim() {
                "/exall" => Some(ro_net::IgnoreRequest::All { block: true }),
                "/inall" => Some(ro_net::IgnoreRequest::All { block: false }),
                "/ex" => Some(ro_net::IgnoreRequest::List),
                t => match t.split_once(' ') {
                    Some(("/ex", n)) => Some(ro_net::IgnoreRequest::Player { name: n.trim().trim_matches('"').to_string(), block: true }),
                    Some(("/in", n)) => Some(ro_net::IgnoreRequest::Player { name: n.trim().trim_matches('"').to_string(), block: false }),
                    _ => None,
                },
            };
            // /viewpointvalue shows the camera values; /setcamera range rotation latitude sets them.
            if text.trim() == "/viewpointvalue" {
                let _ = tx.send(ro_net::Command::Camera { show: true, range: 0.0, rotation: 0.0, latitude: 0.0 });
                return;
            }
            if let Some(args) = text.trim().strip_prefix("/setcamera ") {
                let v: Vec<f32> = args.split_whitespace().filter_map(|s| s.parse().ok()).collect();
                if let [range, rotation, latitude] = v[..] {
                    let _ = tx.send(ro_net::Command::Camera { show: false, range, rotation, latitude });
                }
                return;
            }
            if text.trim() == "/macroreport" {
                self.reporter.toggle();
                return;
            }
            if text.trim() == "/agency" {
                self.agency.toggle();
                return;
            }
            if text.trim() == "/limitedsale" {
                let _ = tx.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Open));
                return;
            }
            if text.trim() == "/roulette" {
                let _ = tx.send(ro_net::Command::Roulette(ro_net::RouletteRequest::Open));
                let _ = tx.send(ro_net::Command::Roulette(ro_net::RouletteRequest::Info));
                return;
            }
            if let Some(k) = ["/blacksmith", "/alchemist", "/taekwon", "/pk"].iter().position(|c| text.trim() == *c) {
                let _ = tx.send(ro_net::Command::Ranking(k as u16));
                return;
            }
            // /navi map [x/y]: guide there, as the original client; /navi alone opens the window.
            if let Some(rest) = text.trim().strip_prefix("/navi") {
                self.navi.ensure_loaded(self.vfs.as_ref());
                let mut parts = rest.split(|c: char| c == ' ' || c == '/' || c == ',').filter(|s| !s.is_empty());
                match parts.next() {
                    Some(map) => {
                        let cell = match (parts.next().and_then(|x| x.parse().ok()), parts.next().and_then(|y| y.parse().ok())) {
                            (Some(x), Some(y)) => Some((x, y)),
                            _ => None,
                        };
                        let label = match cell { Some((x, y)) => format!("{map} ({x}, {y})"), None => map.to_string() };
                        self.navi.go(map, cell, label);
                    }
                    None => self.navi.open = true,
                }
                return;
            }
            if let Some(r) = ignore {
                if let ro_net::IgnoreRequest::Player { name, block } = &r {
                    self.ignore_pending = Some((name.clone(), *block));
                }
                let _ = tx.send(ro_net::Command::Ignore(r));
                return;
            }
            let cmd = match text.split_once(' ') {
                Some(("/p", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::Chat { speaker, text: rest.to_string() })),
                Some(("/g", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::GuildChat { speaker, text: rest.to_string() })),
                Some(("/cl", rest)) => Some(ro_net::Command::Party(ro_net::PartyRequest::ClanChat { speaker, text: rest.to_string() })),
                Some(("/bc", rest)) => Some(ro_net::Command::Bg(ro_net::BgRequest::Chat(format!("{speaker} : {rest}")))),
                // /guild "Name": found a guild, as in the original client.
                Some(("/guild", rest)) => Some(ro_net::Command::Guild(ro_net::GuildRequest::Create { char_id: self.char_id, name: rest.trim().trim_matches('"').to_string() })),
                // /r text: answer whoever whispered last.
                Some(("/r", rest)) => match self.last_whisper_from.clone() {
                    Some(to) => {
                        note = Some((ro_net::ChatKind::Whisper, format!("(To {to}) : {rest}")));
                        Some(ro_net::Command::Whisper { to, text: rest.to_string() })
                    }
                    None => {
                        note = Some((ro_net::ChatKind::System, "Nobody has whispered you yet.".into()));
                        None
                    }
                },
                _ if text.starts_with("/w ") || text.starts_with("/whisper ") => match emotes::whisper(&text) {
                    Some((to, msg)) => {
                        note = Some((ro_net::ChatKind::Whisper, format!("(To {to}) : {msg}")));
                        Some(ro_net::Command::Whisper { to, text: msg })
                    }
                    None => {
                        note = Some((ro_net::ChatKind::System, "Usage: /w Name message  (quote names with spaces)".into()));
                        None
                    }
                },
                _ => match emotes::command(&text) {
                    Some(e) => Some(ro_net::Command::Emotion(e)),
                    // Commands the client handles itself.
                    None if text == "/sit" || text == "/stand" => Some(ro_net::Command::Sit(text == "/sit")),
                    None if text == "/where" => {
                        let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                        if let Some((x, y)) = self.player.as_ref().map(|p| p.walker.cell) {
                            note = Some((ro_net::ChatKind::System, format!("{map} {x}, {y}")));
                        }
                        None
                    }
                    // An unknown /command is not said aloud.
                    None if text.starts_with('/') => {
                        let name = text.split_whitespace().next().unwrap_or(&text);
                        note = Some((ro_net::ChatKind::System, format!("{name}: unknown command.")));
                        None
                    }
                    None => Some(ro_net::Command::Chat(text)),
                },
            };
            if let Some(cmd) = cmd {
                let _ = tx.send(cmd);
            }
            if let Some((kind, line)) = note {
                self.hud.push_chat(kind, line);
            }
    }

    /// Units the controller can target, nearest first: monsters and named NPCs in reach, or
    /// other players while aiming a support skill.
    fn pad_candidates(&self, players: bool) -> Vec<u32> {
        let Some(me) = self.player.as_ref().map(|p| p.walker.cell) else { return vec![] };
        let mut v: Vec<(i32, u32)> = self
            .units
            .iter()
            .filter(|(_, m)| m.dying.is_none() && m.actor.alpha > 0.01 && if players { m.kind == 0 } else { m.kind == 5 || (is_npc(m.kind) && !m.name.is_empty()) })
            .map(|(id, m)| ((m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2), *id))
            .filter(|(d, _)| *d <= PAD_REACH * PAD_REACH)
            .collect();
        v.sort();
        v.into_iter().map(|(_, id)| id).collect()
    }

    /// Plays a controller feedback cue, when a controller is being played with.
    fn feel(&mut self, c: haptics::Cue) {
        if self.pad.in_use() && self.settings.vibration > 0.0 {
            tracing::debug!(?c, "vibration cue");
            self.haptics.play(c);
        }
    }

    /// Feedback for an attack: a thud when we are hit, as hard as the share of our HP it
    /// took; a sharp tick when we land a critical.
    fn feel_act(&mut self, act: &ro_net::Act) {
        let me = self.account_id;
        if act.target == me && act.src != me && act.damage > 0 {
            let max = self.hud.status.max_hp.max(1) as f32;
            self.feel(haptics::Cue::Hit((act.damage as f32 + act.left_damage.max(0) as f32) / max));
        } else if act.src == me && act.target != me && act.kind == 10 && act.damage > 0 {
            self.feel(haptics::Cue::Crit);
        }
    }

    /// Feedback for a non-damaging skill: ours on ourselves or a player swells, on a monster
    /// zaps; another player's on us swells gently.
    fn feel_skill_used(&mut self, src: u32, target: u32) {
        let me = self.account_id;
        let kind = |app: &Self, id: u32| app.units.get(&id).map(|m| m.kind);
        if src == me {
            self.haptics.end_charge();
            let cue = if target == me || target == 0 || kind(self, target) == Some(0) { haptics::Cue::Support } else { haptics::Cue::Hex };
            self.feel(cue);
        } else if target == me && kind(self, src) == Some(0) {
            self.feel(haptics::Cue::Blessed);
        }
    }

    /// Controller feedback each frame: finds a Sony or Nintendo pad to open directly, beats
    /// the heart at low HP, and sends the mixed rumble (and the light bar's HP colour) to the
    /// pad: as audio to a USB DualSense's actuators, as HID reports to a Sony or Nintendo
    /// pad, else through gilrs.
    fn feedback_tick(&mut self, dt: f32) {
        // Follow the pad in use: another one picked up closes this one.
        let want = self.pad.active_ids();
        if self.hid.as_ref().is_some_and(|h| !h.alive() || want.is_some_and(|w| w != h.ids)) {
            self.hid = None;
            self.audio_haptics = None;
            self.hid_retry_ms = 0.0;
        }
        if self.hid.is_none() && self.pad.connected() {
            self.hid_retry_ms -= dt;
            if self.hid_retry_ms <= 0.0 {
                self.hid_retry_ms = 3000.0;
                self.hid = hidpad::HidPad::open(want, self.pad.detected);
                if self.settings.dualsense_hd && self.hid.as_ref().is_some_and(|h| h.kind == hidpad::Kind::DualSense) {
                    self.audio_haptics = haptics::AudioHaptics::open();
                }
            }
        }
        // The DualSense switched between HD haptics and classic rumble.
        if self.haptics_hd != self.settings.dualsense_hd {
            self.haptics_hd = self.settings.dualsense_hd;
            self.audio_haptics = None;
            if self.haptics_hd && self.hid.as_ref().is_some_and(|h| h.kind == hidpad::Kind::DualSense) {
                self.audio_haptics = haptics::AudioHaptics::open();
            }
        }
        let (hp, max) = (self.hud.status.hp, self.hud.status.max_hp);
        let share = if max > 0 { hp as f32 / max as f32 } else { 1.0 };
        if !self.dead && share < 0.25 {
            self.heartbeat_ms -= dt;
            if self.heartbeat_ms <= 0.0 {
                self.heartbeat_ms = if share < 0.1 { 650.0 } else { 1000.0 };
                self.feel(haptics::Cue::Heartbeat);
            }
        } else {
            self.heartbeat_ms = 0.0;
        }
        if std::mem::take(&mut self.settings.vibration_test) {
            tracing::info!(connected = self.pad.connected(), hid = ?self.hid.as_ref().map(|h| h.kind), audio = self.audio_haptics.is_some(), "vibration test");
            self.haptics.play(haptics::Cue::Hit(0.6));
            self.haptics.play(haptics::Cue::Crit);
            self.haptics.play(haptics::Cue::LevelUp);
            self.haptics_test_ms = 800.0;
        }
        self.haptics_test_ms -= dt;
        let (mut low, mut high) = self.haptics.frame(dt, self.settings.vibration.max(if self.haptics_test_ms > 0.0 { 0.5 } else { 0.0 }));
        if !self.pad.in_use() && self.haptics_test_ms <= 0.0 {
            (low.amp, high.amp) = (0.0, 0.0);
        }
        // The light bar shows HP: green, amber below half, red below a quarter, pulsing near death.
        let light = (self.settings.light_bar && max > 0).then(|| {
            let base: [f32; 3] = if self.dead {
                [0.35, 0.0, 0.0]
            } else if share > 0.5 {
                [0.15, 0.85, 0.3]
            } else if share > 0.25 {
                [0.95, 0.6, 0.1]
            } else {
                [0.95, 0.12, 0.12]
            };
            let secs = std::time::UNIX_EPOCH.elapsed().map_or(0.0, |d| (d.as_millis() % 100_000) as f32 / 1000.0);
            let pulse = if !self.dead && share < 0.1 { 0.35 + 0.65 * (secs * 3.0).sin().abs() } else { 1.0 };
            base.map(|c| (c * pulse * 255.0) as u8)
        });
        let route = match (&self.audio_haptics, &self.hid) {
            (Some(_), _) => "DualSense haptics (audio)",
            (None, Some(_)) => "controller HID rumble",
            (None, None) => "gilrs rumble",
        };
        if (low.amp > 0.0 || high.amp > 0.0) && route != self.haptics_route {
            tracing::info!(route, "vibration");
            self.haptics_route = route;
        }
        let audio_haptics = self.audio_haptics.is_some();
        if let Some(a) = &self.audio_haptics {
            a.set(low, high);
            (low.amp, high.amp) = (0.0, 0.0);
        }
        match self.hid.as_mut() {
            Some(h) => h.set(hidpad::Output { low, high, light, audio_haptics }),
            None => self.pad.set_rumble(low.amp, high.amp),
        }
    }

    /// The PlayStation touchpad: one finger moves the pointer, pressing clicks (with two
    /// fingers, right-clicks), a two-finger swipe across turns the shortcut row on show and a
    /// two-finger slide up or down zooms.
    fn touchpad_tick(&mut self, el: &ActiveEventLoop) {
        let Some(t) = self.hid.as_ref().filter(|_| self.settings.touchpad).map(|h| h.touch()) else { return };
        let prev = std::mem::replace(&mut self.touch_prev, t);
        let Some(window) = self.window.clone() else { return };
        let size = window.inner_size();
        let down: Vec<hidpad::Finger> = t.fingers.iter().flatten().copied().collect();
        let held = |f: &hidpad::Finger| prev.fingers.iter().flatten().find(|p| p.id == f.id).copied();
        match down.as_slice() {
            [f] => {
                self.touch_slide = glam::Vec2::ZERO;
                if let Some(p) = held(f) {
                    let d = glam::Vec2::new(f.x - p.x, f.y - p.y);
                    // A full stroke crosses the window; faster strokes go further.
                    let gain = (0.9 + 6.0 * d.length().min(0.05)) * self.settings.pad_tuning.touch_speed;
                    let (x, y) = self.touch_pos.or(self.cursor).unwrap_or((size.width as f64 / 2.0, size.height as f64 / 2.0));
                    let pos = ((x + (d.x * gain * size.width as f32) as f64).clamp(0.0, size.width as f64 - 1.0), (y + (d.y * gain * size.width as f32 * 0.6) as f64).clamp(0.0, size.height as f64 - 1.0));
                    if pos != (x, y) {
                        self.touch_pos = Some(pos);
                        if window.set_cursor_position(winit::dpi::PhysicalPosition::new(pos.0, pos.1)).is_err() {
                            // Where the pointer can't be moved (Wayland), move ours alone.
                            self.touch_feeding = true;
                            self.window_event(el, window.id(), WindowEvent::CursorMoved { device_id: winit::event::DeviceId::dummy(), position: winit::dpi::PhysicalPosition::new(pos.0, pos.1) });
                            self.touch_feeding = false;
                        }
                    }
                }
            }
            [a, b] => {
                if let (Some(pa), Some(pb)) = (held(a), held(b)) {
                    self.touch_slide += glam::Vec2::new((a.x + b.x - pa.x - pb.x) / 2.0, (a.y + b.y - pa.y - pb.y) / 2.0);
                    if self.touch_slide.x.abs() > 0.3 {
                        let n = HOTKEY_ROWS;
                        let row = if self.touch_slide.x > 0.0 { (self.hotkey_row + 1) % n } else { (self.hotkey_row + n - 1) % n };
                        self.queued_action = Some(windows::UiAction::HotkeyRow(row));
                        self.touch_slide = glam::Vec2::ZERO;
                    } else if self.touch_slide.y.abs() > 0.06 {
                        if let Some(r) = self.renderer.as_mut() {
                            r.camera.zoom(-self.touch_slide.y * 12.0);
                        }
                        self.touch_slide.y = 0.0;
                    }
                }
            }
            _ => {
                self.touch_slide = glam::Vec2::ZERO;
                self.touch_pos = None;
            }
        }
        let press = |app: &mut Self, button: MouseButton, state: ElementState| {
            app.touch_feeding = true;
            app.window_event(el, window.id(), WindowEvent::MouseInput { device_id: winit::event::DeviceId::dummy(), state, button });
            app.touch_feeding = false;
        };
        if t.click && !prev.click {
            let button = if down.len() >= 2 { MouseButton::Right } else { MouseButton::Left };
            self.touch_button = Some(button);
            press(self, button, ElementState::Pressed);
        } else if !t.click && prev.click {
            if let Some(button) = self.touch_button.take() {
                press(self, button, ElementState::Released);
            }
        }
    }

    /// Keys, clicks, the wheel or a real stroke of the mouse put the controller down: its button
    /// hints give way to the keyboard's until it is touched again. The touchpad steering the
    /// pointer is the controller's own input, and so is resting mouse jitter.
    fn note_keyboard_mouse(&mut self, event: &WindowEvent) {
        if !self.pad.in_use() || self.touch_feeding {
            self.mouse_travel = 0.0;
            return;
        }
        let put_down = match event {
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { state: ElementState::Pressed, .. }, .. }
            | WindowEvent::MouseInput { state: ElementState::Pressed, .. }
            | WindowEvent::MouseWheel { .. } => true,
            WindowEvent::CursorMoved { position, .. } if self.touch_pos.is_none() => {
                if let Some((x, y)) = self.cursor {
                    self.mouse_travel += (position.x - x).hypot(position.y - y);
                }
                self.mouse_travel > 48.0
            }
            _ => false,
        };
        if put_down {
            self.pad.put_down();
            self.mouse_travel = 0.0;
        }
    }

    /// Battle mode on a controller: a shortcut slot fires at once, without aiming. Attacks go
    /// on the target (the nearest monster when there is none), support skills on the targeted
    /// player or ourselves, ground skills under the target or at our feet.
    fn pad_quick_slot(&mut self, slot: usize) {
        let Some(key) = self.hotkeys.get(slot).copied().filter(|k| k.id != 0) else { return };
        let skill = key.id as u16;
        let Some(s) = self.skills.iter().find(|s| key.skill && s.id == skill) else {
            // Items, and skills we no longer have, as a click would.
            self.queued_action = Some(windows::UiAction::UseHotkey(slot));
            return;
        };
        let (flags, level) = (s.target, (key.count.max(1) as u16).clamp(1, s.level.max(1)));
        let target = self.pad_target.and_then(|t| self.units.get(&t).map(|m| (t, m.kind, m.walker.cell)));
        let me = self.player.as_ref().map(|p| p.walker.cell);
        if flags & 4 != 0 {
            self.use_skill_on(skill, level, self.account_id);
        } else if flags & 16 != 0 && flags & 1 == 0 {
            let ally = target.filter(|t| t.1 == 0).map_or(self.account_id, |t| t.0);
            self.use_skill_on(skill, level, ally);
        } else if flags & 2 != 0 {
            if let Some(c) = target.map(|t| t.2).or(me) {
                self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16));
            }
        } else if flags & 1 != 0 {
            let foe = target
                .filter(|t| t.1 == 5 || (t.1 == 0 && self.is_enemy_player(t.0)))
                .map(|t| t.0)
                .or_else(|| self.pad_candidates(false).into_iter().find(|id| self.units.get(id).is_some_and(|m| m.kind == 5)));
            if let Some(foe) = foe {
                self.pad_target = Some(foe);
                self.use_skill_on(skill, level, foe);
            }
        } else {
            self.start_skill(skill, level);
        }
    }

    /// Controller play: left stick walks (camera-relative), right stick turns and tilts the
    /// camera, A acts on the highlighted target, LB/RB change target, X picks up, Y sits,
    /// B cancels. Holding RT reaches the shortcut row on show, LT the next and, in battle
    /// mode, both the one after; then a face button, the D-pad or R3 fires a slot (at once in
    /// battle mode, else aimed first).
    fn pad_tick(&mut self, dt: f32) {
        use gilrs::Button::*;
        self.pad.tuning = self.settings.pad_tuning;
        let mut f = self.pad.poll();
        self.pad_row = None;
        let mut testing = false;
        if let Some((v, left)) = self.test_stick.as_mut() {
            *left -= dt;
            if *left > 0.0 {
                f.left = *v;
                testing = true;
            } else {
                self.test_stick = None;
            }
        }
        if !self.pad.in_use() && !testing {
            if self.pad_dir.is_some() && self.test_stick.is_none() {
                // A test stick just let go: stop as a released stick does.
                f.left = glam::Vec2::ZERO;
            } else {
                return;
            }
        }
        self.hud.pad_used = true;
        let secs = dt / 1000.0;

        // Camera: right stick turns and tilts; D-pad left/right turn 45 degrees, up/down zoom.
        if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
            if f.right != glam::Vec2::ZERO {
                r.camera.rotate(-f.right.x * secs * 2.2, -f.right.y * secs * 1.2);
            }
            // (With a trigger held, R3 is a shortcut slot.)
            if f.just(RightThumb) && !(f.lt || f.rt) {
                r.camera.yaw = 0.0;
            }
        }

        // Dead: A returns to the save point; nothing else acts.
        if self.dead {
            if f.just(South) {
                self.send(ro_net::Command::Restart(0));
            }
            return;
        }

        // NPC dialogs take the D-pad and face buttons while open.
        if self.dialog.is_some() {
            self.hud.pad_nav = if f.just(DPadUp) {
                Some(hud::PadNav::Up)
            } else if f.just(DPadDown) {
                Some(hud::PadNav::Down)
            } else if f.just(South) {
                Some(hud::PadNav::Confirm)
            } else if f.just(East) {
                Some(hud::PadNav::Cancel)
            } else {
                None
            };
            return;
        }

        // Shortcut bar: hold RT (the row on show), LT (the next) or, in battle mode, both (the
        // one after), then a face button, the D-pad or R3 (padglyph::SLOT_BUTTONS' order).
        if f.rt || f.lt {
            let offset = match (f.lt, f.rt) {
                (true, true) if self.settings.battle_mode => 2,
                (true, false) => 1,
                _ => 0,
            };
            self.pad_row = Some(offset);
            const KEYS: [gilrs::Button; 9] = [South, East, West, North, DPadUp, DPadRight, DPadDown, DPadLeft, RightThumb];
            if let Some(col) = KEYS.iter().position(|k| f.just(*k)) {
                let slot = ((self.hotkey_row + offset) % HOTKEY_ROWS) as usize * HOTKEYS_PER_ROW + col;
                if self.settings.battle_mode {
                    self.pad_quick_slot(slot);
                } else {
                    self.queued_action = Some(windows::UiAction::UseHotkey(slot));
                }
            }
        } else {
            // Target: keep the current one while it is in reach, else the nearest.
            let support_aim = self.aiming.and_then(|(s, _)| self.skills.iter().find(|k| k.id == s)).is_some_and(|k| k.target & 16 != 0 && k.target & 1 == 0);
            let list = self.pad_candidates(support_aim);
            if self.pad_target.is_none_or(|t| !list.contains(&t)) {
                self.pad_target = list.first().copied();
            }
            if f.just(RightTrigger) || f.just(LeftTrigger) {
                let at = self.pad_target.and_then(|t| list.iter().position(|&x| x == t)).unwrap_or(0);
                let n = list.len().max(1);
                let next = if f.just(RightTrigger) { (at + 1) % n } else { (at + n - 1) % n };
                self.pad_target = list.get(next).copied();
            }
            let target = self.pad_target.and_then(|t| self.units.get(&t).map(|m| (t, m.kind, m.walker.cell)));
            if let Some((skill, level)) = self.aiming {
                // Aiming a skill: A on the target (or its cell), Y on yourself, B cancels.
                let flags = self.skills.iter().find(|s| s.id == skill).map_or(0, |s| s.target);
                let ground = flags & 2 != 0;
                // Support skills (Heal, Detoxify, Blessing...) go on a targeted player, else on us.
                let support = flags & 16 != 0 && flags & 1 == 0;
                if f.just(South) && support {
                    self.aiming = None;
                    let ally = target.filter(|t| t.1 == 0).map_or(self.account_id, |t| t.0);
                    self.use_skill_on(skill, level, ally);
                } else if f.just(South) {
                    self.aiming = None;
                    match target {
                        Some((_, _, c)) if ground => self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16)),
                        Some((id, _, _)) => self.use_skill_on(skill, level, id),
                        None if ground => {
                            if let Some(c) = self.player.as_ref().map(|p| p.walker.cell) {
                                self.use_skill_at(skill, level, (c.0 as u16, c.1 as u16));
                            }
                        }
                        None => {}
                    }
                } else if f.just(North) {
                    self.aiming = None;
                    self.use_skill_on(skill, level, self.account_id);
                } else if f.just(East) {
                    self.aiming = None;
                }
            } else {
                if f.just(South) {
                    match target {
                        Some((id, 5, _)) => self.attack_unit(id),
                        Some((id, _, _)) => {
                            self.walk_goal = None;
                            self.talk(id);
                        }
                        None => {}
                    }
                }
                if f.just(East) {
                    self.attack = None;
                    self.chasing = false;
                    self.pending_skill = None;
                    self.pending_pickup = None;
                }
                if f.just(West) {
                    // Nearest item on the ground.
                    let me = self.player.as_ref().map(|p| p.walker.cell);
                    let item = me.and_then(|me| {
                        self.floor
                            .iter()
                            .map(|(id, (fi, _))| ((fi.x as i32 - me.0).pow(2) + (fi.y as i32 - me.1).pow(2), *id))
                            .filter(|(d, _)| *d <= 100)
                            .min()
                            .map(|(_, id)| id)
                    });
                    if let Some(id) = item {
                        self.pick_up(id);
                    }
                }
                if f.just(North) {
                    let sitting = self.player.as_ref().is_some_and(|p| p.sitting);
                    self.send(ro_net::Command::Sit(!sitting));
                }
            }
            if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
                if f.just(DPadLeft) {
                    r.camera.rotate(std::f32::consts::FRAC_PI_4, 0.0);
                }
                if f.just(DPadRight) {
                    r.camera.rotate(-std::f32::consts::FRAC_PI_4, 0.0);
                }
                if f.just(DPadUp) {
                    r.camera.zoom(1.0);
                }
                if f.just(DPadDown) {
                    r.camera.zoom(-1.0);
                }
            }
        }
        if f.just(Start) {
            self.windows.settings = !self.windows.settings;
        }
        if f.just(Select) {
            self.windows.toggle_inventory();
        }

        // Left stick: walk along the line the stick points, from where that heading was set,
        // toward a goal PAD_STEP cells further along it. Re-aimed only when the walk is
        // nearly done or the stick turns past ~25 degrees: re-planning on every wobble makes
        // each path start with the same step, drifting a heading between straight and
        // diagonal to the diagonal, and costs stutters. Measuring along the line from its
        // start (not from where we are) keeps the heading true over a long walk.
        let Some(yaw) = self.renderer.as_ref().map(|r| r.camera.yaw) else { return };
        self.pad_walk_ms -= dt;
        if f.left != glam::Vec2::ZERO {
            let (sy, cy) = yaw.sin_cos();
            let (fwd, right) = (glam::Vec2::new(-sy, cy), glam::Vec2::new(cy, sy));
            let dir = (right * f.left.x + fwd * f.left.y).normalize_or_zero();
            let Some(p) = self.player.as_ref() else { return };
            let me = p.walker.cell;
            let here = glam::Vec2::new(me.0 as f32, me.1 as f32);
            let turned = self.pad_dir.is_none_or(|d| d.dot(dir) < 0.9);
            if turned {
                self.pad_dir = Some(dir);
                self.pad_line = Some(here);
            }
            let nearly_done = !p.walker.is_walking() || p.walker.remaining() <= 2;
            if (turned || nearly_done) && self.pad_walk_ms <= 0.0 {
                self.attack = None;
                self.chasing = false;
                self.pending_npc = None;
                self.pending_skill = None;
                self.pending_pickup = None;
                if p.sitting {
                    self.send(ro_net::Command::Sit(false));
                }
                let (dir, origin) = (self.pad_dir.unwrap_or(dir), self.pad_line.unwrap_or(here));
                let along = (here - origin).dot(dir);
                let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
                let goal = (1..=PAD_STEP).rev().map(|k| origin + dir * (along + k as f32)).map(|v| (v.x.round() as i32, v.y.round() as i32)).find(|&(x, y)| (x, y) != me && gat.is_some_and(|g| g.walkable(x, y)));
                if let Some((x, y)) = goal {
                    self.request_walk((x as u16, y as u16));
                }
                self.pad_walk_ms = 100.0;
            }
        } else if self.pad_dir.take().is_some() {
            // Let go: stop on the cell being stepped into (stopping on the one we are leaving
            // would walk us back to it).
            self.pad_line = None;
            if let Some(p) = self.player.as_ref() {
                let c = p.walker.next().unwrap_or(p.walker.cell);
                self.request_walk((c.0 as u16, c.1 as u16));
            }
        }
    }

    fn stop_fx(&mut self, tag: u32) {
        if let Some(r) = self.renderer.as_mut() {
            r.effects.retain(|e| e.tag != tag);
        }
    }

    fn mob_mut(&mut self, id: u32) -> Option<&mut Mob> {
        if id == self.account_id { self.player.as_mut() } else { self.units.get_mut(&id) }
    }

    /// Someone acted on someone: animate both, and show what happened.
    fn act(&mut self, act: ro_net::Act) {
        let me = self.account_id;
        let cell_of = |app: &Self, id: u32| -> Option<(i32, i32)> {
            if id == me { app.player.as_ref().map(|p| p.walker.cell) } else { app.units.get(&id).map(|m| m.walker.cell) }
        };
        let (src_cell, target_cell) = (cell_of(self, act.src), cell_of(self, act.target));
        self.casts.remove(&act.src);
        match act.kind {
            2 | 3 => {
                if let Some(m) = self.mob_mut(act.src) {
                    m.sitting = act.kind == 2;
                }
                return;
            }
            1 => {
                tracing::debug!(src = act.src, "pick-up animation");
                // Picked something up: crouch (three frames, about 300 ms).
                if let Some(m) = self.mob_mut(act.src) {
                    m.set_pose(Pose::PickUp, 300.0);
                }
                return;
            }
            _ => {}
        }
        if let Some(m) = self.mob_mut(act.src) {
            if let Some(c) = target_cell {
                m.face(c);
            }
            m.set_pose(Pose::Attack, act.src_speed as f32);
        }
        let damage = act.damage + act.left_damage;
        // Players swing with their weapon's sound and land its hit sound; monsters' attacks
        // sound through their animation frames (see the unit update).
        if let Some(kind) = self.weapon_sound(act.src) {
            self.play_sound(&format!("_attack_{kind}.wav"));
            if damage > 0 {
                let hit = if kind == "fist" { format!("_hit_fist{}.wav", act.src % 4 + 1) } else { format!("_hit_{kind}.wav") };
                self.play_sound(&hit);
            }
        }
        if let Some(m) = self.mob_mut(act.target) {
            if damage > 0 && act.kind != 4 && act.kind != 9 {
                if let Some(c) = src_cell {
                    m.face(c);
                }
                m.set_pose(Pose::Hurt, act.dmg_speed as f32);
            }
            let pos = m.actor.pos;
            let (text, color) = match act.kind {
                11 => ("Lucky".to_string(), egui::Color32::from_rgb(140, 220, 255)),
                _ if damage <= 0 => ("Miss".to_string(), egui::Color32::from_rgb(200, 200, 200)),
                10 => (format!("{damage}!"), egui::Color32::from_rgb(255, 210, 60)),
                _ if act.target == me => (damage.to_string(), egui::Color32::from_rgb(255, 90, 90)),
                _ => (damage.to_string(), egui::Color32::WHITE),
            };
            self.floaters.push((pos, text, color, 0.0));
        }
    }

    /// Attacks a monster until it dies or we pick something else.
    fn attack_unit(&mut self, id: u32) {
        self.walk_goal = None;
        self.pending_npc = None;
        self.attack = Some((id, self.attack_range as i16));
        if let Some((tx, _)) = self.net.as_ref() {
            let _ = tx.send(ro_net::Command::Attack { target: id, keep: true });
        }
    }

    /// Applies an NPC script step to the dialog window.
    fn npc_dialog(&mut self, d: ro_net::NpcDialog) {
        use hud::DialogMode;
        use ro_net::NpcDialog::*;
        let title = |units: &std::collections::HashMap<u32, Mob>, npc: u32| units.get(&npc).map(|m| m.name.clone()).unwrap_or_default();
        let open = |dialog: &mut Option<hud::Dialog>, units: &std::collections::HashMap<u32, Mob>, npc: u32| {
            if dialog.as_ref().is_none_or(|d| d.npc != npc) {
                *dialog = Some(hud::Dialog { npc, title: title(units, npc), lines: vec![], mode: DialogMode::Reading });
            }
        };
        match d {
            Say { npc, text } => {
                open(&mut self.dialog, &self.units, npc);
                self.dialog.as_mut().unwrap().lines.push(text);
            }
            // A close with nothing on screen (touch scripts that only `close`): end it silently.
            Close { npc } if self.dialog.as_ref().is_none_or(|d| d.npc != npc || d.lines.is_empty()) => {
                self.dialog = None;
                if let Some((tx, _)) = self.net.as_ref() {
                    let _ = tx.send(ro_net::Command::Npc { npc, reply: ro_net::NpcReply::Close });
                }
            }
            Next { npc } | Close { npc } | Menu { npc, .. } | InputNumber { npc } | InputText { npc } => {
                open(&mut self.dialog, &self.units, npc);
                self.dialog.as_mut().unwrap().mode = match d {
                    Next { .. } => DialogMode::Next,
                    Close { .. } => DialogMode::Close,
                    Menu { options, .. } => DialogMode::Menu(options),
                    InputNumber { .. } => DialogMode::Number(String::new()),
                    _ => DialogMode::Text(String::new()),
                };
            }
            Clear { npc } => {
                if let Some(d) = self.dialog.as_mut().filter(|d| d.npc == npc) {
                    d.lines.clear();
                }
            }
            Cutin { image, kind } => {
                self.cutin = if kind == 255 { None } else { self.load_cutin(&image, kind) };
            }
            Align(a) => self.hud.dialog_layout.align = a,
            Size { width, height } => self.hud.dialog_layout.size = Some((width, height)),
            Pos { x, y } => self.hud.dialog_layout.pos = Some(hud::DialogPos::Pixels(x, y)),
            PosPercent { x, y } => self.hud.dialog_layout.pos = Some(hud::DialogPos::Percent(x, y)),
        }
    }

    /// Loads an NPC illustration into an egui texture.
    fn load_cutin(&self, image: &str, position: u8) -> Option<hud::Cutin> {
        let vfs = self.vfs.as_ref()?;
        let name = if image.contains('.') { image.to_string() } else { format!("{image}.bmp") };
        let path = format!("data\\texture\\유저인터페이스\\illust\\{name}");
        let data = vfs.read(&path).ok().flatten().or_else(|| {
            tracing::warn!(%path, "cutin not found");
            None
        })?;
        let img = ro_formats::image::decode(&data, &path).inspect_err(|e| tracing::warn!(%path, "cutin: {e:#}")).ok()?;
        let color = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.pixels);
        let texture = self.egui_ctx.load_texture(format!("cutin:{name}"), color, egui::TextureOptions::LINEAR);
        Some(hud::Cutin { texture, position })
    }

    /// Talks to an NPC, walking next to it first when it is out of reach.
    fn talk(&mut self, npc: u32) {
        let (Some(p), Some(m)) = (self.player.as_ref(), self.units.get(&npc)) else { return };
        let (a, b) = (p.walker.cell, m.walker.cell);
        // rAthena lets you talk within a few cells (npc_checknear).
        if (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= 3 {
            if let Some((tx, _)) = self.net.as_ref() {
                let _ = tx.send(ro_net::Command::Npc { npc, reply: ro_net::NpcReply::Contact });
            }
            return;
        }
        // Walk to the nearest walkable cell beside the NPC, then talk.
        let gat = self.world.as_ref().and_then(|w| w.gat.as_ref());
        let target = (-2..=2)
            .flat_map(|dy| (-2..=2).map(move |dx| (b.0 + dx, b.1 + dy)))
            .filter(|&(x, y)| gat.is_some_and(|g| g.walkable(x, y)))
            .min_by_key(|&(x, y)| (x - a.0).abs().max((y - a.1).abs()));
        if let Some((x, y)) = target {
            self.pending_npc = Some(npc);
            self.request_walk((x as u16, y as u16));
        }
    }

    /// The NPC or monster under the cursor, nearest to the camera first.
    fn pick_unit(&self) -> Option<u32> {
        let (r, (cx, cy)) = (self.renderer.as_ref()?, self.cursor?);
        let size = r.size();
        let eye = r.camera.eye();
        self.units
            .iter()
            .filter_map(|(&id, m)| {
                if m.actor.alpha <= 0.01 {
                    return None;
                }
                let feet = r.project(m.actor.pos, size)?;
                // Rough sprite box: about two cells tall, a little over one wide.
                let top = r.project(m.actor.pos + glam::Vec3::Y * 2.2 * ro_render::CELL, size)?;
                let h = (feet.y - top.y).max(8.0);
                let w = h * 0.55;
                let inside = (cx as f32 - feet.x).abs() <= w / 2.0 && (cy as f32) <= feet.y + h * 0.1 && (cy as f32) >= top.y;
                inside.then(|| (id, m.actor.pos.distance_squared(eye)))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Ground cell under the cursor, if any.
    fn pick(&self) -> Option<(u16, u16)> {
        let (r, w, (cx, cy)) = (self.renderer.as_ref()?, self.world.as_ref()?, self.cursor?);
        let (origin, dir) = r.ray(cx as f32, cy as f32);
        // March along the ray until it passes below what actors stand on (`floor_at`), not
        // the drawn ground: Herosria's platforms stand 30 units over it, so picking the ground
        // landed clicks cells away.
        let mut t = 0.0;
        while t < 5000.0 {
            let p = origin + dir * t;
            if p.y <= w.floor_at(p.x, p.z) {
                let (x, y) = ((p.x / ro_render::CELL).floor(), (p.z / ro_render::CELL).floor());
                return (x >= 0.0 && y >= 0.0).then_some((x as u16, y as u16));
            }
            t += 1.0;
        }
        None
    }

    /// One frame: network events, movement, camera, render (or capture).
    fn tick(&mut self, el: &ActiveEventLoop) {
        // Leave as soon as quitting is decided (the server's go-ahead arrives between window events).
        if self.quit {
            el.exit();
            return;
        }
        if self.paused {
            return;
        }
        let now = std::time::Instant::now();
        let dt = (now - self.last_tick).as_secs_f32() * 1000.0;
        self.last_tick = now;
        self.perf.frame(dt);
        if self.front.is_none() {
            self.pad_tick(dt);
            self.feedback_tick(dt);
            self.touchpad_tick(el);
        } else if let (Some(r), false) = (self.renderer.as_mut(), self.still) {
            // Behind the login screens the backdrop slowly turns.
            r.camera.rotate(dt * 0.00004, 0.0);
        }
        self.cursor_action = self.hover_cursor();
        self.steer_to_cursor(dt);
        self.companion_follow(now);

        // `--do a,b,c`: scripted actions once the inventory has arrived (tests).
        // Scripted helpers for two-client tests: accept requests, run a trade to completion.
        if self.auto_accept {
            if let Some((inviter, _)) = self.guild.ally_offer.take() {
                self.send(ro_net::Command::Relation(ro_net::RelationRequest::Answer { inviter, accept: true }));
            }
            if let Some(c) = self.social.accept_first() {
                tracing::info!(?c, "auto-accept");
                self.send(c);
            }
        }
        if let (Some(item), Some(t)) = (self.auto_trade, self.social.trade.as_ref()) {
            match self.trade_stage {
                0 => {
                    if item != 0 {
                        if let Some(index) = self.inventory.items.values().find(|i| i.id == item).map(|i| i.index) {
                            self.send(ro_net::Command::Trade(ro_net::TradeRequest::Add { index, amount: 1 }));
                        }
                    }
                    self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ok));
                    self.trade_stage = 1;
                }
                1 if t.we_locked && t.they_locked => {
                    tracing::info!(theirs = ?t.theirs, ours = ?t.ours, "trade: both locked, committing");
                    self.send(ro_net::Command::Trade(ro_net::TradeRequest::Commit));
                    self.trade_stage = 2;
                }
                _ => {}
            }
        }
        if !self.script.is_empty() && !self.inventory.items.is_empty() && self.stats.is_some() && self.ready_at.is_some_and(|t| t.elapsed().as_millis() > 1500) && self.script_resume.is_none_or(|t| std::time::Instant::now() >= t) {
            let mut steps = std::mem::take(&mut self.script).into_iter();
            while let Some(step) = steps.next() {
                // wait:ms pauses the script; the rest runs on a later frame.
                if let Some(ms) = step.strip_prefix("wait:").and_then(|m| m.parse::<u64>().ok()) {
                    self.script = steps.collect();
                    self.script_resume = Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                    break;
                }
                let item = |id: u32| self.inventory.items.values().find(|i| i.id == id).map(|i| (i.index, i.location));
                let action = match step.split_once(':').or(Some((step.as_str(), ""))) {
                    Some(("equip", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::Equip { index, location }),
                    // equipl:id puts a one-handed weapon in the left hand (dual wielding).
                    Some(("equipl", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Equip { index, location: 0x20 }),
                    // macro: apply the macro detector to ourselves (GM); captcha:text answers it.
                    Some(("macro", _)) => {
                        self.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Apply(self.account_id)));
                        None
                    }
                    Some(("captcha", a)) => {
                        self.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Answer(a.to_string())));
                        None
                    }
                    // chat:<text> says it (GM commands mid-script); petevolve:<egg> asks to evolve.
                    Some(("chat", text)) => {
                        self.submit_chat(text.to_string());
                        None
                    }
                    Some(("macroscan", r)) => {
                        if let (Ok(radius), Some(p)) = (r.parse(), self.player.as_ref()) {
                            let (x, y) = p.walker.cell;
                            self.reporter.open = true;
                            self.send(ro_net::Command::MacroReport(ro_net::MacroReport::Scan { x: x as u16, y: y as u16, radius }));
                        }
                        None
                    }
                    Some(("petevolve", egg)) => {
                        if let Ok(egg) = egg.parse() {
                            self.send(ro_net::Command::Companion(ro_net::CompanionRequest::PetEvolve(egg)));
                        }
                        None
                    }
                    // write:<text> answers a Graffiti / Talkie Box message prompt.
                    Some(("write", text)) => self.windows.skill_text.take().map(|(skill, level, cell, _)| windows::UiAction::SkillText { skill, level, cell, text: text.to_string() }),
                    Some(("use", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::UseItem(index)),
                    Some((f @ ("fav" | "unfav"), id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Favorite { index, on: f == "fav" }),
                    Some(("invtab", n)) => {
                        self.windows.inventory_tab(n.parse().unwrap_or(0));
                        None
                    }
                    Some(("card", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::CardTargets(index)),
                    // refsel:item picks equipment in the refine window; refine:material refines it.
                    // gradesel:item picks equipment in the grade window; gradeup:material tries.
                    Some(("gradesel", id)) => {
                        if let Some(index) = id.parse().ok().and_then(item).map(|(index, _)| index) {
                            self.grade.pick(index);
                            self.queued_commands.push(ro_net::Command::Grade(ro_net::GradeRequest::Select(index)));
                        }
                        None
                    }
                    Some(("gradeup", m)) => {
                        if let Some(r) = self.grade.upgrade(m.parse().unwrap_or(0)) {
                            self.queued_commands.push(ro_net::Command::Grade(r));
                        }
                        None
                    }
                    // navi:map:x:y guides there; navigo walks to the next stop.
                    Some(("navi", spec)) => {
                        let f: Vec<&str> = spec.split(':').collect();
                        self.navi.ensure_loaded(self.vfs.as_ref());
                        let cell = match (f.get(1).and_then(|x| x.parse().ok()), f.get(2).and_then(|y| y.parse().ok())) {
                            (Some(x), Some(y)) => Some((x, y)),
                            _ => None,
                        };
                        self.navi.go(f[0], cell, f[0].to_string());
                        None
                    }
                    Some(("navigo", _)) => {
                        let map = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
                        if let Some(p) = self.player.as_ref() {
                            self.navi.update(&map, (p.walker.cell.0 as u16, p.walker.cell.1 as u16));
                        }
                        self.navi_walk = self.navi.waypoint(&map);
                        None
                    }
                    // bg:Name applies solo to a battleground; bgauto enters it when ready.
                    Some(("walk", xy)) => {
                        if let Some((x, y)) = xy.split_once('_').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?))) {
                            self.request_walk((x, y));
                        }
                        None
                    }
                    Some(("lapine", _)) => {
                        let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                        if let Some(a) = self.lapine.test_go(&inv) {
                            self.lapine_action(a);
                        }
                        None
                    }
                    // cam:yaw_pitch in degrees (test hook for camera angles).
                    Some(("cam", yp)) => {
                        if let (Some((y, p)), Some(r)) = (yp.split_once('_').and_then(|(y, p)| Some((y.parse::<f32>().ok()?, p.parse::<f32>().ok()?))), self.renderer.as_mut()) {
                            r.camera.yaw = y.to_radians();
                            r.camera.pitch = p.to_radians().clamp(25f32.to_radians(), 75f32.to_radians());
                        }
                        None
                    }
                    Some(("comptab", n)) => {
                        self.companions.select_tab(n.parse().unwrap_or(0));
                        None
                    }
                    Some(("petmenu", n)) => {
                        if let Ok(n) = n.parse() {
                            self.queued_commands.push(ro_net::Command::Companion(ro_net::CompanionRequest::PetMenu(n)));
                        }
                        None
                    }
                    Some(("bg", name)) => {
                        self.battle.ensure_loaded(self.vfs.as_ref());
                        self.battle.open = true;
                        self.queued_commands.push(ro_net::Command::Bg(ro_net::BgRequest::Apply { kind: 1, name: name.to_string() }));
                        None
                    }
                    Some(("bgauto", _)) => {
                        self.battle.auto_accept = true;
                        None
                    }
                    // mbuy:row:n buys from an open market or barter shop.
                    Some(("mbuy", spec)) => {
                        let f: Vec<u32> = spec.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let (Some(row), Some(n)) = (f.first(), f.get(1)) {
                            self.market_buy = Some((*row as usize, *n));
                            if self.market.shop.is_some() {
                                self.market_test_buy();
                            }
                        }
                        None
                    }
                    Some(("emblem", f)) => {
                        self.guild.upload_emblem(f);
                        None
                    }
                    Some(("enchant", id)) => {
                        if let Some(r) = id.parse().ok().and_then(item).and_then(|(index, _)| self.enchant.test_random(index)) {
                            self.queued_commands.push(ro_net::Command::Enchant(r));
                        }
                        None
                    }
                    Some(("reform", _)) => {
                        let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                        if let Some(reform::ReformAction::Reform { tool, index }) = self.reform.test_reform(&inv) {
                            self.queued_commands.push(ro_net::Command::Reform(ro_net::ReformRequest::Reform { tool, index }));
                        }
                        None
                    }
                    Some(("style", h)) => {
                        let o = self.stylist.test_order(h.parse().unwrap_or(1));
                        self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Buy { hair_color: o.hair_color, hair: o.hair, clothes_color: o.clothes_color }));
                        None
                    }
                    Some(("ssearch", id)) => {
                        let r = self.store_search.test_search(id.parse().unwrap_or(0));
                        self.queued_commands.push(ro_net::Command::Search(r));
                        None
                    }
                    Some(("sclick", _)) => {
                        if let Some(f) = self.store_search.results.first() {
                            self.queued_commands.push(ro_net::Command::Search(ro_net::SearchRequest::Click { owner: f.owner, store: f.store, item: f.item }));
                        }
                        None
                    }
                    Some(("option", spec)) => {
                        if let Some((k, v)) = spec.split_once(':') {
                            self.queued_commands.push(ro_net::Command::Option(ro_net::OptionRequest::Config { kind: k.parse().unwrap_or(0), on: v == "1" }));
                        }
                        None
                    }
                    Some(("swadd", id)) => id.parse().ok().and_then(item).map(|(index, location)| windows::UiAction::SwitchAdd { index, location: location & 0x3ff }),
                    Some(("swap", _)) => Some(windows::UiAction::SwitchAll),
                    Some(("tame", _)) => {
                        let me = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
                        let target = self.units.iter().filter(|(_, m)| m.kind == 5 && m.dying.is_none()).min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2)).map(|(id, _)| *id);
                        if let (Some(id), true) = (target, std::mem::take(&mut self.capturing)) {
                            self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Capture(id)));
                        }
                        None
                    }
                    Some(("ally", who)) => {
                        if let Some(id) = self.units.iter().find(|(_, m)| m.name == who).map(|(id, _)| *id) {
                            self.send(ro_net::Command::Relation(ro_net::RelationRequest::Ally { target: id, me: self.account_id, char_id: self.char_id }));
                        }
                        None
                    }
                    Some(("mergeall", _)) => {
                        if let Some(list) = self.merge.open.take() {
                            let first = list.first().and_then(|i| self.inventory.items.get(i)).map(|i| i.id);
                            let same: Vec<u16> = list.into_iter().filter(|i| self.inventory.items.get(i).map(|x| x.id) == first).collect();
                            self.queued_commands.push(ro_net::Command::Merge(same));
                        }
                        None
                    }
                    Some(("equiptab", t)) => {
                        self.windows.equip_tab = t.parse().unwrap_or(0);
                        None
                    }
                    Some(("viewequip", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::ViewEquip(aid)));
                        None
                    }
                    Some(("showequip", on)) => Some(windows::UiAction::ShowEquip(on == "1")),
                    Some(("bank", _)) => {
                        self.toggle_bank();
                        None
                    }
                    Some(("deposit", z)) => {
                        z.parse().ok().map(|zeny| self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Deposit { aid: self.account_id, zeny })));
                        None
                    }
                    Some(("refsel", id)) => {
                        if let Some((index, _)) = id.parse().ok().and_then(item) {
                            self.refine.selected = Some((index, 0, vec![]));
                            self.queued_commands.push(ro_net::Command::Refine(ro_net::RefineRequest::Select(index)));
                        }
                        None
                    }
                    Some(("refine", mat)) => {
                        if let (Some((index, _, _)), Ok(material)) = (self.refine.selected.as_ref(), mat.parse()) {
                            self.queued_commands.push(ro_net::Command::Refine(ro_net::RefineRequest::Refine { index: *index, material, blessing: false }));
                        }
                        None
                    }
                    Some(("info", id)) => id.parse().ok().map(|id| windows::UiAction::ItemInfo { id, refine: 0, cards: [0; 4] }),
                    // Merchant tests: cartwin opens the cart; vend:price fills the shop
                    // set-up with everything in the cart; browse:aid looks in a shop; buyone
                    // buys one of each item in the shop being browsed.
                    Some(("cartwin", _)) => {
                        self.merchant.cart_open = true;
                        None
                    }
                    Some(("vend", price)) => {
                        let price: u32 = price.parse().unwrap_or(100);
                        if let Some((slots, title, _)) = self.merchant.setup.take() {
                            let items = self.merchant.cart.values().take(slots as usize).map(|it| (it.index, it.amount, price)).collect();
                            self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::OpenShop { title, items }));
                        }
                        None
                    }
                    // Buying store / cash shop tests: buystore:id:amount:price opens a store
                    // from the set-up; buybrowse:aid looks in one; sellall sells everything it
                    // wants that we carry; cashshop toggles the shop; cashbuy:id buys one.
                    Some(("buystore", spec)) => {
                        let f: Vec<u32> = spec.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let (Some((_, title, _, _)), [id, amount, price]) = (self.stores.setup.take(), f.as_slice()) {
                            let limit = self.hud.status.zeny.clamp(0, u32::MAX as i64) as u32;
                            self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Open { limit, title, items: vec![(*id, *amount as u16, *price)] }));
                        }
                        None
                    }
                    Some(("buybrowse", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Browse(aid))));
                        None
                    }
                    Some(("sellall", _)) => {
                        if let Some((buyer, store, _, items, _)) = self.stores.browsing.take() {
                            let list = items
                                .iter()
                                .filter_map(|w| self.inventory.items.values().find(|i| i.id == w.id && i.equipped == 0).map(|i| (i.index, w.id, w.amount.min(i.amount))))
                                .collect();
                            self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Sell { buyer, store, items: list }));
                        }
                        None
                    }
                    // mail:to:title:zeny writes and sends a mail.
                    Some(("mail", spec)) => {
                        let f: Vec<&str> = spec.split(':').collect();
                        // mail:to:title:zeny:itemid also attaches one of that item.
                        if let [to, title, zeny, rest @ ..] = f.as_slice() {
                            let [begin, check] = self.mailbox.compose(to, title, "Sent by the test script.", zeny);
                            self.send(ro_net::Command::Mail(begin));
                            if let Some(item) = rest.first().and_then(|s| s.parse::<u32>().ok()) {
                                if let Some(i) = self.inventory.items.values().find(|i| i.id == item && i.equipped == 0) {
                                    let index = i.index;
                                    self.send(ro_net::Command::Mail(ro_net::MailRequest::AddItem { index, amount: 1 }));
                                }
                            }
                            self.send(ro_net::Command::Mail(check));
                        }
                        None
                    }
                    // room:title creates a public chat room; joinroom joins the first one in sight.
                    Some(("room", title)) => {
                        self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Create { title: title.into(), limit: 10, public: true, password: String::new(), change: false }));
                        None
                    }
                    Some(("joinroom", _)) => {
                        if let Some(id) = self.rooms.signs.values().next().map(|r| r.id) {
                            self.send(ro_net::Command::ChatRoom(ro_net::ChatRoomRequest::Join { id, password: String::new() }));
                        }
                        None
                    }
                    // postad posts a party booking ad at our level minus ten.
                    Some(("postad", _)) => {
                        let level = (self.hud.status.base_level.clamp(1, 999) as u16).saturating_sub(10).max(1);
                        self.send(ro_net::Command::Booking(ro_net::BookingRequest::Register { level, map: 0, jobs: vec![] }));
                        None
                    }
                    // altkey:x presses Alt+x.
                    Some(("altkey", k)) => {
                        self.alt_key(k);
                        None
                    }
                    // screenshot takes one, as Print Screen does.
                    Some(("screenshot", _)) => {
                        self.screenshot = true;
                        None
                    }
                    // press:k presses a key over the game: a letter, F1..F12, Space or Enter.
                    Some(("press", k)) => {
                        use winit::keyboard::{Key, NamedKey};
                        let named = match k {
                            "Space" => Some(NamedKey::Space),
                            "Enter" => Some(NamedKey::Enter),
                            "Escape" => Some(NamedKey::Escape),
                            "Insert" => Some(NamedKey::Insert),
                            "F11" => Some(NamedKey::F11),
                            "F12" => Some(NamedKey::F12),
                            f => f.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()).and_then(|n| [NamedKey::F1, NamedKey::F2, NamedKey::F3, NamedKey::F4, NamedKey::F5, NamedKey::F6, NamedKey::F7, NamedKey::F8, NamedKey::F9].get(n.wrapping_sub(1)).copied()),
                        };
                        match named {
                            Some(NamedKey::Enter) => self.hud.focus_chat(),
                            Some(n) => self.key_pressed(Key::Named(n)),
                            None => self.key_pressed(Key::Character(k.into())),
                        }
                        None
                    }
                    // pad:style[:row] draws the controller UI in that style (xbox, playstation,
                    // nintendo), as if that row's trigger were held; keys:n opens Shortcut
                    // Information on tab n.
                    Some(("pad", p)) => {
                        let mut it = p.split(':');
                        let style = match it.next() {
                            Some("playstation") => padglyph::Style::PlayStation,
                            Some("nintendo") => padglyph::Style::Nintendo,
                            _ => padglyph::Style::Xbox,
                        };
                        self.pad_preview = Some((style, it.next().and_then(|r| r.parse().ok())));
                        None
                    }
                    // stick:x:y:ms holds the left stick there (-1..1, y up) for ms.
                    Some(("stick", a)) => {
                        let v: Vec<f32> = a.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let [x, y, ms] = v[..] {
                            self.test_stick = Some((glam::Vec2::new(x, y), ms));
                        }
                        None
                    }
                    // holdwalk:dx:dy:ms holds a walk target dx, dy cells from the player.
                    Some(("holdwalk", a)) => {
                        let v: Vec<f32> = a.split(':').filter_map(|v| v.parse().ok()).collect();
                        if let [dx, dy, ms] = v[..] {
                            self.hold_walk = Some(((dx as i32, dy as i32), ms));
                            self.mouse_walk_ms = 0.0;
                        }
                        None
                    }
                    // vibetest: Settings > Controls > Vibration > Test.
                    Some(("vibetest", _)) => {
                        self.settings.vibration_test = true;
                        None
                    }
                    Some(("keys", t)) => {
                        self.shortcut_win.open = true;
                        self.shortcut_win.tab = t.parse().unwrap_or(0);
                        None
                    }
                    // hover:x:y holds the UI pointer there (UI points), for tooltips.
                    Some(("hover", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        self.fake_pointer = (f.len() == 2).then(|| egui::pos2(f[0], f[1]));
                        None
                    }
                    // click:x:y clicks the UI there.
                    // drag:x1:y1:x2:y2 presses at one point, moves over a few frames, releases at the other.
                    // idle:n holds the next test drag for n frames.
                    Some(("idle", n)) => {
                        for _ in 0..n.parse().unwrap_or(0) {
                            self.fake_frames.push_back(vec![]);
                        }
                        None
                    }
                    Some(("drag", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        if f.len() == 4 {
                            let (a, b) = (egui::pos2(f[0], f[1]), egui::pos2(f[2], f[3]));
                            let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
                            self.fake_frames.push_back(vec![egui::Event::PointerMoved(a)]);
                            self.fake_frames.push_back(vec![button(a, true)]);
                            for t in [0.1, 0.3, 0.6, 0.9, 1.0, 1.0] {
                                self.fake_frames.push_back(vec![egui::Event::PointerMoved(a + (b - a) * t)]);
                            }
                            self.fake_frames.push_back(vec![button(b, false)]);
                        }
                        None
                    }
                    Some(("click", at)) => {
                        let f: Vec<f32> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        self.fake_click = (f.len() == 2).then(|| (egui::pos2(f[0], f[1]), true));
                        None
                    }
                    // rclick:x:y right-clicks the world there (window pixels).
                    Some(("rclick", at)) => {
                        let f: Vec<f64> = at.split(':').filter_map(|v| v.parse().ok()).collect();
                        if f.len() == 2 {
                            self.cursor = Some((f[0], f[1]));
                            self.right_button(true);
                            self.right_button(false);
                        }
                        None
                    }
                    Some(("trait", i)) => i.parse().ok().filter(|i: &usize| *i < 6).map(windows::UiAction::RaiseTrait),
                    Some(("identify", id)) => id.parse().ok().and_then(|id: u32| self.inventory.items.values().find(|i| i.id == id && !i.identified)).map(|i| windows::UiAction::Identify(i.index)),
                    Some(("revive", _)) => {
                        self.send(ro_net::Command::AutoRevive);
                        None
                    }
                    Some(("exit", _)) => {
                        self.request_exit();
                        None
                    }
                    Some(("cashshop", _)) => {
                        self.toggle_cash_shop();
                        None
                    }
                    // talk:Name talks to that NPC once it is in view; pointbuy:item:amount buys
                    // from the cashshop / itemshop / pointshop NPC that is open.
                    Some(("talk", name)) => {
                        self.talk_to = Some(name.to_string());
                        None
                    }
                    Some(("shopclose", _)) => {
                        self.social.shop = None;
                        self.send(ro_net::Command::ShopClosed);
                        None
                    }
                    Some(("pointbuy", spec)) => {
                        if let Some((id, n)) = spec.split_once(':').and_then(|(i, n)| Some((i.parse().ok()?, n.parse().ok()?))) {
                            self.send(ro_net::Command::Shop(ro_net::ShopRequest::CashBuy { kafra: 0, items: vec![(id, n)] }));
                        }
                        None
                    }
                    Some(("cashtab", n)) => {
                        self.stores.cash_tab = n.parse().unwrap_or(0);
                        None
                    }
                    // salefind:name / saleadd:item:amount:hours / saledel:item drive the GM sale tool.
                    Some(("salefind", name)) => {
                        self.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Search(name.to_string())));
                        None
                    }
                    // agency: open the Adventurer's Agency; agencyadd:purpose:min:max:memo registers
                    // our party; agencyjoin:name asks that ad's leader to let us in.
                    Some(("agency", _)) => {
                        if !self.agency.open {
                            self.agency.toggle();
                        }
                        None
                    }
                    Some(("agencyadd", spec)) => {
                        let mut f = spec.splitn(4, ':');
                        let mut num = || f.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                        let (purpose, min_lv, max_lv) = (num(), num(), num());
                        let memo = f.next().unwrap_or("").to_string();
                        let name = self.hud.status.name.clone();
                        self.agency.register(agency::Ad { gid: self.char_id, char_name: name, purpose, min_lv, max_lv, dealer: 1, memo, ..Default::default() });
                        None
                    }
                    Some(("partyleave", _)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Leave));
                        None
                    }
                    Some(("agencyjoin", name)) => {
                        if let Some(ad) = self.agency.find(name) {
                            self.send(ro_net::Command::Party(ro_net::PartyRequest::AgencyJoin { char_id: ad.gid, account_id: ad.aid }));
                        }
                        None
                    }
                    Some(("saleadd", spec)) => {
                        let v: Vec<u32> = spec.split(':').filter_map(|s| s.parse().ok()).collect();
                        if let [item, amount, hours] = v[..] {
                            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as u32;
                            self.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Add { item, amount, start: now, hours: hours as u16 }));
                        }
                        None
                    }
                    Some(("saledel", item)) => {
                        if let Ok(item) = item.parse() {
                            self.send(ro_net::Command::Sale(self.account_id, ro_net::SaleRequest::Remove(item)));
                        }
                        None
                    }
                    Some(("cashbuy", id)) => {
                        let id: u32 = id.parse().unwrap_or(0);
                        if let Some(tab) = self.stores.cash_tabs.iter().find(|(_, items)| items.iter().any(|(i, _)| *i == id)).map(|(t, _)| *t) {
                            self.queued_commands.push(ro_net::Command::CashShop(ro_net::CashShopRequest::Buy { kafra: 0, items: vec![(id, 1, tab)] }));
                        }
                        None
                    }
                    Some(("expand", "")) => Some(windows::UiAction::ExpandInventory),
                    Some(("expand", "yes")) => Some(windows::UiAction::ExpandConfirm(true)),
                    Some(("offall", mask)) => u32::from_str_radix(mask, 16).ok().map(windows::UiAction::UnequipAll),
                    Some(("browse", aid)) => {
                        aid.parse().ok().map(|aid| self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Browse(aid))));
                        None
                    }
                    Some(("buyone", _)) => {
                        if let Some((seller, shop, items, _)) = self.merchant.browsing.take() {
                            let list = items.iter().map(|i| (1, i.index)).collect();
                            self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Buy { seller, shop, items: list }));
                        }
                        None
                    }
                    // pick:id answers the open menu with that item/skill id (or inventory index).
                    Some(("pick", id)) => {
                        let id: u32 = id.parse().unwrap_or(0);
                        self.windows.menu.take().and_then(|menu| {
                            use ro_net::{Menu, MenuReply};
                            let reply = match menu {
                                Menu::Items(ids) => ids.contains(&id).then_some(MenuReply::Item(id)),
                                Menu::Make { kind, items } => items.contains(&id).then_some(MenuReply::Make { kind, item: id }),
                                Menu::Produce(list) => list.iter().find(|(i, _)| *i == id).map(|(i, m)| MenuReply::Produce { item: *i, materials: *m }),
                                Menu::AutoSpell(s) => s.contains(&id).then_some(MenuReply::AutoSpell(id)),
                                Menu::Skill { flag, skills } => skills.contains(&(id as u16)).then_some(MenuReply::Skill { flag, skill: id as u16 }),
                                Menu::Identify(list) => {
                                    let by_item = self.inventory.items.values().find(|i| i.id == id && !i.identified && list.contains(&i.index)).map(|i| i.index);
                                    Some(MenuReply::Identify(by_item.unwrap_or(id as u16)))
                                }
                                Menu::Repair(list) => list.iter().find(|r| r.1 == id).map(|&(index, item, refine)| MenuReply::Repair { index, item, refine }),
                                Menu::WeaponRefine(list) => list.iter().find(|r| r.1 == id).map(|r| MenuReply::WeaponRefine(r.0)),
                                Menu::Cart { aid, looks } => looks.contains(&(id as u8)).then_some(MenuReply::Cart { aid, look: id as u8 }),
                                // The whole stack of that item.
                                Menu::Materials(kind) => self.inventory.items.values().find(|i| i.id == id).map(|i| MenuReply::Materials { kind, items: vec![(i.index, i.amount)] }),
                                Menu::Egg(_) => Some(MenuReply::Egg(self.inventory.items.values().find(|i| i.id == id).map_or(id as u16, |i| i.index))),
                                Menu::Warp { skill, maps } => maps.get(id as usize).map(|m| MenuReply::Warp { skill, map: m.clone() }),
                            };
                            reply.map(windows::UiAction::Menu)
                        })
                    }
                    Some(("raise", n)) => n.parse().ok().map(windows::UiAction::RaiseStat),
                    Some(("skillup", id)) => id.parse().ok().map(windows::UiAction::SkillUp),
                    // hotkey:slot=skill_id (at its learned level), key:slot presses it.
                    Some(("hotkey", spec)) => spec.split_once('=').and_then(|(slot, id)| {
                        let (index, id) = (slot.parse().ok()?, id.parse::<u16>().ok()?);
                        let level = self.skills.iter().find(|s| s.id == id)?.level as i16;
                        Some(windows::UiAction::SetHotkey { index, key: ro_net::Hotkey { skill: true, id: id as u32, count: level } })
                    }),
                    Some(("key", slot)) => slot.parse().ok().map(windows::UiAction::UseHotkey),
                    // selfcast:skill_id on ourselves.
                    Some(("selfcast", id)) => {
                        if let Ok(skill) = id.parse::<u16>() {
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_on(skill, level, self.account_id);
                        }
                        None
                    }
                    // ground:skill_id[:dx] on the cell dx (default 2) east of us.
                    Some(("ground", spec)) => {
                        let (id, dx) = spec.split_once(':').map_or((spec, 2), |(id, dx)| (id, dx.parse().unwrap_or(2)));
                        if let (Ok(skill), Some(p)) = (id.parse::<u16>(), self.player.as_ref()) {
                            let cell = ((p.walker.cell.0 + dx) as u16, p.walker.cell.1 as u16);
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_at(skill, level, cell);
                        }
                        None
                    }
                    // cast:skill_id on the nearest monster.
                    // attack: like --attack, start attacking the nearest monster.
                    Some(("attack", _)) => {
                        self.auto_attack = true;
                        None
                    }
                    Some(("cast", id)) => {
                        let me = self.player.as_ref().map_or((0, 0), |p| p.walker.cell);
                        let target = self
                            .units
                            .iter()
                            .filter(|(_, m)| m.kind == 5)
                            .min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2))
                            .map(|(id, _)| *id);
                        tracing::info!(?target, monsters = self.units.values().filter(|m| m.kind == 5).count(), "script cast {id}");
                        if let (Ok(skill), Some(target)) = (id.parse::<u16>(), target) {
                            let level = self.skills.iter().find(|s| s.id == skill).map_or(1, |s| s.level.max(1));
                            self.use_skill_on(skill, level, target);
                        }
                        None
                    }
                    Some(("party", name)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Create { name: name.into() }));
                        None
                    }
                    Some(("invite", name)) => {
                        self.send(ro_net::Command::Party(ro_net::PartyRequest::Invite { name: name.into() }));
                        None
                    }
                    Some(("guild", name)) => {
                        self.send(ro_net::Command::Guild(ro_net::GuildRequest::Create { char_id: self.char_id, name: name.into() }));
                        None
                    }
                    Some(("friend", name)) => {
                        self.send(ro_net::Command::Friend(ro_net::FriendRequest::Add { name: name.into() }));
                        None
                    }
                    Some(("trade", name)) => {
                        let id = self.units.iter().find(|(_, m)| m.name == name).map(|(id, _)| *id);
                        if let Some(target) = id {
                            self.trade_partner = Some(name.into());
                            self.send(ro_net::Command::Trade(ro_net::TradeRequest::Ask { target }));
                        }
                        None
                    }
                    Some(("drop", id)) => id.parse().ok().and_then(item).map(|(index, _)| windows::UiAction::Drop { index, amount: 1 }),
                    _ if step == "charselect" => {
                        self.send(ro_net::Command::Restart(1));
                        None
                    }
                    _ if step == "restart" => {
                        self.send(ro_net::Command::Restart(0));
                        None
                    }
                    _ if step == "pickup" => {
                        self.auto_pickup = true;
                        None
                    }
                    _ => None,
                };
                tracing::info!(%step, ?action, "script");
                if let Some(a) = action {
                    self.ui_action(a);
                }
            }
        }
        if self.auto_pickup {
            let me = self.player.as_ref().map(|p| p.walker.cell);
            let nearest = me.and_then(|a| self.floor.values().min_by_key(|(f, _)| (f.x as i32 - a.0).abs().max((f.y as i32 - a.1).abs())).map(|(f, _)| f.id));
            if let Some(id) = nearest {
                self.auto_pickup = false;
                tracing::info!(id, "picking up");
                self.pick_up(id);
            }
        }
        if std::mem::take(&mut self.open_windows) {
            let args: Vec<String> = std::env::args().collect();
            self.quests.open = args.iter().any(|a| a == "--quest-win");
            if args.iter().any(|a| a == "--skills-win") { self.windows.skills = true; self.windows.skill_list = args.iter().any(|a| a == "--skill-list"); self.windows.skill_tab = args.iter().skip_while(|a| *a != "--guild-tab").nth(1).and_then(|t| t.parse().ok()); } else if args.iter().any(|a| a == "--equip-win") { self.windows.equipment = true; } else if args.iter().any(|a| a == "--status-win") { self.windows.stats = true; } else { self.windows.guild = true; }
            self.guild.start_tab = std::env::args().skip_while(|a| a != "--guild-tab").nth(1).and_then(|t| t.parse().ok()).unwrap_or(0);
        }
        self.update_emblems();
        if self.quit_asked.is_some_and(|t| t.elapsed().as_secs() >= 3) {
            tracing::warn!("no answer to the exit request; leaving");
            self.quit = true;
        }
        for text in std::mem::take(&mut self.queued_chat) {
            self.submit_chat(text);
        }
        for c in std::mem::take(&mut self.queued_commands) {
            self.send(c);
        }
        if let Some(a) = self.queued_action.take() {
            self.ui_action(a);
        }
        let events: Vec<_> = self.net.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
        for e in events {
            self.handle(e);
        }

        // Chasing an out-of-reach target: attack again once close enough (or arrived).
        if self.chasing {
            if let (Some((target, range)), Some(p)) = (self.attack, self.player.as_ref()) {
                let near = self.units.get(&target).is_some_and(|m| {
                    let (a, b) = (p.walker.cell, m.walker.cell);
                    (a.0 - b.0).abs().max((a.1 - b.1).abs()) <= range as i32
                });
                if near || (self.walk_goal.is_none() && !p.walker.is_walking()) {
                    self.chasing = false;
                    self.attack_unit(target);
                }
            } else {
                self.chasing = false;
            }
        }
        self.units.retain(|_, m| m.dying.is_none_or(|t| t > 0.0));
        self.casts.retain(|_, (t, total)| {
            *t += dt;
            *t < *total
        });
        self.floaters.retain_mut(|f| {
            f.3 += dt;
            f.3 < 1200.0
        });
        // Script progress bars: ours tells the server when it fills, so the script resumes.
        // Wall-clock timed, and reported a little late: the server takes a report that
        // arrives before its own timer runs out as a cancel and ends the script.
        if let Some((start, total)) = self.script_progress {
            if start.elapsed().as_secs_f32() * 1000.0 >= total + 150.0 {
                self.script_progress = None;
                if let Some((tx, _)) = self.net.as_ref() {
                    tracing::debug!("script progress bar filled");
                    let _ = tx.send(ro_net::Command::ProgressDone);
                }
            }
        }
        self.unit_progress.retain(|_, (t, total)| {
            *t += dt;
            *t < *total
        });
        // showscript call-outs stay up a few seconds.
        self.shouts.retain(|_, (_, t)| {
            *t += dt;
            *t < 4000.0
        });
        if let Some((kind, _, t)) = self.digit.as_mut() {
            *t += dt;
            // Type 0 shows for 5 s; the counters run until replaced or the map changes.
            if *kind == 0 && *t > 5000.0 {
                self.digit = None;
            }
        }

        if self.auto_attack {
            let me = self.player.as_ref().map(|p| p.walker.cell);
            let nearest = me.and_then(|a| {
                self.units
                    .iter()
                    .filter(|(_, m)| m.kind == 5 && m.dying.is_none())
                    .min_by_key(|(_, m)| (m.walker.cell.0 - a.0).abs().max((m.walker.cell.1 - a.1).abs()))
                    .map(|(id, _)| *id)
            });
            if let Some(id) = nearest {
                self.auto_attack = false;
                self.attack_unit(id);
            }
        }
        if let Some(name) = self.talk_to.clone() {
            if let Some(id) = self.units.iter().find(|(_, m)| m.name.eq_ignore_ascii_case(&name)).map(|(id, _)| *id) {
                self.talk_to = None;
                self.talk(id);
            }
        }

        // Pick up the item we were walking to once we have arrived.
        if self.pending_pickup.is_some() && self.walk_goal.is_none() && self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
            if let Some(id) = self.pending_pickup.take() {
                tracing::debug!(id, "pickup request");
            self.send(ro_net::Command::Item(ro_net::ItemRequest::PickUp { floor_id: id }));
            }
        }

        // Talk to the NPC we were walking to once we have arrived.
        if self.pending_npc.is_some() && self.walk_goal.is_none() && self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
            if let Some(npc) = self.pending_npc.take() {
                self.talk(npc);
            }
        }
        // `--fx skill:<id>`: replay a skill's effect on the player every 1.5 seconds (a
        // timer rather than waiting for quiet, which maps with lasting effects never are).
        if let Some(id) = self.fx_test.and_then(|n| n.strip_prefix("skill:")).and_then(|n| n.parse::<u16>().ok()) {
            let now = std::time::Instant::now();
            if self.player.is_some() && self.fx_test_next.is_none_or(|t| now >= t) {
                self.fx_test_next = Some(now + std::time::Duration::from_millis(1500));
                let pos = self.unit_pos(self.account_id).or_else(|| self.player.as_ref().map(|p| p.actor.pos));
                if let (137, Some(p)) = (id, pos) {
                    self.spike_line(p, p + glam::Vec3::new(6.0 * ro_render::CELL, 0.0, -3.0 * ro_render::CELL));
                }
                if let (7015, Some(p)) = (id, pos) {
                    self.crescent_gale(self.account_id, p, p + glam::Vec3::new(6.0 * ro_render::CELL, 0.0, -3.0 * ro_render::CELL));
                }
                self.skill_fx(id, pos, true);
            }
        }
        // Cast the skill we walked into range for (re-walks if the target moved).
        if let (Some((skill, level, target, _)), None) = (self.pending_skill, self.walk_goal.as_ref()) {
            if self.player.as_ref().is_some_and(|p| !p.walker.is_walking()) {
                if self.units.contains_key(&target) {
                    self.use_skill_on(skill, level, target);
                } else {
                    self.pending_skill = None;
                }
            }
        }

        // Continue a long walk once the current hop has finished (or the server ignored it).
        let idle = self.player.as_ref().is_some_and(|p| !p.walker.is_walking());
        if idle && self.walk_goal.as_ref().is_some_and(|g| g.sent_at.is_none_or(|t| t.elapsed().as_millis() > 500)) {
            self.next_hop();
        }

        self.update_room();
        if let Some(a) = self.companion_action.take() {
            match a {
                // With a script running, commands go to it (as Alt+right-click does in the
                // original): ATTACK_OBJECT_CMD 3, FOLLOW_CMD 9.
                companions::CompanionAction::Send(r) => {
                    if r == ro_net::CompanionRequest::MercMenu(2) {
                        // The server says nothing on a dismissal.
                        self.companions.merc = None;
                        self.companions.merc_skills.clear();
                        self.merc_ai = None;
                    }
                    let msg = match r {
                        ro_net::CompanionRequest::Attack { id, target } => Some((id, vec![3, target as i64])),
                        ro_net::CompanionRequest::Return(id) => Some((id, vec![9])),
                        _ => None,
                    };
                    let brain = msg.as_ref().and_then(|(id, _)| if *id == self.companions.homun_id { self.homun_ai.as_ref() } else { self.merc_ai.as_ref() });
                    match (brain, msg) {
                        (Some(b), Some((_, m))) => b.world.borrow_mut().msg = m,
                        _ => self.send(ro_net::Command::Companion(r)),
                    }
                }
                companions::CompanionAction::Skill { id, level, on } => self.use_skill_on(id, level, on),
                companions::CompanionAction::SkillUp(id) => self.send(ro_net::Command::Stat(ro_net::StatRequest::SkillUp(id))),
            }
        }
        if let Some(key) = self.hud.menu_click.take().or_else(|| self.windows.menu_key.take()) {
            self.alt_key(key);
        }
        if self.windows.equipment || self.stylist.open {
            self.refresh_portrait();
        }
        if let (Some(w), Some(p)) = (self.world.as_ref(), self.player.as_ref()) {
            let (map, me) = (w.name.clone(), (p.walker.cell.0 as u16, p.walker.cell.1 as u16));
            self.navi.update(&map, me);
            self.battle.map_changed(&map);
        }
        // Hat effects follow their wearer; they end when the wearer leaves view.
        if !self.hat_fx.is_empty() {
            let placed: Vec<(u32, Option<glam::Vec3>)> = self.hat_fx.iter().map(|((unit, _), tag)| (*tag, self.unit_pos(*unit))).collect();
            let gone: std::collections::HashSet<u32> = placed.iter().filter(|(_, p)| p.is_none()).map(|(t, _)| *t).collect();
            self.hat_fx.retain(|_, tag| !gone.contains(tag));
            if let Some(r) = self.renderer.as_mut() {
                for (tag, pos) in placed {
                    match pos {
                        Some(p) => r.effects.iter_mut().filter(|e| e.tag == tag).for_each(|e| e.pos = p),
                        None => r.effects.retain(|e| e.tag != tag),
                    }
                }
            }
        }
        if let Some(cell) = self.navi_walk.take() {
            self.request_walk(cell);
        }
        // The unit and the floor item under the cursor (a monster's or an item's name shows only then).
        let hovered = self.pick_unit();
        let hovered_item = self.pick_floor_item();
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        r.actors.clear();
        r.models.clear();
        r.models.extend(self.traps.values().flatten().cloned());
        r.warps = self
            .warps
            .values()
            .filter_map(|&(x, y)| {
                let w = self.world.as_ref()?;
                if !w.shows_cell(x as i32, y as i32) {
                    return None;
                }
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (wx, wz) = (fx * ro_render::CELL, fy * ro_render::CELL);
                let h = w.floor_at(wx, wz);
                Some(glam::Vec3::new(wx, h, wz))
            })
            .collect();
        r.auras.clear();
        if let Some(w) = self.world.as_ref() {
            let octant = (r.camera.yaw / std::f32::consts::FRAC_PI_4).round() as i32;
            // Aura textures: the level aura's as they are, custom auras' whitened to be tinted.
            let auras = match self.vfs.as_ref() {
                Some(vfs) => {
                    let mut set = |white: bool| {
                        let mut get = |n: &'static str| if white { self.fx.texture_white(r, vfs, n) } else { self.fx.texture(r, vfs, n) };
                        Some([get("ring_blue.tga")?, get("pikapika2.bmp")?, get("whitelight.tga")?])
                    };
                    let (level, white) = (set(false), set(true));
                    let disc = self.fx.fx_texture(r, vfs, fx::FxTexture::Disc);
                    AuraTextures { level, white, disc, level_off: self.settings.aura_off, simple: self.settings.aura_simple }
                }
                None => AuraTextures::default(),
            };
            self.hat_auras.retain(|id, _| *id == self.account_id || self.units.contains_key(id));
            if let Some(p) = self.player.as_mut() {
                // Combat stance (weapon drawn) while attacking something.
                let idle = if self.attack.is_some() && self.action == 0 { 4 } else { self.action };
                p.attack_group = attack_group(r, &p.actor);
                p.fit_pose(r);
                let a = p.update(dt, w, octant, idle);
                tracing::trace!(x = a.pos.x / ro_render::CELL, z = a.pos.z / ro_render::CELL, dt, cell = ?p.walker.cell, next = ?p.walker.next(), "walk trace");
                r.camera.target = a.pos;
                let level = self.hud.status.base_level >= AURA_LEVEL as i64;
                p.aura_ms = auras.push(r, self.hat_auras.get(&self.account_id).copied(), level, &a, p.aura_ms + dt, self.account_id);
                r.actors.push(a);
            }
            for (&id, m) in self.units.iter_mut() {
                let (action, time) = (m.actor.action, m.actor.time_ms);
                m.attack_group = attack_group(r, &m.actor);
                m.fit_pose(r);
                let a = m.update(dt, w, octant, 0);
                // Attack, hurt and death frames carry their sounds in the ACT.
                if let (Some(_), Some(audio), Some(vfs)) = (m.pose, self.audio.as_ref(), self.vfs.as_ref()) {
                    let from = if a.action == action && time > 0.0 && a.time_ms >= time { time } else { -1.0 };
                    if let Some(s) = r.actor_sound(&a, from) {
                        audio.play(vfs, &s);
                    }
                }
                let level = m.kind == 0 && m.level >= AURA_LEVEL;
                let custom = self.hat_auras.get(&id).copied().or_else(|| saga::look(m.class).and_then(|l| l.aura));
                m.aura_ms = auras.push(r, custom, level, &a, m.aura_ms + dt, id);
                if let Some(Some(model)) = self.models3d.get(&m.class).filter(|_| a.alpha > 0.1 && w.shows_cell(m.walker.cell.0, m.walker.cell.1)) {
                    r.models.extend(model.draws(&a, m.walker.dir as usize));
                }
                r.actors.push(a);
            }
            // Ambient map effects (RSW emitters, EF_* ids): torches loop in place; bubbles and
            // fireflies fire again on each emitter's own timer while the player is near.
            if let (Some(vfs), Some(p)) = (self.vfs.as_ref(), self.player.as_ref()) {
                let me = p.actor.pos;
                for e in &mut self.map_fx {
                    let near = e.pos.distance(me) < fx::MAP_FX_RANGE * ro_render::CELL;
                    match e.visual {
                        Some(fx::MapVisual::Sprite(name, lift)) => {
                            e.clock_ms += dt;
                            if let (true, Some(body)) = (w.shows_cell((e.pos.x / ro_render::CELL) as i32, (e.pos.z / ro_render::CELL) as i32), self.fx.sprite(r, vfs, name)) {
                                r.actors.push(ro_render::Actor { pos: e.pos + glam::Vec3::Y * lift, body, head: None, headgear: vec![], weapon: vec![], shadow: None, action: 0, dir: ro_render::Dir::S, time_ms: e.clock_ms, once: false, alpha: 1.0, scale: 1.0, tint: [1.0; 3] });
                            }
                        }
                        Some(visual) => {
                            e.clock_ms -= dt;
                            if e.clock_ms > 0.0 {
                                continue;
                            }
                            e.clock_ms = e.every_ms;
                            if !near {
                                continue;
                            }
                            e.fired += 1;
                            match visual {
                                fx::MapVisual::Str(names) => {
                                    if let Some(asset) = self.fx.get(r, vfs, names[(e.fired as usize * 7) % names.len()]) {
                                        r.effects.push(ro_render::Fx { asset, pos: e.pos, time_ms: 0.0, looping: false, tag: 0 });
                                    }
                                }
                                _ => {
                                    if let Some(tex) = self.fx.texture(r, vfs, "pok1.bmp") {
                                        let a = e.fired as f32 * 2.399;
                                        let drift = glam::Vec3::new(a.cos(), 0.25, a.sin()) * ro_render::CELL * 0.8;
                                        let shape = ro_render::Shape::Motes { drift, size: 1.6 };
                                        r.procs.push(ro_render::ProcFx { tex, shape, pos: e.pos, color: [0.85, 0.95, 0.8], time_ms: 0.0, duration_ms: e.every_ms.clamp(1500.0, 4000.0) });
                                    }
                                }
                            }
                        }
                        None => {}
                    }
                }
                // Underwater, the player breathes out a few bubbles now and then.
                if w.submerged {
                    self.breath_ms -= dt;
                    if self.breath_ms <= 0.0 {
                        self.breath_ms = 2600.0 + (me.x * 7.3 + me.z * 3.1).rem_euclid(1400.0);
                        if let Some(asset) = self.fx.get(r, vfs, "bubble1") {
                            r.effects.push(ro_render::Fx { asset, pos: me + glam::Vec3::new(0.0, ro_render::CELL * 2.0, 0.0), time_ms: 0.0, looping: false, tag: 0 });
                        }
                    }
                }
            }
            if let (Some(name), true, Some(vfs), Some(p)) = (self.fx_test.filter(|n| !n.starts_with("skill:")), r.effects.is_empty(), self.vfs.as_ref(), self.player.as_ref()) {
                if let Some(asset) = self.fx.get(r, vfs, name) {
                    r.effects.push(ro_render::Fx { asset, pos: p.actor.pos, time_ms: 0.0, looping: false, tag: 0 });
                }
            }
            // Skill effects run once, then go.
            for e in &mut r.effects {
                e.time_ms += dt;
            }
            // Ground effects loop until the server removes them.
            let durations: Vec<f32> = r.effects.iter().map(|e| r.fx_duration(e.asset)).collect();
            for (e, d) in r.effects.iter_mut().zip(&durations) {
                if e.looping && *d > 0.0 {
                    e.time_ms %= d;
                }
            }
            let mut alive = durations.into_iter();
            r.effects.retain(|e| alive.next().is_some_and(|d| e.looping || e.time_ms < d));
            for p in &mut r.procs {
                p.time_ms += dt;
            }
            r.procs.retain(|p| p.time_ms < p.duration_ms);
            for (a, left) in &mut self.sprite_fx {
                a.time_ms += dt;
                *left -= dt;
            }
            self.sprite_fx.retain(|(_, left)| *left > 0.0);
            r.actors.extend(self.sprite_fx.iter().map(|(a, _)| a.clone()));
            // Emotes over heads (sprite\이팩트\emotion, one action per ET_* id).
            if let (false, Some(vfs)) = (self.emotes.is_empty(), self.vfs.as_ref()) {
                if let Some(sheet) = self.fx.sprite(r, vfs, "emotion") {
                    for (id, (kind, t)) in &mut self.emotes {
                        *t += dt;
                        let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) };
                        let action = emotes::action(*kind);
                        let Some((m, action)) = m.zip(action).filter(|&(_, a)| *t < r.action_ms(sheet, a).max(1500.0)) else {
                            *t = f32::MAX;
                            continue;
                        };
                        r.actors.push(ro_render::Actor {
                            pos: m.actor.pos + glam::Vec3::Y * ro_render::CELL * 2.8,
                            body: sheet,
                            head: None,
                            headgear: vec![],
                            weapon: vec![],
                            shadow: None,
                            action: action / 8,
                            dir: ro_render::Dir::from_index(action),
                            time_ms: *t,
                            once: true,
                            alpha: 1.0, scale: 1.0, tint: [1.0; 3],
                        });
                    }
                    self.emotes.retain(|_, (_, t)| *t < f32::MAX);
                }
            }
            // Items on the ground: their sprite at the cell plus sub-cell offset.
            for (f, actor) in self.floor.values() {
                if let Some(mut a) = actor.clone() {
                    let (fx, fy) = floor_cell(f);
                    let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
                    let y = w.floor_at(x, z);
                    a.pos = glam::Vec3::new(x, y, z);
                    r.actors.push(a);
                }
            }
        }

        r.post_settings = self.settings.visual.post();
        if self.settings.visual.render_auto {
            // About a million world pixels, in the slider's 5% steps (shown there).
            let (w, h) = r.size();
            let auto = ((1.0e6 / (w as f32 * h as f32).max(1.0)).sqrt().clamp(0.5, 1.0) * 20.0).floor() / 20.0;
            self.settings.visual.render_scale = auto;
        }
        r.set_render_scale(self.settings.visual.render_scale);
        // Catmull-Rom on magnified ground and model textures (the sprite shader never reads it).
        r.upscale_runtime = self.settings.upscale.runtime_bicubic();
        r.underwater = if self.world.as_ref().is_some_and(|w| w.submerged) { self.settings.visual.underwater } else { 0.0 };
        r.set_water_settings(self.settings.visual.water_settings());
        // HUD: names under actors, status, chat. Laid out for the capture size when capturing.
        let size = self.capture.as_ref().map_or_else(|| r.size(), |(_, w, h)| (*w, *h));
        let mut labels = vec![];
        for (uid, m) in self.player.iter().map(|p| (self.account_id, p)).chain(self.units.iter().map(|(i, m)| (*i, m))) {
            let shown = m.actor.alpha > 0.01 && self.world.as_ref().is_none_or(|w| w.shows_cell(m.walker.cell.0, m.walker.cell.1));
            if let (true, Some(color), Some(feet)) = (shown, m.label_color(hovered == Some(uid)), r.project(m.actor.pos, size)) {
                // As the original: "Name (Party)", then "Guild [Rank]" on a second line.
                let mut text = m.name.clone();
                if let Some((party, guild, rank)) = &m.titles {
                    if !party.is_empty() {
                        text = format!("{text} ({party})");
                    }
                    if !guild.is_empty() {
                        text = if rank.is_empty() { format!("{text}
{guild}") } else { format!("{text}
{guild} [{rank}]") };
                    }
                }
                let emblem = (m.guild != 0).then(|| self.emblems.get(&m.guild).cloned().flatten()).flatten().map(|t| t.id());
                let (orbs, orb_kind) = self.spirits.get(&uid).copied().unwrap_or((0, 0));
                labels.push(hud::Label { feet, text, color, emblem, orbs, orb_kind });
            }
        }
        // A floor item's name, as the original: "Jellopy: 3 ea." (msgstringtable 183).
        if let Some((f, w)) = hovered_item.and_then(|id| self.floor.get(&id)).map(|(f, _)| f).zip(self.world.as_ref()) {
            let (fx, fy) = floor_cell(f);
            let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
            let y = w.floor_at(x, z);
            if let Some(feet) = r.project(glam::Vec3::new(x, y, z), size) {
                let name = self.item_db.as_ref().map_or_else(|| format!("Item #{}", f.item), |d| d.name(f.item, f.identified));
                let amount = f.amount.max(1).to_string();
                let text = msgstr::get(183, &[&name, &amount]).unwrap_or_else(|| format!("{name}: {amount} ea."));
                labels.push(hud::Label { feet, text, color: egui::Color32::WHITE, emblem: None, orbs: 0, orb_kind: 0 });
            }
        }
        let floaters: Vec<_> = self.floaters.iter().filter_map(|(pos, text, color, age)| Some((r.project(*pos, size)?, text.clone(), *color, *age))).collect();
        let bars: Vec<_> = self
            .units
            .values()
            .filter(|m| m.dying.is_none())
            .filter_map(|m| {
                let (hp, max) = m.hp?;
                Some((r.project(m.actor.pos, size)?, hp as f32 / max.max(1) as f32))
            })
            .collect();
        // The controller's target, marked over its head.
        let pad_marker = self.pad_target.filter(|_| self.pad.in_use()).and_then(|t| self.units.get(&t)).and_then(|m| r.project(m.actor.pos, size));
        // Cast bars over whoever is chanting.
        // Shop signs over vending players.
        let shop_signs: Vec<(u32, glam::Vec2)> = self
            .merchant
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        let room_signs: Vec<(u32, glam::Vec2)> = self
            .rooms
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        let buy_signs: Vec<(u32, glam::Vec2)> = self
            .stores
            .signs
            .keys()
            .filter_map(|id| { let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?; Some((*id, r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.2, size)?)) })
            .collect();
        // Quest marks over NPC heads (screen pixels).
        let quest_marks: Vec<(glam::Vec2, u16, u16)> = self
            .quests
            .marks
            .iter()
            .filter_map(|(id, &(mark, color))| Some((r.project(self.units.get(id)?.actor.pos + glam::Vec3::Y * ro_render::CELL * 3.0, size)?, mark, color)))
            .collect();
        let cast_bars: Vec<_> = self
            .casts
            .iter()
            .filter_map(|(id, &(t, total))| {
                let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) }?;
                Some((r.project(m.actor.pos, size)?, t / total))
            })
            // Script progress bars draw like cast bars (green, as the original).
            .chain(self.script_progress.map(|(start, total)| (self.account_id, start.elapsed().as_secs_f32() * 1000.0 / total)).into_iter().chain(self.unit_progress.iter().map(|(id, (t, total))| (*id, t / total))).filter_map(|(id, frac)| {
                let m = if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }?;
                Some((r.project(m.actor.pos, size)?, frac))
            }))
            .collect();
        let says: Vec<(glam::Vec2, String)> = self
            .shouts
            .iter()
            .filter_map(|(id, (text, _))| {
                let m = if *id == self.account_id { self.player.as_ref() } else { self.units.get(id) };
                // Talkie Boxes are ground units, not actors.
                let pos = m.map(|m| m.actor.pos).or_else(|| cell_pos_in(self.world.as_ref()?, *self.ground_units.get(id)?))?;
                Some((r.project(pos, size)?, text.clone()))
            })
            .collect();
        let scrawls: Vec<(glam::Vec2, String)> =
            self.graffiti.iter().filter_map(|(id, text)| Some((r.project(cell_pos_in(self.world.as_ref()?, *self.ground_units.get(id)?)?, size)?, text.clone()))).collect();
        self.hud.scrawls = scrawls;
        // Devotion links run chest to chest, from the Crusader to each protected unit.
        let chest = |id: u32| {
            let m = if id == self.account_id { self.player.as_ref() } else { self.units.get(&id) }?;
            r.project(m.actor.pos + glam::Vec3::Y * ro_render::CELL * 1.2, size)
        };
        let chest = &chest;
        let beams = self.devotions.iter().flat_map(|(&src, targets)| targets.iter().filter_map(move |&t| Some((chest(src)?, chest(t)?)))).collect();
        self.hud.beams = beams;
        self.hud.digit_text = self.digit.map(|(kind, value, t)| digit_text(kind, value, t));
        self.hud.pvp_text = self.pvp_rank.map(|(rank, total)| format!("{rank} / {total}"));
        if let Some(state) = self.egui_state.as_mut() {
            // Full screen follows the setting (F11 or the settings window).
            if self.capture.is_none() && window.fullscreen().is_some() != self.settings.fullscreen {
                window.set_fullscreen(self.settings.fullscreen.then_some(winit::window::Fullscreen::Borderless(None)));
            }
            let mut input = state.take_egui_input(window);
            if let Some(p) = self.fake_pointer.take() {
                input.events.push(egui::Event::PointerMoved(p));
            }
            if let Some(events) = self.fake_frames.pop_front() {
                input.events.extend(events);
            }
            if let Some((p, pressed)) = self.fake_click.take() {
                input.events.push(egui::Event::PointerMoved(p));
                input.events.push(egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
                if pressed {
                    self.fake_click = Some((p, false));
                }
            }
            let ppp = window.scale_factor() as f32 * self.ui_scale;
            input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, size.1 as f32) / ppp));
            // Android: the on-screen keyboard shrinks the activity's content rect, not the
            // window (NativeActivity's adjustResize), so the interface fits above it: the login
            // form, a dialog or the chat being typed in stays in view. The world still fills
            // the screen.
            #[cfg(target_os = "android")]
            {
                use winit::platform::android::WindowExtAndroid;
                let visible = window.content_rect().bottom;
                if visible > 0 && (visible as u32) < size.1 {
                    input.screen_rect = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size.0 as f32, visible as f32) / ppp));
                }
            }
            self.egui_ctx.set_pixels_per_point(ppp);
            let mut sent = None;
            let mut answer = None;
            let mut ui_action = None;
            if self.skin.is_none() && self.settings.classic_ui {
                if let Some(vfs) = self.vfs.as_ref() {
                    self.skin = Some(classic::Skin::load(&self.egui_ctx, vfs));
                }
            }
            self.hud.classic_bar = self.skin.as_ref().filter(|_| self.settings.classic_ui).and_then(|s| s.texture("dialog_bg")).cloned();
            classic::install(&self.egui_ctx, self.skin.as_ref().filter(|_| self.settings.classic_ui));
            let guild_name = self.guild.name.clone();
            let items: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
            let skill_names = |n: &str| self.skill_names.display(n);
            // A copy: the UI below may change the settings while the view is held.
            let battle_keys = self.settings.battle_keys.clone();
            let view = windows::View {
                portrait: self.portrait.as_ref().and_then(|p| p.1.as_ref()),
                skin: self.skin.as_ref().filter(|_| self.settings.classic_ui),
                guild_name: Some(guild_name.as_str()),
                items: &items,
                db: self.item_db.as_ref(),
                icons: &self.icons,
                unid_icons: &self.unid_icons,
                skill_icons: &self.skill_icons,
                stats: self.stats.as_ref(),
                fourth_job: jobs::is_fourth(self.hud.status.job_id),
                skills: &self.skills,
                skill_names: &skill_names,
                skill_info: Some(&self.skill_names),
                skill_tree: Some(&self.skill_tree),
                job: self.hud.status.job_id,
                skill_points: self.hud.status.skill_points,
                weight: (self.hud.status.weight, self.hud.status.max_weight),
                zeny: self.hud.status.zeny,
                screen_width: size.0 as f32 / (window.scale_factor() as f32 * self.ui_scale),
                hotkeys: &self.hotkeys,
                hotkey_row: self.hotkey_row,
                hotkey_rows: self.settings.hotkey_rows,
                battle_mode: self.settings.battle_mode,
                battle_keys: &battle_keys,
                cooldowns: &self.cooldowns,
                has_cart: self.merchant.has_cart(),
                show_equip: self.show_equip,
                pad: self.pad_preview.map(|p| p.0).or_else(|| self.pad.in_use().then(|| self.settings.pad_style.resolve(self.pad.detected))),
                pad_row: self.pad_preview.and_then(|p| p.1).or(self.pad_row),
            };
            let my_name = self.hud.status.name.clone();
            let social_ctx = social::Ctx {
                db: self.item_db.as_ref(),
                icons: &self.icons,
                inventory: &items,
                zeny: self.hud.status.zeny,
                my_name: &my_name,
            };
            let mut social_cmds = vec![];
            let mut front_action = None;
            let mut player_cmd = None;
            let party: std::collections::HashSet<&str> = self.social.party.iter().flat_map(|p| p.members.iter().map(|m| m.name.as_str())).collect();
            let mut dots: Vec<_> = self
                .units
                .values()
                .filter(|m| m.dying.is_none() && self.world.as_ref().is_none_or(|w| w.shows_cell(m.walker.cell.0, m.walker.cell.1)))
                .filter_map(|m| {
                    let color = match m.kind {
                        0 if party.contains(m.name.as_str()) => egui::Color32::from_rgb(255, 130, 200),
                        0 => egui::Color32::from_rgb(120, 180, 255),
                        5 => egui::Color32::from_rgb(230, 60, 50),
                        k if is_npc(k) => egui::Color32::from_rgb(90, 220, 90),
                        _ => return None,
                    };
                    Some(((m.walker.cell.0 as f32, m.walker.cell.1 as f32), color))
                })
                .collect();
            dots.extend(self.member_pos.iter().filter(|(id, _)| !self.units.contains_key(id) && **id != self.account_id).map(|(_, (c, guild))| (*c, if *guild { egui::Color32::from_rgb(170, 255, 170) } else { egui::Color32::from_rgb(255, 130, 200) })));
            dots.extend(self.battle.team_dots().into_iter().map(|c| (c, egui::Color32::from_rgb(255, 160, 200))));
            let me = self.player.as_ref().map(|p| (p.walker.cell.0 as f32, p.walker.cell.1 as f32));
            let map_name = self.world.as_ref().map(|w| w.name.clone()).unwrap_or_default();
            if self.settings.perf_overlay && self.perf.vram_due() {
                self.perf.vram = r.vram();
            }
            let perf_lines = self.settings.perf_overlay.then(|| {
                let p = &self.perf;
                let (w, h) = r.size();
                let at = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
                vec![
                    format!("{:.0} fps  {:.1} ms (slowest {:.1})", p.fps, p.avg, p.worst),
                    format!("build {:.1} ms  draw {:.1} ms", p.cpu, p.render),
                    self.ping.map_or("ping -".to_string(), |ms| if ms < 10.0 { format!("ping {ms:.1} ms") } else { format!("ping {ms:.0} ms") }),
                    format!("{map_name} {},{}  {} units  {} drawn", at.0, at.1, self.units.len(), r.actors.len()),
                    match r.scene_size() {
                        (sw, sh) if (sw, sh) != (w, h) => format!("{w}x{h} (world {sw}x{sh})  {}", r.adapter()),
                        _ => format!("{w}x{h}  {}", r.adapter()),
                    },
                    p.vram.map_or("GPU memory -".to_string(), |(used, reserved)| format!("GPU memory {} MB ({} MB reserved)", used >> 20, reserved >> 20)),
                ]
            });
            let out = self.egui_ctx.run_ui(input, |ui| {
                if let Some(lines) = &perf_lines {
                    self.perf.show(ui.ctx(), lines);
                }
                // The game cursor on every screen (its own top layer, so draw order does not matter).
                if let (Some(c), Some((x, y)), false) = (self.cursor_sprite.as_ref(), self.cursor, self.screenshot) {
                    let time = ui.ctx().input(|i| i.time);
                    let action = if self.front.is_some() { hud::cursor::ARROW } else { self.cursor_action };
                    c.draw(ui.ctx(), egui::pos2(x as f32 / ppp, y as f32 / ppp), action, time);
                }
                // Start-up splash (age rating), then the title art behind the login screens.
                if let (Some(s), true) = (self.splash.as_mut(), self.front.is_some()) {
                    if s.intro(ui.ctx()) {
                        return;
                    }
                    s.title(ui.ctx());
                }
                // Loading screen over a map being entered (fades once it is in).
                if let (Some(s), None) = (self.splash.as_mut(), self.front.as_ref()) {
                    if s.loading(ui.ctx(), self.world.is_some() && self.player.is_some()) {
                        return;
                    }
                }
                // Login and character screens replace the game UI until a character is picked.
                if let Some(front) = self.front.as_mut() {
                    let tables = &self.look_tables;
                    let accessory = |v: u16| tables.accessory(v as u32).map(str::to_string);
                    let fctx = screens::Ctx { vfs: self.vfs.as_ref(), job_title: &job_title, job_sprite: &job_sprite, accessory: &accessory };
                    front_action = front.show(ui.ctx(), &fctx);
                    return;
                }
                sent = self.hud.show(ui.ctx(), &labels, &floaters, &bars, &cast_bars, &says);
                // Item links in chat: names for the ones it met, and the one clicked.
                for id in std::mem::take(&mut self.hud.missing_links) {
                    let named = self.item_db.as_ref().and_then(|d| d.get(id)).map(|i| (i.name.clone(), i.slots));
                    self.hud.link_names.insert(id, named.unwrap_or_else(|| (format!("Item #{id}"), 0)));
                }
                // Links in NPC dialogs: <NAVI> plans the route in the Navigation window, as the original.
                match self.hud.dialog_link.take() {
                    Some(npcmarkup::Action::Navi { map, x, y }) => {
                        self.navi.ensure_loaded(self.vfs.as_ref());
                        let label = self.navi.map_name(&map).map_or_else(|| format!("{map} {x}, {y}"), |n| format!("{n} {x}, {y}"));
                        self.navi.go(&map, Some((x, y)), label);
                    }
                    Some(npcmarkup::Action::Url(url)) => self.egui_ctx.open_url(egui::OpenUrl::new_tab(url)),
                    Some(npcmarkup::Action::Item(link)) => self.hud.clicked_link = Some(link),
                    Some(npcmarkup::Action::Tip(_)) | None => {}
                }
                if let (Some(link), Some(vfs)) = (self.hud.clicked_link.take(), self.vfs.as_ref()) {
                    self.item_info.show_item(&self.egui_ctx, vfs, self.item_db.as_ref(), link.id, link.refine, link.cards);
                }
                if let (true, Some(map), Some(me)) = (self.settings.minimap, self.minimap.as_ref(), me) {
                    {
                        let now = std::time::Instant::now();
                        self.viewpoints.retain(|_, v| v.2.is_none_or(|t| t > now));
                        let mut marks: Vec<_> = self.viewpoints.values().map(|v| (v.0, v.1)).collect();
                        marks.extend(self.navi.waypoint(&map_name).map(|(x, y)| ((x as f32, y as f32), egui::Color32::from_rgb(255, 80, 220))));
                        hud::show_minimap(ui.ctx(), map, &map_name, me, &dots, &marks);
                    }
                }
                if let Some(vfs) = self.vfs.as_ref() {
                    self.buffs.show(ui.ctx(), vfs, if self.settings.minimap && self.minimap.is_some() { 270.0 } else { 60.0 });
                }
                if let Some(vfs) = self.vfs.as_ref() {
                    let ppp = ui.ctx().pixels_per_point();
                    let painter = ui.ctx().layer_painter(egui::LayerId::background());
                    for (p, mark, color) in &quest_marks {
                        if let Some(tex) = self.quests.mark_icon(ui.ctx(), vfs, *mark, *color) {
                            let rect = egui::Rect::from_center_size(egui::pos2(p.x / ppp, p.y / ppp), tex.ui_size() * 2.0);
                            painter.image(tex.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                    }
                }
                if let Some((id, refine, cards)) = self.item_info.show_viewing(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    if let Some(vfs) = self.vfs.as_ref() {
                        self.item_info.show_item(ui.ctx(), vfs, self.item_db.as_ref(), id, refine, cards);
                    }
                }
                self.item_info.show(ui.ctx(), self.item_db.as_ref());
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                if let Some(req) = self.refine.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny) {
                    self.queued_commands.push(ro_net::Command::Refine(req));
                }
                let (bank_move, bank_closed) = self.merchant.show_bank(ui.ctx(), self.hud.status.zeny);
                let aid = self.account_id;
                if let Some(z) = bank_move {
                    let req = if z > 0 { ro_net::BankRequest::Deposit { aid, zeny: z as i32 } } else { ro_net::BankRequest::Withdraw { aid, zeny: (-z) as i32 } };
                    self.queued_commands.push(ro_net::Command::Bank(req));
                }
                if bank_closed {
                    self.queued_commands.push(ro_net::Command::Bank(ro_net::BankRequest::Close(aid)));
                }
                let zeny = self.hud.status.zeny;
                if let Some(req) = self.merchant.show(ui.ctx(), self.item_db.as_ref(), &self.icons, zeny) {
                    self.queued_commands.push(ro_net::Command::Merchant(req));
                }
                if let Some(aid) = self.merchant.show_signs(ui.ctx(), &shop_signs) {
                    self.queued_commands.push(ro_net::Command::Merchant(ro_net::MerchantRequest::Browse(aid)));
                }
                if let Some(aid) = self.stores.show_signs(ui.ctx(), &buy_signs) {
                    self.queued_commands.push(ro_net::Command::BuyingStore(ro_net::BuyingStoreRequest::Browse(aid)));
                }
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                if let Some(req) = self.stores.show_buying(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, zeny) {
                    self.queued_commands.push(ro_net::Command::BuyingStore(req));
                }
                self.stores.sale_badges = self.stores.cash_tabs.get(&8).into_iter().flatten().filter_map(|(id, _)| Some((*id, self.sales.badge(*id)?))).collect();
                let shown = if self.settings.classic_ui { self.stores.show_cash_classic(ui.ctx(), self.item_db.as_ref(), &self.icons) } else { self.stores.show_cash_modern(ui.ctx(), self.item_db.as_ref(), &self.icons) };
                if let Some(req) = shown {
                    self.queued_commands.push(ro_net::Command::CashShop(req));
                }
                if std::mem::take(&mut self.windows.cash_toggle) {
                    let req = if self.stores.cash.take().is_some() { ro_net::CashShopRequest::Close } else { ro_net::CashShopRequest::Open };
                    self.queued_commands.push(ro_net::Command::CashShop(req));
                }
                self.windows.cash_shown = self.stores.cash.is_some();
                if std::mem::take(&mut self.windows.menu_toggled) {
                    self.settings.menu_collapsed = self.windows.menu_collapsed;
                    self.settings.save();
                } else {
                    self.windows.menu_collapsed = self.settings.menu_collapsed;
                }
                let target = self.attack.map(|a| a.0).or_else(|| { let me = self.player.as_ref()?.walker.cell; self.units.iter().filter(|(_, m)| m.kind == 5).filter(|(_, m)| (m.walker.cell.0 - me.0).abs().max((m.walker.cell.1 - me.1).abs()) <= 14).min_by_key(|(_, m)| (m.walker.cell.0 - me.0).pow(2) + (m.walker.cell.1 - me.1).pow(2)).map(|(id, _)| *id) });
                self.sense.show(ui.ctx());
                // What our pet can evolve into, with the materials we hold.
                self.companions.evolutions = match (&self.companions.pet, self.companions.open) {
                    (Some(p), true) => {
                        let name = |id: u32| self.item_db.as_ref().map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
                        let have = |id: u32| self.inventory.items.values().filter(|i| i.id == id).map(|i| i.amount as u32).sum::<u32>();
                        self.pet_evo.for_class(p.class).iter().map(|(egg, mats)| (*egg, name(*egg), mats.iter().map(|&(m, n)| (name(m), have(m), n)).collect())).collect()
                    }
                    _ => vec![],
                };
                if let Some(a) = self.companions.show(ui.ctx(), target) {
                    self.companion_action = Some(a);
                }
                let mailable: Vec<(u16, u32, u16)> = {
                    let mut v: Vec<_> = self.inventory.items.values().filter(|i| i.equipped == 0).map(|i| (i.index, i.id, i.amount)).collect();
                    v.sort_unstable();
                    v
                };
                self.clan.show(ui.ctx());
                if let Some(v) = self.merge.show(ui.ctx(), &self.inventory.items, self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Merge(v));
                }
                if let Some((inviter, accept)) = self.guild.show_offer(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Relation(ro_net::RelationRequest::Answer { inviter, accept }));
                }
                self.reputation.show(ui.ctx());
                let db = self.item_db.as_ref();
                self.guild.show_storage_log(ui.ctx(), |id| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true)));
                {
                    let me = self.hud.status.name.clone();
                    let leader = self.social.party.as_ref().is_some_and(|p| p.members.iter().any(|m| m.leader && m.name == me));
                    self.game_opts.gm_look = self.gm.then(|| self.player.as_ref().is_some_and(|p| p.font == GM_LOOK_ON));
                    for r in self.game_opts.show(ui.ctx(), leader) {
                        self.queued_commands.push(ro_net::Command::Option(r));
                    }
                    if let Some(on) = self.game_opts.gm_look_asked.take() {
                        self.queued_chat.push(format!("@gmlook {}", if on { "on" } else { "off" }));
                    }
                }
                if let Some(k) = self.ranking.show(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Ranking(k));
                }
                for r in self.store_search.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Search(r));
                }
                {
                    let base = self.portrait.as_ref().map(|p| p.0.clone());
                    match self.stylist.show(ui.ctx(), self.vfs.as_ref(), base) {
                        Some(Some(o)) => self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Buy { hair_color: o.hair_color, hair: o.hair, clothes_color: o.clothes_color })),
                        Some(None) => self.queued_commands.push(ro_net::Command::Style(ro_net::StyleRequest::Close)),
                        None => {}
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    if let Some(a) = self.lapine.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons) {
                        self.queued_commands.push(lapine_command(a));
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    if let Some(a) = self.reform.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons) {
                        self.queued_commands.push(ro_net::Command::Reform(match a {
                            reform::ReformAction::Reform { tool, index } => ro_net::ReformRequest::Reform { tool, index },
                            reform::ReformAction::Close => ro_net::ReformRequest::Close,
                        }));
                    }
                }
                {
                    let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                    let reqs = self.enchant.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny);
                    self.queued_commands.extend(reqs.into_iter().map(ro_net::Command::Enchant));
                }
                {
                    let inv = &self.inventory;
                    let have = |id: u32| inv.items.values().filter(|i| i.id == id && i.equipped == 0).map(|i| i.amount as u32).sum::<u32>();
                    let reqs = self.market.show(ui.ctx(), self.item_db.as_ref(), &self.icons, self.hud.status.zeny, &have);
                    self.queued_commands.extend(reqs.into_iter().map(ro_net::Command::Market));
                }
                let mut macros_changed = false;
                self.shortcut_win.show(
                    ui.ctx(),
                    shortcuts::Ctx { battle_mode: self.settings.battle_mode, battle_keys: &self.settings.battle_keys, battle_space_chat: self.settings.battle_space_chat, pad: self.pad_preview.map_or_else(|| self.settings.pad_style.resolve(self.pad.detected), |p| p.0) },
                );
                if let Some(line) = self.emote_win.show(ui.ctx(), self.vfs.as_ref(), &mut self.settings.macros, &mut macros_changed) {
                    self.queued_chat.push(line);
                }
                if macros_changed {
                    self.settings.save();
                }
                let (bg_reqs, castle_reqs) = self.battle.show(ui.ctx(), self.hud.status.base_level.max(0) as u32);
                self.queued_commands.extend(bg_reqs.into_iter().map(ro_net::Command::Bg).chain(castle_reqs.into_iter().map(ro_net::Command::Castle)));
                if let Some(navigation::NaviAction::Walk(x, y)) = self.navi.show(ui.ctx(), &map_name) {
                    self.navi_walk = Some((x, y));
                }
                let inv: Vec<&ro_game::items::InvItem> = self.inventory.items.values().collect();
                for r in self.grade.show(ui.ctx(), &inv, self.item_db.as_ref(), &self.icons, self.hud.status.zeny) {
                    self.queued_commands.push(ro_net::Command::Grade(r));
                }
                for req in self.mailbox.show(ui.ctx(), self.item_db.as_ref(), &self.icons, self.hud.status.zeny, &mailable) {
                    self.queued_commands.push(ro_net::Command::Mail(req));
                }
                let me = self.hud.status.name.clone();
                for req in self.rooms.show(ui.ctx(), &me).into_iter().chain(self.rooms.show_signs(ui.ctx(), &room_signs)) {
                    self.queued_commands.push(ro_net::Command::ChatRoom(req));
                }
                let my_level = self.hud.status.base_level.clamp(1, 999) as u16;
                if std::mem::take(&mut self.social.open_agency) {
                    self.agency.toggle();
                }
                for line in self.agency.poll() {
                    self.hud.push_chat(ro_net::ChatKind::System, line);
                }
                let leader = self.social.party.as_ref().map(|p| p.members.iter().any(|m| m.leader && m.char_id == self.char_id));
                let agency_me = agency::Me { char_id: self.char_id, name: &me, level: my_level, leader };
                for a in self.agency.show(ui.ctx(), &agency_me) {
                    match a {
                        agency::AgencyAction::Join { char_id, account_id } => {
                            self.queued_commands.push(ro_net::Command::Party(ro_net::PartyRequest::AgencyJoin { char_id, account_id }));
                        }
                        agency::AgencyAction::Whisper(name) => self.hud.set_input(format!("/w \"{name}\" ")),
                        agency::AgencyAction::Notice(text) => self.hud.push_chat(ro_net::ChatKind::System, text),
                    }
                }
                let job_name = |j: u16| jobs::title(j as i16).to_string();
                for a in self.booking.show(ui.ctx(), my_level, &job_name) {
                    match a {
                        booking::BookingAction::Send(r) => self.queued_commands.push(ro_net::Command::Booking(r)),
                        booking::BookingAction::Whisper(name) => self.hud.set_input(format!("/w \"{name}\" ")),
                    }
                }
                if let Some(req) = self.progress.show(ui.ctx(), self.item_db.as_ref(), &self.icons) {
                    self.queued_commands.push(ro_net::Command::Progress(req));
                }
                if let Some(req) = self.progress.show_attendance(ui.ctx()) {
                    self.queued_commands.push(ro_net::Command::Progress(req));
                }
                if self.quests.open {
                    self.navi.ensure_loaded(self.vfs.as_ref());
                }
                let navi_names = &self.navi;
                if let Some((quest, active)) = self.quests.show(ui.ctx(), &|m| navi_names.map_name(m).map(str::to_string)) {
                    self.queued_commands.push(ro_net::Command::QuestActive { quest, active });
                }
                if let Some((map, cell, label)) = self.quests.navigate.take() {
                    self.navi.ensure_loaded(self.vfs.as_ref());
                    self.navi.go(&map, cell, label);
                }
                if self.esc_menu {
                    match hud::esc_menu(ui.ctx()) {
                        Some(hud::EscChoice::CharSelect) => {
                            self.esc_menu = false;
                            self.queued_commands.push(ro_net::Command::Restart(1));
                        }
                        Some(hud::EscChoice::Options) => {
                            self.esc_menu = false;
                            self.windows.settings = true;
                        }
                        Some(hud::EscChoice::Shortcuts) => {
                            self.esc_menu = false;
                            self.shortcut_win.open = true;
                        }
                        // As request_exit (self is borrowed here): ask the server before leaving.
                        Some(hud::EscChoice::Exit) => {
                            self.esc_menu = false;
                            if self.net.is_some() {
                                self.queued_commands.push(ro_net::Command::Quit);
                                self.quit_asked = Some(std::time::Instant::now());
                            } else {
                                self.quit = true;
                            }
                        }
                        Some(hud::EscChoice::Back) => self.esc_menu = false,
                        None => {}
                    }
                }
                if self.dead {
                    let hint = self.pad_preview.map(|p| p.0).or_else(|| self.pad.in_use().then(|| self.settings.pad_style.resolve(self.pad.detected)));
                    // A Token of Siegfried in any of its forms, or Light of Regene, revives in place.
                    const TOKENS: [u32; 4] = [7621, 6293, 6316, 25310];
                    let token = self.inventory.items.values().find(|i| TOKENS.contains(&i.id) && i.equipped == 0);
                    let revive = token
                        .map(|t| self.item_db.as_ref().map_or_else(|| "Token of Siegfried".to_string(), |db| db.name(t.id, true)))
                        .or_else(|| self.buffs.has("EFST_LIGHT_OF_REGENE").then(|| "Light of Regene".to_string()));
                    match hud::death_window(ui.ctx(), hint, revive.as_deref()) {
                        Some(hud::DeathChoice::SavePoint) => self.queued_commands.push(ro_net::Command::Restart(0)),
                        Some(hud::DeathChoice::Revive) => self.queued_commands.push(ro_net::Command::AutoRevive),
                        None => {}
                    }
                }
                if let Some(feet) = pad_marker {
                    hud::target_marker(ui.ctx(), feet, ppp, self.aiming.is_some());
                }
                // Arrange: the layout editor takes over from the settings window.
                if std::mem::take(&mut self.settings.arrange) {
                    self.arrange.open = true;
                    self.windows.settings = false;
                }
                if self.arrange.show(ui.ctx(), &mut self.settings.hud_layout) {
                    self.settings.save();
                }
                hudlayout::apply(ui.ctx(), &self.settings.hud_layout);
                self.settings.pad_live = self.pad.connected().then_some(self.pad.raw);
                if self.windows.settings && self.settings.show(ui.ctx(), &mut self.windows.settings) {
                    self.ui_scale = self.settings.ui_scale;
                    uitex::set_upscaler(ro_render::Upscaler::new(self.settings.upscale.clone()));
                    if let Some(a) = self.audio.as_mut() {
                        a.set_bgm_volume(if self.settings.bgm_off { 0.0 } else { self.settings.bgm_volume });
                        a.sfx_volume = if self.settings.sfx_off { 0.0 } else { self.settings.sfx_volume };
                    }
                }
                self.books.show(ui.ctx());
                if let (Some(a), Some((tx, _))) = (self.captcha.show(ui.ctx()), self.net.as_ref()) {
                    let _ = tx.send(ro_net::Command::Captcha(ro_net::CaptchaRequest::Answer(a)));
                }
                let (units, my_id, my_name) = (&self.units, self.account_id, self.hud.status.name.clone());
                let me = self.player.as_ref().map(|p| p.walker.cell);
                let name_of = |id: u32| if id == my_id { my_name.clone() } else { units.get(&id).map_or_else(|| format!("Player #{id}"), |m| m.name.clone()) };
                if let Some(act) = self.reporter.show(ui.ctx(), name_of) {
                    let req = match act {
                        captcha::ReporterAction::Scan(radius) => me.map(|(x, y)| ro_net::MacroReport::Scan { x: x as u16, y: y as u16, radius }),
                        captcha::ReporterAction::Apply(id) => Some(ro_net::MacroReport::Apply(id)),
                    };
                    if let (Some(req), Some((tx, _))) = (req, self.net.as_ref()) {
                        let _ = tx.send(ro_net::Command::MacroReport(req));
                    }
                }
                let db = self.item_db.as_ref();
                if let Some(req) = self.sales.show_tool(
ui.ctx(), &self.icons, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true))) {
                    if let Some((tx, _)) = self.net.as_ref() {
                        let _ = tx.send(ro_net::Command::Sale(self.account_id, req));
                    }
                }
                let db = self.item_db.as_ref();
                if let Some(act) = self.roulette.show(
ui.ctx(), &self.icons, |id| db.map_or_else(|| format!("item {id}"), |d| d.name(id, true))) {
                    let r = match act {
                        roulette::Action::Spin => ro_net::RouletteRequest::Spin,
                        roulette::Action::Claim => ro_net::RouletteRequest::Claim,
                        roulette::Action::Close => ro_net::RouletteRequest::Close,
                    };
                    if let Some((tx, _)) = self.net.as_ref() {
                        let _ = tx.send(ro_net::Command::Roulette(r));
                    }
                }
                if self.memorial.show(ui.ctx()) {
                    if let Some((tx, _)) = self.net.as_ref() {
                        let _ = tx.send(ro_net::Command::MemorialDestroy);
                    }
                }
                answer = self.hud.show_dialog(ui.ctx(), &mut self.dialog, self.cutin.as_ref());
                ui_action = self.windows.show(ui.ctx(), &view);
                self.social.show_party = self.windows.party;
                self.social.show_friends = self.windows.friends;
                social_cmds = self.social.show(ui.ctx(), &social_ctx);
                // Guild window (menu button / Alt+G).
                if self.windows.guild != self.guild.open {
                    self.guild.toggle();
                }
                let guild_ctx = guild::Ctx {
                    account_id: self.account_id,
                    char_id: self.char_id,
                    has_emperium: items.iter().any(|i| i.id == guild::EMPERIUM),
                    job_name: &job_title,
                };
                social_cmds.extend(self.guild.show(ui.ctx(), &guild_ctx));
                self.windows.guild = self.guild.open;
                (self.windows.party, self.windows.friends) = (self.social.show_party, self.social.show_friends);
                // Menu for a clicked player.
                if let Some((id, name, pos)) = self.player_menu.clone() {
                    let mut keep = true;
                    egui::Area::new(egui::Id::unique("player menu")).fixed_pos(pos).show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label(egui::RichText::new(&name).strong());
                            if ui.button("Trade").clicked() {
                                player_cmd = Some((ro_net::Command::Trade(ro_net::TradeRequest::Ask { target: id }), Some(name.clone())));
                                keep = false;
                            }
                            if ui.button("View equipment").clicked() {
                                player_cmd = Some((ro_net::Command::ViewEquip(id), None));
                                keep = false;
                            }
                            if ui.button("Invite to party").clicked() {
                                player_cmd = Some((ro_net::Command::Party(ro_net::PartyRequest::Invite { name: name.clone() }), None));
                                keep = false;
                            }
                            if ui.button("Add friend").clicked() {
                                player_cmd = Some((ro_net::Command::Friend(ro_net::FriendRequest::Add { name: name.clone() }), None));
                                keep = false;
                            }
                            if ui.button("Adopt").on_hover_text("A married couple can adopt a player as their child").clicked() {
                                player_cmd = Some((ro_net::Command::Adopt(ro_net::AdoptRequest::Ask(id)), None));
                                keep = false;
                            }
                            if self.guild.id != 0 && ui.button("Invite to guild").clicked() {
                                player_cmd = Some((ro_net::Command::Guild(ro_net::GuildRequest::Invite { name: name.clone() }), None));
                                keep = false;
                            }
                            // Guild masters: offer an alliance, or mark the other guild an enemy.
                            let other_guild = self.units.get(&id).map_or(0, |m| m.guild);
                            if self.guild.master && other_guild != 0 && other_guild != self.guild.id {
                                if ui.button("Ask for an alliance").clicked() {
                                    player_cmd = Some((ro_net::Command::Relation(ro_net::RelationRequest::Ally { target: id, me: self.account_id, char_id: self.char_id }), None));
                                    keep = false;
                                }
                                if ui.button("Set guild as enemy").clicked() {
                                    player_cmd = Some((ro_net::Command::Relation(ro_net::RelationRequest::Enemy(id)), None));
                                    keep = false;
                                }
                            }
                            // Game Masters (config `gm`): the original's GM right-click actions.
                            if self.gm {
                                ui.separator();
                                let gm = |r| Some((ro_net::Command::Gm(r), None));
                                let kick = msgstr::get(127, &[]).unwrap_or_else(|| "Kick Character Out".into());
                                for (label, r) in [
                                    (kick.as_str(), ro_net::GmRequest::Kick(id)),
                                    ("Mute for 60 minutes", ro_net::GmRequest::Mute(name.clone())),
                                    ("Remove all equipment", ro_net::GmRequest::FullStrip(id)),
                                    ("Recall here", ro_net::GmRequest::RecallId(id)),
                                    ("Go to", ro_net::GmRequest::GoTo(id)),
                                    ("Check stats", ro_net::GmRequest::Check(name.clone())),
                                ] {
                                    if ui.button(label).clicked() {
                                        player_cmd = gm(r);
                                        keep = false;
                                    }
                                }
                            }
                            if ui.button("Close").clicked() {
                                keep = false;
                            }
                        });
                    });
                    if !keep {
                        self.player_menu = None;
                    }
                }
                if let Some((skill, _)) = self.aiming {
                    let name = self.skills.iter().find(|s| s.id == skill).map(|s| self.skill_names.display(&s.name)).unwrap_or_default();
                    egui::Area::new(egui::Id::unique("aiming")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 16.0)).show(ui.ctx(), |ui| {
                        ui.label(egui::RichText::new(format!("{name}: choose a target (Esc to cancel)")).size(15.0).color(egui::Color32::from_rgb(255, 220, 120)));
                    });
                }
            });
            drop(view);
            // Handled next tick: the renderer is borrowed here.
            if ui_action.is_some() {
                self.queued_action = ui_action;
            }
            if let Some((cmd, partner)) = player_cmd {
                self.trade_partner = partner.or(self.trade_partner.take());
                social_cmds.push(cmd);
            }
            self.queued_commands.extend(social_cmds);
            match front_action {
                Some(screens::Action::Send(cmd)) => self.queued_commands.push(cmd),
                Some(screens::Action::Logout) => {
                    // A fresh session back at the login screen.
                    let remember = self.front.as_ref().is_some_and(|f| f.remember);
                    let user = self.front.as_ref().map(|f| f.user().to_string()).unwrap_or_default();
                    self.front = Some(screens::Front::new(user, remember));
                    if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_ref()) {
                        a.title(v);
                    }
                    self.net = Some(ro_net::spawn_session(self.session.clone()));
                }
                Some(screens::Action::Quit) => self.quit = true,
                None => {}
            }
            if answer.is_none() {
                if let Some(d) = self.dialog.as_mut().filter(|d| !matches!(d.mode, hud::DialogMode::Reading)) {
                    if let Some(a) = self.npc_answers.pop_front() {
                        tracing::info!(answer = %a, "npc answer");
                        let reply = match a.split_once(':') {
                            Some(("menu", n)) => ro_net::NpcReply::Menu(n.parse().unwrap_or(1)),
                            Some(("num", n)) => ro_net::NpcReply::Number(n.parse().unwrap_or(0)),
                            Some(("text", t)) => ro_net::NpcReply::Text(t.to_string()),
                            _ if a == "close" => ro_net::NpcReply::Close,
                            _ => ro_net::NpcReply::Next,
                        };
                        let npc = d.npc;
                        match reply {
                            ro_net::NpcReply::Close => self.dialog = None,
                            ro_net::NpcReply::Next | ro_net::NpcReply::Menu(_) => {
                                d.mode = hud::DialogMode::Reading;
                                d.lines.clear();
                            }
                            _ => d.mode = hud::DialogMode::Reading,
                        }
                        answer = Some((npc, reply));
                    }
                }
            }
            if let (Some((npc, reply)), Some((tx, _))) = (answer, self.net.as_ref()) {
                if reply == ro_net::NpcReply::Close {
                    self.cutin = None;
                }
                let _ = tx.send(ro_net::Command::Npc { npc, reply });
            }
            // The game draws its own cursor; egui would otherwise show the system one again.
            let mut platform = out.platform_output;
            if self.cursor_sprite.is_some() {
                platform.cursor_icon = egui::CursorIcon::None;
            }
            state.handle_platform_output(window, platform);
            let primitives = self.egui_ctx.tessellate(out.shapes, out.pixels_per_point);
            r.set_ui(ro_render::UiFrame { primitives, textures: out.textures_delta, pixels_per_point: out.pixels_per_point });
            if let Some(text) = sent {
                self.queued_chat.push(text);
                if !self.settings.battle_mode {
                    self.hud.focus_chat();
                }
            }
        }

        if let Some((path, w, h)) = &self.capture {
            let ready = self.ready_at.is_some_and(|t| t.elapsed().as_secs_f32() * 1000.0 >= self.capture_after);
            if ready {
                match r.capture(*w, *h) {
                    Ok(img) => match image::save_buffer(path, &img.pixels, img.width, img.height, image::ColorType::Rgba8) {
                        Ok(()) => {
                            let at = self.player.as_ref().map(|p| p.walker.cell);
                            tracing::info!(path = %path.display(), ?at, "captured");
                        }
                        Err(e) => tracing::error!("save capture: {e:#}"),
                    },
                    Err(e) => tracing::error!("capture: {e:#}"),
                }
                self.capture = None;
                el.exit();
            }
            return;
        }

        if std::mem::take(&mut self.screenshot) {
            // Rendered again offscreen (with or without the UI) and saved; the window keeps
            // the previous frame for this one.
            let (w, h) = r.size();
            match r.capture_with(w, h, self.settings.screenshot_ui) {
                Ok(img) => {
                    let path = screenshot_path();
                    self.hud.push_chat(ro_net::ChatKind::System, format!("Screenshot saved: {}", path.display()));
                    // PNG (lossless), encoded off the frame loop.
                    std::thread::spawn(move || {
                        if let Err(e) = image::save_buffer(&path, &img.pixels, img.width, img.height, image::ColorType::Rgba8) {
                            tracing::error!(path = %path.display(), "save screenshot: {e:#}");
                        }
                    });
                }
                Err(e) => tracing::error!("screenshot: {e:#}"),
            }
        } else {
            let drawing = std::time::Instant::now();
            if let Err(e) = r.frame() {
                tracing::error!("frame: {e:#}");
            }
            self.perf.split((drawing - self.last_tick).as_secs_f32() * 1000.0, drawing.elapsed().as_secs_f32() * 1000.0);
        }
        if let Some(fps) = self.fps.tick() {
            let (w, h) = r.size();
            let at = self.player.as_ref().map(|p| p.walker.cell).unwrap_or_default();
            let map = self.world.as_ref().map_or("-", |w| w.name.as_str());
            let title = match &self.status {
                Some(s) => format!("Tenseiga — {s}"),
                None if self.world.is_none() && self.net.is_some() => "Tenseiga — connecting…".to_string(),
                None => format!("Tenseiga — {map} {},{} — {w}x{h} — {fps:.0} fps", at.0, at.1),
            };
            window.set_title(&title);
        }
    }
}

#[derive(Default)]
struct FpsCounter {
    frames: u32,
    since: Option<std::time::Instant>,
}

impl FpsCounter {
    /// Returns frames per second roughly once a second.
    fn tick(&mut self) -> Option<f64> {
        let now = std::time::Instant::now();
        let since = *self.since.get_or_insert(now);
        self.frames += 1;
        let dt = now - since;
        if dt.as_secs_f64() < 1.0 {
            return None;
        }
        let fps = self.frames as f64 / dt.as_secs_f64();
        self.frames = 0;
        self.since = Some(now);
        Some(fps)
    }
}

/// The window's icon, the logo (packaging/logo.py draws it). On Windows the exe also carries
/// it as a resource (build.rs), which Explorer and the taskbar use.
fn app_icon() -> Option<winit::window::Icon> {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    winit::window::Icon::from_rgba(img.into_raw(), w, h).ok()
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        // Back from the background (Android): the same window has a new surface.
        if let Some(window) = self.window.clone() {
            if std::mem::take(&mut self.paused) {
                if let Some(r) = self.renderer.as_mut() {
                    if let Err(e) = r.recreate_surface(window) {
                        tracing::error!("surface: {e:#}");
                    }
                }
                self.last_tick = std::time::Instant::now();
                el.set_control_flow(winit::event_loop::ControlFlow::Poll);
            }
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Tenseiga")
            .with_window_icon(app_icon())
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 900.0))
            .with_min_inner_size(winit::dpi::LogicalSize::new(640.0, 360.0))
            .with_visible(self.capture.is_none());
        let window = Arc::new(el.create_window(attrs).unwrap());
        // The game draws its own cursors (hud::CursorSprite).
        if self.cursor_sprite.is_some() && self.capture.is_none() {
            window.set_cursor_visible(false);
        }
        let backends = ro_render::parse_backends(&self.backend);
        match pollster::block_on(ro_render::Renderer::new(window.clone(), backends, self.msaa)) {
            Ok(mut r) => {
                r.camera.look_at_cell(self.at.0, self.at.1);
                if let Some(yaw) = self.yaw {
                    r.camera.yaw = yaw.to_radians();
                }
                if let Some(pitch) = self.pitch {
                    r.camera.pitch = pitch.to_radians().clamp(25f32.to_radians(), 75f32.to_radians());
                }
                if let Some(d) = self.distance {
                    r.camera.distance = d;
                }
                self.renderer = Some(r);
                self.apply_sound_and_effects();
            }
            Err(e) => {
                tracing::error!("renderer init failed: {e:#}");
                el.exit();
                return;
            }
        }
        self.egui_state = Some(egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32 * self.ui_scale),
            None,
            None,
        ));
        install_fallback_fonts(&self.egui_ctx);
        if let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_ref()) {
            let path = r"data\texture\effect\ring_blue.tga";
            match vfs.read(path).ok().flatten().map(|d| ro_formats::image::decode(&d, path)) {
                Some(Ok(img)) => r.set_warp_texture(&img),
                _ => tracing::warn!(path, "warp portal texture unavailable"),
            }
        }
        self.window = Some(window);
        el.set_control_flow(winit::event_loop::ControlFlow::Poll);

        // Offline: show the preview map and character right away. Online waits for the server,
        // with the map as the login screen's backdrop.
        self.splash = Some(splash::Splash::new(&self.egui_ctx, self.vfs.as_ref()));
        // Captures of the login screens time from start-up (no map loads behind them).
        if self.front.is_some() {
            self.ready_at = Some(std::time::Instant::now());
        }
        if self.net.is_none() {
            let map = self.map.clone();
            self.enter_map(&map);
            // Offline sky/lighting previews can use moonlight without changing the
            // server's clock. Online sessions continue to follow the server status.
            if std::env::args().any(|a| a == "--night") {
                if let Some(r) = self.renderer.as_mut() {
                    r.night = true;
                }
            }
        }
        if self.net.is_none() {
            let look = self.look.clone();
            self.spawn_player(&look, self.at, self.dir, 150);
        } else if let (Some(r), Some(w)) = (self.renderer.as_mut(), self.world.as_ref()) {
            // Backdrop: look at the map centre from a little further out.
            let c = w.gat.as_ref().map_or((100, 100), |g| ((g.width / 2) as u16, (g.height / 2) as u16));
            r.camera.look_at_cell(c.0, c.1);
            r.camera.distance = 420.0;
        }
    }

    /// Android takes the window surface away while the app is in the background.
    fn suspended(&mut self, el: &ActiveEventLoop) {
        self.paused = true;
        el.set_control_flow(winit::event_loop::ControlFlow::Wait);
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        self.tick(el);
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // No surface to draw or resize while in the background (Android).
        if self.paused {
            return;
        }
        self.note_keyboard_mouse(&event);
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        // Alt+letter shortcuts go straight to the game, even while the chat has the keyboard
        // (AltGr, which is Ctrl+Alt, still types into the UI).
        let alt_shortcut = self.alt && !self.ctrl && matches!(&event, WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key: winit::keyboard::Key::Character(_), .. }, .. });
        // The UI sees input first; clicks and keys it uses don't reach the game.
        let ui_used = !alt_shortcut && self.egui_state.as_mut().is_some_and(|s| s.on_window_event(window, &event).consumed);
        // Track the left button ourselves (hold-to-walk), whatever the UI did with the event.
        if let WindowEvent::MouseInput { state, button: MouseButton::Left, .. } = &event {
            self.left_down = *state == ElementState::Pressed;
        }
        if let WindowEvent::CursorMoved { position, .. } = &event {
            // Keep tracking the cursor even over UI so rotation deltas stay continuous.
            if ui_used {
                self.cursor = Some((position.x, position.y));
            }
        }
        // Login / character screens: the world takes no input.
        let front_blocks = self.front.is_some() && matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::KeyboardInput { .. });
        let function_key = matches!(&event, WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key: winit::keyboard::Key::Named(k), .. }, .. } if hotkey_slot(*k).is_some() || matches!(*k, winit::keyboard::NamedKey::F10 | winit::keyboard::NamedKey::F11 | winit::keyboard::NamedKey::F12));
        let ui_used = ui_used && !function_key;
        // Arranging the interface: clicks and the wheel are for the layout, not the world.
        let arranging = self.arrange.open && matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. });
        let ui_used = ui_used || arranging;
        if (ui_used || front_blocks) && !matches!(event, WindowEvent::Resized(_) | WindowEvent::CloseRequested | WindowEvent::ScaleFactorChanged { .. }) {
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            _ if self.quit => el.exit(),
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, .. }, .. }
                if logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Enter) =>
            {
                self.hud.focus_chat();
            }
            WindowEvent::ModifiersChanged(m) => {
                self.alt = m.state().alt_key();
                self.ctrl = m.state().control_key();
            }
            // Print Screen saves a screenshot, as in the original (Windows reports only its release).
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key: winit::keyboard::Key::Named(winit::keyboard::NamedKey::PrintScreen), state: ElementState::Released, .. }, .. } => {
                self.screenshot = true;
            }
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Released, .. }, .. }
                if self.held_skill.as_ref().is_some_and(|(_, held)| *held == logical_key) =>
            {
                self.release_held_skill();
            }
            WindowEvent::Focused(false) if self.held_skill.is_some() => self.release_held_skill(),
            WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. }, .. } => self.key_pressed(logical_key),
            WindowEvent::Resized(s) => {
                r.resize(s.width, s.height);
                window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => r.scale_factor = scale_factor,
            WindowEvent::Touch(t) => self.touch_input(el, id, t),
            WindowEvent::MouseWheel { .. } | WindowEvent::MouseInput { .. } if self.still => {}
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => (p.y / 60.0) as f32,
                };
                r.camera.zoom(steps);
            }
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => self.right_button(state == ElementState::Pressed),
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                let unit = self.pick_unit().and_then(|id| self.units.get(&id).map(|m| (id, m.kind)));
                match unit {
                    // NPCs: talk (walking over first if needed).
                    Some((id, k)) if is_npc(k) => {
                        self.walk_goal = None;
                        self.attack = None;
                        self.talk(id);
                    }
                    // PvP / GvG maps: clicking an enemy player attacks (the original's attack cursor).
                    Some((id, 0)) if self.aiming.is_none() && self.is_enemy_player(id) => self.attack_unit(id),
                    // Other players: a menu (trade, party, friend).
                    Some((id, 0)) if self.aiming.is_none() => self.open_player_menu(id),
                    // Monsters: attack until it dies (or use the skill being aimed).
                    Some((id, 5)) if std::mem::take(&mut self.capturing) => self.send(ro_net::Command::Companion(ro_net::CompanionRequest::Capture(id))),
                    Some((id, 5)) => match self.aiming.take() {
                        Some((skill, level)) => self.use_skill_on(skill, level, id),
                        None => self.attack_unit(id),
                    },
                    // Aiming at another player (heal, blessing...).
                    Some((id, 0)) => {
                        if let Some((skill, level)) = self.aiming.take() {
                            self.use_skill_on(skill, level, id);
                        }
                    }
                    _ => {
                        self.attack = None;
                        self.chasing = false;
                        if let Some((skill, level)) = self.aiming.take() {
                            let ground = self.skills.iter().find(|s| s.id == skill).is_some_and(|s| s.target & 2 != 0);
                            let me = self.player.as_ref().map(|p| p.walker.cell);
                            match self.pick() {
                                Some(cell) if ground => self.use_skill_at(skill, level, cell),
                                // Clicking our own cell targets ourselves.
                                Some(cell) if Some((cell.0 as i32, cell.1 as i32)) == me => self.use_skill_on(skill, level, self.account_id),
                                _ => {}
                            }
                        } else if let Some(item) = self.pick_floor_item() {
                            self.pick_up(item);
                        } else if let Some(cell) = self.pick() {
                            self.pending_npc = None;
                            self.pending_pickup = None;
                            self.pending_skill = None;
                            // Holding the button keeps steering toward the cursor.
                            self.mouse_walk = true;
                            self.mouse_walk_ms = 150.0;
                            self.request_walk(cell);
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let (true, Some((lx, ly))) = (self.rotating, self.cursor) {
                    self.right_drag += (position.x - lx).hypot(position.y - ly);
                    // Scale by DPI so rotation speed matches across monitors.
                    let s = 0.005 / r.scale_factor as f32;
                    r.camera.rotate((position.x - lx) as f32 * s, (position.y - ly) as f32 * s);
                }
                self.cursor = Some((position.x, position.y));
            }
            _ => {}
        }
    }
}

fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();
    start(None)
}

/// Loads the config and runs the client. Android hands over the event loop its activity made.
fn start(event_loop: Option<EventLoop<()>>) -> anyhow::Result<()> {
    let mut cfg = load_config()?;
    // `--map name` / `--at x,y` override the preview map and cell.
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    // `--login <addr>` / `--login-addr <addr>` beat the config (local or LAN server).
    cfg.login_addr = normalize_login_addr(&arg("--login").or_else(|| arg("--login-addr")).unwrap_or(cfg.login_addr));
    println!("Login server: {}", cfg.login_addr);
    tracing::info!(login_addr = %cfg.login_addr, "connecting to login server");

    if std::env::args().any(|a| a == "--headless") {
        let rt = tokio::runtime::Runtime::new()?;
        return rt.block_on(headless(&cfg));
    }

    let event_loop = match event_loop {
        Some(e) => e,
        None => EventLoop::new()?,
    };
    let vfs = open_vfs(&cfg)?;
    let cell = |s: String| s.split_once(',').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
    // Online unless --offline (or no game data to render with).
    // Scripted and test runs (or `auto_login = true`) log straight in with the configured
    // account; otherwise the login and character screens drive the session.
    const SCRIPTED: [&str; 10] = ["--capture", "--do", "--say", "--attack", "--talk", "--walk", "--windows", "--deal", "--accept", "--auto-trade"];
    // `--front` keeps the screens for captures; `--front-login` also submits the configured login.
    let front_test = args.iter().any(|a| matches!(a.as_str(), "--front" | "--front-login" | "--front-create" | "--front-make" | "--front-start" | "--front-rename" | "--front-move" | "--front-pin"));
    let auto = !front_test && (cfg.auto_login || args.iter().any(|a| SCRIPTED.contains(&a.as_str())));
    let (cfg_user, cfg_pass) = (cfg.username.clone(), cfg.password.clone());
    let session = ro_net::SessionConfig {
        login_addr: cfg.login_addr.clone(),
        username: cfg.username.clone(),
        password: cfg.password.clone(),
        char_slot: cfg.char_slot,
        pincode: cfg.pincode.clone(),
        auto,
    };
    let online = !args.iter().any(|a| a == "--offline") && vfs.is_some();
    let net = online.then(|| ro_net::spawn_session(session.clone()));
    let mut app = App {
        backend: cfg.backend,
        msaa: cfg.msaa,
        vfs,
        map: arg("--map").unwrap_or(cfg.map),
        still: args.iter().any(|a| a == "--still"),
        distance: arg("--distance").and_then(|d| d.parse().ok()),
        yaw: arg("--yaw").and_then(|d| d.parse().ok()),
        pitch: arg("--pitch").and_then(|d| d.parse().ok()),
        look: Look { job: arg("--job").unwrap_or_else(|| "초보자".into()), male: arg("--sex").is_none_or(|s| s != "f"), hair: 1, hair_color: 0, gear: ro_net::Look { weapon: arg("--weapon").and_then(|w| w.parse().ok()).unwrap_or(0), shield: arg("--shield").and_then(|w| w.parse().ok()).unwrap_or(0), ..Default::default() } },
        action: arg("--action").and_then(|a| a.parse().ok()).unwrap_or(0),
        dir: arg("--dir").and_then(|d| d.parse::<usize>().ok()).unwrap_or(0) % 8,
        capture_after: arg("--capture-after").and_then(|d| d.parse().ok()).unwrap_or(0.0),
        walk_on_enter: arg("--walk").and_then(cell),
        say_on_enter: arg("--say"),
        net,
        session: session.clone(),
        quit: false,
        quit_asked: None,
        held_skill: None,
        esc_menu: false,
        splash: None,
        front: (online && !auto).then(|| screens::Front::new(settings::Settings::load(Default::default()).last_id.clone(), true)),
        world: None,
        player: None,
        account_id: 0,
        char_id: 0,
        guild: Default::default(),
        status: None,
        walk_goal: None,
        dialog: None,
        cutin: None,
        minimap: None,
        settings: settings::Settings::load(settings::Settings {
            bgm_volume: cfg.bgm_volume,
            sfx_volume: cfg.sfx_volume,
            ui_scale: cfg.ui_scale,
            minimap: true,
            menu_collapsed: false,
            classic_ui: true,
            visual: Default::default(),
            last_id: String::new(),
            upscale: Default::default(),
            ..Default::default()
        }),
        audio: None,
        pending_npc: None,
        pending_skill: None,
        attack: None,
        chasing: false,
        floaters: vec![],
        item_db: None,
        skill_names: Default::default(),
        skill_tree: Default::default(),
        inventory: Default::default(),
        floor: Default::default(),
        stats: None,
        skills: vec![],
        windows: Default::default(),
        icons: Default::default(),
        icons_tried: Default::default(),
        unid_icons: Default::default(),
        unid_tried: Default::default(),
        skill_icons: Default::default(),
        pending_pickup: None,
        aiming: None,
        hotkeys: vec![Default::default(); HOTKEY_ROWS as usize * HOTKEYS_PER_ROW],
        hotkey_row: 0,
        casts: Default::default(),
        fx: Default::default(),
        map_fx: vec![],
        script_progress: None,
        unit_progress: Default::default(),
        shouts: Default::default(),
        devotions: Default::default(),
        ground_units: Default::default(),
        traps: Default::default(),
        trap_models: Default::default(),
        graffiti: Default::default(),
        digit: None,
        pvp_rank: None,
        breath_ms: 0.0,
        sprite_fx: vec![],
        burst_at: Default::default(),
        emotes: Default::default(),
        last_whisper_from: None,
        queued_chat: vec![],
        buffs: Default::default(),
        quests: Default::default(),
        merchant: Default::default(),
        stores: Default::default(),
        progress: Default::default(),
        companions: Default::default(),
        pet_evo: Default::default(),
        skill_scales: Default::default(),
        area_tex: None,
        mailbox: Default::default(),
        clan: Default::default(),
        merge: Default::default(),
        reputation: Default::default(),
        game_opts: Default::default(),
        ranking: Default::default(),
        store_search: Default::default(),
        stylist: Default::default(),
        reform: Default::default(),
        lapine: Default::default(),
        enchant: Default::default(),
        market: Default::default(),
        market_buy: None,
        battle: Default::default(),
        navi: Default::default(),
        navi_walk: None,
        grade: Default::default(),
        rooms: Default::default(),
        booking: Default::default(),
        agency: Default::default(),
        web_url: web_url(cfg.web_addr.as_deref(), &cfg.login_addr),
        gm: cfg.gm,
        gm_aids: cfg.gm_aids.iter().copied().collect(),
        yellow_aids: cfg.yellow_aids.iter().copied().collect(),
        ignore_pending: None,
        companion_action: None,
        follow_at: std::time::Instant::now(),
        unit_jobs: Default::default(),
        aggro: Default::default(),
        homun_ai: None,
        merc_ai: None,
        client_dir: cfg.client_dir.clone(),
        skin: None,
        item_info: Default::default(),
        refine: Default::default(),
        show_equip: None,
        portrait: None,
        cooldowns: Default::default(),
        script_resume: None,
        pad: gamepad::Pad::new(),
        pad_row: None,
        pad_preview: None,
        hold_walk: None,
        walk_speed: None,
        test_stick: None,
        hid: None,
        hid_retry_ms: 0.0,
        audio_haptics: None,
        haptics: Default::default(),
        heartbeat_ms: 0.0,
        haptics_test_ms: 0.0,
        haptics_route: "",
        haptics_hd: true,
        touch_prev: Default::default(),
        touch_pos: None,
        touch_slide: glam::Vec2::ZERO,
        touch_button: None,
        touch_feeding: false,
        mouse_travel: 0.0,
        pad_target: None,
        pad_dir: None,
        pad_line: None,
        pad_walk_ms: 0.0,
        dead: false,
        fx_test: arg("--fx").map(|s| &*s.leak()),
        fx_test_next: None,
        alt: false,
        ctrl: false,
        screenshot: false,
        emote_win: Default::default(),
        shortcut_win: Default::default(),
        queued_action: None,
        fake_pointer: None,
        fake_click: None,
        fake_frames: Default::default(),
        social: Default::default(),
        books: Default::default(),
        memorial: Default::default(),
        roulette: Default::default(),
        sales: Default::default(),
        reporter: Default::default(),
        captcha: Default::default(),
        trade_partner: None,
        player_menu: None,
        auto_deal: arg("--deal").map(|d| d == "buy"),
        auto_sell: arg("--sell").and_then(|s| s.parse().ok()),
        auto_buy: arg("--buy").and_then(|s| s.parse().ok()),
        auto_accept: args.iter().any(|a| a == "--accept"),
        auto_trade: arg("--auto-trade").and_then(|s| s.parse().ok()),
        trade_stage: 0,
        trade_offers: Default::default(),
        queued_commands: vec![],
        open_windows: args.iter().any(|a| a == "--windows"),
        auto_pickup: false,
        script: arg("--do").map(|s| s.split(",").map(str::to_string).collect()).unwrap_or_default(),
        talk_to: arg("--talk"),
        front_hook: None,
        front_start: None,
        viewpoints: Default::default(),
        member_pos: Default::default(),
        spirits: Default::default(),
        attack_range: 1,
        capturing: false,
        map_flags: 0,
        hat_files: Default::default(),
        hat_fx: Default::default(),
        hat_auras: Default::default(),
        emblems: Default::default(),
        emblem_versions: Default::default(),
        emblem_web: Default::default(),
        npc_answers: arg("--npc-answers").map(|s| s.split(',').map(str::to_string).collect()).unwrap_or_default(),
        auto_attack: args.iter().any(|a| a == "--attack"),
        units: Default::default(),
        warps: Default::default(),
        class_names: Default::default(),
        pet_talk: Default::default(),
        sense: Default::default(),
        look_tables: Default::default(),
        sprites: Default::default(),
        models3d: Default::default(),
        ready_at: None,
        last_tick: std::time::Instant::now(),
        capture: arg("--capture").map(|p| {
            let (w, h) = arg("--size")
                .and_then(|s| s.split_once('x').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))))
                .unwrap_or((1920, 1080));
            (PathBuf::from(p), w, h)
        }),
        at: arg("--at").and_then(cell).unwrap_or((156, 191)),
        window: None,
        renderer: None,
        paused: false,
        rotating: false,
        fingers: Vec::new(),
        right_drag: 0.0,
        cursor_sprite: None,
        cursor_action: 0,
        mouse_walk: false,
        mouse_walk_ms: 0.0,
        left_down: false,
        cursor: None,
        fps: FpsCounter::default(),
        perf: perf::Perf::default(),
        // --arrange opens the layout editor at once (for captures of it).
        arrange: hudlayout::Editor::opened(args.iter().any(|a| a == "--arrange")),
        ping: None,
        egui_ctx: egui::Context::default(),
        egui_state: None,
        hud: hud::Hud::default(),
        ui_scale: 1.0, // from settings below
    };
    // Interface textures loaded from here on follow the saved upscaling settings.
    uitex::set_upscaler(ro_render::Upscaler::new(app.settings.upscale.clone()));
    app.class_names = app.vfs.as_ref().map(load_class_names).unwrap_or_default();
    app.pet_talk = app.vfs.as_ref().map(pettalk::PetTalk::load).unwrap_or_default();
    app.books = app.vfs.as_ref().map(book::Books::load).unwrap_or_default();
    app.guild.client_dir = app.client_dir.clone();
    app.hat_files = app.vfs.as_ref().map(fx::hat_effects).unwrap_or_default();
    app.ui_scale = app.settings.ui_scale;
    if args.iter().any(|a| matches!(a.as_str(), "--front-login" | "--front-create" | "--front-make" | "--front-start" | "--front-rename" | "--front-move" | "--front-pin")) {
        app.queued_commands.push(ro_net::Command::Login { user: cfg_user.clone(), pass: cfg_pass.clone() });
    }
    app.cursor_sprite = app.vfs.as_ref().and_then(|v| hud::CursorSprite::load(&app.egui_ctx, v));
    // Captures and headless runs stay silent.
    if app.capture.is_none() {
        app.audio = app.vfs.as_ref().and_then(|v| audio::Audio::new(v, app.settings.bgm_volume, app.settings.sfx_volume));
        app.apply_sound_and_effects();
        // Title theme while the login screens show.
        if let (Some(a), Some(v), true) = (app.audio.as_mut(), app.vfs.as_ref(), app.front.is_some()) {
            a.title(v);
        }
    }
    if let Some(vfs) = app.vfs.as_ref() {
        // iteminfo.lub lives in the client's System folder; skill names in the GRF.
        let read = |p: &str| vfs.read(p).ok().flatten();
        app.fx.index(vfs);
        app.buffs = buffs::Buffs::load(vfs);
        app.quests = quests::QuestLog::load(vfs);
        app.progress = progress::Progress::load(vfs);
        app.pet_evo = petevo::PetEvolution::load(vfs);
        app.skill_scales = skillscale::SkillScales::load(vfs);
        msgstr::load(vfs);
        app.item_info = iteminfo::ItemInfo::load(vfs);
        let mut db = match read(r"System\iteminfo.lub").map(|d| ro_game::items::ItemDb::from_lub(&d)) {
            Some(Ok(db)) => db,
            Some(Err(e)) => {
                tracing::warn!("iteminfo: {e:#}");
                ro_game::items::ItemDb::default()
            }
            None => {
                tracing::warn!("System/iteminfo.lub not found; loading text item metadata");
                ro_game::items::ItemDb::default()
            }
        };
        // Custom items still work when an installation supplies only text item info.
        for p in customdata::ITEMINFO_SOURCES {
            if let Some(src) = read(p) {
                db.extend(customdata::iteminfo(&src, p));
            }
        }
        if !db.is_empty() {
            tracing::info!(items = db.len(), "loaded item info");
            app.item_db = Some(db);
        }
        let info = |n: &str| read(&format!(r"data\luafiles514\lua files\datainfo\{n}.lub"));
        if let (Some(a), Some(b), Some(c)) = (info("accessoryid"), info("accname"), info("weapontable")) {
            match ro_game::looks::LookTables::from_lubs(&a, &b, &c) {
                Ok(t) => app.look_tables = t,
                Err(e) => tracing::warn!("equipment sprite tables: {e:#}"),
            }
        }
        let skills = read(r"data\luafiles514\lua files\skillinfoz\skillinfolist.lub").map(|d| ro_game::skills::SkillNames::from_lub(&d));
        match skills {
            Some(Ok(s)) => app.skill_names = s,
            Some(Err(e)) => tracing::warn!("skillinfolist: {e:#}"),
            None => tracing::warn!("skillinfolist.lub not found"),
        }
        if let Some(d) = read(r"data\luafiles514\lua files\skillinfoz\skillid.lub") {
            if let Err(e) = app.skill_names.add_ids(&d) {
                tracing::warn!("skillid: {e:#}");
            }
        }
        let tree = read(r"data\luafiles514\lua files\skillinfoz\skilltreeview.lub");
        if let (Some(view), Some(inherit)) = (tree, read(r"data\luafiles514\lua files\skillinfoz\jobinheritlist.lub")) {
            match skilltree::SkillTree::from_lubs(&view, &inherit) {
                Ok(t) => app.skill_tree = t,
                Err(e) => tracing::warn!("skill tree: {e:#}"),
            }
        }
        if let Some(d) = read(r"data\luafiles514\lua files\skillinfoz\skilldescript.lub") {
            if let Err(e) = app.skill_names.add_descriptions(&d) {
                tracing::warn!("skilldescript: {e:#}");
            }
        }
    }
    samurai::register(&mut app.skill_names, &mut app.skill_tree);
    // `--sound-check`: list which classic skills have a sound file, then exit.
    if args.iter().any(|a| a == "--sound-check") {
        let (Some(vfs), true) = (app.vfs.as_ref(), true) else { return Ok(()) };
        let Some(a) = audio::Audio::new(vfs, 0.0, 0.0) else { return Ok(()) };
        const CLASSES: [&str; 19] = ["SM_", "MG_", "AL_", "AC_", "MC_", "TF_", "KN_", "PR_", "WZ_", "BS_", "HT_", "AS_", "CR_", "MO_", "SA_", "RG_", "AM_", "BA_", "DC_"];
        let mut skills: Vec<(u16, &str)> = app.skill_names.all_ids().filter(|(_, n)| CLASSES.iter().any(|c| n.starts_with(c))).collect();
        skills.sort();
        let (mut hit, mut miss) = (vec![], vec![]);
        for (_, n) in &skills {
            match a.skill_sound(n) {
                Some(f) => hit.push(format!("{n} -> {f}")),
                None => miss.push(n.to_string()),
            }
        }
        println!("{} with sound, {} without", hit.len(), miss.len());
        println!("{}", hit.join("
"));
        println!("--- missing:
{}", miss.join(" "));
        return Ok(());
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Which attack motion a player uses: the weapon sprite only has frames in its own one
/// (katars draw in group 11, most weapons in 10).
fn attack_group(r: &ro_render::Renderer, a: &ro_render::Actor) -> usize {
    match a.weapon.first() {
        Some(&w) if !r.action_visible(w, 10) && r.action_visible(w, 11) => 11,
        _ => 10,
    }
}

#[cfg(test)]
mod battle_mode_tests {
    fn battle_slot(key: &str, page: u8) -> Option<usize> {
        let rows: Vec<String> = crate::windows::BATTLE_KEYS.iter().map(|k| k.to_string()).collect();
        super::battle_slot(key, page, &rows)
    }

    #[test]
    fn rebound_keys_are_used() {
        let rows = vec!["12345".to_string(), String::new(), "zx".to_string()];
        assert_eq!(super::battle_slot("3", 0, &rows), Some(11));
        assert_eq!(super::battle_slot("x", 0, &rows), Some(28));
        assert_eq!(super::battle_slot("q", 0, &rows), None);
    }

    #[test]
    fn letter_rows_follow_the_page_on_show() {
        // Page 1 on show: Q ~ O is page 2, A ~ L page 3, Z ~ > page 4.
        assert_eq!(battle_slot("q", 0), Some(9));
        assert_eq!(battle_slot("O", 0), Some(17));
        assert_eq!(battle_slot("a", 0), Some(18));
        assert_eq!(battle_slot("z", 0), Some(27));
        assert_eq!(battle_slot(".", 0), Some(35));
        assert_eq!(battle_slot(">", 0), Some(35));
        // They wrap around past the fourth page.
        assert_eq!(battle_slot("q", 3), Some(0));
        assert_eq!(battle_slot("z", 2), Some(9));
        assert_eq!(battle_slot("p", 0), None);
        assert_eq!(battle_slot("1", 0), None);
    }
}

#[cfg(test)]
mod animation_tests {
    use super::animation_time;
    use ro_game::Walker;

    /// Drive the same clock with actual path state; no renderer or game window needed.
    fn step(walker: &mut Walker, clock: &mut (usize, f32), dt: f32, pose_rate: f32) {
        walker.update(dt);
        let action = if walker.is_walking() { 1 } else { 0 };
        clock.1 = animation_time(clock.0, (action, false), clock.1, dt, pose_rate);
        clock.0 = action;
    }

    #[test]
    fn walk_starts_at_frame_zero_after_idle_and_stops_at_frame_zero() {
        let mut walker = Walker::new((0, 0), 0, 150.0);
        let mut clock = (0, 4250.0);
        walker.walk(vec![(1, 0)]);
        // The walk was queued before the update, so is_walking() is already true.
        assert!(walker.is_walking());
        step(&mut walker, &mut clock, 16.0, 1.0);
        assert_eq!(clock, (1, 0.0));
        step(&mut walker, &mut clock, 84.0, 1.0);
        assert_eq!(clock, (1, 84.0));
        step(&mut walker, &mut clock, 50.0, 1.0);
        assert_eq!(clock, (0, 0.0));
    }

    #[test]
    fn walk_phase_survives_cells_turns_and_path_continuations() {
        let mut walker = Walker::new((0, 0), 6, 100.0);
        let mut clock = (0, 3000.0);
        walker.walk(vec![(1, 0), (2, 0), (2, 1), (2, 2)]);
        step(&mut walker, &mut clock, 10.0, 1.0);
        step(&mut walker, &mut clock, 90.0, 1.0);
        assert_eq!(walker.cell, (1, 0));
        assert_eq!(clock, (1, 90.0));
        assert!(walker.redirect((1, 0), vec![(2, 0), (2, 1), (2, 2)]));
        step(&mut walker, &mut clock, 100.0, 1.0);
        assert_eq!(walker.cell, (2, 0));
        assert_eq!(walker.dir, 4);
        assert_eq!(clock, (1, 190.0));
        step(&mut walker, &mut clock, 50.0, 1.0);
        assert_eq!(clock, (1, 240.0));
    }

    #[test]
    fn movement_uses_authored_timing_when_it_interrupts_a_fast_pose() {
        let mut walker = Walker::new((0, 0), 6, 150.0);
        let mut clock = (10, animation_time(10, (10, true), 25.0, 20.0, 4.0));
        assert_eq!(clock.1, 105.0); // Timed attack still uses its fitted rate.
        walker.walk(vec![(1, 0), (2, 0), (3, 0)]);
        step(&mut walker, &mut clock, 10.0, 4.0);
        assert_eq!(clock, (1, 0.0));
        step(&mut walker, &mut clock, 50.0, 4.0);
        assert_eq!(clock, (1, 50.0));
        walker.speed = 75.0;
        step(&mut walker, &mut clock, 50.0, 4.0);
        assert_eq!(clock, (1, 100.0));
        // Expiring the interrupted pose must not reset a walk already in progress.
        step(&mut walker, &mut clock, 50.0, 1.0);
        assert_eq!(clock, (1, 150.0));
    }
}

#[cfg(test)]
mod login_addr_tests {
    use super::normalize_login_addr as n;

    #[test]
    fn normalizes() {
        assert_eq!(n("127.0.0.1:6900"), "127.0.0.1:6900");
        assert_eq!(n(" 192.168.1.50 "), "192.168.1.50:6900");
        assert_eq!(n("192.168.1.50:6901"), "192.168.1.50:6901");
        assert_eq!(n("myhost.lan"), "myhost.lan:6900");
        assert_eq!(n("myhost.lan:7000"), "myhost.lan:7000");
        assert_eq!(n("[::1]"), "[::1]:6900");
        assert_eq!(n("[::1]:7000"), "[::1]:7000");
        assert_eq!(n("::1"), "[::1]:6900");
    }
}

/// Players at this base level or above carry the level aura. The server decides what
/// level each player reports (rAthena's max_lv and aura_lv); the client only compares.
const AURA_LEVEL: u16 = 99;

/// Custom hat effect ids (rAthena's `hateffect`) the client draws in code as auras. The
/// server's HAT_EF_* constants for them live in src/map/script.hpp.
fn custom_hat_aura(id: u16) -> Option<ro_render::AuraStyle> {
    match id {
        2000 => Some(ro_render::AuraStyle::Tenseiga), // HAT_EF_SAMURAI_TENSEIGA
        _ => None,
    }
}

/// This frame's aura textures (ring_blue, pikapika2 and whitelight) and switches.
#[derive(Default)]
struct AuraTextures {
    level: Option<[ro_render::TexId; 3]>,
    /// The same, whitened, for custom auras that bring their own colours.
    white: Option<[ro_render::TexId; 3]>,
    /// The soft disc that shapes a custom aura's dark pool.
    disc: Option<ro_render::TexId>,
    /// /aura2 and /aura (which touch only the level aura).
    level_off: bool,
    simple: bool,
}

impl AuraTextures {
    /// Puts the aura under `a`, if it carries one: its custom aura, else the level aura
    /// (EF_LEVEL99_2 on the ground, the EF_LEVEL99 ribbons and the EF_LEVEL99_3 sparkles;
    /// simplified, the sparkles alone). Hidden units show none. Returns the aura's clock:
    /// `time_ms` while it shows, 0 otherwise (so its ribbons grow in again).
    fn push(&self, r: &mut ro_render::Renderer, custom: Option<ro_render::AuraStyle>, level: bool, a: &ro_render::Actor, time_ms: f32, seed: u32) -> f32 {
        use ro_render::AuraLayer::*;
        let (style, tex, simple) = match custom {
            Some(style) => (style, self.white, false),
            None if level && !self.level_off => (ro_render::AuraStyle::Level, self.level, self.simple),
            None => return 0.0,
        };
        let Some(tex) = tex.filter(|_| a.alpha >= 1.0) else { return 0.0 };
        // Custom auras lay a dark pool first, shaped by the disc texture.
        for (layer, tex) in [(Shade, self.disc.unwrap_or(tex[2])), (Ground, tex[1]), (Swirl, tex[0]), (Sparkles, tex[2])] {
            if (simple && layer != Sparkles) || (layer == Shade && custom.is_none()) {
                continue;
            }
            let shape = ro_render::Shape::Aura { layer, seed, style };
            r.auras.push(ro_render::ProcFx { tex, shape, pos: a.pos, color: [1.0; 3], time_ms, duration_ms: f32::MAX });
        }
        time_ms
    }
}

/// How opaque a unit is under its OPTION_* flags. Hiding, cloaking and Chase Walk show
/// you faded; other units (and NPCs cloaked by scripts) vanish entirely.
fn option_alpha(option: u32, me: bool) -> f32 {
    match (option & (0x2 | 0x4 | 0x40 | 0x4000) != 0, me) {
        (false, _) => 1.0,
        (true, true) => 0.35,
        (true, false) => 0.0,
    }
}

/// The ground point at a cell's centre.
fn cell_pos_in(w: &World, cell: (u16, u16)) -> Option<glam::Vec3> {
    let (fx, fy) = (cell.0 as f32 + 0.5, cell.1 as f32 + 0.5);
    let (x, z) = (fx * ro_render::CELL, fy * ro_render::CELL);
    let y = w.floor_at(x, z);
    Some(glam::Vec3::new(x, y, z))
}
