//! The target frame: a panel at the top of the screen for the monster we are fighting (the
//! one we attacked or cast on last, else the one hitting us, or the controller's target):
//! its name, the line from mobinfo (level, element, race, size), its HP once the server has
//! told us, and what it is casting. It lets go once the fight with it has been quiet for
//! `KEEP`, or it dies or leaves. Settings > Game > Interface > Monsters; Arrange moves it.

use std::time::{Duration, Instant};

use egui::{Align2, Color32, FontId, Rect, Stroke, Vec2};

/// How long the frame stays on a monster after the last blow either way.
pub const KEEP: Duration = Duration::from_secs(12);
/// The frame's egui area (hudlayout places it by this name).
pub const AREA: &str = "target frame";
const WIDTH: f32 = 290.0;

/// What the frame shows.
pub struct Target {
    pub name: String,
    /// The monster info line and its colour (mobinfo::line).
    pub info: Option<(String, Color32)>,
    /// 0 normal, 1 boss, 2 MVP (mobinfo).
    pub rank: u8,
    pub hp: Option<(i32, i32)>,
    /// What it is casting and how far along (0..1).
    pub cast: Option<(String, f32)>,
}

impl Target {
    /// The stand-in shown while arranging the interface with no target.
    pub fn sample() -> Self {
        Target { name: "Poring".into(), info: Some(("Lv 1 · Water 1 · Plant · Medium".into(), Color32::from_rgb(130, 195, 255))), rank: 0, hp: Some((40, 55)), cast: None }
    }
}

/// The HP bar's text: "1,234 / 5,000 · 25%", or a dash before the server has said.
pub fn hp_text(hp: Option<(i32, i32)>) -> String {
    match hp {
        Some((hp, max)) if max > 0 => format!("{} / {} · {}%", group(hp.max(0)), group(max), ((hp.max(0) as f32 / max as f32) * 100.0).ceil() as i32),
        _ => "HP —".into(),
    }
}

fn group(n: i32) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Whether to keep showing a monster we last traded blows with at `last`.
pub fn keeps(last: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last) <= KEEP
}

pub fn show(ctx: &egui::Context, t: &Target) {
    egui::Area::new(egui::Id::unique(AREA)).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 46.0)).interactable(false).show(ctx, |ui| {
        let lines = 1.0 + t.info.is_some() as u8 as f32;
        let height = 10.0 + lines * 17.0 + 18.0 + if t.cast.is_some() { 18.0 } else { 0.0 } + 8.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(WIDTH, height), egui::Sense::hover());
        let p = ui.painter();
        let (edge, name_color) = match t.rank {
            2 => (Color32::from_rgb(230, 90, 70), Color32::from_rgb(255, 150, 120)),
            1 => (Color32::from_rgb(220, 180, 80), Color32::from_rgb(255, 220, 130)),
            _ => (Color32::from_rgba_unmultiplied(255, 255, 255, 40), Color32::WHITE),
        };
        p.rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(12, 14, 20, 200));
        p.rect_stroke(rect, 6.0, Stroke::new(1.0, edge), egui::StrokeKind::Inside);
        let mut y = rect.min.y + 6.0;
        let shadowed = |text: &str, at: egui::Pos2, font: FontId, color: Color32| {
            p.text(at + Vec2::splat(1.0), Align2::CENTER_TOP, text, font.clone(), Color32::BLACK);
            p.text(at, Align2::CENTER_TOP, text, font, color);
        };
        shadowed(&t.name, egui::pos2(rect.center().x, y), FontId::proportional(14.0), name_color);
        y += 18.0;
        if let Some((info, color)) = &t.info {
            shadowed(info, egui::pos2(rect.center().x, y), FontId::proportional(11.5), *color);
            y += 17.0;
        }
        let bar = |y: f32, frac: f32, fill: Color32, text: &str| {
            let r = Rect::from_min_size(egui::pos2(rect.min.x + 10.0, y), Vec2::new(WIDTH - 20.0, 14.0));
            p.rect_filled(r, 3.0, Color32::from_rgb(40, 22, 24));
            if frac > 0.0 {
                p.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * frac.clamp(0.0, 1.0), r.height())), 3.0, fill);
            }
            p.rect_stroke(r, 3.0, Stroke::new(1.0, Color32::from_rgba_unmultiplied(0, 0, 0, 160)), egui::StrokeKind::Inside);
            p.text(r.center() + Vec2::splat(1.0), Align2::CENTER_CENTER, text, FontId::proportional(11.0), Color32::BLACK);
            p.text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(11.0), Color32::WHITE);
        };
        let frac = t.hp.map_or(0.0, |(hp, max)| hp.max(0) as f32 / max.max(1) as f32);
        bar(y + 2.0, frac, Color32::from_rgb(200, 45, 50), &hp_text(t.hp));
        y += 18.0;
        if let Some((skill, frac)) = &t.cast {
            bar(y + 2.0, *frac, Color32::from_rgb(220, 150, 40), &format!("Casting {skill}"));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hp_reads_as_numbers_and_a_share() {
        assert_eq!(hp_text(Some((1234, 5000))), "1,234 / 5,000 · 25%");
        // A sliver left still reads 1%, not 0%.
        assert_eq!(hp_text(Some((1, 1_000_000))), "1 / 1,000,000 · 1%");
        assert_eq!(hp_text(Some((-5, 50))), "0 / 50 · 0%");
        assert_eq!(hp_text(None), "HP —");
    }

    #[test]
    fn it_lets_go_after_a_quiet_spell() {
        let t = Instant::now();
        assert!(keeps(t, t + Duration::from_secs(5)));
        assert!(!keeps(t, t + KEEP + Duration::from_secs(1)));
    }
}
