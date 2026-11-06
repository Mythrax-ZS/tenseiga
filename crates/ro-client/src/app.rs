// Tenseiga: the client (included by main.rs for the desktop exe and lib.rs for Android).

mod audio;
mod chatlog;
mod combat;
mod mvptimers;
mod mvptimes_db;
mod targetframe;
mod mobinfo;
mod mobinfo_db;
mod skillarea;
mod skillarea_db;
mod photo;
mod surface;
mod weather;
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
mod keybinds;
mod packages;
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
mod app_events;
mod app_input;
mod app_fx;
mod app_chat;
mod app_frame;
mod app_start;

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
    /// Body dye (jobs::body_palette).
    clothes_color: u16,
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
    let body = sheet(&appearance.body, jobs::body_palette(&look.job, look.male, look.clothes_color))?;
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
        head_dir: 0,
        time_ms: 0.0,
        once: false,
        alpha: 1.0, scale: 1.0, tint: [1.0; 3], silhouette: None,
    })
}

struct LoadedMap {
    mesh: ro_render::SceneMesh,
    /// Textures with their mip chains, and the ray tracing geometry (ro_render::prepare_scene).
    scene: ro_render::PreparedScene,
    lighting: ro_render::Lighting,
    /// Light sources (torches, lamps) in world space.
    lights: Vec<ro_render::PointLight>,
    water: Option<(ro_render::WaterMesh, Vec<Option<ro_render::Rgba>>)>,
    /// Ambient effect emitters (EF_* id, world position): torches, bubbles, fireflies.
    effects: Vec<(i32, glam::Vec3, f32, [f32; 4])>,
    /// What the ground is made of, cell by cell (footprints, dust).
    surface: surface::SurfaceMap,
}

/// `up` upscales ground, water and model textures (never sprites, which load elsewhere).
/// Server maps that are copies of a stock map (its cells under a new name in the server's map
/// cache, written by rathena tools/twin_flames_maps.py): the client draws the original's files,
/// at night if the copy says so. The Twin Flames Saga's memorial dungeons are instances of these.
const MAP_COPIES: [(&str, &str, bool); 8] = [
    ("tf_goddess", "herosria", false),   // The Celestial Veil
    ("tf_kagemori", "amatsu", true),     // Kagemori Burning: the night the village burned
    ("tf_fortress", "ama_dun02", false), // Kurogane's Fortress
    ("tf_crimson", "ama_fild01", false), // The Crimson Fields
    ("tf_shrine", "ama_dun03", false),   // The Shrine of Seals
    ("tf_palace", "ama_in02", false),    // The Palace of Masks
    ("tf_abyss", "thor_v03", false),     // The Abyssal Pit
    ("tf_trial", "job_ko", false),       // The Trial of Balance
];

/// The map whose files draw `name` (itself, unless it is a copy).
fn map_files(name: &str) -> &str {
    MAP_COPIES.iter().find(|(copy, ..)| name.eq_ignore_ascii_case(copy)).map_or(name, |(_, original, _)| original)
}

/// Whether `name` is a copy drawn at night whatever the server's clock says.
fn map_night(name: &str) -> bool {
    MAP_COPIES.iter().any(|(copy, _, night)| *night && name.eq_ignore_ascii_case(copy))
}

/// A map loading on its own thread: the event that asked for it (and every event after it)
/// waits until it is in, so the handlers see it loaded as before.
struct MapLoad {
    /// The map's files (see map_files).
    files: String,
    event: ro_net::Event,
    rx: std::sync::mpsc::Receiver<anyhow::Result<LoadedMap>>,
    /// How far the loader is, in thousandths.
    progress: Arc<std::sync::atomic::AtomicU32>,
    started: std::time::Instant,
}

impl MapLoad {
    fn progress(&self) -> f32 {
        self.progress.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0
    }
}

