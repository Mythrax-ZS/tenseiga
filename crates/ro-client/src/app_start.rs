//! Start-up: the config, the window and the event loop. Part of the client (app.rs).

use super::*;

pub(crate) fn run() -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    // The console, and a file: a release build on Windows is a windowed app with no console.
    let config = std::env::var_os("ROCLIENT_CONFIG").map(PathBuf::from);
    let path = log_path(config.as_deref());
    let path = std::path::absolute(&path).unwrap_or(path);
    let (file, error) = match std::fs::File::create(&path) {
        Ok(f) => (Some(tracing_subscriber::fmt::layer().with_ansi(false).with_writer(std::sync::Mutex::new(f))), None),
        Err(e) => (None, Some(e)),
    };
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(file)
        .init();
    match error {
        None => tracing::info!(path = %path.display(), "log file"),
        Some(e) => tracing::warn!(path = %path.display(), "can't write the log file: {e}"),
    }
    // Panics reach the file too, then the usual message (and backtrace) on the console.
    let console = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("panic: {info}");
        console(info);
    }));
    start(None)
}

/// tenseiga.log in the working folder (the one holding config\local.toml, see main.rs), or
/// beside the config ROCLIENT_CONFIG names (the AppImage's ~/.config/tenseiga). A second
/// client on another config (config\local2.toml) logs to tenseiga-local2.log, not over the first.
fn log_path(config: Option<&std::path::Path>) -> PathBuf {
    let Some(config) = config else {
        return PathBuf::from("tenseiga.log");
    };
    let name = match config.file_stem().and_then(|s| s.to_str()) {
        Some("local") | None => "tenseiga.log".to_string(),
        Some(stem) => format!("tenseiga-{stem}.log"),
    };
    config.with_file_name(name)
}

/// Loads the config and runs the client. Android hands over the event loop its activity made.
pub(crate) fn start(event_loop: Option<EventLoop<()>>) -> anyhow::Result<()> {
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
    let vfs = open_vfs(&cfg)?.map(Arc::new);
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
        look: Look { job: arg("--job").unwrap_or_else(|| "초보자".into()), male: arg("--sex").is_none_or(|s| s != "f"), hair: 1, hair_color: 0, clothes_color: arg("--dye").and_then(|d| d.parse().ok()).unwrap_or(0), gear: ro_net::Look { weapon: arg("--weapon").and_then(|w| w.parse().ok()).unwrap_or(0), shield: arg("--shield").and_then(|w| w.parse().ok()).unwrap_or(0), ..Default::default() } },
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
        server_night: false,
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
        shift: false,
        prayer_at: 0,
        doridori: None,
        turn_drag: None,
        quakes: vec![],
        spark_seed: 1,
        screenshot: false,
        emote_win: Default::default(),
        shortcut_win: Default::default(),
        key_win: Default::default(),
        packages: Default::default(),
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
        focused: true,
        minimised: false,
        map_load: None,
        weather: weather::Weather::with_forced(arg("--weather").map(|w| w.split(',').filter_map(weather_kind).collect()).unwrap_or_default()),
        preloaded: None,
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
    app.class_names = app.vfs.as_deref().map(load_class_names).unwrap_or_default();
    app.pet_talk = app.vfs.as_deref().map(pettalk::PetTalk::load).unwrap_or_default();
    app.books = app.vfs.as_deref().map(book::Books::load).unwrap_or_default();
    app.guild.client_dir = app.client_dir.clone();
    app.hat_files = app.vfs.as_deref().map(fx::hat_effects).unwrap_or_default();
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
                db.extend(customdata::iteminfo(&src, p, &read));
            }
        }
        if !db.is_empty() {
            tracing::info!(items = db.len(), "loaded item info");
            app.item_db = Some(db);
        }
        if let Some(src) = read(customdata::CUSTOMJOBS_SOURCE) {
            jobs::set_custom(customdata::custom_jobs(&src, customdata::CUSTOMJOBS_SOURCE));
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

#[cfg(test)]
mod log_path_tests {
    use std::path::{Path, PathBuf};

    #[test]
    fn the_log_sits_beside_its_config() {
        assert_eq!(super::log_path(None), PathBuf::from("tenseiga.log"), "the working folder by default");
        let appimage = Path::new("/home/deck/.config/tenseiga/local.toml");
        assert_eq!(super::log_path(Some(appimage)), Path::new("/home/deck/.config/tenseiga/tenseiga.log"));
        let second = Path::new("config/local2.toml");
        assert_eq!(super::log_path(Some(second)), Path::new("config/tenseiga-local2.log"), "a second client keeps its own log");
    }
}

/// `--weather` names: snow, rain, petals, leaves, fog, clouds, fireworks.
fn weather_kind(name: &str) -> Option<weather::Sky> {
    use ro_render::WeatherKind::*;
    if name.trim() == "fireworks" {
        return Some(weather::Sky::Fireworks);
    }
    Some(weather::Sky::Layer(match name.trim() {
        "snow" => Snow,
        "rain" => Rain,
        "petals" | "sakura" => Petals,
        "leaves" | "maple" => Leaves,
        "fog" => Fog,
        "clouds" => Clouds,
        _ => return None,
    }))
}
