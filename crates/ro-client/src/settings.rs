//! Settings the player changes in game, saved as settings.toml next to the config file
//! (which stays hand-edited). Values there override the config's defaults.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Whether this device can ray trace (set once the renderer is up); the ray tracing settings
/// are greyed out where it can't.
pub static RAY_TRACING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The background frame rate choices (0: no cap).
const BACKGROUND_FPS: [u32; 4] = [15, 30, 60, 0];

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub bgm_volume: f32,
    pub sfx_volume: f32,
    pub ui_scale: f32,
    pub minimap: bool,
    /// The top-right menu bar is collapsed.
    pub menu_collapsed: bool,
    /// Classic: windows drawn with the original client's art where available; Modern: plain egui.
    pub classic_ui: bool,
    /// Login screen: the last ID used ("Remember ID"), empty when not remembered.
    pub last_id: String,
    /// The HD-2D look (lighting, shadows, post-processing).
    pub visual: Visual,
    /// Texture upscaling for ground, models, item icons and interface art. Character and
    /// effect sprites are never upscaled (see ro_render::upscale).
    pub upscale: ro_render::UpscaleSettings,
    /// The original's /bgm, /sound and /effect switches (volumes are kept while off).
    pub bgm_off: bool,
    pub sfx_off: bool,
    pub effects_off: bool,
    /// /aura2: no level aura at all; /aura: the simplified aura (its sparkles alone).
    pub aura_off: bool,
    pub aura_simple: bool,
    /// /bm: letter keys use the shortcut bar (Q..O, A..L, Z..>) and Enter or Space opens chat.
    pub battle_mode: bool,
    /// Battle mode keys for the second, third and fourth shortcut rows, one per slot.
    pub battle_keys: Vec<String>,
    /// Battle mode: Space opens the chat (as the original); off, Space is free for a slot.
    pub battle_space_chat: bool,
    /// Shortcut bar rows on show (F12 cycles 1..4 and hidden).
    pub hotkey_rows: u8,
    /// What Alt+1..Alt+0 say (the macro window, Alt+M).
    pub macros: Vec<String>,
    /// Screenshots (Print Screen) include the interface; off, they show the world alone.
    pub screenshot_ui: bool,
    /// The performance overlay (F10): frame rate, frame times, ping.
    pub perf_overlay: bool,
    /// Each chat line shows the time it arrived.
    pub chat_timestamps: bool,
    /// Chat is also saved to Chat\<character> <date>.txt (see chatlog).
    pub chat_log: bool,
    /// Frame rate cap while another window has focus (0: none). Minimised, nothing is drawn.
    pub background_fps: u32,
    /// Borderless full screen (F11).
    pub fullscreen: bool,
    /// Experience gained from monsters in the chat (the original's "Experience message" chat
    /// option, off by default). Quest rewards always show.
    pub exp_messages: bool,
    /// Controller button icons: the connected pad's family, or one chosen.
    pub pad_style: crate::padglyph::Choice,
    /// Controller rumble strength (0 off, 1 full).
    pub vibration: f32,
    /// A PlayStation pad's touchpad moves the pointer.
    pub touchpad: bool,
    /// A PlayStation pad's light bar shows our HP.
    pub light_bar: bool,
    /// A wired DualSense plays vibration on its haptic actuators (HD); off, as classic
    /// rumble emulation.
    pub dualsense_hd: bool,
    /// The vibration test button was pressed (not saved).
    #[serde(skip)]
    pub vibration_test: bool,
    /// Deadzones, camera speed and response, triggers, stick swap, touchpad speed.
    pub pad_tuning: crate::gamepad::Tuning,
    /// The connected controller's sticks as read, for the preview (not saved).
    #[serde(skip)]
    pub pad_live: Option<[glam::Vec2; 2]>,
    /// Where the HUD elements sit and how big they are (Arrange; see hudlayout).
    pub hud_layout: crate::hudlayout::Layout,
    /// Shortcut Settings: keys moved from their defaults (action id -> key, "" for none).
    pub keys: crate::keybinds::Keys,
    /// Effects such as Meteor Storm and Earth Spike shake the screen, as in the original.
    pub screen_shake: bool,
    /// Arrange was pressed: the game opens the layout editor (not saved).
    #[serde(skip)]
    pub arrange: bool,
    /// Shortcut Settings was pressed: the game opens it (not saved).
    #[serde(skip)]
    pub open_keys: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { bgm_volume: 0.5, sfx_volume: 0.7, ui_scale: 1.0, minimap: true, perf_overlay: false, background_fps: 30, chat_timestamps: false, chat_log: false, hud_layout: Default::default(), keys: Default::default(), screen_shake: true, arrange: false, open_keys: false, menu_collapsed: false, classic_ui: true, last_id: String::new(), visual: Visual::default(), upscale: ro_render::UpscaleSettings::default(), bgm_off: false, sfx_off: false, effects_off: false, aura_off: false, aura_simple: false, battle_mode: false, battle_keys: crate::windows::BATTLE_KEYS.iter().map(|k| k.to_string()).collect(), battle_space_chat: true, hotkey_rows: 1, macros: crate::emotes::DEFAULT_MACROS.iter().map(|m| m.to_string()).collect(), screenshot_ui: true, fullscreen: false, exp_messages: false, pad_style: Default::default(), vibration: 1.0, touchpad: true, light_bar: true, dualsense_hd: true, vibration_test: false, pad_tuning: Default::default(), pad_live: None }
    }
}

