//! The stylist (script `stylist` / ZC_UI_OPEN type 1): choose a hair style, hair colour and
//! clothes colour with a live preview, then buy; the server checks the price (stylist.yml)
//! and answers ZC_STYLE_CHANGE_RES. Options are sent as the client's indices, which in
//! rAthena's stock stylist.yml equal the look values (hair colour 0 is index -1).

use egui::{Color32, RichText, Vec2};

/// What a stylist order asks for (0: unchanged).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Order {
    pub hair_color: i16,
    pub hair: i16,
    pub clothes_color: i16,
}

#[derive(Default)]
pub struct Stylist {
    pub open: bool,
    /// The dressing room (/dressroom): the same previews, nothing to buy.
    pub dressing: bool,
    hair: u16,
    hair_color: u16,
    clothes_color: u16,
    /// What the character had when the window opened.
    start: (u16, u16, u16),
    preview: Option<(crate::preview::Look, Option<egui::TextureHandle>)>,
    pub status: String,
}

/// The stock 42, and the Samurai's tied-back hair (samurai::HAIRSTYLE).
pub const HAIR_STYLES: u16 = crate::samurai::HAIRSTYLE;
pub const HAIR_COLORS: u16 = 8;
pub const CLOTHES_COLORS: u16 = 7;

impl Stylist {
    pub fn open(&mut self, hair: u16, hair_color: u16, clothes_color: u16) {
        self.open = true;
        self.dressing = false;
        self.start = (hair, hair_color, clothes_color);
        (self.hair, self.hair_color, self.clothes_color) = self.start;
        self.status.clear();
    }

    pub fn result(&mut self, failed: bool) {
        self.status = if failed { "You can't afford that style (or lack the item).".into() } else { "Your new style suits you!".into() };
        if !failed {
            self.start = (self.hair, self.hair_color, self.clothes_color);
        }
    }

    fn order(&self) -> Order {
        let idx = |now: u16, was: u16, zero_is: i16| if now == was { 0 } else if now == 0 { zero_is } else { now as i16 };
        Order { hair: idx(self.hair, self.start.0, 0), hair_color: idx(self.hair_color, self.start.1, -1), clothes_color: idx(self.clothes_color, self.start.2, -1) }
    }

    /// Test hook: order a hair style.
    pub fn test_order(&mut self, hair: u16) -> Order {
        self.hair = hair;
        self.order()
    }

    /// `base`: our look (job, sex, headgear) for the preview. Returns an order or a close.
    pub fn show(&mut self, ctx: &egui::Context, vfs: Option<&ro_vfs::Vfs>, base: Option<crate::preview::Look>) -> Option<Option<Order>> {
        if !self.open {
            return None;
        }
        let mut out = None;
        if let (Some(mut look), Some(vfs)) = (base, vfs) {
            look.hair = self.hair.max(1);
            look.hair_color = self.hair_color;
            if self.preview.as_ref().is_none_or(|p| p.0 != look) {
                let tex = crate::preview::compose(vfs, &look).map(|img| ctx.load_texture("stylist preview", img, egui::TextureOptions::NEAREST));
                self.preview = Some((look, tex));
            }
        }
        let mut open = true;
        let title = if self.dressing { "Dressing Room" } else { "Stylist" };
        crate::classic::show(ctx, egui::Window::new(title).id(egui::Id::unique("stylist")).open(&mut open).resizable(false).default_width(300.0), title, true, |ui| {
            if let Some(t) = self.preview.as_ref().and_then(|p| p.1.as_ref()) {
                ui.vertical_centered(|ui| ui.image((t.id(), Vec2::splat(140.0))));
            }
            let row = |ui: &mut egui::Ui, label: &str, v: &mut u16, min: u16, max: u16| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(label).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(">").clicked() {
                            *v = if *v >= max { min } else { *v + 1 };
                        }
                        ui.label(format!("{v:>2}"));
                        if ui.button("<").clicked() {
                            *v = if *v <= min { max } else { *v - 1 };
                        }
                    });
                });
            };
            row(ui, "Hair style", &mut self.hair, 1, HAIR_STYLES);
            row(ui, "Hair colour", &mut self.hair_color, 0, HAIR_COLORS);
            row(ui, "Clothes colour", &mut self.clothes_color, 0, CLOTHES_COLORS);
            ui.separator();
            let order = self.order();
            ui.horizontal(|ui| {
                if !self.dressing && ui.add_enabled(order != Order::default(), egui::Button::new("Buy")).clicked() {
                    out = Some(Some(order));
                    self.status = "Styling...".into();
                }
                if ui.button("Undo").clicked() {
                    (self.hair, self.hair_color, self.clothes_color) = self.start;
                }
            });
            if !self.dressing { ui.label(RichText::new("Prices are set by the server; each change is paid for separately.").size(11.0).color(crate::classic::ink(Color32::GRAY))); }
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(crate::classic::ink(Color32::from_rgb(255, 210, 110))));
            }
        });
        if !open {
            self.open = false;
            if !self.dressing {
                out = Some(None);
            }
        }
        out
    }
}
