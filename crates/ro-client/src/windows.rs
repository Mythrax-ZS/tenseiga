//! Game windows: inventory, equipment, character stats and skills. They read the player's
//! state and return the actions the player chose; main.rs turns those into requests.

use crate::uitex::UiSize;
use std::collections::HashMap;

use egui::{Color32, RichText, Vec2};
use ro_game::items::{InvItem, ItemDb};
use ro_net::{Hotkey, Skill, Stats};

/// What the player asked for through a window.
#[derive(Debug, Clone, PartialEq)]
pub enum UiAction {
    /// Ask to expand the inventory (opens the server's confirmation).
    ExpandInventory,
    /// Answer the expansion confirmation.
    ExpandConfirm(bool),
    /// Cast a ground skill with its message.
    SkillText { skill: u16, level: u16, cell: (u16, u16), text: String },
    /// Yes to saving this map as a Star Gladiator place.
    StarPlace(u8),
    /// Answer an adoption request.
    Adopt(ro_net::AdoptRequest),
    UseItem(u16),
    Equip { index: u16, location: u32 },
    Unequip(u16),
    /// Take off everything in these slots.
    UnequipAll(u32),
    /// Move an item into (true) or out of the favourites ("etc") tab.
    Favorite { index: u16, on: bool },
    /// Equipment switch set: add an item, remove one, swap the whole set.
    SwitchAdd { index: u16, location: u32 },
    SwitchRemove(u16),
    SwitchAll,
    Drop { index: u16, amount: u16 },
    /// Identify an unidentified item with a carried Magnifier (one-click identify).
    Identify(u16),
    /// Raise a base stat; 0..6 = str, agi, vit, int, dex, luk.
    RaiseStat(usize),
    /// Raise trait stat `i` (pow, sta, wis, spl, con, crt) by one.
    RaiseTrait(usize),
    /// Ask which equipment can take this card (inventory index).
    CardTargets(u16),
    InsertCard { card: u16, equip: u16 },
    /// Answer the open skill/item menu.
    Menu(ro_net::MenuReply),
    /// Move an inventory item into the cart.
    CartPut { index: u16, amount: u16 },
    OpenBank,
    ShowEquip(bool),
    /// Show the item information window (cards: their illustration).
    ItemInfo { id: u32, refine: u8, cards: [u32; 4] },
    SkillUp(u16),
    UseSkill(u16),
    /// Activate shortcut slot n (row * HOTKEYS_PER_ROW + column).
    UseHotkey(usize),
    SetHotkey { index: u16, key: Hotkey },
    HotkeyRow(u8),
    /// Show this many shortcut rows (the bar's top edge dragged).
    HotkeyRows(u8),
}

/// The shortcut bar: rows of F1..F9 slots, saved on the server (38 slots per tab).
pub const HOTKEY_ROWS: u8 = 4;
pub const HOTKEYS_PER_ROW: usize = 9;
/// The default battle mode keys for the second, third and fourth shortcut rows (counted from
/// the one on show), as the original: Q ~ O, A ~ L and Z ~ >. Settings can change them.
pub const BATTLE_KEYS: [&str; 3] = ["qwertyuio", "asdfghjkl", "zxcvbnm,."];

/// An inventory item being dragged: onto the shortcut bar (usable items and equipment), into
/// storage, or out onto the world to drop it.
#[derive(Clone, Copy)]
pub struct ItemDrag {
    pub index: u16,
    pub id: u32,
    pub amount: u16,
    /// Usable or equipment: it can go on the shortcut bar.
    pub shortcut: bool,
    /// Not worn: it can be dropped or stored.
    pub loose: bool,
}

impl ItemDrag {
    fn hotkey(&self) -> Option<Hotkey> {
        self.shortcut.then_some(Hotkey { skill: false, id: self.id, count: 0 })
    }
}

#[derive(Default)]
pub struct Windows {
    /// An inventory stack dragged onto the world, waiting for "how many": (index, amount, typed).
    drop_asking: Option<(u16, u16, String)>,
    /// The cash shop is open (it lives in `stores`), and its menu button was clicked.
    pub cash_shown: bool,
    pub cash_toggle: bool,
    /// The top-right menu bar is folded down to its toggle; `menu_toggled` asks the app to save it.
    pub menu_collapsed: bool,
    pub menu_toggled: bool,
    /// A menu-bar button standing for an Alt+key shortcut, handled by the app.
    pub menu_key: Option<&'static str>,
    pub inventory: bool,
    /// Slots added to the base 100 by inventory expansion.
    pub inv_extra: u16,
    /// The server asks to confirm using this item to expand the inventory.
    pub expand_ask: Option<u32>,
    /// A ground skill waiting for its message (Graffiti, Talkie Box): skill, level, cell, text.
    pub skill_text: Option<(u16, u16, (u16, u16), String)>,
    /// Feeling asks to save this map as the place of the sun (0), moon (1) or stars (2).
    pub star_ask: Option<u8>,
    /// A couple asks us to be their child: (father, mother, asker's name).
    pub adopt_ask: Option<(u32, u32, String)>,
    /// Amounts typed in the Change Material / Analysis window, by inventory index.
    pub materials: std::collections::HashMap<u16, u16>,
    pub equipment: bool,
    /// Which equipment tab is showing (see EQUIP_TABS).
    pub equip_tab: u8,
    pub stats: bool,
    pub skills: bool,
    /// Show skills as a plain list instead of the tree.
    pub skill_list: bool,
    /// The skill tree tab shown; None opens on the current job's tab.
    pub skill_tab: Option<usize>,
    /// A card being inserted and the equipment the server offered for it.
    pub card_pick: Option<(u16, Vec<u16>)>,
    /// A pick-one menu from a skill or item (poisons, warp points, eggs...).
    pub menu: Option<ro_net::Menu>,
    /// How long a stat "+" has been held, for repeating.
    stat_hold: f32,
    pub party: bool,
    pub friends: bool,
    pub guild: bool,
    pub settings: bool,
    tab: InvTab,
    /// Classic inventory: rows scrolled.
    inv_scroll: usize,
}

#[derive(Default, PartialEq, Clone, Copy)]
enum InvTab {
    #[default]
    Items,
    Equip,
    Etc,
    /// Our own fourth tab: items marked as favourites (PlaceETCTab).
    Fav,
}

/// Everything the windows draw.
pub struct View<'a> {
    /// The original art, when the Classic interface is on.
    pub skin: Option<&'a crate::classic::Skin>,
    /// Our guild's name (empty without one).
    pub guild_name: Option<&'a str>,
    pub items: &'a [&'a InvItem],
    pub db: Option<&'a ItemDb>,
    pub icons: &'a HashMap<u32, egui::TextureHandle>,
    /// Unidentified items' icons.
    pub unid_icons: &'a HashMap<u32, egui::TextureHandle>,
    pub skill_icons: &'a HashMap<u16, egui::TextureHandle>,
    pub stats: Option<&'a Stats>,
    /// A 4th job: the Status window shows trait stats.
    pub fourth_job: bool,
    pub skills: &'a [Skill],
    pub skill_names: &'a dyn Fn(&str) -> String,
    pub skill_info: Option<&'a ro_game::skills::SkillNames>,
    pub skill_tree: Option<&'a crate::skilltree::SkillTree>,
    /// The player's job id, choosing the skill tree tabs.
    pub job: i16,
    pub skill_points: i64,
    pub weight: (i64, i64),
    pub zeny: i64,
    /// Screen width in UI points, for the default window layout.
    pub screen_width: f32,
    pub hotkeys: &'a [Hotkey],
    pub hotkey_row: u8,
    /// Shortcut bar rows on show (0: hidden), stacked up from `hotkey_row`.
    pub hotkey_rows: u8,
    /// Battle mode (/bm): the rows above the first answer to letter keys.
    pub battle_mode: bool,
    /// Those keys, a string of slot keys per row above the first.
    pub battle_keys: &'a [String],
    /// Keys moved in Shortcut Settings (the first row's slot keys).
    pub keys: &'a crate::keybinds::Keys,
    /// Skills cooling down: id -> when they are ready again.
    pub cooldowns: &'a HashMap<u16, std::time::Instant>,
    /// We have a pushcart (inventory items can go into it).
    pub has_cart: bool,
    /// Our "others may view my equipment" setting, once the server has said.
    pub show_equip: Option<bool>,
    /// Our character as the select screen draws it, for the equipment window.
    pub portrait: Option<&'a egui::TextureHandle>,
    /// A controller is in use: label slots with its buttons, drawn in this family's style.
    pub pad: Option<crate::padglyph::Style>,
    /// The shortcut row a held trigger points at (0 the row on show, 1 the next, 2 the one
    /// after), for the controller's cross bar.
    pub pad_row: Option<u8>,
}

/// Draws a skill or item icon centred in `rect`.
fn paint_icon(painter: &egui::Painter, rect: egui::Rect, tex: Option<&egui::TextureHandle>, tint: Color32) {
    if let Some(tex) = tex {
        let s = tex.ui_size().min(rect.size() - Vec2::splat(6.0));
        painter.image(tex.id(), egui::Rect::from_center_size(rect.center(), s), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
    }
}

fn outlined(painter: &egui::Painter, pos: egui::Pos2, align: egui::Align2, text: &str, size: f32, color: Color32) {
    let font = egui::FontId::proportional(size);
    painter.text(pos + Vec2::new(1.0, 1.0), align, text, font.clone(), Color32::BLACK);
    painter.text(pos, align, text, font, color);
}

/// The triggers a controller holds for a shortcut row (by its offset from the row on show):
/// RT, LT, and in battle mode both.
pub fn pad_row_triggers(offset: u8, battle_mode: bool) -> Option<&'static [crate::padglyph::Btn]> {
    use crate::padglyph::Btn;
    match offset {
        0 => Some(&[Btn::RT]),
        1 => Some(&[Btn::LT]),
        2 if battle_mode => Some(&[Btn::LT, Btn::RT]),
        _ => None,
    }
}

/// The controller glyph on a shortcut slot, when its row answers to a trigger.
fn slot_glyph(v: &View, offset: u8, col: usize) -> Option<(crate::padglyph::Btn, crate::padglyph::Style)> {
    let style = v.pad?;
    pad_row_triggers(offset, v.battle_mode)?;
    Some((crate::padglyph::SLOT_BUTTONS[col], style))
}

/// The key shown on a shortcut slot: F1..F9 (or as Shortcut Settings moved them) on the first row, the battle mode letters on
/// the rows above it, or the controller buttons ("RT + A").
fn slot_key_label(v: &View, offset: u8, col: usize) -> String {
    if let (Some(style), Some(t)) = (v.pad, pad_row_triggers(offset, v.battle_mode)) {
        let mut bs = t.to_vec();
        bs.push(crate::padglyph::SLOT_BUTTONS[col]);
        return crate::padglyph::combo_name(&bs, style);
    }
    match offset {
        0 => crate::keybinds::slot_key(v.keys, col),
        n if v.battle_mode => v.battle_keys.get(n as usize - 1).and_then(|k| k.chars().nth(col)).map_or(String::new(), |c| c.to_uppercase().to_string()),
        _ => String::new(),
    }
}

