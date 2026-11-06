//! The quest log (Alt+U), the quest tracker and the quest marks over NPCs. Titles and
//! descriptions come from System\OngoingQuestInfoList_True.lub, marks from
//! texture\유저인터페이스\minimap\quest_*.bmp.
//!
//! The tracker (beyond the original): quests pinned with the log's Track box, or on their
//! own when a quest with things to hunt is accepted, listed down the left of the screen with
//! their hunt counts as the server sends them; a count that goes up flashes, a finished
//! objective turns green, and a quest with all of them done says to go back to its NPC.
//! Clicking one opens it in the log. Pins are kept per character in settings.toml.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use egui::{Color32, RichText, Vec2};
use ro_net::Quest;

#[derive(Default)]
pub struct QuestLog {
    pub quests: BTreeMap<u32, Quest>,
    /// Quest id -> (title, summary, description lines).
    info: HashMap<u32, (String, String, Vec<String>)>,
    /// Quest id -> where its NPC stands (map, x, y), for navigation.
    npc: HashMap<u32, (String, u16, u16)>,
    /// The Navigate button was pressed: (map, cell, label), for the navigation window.
    pub navigate: Option<(String, Option<(u16, u16)>, String)>,
    /// NPC id -> (mark, colour).
    pub marks: HashMap<u32, (u16, u16)>,
    mark_icons: HashMap<(u16, u16), Option<egui::TextureHandle>>,
    pub open: bool,
    selected: Option<u32>,
    show_inactive: bool,
    /// The character's pinned quests (the tracker), in the order pinned.
    pub tracked: Vec<u32>,
    /// The pins changed (save them).
    pub tracked_changed: bool,
    /// When each objective's count last went up (hunt id), for the tracker's flash.
    advanced: HashMap<u32, Instant>,
}

/// How long a count that went up stays lit in the tracker.
const FLASH: Duration = Duration::from_millis(2500);
/// Quests the tracker lists at most.
const TRACKED_SHOWN: usize = 6;
/// The tracker's egui area (hudlayout places it by this name).
pub const TRACKER_AREA: &str = "quest tracker";

/// Whether a quest has things to hunt and all of them are done.
pub fn complete(q: &Quest) -> bool {
    !q.objectives.is_empty() && q.objectives.iter().all(|o| o.total > 0 && o.count >= o.total)
}

/// An objective's line: "Poring  3 / 10" (no total yet: just the count).
pub fn objective_text(o: &ro_net::Objective) -> String {
    if o.total > 0 {
        format!("{}  {} / {}", o.name, o.count.min(o.total), o.total)
    } else {
        format!("{}  {}", o.name, o.count)
    }
}

impl QuestLog {
    /// Back at character select: the character's quests go (the quest text stays loaded).
    pub fn leave_character(&mut self) {
        self.quests.clear();
        self.marks.clear();
        self.navigate = None;
        self.selected = None;
        self.open = false;
        self.tracked.clear();
        self.advanced.clear();
    }

    /// Pins a quest to the tracker (or unpins it).
    pub fn set_tracked(&mut self, id: u32, on: bool) {
        let has = self.tracked.contains(&id);
        if on && !has {
            self.tracked.push(id);
        } else if !on && has {
            self.tracked.retain(|&t| t != id);
        } else {
            return;
        }
        self.tracked_changed = true;
    }

    /// A new quest: pinned when it has things to hunt.
    pub fn accepted(&mut self, q: Quest) {
        if !q.objectives.is_empty() {
            self.set_tracked(q.id, true);
        }
        self.quests.insert(q.id, q);
    }

    /// The full quest list arrived: pins of quests the character no longer has go.
    pub fn listed(&mut self, list: Vec<Quest>) {
        self.quests = list.into_iter().map(|q| (q.id, q)).collect();
        let before = self.tracked.len();
        let quests = &self.quests;
        self.tracked.retain(|id| quests.contains_key(id));
        self.tracked_changed |= self.tracked.len() != before;
    }