/// Artist-facing look controls, mirrored into the renderer's `PostSettings`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(default)]
pub struct Visual {
    /// Off: the classic look (no bloom, blur, grading or sprite shadows).
    pub hd2d: bool,
    pub bloom: f32,
    pub depth_of_field: f32,
    pub vignette: f32,
    pub shadows: f32,
    pub sun_elevation: f32,
    pub sun_cycle: f32,
    /// Water quality preset: 0 Classic, 1 Low, 2 Medium, 3 High, 4 Ultra.
    pub water: u8,
    pub warmth: f32,
    pub saturation: f32,
    pub contrast: f32,
    pub sprite_light: f32,
    pub rim_light: f32,
    pub point_lights: f32,
    pub fog: f32,
    pub god_rays: f32,
    pub ambient_occlusion: f32,
    /// Exposure follows the scene's brightness, as eyes adapt (0 fixed .. 1 fully).
    pub auto_exposure: f32,
    /// Relief read from ground and model textures, lit by the sun and lamps (0 flat).
    pub relief: f32,
    /// Sun shadows ray traced from all map geometry (needs a ray-tracing GPU).
    pub ray_traced_shadows: bool,
    /// Water reflections ray traced from the map (needs a ray-tracing GPU).
    pub ray_traced_reflections: bool,
    pub pixel_snap: bool,
    /// Strength of the underwater look on submerged maps (caustics, murk, shafts, motes).
    pub underwater: f32,
    /// The world's resolution as a share of the screen's (0.5..2): lower is faster, above 100%
    /// supersamples (the smoothest edges). The interface stays sharp either way.
    pub render_scale: f32,
    /// Picks the render scale from the screen: about a million world pixels (a 1080p phone
    /// at 70%, a 1440p one at 50%), full resolution on smaller screens. On by default on
    /// Android, where dense screens and per-pixel GPU cost meet.
    pub render_auto: bool,
    /// Snow, rain, petals...: the server's alone, or also the client's own by climate.
    pub weather: crate::weather::WeatherMode,
    /// Shadows of clouds drifting over outdoor maps (0 off .. 1 deep).
    pub cloud_shadows: f32,
    /// Trees, grass and flowers swaying in the wind (0 still .. 1 lively).
    pub foliage_sway: f32,
}

