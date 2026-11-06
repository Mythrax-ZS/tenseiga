//! Shortcut Settings (the original's key-binding window, msgstringtable 1494): which key does
//! what, in the original's tabs (Hotkey Bar, Interface, Macros). A binding is a key's name,
//! "Alt+E", "F1" or "Ctrl+Tab"; settings.toml keeps only the ones changed (`keys`, an empty
//! name for none). Esc, Enter, Print Screen and the battle mode rows (Settings > Controls)
//! stay where they are.

use std::collections::BTreeMap;

use egui::RichText;
use winit::keyboard::{Key, NamedKey};

/// What a key does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Does {
    /// A slot of the shortcut row on show (0-8).
    Slot(usize),
    /// Show one to four shortcut rows, or hide the bar.
    BarRows,
    /// Open or close a window, by its `alt_key` code.
    Window(&'static str),
    Sit,
    Minimap,
    Perf,
    Fullscreen,
    Screenshot,
    /// Photo mode on or off.
    Photo,
    /// Say a macro: 1-9, then 0 for the tenth (`emotes::macro_slot`).
    Macro(u8),
}

pub struct Action {
    pub id: &'static str,
    pub what: &'static str,
    pub default: &'static str,
    pub does: Does,
}

const fn a(id: &'static str, what: &'static str, default: &'static str, does: Does) -> Action {
    Action { id, what, default, does }
}

const BAR: &[Action] = &[
    a("slot1", "Slot 1 of the row on show", "F1", Does::Slot(0)),
    a("slot2", "Slot 2", "F2", Does::Slot(1)),
    a("slot3", "Slot 3", "F3", Does::Slot(2)),
    a("slot4", "Slot 4", "F4", Does::Slot(3)),
    a("slot5", "Slot 5", "F5", Does::Slot(4)),
    a("slot6", "Slot 6", "F6", Does::Slot(5)),
    a("slot7", "Slot 7", "F7", Does::Slot(6)),
    a("slot8", "Slot 8", "F8", Does::Slot(7)),
    a("slot9", "Slot 9", "F9", Does::Slot(8)),
    a("bar_rows", "Show one to four shortcut rows, or hide the bar", "F12", Does::BarRows),
];

const INTERFACE: &[Action] = &[
    a("inventory", "Inventory", "Alt+E", Does::Window("e")),
    a("equipment", "Equipment", "Alt+Q", Does::Window("q")),
    a("status", "Status", "Alt+A", Does::Window("a")),
    a("skills", "Skills", "Alt+S", Does::Window("s")),
    a("party", "Party", "Alt+Z", Does::Window("z")),
    a("friends", "Friends", "Alt+H", Does::Window("h")),
    a("guild", "Guild", "Alt+G", Does::Window("g")),
    a("quests", "Quests", "Alt+U", Does::Window("u")),
    a("cart", "Cart", "Alt+W", Does::Window("w")),
    a("companions", "Companions (pet, homunculus, mercenary)", "Alt+R", Does::Window("r")),
    a("mail", "Mail", "Alt+X", Does::Window("x")),
    a("bank", "Bank", "Alt+B", Does::Window("b")),
    a("cash_shop", "Cash Shop", "Alt+C", Does::Window("c")),
    a("achievements", "Achievements", "Alt+Y", Does::Window("y")),
    a("attendance", "Attendance", "Alt+J", Does::Window("j")),
    a("booking", "Party Booking", "Alt+N", Does::Window("n")),
    a("chat_room", "Create a chat room", "Alt+K", Does::Window("k")),
    a("emotes", "Emotion List", "Alt+L", Does::Window("l")),
    a("macros", "Macros (what the macro keys say)", "Alt+M", Does::Window("m")),
    a("settings", "Settings", "Alt+O", Does::Window("o")),
    a("sit", "Sit down or stand up", "Insert", Does::Sit),
    a("minimap", "Minimap on or off", "Ctrl+Tab", Does::Minimap),
    a("perf", "Performance overlay on or off (frame rate, frame times, ping)", "F10", Does::Perf),
    a("fullscreen", "Full screen on or off", "F11", Does::Fullscreen),
    a("screenshot", "Screenshot (Print Screen always takes one too)", "ScrollLock", Does::Screenshot),
    a("photo", "Photo mode (the world holds still, the camera goes free)", "Ctrl+P", Does::Photo),
];