/// One shortcut slot: draws it and returns what the player did with it.
fn hotkey_slot_ui(ui: &mut egui::Ui, v: &View, index: usize, key_label: &str, glyph: Option<(crate::padglyph::Btn, crate::padglyph::Style)>, size: f32) -> Option<UiAction> {
    let mut action = None;
    let key = v.hotkeys.get(index).copied().unwrap_or_default();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let hovering_drop = resp.dnd_hover_payload::<Hotkey>().is_some() || resp.dnd_hover_payload::<ItemDrag>().is_some_and(|d| d.shortcut);
    let bg = if hovering_drop { Color32::from_rgba_unmultiplied(255, 220, 120, 60) } else if resp.hovered() { Color32::from_white_alpha(30) } else { Color32::from_white_alpha(12) };
    if let Some(t) = v.skin.and_then(|s| s.texture("itemwin_mid")) {
        painter.image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        if hovering_drop || resp.hovered() {
            painter.rect_filled(rect.shrink(1.0), 3.0, Color32::from_rgba_unmultiplied(120, 150, 230, 50));
        }
    } else {
        painter.rect_filled(rect, 4.0, bg);
    }
    let skill = key.skill.then(|| v.skills.iter().find(|s| s.id as u32 == key.id)).flatten();
    if key.id != 0 {
        if key.skill {
            paint_icon(&painter, rect, v.skill_icons.get(&(key.id as u16)), Color32::WHITE);
            if skill.is_some_and(|s| s.level > 1) {
                outlined(&painter, rect.right_bottom() - Vec2::new(3.0, 1.0), egui::Align2::RIGHT_BOTTOM, &format!("{}", key.count), 11.0, Color32::from_rgb(255, 220, 120));
            }
            // Cooldown: a dark sweep with the seconds left, as in the original.
            if let Some(left) = v.cooldowns.get(&(key.id as u16)).map(|e| e.saturating_duration_since(std::time::Instant::now()).as_secs_f32()).filter(|s| *s > 0.0) {
                painter.rect_filled(rect.shrink(2.0), 4.0, Color32::from_black_alpha(150));
                let text = if left >= 60.0 { format!("{}m", (left / 60.0).ceil()) } else { format!("{left:.1}") };
                outlined(&painter, rect.center(), egui::Align2::CENTER_CENTER, &text, 12.0, Color32::WHITE);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        } else {
            let count: u32 = v.items.iter().filter(|it| it.id == key.id && it.equipped == 0).map(|it| it.amount as u32).sum();
            let tint = if count == 0 { Color32::from_white_alpha(70) } else { Color32::WHITE };
            paint_icon(&painter, rect, v.icons.get(&key.id), tint);
            outlined(&painter, rect.right_bottom() - Vec2::new(3.0, 1.0), egui::Align2::RIGHT_BOTTOM, &count.to_string(), 11.0, Color32::WHITE);
        }
    }
    if let Some((b, style)) = glyph {
        crate::padglyph::paint_at(&painter, rect.left_top() + Vec2::new(2.0, 2.0), b, style, (size * 0.36).round());
    } else if v.skin.is_some() {
        painter.text(rect.left_top() + Vec2::new(3.0, 1.0), egui::Align2::LEFT_TOP, key_label, egui::FontId::proportional(9.5), Color32::from_rgb(90, 100, 140));
    } else {
        outlined(&painter, rect.left_top() + Vec2::new(3.0, 1.0), egui::Align2::LEFT_TOP, key_label, 10.0, Color32::from_gray(200));
    }

    if let Some(dropped) = resp.dnd_release_payload::<Hotkey>().map(|k| *k).or_else(|| resp.dnd_release_payload::<ItemDrag>().and_then(|d| d.hotkey())) {
        action = Some(UiAction::SetHotkey { index: index as u16, key: dropped });
    } else if key.id != 0 {
        resp.dnd_set_drag_payload(key);
        if resp.clicked() {
            action = Some(UiAction::UseHotkey(index));
        } else if resp.secondary_clicked() {
            action = Some(UiAction::SetHotkey { index: index as u16, key: Hotkey::default() });
        } else if let (true, Some(s)) = (resp.hovered(), skill) {
            let scroll: f32 = ui.input(|i| i.events.iter().map(|e| if let egui::Event::MouseWheel { delta, .. } = e { delta.y } else { 0.0 }).sum());
            if scroll != 0.0 {
                let count = (key.count + scroll.signum() as i16).clamp(1, s.level.max(1) as i16);
                if count != key.count {
                    action = Some(UiAction::SetHotkey { index: index as u16, key: Hotkey { count, ..key } });
                }
            }
        }
        let name = match skill {
            Some(s) => format!("{} Lv {}", (v.skill_names)(&s.name), key.count),
            None if key.skill => format!("Skill {}", key.id),
            None => v.db.map_or_else(|| format!("Item {}", key.id), |db| db.name(key.id, true)),
        };
        resp.on_hover_text(if key_label.is_empty() { name } else { format!("[ {key_label} ] {name}") });
    }
    action
}

/// The controller's cross bar: while a trigger is held, the row it reaches laid out the way
/// the buttons sit, the D-pad's four on the left, the face buttons' four on the right and the
/// right stick's click between them.
fn cross_bar(ctx: &egui::Context, v: &View) -> Option<UiAction> {
    use crate::padglyph::{Btn, SLOT_BUTTONS};
    let (style, offset) = (v.pad?, v.pad_row?);
    let triggers = pad_row_triggers(offset, v.battle_mode)?;
    const SIZE: f32 = 40.0;
    const GAP: f32 = 4.0;
    let row = (v.hotkey_row + offset) % HOTKEY_ROWS;
    let rows_shown = v.hotkey_rows.min(HOTKEY_ROWS) as f32;
    let diamond = 3.0 * SIZE + 2.0 * GAP;
    let width = diamond * 2.0 + SIZE + 32.0 + 20.0;
    // Right of the shortcut bar when the screen has room there, else above it; never over
    // the middle of the screen, where the character stands.
    let screen = ctx.content_rect();
    let bar: Option<egui::Rect> = ctx.data(|d| d.get_temp(egui::Id::unique("hotkey bar rect")));
    let (anchor, offset_px) = match bar {
        Some(bar) if screen.right() - bar.right() >= width + 32.0 => (egui::Align2::RIGHT_BOTTOM, Vec2::new(-16.0, -12.0)),
        Some(bar) => (egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -(screen.bottom() - bar.top() + 8.0))),
        None => (egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -(12.0 + 24.0 + rows_shown * 44.0))),
    };
    let mut action = None;
    egui::Area::new(egui::Id::unique("cross bar"))
        .anchor(anchor, offset_px)
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(14, 16, 24, 225)).stroke((1.0, Color32::from_rgb(70, 76, 96))).corner_radius(10).inner_margin(10).show(ui, |ui| {
                ui.style_mut().visuals = egui::Visuals::dark();
                ui.horizontal(|ui| {
                    crate::padglyph::combo(ui, triggers, style, 18.0);
                    ui.label(RichText::new(format!("Row {}", row + 1)).size(12.5).color(Color32::from_gray(200)));
                    if v.battle_mode {
                        ui.label(RichText::new("instant cast").size(11.0).color(Color32::from_rgb(230, 140, 100)));
                    }
                });
                ui.add_space(4.0);
                let (canvas, _) = ui.allocate_exact_size(Vec2::new(diamond * 2.0 + SIZE + 32.0, diamond), egui::Sense::hover());
                let left = canvas.left_center() + Vec2::new(diamond / 2.0, 0.0);
                let right = canvas.right_center() - Vec2::new(diamond / 2.0, 0.0);
                let step = SIZE + GAP;
                for (col, b) in SLOT_BUTTONS.iter().enumerate() {
                    let at = match b {
                        Btn::Up => left + Vec2::new(0.0, -step),
                        Btn::Right => left + Vec2::new(step, 0.0),
                        Btn::Down => left + Vec2::new(0.0, step),
                        Btn::Left => left + Vec2::new(-step, 0.0),
                        Btn::North => right + Vec2::new(0.0, -step),
                        Btn::East => right + Vec2::new(step, 0.0),
                        Btn::South => right + Vec2::new(0.0, step),
                        Btn::West => right + Vec2::new(-step, 0.0),
                        _ => canvas.center(),
                    };
                    let rect = egui::Rect::from_center_size(at, Vec2::splat(SIZE));
                    let mut bs = triggers.to_vec();
                    bs.push(*b);
                    let label = crate::padglyph::combo_name(&bs, style);
                    let index = row as usize * HOTKEYS_PER_ROW + col;
                    if let Some(a) = ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| hotkey_slot_ui(ui, v, index, &label, Some((*b, style)), SIZE)).inner {
                        action = Some(a);
                    }
                }
            });
        });
    action
}

/// The shortcut bar along the bottom: drag skills and items in, click or press F1..F9 to
/// use, right-click to clear, scroll over a skill to change its level. F12 shows one to four
/// rows or hides the bar, as in the original; the page button turns which row is first. In
/// battle mode the rows above the first answer to Q ~ O, A ~ L and Z ~ >. Dragging the bar's
/// top edge shows more or fewer rows, as the original's shortcut window resizes.
fn hotkey_bar(ctx: &egui::Context, v: &View) -> Option<UiAction> {
    const SIZE: f32 = 40.0;
    let mut action = None;
    // Gap under the bar.
    const BAR_BOTTOM: f32 = 12.0;
    // A bar anchored by its bottom is placed from last frame's size; when the row count
    // changes, lay the frame out again so it is never drawn a frame out of place.
    let shown = egui::Id::unique("hotkey rows shown");
    if ctx.data(|d| d.get_temp::<u8>(shown)) != Some(v.hotkey_rows) {
        ctx.data_mut(|d| d.insert_temp(shown, v.hotkey_rows));
        ctx.request_discard("shortcut bar rows changed");
    }
    if v.hotkey_rows > 0 {
        egui::Area::new(egui::Id::unique("hotkeys")).anchor(egui::Align2::CENTER_BOTTOM, Vec2::new(0.0, -BAR_BOTTOM)).show(ctx, |ui| {
            let frame = if v.skin.is_some() {
                egui::Frame::new().fill(Color32::WHITE).stroke(egui::Stroke::new(1.0, Color32::from_rgb(150, 160, 190))).inner_margin(4)
            } else {
                egui::Frame::new().fill(Color32::from_rgba_unmultiplied(16, 20, 28, 210)).corner_radius(8).inner_margin(6)
            };
            let bar = frame.show(ui, |ui| {
                if v.skin.is_none() {
                    ui.style_mut().visuals = egui::Visuals::dark();
                }
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        // The first row is at the bottom, the next ones stacked above it.
                        for offset in (0..v.hotkey_rows.min(HOTKEY_ROWS)).rev() {
                            let row = (v.hotkey_row + offset) % HOTKEY_ROWS;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                // With a controller, each row starts with the trigger(s) that reach it.
                                if let Some(style) = v.pad {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::new(46.0, SIZE), egui::Sense::hover());
                                    if let Some(t) = pad_row_triggers(offset, v.battle_mode) {
                                        let h = 16.0;
                                        let w: f32 = t.iter().map(|b| crate::padglyph::width(*b, h)).sum::<f32>() + 2.0 * (t.len() as f32 - 1.0);
                                        let mut x = rect.center().x - w / 2.0;
                                        for b in t {
                                            x += crate::padglyph::paint_at(ui.painter(), egui::pos2(x, rect.center().y - h / 2.0), *b, style, h) + 2.0;
                                        }
                                    }
                                }
                                for col in 0..HOTKEYS_PER_ROW {
                                    let label = slot_key_label(v, offset, col);
                                    if let Some(a) = hotkey_slot_ui(ui, v, row as usize * HOTKEYS_PER_ROW + col, &label, slot_glyph(v, offset, col), SIZE) {
                                        action = Some(a);
                                    }
                                }
                            });
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        let page = ui.add(egui::Button::new(RichText::new(format!("{}", v.hotkey_row + 1)).size(13.0)).min_size(Vec2::new(22.0, SIZE)));
                        if page.on_hover_text("Next row").clicked() {
                            action = Some(UiAction::HotkeyRow((v.hotkey_row + 1) % HOTKEY_ROWS));
                        }
                        if v.battle_mode {
                            ui.label(RichText::new("BM").size(10.0).color(Color32::from_rgb(230, 120, 90))).on_hover_text("Battle mode (/bm): letter keys use the shortcut rows; Enter or Space to chat");
                        }
                    });
                });
            });
            // The top edge: drag it up for more rows, down for fewer.
            let rect = bar.response.rect;
            // Where the bar is, for the controller's cross bar beside it.
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique("hotkey bar rect"), rect));
            let edge = egui::Rect::from_min_max(rect.left_top() - Vec2::new(0.0, 5.0), rect.right_top() + Vec2::new(0.0, 5.0));
            let grip = ui.interact(edge, egui::Id::unique("hotkey bar resize"), egui::Sense::drag());
            if grip.hovered() || grip.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                let c = egui::pos2(rect.center().x, rect.top() + 3.0);
                let color = if v.skin.is_some() { Color32::from_rgb(90, 100, 140) } else { Color32::from_gray(200) };
                for dx in [-8.0, 0.0, 8.0] {
                    ui.painter().circle_filled(c + Vec2::new(dx, 0.0), 1.6, color);
                }
            }
            if let (true, Some(p)) = (grip.dragged(), grip.interact_pointer_pos()) {
                // Rows stack up from where the bar is anchored, one slot plus spacing each. (Not
                // from the bar's own rect: while it changes size that lags a frame, and the count
                // would flicker.)
                let bottom = ui.ctx().content_rect().bottom() - BAR_BOTTOM;
                let rows = ((bottom - p.y) / (SIZE + 4.0)).round().clamp(1.0, HOTKEY_ROWS as f32) as u8;
                if rows != v.hotkey_rows {
                    action = Some(UiAction::HotkeyRows(rows));
                }
            }
        });
    }
    // Show what is being dragged under the pointer.
    let dragged = egui::DragAndDrop::payload::<Hotkey>(ctx).map(|k| *k).or_else(|| egui::DragAndDrop::payload::<ItemDrag>(ctx).map(|d| Hotkey { skill: false, id: d.id, count: 0 }));
    if let (Some(key), Some(pos)) = (dragged, ctx.pointer_interact_pos()) {
        let tex = if key.skill { v.skill_icons.get(&(key.id as u16)) } else { v.icons.get(&key.id) };
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("drag icon")));
        paint_icon(&painter, egui::Rect::from_center_size(pos, Vec2::splat(SIZE)), tex, Color32::from_white_alpha(200));
    }
    action
}

