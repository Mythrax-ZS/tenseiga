//! Selectable item packages (CZ_USE_PACKAGEITEM): a box whose contents the player picks from
//! a few sets, chosen in a window when it is used, as the original does. The sets come from
//! the client table selectpackage\selectpackageitem.lub: `packageitemsetbox` rows
//! `{ box item, set, item, amount }` and `packageitemboxName` rows `{ box item, set, name }`.
//! rAthena's item_packages.yml numbers its groups the same way.

use std::collections::{BTreeMap, HashMap};

use egui::{RichText, Vec2};
use ro_formats::lub::Chunk;
use ro_game::items::ItemDb;

const TABLE: &str = r"data\luafiles514\lua files\selectpackage\selectpackageitem.lub";

#[derive(Debug, Default, PartialEq)]
pub struct Set {
    pub name: String,
    /// (item id, amount)
    pub items: Vec<(u32, u32)>,
}

#[derive(Default)]
pub struct Packages {
    loaded: bool,
    /// box item -> set index -> set
    sets: HashMap<u32, BTreeMap<u32, Set>>,
    /// The box being opened: inventory index, item id, and the set picked.
    open: Option<(u16, u32, u32)>,
}

/// Adds a table's boxes, keeping those already read (a higher-priority data layer's).
fn read(c: &Chunk, into: &mut HashMap<u32, BTreeMap<u32, Set>>) {
    let rows = |name: &str| c.globals.get(name).map(|t| c.entries(t)).unwrap_or_default();
    let num = |row: &ro_formats::lub::Value, i| c.field_index(row, i).and_then(|v| v.as_number()).map(|n| n as u32);
    let mut found: HashMap<u32, BTreeMap<u32, Set>> = HashMap::new();
    for (_, row) in rows("packageitemsetbox") {
        if let (Some(b), Some(set), Some(item), Some(amount)) = (num(row, 1), num(row, 2), num(row, 3), num(row, 4)) {
            found.entry(b).or_default().entry(set).or_default().items.push((item, amount.max(1)));
        }
    }
    for (_, row) in rows("packageitemboxName") {
        let name = c.field_index(row, 3).and_then(|v| v.as_str());
        if let (Some(b), Some(set), Some(name)) = (num(row, 1), num(row, 2), name) {
            if let Some(s) = found.get_mut(&b).and_then(|sets| sets.get_mut(&set)) {
                s.name = name.to_string();
            }
        }
    }
    for (b, sets) in found {
        into.entry(b).or_insert(sets);
    }
}

impl Packages {
    /// Reads the table on first use, from every data layer (iRO first, then kRO's newer boxes).
    pub fn ensure_loaded(&mut self, vfs: Option<&ro_vfs::Vfs>) {
        if self.loaded {
            return;
        }
        let Some(vfs) = vfs else { return };
        self.loaded = true;
        for data in vfs.read_all(TABLE).unwrap_or_default() {
            match ro_formats::lub::eval(&data) {
                Ok(c) => read(&c, &mut self.sets),
                Err(e) => tracing::warn!("selectpackageitem: {e:#}"),
            }
        }
        tracing::info!(boxes = self.sets.len(), "select packages");
    }

    /// The item ids in a box's sets, when it is a selectable package.
    pub fn contents(&self, item: u32) -> Option<Vec<u32>> {
        self.sets.get(&item).map(|sets| sets.values().flat_map(|s| s.items.iter().map(|i| i.0)).collect())
    }

    /// Opens the window for a box being used.
    pub fn open(&mut self, index: u16, item: u32) {
        let first = self.sets.get(&item).and_then(|s| s.keys().next().copied()).unwrap_or(0);
        self.open = Some((index, item, first));
    }

    /// Returns (inventory index, box item, set) once a set is chosen.
    pub fn show(&mut self, ctx: &egui::Context, db: Option<&ItemDb>, icons: &HashMap<u32, egui::TextureHandle>) -> Option<(u16, u32, u32)> {
        let (index, item, mut pick) = self.open?;
        let sets = self.sets.get(&item)?;
        let name = |id: u32| db.map_or_else(|| format!("Item #{id}"), |d| d.name(id, true));
        let title = name(item);
        let mut open = true;
        let mut chosen = None;
        let mut cancel = false;
        crate::classic::show(ctx, egui::Window::new(&title).id(egui::Id::unique("select package")).open(&mut open).resizable(false).collapsible(false).anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO), &title, true, |ui| {
            ui.label("Choose one set:");
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, set) in sets {
                    let label = if set.name.is_empty() { format!("Set {}", i + 1) } else { set.name.clone() };
                    ui.radio_value(&mut pick, *i, RichText::new(label).strong());
                    ui.indent(("set", *i), |ui| {
                        for &(id, amount) in &set.items {
                            ui.horizontal(|ui| {
                                if let Some(t) = icons.get(&id) {
                                    ui.image((t.id(), Vec2::splat(22.0)));
                                }
                                ui.label(if amount > 1 { format!("{} x{amount}", name(id)) } else { name(id) });
                            });
                        }
                    });
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("OK").clicked() {
                    chosen = Some((index, item, pick));
                }
                cancel = ui.button("Cancel").clicked();
            });
        });
        self.open = (open && !cancel && chosen.is_none()).then_some((index, item, pick));
        chosen
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sets_come_from_both_tables() {
        let src = br#"
            packageitemsetbox = { { 101060, 0, 2301, 1 }, { 101060, 0, 2403, 1 }, { 101060, 1, 501, 5 } }
            packageitemboxName = { { 101060, 0, "Equipment Sets" }, { 101060, 1, "Potions" } }
        "#;
        let c = ro_formats::lub::eval(src).unwrap();
        let mut sets = Default::default();
        super::read(&c, &mut sets);
        let b = &sets[&101060];
        assert_eq!((b[&0].name.as_str(), b[&0].items.as_slice()), ("Equipment Sets", &[(2301, 1), (2403, 1)][..]));
        assert_eq!((b[&1].name.as_str(), b[&1].items.as_slice()), ("Potions", &[(501, 5)][..]));
    }
}