impl Default for Visual {
    fn default() -> Self {
        let p = ro_render::PostSettings::default();
        Self {
            hd2d: p.enabled,
            bloom: p.bloom_intensity,
            depth_of_field: p.dof_strength,
            vignette: p.vignette,
            shadows: p.shadow_strength,
            sun_elevation: p.sun_elevation,
            sun_cycle: p.sun_cycle_minutes,
            water: 3,
            weather: Default::default(),
            cloud_shadows: 0.6,
            foliage_sway: 0.7,
            warmth: p.warmth,
            saturation: p.saturation,
            contrast: p.contrast,
            sprite_light: p.sprite_light,
            rim_light: p.rim_light,
            point_lights: p.point_lights,
            fog: p.fog,
            god_rays: p.god_rays,
            ambient_occlusion: p.ambient_occlusion,
            auto_exposure: 0.5,
            relief: 6.0,
            ray_traced_shadows: !cfg!(target_os = "android"),
            ray_traced_reflections: !cfg!(target_os = "android"),
            pixel_snap: p.pixel_snap,
            underwater: 1.0,
            render_scale: if cfg!(target_os = "android") { 0.7 } else { 1.0 },
            render_auto: cfg!(target_os = "android"),
        }
    }
}

impl Visual {
    /// The water preset in use; the classic look keeps classic water.
    pub fn water_settings(&self) -> ro_render::WaterSettings {
        let q = if self.hd2d { ro_render::WaterQuality::ALL[(self.water as usize).min(4)] } else { ro_render::WaterQuality::Classic };
        ro_render::WaterSettings::preset(q)
    }

    pub fn post(&self) -> ro_render::PostSettings {
        ro_render::PostSettings {
            enabled: self.hd2d,
            bloom_intensity: self.bloom,
            dof_strength: self.depth_of_field,
            vignette: self.vignette,
            shadow_strength: self.shadows,
            sun_elevation: self.sun_elevation,
            sun_cycle_minutes: self.sun_cycle,
            warmth: self.warmth,
            saturation: self.saturation,
            contrast: self.contrast,
            sprite_light: self.sprite_light,
            rim_light: self.rim_light,
            point_lights: self.point_lights,
            fog: self.fog,
            god_rays: self.god_rays,
            ambient_occlusion: self.ambient_occlusion,
            auto_exposure: self.auto_exposure,
            relief: self.relief,
            ray_traced_shadows: self.ray_traced_shadows,
            ray_traced_reflections: self.ray_traced_reflections,
            pixel_snap: self.pixel_snap,
            ..Default::default()
        }
    }
}

fn path() -> PathBuf {
    let config = std::env::var("ROCLIENT_CONFIG").unwrap_or_else(|_| "config/local.toml".into());
    PathBuf::from(config).with_file_name("settings.toml")
}