/// Equipment slots in display order: (bits, label).
/// The equipment window's tabs: worn gear, costumes, shadow gear.
const EQUIP_TABS: [&str; 4] = ["Equip", "Costume", "Shadow", "Switch"];

/// Moves costume and shadow slot bits onto the normal slot each one stands in for, so
/// every tab can use the same layout; bits that belong to another tab are dropped.
fn tab_bits(equipped: u32, tab: u8) -> u32 {
    const COSTUME: [(u32, u32); 4] = [(0x400, 0x100), (0x800, 0x200), (0x1000, 0x001), (0x2000, 0x004)];
    const SHADOW: [(u32, u32); 6] = [(0x10000, 0x010), (0x20000, 0x002), (0x40000, 0x020), (0x80000, 0x040), (0x100000, 0x008), (0x200000, 0x080)];
    let map: &[(u32, u32)] = match tab {
        0 | 3 => return equipped & 0x3ff,
        1 => &COSTUME,
        _ => &SHADOW,
    };
    map.iter().filter(|(from, _)| equipped & from != 0).fold(0, |acc, (_, to)| acc | to)
}

/// The slots a tab has (costumes have no weapon, armor, shoes or accessories).
fn tab_slots(tab: u8) -> u32 {
    match tab {
        0 | 3 => 0x3ff,
        1 => 0x100 | 0x200 | 0x001 | 0x004,
        _ => 0x002 | 0x010 | 0x020 | 0x040 | 0x008 | 0x080,
    }
}

const STAT_NAMES: [&str; 6] = ["STR", "AGI", "VIT", "INT", "DEX", "LUK"];
const TRAIT_NAMES: [&str; 6] = ["POW", "STA", "WIS", "SPL", "CON", "CRT"];
/// What each trait raises, beside it as in the original's trait page.
const ADVANCED_NAMES: [&str; 6] = ["P.Atk", "S.Matk", "Res", "Mres", "H.Plus", "C.Rate"];

/// The 4th-job trait stats under the base stats: each trait with its bonus and a raise button
/// (costing trait points), the values they give, and the trait points left.
fn trait_section(ui: &mut egui::Ui, s: &Stats, skin: Option<&crate::classic::Skin>) -> Option<UiAction> {
    let mut action = None;
    let gray = crate::classic::ink(Color32::GRAY);
    egui::Grid::new("traits").num_columns(5).spacing(Vec2::new(10.0, 3.0)).show(ui, |ui| {
        for i in 0..6 {
            ui.label(RichText::new(TRAIT_NAMES[i]).strong().size(11.5));
            ui.label(RichText::new(if s.trait_plus[i] != 0 { format!("{} + {}", s.traits[i], s.trait_plus[i]) } else { s.traits[i].to_string() }).size(11.5));
            let can = s.trait_points >= s.trait_cost[i].max(1) as u32;
            let tip = format!("Costs {} trait point{}", s.trait_cost[i], if s.trait_cost[i] == 1 { "" } else { "s" });
            let resp = match skin {
                Some(skin) if can => skin.button(ui, "+"),
                Some(_) => ui.label(""),
                None => ui.add_enabled(can, egui::Button::new("+").small()),
            };
            if can && resp.on_hover_text(tip).clicked() {
                action = Some(UiAction::RaiseTrait(i));
            }
            ui.label(RichText::new(ADVANCED_NAMES[i]).color(gray).size(11.5));
            ui.label(RichText::new(s.advanced[i].to_string()).size(11.5));
            ui.end_row();
        }
    });
    ui.label(RichText::new(format!("Trait points: {}", s.trait_points)).size(11.5));
    action
}

fn item_name(db: Option<&ItemDb>, it: &InvItem) -> String {
    let base = db.map_or_else(|| format!("Item #{}", it.id), |d| d.name(it.id, it.identified));
    let base = if it.refine > 0 { format!("+{} {base}", it.refine) } else { base };
    // Slotted equipment shows its slot count, as in the original ("Knife [3]").
    match db.and_then(|d| d.get(it.id)).map(|i| i.slots) {
        Some(n) if n > 0 && it.identified => format!("{base} [{n}]"),
        _ => base,
    }
}

/// Cards compounded into an item (forged, brewed and pet-egg items reuse the slots for other data).
fn inserted_cards(it: &InvItem) -> impl Iterator<Item = u32> + '_ {
    let special = matches!(it.cards[0], 0x00ff | 0x00fe | 0xff00);
    it.cards.iter().copied().filter(move |&c| !special && c != 0)
}

/// Item tooltip: name, inserted cards and the client's description (^RRGGBB codes stripped).
fn item_tooltip(ui: &mut egui::Ui, db: Option<&ItemDb>, it: &InvItem) {
    ui.label(RichText::new(item_name(db, it)).strong());
    let slots = db.and_then(|d| d.get(it.id)).map_or(0, |i| i.slots) as usize;
    if slots > 0 && it.is_equipment() {
        let cards: Vec<u32> = inserted_cards(it).collect();
        for i in 0..slots.max(cards.len()) {
            let text = match cards.get(i) {
                Some(&c) => db.map_or_else(|| format!("Card #{c}"), |d| d.name(c, true)),
                None => "(empty slot)".to_string(),
            };
            ui.label(RichText::new(text).color(crate::classic::ink(if i < cards.len() { Color32::from_rgb(140, 200, 255) } else { Color32::GRAY })));
        }
        ui.separator();
    }
    if let Some(info) = db.and_then(|d| d.get(it.id)) {
        for line in &info.description {
            ui.label(strip_colors(line));
        }
    }
    if it.kind == ro_game::items::kind::CARD {
        ui.separator();
        ui.label(RichText::new("Double-click to insert into equipment").color(crate::classic::ink(Color32::GRAY)).size(11.5));
    }
}

