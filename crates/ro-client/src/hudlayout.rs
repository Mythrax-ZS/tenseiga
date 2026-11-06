//! Arranging the interface: each HUD element (character status, chat, shortcut bar, menu,
//! minimap, status effects, controller bar, performance overlay) can be moved and resized,
//! so the layout suits a phone as well as a monitor. Settings > Game > Interface > Arrange
//! opens the editor; the layout is kept in settings.toml.
//!
//! Each element is an egui layer, and its placement becomes that layer's transform (egui maps
//! input through it too), so the HUD code lays itself out as always.

use egui::emath::TSTransform;
use egui::{Align2, Color32, Id, LayerId, Order, Rect, Vec2};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where an element sits relative to its own spot (points) and how big it is.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(default)]
pub struct Place {
    pub x: f32,
    pub y: f32,
    pub scale: f32,
}

impl Default for Place {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0, scale: 1.0 }
    }
}

/// Placements by element key; elements left out sit where they always do.
pub type Layout = BTreeMap<String, Place>;

const MIN_SCALE: f32 = 0.5;
const MAX_SCALE: f32 = 2.5;

struct Element {
    key: &'static str,
    name: &'static str,
    /// The layers that draw it (the classic and modern status are different ones).
    layers: &'static [Layer],
    /// The point it grows and shrinks around: the corner or edge it is anchored to.
    pivot: Align2,
}