/// `progress` counts thousandths of the load as it goes (for the loading screen's bar).
/// `rt_layers` is ro_render::Renderer::rt_layers, for the ray tracing geometry.
fn load_map(vfs: &ro_vfs::Vfs, name: &str, up: &ro_render::Upscaler, rt_layers: Option<usize>, progress: &std::sync::atomic::AtomicU32) -> anyhow::Result<LoadedMap> {
    let started = std::time::Instant::now();
    let step = |thousandths: u32| progress.store(thousandths, std::sync::atomic::Ordering::Relaxed);
    // `from..to` as `n` things get done (in any order, from any thread).
    let span = |from: u32, to: u32, n: usize| {
        let done = std::sync::atomic::AtomicUsize::new(0);
        move || {
            let i = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            step(from + ((to - from) as usize * i.min(n) / n.max(1)) as u32);
        }
    };
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
    step(20);
    for (i, inst) in rsw.models.iter().enumerate() {
        step(20 + (100 * i / rsw.models.len().max(1)) as u32);
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
    step(120);

    // Where most of the time goes: textures read, upscaled and given their mip chains.
    let load_textures = |names: &[String], each: &(dyn Fn() + Sync)| -> Vec<Option<ro_render::Rgba>> {
        let load = |t: &String| {
            let path = format!("data\\texture\\{t}");
            let img = match vfs.read(&path).and_then(|d| d.map(|d| ro_formats::image::decode(&d, &path)).transpose()) {
                Ok(Some(img)) => Some(img),
                Ok(None) => {
                    tracing::warn!(%path, "texture missing");
                    None
                }
                Err(e) => {
                    tracing::warn!(%path, "texture unreadable: {e:#}");
                    None
                }
            };
            each();
            img
        };
        // Decoded side by side (a GRF holds its lock only to read the bytes), leaving a core
        // for the frame loop.
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_sub(1).clamp(1, 16);
        let chunk = names.len().div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            let parts: Vec<_> = names.chunks(chunk).map(|part| scope.spawn(move || part.iter().map(load).collect::<Vec<_>>())).collect();
            parts.into_iter().flat_map(|h| h.join().unwrap_or_default()).collect()
        })
    };
    // Ground textures come first in the mesh's list, then the models'.
    let classes = (0..mesh.textures.len()).map(|i| if i < mesh.ground_textures { ro_render::AssetClass::Ground } else { ro_render::AssetClass::Model });
    let n = mesh.textures.len();
    let decoded = load_textures(&mesh.textures, &span(120, 600, n));
    let images = up.apply_all_counted(classes.zip(mesh.textures.iter().cloned()).zip(decoded).map(|((c, n), img)| (c, n, img)).collect(), &span(600, 750, n));
    let scene = ro_render::prepare_scene_counted(&mesh, images, rt_layers, &span(750, 950, n));
    // RSW 2.6+ moved the water into the GND (1.8+).
    let water = rsw.water.or(gnd.water).as_ref().map(|w| {
        let mesh = ro_render::WaterMesh::build(&gnd, w);
        let images = up.apply_all(mesh.textures.iter().cloned().zip(load_textures(&mesh.textures, &|| {})).map(|(n, img)| (ro_render::AssetClass::Ground, n, img)).collect());
        (mesh, images)
    });
    step(1000);
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
    Ok(LoadedMap { mesh, scene, water, lighting: ro_render::Lighting::from_rsw(&rsw.light), lights, effects, surface: surface::SurfaceMap::new(&gnd) })
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
    /// Its sky profile's climate ("snow", "forest"...), for ambient weather; None indoors.
    climate: Option<&'static str>,
    /// What the ground is made of (footprints, dust).
    surface: surface::SurfaceMap,
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

/// The skill rAthena keeps using while its key is held (CZ_USE_SKILL_START): Rolling Cutter.
const HOLD_SKILL: u16 = 2036;

/// F1..F9 -> their place in a row (with F10-F12, the keys that reach the game over the UI).
fn hotkey_slot(k: winit::keyboard::NamedKey) -> Option<usize> {
    use winit::keyboard::NamedKey::*;
    [F1, F2, F3, F4, F5, F6, F7, F8, F9].iter().position(|&f| f == k)
}