impl Windows {
    /// The skill window's contents (points, tree/list toggle, job tabs, the skills). Classic
    /// draws the tabs and cells in the original's light style.
    fn skills_body(&mut self, ui: &mut egui::Ui, v: &View) -> Option<UiAction> {
        let mut action = None;
        if let Some(skin) = v.skin {
            // Classic: the points sit on the bottom bar; the view toggle as tabs.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if skin.tab(ui, "List", self.skill_list).clicked() {
                        self.skill_list = true;
                    }
                    if skin.tab(ui, "Tree", !self.skill_list).clicked() {
                        self.skill_list = false;
                    }
                });
            });
        } else {
            ui.horizontal(|ui| {
                ui.label(format!("Skill points: {}", v.skill_points));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut self.skill_list, true, "List");
                    ui.selectable_value(&mut self.skill_list, false, "Tree");
                });
            });
            ui.separator();
        }
        let tabs = v.skill_tree.map(|t| t.tabs(v.job)).unwrap_or_default();
        if self.skill_list || tabs.is_empty() {
            if v.skills.is_empty() {
                ui.label(RichText::new("No skills").color(crate::classic::ink(Color32::GRAY)));
            }
            // Long skill lists (3rd and 4th jobs) scroll instead of running off the screen.
            egui::ScrollArea::vertical().max_height(420.0).min_scrolled_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("skills").num_columns(4).spacing(Vec2::new(8.0, 4.0)).show(ui, |ui| {
                for s in v.skills {
                    let name = (v.skill_names)(&s.name);
                    let icon = skill_icon(ui, v, s.id, Some(s), 24.0);
                    let label = ui.add(egui::Label::new(RichText::new(&name)).sense(egui::Sense::click()));
                    if s.level > 0 && s.target != 0 && (icon.double_clicked() || label.double_clicked()) {
                        action = Some(UiAction::UseSkill(s.id));
                    }
                    icon.on_hover_ui(|ui| skill_tooltip(ui, v, &s.name, Some(s)));
                    label.on_hover_ui(|ui| skill_tooltip(ui, v, &s.name, Some(s)));
                    ui.label(format!("Lv {}", s.level));
                    let can = s.upgradable && v.skill_points > 0;
                    let clicked = match v.skin {
                        Some(skin) if can => skin.button(ui, "+").clicked(),
                        Some(_) => {
                            ui.label("");
                            false
                        }
                        None => ui.add_enabled(can, egui::Button::new("+")).clicked(),
                    };
                    if clicked {
                        action = Some(UiAction::SkillUp(s.id));
                    }
                    ui.end_row();
                }
            });
            });
        } else {
            // One tab per job tier, as in the original skill tree window.
            let mut tab = self.skill_tab.unwrap_or(usize::MAX).min(tabs.len() - 1);
            ui.horizontal(|ui| {
                for (i, (job, _)) in tabs.iter().enumerate() {
                    let clicked = match v.skin {
                        Some(skin) => skin.tab(ui, crate::jobs::title(*job), tab == i).clicked(),
                        None => ui.selectable_label(tab == i, crate::jobs::title(*job)).clicked(),
                    };
                    if clicked {
                        tab = i;
                    }
                }
            });
            ui.add_space(4.0);
            self.skill_tab = Some(tab);
            let grid = tabs[tab].1;
            if tabs[tab].0 == crate::samurai::JOB {
                ui.horizontal(|ui| {
                    ui.add_sized([4.0 * 58.0, 20.0], egui::Label::new(RichText::new("Divinity / Kusanagi").strong()));
                    ui.label(RichText::new("Cursed / Muramasa").strong());
                });
            }
            let rows = grid.iter().map(|(slot, _)| slot / crate::skilltree::COLUMNS + 1).max().unwrap_or(1);
            const CELL: f32 = 58.0;
            let cols = crate::skilltree::COLUMNS;
            let (area, _) = ui.allocate_exact_size(Vec2::new(cols as f32 * CELL, rows as f32 * CELL), egui::Sense::hover());
            for (slot, internal) in grid {
                let at = area.min + Vec2::new((slot % cols) as f32 * CELL, (slot / cols) as f32 * CELL);
                let cell = egui::Rect::from_min_size(at, Vec2::splat(CELL)).shrink(2.0);
                let skill = v.skills.iter().find(|s| &s.name == internal);
                let id = skill.map(|s| s.id).or_else(|| v.skill_info.and_then(|i| i.id_of(internal)));
                let lv = skill.map_or(0, |s| s.level);
                if v.skin.is_some() {
                    let fill = if lv > 0 { Color32::from_rgb(230, 236, 250) } else { Color32::from_rgb(246, 247, 250) };
                    ui.painter().rect(cell, 3.0, fill, egui::Stroke::new(1.0, Color32::from_rgb(196, 204, 226)), egui::StrokeKind::Inside);
                } else {
                    ui.painter().rect_filled(cell, 4.0, if lv > 0 { Color32::from_gray(48) } else { Color32::from_gray(30) });
                }
                let icon_rect = egui::Rect::from_center_size(cell.center_top() + Vec2::new(0.0, 19.0), Vec2::splat(32.0));
                let icon = ui.scope_builder(egui::UiBuilder::new().max_rect(icon_rect), |ui| skill_icon(ui, v, id.unwrap_or(0), skill, 32.0)).inner;
                if let Some(s) = skill.filter(|s| s.level > 0 && s.target != 0) {
                    if icon.double_clicked() {
                        action = Some(UiAction::UseSkill(s.id));
                    }
                }
                icon.on_hover_ui(|ui| skill_tooltip(ui, v, internal, skill));
                let color = match (lv > 0, v.skin.is_some()) {
                    (true, true) => crate::classic::TEXT,
                    (true, false) => Color32::WHITE,
                    _ => Color32::GRAY,
                };
                ui.painter().text(cell.left_bottom() + Vec2::new(5.0, -3.0), egui::Align2::LEFT_BOTTOM, format!("{lv}"), egui::FontId::proportional(11.0), color);
                if let Some(s) = skill.filter(|s| s.upgradable && v.skill_points > 0) {
                    let plus = egui::Rect::from_min_size(cell.right_bottom() - Vec2::new(18.0, 16.0), Vec2::new(16.0, 14.0));
                    let clicked = match v.skin {
                        Some(skin) => ui.scope_builder(egui::UiBuilder::new().max_rect(plus), |ui| skin.button(ui, "+")).inner.clicked(),
                        None => ui.put(plus, egui::Button::new(RichText::new("+").size(10.0))).clicked(),
                    };
                    if clicked {
                        action = Some(UiAction::SkillUp(s.id));
                    }
                }
            }
        }
        action
    }
}

/// What an inventory slot does: tooltip, drag (to the shortcut bar, storage or the world),
/// double-click to use or equip, and the right-click menu. Shared by both interface styles.
fn inventory_item_actions(resp: egui::Response, it: &InvItem, v: &View) -> Option<UiAction> {
    let mut action = None;
    let resp = resp.on_hover_ui(|ui| compare_tooltip(ui, v.db, it, v.items));
    resp.dnd_set_drag_payload(ItemDrag { index: it.index, id: it.id, amount: it.amount, shortcut: it.is_usable() || it.is_equipment(), loose: it.equipped == 0 });
    // One-click identify: an unidentified item and a Magnifier (or Novice Magnifier) to hand.
    let identify = !it.identified && v.items.iter().any(|m| matches!(m.id, 611 | 12325) && m.equipped == 0);
    if identify && resp.double_clicked() {
        action = Some(UiAction::Identify(it.index));
    } else if resp.double_clicked() {
        action = Some(if it.kind == ro_game::items::kind::CARD {
            UiAction::CardTargets(it.index)
        } else if it.is_equipment() {
            UiAction::Equip { index: it.index, location: it.location }
        } else {
            UiAction::UseItem(it.index)
        });
    }
    resp.context_menu(|ui| {
        let card = it.kind == ro_game::items::kind::CARD;
        if ui.button(if card { "View card" } else { "Info" }).clicked() {
            action = Some(UiAction::ItemInfo { id: it.id, refine: it.refine, cards: it.cards });
            ui.close();
        }
        if identify && ui.button("Identify (uses a Magnifier)").clicked() {
            action = Some(UiAction::Identify(it.index));
            ui.close();
        }
        if it.is_equipment() && ui.button("Equip").clicked() {
            action = Some(UiAction::Equip { index: it.index, location: it.location });
            ui.close();
        }
        if it.is_usable() && ui.button("Use").clicked() {
            action = Some(UiAction::UseItem(it.index));
            ui.close();
        }
        if it.equipped == 0 && ui.button(if it.favorite { "Move out of favourites" } else { "Move to favourites" }).clicked() {
            action = Some(UiAction::Favorite { index: it.index, on: !it.favorite });
            ui.close();
        }
        if v.has_cart && it.equipped == 0 && ui.button("Move to cart").clicked() {
            action = Some(UiAction::CartPut { index: it.index, amount: it.amount });
            ui.close();
        }
        if ui.button(if it.amount > 1 { "Drop all" } else { "Drop" }).clicked() {
            action = Some(UiAction::Drop { index: it.index, amount: it.amount });
            ui.close();
        }
    });
    action
}

/// The item's tooltip; for unworn equipment, beside whatever is worn in the same slots.
fn compare_tooltip(ui: &mut egui::Ui, db: Option<&ItemDb>, it: &InvItem, items: &[&InvItem]) {
    let worn: Vec<&&InvItem> = if it.is_equipment() && it.equipped == 0 { items.iter().filter(|w| w.equipped & it.location != 0).collect() } else { vec![] };
    if worn.is_empty() {
        item_tooltip(ui, db, it);
        return;
    }
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width(260.0);
            item_tooltip(ui, db, it);
        });
        for w in worn {
            ui.separator();
            ui.vertical(|ui| {
                ui.set_max_width(260.0);
                ui.label(RichText::new("Currently worn").color(crate::classic::ink(Color32::GRAY)).size(11.5));
                item_tooltip(ui, db, w);
            });
        }
    });
}

fn strip_colors(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('^') {
        out.push_str(&rest[..i]);
        let code = rest.get(i + 1..i + 7);
        if code.is_some_and(|c| c.chars().all(|ch| ch.is_ascii_hexdigit())) {
            rest = &rest[i + 7..];
        } else {
            out.push('^');
            rest = &rest[i + 1..];
        }
    }
    out.push_str(rest);
    out
}

impl View<'_> {
    /// An inventory item's icon: its unidentified look until it is identified.
    fn item_icon(&self, it: &InvItem) -> Option<&egui::TextureHandle> {
        if it.identified {
            self.icons.get(&it.id)
        } else {
            self.unid_icons.get(&it.id).or_else(|| self.icons.get(&it.id))
        }
    }
}