const MACROS: &[Action] = &[
    a("macro1", "Macro 1", "Alt+1", Does::Macro(1)),
    a("macro2", "Macro 2", "Alt+2", Does::Macro(2)),
    a("macro3", "Macro 3", "Alt+3", Does::Macro(3)),
    a("macro4", "Macro 4", "Alt+4", Does::Macro(4)),
    a("macro5", "Macro 5", "Alt+5", Does::Macro(5)),
    a("macro6", "Macro 6", "Alt+6", Does::Macro(6)),
    a("macro7", "Macro 7", "Alt+7", Does::Macro(7)),
    a("macro8", "Macro 8", "Alt+8", Does::Macro(8)),
    a("macro9", "Macro 9", "Alt+9", Does::Macro(9)),
    a("macro10", "Macro 10", "Alt+0", Does::Macro(0)),
];

/// The window's tabs: msgstringtable name, fallback, actions.
const TABS: [(usize, &str, &[Action]); 3] = [(1491, "Hotkey Bar", BAR), (1492, "Interface", INTERFACE), (1493, "Macros", MACROS)];

pub type Keys = BTreeMap<String, String>;

fn all() -> impl Iterator<Item = &'static Action> {
    TABS.iter().flat_map(|t| t.2.iter())
}

/// The key an action is on now ("" for none).
pub fn binding<'a>(keys: &'a Keys, a: &'a Action) -> &'a str {
    keys.get(a.id).map_or(a.default, String::as_str)
}

/// The key on a shortcut slot of the row on show, as the bar labels it.
pub fn slot_key(keys: &Keys, col: usize) -> String {
    BAR.iter().find(|a| a.does == Does::Slot(col)).map(|a| pretty(binding(keys, a))).unwrap_or_default()
}

/// The actions in a window group ("Windows", "Game"), with their keys, for Shortcut Information.
pub fn rows(keys: &Keys, windows: bool) -> Vec<(String, &'static str)> {
    INTERFACE.iter().filter(|a| matches!(a.does, Does::Window(_)) == windows).map(|a| (pretty(binding(keys, a)), a.what)).filter(|(k, _)| !k.is_empty()).collect()
}

/// A run of keys for Shortcut Information: one row ("F1 - F9") while they are the defaults,
/// else a row per key.
fn span(keys: &Keys, actions: &'static [Action], span: &str, what: &'static str) -> Vec<(String, &'static str)> {
    if actions.iter().all(|a| binding(keys, a) == a.default) {
        return vec![(span.into(), what)];
    }
    actions.iter().map(|a| (pretty(binding(keys, a)), a.what)).filter(|(k, _)| !k.is_empty()).collect()
}

/// The shortcut bar's keys, for Shortcut Information.
pub fn bar_rows(keys: &Keys) -> Vec<(String, &'static str)> {
    let mut out = span(keys, &BAR[..9], "F1 - F9", "Use the slots of the shortcut row on show");
    out.extend(span(keys, &BAR[9..], "F12", BAR[9].what));
    out
}

/// The macro keys, for Shortcut Information.
pub fn macro_rows(keys: &Keys) -> Vec<(String, &'static str)> {
    span(keys, MACROS, "Alt+1 - Alt+0", "Say a macro (emotes until changed in the Macros window)")
}

/// What a key press does, if anything is bound to it.
pub fn lookup(keys: &Keys, name: &str) -> Option<Does> {
    all().find(|a| binding(keys, a).eq_ignore_ascii_case(name)).map(|a| a.does)
}

