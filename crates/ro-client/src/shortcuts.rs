//! The Shortcut Information window (the original's menu entry of that name, msgstringtable
//! 3880): every keyboard, mouse and gamepad shortcut and what it does. The keys Shortcut
//! Settings can change come from `keybinds`; the rest describe what main.rs does
//! (`key_pressed`, the mouse events and `gamepad_frame`), so a new shortcut there belongs
//! here too.

use crate::padglyph::{Btn, Style};
use egui::{Color32, RichText};

const CHAT: &[(&str, &str)] = &[
    ("Enter", "Type in the chat (with battle mode off, any letter starts typing too)"),
    ("Up / Down", "While typing: the lines you sent before"),
];

const MOUSE: &[(&str, &str)] = &[
    ("Click the ground", "Walk there; hold the button to keep walking toward the cursor"),
    ("Click a monster", "Attack it until it falls"),
    ("Click an NPC", "Talk to it"),
    ("Click a player", "Their menu (trade, party, friend); on PvP and GvG maps, attack"),
    ("Click an item", "Walk to it and pick it up"),
    ("Hover", "Names of monsters and items on the ground"),
    ("Right-drag", "Turn and tilt the camera"),
    ("Wheel", "Zoom"),
];

/// One piece of a controller row's left column: a button glyph or a few words.
enum Seg {
    B(Btn),
    T(&'static str),
}

use Seg::{B, T};

/// The buttons that fire shortcut slots once a trigger is held.
const SLOT_KEYS: [Seg; 6] = [B(Btn::South), B(Btn::East), B(Btn::West), B(Btn::North), B(Btn::DPad), B(Btn::RS)];

fn pad_section(ui: &mut egui::Ui, title: &str, rows: Vec<(Vec<Seg>, &str)>, style: Style) {
    section(ui, title);
    for (segs, what) in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for seg in segs {
                match seg {
                    B(b) => {
                        crate::padglyph::show(ui, b, style, 20.0);
                    }
                    T(t) => {
                        ui.label(RichText::new(t).size(12.0));
                    }
                }
            }
        });
        ui.label(what);
        ui.end_row();
    }
}

fn gamepad(ui: &mut egui::Ui, s: &Ctx<'_>) {
    let style = s.pad;
    ui.label(RichText::new(format!("{} buttons", style.name())).size(11.5).weak());
    ui.label(RichText::new("Settings > Controls changes them").size(11.5).weak());
    ui.end_row();
    pad_section(
        ui,
        "Moving and looking",
        vec![
            (vec![B(Btn::LStick)], "Walk"),
            (vec![B(Btn::RStick)], "Turn and tilt the camera"),
            (vec![B(Btn::RS)], "Reset the camera's turn"),
            (vec![B(Btn::Left), B(Btn::Right)], "Turn the camera 45 degrees"),
            (vec![B(Btn::Up), B(Btn::Down)], "Zoom"),
        ],
        style,
    );
    pad_section(
        ui,
        "Fighting",
        vec![
            (vec![B(Btn::LB), T("/"), B(Btn::RB)], "Previous or next target"),
            (vec![B(Btn::South)], "Attack the target, or talk to it"),
            (vec![B(Btn::East)], "Stop attacking"),
            (vec![B(Btn::West)], "Pick up the nearest item"),
            (vec![B(Btn::North)], "Sit down or stand up"),
        ],
        style,
    );
    let hold = |triggers: &[Btn]| -> Vec<Seg> {
        let mut v = vec![T("Hold")];
        for (i, t) in triggers.iter().enumerate() {
            if i > 0 {
                v.push(T("+"));
            }
            v.push(B(*t));
        }
        v.push(T("+"));
        v.extend(SLOT_KEYS.iter().map(|k| match k {
            B(b) => B(*b),
            T(t) => T(t),
        }));
        v
    };
    let mut bar = vec![(hold(&[Btn::RT]), "Use the slots of the shortcut row on show"), (hold(&[Btn::LT]), "Use the slots of the next row")];
    if s.battle_mode {
        bar.push((hold(&[Btn::LT, Btn::RT]), "Use the slots of the row after that"));
    } else {
        bar.push((vec![T("Aiming:"), B(Btn::South), T("/"), B(Btn::North), T("/"), B(Btn::East)], "Use the skill on the target / on yourself / cancel"));
    }
    pad_section(ui, &format!("Shortcut bar (battle mode {})", if s.battle_mode { "on" } else { "off" }), bar, style);
    let text = if s.battle_mode {
        "While a trigger is held, its row shows around the buttons. Skills fire at once: attacks on the target (or the nearest monster), support skills on the targeted player or yourself, ground skills under the target."
    } else {
        "While a trigger is held, its row shows around the buttons. Battle mode (/bm) fires skills at once, without aiming, and adds a third row on both triggers."
    };
    note(ui, text);
    if style == Style::PlayStation {
        pad_section(
            ui,
            "Touchpad",
            vec![
                (vec![B(Btn::Touchpad), T("slide")], "Move the pointer"),
                (vec![B(Btn::Touchpad), T("press")], "Click; with two fingers, right-click"),
                (vec![T("Two fingers across")], "Change the shortcut row on show"),
                (vec![T("Two fingers up / down")], "Zoom"),
            ],
            style,
        );
    }
    pad_section(
        ui,
        "Menus",
        vec![
            (vec![B(Btn::Start)], "Settings"),
            (vec![B(Btn::Select)], "Inventory"),
            (vec![T("NPC talk:"), B(Btn::Up), B(Btn::Down), T("/"), B(Btn::South), T("/"), B(Btn::East)], "Choose / confirm / cancel"),
            (vec![T("Dead:"), B(Btn::South)], "Return to your save point"),
        ],
        style,
    );
}