/// The camera's jolt while a screen quake runs. roBrowser's Camera.processQuake flips a tenth
/// of a cell side to side every frame, which at today's frame rates blurs into nothing; here the
/// camera jumps to the other side every 33 ms (about 30 times a second), a sixth of a cell
/// across and half that up or down, each jolt a little bigger or smaller.
fn quake_offset(quakes: &mut Vec<(std::time::Instant, std::time::Instant)>, now: std::time::Instant, yaw: f32) -> glam::Vec3 {
    quakes.retain(|q| q.1 > now);
    let Some(start) = quakes.iter().filter(|q| q.0 <= now).map(|q| q.0).min() else { return glam::Vec3::ZERO };
    let step = ((now - start).as_millis() / 33) as u32;
    let sign = |b: bool| if b { 1.0 } else { -1.0 };
    // A fixed scramble of the step: 0.75 to 1.25.
    let size = (step.wrapping_mul(2_654_435_761) >> 24) as f32 / 255.0 * 0.5 + 0.75;
    let across = sign(step % 2 == 0) * size * 0.175 * ro_render::CELL;
    glam::Vec3::new(yaw.cos() * across, sign(step % 4 < 2) * across.abs() * 0.5, -yaw.sin() * across)
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
    /// The costume a status dresses the player in (`costume_job`), drawn over `look`.
    costume: Option<i16>,
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
            costume: None,
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
            self.actor.head_dir = 0;
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
        // Walking straightens the head.
        if walking {
            self.actor.head_dir = 0;
        }
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
    /// Shared with the map loader thread.
    vfs: Option<Arc<ro_vfs::Vfs>>,
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
    /// How many lines of the Super Novice prayer have been said in a row.
    prayer_at: usize,
    /// /doridori: milliseconds into our head shake.
    doridori: Option<f32>,
    /// Holding the button after a turn in place: the direction last looked toward, so moving
    /// the mouse round keeps turning.
    turn_drag: Option<usize>,
    /// Screen quakes running or due: (start, end).
    quakes: Vec<(std::time::Instant, std::time::Instant)>,
    /// Varies the hit sparks' flecks.
    spark_seed: u32,
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
    /// The skill each cast in progress is of (for the target frame).
    cast_skill: std::collections::HashMap<u32, u16>,
    /// The monster the target frame shows and when we last traded blows with it.
    focus: Option<(u32, std::time::Instant)>,
    fx: fx::FxLib,
    /// The current map's ambient effect emitters (RSW torches, bubbles, fireflies).
    map_fx: Vec<fx::MapEmitter>,
    /// Script `progressbar` over our head: (when it started, total ms).
    script_progress: Option<(std::time::Instant, f32)>,
    /// `progressbar_npc` over other units: id -> (elapsed ms, total ms).
    unit_progress: std::collections::HashMap<u32, (f32, f32)>,
    /// Text over units: id -> (text, age ms, speech). Speech is what a unit says aloud (chat,
    /// npctalk, unittalk); the rest are `showscript` call-outs and Talkie Boxes.
    shouts: std::collections::HashMap<u32, (String, f32, bool)>,
    /// The server's night (EFST_SKE on ourselves); some map copies are drawn at night anyway.
    server_night: bool,
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
    /// The skill reach ring's texture (skillarea::reach_texture), made on first use.
    reach_tex: Option<ro_render::TexId>,
    /// RODEX mail.
    mailbox: mail::Mailbox,
    clan: clan::Clan,
    merge: merge::Merge,
    reputation: reputation::Reputation,
    game_opts: gameopts::GameOptions,
    ranking: ranking::Ranking,
    /// The combat log and damage meter (Alt+D).
    combat: combat::Combat,
    /// MVP timers (More… > MVP Timers); the deaths live in settings.
    mvp: mvptimers::MvpTimers,
    /// When the MVP timers last checked for windows opening (unix seconds).
    mvp_checked: i64,
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
    shift: bool,
    /// Print Screen was pressed: the next frame is saved instead of shown.
    screenshot: bool,
    /// The Emotion List (Alt+L) and the macro window (Alt+M).
    emote_win: emotes::EmoteWindow,
    /// Shortcut Information: every key and what it does (from the Esc menu).
    shortcut_win: shortcuts::ShortcutWindow,
    /// Shortcut Settings: which key does what.
    key_win: keybinds::KeyWindow,
    /// Selectable item packages: the set picked when one is used.
    packages: packages::Packages,
    queued_action: Option<windows::UiAction>,
    /// A pointer move the test script queued (UI points); egui keeps the position.
    fake_pointer: Option<egui::Pos2>,
    /// Test hook (`pointmob[:class]`): the pointer stays on the nearest monster (of that
    /// class, 0 any), every frame.
    point_mob: Option<i16>,
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
    /// Guild emblems by guild id (None while being fetched, or when the fetch failed).
    emblems: std::collections::HashMap<u32, Option<egui::TextureHandle>>,
    /// The emblem version last asked for, and when, by guild id.
    emblem_versions: std::collections::HashMap<u32, (u32, std::time::Instant)>,
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
    /// A map loading on its own thread; server events wait until it is in.
    map_load: Option<MapLoad>,
    /// Snow, rain, petals... (the server's, and ambient by climate).
    weather: weather::Weather,
    /// Each walking unit's last cell, for its footsteps (prints and dust), and a step count
    /// (which foot, and a seed).
    steps: std::collections::HashMap<u32, (i32, i32)>,
    step_count: u32,
    /// Photo mode (Ctrl+P): the world still, the interface away, the camera free.
    photo: Option<photo::Photo>,
    /// --pose action,frame,facing (for captures): into photo mode once a map shows, posed so.
    pose_arg: Option<(usize, usize, usize)>,
    /// A map the loader finished (its files' name), for enter_map to use.
    preloaded: Option<(String, anyhow::Result<LoadedMap>)>,
    /// The window has keyboard focus (off: Settings' background frame rate applies).
    focused: bool,
    /// The window is minimised: the game keeps ticking, slowly, and draws nothing.
    minimised: bool,
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
        if let Some(vfs) = self.vfs.as_deref() {
            if !self.books.open(vfs, id, page, title) {
                self.hud.push_chat(ro_net::ChatKind::System, "This book cannot be read.".into());
            }
        }
    }

    /// Loads a map into the renderer and remembers its ground for picking.
    fn enter_map(&mut self, name: &str) {
        // The server's name for the map stays the world's; its files may be another map's.
        let server_name = name;
        let name = map_files(name);
        // The server sends this map's weather again once we are in.
        self.weather.clear_server();
        // Loaded on the map loader thread (start_map_load), or here and now.
        let loaded = match self.preloaded.take() {
            Some((files, loaded)) if files == name => Some(loaded),
            _ => None,
        };
        // A loading screen covers the map load (not behind the login screens or in captures).
        if loaded.is_none() && self.front.is_none() && (self.capture.is_none() || std::env::args().any(|a| a == "--front-start")) {
            if let Some(s) = self.splash.as_mut() {
                s.begin_loading(&self.egui_ctx, self.vfs.as_deref());
            }
        }
        let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) else { return };
        let loaded = loaded.unwrap_or_else(|| load_map(vfs, name, &ro_render::Upscaler::new(self.settings.upscale.clone()), r.rt_layers(), &Default::default()));
        let applying = std::time::Instant::now();
        match loaded {
            Ok(map) => {
                r.set_prepared_scene(&map.mesh, map.scene, map.lighting);
                // Captures draw no frames until theirs, so the textures would never go up over
                // frames (and the loading screen would wait on them): all now.
                if self.capture.is_some() {
                    r.finish_uploads();
                }
                let submerged = map.effects.iter().filter(|e| e.0 == 109).count() >= 40;
                let sky = sky::for_map(vfs, name, &map.lighting, submerged);
                let outdoors = sky.is_some();
                let climate = sky.as_ref().map(|s| s.name);
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
                if self.net.is_some() {
                    r.night = self.server_night || map_night(server_name);
                }
                self.world = Some(World { name: server_name.to_string(), outdoors, ground: map.mesh, gat, room: None, submerged, climate, surface: map.surface });
                self.ready_at = Some(std::time::Instant::now());
                // The part of a map load that stays on the frame loop (GPU uploads, effects).
                tracing::info!(map = name, ms = applying.elapsed().as_millis() as u64, "map applied");
            }
            Err(e) => {
                tracing::error!("map {name}: {e:#}");
                if let Some(s) = self.splash.as_mut() {
                    s.fail(format!("{server_name} could not be loaded: {e:#}"));
                }
                // Not the last map under this one's units and coordinates: nothing, and a word why.
                r.clear_map();
                self.world = None;
                // Scripted runs and --capture go on rather than wait for a map that won't come.
                self.ready_at = Some(std::time::Instant::now());
                self.hud.push_chat(ro_net::ChatKind::System, format!("{server_name} can't be shown: {e:#}. You are there on the server; warp to leave."));
            }
        }
    }

    /// The map `event` takes us to, when it needs loading (as its handler would call enter_map).
    fn map_to_load(&self, event: &ro_net::Event) -> Option<String> {
        match event {
            ro_net::Event::Entered { map, .. } => Some(map.clone()),
            ro_net::Event::MapMove { map, .. } if self.world.as_ref().is_none_or(|w| w.name != *map) => Some(map.clone()),
            _ => None,
        }
    }

    /// Loads `map` on its own thread; `event` (which asked for it) is handled once it is in.
    /// Until then the frame loop keeps drawing (the loading screen) and the old world takes no
    /// input: the server already has us on the new map.
    fn start_map_load(&mut self, map: &str, event: ro_net::Event) {
        let Some(vfs) = self.vfs.clone() else {
            self.handle(event);
            return;
        };
        if self.capture.is_none() || std::env::args().any(|a| a == "--front-start") {
            if let Some(s) = self.splash.as_mut() {
                s.begin_loading(&self.egui_ctx, Some(&vfs));
            }
        }
        // Walks, chases and held steering would go to the server in the old map's cells.
        self.walk_goal = None;
        self.hold_walk = None;
        self.attack = None;
        self.chasing = false;
        let files = map_files(map).to_string();
        let progress: Arc<std::sync::atomic::AtomicU32> = Default::default();
        let (tx, rx) = std::sync::mpsc::channel();
        let up = ro_render::Upscaler::new(self.settings.upscale.clone());
        let rt_layers = self.renderer.as_ref().and_then(|r| r.rt_layers());
        let (name, counter) = (files.clone(), progress.clone());
        let spawned = std::thread::Builder::new().name("map loader".into()).spawn(move || {
            let _ = tx.send(load_map(&vfs, &name, &up, rt_layers, &counter));
        });
        if let Err(e) = spawned {
            // No thread: load as before, on the frame loop.
            tracing::warn!("map loader thread: {e}");
            self.handle(event);
            return;
        }
        tracing::debug!(map, "map loading");
        self.map_load = Some(MapLoad { files, event, rx, progress, started: std::time::Instant::now() });
    }

    /// Once the loader is done, handles the event that asked for the map (enter_map then
    /// takes the loaded map).
    fn poll_map_load(&mut self) {
        let Some(load) = self.map_load.as_ref() else { return };
        let loaded = match load.rx.try_recv() {
            Ok(loaded) => loaded,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(anyhow::anyhow!("the map loader stopped")),
        };
        let Some(load) = self.map_load.take() else { return };
        tracing::info!(map = %load.files, ms = load.started.elapsed().as_millis() as u64, "map loaded in the background");
        self.preloaded = Some((load.files, loaded));
        self.handle(load.event);
        self.preloaded = None;
    }

    /// Actor template for a player look, cached so sheets upload once per look.
    fn player_actor(&mut self, look: &Look) -> Option<ro_render::Actor> {
        let key = format!("player:{}:{}:{}:{}:{}:{:?}", look.job, look.male, look.hair, look.hair_color, look.clothes_color, look.gear);
        if let Some(a) = self.sprites.get(&key) {
            return a.clone();
        }
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_deref()?);
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
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_deref()?);
        // Classes the original draws as 3D models: the model replaces the body sprite.
        if models3d::is_model(&name) {
            let loaded = models3d::load(vfs, r, &name).inspect_err(|e| tracing::warn!(job, "3D model {name}: {e:#}")).ok().map(std::rc::Rc::new);
            let actor = loaded.is_some().then(|| {
                let shadow = self.sprites.get("shadow").cloned().flatten().and_then(|a| a.shadow);
                ro_render::Actor { pos: Default::default(), body: ro_render::SheetId::NONE, head: None, headgear: vec![], weapon: vec![], shadow, action: 0, dir: ro_render::Dir::S, head_dir: 0, time_ms: 0.0, once: false, alpha: 1.0, scale: 1.0, tint: [1.0; 3], silhouette: None }
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
                Some(ro_render::Actor { pos: Default::default(), body, head, headgear: vec![], weapon, shadow, action: 0, dir: ro_render::Dir::S, head_dir: 0, time_ms: 0.0, once: false, alpha: 1.0, scale, tint, silhouette: None })
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
        let want = preview::Look { job: look.job.clone(), male: look.male, hair: look.hair.max(1), hair_color: look.hair_color, clothes_color: look.clothes_color, headgear };
        if self.portrait.as_ref().is_some_and(|p| p.0 == want) {
            return;
        }
        let tex = self.vfs.as_deref().and_then(|v| preview::compose(v, &want)).map(|img| self.egui_ctx.load_texture("portrait", img, egui::TextureOptions::NEAREST));
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
        self.redress(id, look);
    }

    /// Rebuilds a player's layers from their look, in their costume if they wear one, keeping
    /// position, pose and timing, and keeps the look.
    fn redress(&mut self, id: u32, look: Look) {
        let costume = self.mob_mut(id).and_then(|m| m.costume);
        let Some(fresh) = self.player_actor(&dressed(look.clone(), costume)) else { return };
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
            7 => look.clothes_color = value as u16,
            _ => return,
        }
        self.redress(id, look);
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
                    m.actor.head_dir = info.head_dir;
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
            clothes_color: info.clothes_color.max(0) as u16,
            gear: info.look,
        });
        let costume = costume_job(info.option);
        let actor = match &look {
            Some(l) => self.player_actor(&dressed(l.clone(), costume)),
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
        mob.actor.head_dir = info.head_dir;
        mob.human = is_player_class(info.job);
        mob.look = look;
        mob.costume = costume;
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

    /// Out of the game world (to character select or the login screen): what belonged to the
    /// map and character goes, and the title music plays.
    fn leave_world(&mut self) {
        self.leave_character();
        self.ping = None;
        self.units.clear();
        // Our own entry is keyed by the account id, which every character on the account
        // shares, and the server only tells a character about effects it has: kept over,
        // one character's Tenseiga aura showed on the next one picked.
        self.hat_auras.clear();
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
        if let (Some(a), Some(v)) = (self.audio.as_mut(), self.vfs.as_deref()) {
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

    /// Fetches a guild's emblem unless this version was already asked for: from the web
    /// server, or the map server when there is no web login.
    fn want_emblem(&mut self, guild: u32, version: u32) {
        // Asked already: wait for it. A fetch that came to nothing (the web server was down)
        // is tried again once half a minute has passed.
        let asked = self.emblem_versions.get(&guild).filter(|(v, _)| *v == version);
        let have = self.emblems.get(&guild).is_some_and(Option::is_some);
        if guild == 0 || asked.is_some_and(|(_, at)| have || at.elapsed() < std::time::Duration::from_secs(30)) {
            return;
        }
        self.emblem_versions.insert(guild, (version, std::time::Instant::now()));
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
                let id = self.inventory.items.get(&index).map(|i| i.id);
                self.packages.ensure_loaded(self.vfs.as_deref());
                match id {
                    // Book items open locally, as the original (no use request is sent).
                    Some(id) if self.books.is_book(id) => self.open_book(id, 1),
                    // Selectable packages ask which set first (CZ_USE_PACKAGEITEM, not a use).
                    Some(id) if self.packages.contents(id).is_some() => {
                        let items = self.packages.contents(id).unwrap_or_default();
                        self.load_icons_for(&items);
                        self.packages.open(index, id);
                    }
                    _ => self.send(Command::Item(ItemRequest::Use { index, account_id: self.account_id })),
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
                if let Some(vfs) = self.vfs.as_deref() {
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
        let Some(s) = self.skills.iter().find(|s| s.id == id) else {
            tracing::debug!(skill = id, "skill key: not a skill we have");
            return;
        };
        let level = level.clamp(1, s.level.max(1));
        if s.target & 4 != 0 {
            self.use_skill_on(id, level, self.account_id);
        } else if s.target & (1 | 2 | 16 | 32) != 0 {
            self.aiming = Some((id, level));
        } else {
            tracing::debug!(skill = id, flags = s.target, "skill key: nothing to use (passive)");
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
        tracing::debug!(skill, level, target, "use skill");
        self.focus_on(target);
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
        let (r, vfs) = (self.renderer.as_mut()?, self.vfs.as_deref()?);
        let base = format!("data\\sprite\\아이템\\{res}");
        let spr = vfs.read(&format!("{base}.spr")).ok().flatten().and_then(|d| ro_formats::spr::Spr::parse(&d).ok());
        let act = vfs.read(&format!("{base}.act")).ok().flatten().and_then(|d| ro_formats::act::Act::parse(&d).ok());
        let actor = match (spr, act) {
            (Some(spr), Some(act)) => {
                let body = r.add_sheet(&spr, act, None);
                Some(ro_render::Actor { pos: Default::default(), body, head: None, headgear: vec![], weapon: vec![], shadow: None, action: 0, dir: ro_render::Dir::S, head_dir: 0, time_ms: 0.0, once: false, alpha: 1.0, scale: 1.0, tint: [1.0; 3], silhouette: None })
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
        let (Some(db), Some(vfs)) = (self.item_db.as_ref(), self.vfs.as_deref()) else { return };
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

    /// Loads inventory icons not yet loaded into egui textures (see `load_item_icons`).
    fn load_icons(&mut self) {
        let items: Vec<(u32, bool)> = self.inventory.items.values().map(|i| (i.id, i.identified)).collect();
        self.load_item_icons(&items);
    }

    /// Loads storage icons not yet loaded: the storage window draws from the same icons as
    /// the inventory, so items only in storage had none.
    fn load_storage_icons(&mut self) {
        let items: Vec<(u32, bool)> = self.social.storage.items.values().map(|i| (i.id, i.identified)).collect();
        self.load_item_icons(&items);
    }

    /// Icons for (item id, identified) not yet loaded: an item's own icon, and for
    /// unidentified ones their unidentified look as well (kept apart, so identifying one swaps
    /// the icon).
    fn load_item_icons(&mut self, items: &[(u32, bool)]) {
        let ids: Vec<u32> = items.iter().map(|i| i.0).collect();
        self.load_icons_for(&ids);
        let (Some(db), Some(vfs)) = (self.item_db.as_ref(), self.vfs.as_deref()) else { return };
        let unid: Vec<u32> = items.iter().filter(|i| !i.1).map(|i| i.0).collect();
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
        let Some(vfs) = self.vfs.as_deref() else { return };
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
        if let (Some(a), Some(vfs)) = (self.audio.as_ref(), self.vfs.as_deref()) {
            a.play(vfs, name);
        }
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
            "d" => self.combat.open = !self.combat.open,
            "mvp" => {
                self.navi.ensure_loaded(self.vfs.as_deref());
                self.mvp.open = !self.mvp.open;
            }
            "clan" => self.clan.open = !self.clan.open,
            "repute" => {
                self.reputation.load(self.vfs.as_deref());
                self.reputation.open = !self.reputation.open;
            }
            "opts" => self.game_opts.open = !self.game_opts.open,
            "keys" => self.shortcut_win.open = !self.shortcut_win.open,
            "bindings" => self.key_win.open = !self.key_win.open,
            "rank" => self.send(ro_net::Command::Ranking(self.ranking.kind)),
            "roulette" => self.open_roulette(),
            "battle" => {
                self.battle.ensure_loaded(self.vfs.as_deref());
                self.battle.open = !self.battle.open;
            }
            "navi" => {
                self.navi.ensure_loaded(self.vfs.as_deref());
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

    /// Attacks a monster until it dies or we pick something else.
    /// The target frame turns to a monster we attack or cast on.
    fn focus_on(&mut self, id: u32) {
        if self.units.get(&id).is_some_and(|m| m.kind == 5) {
            self.focus = Some((id, std::time::Instant::now()));
        }
    }

    fn attack_unit(&mut self, id: u32) {
        self.walk_goal = None;
        self.pending_npc = None;
        self.attack = Some((id, self.attack_range as i16));
        self.focus_on(id);
        if let Some((tx, _)) = self.net.as_ref() {
            let _ = tx.send(ro_net::Command::Attack { target: id, keep: true });
        }
    }

    /// Applies an NPC script step to the dialog window.
    fn npc_dialog(&mut self, d: ro_net::NpcDialog) {
        use hud::DialogMode;
        use ro_net::NpcDialog::*;
        tracing::debug!(?d, "npc dialog");
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
            // rAthena's `end` closes the dialog of every script that ran on us, so event
            // scripts (OnPCLoadMapEvent...) send closes for NPCs we never talked to: those are
            // not ours to answer, and must not take over the dialog that is open.
            Close { npc } if self.dialog.as_ref().is_none_or(|d| d.npc != npc) => {}
            Close { npc } if self.dialog.as_ref().is_some_and(|d| d.lines.is_empty()) => {
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
        let vfs = self.vfs.as_deref()?;
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
            self.turn_to(b);
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
                settings::RAY_TRACING.store(r.ray_tracing(), std::sync::atomic::Ordering::Relaxed);
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
        if let (Some(r), Some(vfs)) = (self.renderer.as_mut(), self.vfs.as_deref()) {
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
        self.splash = Some(splash::Splash::new(&self.egui_ctx, self.vfs.as_deref()));
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
        // Android in the background keeps its Wait; ticking there only notices quitting.
        if self.paused {
            self.tick(el);
            return;
        }
        let Some(gap) = self.frame_gap() else {
            el.set_control_flow(winit::event_loop::ControlFlow::Poll);
            self.tick(el);
            return;
        };
        // Woken early (a window event): wait out the rest of the gap.
        if std::time::Instant::now() >= self.last_tick + gap {
            self.tick(el);
        }
        el.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(self.last_tick + gap));
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // No surface to draw or resize while in the background (Android).
        if self.paused {
            return;
        }
        match &event {
            WindowEvent::Focused(focused) => self.focused = *focused,
            WindowEvent::Resized(s) => self.minimised = s.width == 0 || s.height == 0,
            _ => {}
        }
        self.note_keyboard_mouse(&event);
        // Shortcut Settings waiting for a key: the next press becomes the binding, before the UI
        // or the game sees it (Esc removes the binding, as in the original).
        if self.key_win.capturing.is_some() {
            if let WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. }, .. } = &event {
                if *logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                    self.key_win.captured(&mut self.settings.keys, None);
                } else if let Some(name) = keybinds::name(logical_key, self.ctrl, self.alt, self.shift) {
                    self.key_win.captured(&mut self.settings.keys, Some(name));
                }
                self.settings.save();
                return;
            }
        }
        let (Some(r), Some(window)) = (self.renderer.as_mut(), self.window.as_ref()) else { return };
        // Alt+letter shortcuts go straight to the game, even while the chat has the keyboard
        // (AltGr, which is Ctrl+Alt, still types into the UI).
        let alt_shortcut = self.alt && !self.ctrl && matches!(&event, WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key: winit::keyboard::Key::Character(_), .. }, .. });
        // The UI sees input first; clicks and keys it uses don't reach the game.
        let ui_used = !alt_shortcut && self.egui_state.as_mut().is_some_and(|s| s.on_window_event(window, &event).consumed);
        // Track the left button ourselves (hold-to-walk), whatever the UI did with the event.
        if let WindowEvent::MouseInput { state, button: MouseButton::Left, .. } = &event {
            self.left_down = *state == ElementState::Pressed;
            if !self.left_down {
                self.turn_drag = None;
            }
        }
        if let WindowEvent::CursorMoved { position, .. } = &event {
            // Keep tracking the cursor even over UI so rotation deltas stay continuous.
            if ui_used {
                self.cursor = Some((position.x, position.y));
            }
        }
        // Login / character screens: the world takes no input.
        // So does the old world while the next map loads (start_map_load).
        let front_blocks = (self.front.is_some() || self.map_load.is_some()) && matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. } | WindowEvent::KeyboardInput { .. });
        let function_key = matches!(&event, WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key: winit::keyboard::Key::Named(k), .. }, .. } if hotkey_slot(*k).is_some() || matches!(*k, winit::keyboard::NamedKey::F10 | winit::keyboard::NamedKey::F11 | winit::keyboard::NamedKey::F12));
        let ui_used = ui_used && !function_key;
        // Arranging the interface: clicks and the wheel are for the layout, not the world.
        let arranging = self.arrange.open && matches!(event, WindowEvent::MouseInput { .. } | WindowEvent::MouseWheel { .. });
        let ui_used = ui_used || arranging;
        if (ui_used || front_blocks) && !matches!(event, WindowEvent::Resized(_) | WindowEvent::CloseRequested | WindowEvent::ScaleFactorChanged { .. }) {
            return;
        }
        // Photo mode: its keys move the camera (held), Esc leaves; clicks don't walk.
        if let (Some(p), WindowEvent::KeyboardInput { event: winit::event::KeyEvent { logical_key, state, .. }, .. }) = (self.photo.as_mut(), &event) {
            if *logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                if *state == ElementState::Pressed {
                    self.leave_photo();
                }
                return;
            }
            if p.key(logical_key, *state == ElementState::Pressed) {
                return;
            }
        }
        if self.photo.is_some() && matches!(event, WindowEvent::MouseInput { button: MouseButton::Left, .. }) {
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
                self.shift = m.state().shift_key();
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
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => self.left_click(),
            WindowEvent::CursorMoved { position, .. } => {
                if let (true, Some((lx, ly))) = (self.rotating, self.cursor) {
                    self.right_drag += (position.x - lx).hypot(position.y - ly);
                    // Scale by DPI so rotation speed matches across monitors.
                    let s = 0.005 / r.scale_factor as f32;
                    r.camera.rotate((position.x - lx) as f32 * s, (position.y - ly) as f32 * s);
                }
                self.cursor = Some((position.x, position.y));
                if self.left_down && self.turn_drag.is_some() {
                    self.drag_turn();
                }
            }
            _ => {}
        }
    }
}

impl App {
    /// Into photo mode, or out of it.
    pub(crate) fn toggle_photo(&mut self) {
        if self.photo.is_some() {
            self.leave_photo();
            return;
        }
        let Some(r) = self.renderer.as_mut() else { return };
        if self.world.is_none() {
            return;
        }
        self.photo = Some(photo::Photo::new(r.camera, &r.post_settings));
        r.camera.free = true;
        r.set_frozen(true);
        self.hud.push_chat(ro_net::ChatKind::System, "Photo mode: WASD moves, right-drag turns, the wheel zooms, H hides the panel, Esc leaves".into());
    }

    pub(crate) fn leave_photo(&mut self) {
        let Some(p) = self.photo.take() else { return };
        if let Some(r) = self.renderer.as_mut() {
            let was = p.camera_before();
            r.camera = ro_render::Camera { target: r.camera.target, ..was };
            r.set_frozen(false);
        }
    }
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
mod quake_tests {
    use std::time::{Duration, Instant};

    #[test]
    fn jolts_change_sides_on_a_fixed_beat_while_a_quake_runs() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let mut quakes = vec![(at(5000), at(6000))];
        assert_eq!(super::quake_offset(&mut quakes, t0, 0.0), glam::Vec3::ZERO, "not started yet");
        quakes.push((t0, at(650)));
        let (a, same, b) = (super::quake_offset(&mut quakes, at(1), 0.0), super::quake_offset(&mut quakes, at(20), 0.0), super::quake_offset(&mut quakes, at(40), 0.0));
        assert_eq!(a, same, "steady within a beat, whatever the frame rate");
        assert!(a.x > 0.0 && b.x < 0.0);
        assert!((0.13..=0.22).contains(&(a.x / ro_render::CELL)));
        assert_eq!(super::quake_offset(&mut quakes, at(700), 0.0), glam::Vec3::ZERO);
        assert_eq!(quakes.len(), 1, "the finished quake is dropped");
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

/// The costume an OPTION_* state dresses a player in, as a job: wedding, Xmas, summer, hanbok,
/// Oktoberfest, summer 2 (rAthena's @costume and the wedding). The original client swaps the
/// body itself; the server sends no job change for it.
fn costume_job(option: u32) -> Option<i16> {
    [(0x1000, 22), (0x1_0000, 26), (0x4_0000, 27), (0x0800_0000, 28), (0x1000_0000, 29), (0x2000_0000, 30)]
        .into_iter()
        .find(|&(bit, _)| option & bit != 0)
        .map(|(_, job)| job)
}

/// A look in its costume: the costume's body, and no weapon or shield.
fn dressed(mut look: Look, costume: Option<i16>) -> Look {
    // shortcut: Doram keep their own body (their few costume sprites live under 도람족), until wanted.
    if let Some(job) = costume.filter(|_| jobs::race_dir(&look.job) != "도람족") {
        look.job = job_sprite(job).to_string();
        (look.gear.weapon, look.gear.shield) = (0, 0);
    }
    look
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