/// An egui area, or a window (whose area egui names after its title).
#[derive(Clone, Copy)]
enum Layer {
    Area(Order, &'static str),
    Window(&'static str),
}

impl Layer {
    fn id(self) -> LayerId {
        match self {
            Layer::Area(order, name) => LayerId::new(order, Id::unique(name)),
            Layer::Window(title) => LayerId::new(Order::Middle, Id::unique(Some(title))),
        }
    }
}

const ELEMENTS: [Element; 10] = [
    Element { key: "status", name: "Character", layers: &[Layer::Area(Order::Middle, "basic_info"), Layer::Window("status")], pivot: Align2::LEFT_TOP },
    Element { key: "chat", name: "Chat", layers: &[Layer::Window("chat")], pivot: Align2::LEFT_BOTTOM },
    Element { key: "hotkeys", name: "Shortcut bar", layers: &[Layer::Area(Order::Middle, "hotkeys")], pivot: Align2::CENTER_BOTTOM },
    Element { key: "menu", name: "Menu", layers: &[Layer::Area(Order::Middle, "menu")], pivot: Align2::RIGHT_TOP },
    Element { key: "minimap", name: "Minimap", layers: &[Layer::Area(Order::Middle, "minimap")], pivot: Align2::RIGHT_TOP },
    Element { key: "buffs", name: "Status effects", layers: &[Layer::Area(Order::Middle, "buffs")], pivot: Align2::RIGHT_TOP },
    Element { key: "cross bar", name: "Controller bar", layers: &[Layer::Area(Order::Middle, "cross bar")], pivot: Align2::CENTER_BOTTOM },
    Element { key: "quests", name: "Quest tracker", layers: &[Layer::Area(Order::Middle, crate::quests::TRACKER_AREA)], pivot: Align2::LEFT_TOP },
    Element { key: "target", name: "Target", layers: &[Layer::Area(Order::Middle, crate::targetframe::AREA)], pivot: Align2::CENTER_TOP },
    Element { key: "perf", name: "Performance", layers: &[Layer::Area(Order::Foreground, "perf overlay")], pivot: Align2::CENTER_TOP },
];

/// The transform that puts `rect` (laid out at its usual spot) at `place`, nudging `place` so
/// the element stays on `screen` (a layout made on a bigger screen, or a drag past the edge).
/// One larger than the screen (a passing measurement) is left where it is.
fn fit(rect: Rect, pivot: Align2, place: &mut Place, screen: Rect) -> TSTransform {
    place.scale = place.scale.clamp(MIN_SCALE, MAX_SCALE);
    let at = pivot.pos_in_rect(&rect).to_vec2();
    let transform = |p: &Place| TSTransform::new(at * (1.0 - p.scale) + Vec2::new(p.x, p.y), p.scale);
    let shown = transform(place).mul_rect(rect);
    let shift = |lo: f32, hi: f32, min: f32, max: f32| {
        if hi - lo > max - min {
            0.0
        } else if lo < min {
            min - lo
        } else if hi > max {
            max - hi
        } else {
            0.0
        }
    };
    place.x += shift(shown.left(), shown.right(), screen.left(), screen.right());
    place.y += shift(shown.top(), shown.bottom(), screen.top(), screen.bottom());
    transform(place)
}

/// The element's layer on screen now, and where it lays itself out.
fn shown_layer(ctx: &egui::Context, e: &Element) -> Option<(LayerId, Rect)> {
    e.layers.iter().find_map(|l| {
        let layer = l.id();
        ctx.memory(|m| m.areas().is_visible(&layer).then(|| m.area_rect(layer.id)).flatten()).map(|rect| (layer, rect))
    })
}

/// Places this frame's HUD layers by the layout (after the HUD is drawn).
pub fn apply(ctx: &egui::Context, layout: &Layout) {
    let screen = ctx.content_rect();
    for e in &ELEMENTS {
        let place = layout.get(e.key).copied().unwrap_or_default();
        for l in e.layers {
            let layer = l.id();
            let transform = match ctx.memory(|m| m.area_rect(layer.id)) {
                Some(rect) if place != Place::default() => fit(rect, e.pivot, &mut place.clone(), screen),
                _ => TSTransform::IDENTITY,
            };
            ctx.set_transform_layer(layer, transform);
        }
    }
}

/// The arranging mode: a frame over each element to drag, and a panel to resize and reset.
pub struct Editor {
    pub open: bool,
    selected: Option<&'static str>,
    /// Two-finger pinch since the last frame (a ratio), for the selected element.
    pub pinch: f32,
}

impl Default for Editor {
    fn default() -> Self {
        Self { open: false, selected: None, pinch: 1.0 }
    }
}

impl Editor {
    pub fn opened(open: bool) -> Self {
        Self { open, ..Self::default() }
    }

    /// Draws the editor over the HUD. True when the layout should be saved.
    pub fn show(&mut self, ctx: &egui::Context, layout: &mut Layout) -> bool {
        let pinch = std::mem::replace(&mut self.pinch, 1.0);
        if !self.open {
            return false;
        }
        let screen = ctx.content_rect();
        let mut save = false;
        for e in &ELEMENTS {
            let Some((_, rect)) = shown_layer(ctx, e) else { continue };
            let place = layout.entry(e.key.to_string()).or_default();
            let selected = self.selected == Some(e.key);
            if selected && pinch != 1.0 {
                place.scale *= pinch;
            }
            // Kept on screen for showing; the place itself only takes that on a drag (a passing
            // measurement must not move it for good).
            let mut on_screen = *place;
            let shown = fit(rect, e.pivot, &mut on_screen, screen).mul_rect(rect);
            let resp = egui::Area::new(Id::unique(("arrange", e.key)))
                .order(Order::Foreground)
                .fixed_pos(shown.min)
                .show(ctx, |ui| {
                    let (r, resp) = ui.allocate_exact_size(shown.size(), egui::Sense::click_and_drag());
                    let p = ui.painter();
                    p.rect_filled(r, 4.0, Color32::from_rgba_unmultiplied(70, 130, 255, if selected { 80 } else { 40 }));
                    p.rect_stroke(r, 4.0, egui::Stroke::new(if selected { 2.0 } else { 1.0 }, Color32::from_rgb(130, 180, 255)), egui::StrokeKind::Inside);
                    for (dx, color) in [(1.0, Color32::BLACK), (0.0, Color32::WHITE)] {
                        p.text(r.center() + Vec2::splat(dx), Align2::CENTER_CENTER, e.name, egui::FontId::proportional(15.0), color);
                    }
                    resp
                })
                .inner;
            if resp.drag_started() || resp.clicked() {
                self.selected = Some(e.key);
            }
            if resp.dragged() {
                *place = on_screen;
                place.x += resp.drag_delta().x;
                place.y += resp.drag_delta().y;
            }
            save |= resp.drag_stopped();
            // The mouse wheel resizes the element under the pointer.
            if resp.hovered() {
                let wheel = ctx.input(|i| i.smooth_scroll_delta.y);
                if wheel != 0.0 {
                    place.scale = (place.scale * (1.0 + wheel * 0.002)).clamp(MIN_SCALE, MAX_SCALE);
                    self.selected = Some(e.key);
                }
            }
        }
        // Elements left where they always are stay out of the file.
        layout.retain(|_, p| *p != Place::default());

        egui::Area::new(Id::unique("arrange panel"))
            .order(Order::Tooltip)
            .default_pos(screen.center() - Vec2::new(150.0, 60.0))
            .movable(true)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).inner_margin(10).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.strong("Arrange interface");
                    ui.label("Drag an element to move it. Select one to resize it: the buttons, the mouse wheel, or a two-finger pinch.");
                    ui.separator();
                    match self.selected.and_then(|key| ELEMENTS.iter().find(|e| e.key == key)) {
                        Some(e) => {
                            let place = layout.entry(e.key.to_string()).or_default();
                            ui.horizontal(|ui| {
                                ui.label(e.name);
                                if ui.button("−").clicked() {
                                    place.scale = (place.scale - 0.1).max(MIN_SCALE);
                                    save = true;
                                }
                                ui.label(format!("{:.0}%", place.scale * 100.0));
                                if ui.button("+").clicked() {
                                    place.scale = (place.scale + 0.1).min(MAX_SCALE);
                                    save = true;
                                }
                                if ui.button("Reset").clicked() {
                                    *place = Place::default();
                                    save = true;
                                }
                            });
                        }
                        None => {
                            ui.weak("Nothing selected");
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Reset all").clicked() {
                            layout.clear();
                            save = true;
                        }
                        if ui.button("Done").clicked() {
                            self.open = false;
                            self.selected = None;
                            save = true;
                        }
                    });
                });
            });
        layout.retain(|_, p| *p != Place::default());
        save
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_around_the_pivot_and_stays_on_screen() {
        let screen = Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1000.0, 600.0));
        let rect = Rect::from_min_size(egui::pos2(12.0, 12.0), Vec2::new(200.0, 100.0));
        // Doubled around its top-left corner: that corner stays put.
        let mut p = Place { x: 0.0, y: 0.0, scale: 2.0 };
        let shown = fit(rect, Align2::LEFT_TOP, &mut p, screen).mul_rect(rect);
        assert_eq!(shown.min, rect.min);
        assert_eq!(shown.size(), rect.size() * 2.0);
        // Dragged far off the right edge: pulled back, and the place remembers it.
        let mut p = Place { x: 5000.0, y: 0.0, scale: 1.0 };
        let shown = fit(rect, Align2::LEFT_TOP, &mut p, screen).mul_rect(rect);
        assert_eq!(shown.right(), screen.right());
        assert_eq!(p.x, screen.right() - rect.right());
    }
    #[test]
    fn places_windows_and_areas() {
        // The chat is an egui window and the minimap an area: both must take their placement.
        let ctx = egui::Context::default();
        let layout: Layout = [("chat".to_string(), Place { x: 0.0, y: 0.0, scale: 0.75 }), ("minimap".to_string(), Place { x: -100.0, y: 0.0, scale: 1.5 })].into();
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1600.0, 900.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                egui::Window::new("chat").title_bar(false).anchor(Align2::LEFT_BOTTOM, Vec2::new(12.0, -12.0)).show(&ctx, |ui| ui.label("chat"));
                egui::Area::new(Id::unique("minimap")).anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 52.0)).show(&ctx, |ui| ui.label("map"));
                apply(&ctx, &layout);
            });
            out.textures_delta.clear();
        }
        let scale = |l: Layer| ctx.layer_transform_to_global(l.id()).map(|t| t.scaling);
        assert_eq!(scale(Layer::Window("chat")), Some(0.75));
        assert_eq!(scale(Layer::Area(Order::Middle, "minimap")), Some(1.5));
    }
}