    pub fn load(vfs: &ro_vfs::Vfs) -> Self {
        let mut q = Self::default();
        // Translation packs name it after kRO's file; prefer theirs when present.
        let chunk = [r"System\OngoingQuests.lub", r"System\OngoingQuestInfoList_True.lub"].iter().find_map(|p| vfs.read(p).ok().flatten()).and_then(|d| ro_formats::lub::eval(&d).ok());
        if chunk.is_none() {
            tracing::warn!("OngoingQuestInfoList_True.lub not found; quests show by id");
        }
        if let Some(c) = &chunk {
        for (_, table) in c.globals.iter().filter(|(n, _)| *n == "QuestInfoList") {
            for (id, entry) in c.entries(table) {
                let Some(id) = id.as_number() else { continue };
                let s = |k: &str| c.field(entry, k).and_then(|v| v.as_str()).unwrap_or_default().to_string();
                let desc = c.field(entry, "Description").map(|d| c.entries(d).iter().filter_map(|(_, l)| l.as_str().map(str::to_string)).collect()).unwrap_or_default();
                q.info.insert(id as u32, (s("Title"), s("Summary"), desc));
                let num = |k: &str| c.field(entry, k).and_then(|v| v.as_number()).unwrap_or(0.0) as u16;
                if let Some(map) = c.field(entry, "NpcNavi").and_then(|v| v.as_str()).filter(|m| !m.is_empty()) {
                    q.npc.insert(id as u32, (map.to_string(), num("NpcPosX"), num("NpcPosY")));
                }
            }
        }
        }
        for p in crate::customdata::QUESTINFO_SOURCES {
            if let Some(src) = vfs.read(p).ok().flatten() {
                for (id, t) in crate::customdata::questinfo(&src, p) {
                    q.info.insert(id, (t.title, t.summary, t.description));
                    match t.npc {
                        Some(npc) => q.npc.insert(id, npc),
                        None => q.npc.remove(&id),
                    };
                }
            }
        }
        tracing::info!(quests = q.info.len(), "quest descriptions");
        q
    }