/// A key press's name ("Ctrl+Alt+K", "Shift+F3", "E"); None for a modifier on its own.
/// Shift only counts with named keys: with a character it changes the character instead.
pub fn name(key: &Key, ctrl: bool, alt: bool, shift: bool) -> Option<String> {
    let (base, named) = match key {
        Key::Named(NamedKey::Control | NamedKey::Alt | NamedKey::Shift | NamedKey::Super | NamedKey::AltGraph | NamedKey::Meta | NamedKey::Hyper | NamedKey::Fn) => return None,
        Key::Named(n) => (format!("{n:?}"), true),
        Key::Character(c) => (c.to_uppercase(), false),
        _ => return None,
    };
    let mut s = String::new();
    for (on, m) in [(ctrl, "Ctrl+"), (alt, "Alt+"), (shift && named, "Shift+")] {
        if on {
            s.push_str(m);
        }
    }
    s.push_str(&base);
    Some(s)
}

/// A name for display: "ScrollLock" as "Scroll Lock".
pub fn pretty(name: &str) -> String {
    let mut out = String::new();
    let mut prev = ' ';
    for c in name.chars() {
        if c.is_ascii_uppercase() && prev.is_ascii_lowercase() {
            out.push(' ');
        }
        out.push(c);
        prev = c;
    }
    out
}

enum Ask {
    /// The key is another action's: take it from that one?
    Steal { id: &'static str, key: String, from: &'static Action },
    Reset,
}

#[derive(Default)]
pub struct KeyWindow {
    pub open: bool,
    tab: usize,
    /// The action waiting for a key press (the game hands it the next press).
    pub capturing: Option<&'static str>,
    ask: Option<Ask>,
    note: Option<String>,
}

fn msg(id: usize, args: &[&str], en: &str) -> String {
    crate::msgstr::get(id, args).unwrap_or_else(|| en.to_string())
}

fn set(keys: &mut Keys, a: &Action, key: &str) {
    if key == a.default {
        keys.remove(a.id);
    } else {
        keys.insert(a.id.to_string(), key.to_string());
    }
}

impl KeyWindow {
    /// The key pressed while waiting for one: Esc (None) removes the binding, as in the original.
    pub fn captured(&mut self, keys: &mut Keys, key: Option<String>) {
        let Some(id) = self.capturing else { return };
        let Some(action) = all().find(|a| a.id == id) else { return };
        let Some(key) = key else {
            set(keys, action, "");
            self.capturing = None;
            return;
        };
        // Typing goes to the chat, so a letter or digit needs Ctrl or Alt.
        if !key.contains('+') && key.chars().count() == 1 {
            self.note = Some(msg(1487, &[], "Unable to specify a single key."));
            return;
        }
        if matches!(key.as_str(), "Enter" | "Space" | "Escape" | "PrintScreen") {
            self.note = Some(msg(1488, &[], "Unable to specify the key assigned."));
            return;
        }
        self.capturing = None;
        self.note = None;
        match all().find(|a| a.id != id && binding(keys, a).eq_ignore_ascii_case(&key)) {
            Some(from) => self.ask = Some(Ask::Steal { id, key, from }),
            None => set(keys, action, &key),
        }
    }