/// A square slot with the item icon (or a placeholder) and its amount.
fn item_slot(ui: &mut egui::Ui, v: &View, it: &InvItem, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    let hovered = resp.hovered();
    painter.rect_filled(rect, 4.0, if hovered { Color32::from_white_alpha(30) } else { Color32::from_white_alpha(12) });
    if let Some(tex) = v.item_icon(it) {
        let s = tex.ui_size().min(Vec2::splat(size - 6.0));
        painter.image(tex.id(), egui::Rect::from_center_size(rect.center(), s), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    if it.amount > 1 {
        let font = egui::FontId::proportional(11.0);
        let pos = rect.right_bottom() - Vec2::new(3.0, 1.0);
        painter.text(pos + Vec2::new(1.0, 1.0), egui::Align2::RIGHT_BOTTOM, it.amount.to_string(), font.clone(), Color32::BLACK);
        painter.text(pos, egui::Align2::RIGHT_BOTTOM, it.amount.to_string(), font, Color32::WHITE);
    }
    if it.equipped != 0 {
        painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.5, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Inside);
    }
    resp
}

impl Windows {
    /// Test hook: open the inventory on a tab (0 item, 1 equip, 2 etc, 3 favourites).
    pub fn inventory_tab(&mut self, n: u8) {
        self.inventory = true;
        self.tab = match n {
            0 => InvTab::Items,
            1 => InvTab::Equip,
            2 => InvTab::Etc,
            _ => InvTab::Fav,
        };
    }

    pub fn toggle_inventory(&mut self) {
        self.inventory = !self.inventory;
    }
    pub fn toggle_equipment(&mut self) {
        self.equipment = !self.equipment;
    }
    pub fn toggle_stats(&mut self) {
        self.stats = !self.stats;
    }
    pub fn toggle_skills(&mut self) {
        self.skills = !self.skills;
    }

    /// Small always-visible buttons for the windows (top right).
    fn menu(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::unique("menu")).anchor(egui::Align2::RIGHT_TOP, Vec2::new(-12.0, 12.0)).show(ctx, |ui| {
            let classic = crate::classic::skin(ui.ctx()).is_some();
            let frame = if classic {
                egui::Frame::new().fill(Color32::WHITE).stroke(egui::Stroke::new(1.0, Color32::from_rgb(150, 160, 190))).inner_margin(4)
            } else {
                egui::Frame::new().fill(Color32::from_rgba_unmultiplied(16, 20, 28, 210)).corner_radius(8).inner_margin(6)
            };
            frame.show(ui, |ui| {
                    if !classic {
                        ui.style_mut().visuals = egui::Visuals::dark();
                    }
                    ui.horizontal(|ui| {
                        let (icon, tip) = if self.menu_collapsed { ("«", "Show the menu") } else { ("»", "Hide the menu") };
                        if ui.selectable_label(false, icon).on_hover_text(tip).clicked() {
                            self.menu_collapsed = !self.menu_collapsed;
                            self.menu_toggled = true;
                        }
                        if self.menu_collapsed {
                            return;
                        }
                        for (label, key, open) in [
                            ("Inventory", "Alt+E", &mut self.inventory),
                            ("Equipment", "Alt+Q", &mut self.equipment),
                            ("Status", "Alt+A", &mut self.stats),
                            ("Skills", "Alt+S", &mut self.skills),
                            ("Party", "Alt+Z", &mut self.party),
                            ("Guild", "Alt+G", &mut self.guild),
                        ] {
                            if ui.selectable_label(*open, label).on_hover_text(key).clicked() {
                                *open = !*open;
                            }
                        }
                        if ui.selectable_label(false, "Mail").on_hover_text("Alt+X").clicked() {
                            self.menu_key = Some("x");
                        }
                        if ui.selectable_label(self.cash_shown, "Cash Shop").on_hover_text("Alt+C").clicked() {
                            self.cash_toggle = true;
                        }
                        // Everything else, in a dropdown so the bar stays short.
                        ui.menu_button("More…", |ui| {
                            ui.set_min_width(190.0);
                            if ui.selectable_label(self.friends, "Friends   Alt+H").clicked() {
                                self.friends = !self.friends;
                                ui.close();
                            }
                            for (label, key) in [
                                ("Clan", "clan"),
                                ("Ranking", "rank"),
                                ("Roulette", "roulette"),
                                ("Reputation", "repute"),
                                ("Game Options", "opts"),
                                ("Companions   Alt+R", "r"),
                                ("Navigation   /navi", "navi"),
                                ("Party Booking   Alt+N", "n"),
                                ("Chat Room   Alt+K", "k"),
                                ("Battle & Castles", "battle"),
                                ("Achievements   Alt+Y", "y"),
                                ("Combat Log   Alt+D", "d"),
                                ("MVP Timers", "mvp"),
                                ("Attendance   Alt+J", "j"),
                            ] {
                                if ui.selectable_label(false, label).clicked() {
                                    self.menu_key = Some(key);
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui.selectable_label(self.settings, "Settings   Alt+O").clicked() {
                                self.settings = !self.settings;
                                ui.close();
                            }
                        });
                    });
                });
        });
    }

    pub fn show(&mut self, ctx: &egui::Context, v: &View) -> Option<UiAction> {
        self.menu(ctx);
        let mut action = hotkey_bar(ctx, v);
        if let Some(a) = cross_bar(ctx, v) {
            action = Some(a);
        }

        // Default layout across the top, sized to the screen (windows remember moves after).
        let w = v.screen_width;
        let col = |i: f32| egui::pos2((w * 0.22 + i * 330.0).min(w - 340.0), 110.0);

        if let (true, Some(skin)) = (self.inventory, v.skin) {
            const COLS: usize = 7;
            let mut open = true;
            let mut tab = self.tab;
            let mut scroll = self.inv_scroll;
            let mut picked = None;
            let mut bank = false;
            let mut expand = false;
            let shown: Vec<&&InvItem> = v
                .items
                .iter()
                .filter(|it| it.equipped == 0)
                .filter(|it| match tab {
                    InvTab::Items => !it.favorite && it.is_usable(),
                    InvTab::Equip => !it.favorite && it.is_equipment(),
                    InvTab::Etc => !it.favorite && !it.is_usable() && !it.is_equipment(),
                    InvTab::Fav => it.favorite,
                })
                .collect();
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            skin.window(
                ctx,
                "Inventory",
                col(2.0),
                20.0 + COLS as f32 * 32.0 + 16.0,
                &mut open,
                |ui| {
                    // Fixed layout over the art: the tab strip, then the slot grid with a
                    // thin scrollbar; the wheel scrolls rows.
                    let rows = shown.len().div_ceil(COLS).clamp(4, 6);
                    let total_rows = shown.len().div_ceil(COLS).max(rows);
                    let (area, area_resp) = ui.allocate_exact_size(Vec2::new(20.0 + COLS as f32 * 32.0 + 16.0, rows as f32 * 32.0), egui::Sense::hover());
                    let p = ui.painter().clone();
                    // The art has three tabs (20x82); a fourth "fav" tab is stitched from
                    // slices of the same images: tab borders, plain body rows and the end cap.
                    let strip_rect = egui::Rect::from_min_size(area.min, Vec2::new(20.0, 109.0));
                    let blit = |name: &str, src: (f32, f32), dst: (f32, f32)| {
                        if let Some(t) = skin.texture(name) {
                            let h = t.ui_size().y;
                            let uv = egui::Rect::from_min_max(egui::pos2(0.0, src.0 / h), egui::pos2(1.0, src.1 / h));
                            let r = egui::Rect::from_min_max(strip_rect.min + Vec2::new(0.0, dst.0), strip_rect.min + Vec2::new(20.0, dst.1));
                            p.image(t.id(), r, uv, Color32::WHITE);
                        }
                    };
                    // Plain body rows repeat every 4 pixels (the dotted left edge).
                    let body = |name: &str, src: f32, dst: (f32, f32)| {
                        let mut y = dst.0;
                        while y < dst.1 {
                            let n = (dst.1 - y).min(4.0);
                            blit(name, (src, src + n), (y, y + n));
                            y += n;
                        }
                    };
                    let fav = tab == InvTab::Fav;
                    match tab {
                        InvTab::Items => blit("tab_itm_01", (0.0, 75.0), (0.0, 75.0)),
                        InvTab::Equip => blit("tab_itm_02", (0.0, 75.0), (0.0, 75.0)),
                        InvTab::Etc => blit("tab_itm_03", (0.0, 75.0), (0.0, 75.0)),
                        InvTab::Fav => {
                            // All three unselected: item from the equip art, the rest from the item art.
                            blit("tab_itm_02", (0.0, 22.0), (0.0, 22.0));
                            blit("tab_itm_01", (48.0, 53.0), (22.0, 27.0));
                            blit("tab_itm_01", (27.0, 75.0), (27.0, 75.0));
                        }
                    }
                    // Border between etc and fav, then the fav body and the end cap.
                    match tab {
                        InvTab::Etc => blit("tab_itm_01", (22.0, 27.0), (75.0, 80.0)),
                        InvTab::Fav => blit("tab_itm_03", (48.0, 53.0), (75.0, 80.0)),
                        _ => blit("tab_itm_01", (48.0, 53.0), (75.0, 80.0)),
                    }
                    if fav {
                        body("tab_itm_03", 54.0, (80.0, 102.0));
                        blit("tab_itm_03", (75.0, 82.0), (102.0, 109.0));
                    } else {
                        body("tab_itm_01", 54.0, (80.0, 102.0));
                        blit("tab_itm_01", (75.0, 82.0), (102.0, 109.0));
                    }
                    // The label, rotated to read bottom-to-top like the art's.
                    let galley = p.layout_no_wrap("fav".into(), egui::FontId::proportional(10.0), Color32::from_gray(60));
                    let cx = strip_rect.min.x + if fav { 10.0 } else { 11.5 };
                    let at = egui::pos2(cx - galley.size().y / 2.0, strip_rect.min.y + 91.0 + galley.size().x / 2.0);
                    p.add(egui::epaint::TextShape::new(at, galley, Color32::from_gray(60)).with_angle(-std::f32::consts::FRAC_PI_2));
                    let resp = ui.interact(strip_rect, ui.scope_id().with("inv_tabs"), egui::Sense::click());
                    if let Some(at) = resp.clicked().then(|| resp.interact_pointer_pos()).flatten() {
                        tab = match ((at.y - strip_rect.min.y) / 27.0) as i32 {
                            0 => InvTab::Items,
                            1 => InvTab::Equip,
                            2 => InvTab::Etc,
                            _ => InvTab::Fav,
                        };
                        scroll = 0;
                    }
                    let max_scroll = total_rows - rows;
                    if area_resp.hovered() {
                        let dy: f32 = ui.input(|i| i.raw.events.iter().map(|e| if let egui::Event::MouseWheel { delta, .. } = e { delta.y } else { 0.0 }).sum());
                        if dy < 0.0 {
                            scroll = (scroll + 1).min(max_scroll);
                        } else if dy > 0.0 {
                            scroll = scroll.saturating_sub(1);
                        }
                    }
                    scroll = scroll.min(max_scroll);
                    for r in 0..rows {
                        for c in 0..COLS {
                            let cell = egui::Rect::from_min_size(area.min + Vec2::new(20.0 + c as f32 * 32.0, r as f32 * 32.0), Vec2::splat(32.0));
                            skin.draw(&p, "itemwin_mid", cell.min);
                            let Some(it) = shown.get((r + scroll) * COLS + c) else { continue };
                            let resp = ui.interact(cell, ui.scope_id().with(("inv_cell", it.index)), egui::Sense::click_and_drag());
                            if resp.hovered() {
                                p.rect_filled(cell.shrink(1.0), 3.0, Color32::from_rgba_unmultiplied(120, 150, 230, 50));
                            }
                            if let Some(t) = v.item_icon(it) {
                                p.image(t.id(), egui::Rect::from_center_size(cell.center(), t.ui_size().min(Vec2::splat(24.0))), uv, Color32::WHITE);
                            }
                            if it.amount > 1 {
                                let font = egui::FontId::proportional(10.5);
                                let at = cell.right_bottom() - Vec2::new(2.0, 0.0);
                                p.text(at + Vec2::new(1.0, 1.0), egui::Align2::RIGHT_BOTTOM, it.amount.to_string(), font.clone(), Color32::WHITE);
                                p.text(at, egui::Align2::RIGHT_BOTTOM, it.amount.to_string(), font, Color32::BLACK);
                            }
                            if let Some(a) = inventory_item_actions(resp, it, v) {
                                picked = Some(a);
                            }
                        }
                    }
                    // Scrollbar: a track and a thumb sized to the visible share.
                    let track = egui::Rect::from_min_max(egui::pos2(area.max.x - 12.0, area.min.y + 2.0), egui::pos2(area.max.x - 4.0, area.max.y - 2.0));
                    p.rect_filled(track, 3.0, Color32::from_rgb(226, 230, 242));
                    let share = rows as f32 / total_rows as f32;
                    let top = track.min.y + track.height() * scroll as f32 / total_rows as f32;
                    p.rect_filled(egui::Rect::from_min_size(egui::pos2(track.min.x, top), Vec2::new(track.width(), track.height() * share)), 3.0, Color32::from_rgb(140, 160, 214));
                },
                |ui| {
                    ui.label(RichText::new(format!("{} z", v.zeny)).size(11.5));
                    ui.add_space(6.0);
                    bank = skin.button(ui, "Bank").on_hover_text("Alt+B").clicked();
                    ui.label(RichText::new(format!("{} / {}", v.items.len(), 100 + self.inv_extra)).size(11.5)).on_hover_text("Inventory slots");
                    if skin.button(ui, "+").on_hover_text("Expand the inventory").clicked() {
                        expand = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} / {}", v.weight.0 / 10, v.weight.1 / 10)).size(11.5)).on_hover_text("Weight");
                    });
                },
            );
            self.tab = tab;
            self.inv_scroll = scroll;
            if picked.is_some() {
                action = picked;
            }
            if bank {
                action = Some(UiAction::OpenBank);
            }
            if expand {
                action = Some(UiAction::ExpandInventory);
            }
            self.inventory = open;
        } else if self.inventory {
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new("Inventory").open(&mut open).default_pos(col(2.0)).resizable(false), "Inventory", true, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, InvTab::Items, "Items");
                    ui.selectable_value(&mut self.tab, InvTab::Equip, "Equip");
                    ui.selectable_value(&mut self.tab, InvTab::Etc, "Etc");
                    ui.selectable_value(&mut self.tab, InvTab::Fav, "Favorites");
                });
                ui.separator();
                let shown: Vec<&&InvItem> = v
                    .items
                    .iter()
                    .filter(|it| it.equipped == 0)
                    .filter(|it| match self.tab {
                        InvTab::Items => !it.favorite && it.is_usable(),
                        InvTab::Equip => !it.favorite && it.is_equipment(),
                        InvTab::Etc => !it.favorite && !it.is_usable() && !it.is_equipment(),
                        InvTab::Fav => it.favorite,
                    })
                    .collect();
                egui::Grid::new("inv").spacing(Vec2::splat(4.0)).show(ui, |ui| {
                    for (i, it) in shown.iter().enumerate() {
                        let resp = item_slot(ui, v, it, 38.0);
                        if let Some(a) = inventory_item_actions(resp, it, v) {
                            action = Some(a);
                        }
                        if i % 8 == 7 {
                            ui.end_row();
                        }
                    }
                });
                if shown.is_empty() {
                    ui.label(RichText::new("Empty").color(crate::classic::ink(Color32::GRAY)));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("{} z", v.zeny));
                    if ui.small_button("Bank").on_hover_text("Alt+B").clicked() {
                        action = Some(UiAction::OpenBank);
                    }
                    ui.label(format!("{} / {}", v.items.len(), 100 + self.inv_extra)).on_hover_text("Inventory slots");
                    if ui.small_button("+").on_hover_text("Expand the inventory").clicked() {
                        action = Some(UiAction::ExpandInventory);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!("Weight {} / {}", v.weight.0 / 10, v.weight.1 / 10));
                    });
                });
            });
            self.inventory = open;
        }

        if let (true, Some(skin)) = (self.equipment, v.skin) {
            let mut open = true;
            let mut picked = None;
            let mut show_equip = None;
            let tab = self.equip_tab;
            let mut new_tab = None;
            skin.window(
                ctx,
                "Equipment",
                col(1.0),
                280.0,
                &mut open,
                |ui| {
                    // The tabs head the body: the bottom bar has room only for the checkbox.
                    ui.horizontal(|ui| {
                        for (i, name) in EQUIP_TABS.iter().enumerate() {
                            if skin.tab(ui, name, tab == i as u8).clicked() {
                                new_tab = Some(i as u8);
                            }
                        }
                    });
                    let (area, _) = ui.allocate_exact_size(Vec2::new(280.0, 130.0), egui::Sense::hover());
                    let p = ui.painter().clone();
                    skin.draw(&p, "equipwin_bg", area.min);
                    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                    // The character stands in the middle column, between the slots.
                    if let Some(t) = v.portrait {
                        let column = egui::Rect::from_min_max(egui::pos2(area.min.x + 105.0, area.min.y + 2.0), egui::pos2(area.min.x + 175.0, area.max.y - 2.0));
                        p.with_clip_rect(column).image(t.id(), egui::Rect::from_center_size(column.center() + Vec2::new(0.0, 8.0), Vec2::splat(126.0)), uv, Color32::WHITE);
                    }
                    let mut used_acc = vec![];
                    // (slot bits, left column?, row) as the art lays them out.
                    const LAYOUT: [(u32, bool, usize); 10] =
                        [(0x100, true, 0), (0x200, false, 0), (0x001, true, 1), (0x010, false, 1), (0x002, true, 2), (0x020, false, 2), (0x004, true, 3), (0x040, false, 3), (0x008, true, 4), (0x080, false, 4)];
                    for (bits, left, row) in LAYOUT {
                        let y = area.min.y + 17.0 + row as f32 * 25.6;
                        // Slots this tab lacks: blank the faint label.
                        if bits & tab_slots(tab) == 0 {
                            p.rect_filled(egui::Rect::from_min_max(egui::pos2(area.min.x + if left { 3.0 } else { 176.0 }, y - 10.0), egui::pos2(area.min.x + if left { 104.0 } else { 277.0 }, y + 10.0)), 0.0, Color32::WHITE);
                            continue;
                        }
                        let Some(it) = v.items.iter().find(|it| tab_bits(if tab == 3 { it.switch } else { it.equipped }, tab) & bits != 0 && !used_acc.contains(&it.index)) else { continue };
                        if bits & 0x88 != 0 {
                            used_acc.push(it.index);
                        }

                        let icon_c = egui::pos2(area.min.x + if left { 17.0 } else { 263.0 }, y);
                        let icon = egui::Rect::from_center_size(icon_c, Vec2::splat(24.0));
                        if let Some(t) = v.item_icon(it) {
                            p.image(t.id(), icon, uv, Color32::WHITE);
                        }
                        // The name over the faint slot label, cut to the column.
                        let mut name = item_name(v.db, it);
                        if name.chars().count() > 13 {
                            name = name.chars().take(12).collect::<String>() + "…";
                        }
                        let (at, align) = if left { (egui::pos2(area.min.x + 32.0, y), egui::Align2::LEFT_CENTER) } else { (egui::pos2(area.min.x + 248.0, y), egui::Align2::RIGHT_CENTER) };
                        p.rect_filled(egui::Rect::from_min_max(egui::pos2(area.min.x + if left { 30.0 } else { 176.0 }, y - 8.0), egui::pos2(area.min.x + if left { 104.0 } else { 250.0 }, y + 7.0)), 0.0, Color32::WHITE);
                        p.text(at, align, name, egui::FontId::proportional(11.0), crate::classic::TEXT);
                        let hit = egui::Rect::from_min_max(egui::pos2(area.min.x + if left { 2.0 } else { 176.0 }, y - 11.0), egui::pos2(area.min.x + if left { 104.0 } else { 278.0 }, y + 11.0));
                        let resp = ui.interact(hit, ui.scope_id().with(("equip_slot", bits)), egui::Sense::click()).on_hover_ui(|ui| item_tooltip(ui, v.db, it));
                        if resp.hovered() {
                            p.rect_filled(hit, 3.0, Color32::from_rgba_unmultiplied(120, 150, 230, 40));
                        }
                        if resp.secondary_clicked() {
                            picked = Some(UiAction::ItemInfo { id: it.id, refine: it.refine, cards: it.cards });
                        }
                        if resp.double_clicked() {
                            picked = Some(if tab == 3 { UiAction::SwitchRemove(it.index) } else { UiAction::Unequip(it.index) });
                        }
                    }
                },
                |ui| {
                    if let Some(mut on) = v.show_equip {
                        if ui.checkbox(&mut on, RichText::new("Let others view my equipment").size(11.0)).changed() {
                            show_equip = Some(on);
                        }
                    }
                },
            );
            if let Some(t) = new_tab {
                self.equip_tab = t;
            }
            if picked.is_some() {
                action = picked;
            }
            if let Some(on) = show_equip {
                action = Some(UiAction::ShowEquip(on));
            }
            self.equipment = open;
        } else if self.equipment {
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new("Equipment").open(&mut open).default_pos(col(1.0)).resizable(false), "Equipment", true, |ui| {
                if let Some(mut on) = v.show_equip {
                    if ui.checkbox(&mut on, "Let others view my equipment").changed() {
                        action = Some(UiAction::ShowEquip(on));
                    }
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    for (i, name) in EQUIP_TABS.iter().enumerate() {
                        if ui.selectable_label(self.equip_tab == i as u8, *name).clicked() {
                            self.equip_tab = i as u8;
                        }
                    }
                });
                ui.separator();
                // A paper doll like the original: slots down both sides of the character.
                const ROW: f32 = 46.0;
                const SIDE: f32 = 170.0;
                const MID: f32 = 120.0;
                let (area, _) = ui.allocate_exact_size(Vec2::new(SIDE * 2.0 + MID, ROW * 5.0), egui::Sense::hover());
                let p = ui.painter_at(area);
                if let Some(t) = v.portrait {
                    let c = area.center() + Vec2::new(0.0, -10.0);
                    p.image(t.id(), egui::Rect::from_center_size(c, Vec2::splat(MID + 20.0)), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                // (slot bits, left column?, row, label) in the same order as the Classic art.
                const DOLL: [(u32, bool, usize, &str); 10] = [
                    (0x100, true, 0, "Upper"), (0x200, false, 0, "Middle"), (0x001, true, 1, "Lower"), (0x010, false, 1, "Armor"), (0x002, true, 2, "Weapon"),
                    (0x020, false, 2, "Shield"), (0x004, true, 3, "Garment"), (0x040, false, 3, "Shoes"), (0x008, true, 4, "Accessory"), (0x080, false, 4, "Accessory"),
                ];
                let mut used_acc = vec![];
                for (bits, left, row, label) in DOLL {
                    let y = area.min.y + row as f32 * ROW + 4.0;
                    let x = if left { area.min.x } else { area.max.x - SIDE };
                    let cell = egui::Rect::from_min_size(egui::pos2(x, y), Vec2::new(SIDE, ROW - 8.0));
                    let icon = egui::Rect::from_min_size(egui::pos2(if left { cell.min.x } else { cell.max.x - 38.0 }, y), Vec2::splat(38.0));
                    let (text_at, align) = if left { (egui::pos2(icon.max.x + 6.0, icon.center().y), egui::Align2::LEFT_CENTER) } else { (egui::pos2(icon.min.x - 6.0, icon.center().y), egui::Align2::RIGHT_CENTER) };
                    let available = bits & tab_slots(self.equip_tab) != 0;
                    if !available {
                        continue;
                    }
                    p.rect_filled(icon, 5.0, Color32::from_white_alpha(14));
                    let worn = v.items.iter().find(|it| tab_bits(if self.equip_tab == 3 { it.switch } else { it.equipped }, self.equip_tab) & bits != 0 && !used_acc.contains(&it.index));
                    let Some(it) = worn else {
                        p.text(text_at, align, label, egui::FontId::proportional(12.0), crate::classic::ink(Color32::from_gray(110)));
                        continue;
                    };
                    if bits & 0x88 != 0 {
                        used_acc.push(it.index);
                    }
                    p.rect_stroke(icon, 5.0, egui::Stroke::new(1.5, Color32::from_rgb(120, 200, 255)), egui::StrokeKind::Inside);
                    match v.item_icon(it) {
                        Some(t) => {
                            p.image(t.id(), egui::Rect::from_center_size(icon.center(), Vec2::splat(28.0)), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                        }
                        None => {
                            p.text(icon.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(16.0), Color32::from_gray(150));
                        }
                    }
                    let mut name = item_name(v.db, it);
                    if name.chars().count() > 18 {
                        name = name.chars().take(17).collect::<String>() + "…";
                    }
                    p.text(text_at, align, name, egui::FontId::proportional(12.5), crate::classic::ink(Color32::WHITE));
                    let resp = ui.interact(cell, ui.scope_id().with(("doll", bits)), egui::Sense::click()).on_hover_ui(|ui| item_tooltip(ui, v.db, it));
                    if resp.hovered() {
                        p.rect_filled(cell, 5.0, Color32::from_rgba_unmultiplied(120, 150, 230, 30));
                    }
                    if resp.secondary_clicked() {
                        action = Some(UiAction::ItemInfo { id: it.id, refine: it.refine, cards: it.cards });
                    }
                    if resp.double_clicked() {
                        action = Some(if self.equip_tab == 3 { UiAction::SwitchRemove(it.index) } else { UiAction::Unequip(it.index) });
                    }
                }
                if self.equip_tab != 3 && ui.button("Take off all").clicked() {
                    // Real equip bits: worn gear (and ammo), costumes, shadow gear.
                    action = Some(UiAction::UnequipAll(match self.equip_tab {
                        0 => 0x3ff | 0x8000,
                        1 => 0x3c00,
                        _ => 0x3f_0000,
                    }));
                }
                if self.equip_tab == 3 {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("switch_add").selected_text("Add equipment...").width(220.0).show_ui(ui, |ui| {
                            for it in v.items.iter().filter(|i| i.is_equipment() && i.equipped == 0 && i.switch == 0 && i.location & 0x3ff != 0) {
                                if ui.selectable_label(false, item_name(v.db, it)).clicked() {
                                    action = Some(UiAction::SwitchAdd { index: it.index, location: it.location & 0x3ff });
                                }
                            }
                        });
                        if ui.button("Swap").on_hover_text("Wear this set (what you wear now goes into it)").clicked() {
                            action = Some(UiAction::SwitchAll);
                        }
                    });
                }
                ui.label(RichText::new("Double-click to take off, right-click for details").size(11.0).color(crate::classic::ink(Color32::GRAY)));
            });
            self.equipment = open;
        }

        let mut held = false;
        if let (true, Some(skin), Some(s)) = (self.stats, v.skin, v.stats) {
            let mut open = true;
            let mut raise = None;
            let mut trait_raise = None;
            let mut hold = self.stat_hold;
            skin.window(
                ctx,
                "Status",
                col(0.0),
                280.0,
                &mut open,
                |ui| {
                    let (area, _) = ui.allocate_exact_size(Vec2::new(280.0, 103.0), egui::Sense::hover());
                    let p = ui.painter().clone();
                    skin.draw(&p, "statwin_bg", area.min);
                    let at = |x: f32, row: usize| egui::pos2(area.min.x + x, area.min.y + 10.0 + row as f32 * 16.0);
                    let small = egui::FontId::proportional(10.5);
                    let text = egui::FontId::proportional(11.5);
                    for i in 0..6 {
                        // Value box: base, and bonuses beside it.
                        let value = if s.plus[i] != 0 { format!("{}{:+}", s.base[i], s.plus[i]) } else { s.base[i].to_string() };
                        p.text(at(73.0, i), egui::Align2::RIGHT_CENTER, value, small.clone(), crate::classic::TEXT);
                        p.text(at(94.0, i), egui::Align2::CENTER_CENTER, s.cost[i].to_string(), small.clone(), crate::classic::TEXT);
                        // The arrow raises; holding repeats after 0.4 s, one point every 50 ms.
                        let can = s.points >= s.cost[i] as u32;
                        let r = egui::Rect::from_center_size(at(80.5, i), Vec2::splat(11.0));
                        let resp = ui.interact(r, ui.scope_id().with(("stat_up", i)), egui::Sense::click());
                        if can {
                            skin.draw(&p, if resp.hovered() { "arw_right_on" } else { "arw_right" }, r.min);
                        }
                        let resp = resp.on_hover_text(format!("Costs {} — hold to keep raising", s.cost[i]));
                        if can && resp.clicked() {
                            raise = Some(i);
                        }
                        if can && resp.is_pointer_button_down_on() {
                            held = true;
                            let before = hold;
                            hold += ui.input(|inp| inp.stable_dt);
                            if hold > 0.4 && ((hold - 0.4) / 0.05) as u32 != ((before - 0.4).max(0.0) / 0.05) as u32 {
                                raise = Some(i);
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                    let aspd = ((2000 - (s.aspd.0 + s.aspd.1) as i32) / 10).to_string();
                    let pairs = [
                        (format!("{} + {}", s.atk.0, s.atk.1), format!("{} + {}", s.def.0, s.def.1)),
                        (format!("{} ~ {}", s.matk.0, s.matk.1), format!("{} + {}", s.mdef.0, s.mdef.1)),
                        (s.hit.to_string(), format!("{} + {}", s.flee.0, s.flee.1)),
                        (s.crit.to_string(), aspd),
                    ];
                    for (row, (l, r)) in pairs.into_iter().enumerate() {
                        p.text(at(190.0, row), egui::Align2::RIGHT_CENTER, l, small.clone(), crate::classic::TEXT);
                        p.text(at(274.0, row), egui::Align2::RIGHT_CENTER, r, small.clone(), crate::classic::TEXT);
                    }
                    p.text(at(274.0, 4), egui::Align2::RIGHT_CENTER, s.points.to_string(), text.clone(), crate::classic::TEXT);
                    if let Some(g) = v.guild_name.filter(|g| !g.is_empty()) {
                        p.text(at(274.0, 5), egui::Align2::RIGHT_CENTER, g, small, crate::classic::TEXT);
                    }
                    if v.fourth_job {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            ui.vertical(|ui| {
                                if let Some(a) = trait_section(ui, s, Some(skin)) {
                                    trait_raise = Some(a);
                                }
                            });
                        });
                        ui.add_space(4.0);
                    }
                },
                |_| {},
            );
            self.stat_hold = hold;
            if let Some(i) = raise {
                action = Some(UiAction::RaiseStat(i));
            }
            if trait_raise.is_some() {
                action = trait_raise;
            }
            self.stats = open;
            if !held {
                self.stat_hold = 0.0;
            }
        } else if self.stats {
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new("Status").open(&mut open).default_pos(col(0.0)).resizable(false), "Status", true, |ui| {
                let Some(s) = v.stats else {
                    ui.label("Waiting for the server…");
                    return;
                };
                ui.columns(2, |cols| {
                    egui::Grid::new("base").num_columns(3).show(&mut cols[0], |ui| {
                        for (i, name) in STAT_NAMES.iter().enumerate() {
                            ui.label(RichText::new(*name).strong());
                            ui.label(if s.plus[i] != 0 { format!("{} + {}", s.base[i], s.plus[i]) } else { s.base[i].to_string() });
                            // The cost arrives as one byte (capped at 255), so past that the server decides.
                            let can = s.points >= s.cost[i] as u32;
                            let plus = ui.add_enabled(can, egui::Button::new("+")).on_hover_text(format!("Costs {} — hold to keep raising", s.cost[i]));
                            if plus.clicked() {
                                action = Some(UiAction::RaiseStat(i));
                            }
                            // Holding the button repeats: after 0.4 s, one point every 50 ms.
                            if can && plus.is_pointer_button_down_on() {
                                held = true;
                                let dt = ui.input(|inp| inp.stable_dt);
                                let before = self.stat_hold;
                                self.stat_hold += dt;
                                if self.stat_hold > 0.4 && ((self.stat_hold - 0.4) / 0.05) as u32 != ((before - 0.4).max(0.0) / 0.05) as u32 {
                                    action = Some(UiAction::RaiseStat(i));
                                }
                                ui.ctx().request_repaint();
                            }
                            ui.end_row();
                        }
                    });
                    egui::Grid::new("derived").num_columns(2).show(&mut cols[1], |ui| {
                        let rows = [
                            ("Atk", format!("{} + {}", s.atk.0, s.atk.1)),
                            ("Matk", format!("{} + {}", s.matk.0, s.matk.1)),
                            ("Def", format!("{} + {}", s.def.0, s.def.1)),
                            ("Mdef", format!("{} + {}", s.mdef.0, s.mdef.1)),
                            ("Hit", s.hit.to_string()),
                            ("Flee", format!("{} + {}", s.flee.0, s.flee.1)),
                            ("Critical", s.crit.to_string()),
                            // The server sends the attack delay (amotion); players read ASPD.
                            ("Aspd", ((2000 - (s.aspd.0 + s.aspd.1) as i32) / 10).to_string()),
                        ];
                        for (k, val) in rows {
                            ui.label(RichText::new(k).color(crate::classic::ink(Color32::GRAY)));
                            ui.label(val);
                            ui.end_row();
                        }
                    });
                });
                ui.separator();
                ui.label(format!("Status points: {}", s.points));
                if v.fourth_job {
                    ui.separator();
                    if let Some(a) = trait_section(ui, s, None) {
                        action = Some(a);
                    }
                }
            });
            self.stats = open;
            if !held {
                self.stat_hold = 0.0;
            }
        }

        if let Some((skill, level, cell, mut text)) = self.skill_text.take() {
            let title = if skill == 125 { "Talkie Box" } else { "Graffiti" };
            let mut done = None;
            crate::classic::show(ctx, egui::Window::new(title).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), title, true, |ui| {
                ui.label("Enter the message.");
                let r = ui.add(egui::TextEdit::singleline(&mut text).char_limit(20).desired_width(220.0));
                r.request_focus();
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                ui.horizontal(|ui| {
                    if (ui.button("OK").clicked() || enter) && !text.is_empty() {
                        done = Some(true);
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        done = Some(false);
                    }
                });
            });
            match done {
                Some(true) => action = Some(UiAction::SkillText { skill, level, cell, text }),
                Some(false) => {}
                None => self.skill_text = Some((skill, level, cell, text)),
            }
        }

        if let Some((father, mother, name)) = self.adopt_ask.clone() {
            let mut answer = None;
            crate::classic::show(ctx, egui::Window::new("Adoption").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), "Adoption", true, |ui| {
                ui.label(format!("{name} and their partner want to adopt you. Become their child?"));
                ui.label(RichText::new("You will become a Baby class and cannot be adopted again.").color(crate::classic::ink(Color32::GRAY)));
                ui.horizontal(|ui| {
                    if ui.button("Accept").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Decline").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(accept) = answer {
                self.adopt_ask = None;
                action = Some(UiAction::Adopt(ro_net::AdoptRequest::Answer { father, mother, accept }));
            }
        }

        if let Some(which) = self.star_ask {
            let body = ["Sun", "Moon", "Stars"].get(which as usize).copied().unwrap_or("Stars");
            let mut answer = None;
            crate::classic::show(ctx, egui::Window::new("Feeling").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), "Feeling", true, |ui| {
                ui.label(format!("Save this map as your Place of the {body}? It cannot be changed later."));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(yes) = answer {
                self.star_ask = None;
                if yes {
                    action = Some(UiAction::StarPlace(which));
                }
            }
        }

        if let Some(item) = self.expand_ask {
            let name = v.db.map_or_else(|| format!("Item #{item}"), |d| d.name(item, true));
            let mut answer = None;
            crate::classic::show(ctx, egui::Window::new("Expand Inventory").collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), "Expand Inventory", true, |ui| {
                ui.label(format!("Use 1 {name} to add 10 inventory slots?"));
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
            if let Some(yes) = answer {
                self.expand_ask = None;
                action = Some(UiAction::ExpandConfirm(yes));
            }
        }

        // Skill and item menus: one choice answers the server.
        if let Some(menu) = self.menu.clone() {
            use ro_net::{Menu, MenuReply};
            let title = match &menu {
                Menu::Items(_) => "Select an Item",
                Menu::Produce(_) | Menu::Make { .. } => "Create",
                Menu::Warp { .. } => "Warp Portal",
                Menu::AutoSpell(_) => "Auto Spell",
                Menu::Identify(_) => "Item Appraisal",
                Menu::Skill { .. } => "Select a Skill",
                Menu::Egg(_) => "Hatch an Egg",
                Menu::Repair(_) => "Weapon Repair",
                Menu::WeaponRefine(_) => "Weapon Refine",
                Menu::Materials(0) => "Change Material",
                Menu::Materials(_) => "Four Spirit Analysis",
                Menu::Cart { .. } => "Cart Decoration",
            };
            let skill_name = |id: u16| v.skill_info.and_then(|i| i.internal(id)).map_or_else(|| format!("Skill {id}"), |n| (v.skill_names)(n));
            let item_name_of = |id: u32| v.db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
            let inv = |index: u16| v.items.iter().find(|it| it.index == index).copied();
            let mut chosen = None;
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new(title).open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), &title, true, |ui| {
                egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                    // An item row: icon (when loaded) and a button with its name.
                    let item_row = |ui: &mut egui::Ui, id: u32, label: String| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                            paint_icon(ui.painter(), rect, v.icons.get(&id), Color32::WHITE);
                            ui.button(label).clicked()
                        })
                        .inner
                    };
                    match &menu {
                        Menu::Items(ids) | Menu::Make { items: ids, .. } => {
                            for &id in ids {
                                if item_row(ui, id, item_name_of(id)) {
                                    chosen = Some(match &menu {
                                        Menu::Make { kind, .. } => MenuReply::Make { kind: *kind, item: id },
                                        _ => MenuReply::Item(id),
                                    });
                                }
                            }
                        }
                        Menu::Produce(list) => {
                            for (id, materials) in list {
                                let extra: Vec<String> = materials.iter().filter(|&&m| m != 0).map(|&m| item_name_of(m)).collect();
                                let label = if extra.is_empty() { item_name_of(*id) } else { format!("{}  ({})", item_name_of(*id), extra.join(", ")) };
                                if item_row(ui, *id, label) {
                                    chosen = Some(MenuReply::Produce { item: *id, materials: *materials });
                                }
                            }
                        }
                        Menu::Identify(list) | Menu::Egg(list) => {
                            for &index in list {
                                let Some(it) = inv(index) else { continue };
                                if item_row(ui, it.id, item_name(v.db, it)) {
                                    chosen = Some(if matches!(menu, Menu::Egg(_)) { MenuReply::Egg(index) } else { MenuReply::Identify(index) });
                                }
                            }
                        }
                        Menu::Repair(list) => {
                            for &(index, id, refine) in list {
                                let label = if refine > 0 { format!("+{refine} {}", item_name_of(id)) } else { item_name_of(id) };
                                if item_row(ui, id, label) {
                                    chosen = Some(MenuReply::Repair { index, item: id, refine });
                                }
                            }
                        }
                        Menu::WeaponRefine(list) => {
                            for &(index, id, refine) in list {
                                let label = if refine > 0 { format!("+{refine} {}", item_name_of(id)) } else { item_name_of(id) };
                                if item_row(ui, id, label) {
                                    chosen = Some(MenuReply::WeaponRefine(index));
                                }
                            }
                        }
                        Menu::Materials(kind) => {
                            ui.label(match kind {
                                0 => "Choose the materials to convert.",
                                1 => "Choose the elemental stones to split (pure to rough).",
                                _ => "Choose the rough stones to purify (ten make one).",
                            });
                            for it in v.items.iter().filter(|it| it.kind == ro_game::items::kind::ETC && it.equipped == 0) {
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::hover());
                                    paint_icon(ui.painter(), rect, v.item_icon(it), Color32::WHITE);
                                    ui.label(format!("{} ({})", item_name(v.db, it), it.amount));
                                    let n = self.materials.entry(it.index).or_default();
                                    ui.add(egui::DragValue::new(n).range(0..=it.amount));
                                });
                            }
                            ui.horizontal(|ui| {
                                if ui.button("OK").clicked() {
                                    let items: Vec<(u16, u16)> = self.materials.iter().filter(|(_, &n)| n > 0).map(|(&i, &n)| (i, n)).collect();
                                    if !items.is_empty() {
                                        chosen = Some(MenuReply::Materials { kind: *kind, items });
                                    }
                                }
                                if ui.button("Cancel").clicked() {
                                    chosen = Some(MenuReply::Materials { kind: *kind, items: vec![] });
                                }
                            });
                        }
                        Menu::Cart { aid, looks } => {
                            ui.label("Choose a look for your cart.");
                            ui.horizontal(|ui| {
                                for &look in looks {
                                    if ui.button(format!("Cart {look}")).clicked() {
                                        chosen = Some(MenuReply::Cart { aid: *aid, look });
                                    }
                                }
                            });
                        }
                        Menu::Warp { skill, maps } => {
                            for map in maps {
                                if ui.button(map.trim_end_matches(".gat")).clicked() {
                                    chosen = Some(MenuReply::Warp { skill: *skill, map: map.clone() });
                                }
                            }
                        }
                        Menu::AutoSpell(skills) => {
                            for &id in skills {
                                if ui.button(skill_name(id as u16)).clicked() {
                                    chosen = Some(MenuReply::AutoSpell(id));
                                }
                            }
                        }
                        Menu::Skill { flag, skills } => {
                            for &id in skills {
                                if ui.button(skill_name(id)).clicked() {
                                    chosen = Some(MenuReply::Skill { flag: *flag, skill: id });
                                }
                            }
                        }
                    }
                });
            });
            if let Some(reply) = chosen {
                action = Some(UiAction::Menu(reply));
                self.menu = None;
                self.materials.clear();
            } else if !open {
                // Closing the portal menu tells the server to drop the cast.
                if let Menu::Warp { skill, .. } = menu {
                    action = Some(UiAction::Menu(MenuReply::Warp { skill, map: "cancel".into() }));
                }
                if let Menu::Materials(kind) = menu {
                    action = Some(UiAction::Menu(MenuReply::Materials { kind, items: vec![] }));
                }
                self.materials.clear();
                self.menu = None;
            }
        }

        // Inserting a card: pick the equipment the server says can take it.
        if let Some((card, equips)) = self.card_pick.clone() {
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new("Insert Card").open(&mut open).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), "Insert Card", true, |ui| {
                let card_item = v.items.iter().find(|it| it.index == card);
                if let Some(c) = card_item {
                    ui.label(RichText::new(item_name(v.db, c)).strong());
                }
                ui.separator();
                if equips.is_empty() {
                    ui.label(RichText::new("No equipment can take this card.").color(crate::classic::ink(Color32::GRAY)));
                }
                for e in &equips {
                    let Some(it) = v.items.iter().find(|it| it.index == *e) else { continue };
                    ui.horizontal(|ui| {
                        item_slot(ui, v, it, 32.0).on_hover_ui(|ui| item_tooltip(ui, v.db, it));
                        if ui.button(item_name(v.db, it)).clicked() {
                            action = Some(UiAction::InsertCard { card, equip: *e });
                            self.card_pick = None;
                        }
                    });
                }
            });
            if !open {
                self.card_pick = None;
            }
        }

        if let (true, Some(skin)) = (self.skills, v.skin) {
            let mut open = true;
            let mut picked = None;
            let width = crate::skilltree::COLUMNS as f32 * 58.0 + 12.0;
            skin.window(
                ctx,
                "Skills",
                egui::pos2(col(0.0).x, 250.0),
                width,
                &mut open,
                |ui| {
                    egui::Frame::new().inner_margin(6).show(ui, |ui| {
                        ui.set_width(width - 12.0);
                        picked = self.skills_body(ui, v);
                    });
                },
                |ui| {
                    ui.label(RichText::new(format!("Skill points: {}", v.skill_points)).size(11.5));
                },
            );
            if picked.is_some() {
                action = picked;
            }
            self.skills = open;
        } else if self.skills {
            let mut open = true;
            crate::classic::show(ctx, egui::Window::new("Skills").open(&mut open).default_pos(egui::pos2(col(0.0).x, 420.0)).resizable(false), "Skills", true, |ui| {
                if let Some(a) = self.skills_body(ui, v) {
                    action = Some(a);
                }
            });
            self.skills = open;
        }
        if let Some(a) = self.drop_to_world(ctx) {
            action = Some(a);
        }
        action
    }

    /// An item dragged out of the windows and released over the world is dropped, as in the
    /// original client; a stack first asks how many.
    fn drop_to_world(&mut self, ctx: &egui::Context) -> Option<UiAction> {
        if let Some(d) = egui::DragAndDrop::payload::<ItemDrag>(ctx).filter(|d| d.loose) {
            if ctx.input(|i| i.pointer.any_released()) && !ctx.is_pointer_over_egui() {
                egui::DragAndDrop::clear_payload(ctx);
                if d.amount <= 1 {
                    return Some(UiAction::Drop { index: d.index, amount: 1 });
                }
                self.drop_asking = Some((d.index, d.amount, d.amount.to_string()));
            }
        }
        let (index, max, typed) = self.drop_asking.as_mut()?;
        let (index, max) = (*index, *max);
        let mut done = None;
        egui::Window::new("Drop how many?").id(egui::Id::unique("drop_amount")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(typed).desired_width(80.0)).request_focus();
                ui.label(format!("/ {max}"));
            });
            let n = typed.trim().parse::<u16>().unwrap_or(0).min(max);
            ui.horizontal(|ui| {
                if ui.add_enabled(n > 0, egui::Button::new("OK")).clicked() || (n > 0 && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    done = Some(Some(n));
                }
                if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    done = Some(None);
                }
            });
        });
        let choice = done?;
        self.drop_asking = None;
        choice.map(|amount| UiAction::Drop { index, amount })
    }
}