impl Settings {
    /// The saved settings, or `defaults` if none were saved yet.
    pub fn load(defaults: Settings) -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|t| toml::from_str(&t).inspect_err(|e| tracing::warn!("settings.toml: {e}")).ok())
            .unwrap_or(defaults)
    }

    pub fn save(&self) {
        let text = toml::to_string(self).expect("settings serialize");
        if let Err(e) = std::fs::write(path(), text) {
            tracing::warn!("save settings: {e}");
        }
    }

    /// The settings window, in tabs (Game, Controls, Graphics) that scroll when the screen is
    /// short. Returns true when something changed (already saved).
    pub fn show(&mut self, ctx: &egui::Context, open: &mut bool) -> bool {
        const TABS: [&str; 3] = ["Game", "Controls", "Graphics"];
        let before = self.clone();
        let tab_id = egui::Id::unique("settings tab");
        let mut tab: usize = ctx.data(|d| d.get_temp(tab_id)).unwrap_or(0);
        // Room for the window inside the screen, below its title bar.
        let max_height = (ctx.content_rect().height() - 140.0).clamp(220.0, 620.0);
        crate::classic::show(ctx, egui::Window::new("Settings").open(open).resizable(false).collapsible(false).default_width(420.0).default_pos(egui::pos2(ctx.content_rect().center().x - 210.0, 70.0)), "Settings", true, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in TABS.iter().enumerate() {
                    ui.selectable_value(&mut tab, i, *name);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(max_height).min_scrolled_height(max_height.min(500.0)).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(400.0);
                egui::Grid::new(("settings", tab)).num_columns(2).spacing([16.0, 8.0]).min_col_width(130.0).show(ui, |ui| match tab {
                    0 => self.game_tab(ui),
                    1 => self.controls_tab(ui),
                    _ => self.graphics_tab(ui),
                });
            });
        });
        ctx.data_mut(|d| d.insert_temp(tab_id, tab));
        let plain = |s: &Settings| Settings { vibration_test: false, pad_live: None, ..s.clone() };
        let changed = plain(self) != plain(&before);
        if changed {
            self.save();
        }
        changed
    }

    fn game_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Sound");
        ui.label("Music");
        ui.add(egui::Slider::new(&mut self.bgm_volume, 0.0..=1.0).show_value(false));
        ui.end_row();
        ui.label("Effects");
        ui.add(egui::Slider::new(&mut self.sfx_volume, 0.0..=1.0).show_value(false));
        ui.end_row();
        section(ui, "Interface");
        ui.label("Window");
        ui.checkbox(&mut self.fullscreen, "Full screen (F11)");
        ui.end_row();
        ui.label("Performance");
        ui.checkbox(&mut self.perf_overlay, "Frame rate, frame times and ping (F10)");
        ui.end_row();
        ui.label("Chat");
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.chat_timestamps, "Time stamps");
            ui.checkbox(&mut self.chat_log, "Save to file")
                .on_hover_text("Each character's chat goes to Chat\\<name> <date>.txt in the game folder (beside ScreenShot), a file a day");
        });
        ui.end_row();
        ui.label("In the background");
        let fps_name = |fps: u32| if fps == 0 { "No frame rate cap".to_string() } else { format!("Up to {fps} fps") };
        egui::ComboBox::from_id_salt("background_fps").selected_text(fps_name(self.background_fps)).show_ui(ui, |ui| {
            for fps in BACKGROUND_FPS {
                ui.selectable_value(&mut self.background_fps, fps, fps_name(fps));
            }
        })
        .response
        .on_hover_text("While another window has focus; less work for the CPU and GPU. Minimised, the game draws nothing.");
        ui.end_row();
        ui.label("Layout");
        ui.horizontal(|ui| {
            if ui.button("Arrange…").on_hover_text("Move and resize the character status, chat, shortcut bar, menu, minimap and the rest").clicked() {
                self.arrange = true;
            }
            if ui.add_enabled(!self.hud_layout.is_empty(), egui::Button::new("Reset")).clicked() {
                self.hud_layout.clear();
            }
        });
        ui.end_row();
        ui.label("UI scale");
        // Applied on release, so the slider doesn't move under the pointer while dragging.
        let mut scale = self.ui_scale;
        let r = ui.add(egui::Slider::new(&mut scale, 0.75..=2.0).step_by(0.05));
        if r.drag_stopped() || (r.changed() && !r.dragged()) {
            self.ui_scale = scale;
        }
        ui.end_row();
        ui.label("Style");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.classic_ui, true, "Classic");
            ui.selectable_value(&mut self.classic_ui, false, "Modern");
        });
        ui.end_row();
        ui.label("Minimap");
        ui.checkbox(&mut self.minimap, "Show (Ctrl+Tab)");
        ui.end_row();
        ui.label("Effects");
        ui.checkbox(&mut self.screen_shake, "Screen shake").on_hover_text("Meteor Storm, Earth Spike, Heaven's Drive and other heavy effects shake the screen, as in the original");
        ui.end_row();
        ui.label("Chat");
        ui.checkbox(&mut self.exp_messages, "Experience messages").on_hover_text("Experience gained from monsters; quest rewards always show");
        ui.end_row();
        section(ui, "Screenshots");
        ui.label("Print Screen");
        ui.checkbox(&mut self.screenshot_ui, "Include the interface").on_hover_text("Print Screen or Scroll Lock saves a lossless PNG in the ScreenShot folder");
        ui.end_row();
    }

    fn controls_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Controller");
        ui.label("Button icons");
        {
            use crate::padglyph::Choice;
            let label = |c: Choice| match c {
                Choice::Auto => "Match the controller",
                Choice::Xbox => "Xbox",
                Choice::PlayStation => "PlayStation",
                Choice::Nintendo => "Nintendo",
            };
            egui::ComboBox::from_id_salt("pad style").selected_text(label(self.pad_style)).show_ui(ui, |ui| {
                for c in [Choice::Auto, Choice::Xbox, Choice::PlayStation, Choice::Nintendo] {
                    ui.selectable_value(&mut self.pad_style, c, label(c));
                }
            });
        }
        ui.end_row();
        let live = self.pad_live;
        let t = &mut self.pad_tuning;
        let pct = |v: f64, _| format!("{:.0}%", v * 100.0);
        for (i, name) in ["Left stick", "Right stick"].into_iter().enumerate() {
            ui.label(name);
            ui.horizontal(|ui| {
                let dz = if i == 0 { &mut t.left_deadzone } else { &mut t.right_deadzone };
                stick_preview(ui, live.map(|l| l[i]), *dz, t.outer_deadzone);
                ui.vertical(|ui| {
                    ui.add(egui::Slider::new(dz, 0.0..=0.5).custom_formatter(pct).text("deadzone")).on_hover_text("Travel ignored around the centre. Raise it if the character or camera drifts on its own; lower it for a quicker response");
                    let role = if (i == 1) != t.swap_sticks { "Turns the camera" } else { "Walks" };
                    ui.label(egui::RichText::new(role).weak());
                });
            });
            ui.end_row();
        }
        ui.label("Full tilt");
        ui.add(egui::Slider::new(&mut t.outer_deadzone, 0.0..=0.3).custom_formatter(|v, _| format!("at {:.0}%", (1.0 - v) * 100.0)))
            .on_hover_text("How far a stick leans before it reads as fully pushed (an outer deadzone, for sticks that no longer reach the rim)");
        ui.end_row();
        ui.label("Swap sticks");
        ui.checkbox(&mut t.swap_sticks, "Walk with the right stick, turn with the left");
        ui.end_row();
        ui.label("Camera speed");
        ui.vertical(|ui| {
            let times = |v: f64, _| format!("{v:.1}×");
            ui.add(egui::Slider::new(&mut t.turn_speed, 0.2..=3.0).custom_formatter(times).text("turn"));
            ui.add(egui::Slider::new(&mut t.tilt_speed, 0.2..=3.0).custom_formatter(times).text("tilt"));
        });
        ui.end_row();
        ui.label("Camera response");
        ui.add(egui::Slider::new(&mut t.curve, 1.0..=3.0).custom_formatter(|v, _| if v <= 1.0 { "Linear".into() } else { format!("Curved {v:.1}") }))
            .on_hover_text("Linear: speed follows the stick. Curved: gentle near the centre for fine turns, still fast at full tilt");
        ui.end_row();
        ui.label("Invert camera");
        ui.horizontal(|ui| {
            ui.checkbox(&mut t.invert_turn, "Turn");
            ui.checkbox(&mut t.invert_tilt, "Tilt");
        });
        ui.end_row();
        ui.label("Triggers");
        ui.add(egui::Slider::new(&mut t.trigger, 0.1..=0.9).custom_formatter(pct).text("press point"))
            .on_hover_text("How far LT / RT (L2 / R2, ZL / ZR) press before the shortcut row opens");
        ui.end_row();
        ui.label("");
        if ui.button("Reset controller tuning").clicked() {
            *t = Default::default();
        }
        ui.end_row();
        ui.label("Vibration");
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut self.vibration, 0.0..=1.0).custom_formatter(|v, _| if v <= 0.0 { "Off".into() } else { format!("{:.0}%", v * 100.0) }))
                .on_hover_text("Rumble when you are hit, land a critical, level up or run low on HP. HD rumble on Switch Pro Controllers; a wired DualSense uses its haptic motors.");
            if ui.button("Test").on_hover_text("A hit, a critical and a level up, on the controller in use").clicked() {
                self.vibration_test = true;
            }
        });
        ui.end_row();
        ui.label("PlayStation");
        ui.vertical(|ui| {
            ui.checkbox(&mut self.touchpad, "Touchpad moves the pointer").on_hover_text("Press to click, two fingers to right-click; swipe two fingers across to change the shortcut row, up or down to zoom");
            ui.add_enabled(self.touchpad, egui::Slider::new(&mut self.pad_tuning.touch_speed, 0.25..=3.0).custom_formatter(|v, _| format!("{v:.1}×")).text("pointer speed"));
            ui.checkbox(&mut self.light_bar, "Light bar shows HP");
            ui.horizontal(|ui| {
                ui.label("DualSense");
                ui.selectable_value(&mut self.dualsense_hd, true, "HD haptics").on_hover_text("Wired: vibration played on its haptic motors, as PS5 games do");
                ui.selectable_value(&mut self.dualsense_hd, false, "Classic rumble").on_hover_text("Rumble emulation, as over Bluetooth");
            });
        });
        ui.end_row();
        section(ui, "Shortcut bar");
        ui.label("Rows shown");
        egui::ComboBox::from_id_salt("hotkey rows")
            .selected_text(match self.hotkey_rows { 0 => "Hidden".to_string(), n => n.to_string() })
            .show_ui(ui, |ui| {
                for n in [1, 2, 3, 4, 0] {
                    ui.selectable_value(&mut self.hotkey_rows, n, if n == 0 { "Hidden".to_string() } else { n.to_string() });
                }
            })
            .response
            .on_hover_text("F12 cycles these too");
        ui.end_row();
        ui.label("Keys");
        if ui.button(crate::msgstr::get(1494, &[]).unwrap_or_else(|| "Shortcut Settings".into())).on_hover_text("Which key does what: the shortcut bar, windows, macros").clicked() {
            self.open_keys = true;
        }
        ui.end_row();
        section(ui, "Battle mode");
        ui.label("Enabled");
        ui.checkbox(&mut self.battle_mode, "On (/bm)").on_hover_text("Letter keys use the shortcut rows; Enter opens the chat");
        ui.end_row();
        ui.label("Chat");
        ui.checkbox(&mut self.battle_space_chat, "Space opens the chat");
        ui.end_row();
        self.battle_keys.resize(3, String::new());
        for (i, keys) in self.battle_keys.iter_mut().enumerate() {
            ui.label(format!("Row {} keys", i + 2)).on_hover_text("One key per slot, left to right (nine). Counted from the row on show.");
            ui.horizontal(|ui| {
                let mut text = keys.to_uppercase();
                if ui.add(egui::TextEdit::singleline(&mut text).char_limit(9).desired_width(110.0).font(egui::TextStyle::Monospace)).changed() {
                    *keys = text.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
                }
                if keys.chars().count() < 9 {
                    ui.label(egui::RichText::new(format!("{} of 9", keys.chars().count())).size(11.0).color(egui::Color32::from_rgb(220, 140, 60)));
                }
            });
            ui.end_row();
        }
        ui.label("");
        if ui.button("Default keys").clicked() {
            self.battle_keys = crate::windows::BATTLE_KEYS.iter().map(|k| k.to_string()).collect();
            self.battle_space_chat = true;
        }
        ui.end_row();
    }

    fn graphics_tab(&mut self, ui: &mut egui::Ui) {
        section(ui, "Performance");
        ui.label("Render resolution");
        // Applied on release, so the world isn't rebuilt at every step of a drag.
        ui.horizontal(|ui| {
            let mut scale = self.visual.render_scale;
            let slider = ui.add_enabled(!self.visual.render_auto, egui::Slider::new(&mut scale, 0.5..=2.0).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.visual.render_scale = scale;
            }
            ui.checkbox(&mut self.visual.render_auto, "Auto").on_hover_text("Chosen from the screen's size: about a million pixels of world");
        });
        ui.end_row();
        section(ui, "HD-2D");
        ui.label("Enabled");
        ui.checkbox(&mut self.visual.hd2d, "Lighting and effects");
        ui.end_row();
        let v = &mut self.visual;
        section(ui, "Camera and light");
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Bloom"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.bloom, 0.0..=1.0));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Auto exposure")).response.on_hover_text("Brightness adapts like eyes do: dungeons open up, bright snow tones down");
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.auto_exposure, 0.0..=1.0));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Surface relief")).response.on_hover_text("Stone, brick and roof tiles catch the light as if carved");
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.relief, 0.0..=12.0));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Depth of field"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.depth_of_field, 0.0..=1.0));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Shadows"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.shadows, 0.0..=1.0));
        ui.end_row();
        let rt = v.hd2d && RAY_TRACING.load(std::sync::atomic::Ordering::Relaxed);
        let unavailable = "Not available on this device: it needs a desktop GPU with ray tracing";
        ui.add_enabled_ui(rt, |ui| ui.label("Ray-traced shadows")).response.on_hover_text("Outdoors, buildings and trees cast shadows that follow the sun. Needs a GPU with ray tracing").on_disabled_hover_text(unavailable);
        ui.add_enabled(rt, egui::Checkbox::without_text(&mut v.ray_traced_shadows)).on_disabled_hover_text(unavailable);
        ui.end_row();
        ui.add_enabled_ui(rt, |ui| ui.label("Ray-traced reflections")).response.on_hover_text("Outdoors, water reflects buildings, ships and trees, even off screen. Needs a GPU with ray tracing").on_disabled_hover_text(unavailable);
        ui.add_enabled(rt, egui::Checkbox::without_text(&mut v.ray_traced_reflections)).on_disabled_hover_text(unavailable);
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Sun height"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.sun_elevation, 15.0..=85.0).suffix("°"));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Sun cycle")).response.on_hover_text("Minutes for the sun to cross the sky; 0 keeps it still");
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.sun_cycle, 0.0..=180.0).suffix(" min"));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Water"));
        ui.add_enabled_ui(v.hd2d, |ui| {
            egui::ComboBox::from_id_salt("water quality").selected_text(ro_render::WaterQuality::ALL[(v.water as usize).min(4)].name()).show_ui(ui, |ui| {
                for (i, q) in ro_render::WaterQuality::ALL.iter().enumerate() {
                    ui.selectable_value(&mut v.water, i as u8, q.name());
                }
            });
        });
        ui.end_row();
        ui.label("Weather").on_hover_text("The server's snow, petals, leaves, clouds and fog; ambient adds the client's own by climate: snow on snowy maps, petals by cherry trees, leaves in forests, fog in Niflheim, and now and then rain");
        egui::ComboBox::from_id_salt("weather").selected_text(v.weather.name()).show_ui(ui, |ui| {
            for m in crate::weather::WeatherMode::ALL {
                ui.selectable_value(&mut v.weather, m, m.name());
            }
        });
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Cloud shadows")).inner.on_hover_text("Shadows of clouds drifting over outdoor maps; cloudy weather brings more of them");
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.cloud_shadows, 0.0..=1.0));
        ui.end_row();
        ui.label("Swaying trees").on_hover_text("Trees, grass and flowers sway in the wind, harder in rain and storms");
        ui.add(egui::Slider::new(&mut v.foliage_sway, 0.0..=1.0));
        ui.end_row();
        section(ui, "Look");
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Vignette"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.vignette, 0.0..=1.0));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Warmth"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.warmth, -1.0..=1.0));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Saturation"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.saturation, 0.5..=1.5));
        ui.end_row();
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Contrast"));
        ui.add_enabled(v.hd2d, egui::Slider::new(&mut v.contrast, 0.7..=1.4));
        ui.end_row();
        for (label, value, max) in [
            ("Sprite lighting", &mut v.sprite_light, 1.0),
            ("Rim light", &mut v.rim_light, 1.0),
            ("Light sources", &mut v.point_lights, 2.0),
            ("Fog", &mut v.fog, 1.0),
            ("God rays", &mut v.god_rays, 1.0),
            ("Ambient occlusion", &mut v.ambient_occlusion, 1.0),
            ("Underwater", &mut v.underwater, 1.0),
        ] {
            ui.add_enabled_ui(v.hd2d, |ui| ui.label(label));
            ui.add_enabled(v.hd2d, egui::Slider::new(value, 0.0..=max));
            ui.end_row();
        }
        ui.add_enabled_ui(v.hd2d, |ui| ui.label("Pixel snapping"));
        ui.add_enabled(v.hd2d, egui::Checkbox::new(&mut v.pixel_snap, "Crisp sprites"));
        ui.end_row();
        if ui.add_enabled(v.hd2d, egui::Button::new("Reset look")).clicked() {
            *v = Visual { hd2d: true, ..Visual::default() };
        }
        ui.end_row();
        section(ui, "Textures");
        let u = &mut self.upscale;
        ui.label("Upscaling").on_hover_text("Ground, models, item icons and interface art (never character sprites). Load-time upscaling applies from the next map or window opened.");
        use ro_render::UpscaleQuality as Q;
        let names = |q: Q| match q {
            Q::Off => "Off",
            Q::Fast => "Fast (filtering only)",
            Q::Balanced => "Balanced (bicubic)",
            Q::High => "High (Lanczos)",
        };
        egui::ComboBox::from_id_salt("upscale quality").selected_text(names(u.quality)).show_ui(ui, |ui| {
            for q in [Q::Off, Q::Fast, Q::Balanced, Q::High] {
                ui.selectable_value(&mut u.quality, q, names(q));
            }
        });
        ui.end_row();
        let scaling = matches!(u.quality, Q::Balanced | Q::High);
        ui.add_enabled_ui(scaling, |ui| ui.label("Upscale factor"));
        ui.add_enabled_ui(scaling, |ui| {
            egui::ComboBox::from_id_salt("upscale factor").selected_text(format!("{}x", u.factor)).show_ui(ui, |ui| {
                for f in ro_render::UpscaleSettings::FACTORS {
                    ui.selectable_value(&mut u.factor, f, format!("{f}x"));
                }
            });
        });
        ui.end_row();
    }
}