    /// Returns true when a binding changed (to be saved).
    pub fn show(&mut self, ctx: &egui::Context, keys: &mut Keys) -> bool {
        if !self.open {
            self.capturing = None;
            return false;
        }
        let before = keys.clone();
        let title = msg(1494, &[], "Shortcut Settings");
        let mut open = self.open;
        crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("shortcut settings")).open(&mut open).resizable(false).collapsible(false).default_width(430.0).default_pos(egui::pos2(ctx.content_rect().center().x - 215.0, 90.0)), &title, true, |ui| {
            ui.horizontal(|ui| {
                for (i, (id, en, _)) in TABS.iter().enumerate() {
                    ui.selectable_value(&mut self.tab, i, msg(*id, &[], en));
                }
            });
            ui.separator();
            let max_height = (ui.ctx().content_rect().height() - 220.0).clamp(200.0, 480.0);
            egui::ScrollArea::vertical().max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new(("key bindings", self.tab)).num_columns(2).spacing([14.0, 5.0]).show(ui, |ui| {
                    for a in TABS[self.tab].2 {
                        ui.label(a.what);
                        let waiting = self.capturing == Some(a.id);
                        let text = if waiting { "...".to_string() } else { pretty(binding(keys, a)) };
                        let button = ui.add(egui::Button::new(RichText::new(text).monospace()).min_size(egui::vec2(110.0, 0.0)).selected(waiting));
                        if button.clicked() {
                            self.capturing = if waiting { None } else { Some(a.id) };
                            self.note = None;
                        }
                        ui.end_row();
                    }
                });
            });
            ui.separator();
            // Yes / No under a question; None until one is clicked.
            let answer = |ui: &mut egui::Ui, question: String| {
                ui.label(question);
                ui.horizontal(|ui| if ui.button("Yes").clicked() { Some(true) } else if ui.button("No").clicked() { Some(false) } else { None }).inner
            };
            match self.ask.take() {
                Some(Ask::Steal { id, key, from }) => match answer(ui, msg(1489, &[from.what], &format!("This key is already registered to [{}]. Bind it to this function instead?", from.what)).replace("[[", "[").replace("]]", "]")) {
                    Some(true) => {
                        set(keys, from, "");
                        if let Some(a) = all().find(|a| a.id == id) {
                            set(keys, a, &key);
                        }
                    }
                    Some(false) => {}
                    None => self.ask = Some(Ask::Steal { id, key, from }),
                },
                Some(Ask::Reset) => match answer(ui, msg(1490, &[], "Stored shortcut key combination will be initialized. Do you want to continue?")) {
                    Some(true) => keys.clear(),
                    Some(false) => {}
                    None => self.ask = Some(Ask::Reset),
                },
                None => {
                    let hint = self.note.clone().unwrap_or_else(|| if self.capturing.is_some() { msg(1486, &[], "Press a key to assign. Pressing 'ESC' will remove the assigned key.") } else { "Click a key to change it.".into() });
                    ui.label(RichText::new(hint).size(12.0).weak());
                    if ui.button(msg(3994, &[], "Default")).clicked() {
                        self.capturing = None;
                        self.ask = Some(Ask::Reset);
                    }
                }
            }
        });
        self.open = open;
        *keys != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_lookups() {
        let f1 = Key::Named(NamedKey::F1);
        assert_eq!(name(&f1, false, false, true).as_deref(), Some("Shift+F1"));
        assert_eq!(name(&Key::Character("e".into()), false, true, true).as_deref(), Some("Alt+E"));
        assert_eq!(name(&Key::Named(NamedKey::Alt), false, true, false), None);
        assert_eq!(pretty("Ctrl+ScrollLock"), "Ctrl+Scroll Lock");
        let mut keys = Keys::new();
        assert_eq!(lookup(&keys, "alt+e"), Some(Does::Window("e")));
        // Moving the inventory to Ctrl+I frees Alt+E.
        set(&mut keys, &INTERFACE[0], "Ctrl+I");
        assert_eq!(lookup(&keys, "Alt+E"), None);
        assert_eq!(lookup(&keys, "Ctrl+I"), Some(Does::Window("e")));
        set(&mut keys, &INTERFACE[0], "Alt+E");
        assert!(keys.is_empty(), "a default binding is not stored");
    }

    #[test]
    fn capturing_refuses_single_keys_and_asks_before_taking_a_key() {
        let mut w = KeyWindow { capturing: Some("inventory"), ..Default::default() };
        let mut keys = Keys::new();
        w.captured(&mut keys, Some("I".into()));
        assert!(w.capturing.is_some() && keys.is_empty());
        w.captured(&mut keys, Some("Alt+Q".into()));
        assert!(matches!(w.ask, Some(Ask::Steal { from, .. }) if from.id == "equipment"));
        w.capturing = Some("inventory");
        w.captured(&mut keys, None);
        assert_eq!(keys.get("inventory").map(String::as_str), Some(""));
        assert_eq!(lookup(&keys, "Alt+E"), None);
    }

    #[test]
    fn ids_are_unique() {
        let mut ids: Vec<_> = all().map(|a| a.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), all().count());
    }
}