/// A skill icon that can be dragged to the shortcut bar; greyed out until learned.
fn skill_icon(ui: &mut egui::Ui, v: &View, id: u16, skill: Option<&Skill>, size: f32) -> egui::Response {
    let learned = skill.is_some_and(|s| s.level > 0);
    let resp = match v.skill_icons.get(&id) {
        Some(t) => ui.add(egui::Image::new((t.id(), Vec2::splat(size))).tint(if learned { Color32::WHITE } else { Color32::from_gray(90) }).sense(egui::Sense::click_and_drag())),
        None => ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click()).1,
    };
    if let Some(s) = skill.filter(|s| s.level > 0 && s.target != 0) {
        resp.dnd_set_drag_payload(Hotkey { skill: true, id: s.id as u32, count: s.level as i16 });
    }
    resp
}

/// Hovering a skill shows its name, numbers and description.
fn skill_tooltip(ui: &mut egui::Ui, v: &View, internal: &str, skill: Option<&Skill>) {
    ui.set_max_width(360.0);
    ui.label(RichText::new((v.skill_names)(internal)).strong());
    match skill {
        Some(s) => ui.label(RichText::new(format!("Lv {}   SP {}   Range {}", s.level, s.sp, s.range)).color(crate::classic::ink(Color32::GRAY))),
        None => ui.label(RichText::new("Not yet available").color(crate::classic::ink(Color32::GRAY))),
    };
    if let Some(lines) = v.skill_info.and_then(|i| i.description(internal)) {
        ui.separator();
        for line in lines {
            ui.label(crate::hud::npc_text(line, 12.5));
        }
    }
    if skill.is_some_and(|s| s.level > 0 && s.target != 0) {
        ui.separator();
        ui.label(RichText::new("Double-click to use, or drag to the shortcut bar").color(crate::classic::ink(Color32::GRAY)).size(11.5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The left button released at `at` while `drag` is being carried, after a frame that lets
    /// egui learn where its windows end (as every frame but the first has).
    fn release(w: &mut Windows, drag: ItemDrag, at: egui::Pos2) -> Option<UiAction> {
        let ctx = egui::Context::default();
        let screen = Some(egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 800.0)));
        let frame = |events| egui::RawInput { events, screen_rect: screen, ..Default::default() };
        ctx.run_ui(frame(vec![egui::Event::PointerMoved(at)]), |_| {}).textures_delta.clear();
        let up = egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() };
        let mut action = None;
        ctx.run_ui(frame(vec![up]), |ui| {
            egui::DragAndDrop::set_payload(ui.ctx(), drag);
            action = w.drop_to_world(ui.ctx());
        })
        .textures_delta
        .clear();
        action
    }

    #[test]
    fn an_item_released_over_the_world_is_dropped_and_a_stack_asks_how_many() {
        let mut w = Windows::default();
        let single = ItemDrag { index: 4, id: 501, amount: 1, shortcut: true, loose: true };
        assert!(matches!(release(&mut w, single, egui::pos2(300.0, 300.0)), Some(UiAction::Drop { index: 4, amount: 1 })));
        let stack = ItemDrag { index: 9, id: 909, amount: 25, shortcut: false, loose: true };
        assert!(release(&mut w, stack, egui::pos2(300.0, 300.0)).is_none());
        assert_eq!(w.drop_asking.as_ref().map(|(i, n, typed)| (*i, *n, typed.as_str())), Some((9, 25, "25")));
    }

    #[test]
    fn worn_items_are_not_dropped() {
        let mut w = Windows::default();
        let worn = ItemDrag { index: 2, id: 1201, amount: 1, shortcut: true, loose: false };
        assert!(release(&mut w, worn, egui::pos2(300.0, 300.0)).is_none());
        assert!(w.drop_asking.is_none());
    }
}