/// A heading row in a settings grid, in the accent colour.
/// A stick's live position in its circle: the deadzone shaded, the full-tilt ring, and the dot
/// lit once the stick reads past the deadzone.
fn stick_preview(ui: &mut egui::Ui, stick: Option<glam::Vec2>, inner: f32, outer: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
    let p = ui.painter();
    let c = rect.center();
    let r = rect.width() / 2.0 - 2.0;
    let line = ui.visuals().widgets.noninteractive.fg_stroke.color;
    p.circle(c, r, ui.visuals().extreme_bg_color, egui::Stroke::new(1.0, line));
    p.circle_filled(c, r * inner, line.gamma_multiply(0.25));
    if outer > 0.0 {
        p.circle_stroke(c, r * (1.0 - outer), egui::Stroke::new(1.0, line.gamma_multiply(0.5)));
    }
    let Some(v) = stick else {
        p.text(c, egui::Align2::CENTER_CENTER, "—", egui::FontId::proportional(12.0), line);
        return;
    };
    let on = v.length() > inner;
    let dot = c + egui::vec2(v.x, -v.y).clamp(egui::vec2(-1.0, -1.0), egui::vec2(1.0, 1.0)) * r;
    p.circle_filled(dot, 3.5, if on { egui::Color32::from_rgb(90, 200, 120) } else { line });
}

fn section(ui: &mut egui::Ui, title: &str) {
    let color = if ui.visuals().dark_mode { egui::Color32::from_rgb(140, 175, 255) } else { egui::Color32::from_rgb(50, 70, 150) };
    ui.vertical(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(title).strong().size(13.5).color(color));
    });
    ui.end_row();
}