/// What the window needs to know about the current settings.
pub struct Ctx<'a> {
    pub battle_mode: bool,
    /// The keys of the three rows after the one on show (settings.battle_keys).
    pub battle_keys: &'a [String],
    pub battle_space_chat: bool,
    /// The controller button family to draw.
    pub pad: Style,
    /// Keys moved in Shortcut Settings.
    pub keys: &'a crate::keybinds::Keys,
}

#[derive(Default)]
pub struct ShortcutWindow {
    pub open: bool,
    pub tab: usize,
    /// Its Shortcut Settings button was pressed (the game opens that window).
    pub open_keys: bool,
}

impl ShortcutWindow {
    pub fn show(&mut self, ctx: &egui::Context, s: Ctx<'_>) {
        if !self.open {
            return;
        }
        let title = crate::msgstr::get(3880, &[]).unwrap_or_else(|| "Shortcut Information".into());
        let max_height = (ctx.content_rect().height() - 140.0).clamp(220.0, 620.0);
        let mut open = self.open;
        let tab = &mut self.tab;
        let open_keys = &mut self.open_keys;
        crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("shortcut information")).open(&mut open).resizable(false).collapsible(false).default_width(470.0).default_pos(egui::pos2(ctx.content_rect().center().x - 235.0, 70.0)), &title, true, |ui| {
            ui.horizontal(|ui| {
                for (i, name) in ["Keyboard", "Mouse", "Gamepad"].iter().enumerate() {
                    ui.selectable_value(tab, i, *name);
                }
                if *tab == 0 && ui.button(crate::msgstr::get(1494, &[]).unwrap_or_else(|| "Shortcut Settings".into())).on_hover_text("Change the keys").clicked() {
                    *open_keys = true;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
                ui.set_width(450.0);
                egui::Grid::new(("shortcuts", *tab)).num_columns(2).spacing([14.0, 6.0]).min_col_width(120.0).show(ui, |ui| match *tab {
                    0 => keyboard(ui, &s),
                    1 => rows(ui, MOUSE.iter().map(|(k, d)| (k.to_string(), d.to_string()))),
                    _ => gamepad(ui, &s),
                });
            });
        });
        self.open = open;
    }
}

fn keyboard(ui: &mut egui::Ui, s: &Ctx<'_>) {
    let list = |ui: &mut egui::Ui, l: &[(&str, &str)]| rows(ui, l.iter().map(|(k, d)| (k.to_string(), d.to_string())));
    let bound = |ui: &mut egui::Ui, l: Vec<(String, &str)>| rows(ui, l.into_iter().map(|(k, d)| (k, d.to_string())));
    section(ui, "Windows");
    bound(ui, crate::keybinds::rows(s.keys, true));
    section(ui, "Shortcut bar");
    bound(ui, crate::keybinds::bar_rows(s.keys));
    list(ui, &[("Drag the top edge", "Show more or fewer shortcut rows")]);
    section(ui, &format!("Battle mode (/bm): {}", if s.battle_mode { "on" } else { "off" }));
    let keys: Vec<(String, String)> = s
        .battle_keys
        .iter()
        .enumerate()
        .filter(|(_, k)| !k.is_empty())
        .map(|(i, k)| {
            let k = k.to_uppercase();
            let span = match (k.chars().next(), k.chars().last()) {
                (Some(a), Some(b)) if a != b => format!("{a} - {b}"),
                _ => k.clone(),
            };
            (span, format!("Use the slots of the {} row after the one on show", ["first", "second", "third"][i.min(2)]))
        })
        .collect();
    rows(ui, keys.into_iter());
    if s.battle_space_chat {
        rows(ui, std::iter::once(("Space".to_string(), "Type in the chat".to_string())));
    }
    note(ui, "With battle mode on, letters use skills instead of typing. Settings > Controls changes the keys.");
    section(ui, "Chat");
    list(ui, CHAT);
    bound(ui, crate::keybinds::macro_rows(s.keys));
    section(ui, "Game");
    list(ui, &[("Esc", "Game menu; cancels a skill being aimed")]);
    bound(ui, crate::keybinds::rows(s.keys, false));
    list(ui, &[("Print Screen", "Screenshot, saved in the ScreenShot folder")]);
}

/// A note in the description column, wrapped to the column.
fn note(ui: &mut egui::Ui, text: &str) {
    ui.label("");
    ui.scope(|ui| {
        ui.set_max_width(300.0);
        ui.add(egui::Label::new(RichText::new(text).size(11.0).weak()).wrap());
    });
    ui.end_row();
}

fn rows(ui: &mut egui::Ui, items: impl Iterator<Item = (String, String)>) {
    for (key, what) in items {
        key_chip(ui, &key);
        ui.label(what);
        ui.end_row();
    }
}

/// A key drawn as a small keycap.
fn key_chip(ui: &mut egui::Ui, key: &str) {
    let dark = ui.visuals().dark_mode;
    let (fill, stroke, text) = if dark {
        (Color32::from_rgb(44, 48, 60), Color32::from_rgb(90, 98, 120), Color32::from_rgb(230, 232, 240))
    } else {
        (Color32::from_rgb(246, 246, 250), Color32::from_rgb(170, 174, 190), Color32::from_rgb(30, 34, 48))
    };
    egui::Frame::new().fill(fill).stroke((1.0, stroke)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
        ui.label(RichText::new(key).monospace().size(12.0).color(text));
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    let color = if ui.visuals().dark_mode { Color32::from_rgb(140, 175, 255) } else { Color32::from_rgb(50, 70, 150) };
    ui.vertical(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(title).strong().size(13.5).color(color));
    });
    ui.end_row();
}