    pub fn title(&self, id: u32) -> String {
        self.info.get(&id).map(|i| i.0.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Quest {id}"))
    }

    /// Hunting progress from the server.
    pub fn progress(&mut self, updates: &[(u32, u32, u16, u16)]) {
        for &(quest, hunt, total, count) in updates {
            if let Some(o) = self.quests.get_mut(&quest).and_then(|q| q.objectives.iter_mut().find(|o| o.hunt_id == hunt)) {
                if count > o.count {
                    self.advanced.insert(hunt, Instant::now());
                }
                o.count = count;
                o.total = total;
            }
        }
    }

    pub fn mark_icon(&mut self, ctx: &egui::Context, vfs: &ro_vfs::Vfs, mark: u16, color: u16) -> Option<egui::TextureHandle> {
        self.mark_icons
            .entry((mark, color))
            .or_insert_with(|| crate::load_texture(ctx, vfs, &format!(r"data\texture\유저인터페이스\minimap\quest_{mark}_{}.bmp", color.max(1))))
            .clone()
    }

    /// The log window. Returns a quest to show or hide in the active list. `map_name` gives a
    /// map's display name for the quest's location.
    pub fn show(&mut self, ctx: &egui::Context, map_name: &dyn Fn(&str) -> Option<String>) -> Option<(u32, bool)> {
        if !self.open {
            return None;
        }
        let mut toggle = None;
        let mut pin = None;
        let mut open = true;
        crate::classic::show(ctx, egui::Window::new("Quests").open(&mut open).default_size(Vec2::new(520.0, 360.0)), "Quests", true, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.show_inactive, false, "Active");
                ui.selectable_value(&mut self.show_inactive, true, "Inactive");
            });
            ui.separator();
            let list: Vec<(u32, bool)> = self.quests.values().filter(|q| q.active != self.show_inactive).map(|q| (q.id, q.active)).collect();
            if list.is_empty() {
                ui.label(RichText::new("No quests here.").color(crate::classic::ink(Color32::GRAY)));
                return;
            }
            ui.columns(2, |cols| {
                egui::ScrollArea::vertical().id_salt("quest_list").max_height(300.0).show(&mut cols[0], |ui| {
                    for (id, _) in &list {
                        if ui.selectable_label(self.selected == Some(*id), self.title(*id)).clicked() {
                            self.selected = Some(*id);
                        }
                    }
                });
                let ui = &mut cols[1];
                let Some(q) = self.selected.and_then(|id| self.quests.get(&id)) else {
                    ui.label(RichText::new("Select a quest.").color(crate::classic::ink(Color32::GRAY)));
                    return;
                };
                ui.label(RichText::new(self.title(q.id)).strong().size(15.0));
                if let Some((_, summary, desc)) = self.info.get(&q.id) {
                    if !summary.is_empty() {
                        ui.label(crate::hud::npc_text(summary, 12.5));
                    }
                    egui::ScrollArea::vertical().id_salt("quest_desc").max_height(160.0).show(ui, |ui| {
                        for line in desc {
                            ui.label(crate::hud::npc_text(line, 12.5));
                        }
                    });
                }
                if !q.objectives.is_empty() {
                    ui.separator();
                    for o in &q.objectives {
                        let done = o.total > 0 && o.count >= o.total;
                        ui.label(RichText::new(objective_text(o)).color(crate::classic::ink(if done { Color32::LIGHT_GREEN } else { Color32::WHITE })));
                    }
                }
                // Where its NPC stands, as the original's quest window shows it, with navigation.
                if let Some((map, x, y)) = self.npc.get(&q.id).cloned() {
                    ui.separator();
                    let place = map_name(&map).unwrap_or_else(|| map.clone());
                    let at = if x > 0 && y > 0 { format!("{place} ({x}, {y})") } else { place };
                    let title = self.title(q.id);
                    let mut go = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Location: {at}")).color(crate::classic::ink(Color32::from_rgb(150, 200, 255))));
                        go = ui.button("Navigate").on_hover_text("Show the way there").clicked();
                    });
                    if go {
                        self.navigate = Some((map, (x > 0 && y > 0).then_some((x, y)), title));
                    }
                }
                if q.end > 0 {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
                    let left = q.end as i64 - now;
                    ui.label(RichText::new(if left > 0 { format!("Time left: {}h {}m", left / 3600, left % 3600 / 60) } else { "Expired".into() }).color(crate::classic::ink(Color32::LIGHT_BLUE)));
                }
                ui.separator();
                let id = q.id;
                let label = if q.active { "Move to Inactive" } else { "Move to Active" };
                let mut track = self.tracked.contains(&id);
                ui.horizontal(|ui| {
                    if ui.button(label).clicked() {
                        toggle = Some((id, !q.active));
                    }
                    if ui.checkbox(&mut track, "Track").on_hover_text("Show it in the quest tracker, down the left of the screen").changed() {
                        pin = Some((id, track));
                    }
                });
            });
        });
        self.open = open;
        if let Some((id, on)) = pin {
            self.set_tracked(id, on);
        }
        toggle
    }

    /// Opens the log on a quest (a click in the tracker).
    pub fn open_at(&mut self, id: u32) {
        self.open = true;
        self.show_inactive = self.quests.get(&id).is_some_and(|q| !q.active);
        self.selected = Some(id);
    }

    /// The tracker down the left of the screen, under the character status. Returns a quest
    /// clicked (to open in the log). `map_name` gives a map's display name.
    pub fn tracker(&mut self, ctx: &egui::Context, map_name: &dyn Fn(&str) -> Option<String>) -> Option<u32> {
        let now = Instant::now();
        self.advanced.retain(|_, t| now.duration_since(*t) < FLASH);
        let shown: Vec<u32> = self.tracked.iter().copied().filter(|id| self.quests.contains_key(id)).take(TRACKED_SHOWN).collect();
        if shown.is_empty() {
            return None;
        }
        let mut clicked = None;
        egui::Area::new(egui::Id::unique(TRACKER_AREA)).anchor(egui::Align2::LEFT_TOP, Vec2::new(12.0, 196.0)).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_rgba_unmultiplied(12, 14, 20, 170)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(10, 8)).show(ui, |ui| {
                ui.set_width(230.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Quests").size(11.0).color(Color32::from_rgb(170, 180, 200)));
                for (n, id) in shown.iter().enumerate() {
                    let Some(q) = self.quests.get(id) else { continue };
                    if n > 0 {
                        ui.add_space(5.0);
                    }
                    let done = complete(q);
                    let title = RichText::new(self.title(*id)).strong().size(12.5).color(if done { Color32::from_rgb(140, 230, 140) } else { Color32::from_rgb(255, 225, 150) });
                    if ui.add(egui::Label::new(title).sense(egui::Sense::click())).on_hover_text("Open in the quest log").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        clicked = Some(*id);
                    }
                    for o in &q.objectives {
                        let finished = o.total > 0 && o.count >= o.total;
                        let lit = self.advanced.get(&o.hunt_id).map(|t| 1.0 - now.duration_since(*t).as_secs_f32() / FLASH.as_secs_f32());
                        let color = match (finished, lit) {
                            (true, _) => Color32::from_rgb(140, 230, 140),
                            (false, Some(k)) => Color32::from_rgb(255, (220.0 + 35.0 * k) as u8, (220.0 - 160.0 * k) as u8),
                            _ => Color32::from_rgb(220, 220, 220),
                        };
                        // (Green says done: the game's fonts have no tick.)
                        ui.label(RichText::new(format!("  • {}", objective_text(o))).size(11.5).color(color));
                    }
                    if done {
                        let place = self.npc.get(id).map(|(map, x, y)| {
                            let name = map_name(map).unwrap_or_else(|| map.clone());
                            if *x > 0 && *y > 0 { format!("{name} ({x}, {y})") } else { name }
                        });
                        let text = place.map_or_else(|| "  Done: report back".to_string(), |p| format!("  Done: report back in {p}"));
                        ui.label(RichText::new(text).size(11.0).color(Color32::from_rgb(150, 200, 255)));
                    }
                    if q.end > 0 {
                        let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
                        let left = q.end as i64 - unix;
                        let text = if left > 0 { format!("  {}h {}m left", left / 3600, left % 3600 / 60) } else { "  Expired".into() };
                        ui.label(RichText::new(text).size(11.0).color(Color32::from_rgb(170, 190, 230)));
                    }
                }
            });
        });
        if !self.advanced.is_empty() {
            ctx.request_repaint();
        }
        clicked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quest(id: u32, counts: &[(u16, u16)]) -> Quest {
        let objectives = counts.iter().enumerate().map(|(i, &(count, total))| ro_net::Objective { hunt_id: id * 1000 + i as u32, mob_id: 1002, count, total, name: "Poring".into() }).collect();
        Quest { id, active: true, end: 0, objectives }
    }

    #[test]
    fn hunting_quests_pin_themselves_and_drop_when_gone() {
        let mut log = QuestLog::default();
        log.accepted(quest(7, &[(0, 10)]));
        log.accepted(quest(8, &[]));
        assert_eq!(log.tracked, vec![7]);
        assert!(std::mem::take(&mut log.tracked_changed));
        log.set_tracked(8, true);
        log.listed(vec![quest(8, &[])]);
        assert_eq!(log.tracked, vec![8]);
        assert!(log.tracked_changed);
    }

    #[test]
    fn counts_flash_when_they_go_up_and_complete_when_all_are_done() {
        let mut log = QuestLog::default();
        log.accepted(quest(7, &[(0, 2), (5, 5)]));
        assert!(!complete(&log.quests[&7]));
        log.progress(&[(7, 7000, 2, 1)]);
        assert!(log.advanced.contains_key(&7000));
        log.progress(&[(7, 7000, 2, 2)]);
        assert!(complete(&log.quests[&7]));
        assert_eq!(objective_text(&log.quests[&7].objectives[0]), "Poring  2 / 2");
    }
}
