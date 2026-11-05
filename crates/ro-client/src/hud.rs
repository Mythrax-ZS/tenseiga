//! In-game HUD: character status, chat, and names over actors. Drawn with egui on top of
//! the world; layout scales with the window's DPI and the configured UI scale.

use crate::uitex::UiSize;
use std::collections::VecDeque;

use egui::{Align2, Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, Vec2};
use ro_net::{sp, ChatKind};

const CHAT_LINES: usize = 200;

/// The local character's numbers, as the server reports them.
#[derive(Default, Clone)]
pub struct Status {
    pub name: String,
    pub job: String,
    pub job_id: i16,
    pub base_level: i64,
    pub job_level: i64,
    pub hp: i64,
    pub max_hp: i64,
    pub sp: i64,
    pub max_sp: i64,
    /// 4th jobs' Activity Points (SP_AP / SP_MAXAP); max 0 for everyone else.
    pub ap: i64,
    pub max_ap: i64,
    pub base_exp: i64,
    pub next_base_exp: i64,
    pub job_exp: i64,
    pub next_job_exp: i64,
    pub zeny: i64,
    pub skill_points: i64,
    pub weight: i64,
    pub max_weight: i64,
    /// Percent of max weight where the server starts limiting (ZC_OVERWEIGHT_PERCENT; 0: 50).
    pub overweight_pct: u32,
}

impl Status {
    pub fn set(&mut self, id: u16, value: i64) {
        match id {
            sp::HP => self.hp = value,
            sp::MAXHP => self.max_hp = value,
            sp::SP => self.sp = value,
            sp::MAXSP => self.max_sp = value,
            sp::BASELEVEL => self.base_level = value,
            sp::JOBLEVEL => self.job_level = value,
            sp::BASEEXP => self.base_exp = value,
            sp::NEXTBASEEXP => self.next_base_exp = value,
            sp::JOBEXP => self.job_exp = value,
            sp::NEXTJOBEXP => self.next_job_exp = value,
            sp::ZENY => self.zeny = value,
            12 => self.skill_points = value, // SP_SKILLPOINT
            232 => self.ap = value,          // SP_AP
            233 => self.max_ap = value,      // SP_MAXAP
            sp::WEIGHT => self.weight = value,
            sp::MAXWEIGHT => self.max_weight = value,
            _ => {}
        }
    }
}

/// A name label for an actor on screen.
pub struct Label {
    /// Window pixels (physical) of the actor's feet.
    pub feet: glam::Vec2,
    pub text: String,
    pub color: Color32,
    /// Guild emblem drawn left of the name.
    pub emblem: Option<egui::TextureId>,
    /// Spirit spheres circling the unit.
    pub orbs: u16,
    /// What they are (see `ro_net::Event::Spirits`); sets the colour.
    pub orb_kind: u8,
    /// Window pixels per sprite pixel at the actor (`Renderer::sprite_pixel`): the spheres
    /// keep to the sprite's head at any zoom.
    pub sprite_px: f32,
}

#[derive(Default)]
pub struct Hud {
    /// `showdigit` clock text, drawn large at the top of the screen.
    pub digit_text: Option<String>,
    /// Our PvP rank on this map ("rank / players").
    pub pvp_text: Option<String>,
    /// Devotion links (screen pixels), drawn under the names.
    pub beams: Vec<(glam::Vec2, glam::Vec2)>,
    /// Graffiti messages on the ground (screen pixels).
    pub scrawls: Vec<(glam::Vec2, String)>,
    pub status: Status,
    /// Script overrides for the NPC dialog's layout; cleared when the dialog ends.
    pub dialog_layout: DialogLayout,
    /// Floating notice across the upper screen (special popups): lines, colour, when shown.
    pub banner: Option<(Vec<String>, Color32, std::time::Instant)>,
    /// Chat lines: kind, text, the colour a script asked for (else the kind's), and when it
    /// arrived (HH:MM).
    chat: VecDeque<(ChatKind, String, Option<Color32>, String)>,
    /// Show each chat line's time (Settings > Game > Chat).
    pub timestamps: bool,
    /// The chat log file, while Settings > Game > Chat > Save to file is on.
    pub log: Option<crate::chatlog::ChatLog>,
    /// Lines since the last flush_log, for the log (the game names their item links first).
    unlogged: Vec<(chrono::DateTime<chrono::Local>, ChatKind, String)>,
    /// Names for item links in chat, by item id: (name, slots), filled in by the game from
    /// its item table. Ids the chat met without a name are left in `missing_links`.
    pub link_names: std::collections::HashMap<u32, (String, u8)>,
    pub missing_links: Vec<u32>,
    /// An item link the player clicked, for the game to open.
    pub clicked_link: Option<crate::itemlink::Link>,
    /// A link clicked in an NPC dialog (<NAVI>, <URL>, item), for the game to follow.
    pub dialog_link: Option<crate::npcmarkup::Action>,
    input: String,
    /// Enter focuses the chat box; Enter again sends, Escape leaves.
    want_focus: bool,
    /// Lines sent, oldest first; Up/Down in the chat box walk through them.
    history: Vec<String>,
    /// Position while browsing history (None: editing a fresh line).
    recall: Option<usize>,
    /// Controller input for the NPC dialog this frame, and the highlighted menu option.
    pub pad_nav: Option<PadNav>,
    menu_cursor: usize,
    /// A controller is in use: keep the menu highlight visible.
    pub pad_used: bool,
    /// Classic interface: the original chat input bar (dialog_bg); None draws the modern box.
    pub classic_bar: Option<egui::TextureHandle>,
    /// Classic chat: the whisper name field; lines go to this player when it is filled.
    whisper_to: String,
    /// A Classic Basic Info button clicked this frame (the Alt+key letter of its window).
    pub menu_click: Option<&'static str>,
}

/// Dialog navigation from a controller.
#[derive(Clone, Copy, PartialEq)]
pub enum PadNav {
    Up,
    Down,
    Confirm,
    Cancel,
}

fn chat_color(kind: ChatKind) -> Color32 {
    match kind {
        ChatKind::Public => Color32::from_rgb(235, 235, 235),
        ChatKind::Npc => Color32::from_rgb(150, 220, 255),
        ChatKind::Whisper => Color32::from_rgb(255, 230, 120),
        ChatKind::Broadcast => Color32::from_rgb(255, 255, 120),
        ChatKind::System => Color32::from_rgb(170, 255, 170),
        ChatKind::Party => Color32::from_rgb(255, 200, 220),
        ChatKind::Guild => Color32::from_rgb(180, 255, 200),
        ChatKind::Clan => Color32::from_rgb(255, 200, 120),
        ChatKind::Battle => Color32::from_rgb(255, 160, 200),
    }
}

fn panel() -> Frame {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(16, 20, 28, 210))
        .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 30)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
}

/// Popups (NPC dialog, death, game menu): the original's white box with a blue-grey border
/// in Classic, the dark panel otherwise.
fn popup_frame(ctx: &egui::Context) -> Frame {
    if crate::classic::skin(ctx).is_some() {
        Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, Color32::from_rgb(130, 146, 190)))
            .corner_radius(CornerRadius::same(2))
            .inner_margin(Margin::same(10))
            .shadow(egui::Shadow { offset: [2, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(60) })
    } else {
        panel()
    }
}

/// A labelled bar like "HP 120 / 200".
fn bar(ui: &mut egui::Ui, label: &str, value: i64, max: i64, color: Color32, show_numbers: bool) {
    let frac = if max > 0 { (value as f32 / max as f32).clamp(0.0, 1.0) } else { 0.0 };
    let text = if show_numbers { format!("{label}  {value} / {max}") } else { format!("{label}  {:.1}%", frac * 100.0) };
    ui.add(
        egui::ProgressBar::new(frac)
            .fill(color)
            .desired_height(16.0)
            .corner_radius(CornerRadius::same(4))
            .text(RichText::new(text).size(11.0).color(Color32::WHITE)),
    );
}

/// An item link as the original shows it: <+7 Name [slots]>. An unknown id is noted in
/// `missing` for the game to name.
fn link_label(names: &std::collections::HashMap<u32, (String, u8)>, missing: &mut Vec<u32>, link: &crate::itemlink::Link) -> String {
    match names.get(&link.id) {
        Some((name, slots)) => {
            let refine = if link.refine > 0 { format!("+{} ", link.refine) } else { String::new() };
            let slots = if link.equip && *slots > 0 { format!(" [{slots}]") } else { String::new() };
            format!("<{refine}{name}{slots}>")
        }
        None => {
            if !missing.contains(&link.id) {
                missing.push(link.id);
            }
            format!("<Item #{}>", link.id)
        }
    }
}

impl Hud {
    /// Puts text in the chat box and focuses it (e.g. a whisper to finish typing).
    pub fn set_input(&mut self, text: String) {
        self.input = text;
        self.want_focus = true;
    }

    pub fn push_chat(&mut self, kind: ChatKind, text: String) {
        self.push_chat_coloured(kind, text, None);
    }

    /// A chat line in the colour a script asked for (npctalk, announce), if any.
    pub fn push_chat_coloured(&mut self, kind: ChatKind, text: String, rgb: Option<[u8; 3]>) {
        tracing::debug!(?kind, %text, "chat");
        let at = chrono::Local::now();
        // Kept for the log until the next frame, which may be the one that opens it (the
        // server's welcome arrives with the map).
        self.unlogged.push((at, kind, text.clone()));
        self.chat.push_back((kind, text, rgb.map(|[r, g, b]| Color32::from_rgb(r, g, b)), crate::chatlog::stamp(at)));
        while self.chat.len() > CHAT_LINES {
            self.chat.pop_front();
        }
    }

    /// Writes the lines since the last call to the chat log, item links by name (`item`: an
    /// item's name and slots from the game's item table).
    pub fn flush_log(&mut self, item: impl Fn(u32) -> Option<(String, u8)>) {
        let Some(log) = self.log.as_mut() else {
            self.unlogged.clear();
            return;
        };
        for (at, kind, text) in std::mem::take(&mut self.unlogged) {
            let plain: String = crate::itemlink::split(&text)
                .into_iter()
                .map(|piece| match piece {
                    crate::itemlink::Piece::Text(t) => t.to_string(),
                    crate::itemlink::Piece::Item(link) => {
                        if let (false, Some(named)) = (self.link_names.contains_key(&link.id), item(link.id)) {
                            self.link_names.insert(link.id, named);
                        }
                        link_label(&self.link_names, &mut self.missing_links, &link)
                    }
                })
                .collect();
            log.write(at, kind, &plain);
        }
    }

    /// True while the chat box has keyboard focus (game hotkeys should yield).
    pub fn focus_chat(&mut self) {
        self.want_focus = true;
    }

    /// A key typed over the game outside battle mode: it starts (or continues) a chat line.
    pub fn type_into_chat(&mut self, text: &str) {
        self.input.push_str(text);
        self.want_focus = true;
    }

    /// Draws the HUD; returns a chat line the player submitted.
    /// `floaters`: (screen pos, text, colour, age ms) damage numbers; `bars`: (feet, hp fraction).
    /// `casts`: (feet, progress 0..1) cast bars drawn over the caster's head.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        labels: &[Label],
        floaters: &[(glam::Vec2, String, Color32, f32)],
        bars: &[(glam::Vec2, f32)],
        casts: &[(glam::Vec2, f32)],
        says: &[(glam::Vec2, String, bool)],
    ) -> Option<String> {
        // Names under actors, outlined for readability on any background.
        let painter = ctx.layer_painter(egui::LayerId::background());
        let ppp = ctx.pixels_per_point();
        if !self.beams.is_empty() {
            // A soft glow around a bright core that shimmers along its length.
            let t = ctx.input(|i| i.time) as f32;
            for &(a, b) in &self.beams {
                let (a, b) = (egui::pos2(a.x / ppp, a.y / ppp), egui::pos2(b.x / ppp, b.y / ppp));
                let pulse = 0.75 + 0.25 * (t * 4.0).sin();
                painter.line_segment([a, b], egui::Stroke::new(6.0, Color32::from_rgba_unmultiplied(255, 200, 110, (50.0 * pulse) as u8)));
                painter.line_segment([a, b], egui::Stroke::new(2.0, Color32::from_rgba_unmultiplied(255, 236, 190, (210.0 * pulse) as u8)));
                let k = (t * 0.8).fract();
                painter.circle_filled(a + (b - a) * k, 3.0, Color32::from_rgba_unmultiplied(255, 250, 230, 220));
            }
            ctx.request_repaint();
        }
        // Graffiti: the scrawled message sits on its cell, in spray-paint colours.
        for (pos, text) in &self.scrawls {
            let p = egui::pos2(pos.x / ppp, pos.y / ppp);
            let font = FontId::proportional(16.0);
            painter.text(p + Vec2::splat(1.5), Align2::CENTER_CENTER, text, font.clone(), Color32::from_black_alpha(160));
            painter.text(p, Align2::CENTER_CENTER, text, font, Color32::from_rgb(255, 110, 200));
        }
        for l in labels {
            let pos = egui::pos2(l.feet.x / ppp, l.feet.y / ppp + 6.0);
            let font = FontId::proportional(12.0);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                painter.text(pos + Vec2::new(dx, dy), Align2::CENTER_TOP, &l.text, font.clone(), Color32::BLACK);
            }
            if let Some(e) = l.emblem {
                let w = ctx.fonts_mut(|f| f.layout_no_wrap(l.text.clone(), FontId::proportional(12.0), Color32::WHITE).size().x);
                let r = egui::Rect::from_min_size(egui::pos2(pos.x - w / 2.0 - 22.0, pos.y - 2.0), Vec2::splat(18.0));
                painter.image(e, r, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            painter.text(pos, Align2::CENTER_TOP, &l.text, font, l.color);
            if l.orbs > 0 {
                // Spheres circling over the head, as the original draws them. Sized in sprite
                // pixels: a fixed offset in window points sat at the waist when zoomed in.
                let t = ctx.input(|i| i.time) as f32;
                let px = l.sprite_px / ppp;
                let centre = egui::pos2(l.feet.x / ppp, l.feet.y / ppp - 73.0 * px);
                for k in 0..l.orbs.min(15) {
                    let a = t * 1.6 + k as f32 * std::f32::consts::TAU / l.orbs.min(15) as f32;
                    let p = centre + Vec2::new(a.cos() * 20.0 * px, a.sin() * 7.0 * px);
                    let (glow, core) = match l.orb_kind {
                        1 => ((170, 80, 255), (215, 170, 255)),
                        2 => ((80, 200, 255), (200, 240, 255)),
                        3 => ((70, 20, 110), (150, 90, 200)),
                        11 => ((40, 110, 255), (140, 190, 255)),
                        12 => ((150, 100, 40), (210, 170, 100)),
                        13 => ((255, 60, 30), (255, 150, 90)),
                        14 => ((60, 220, 90), (160, 255, 170)),
                        _ => ((255, 160, 30), (255, 215, 80)),
                    };
                    painter.circle_filled(p, (5.3 * px).max(2.0), Color32::from_rgba_unmultiplied(glow.0, glow.1, glow.2, 90));
                    painter.circle(p, (3.0 * px).max(1.2), Color32::from_rgb(core.0, core.1, core.2), egui::Stroke::new(1.0, Color32::from_rgb(glow.0 / 2, glow.1 / 2, glow.2 / 2)));
                }
                ctx.request_repaint();
            }
        }

        // HP bars under monsters that have been hit.
        for &(feet, frac) in bars {
            let c = egui::pos2(feet.x / ppp, feet.y / ppp + 4.0);
            let rect = egui::Rect::from_center_size(c, Vec2::new(40.0, 5.0));
            painter.rect_filled(rect.expand(1.0), 1.0, Color32::BLACK);
            let mut fill = rect;
            fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
            painter.rect_filled(fill, 1.0, Color32::from_rgb(220, 60, 60));
        }
        for &(feet, frac) in casts {
            let c = egui::pos2(feet.x / ppp, feet.y / ppp - 100.0);
            let rect = egui::Rect::from_center_size(c, Vec2::new(60.0, 6.0));
            painter.rect_filled(rect.expand(1.0), 2.0, Color32::BLACK);
            let mut fill = rect;
            fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
            painter.rect_filled(fill, 2.0, Color32::from_rgb(90, 200, 90));
        }
        // Text over heads, outlined like names: speech (chat, npctalk) in white, wrapped as the
        // original wraps it; showscript call-outs in gold.
        for (feet, text, speech) in says {
            let colour = if *speech { Color32::from_rgb(240, 240, 240) } else { Color32::from_rgb(255, 240, 160) };
            let mut job = egui::text::LayoutJob::simple(text.clone(), FontId::proportional(15.0), colour, if *speech { 260.0 } else { f32::INFINITY });
            // Centred rows are laid out around x = 0, so the galley is placed at the head's x.
            job.halign = egui::Align::Center;
            let galley = painter.layout_job(job);
            let p = egui::pos2(feet.x / ppp, feet.y / ppp - 118.0 - galley.size().y);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                painter.galley_with_override_text_color(p + Vec2::new(dx, dy), galley.clone(), Color32::BLACK);
            }
            painter.galley(p, galley, colour);
        }
        if let Some(t) = &self.digit_text {
            let p = egui::pos2(ctx.content_rect().center().x, 70.0);
            let font = FontId::monospace(34.0);
            for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0)] {
                painter.text(p + Vec2::new(dx, dy), egui::Align2::CENTER_TOP, t, font.clone(), Color32::BLACK);
            }
            painter.text(p, egui::Align2::CENTER_TOP, t, font, Color32::from_rgb(255, 210, 90));
        }
        if let Some(t) = &self.pvp_text {
            let p = egui::pos2(ctx.content_rect().right() - 20.0, 300.0);
            let font = FontId::proportional(22.0);
            painter.text(p + Vec2::splat(1.5), egui::Align2::RIGHT_TOP, t, font.clone(), Color32::BLACK);
            painter.text(p, egui::Align2::RIGHT_TOP, t, font, Color32::from_rgb(255, 120, 120));
        }
        // Damage numbers rise and fade over their lifetime.
        for (pos, text, color, age) in floaters {
            let t = age / 1200.0;
            let p = egui::pos2(pos.x / ppp, pos.y / ppp - 60.0 - t * 40.0);
            let alpha = ((1.0 - t) * 255.0) as u8;
            let font = FontId::proportional(20.0);
            for (dx, dy) in [(-1.5, 0.0), (1.5, 0.0), (0.0, -1.5), (0.0, 1.5)] {
                painter.text(p + Vec2::new(dx, dy), Align2::CENTER_BOTTOM, text, font.clone(), Color32::from_black_alpha(alpha));
            }
            painter.text(p, Align2::CENTER_BOTTOM, text, font, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha));
        }

        let s = &self.status;
        let mut click = None;
        if let Some(skin) = crate::classic::skin(ctx) {
            // The original Basic Info window: name on the title bar, HP/SP gauges, levels and
            // EXP in the grey box, weight and zeny along the bottom, menu buttons at the right.
            egui::Area::new(egui::Id::unique("basic_info")).anchor(Align2::LEFT_TOP, Vec2::new(12.0, 12.0)).show(ctx, |ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::new(280.0, 120.0), egui::Sense::hover());
                let p = ui.painter().clone();
                skin.draw(&p, "basewin_bg", r.min);
                let at = |x: f32, y: f32| r.min + Vec2::new(x, y);
                let ink = crate::classic::TEXT;
                let f = |s: f32| egui::FontId::proportional(s);
                p.text(at(8.0, 8.5), Align2::LEFT_CENTER, &s.name, f(12.0), Color32::from_rgb(30, 30, 60));
                p.text(at(8.0, 27.0), Align2::LEFT_CENTER, &s.job, f(11.0), ink);
                let gauge = |y: f32, label: &str, v: i64, max: i64| {
                    p.text(at(104.0, y + 4.0), Align2::RIGHT_CENTER, label, f(10.5), ink);
                    p.text(at(196.0, y + 13.0), Align2::RIGHT_CENTER, format!("{v} / {max}"), f(9.5), ink);
                    skin.draw(&p, "gze_bg", at(112.0, y));
                    let frac = if max > 0 { (v as f32 / max as f32).clamp(0.0, 1.0) } else { 0.0 };
                    let red = label == "HP" && frac < 0.25;
                    let (l, m, rt) = if red { ("gzered_left", "gzered_mid", "gzered_right") } else { ("gzeblue_left", "gzeblue_mid", "gzeblue_right") };
                    let w = 83.0 * frac;
                    if w >= 2.0 {
                        skin.bar(&p, egui::Rect::from_min_size(at(113.0, y + 0.5), Vec2::new(w, 8.0)), l, m, rt);
                    }
                };
                gauge(21.5, "HP", s.hp, s.max_hp);
                gauge(42.5, "SP", s.sp, s.max_sp);
                // 4th jobs: an AP gauge in the gap above the grey box, its numbers beside it.
                if s.max_ap > 0 && crate::jobs::is_fourth(s.job_id) {
                    let y = 60.0;
                    p.text(at(108.0, y + 4.0), Align2::RIGHT_CENTER, format!("AP {} / {}", s.ap, s.max_ap), f(9.5), ink);
                    skin.draw(&p, "gze_bg", at(112.0, y));
                    let w = 83.0 * (s.ap as f32 / s.max_ap as f32).clamp(0.0, 1.0);
                    if w >= 1.0 {
                        p.rect_filled(egui::Rect::from_min_size(at(113.0, y + 1.5), Vec2::new(w, 6.0)), 3.0, Color32::from_rgb(235, 165, 40));
                    }
                }
                let pct = |v: i64, n: i64| if n > 0 { v as f32 * 100.0 / n as f32 } else { 0.0 };
                p.text(at(14.0, 78.5), Align2::LEFT_CENTER, format!("Base Lv. {}", s.base_level), f(10.5), ink);
                p.text(at(192.0, 78.5), Align2::RIGHT_CENTER, format!("{:.1}%", pct(s.base_exp, s.next_base_exp)), f(10.5), ink);
                p.text(at(14.0, 90.5), Align2::LEFT_CENTER, format!("Job Lv. {}", s.job_level), f(10.5), ink);
                p.text(at(192.0, 90.5), Align2::RIGHT_CENTER, format!("{:.1}%", pct(s.job_exp, s.next_job_exp)), f(10.5), ink);
                let pct = if s.overweight_pct == 0 { 50 } else { s.overweight_pct as i64 };
                        let heavy = s.max_weight > 0 && s.weight * 100 >= s.max_weight * pct;
                p.text(at(8.0, 114.0), Align2::LEFT_CENTER, format!("Weight : {} / {}", s.weight / 10, s.max_weight / 10), f(10.0), if heavy { Color32::from_rgb(200, 60, 40) } else { ink });
                p.text(at(198.0, 114.0), Align2::RIGHT_CENTER, format!("Zeny : {}", s.zeny), f(10.0), ink);
                // The menu buttons, two columns down the striped side.
                const BUTTONS: [(&str, &str, &str); 8] = [
                    ("btn_status", "a", "Status (Alt+A)"),
                    ("btn_equip", "q", "Equipment (Alt+Q)"),
                    ("btn_items", "e", "Inventory (Alt+E)"),
                    ("btn_skill", "s", "Skills (Alt+S)"),
                    ("btn_comm", "z", "Party (Alt+Z)"),
                    ("btn_friend", "h", "Friends (Alt+H)"),
                    ("btn_option", "o", "Settings (Alt+O)"),
                    ("btn_map", "m", "Minimap (Alt+M)"),
                ];
                for (i, (img, key, tip)) in BUTTONS.iter().enumerate() {
                    let b = egui::Rect::from_min_size(at(207.0 + (i % 2) as f32 * 36.0, 22.0 + (i / 2) as f32 * 22.0), Vec2::new(30.0, 20.0));
                    let resp = ui.interact(b, ui.scope_id().with(("basic_btn", i)), egui::Sense::click()).on_hover_text(*tip);
                    skin.draw(&p, &format!("{img}_{}", if resp.hovered() { "on" } else { "off" }), b.min);
                    if resp.clicked() {
                        click = Some(*key);
                    }
                }
            });
        } else {
        egui::Window::new("status")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::LEFT_TOP, Vec2::new(12.0, 12.0))
            .frame(panel())
            .show(ctx, |ui| {
                ui.style_mut().visuals = egui::Visuals::dark();
                ui.set_width(250.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&s.name).strong().size(15.0).color(Color32::WHITE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("Lv {} / {}", s.base_level, s.job_level)).color(Color32::LIGHT_GRAY));
                    });
                });
                ui.label(RichText::new(&s.job).size(11.0).color(Color32::GRAY));
                ui.add_space(4.0);
                bar(ui, "HP", s.hp, s.max_hp, Color32::from_rgb(70, 190, 90), true);
                bar(ui, "SP", s.sp, s.max_sp, Color32::from_rgb(70, 120, 220), true);
                if s.max_ap > 0 && crate::jobs::is_fourth(s.job_id) {
                    bar(ui, "AP", s.ap, s.max_ap, Color32::from_rgb(235, 165, 40), true);
                }
                ui.add_space(2.0);
                bar(ui, "Base", s.base_exp, s.next_base_exp, Color32::from_rgb(200, 160, 60), false);
                bar(ui, "Job", s.job_exp, s.next_job_exp, Color32::from_rgb(160, 110, 200), false);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} z", s.zeny)).size(11.0).color(Color32::from_rgb(240, 210, 120)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let pct = if s.overweight_pct == 0 { 50 } else { s.overweight_pct as i64 };
                        let heavy = s.max_weight > 0 && s.weight * 100 >= s.max_weight * pct;
                        let c = if heavy { Color32::from_rgb(255, 140, 90) } else { Color32::LIGHT_GRAY };
                        ui.label(RichText::new(format!("Weight {} / {}", s.weight / 10, s.max_weight / 10)).size(11.0).color(c));
                    });
                });
            });
        }
        if click.is_some() {
            self.menu_click = click;
        }

        let mut sent = None;
        let classic = self.classic_bar.clone();
        // Classic: the original's square, see-through log above its input bar.
        let frame = if classic.is_some() { Frame::new().fill(Color32::from_rgba_unmultiplied(0, 0, 0, 120)).inner_margin(Margin::same(4)) } else { panel() };
        egui::Window::new("chat")
            .title_bar(false)
            .resizable(true)
            .default_size(Vec2::new(460.0, 190.0))
            .anchor(Align2::LEFT_BOTTOM, Vec2::new(12.0, -12.0))
            .frame(frame)
            .show(ctx, |ui| {
                ui.style_mut().visuals = egui::Visuals::dark();
                let input_h = if classic.is_some() { 26.0 } else { 30.0 };
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .min_scrolled_height(if classic.is_some() { 150.0 } else { 0.0 })
                    .max_height(if classic.is_some() { 150.0 } else { ui.available_height() - input_h })
                    .show(ui, |ui| {
                        let stamp_color = Color32::from_gray(150);
                        for (kind, text, custom, time) in &self.chat {
                            let color = custom.unwrap_or_else(|| chat_color(*kind));
                            if !text.contains("<ITEM") {
                                if self.timestamps {
                                    // One label, so the line wraps as it would without the time.
                                    let font = egui::FontId::proportional(12.5);
                                    let mut job = egui::text::LayoutJob::default();
                                    job.append(&format!("[{time}] "), 0.0, egui::TextFormat::simple(font.clone(), stamp_color));
                                    job.append(text, 0.0, egui::TextFormat::simple(font, color));
                                    ui.label(job);
                                } else {
                                    ui.label(RichText::new(text).size(12.5).color(color));
                                }
                                continue;
                            }
                            // Item links: the item's name, which opens its description.
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                if self.timestamps {
                                    ui.label(RichText::new(format!("[{time}] ")).size(12.5).color(stamp_color));
                                }
                                for piece in crate::itemlink::split(text) {
                                    match piece {
                                        crate::itemlink::Piece::Text(t) => {
                                            ui.label(RichText::new(t).size(12.5).color(color));
                                        }
                                        crate::itemlink::Piece::Item(link) => {
                                            let name = link_label(&self.link_names, &mut self.missing_links, &link);
                                            let r = ui.add(egui::Label::new(RichText::new(name).size(12.5).color(LINK_COLOR)).sense(egui::Sense::click()));
                                            let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if r.clicked() {
                                                self.clicked_link = Some(link);
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    });
                let edit = if let Some(bar) = &classic {
                    // dialog_bg, three-sliced to the box width: whisper name box, then the
                    // message box stretched across.
                    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), egui::Sense::hover());
                    let p = ui.painter();
                    let tw = bar.ui_size().x;
                    let (cut_l, cut_r) = (114.0, tw - 30.0);
                    let slice = |x0: f32, x1: f32, u0: f32, u1: f32| {
                        p.image(bar.id(), egui::Rect::from_min_max(egui::pos2(x0, r.min.y), egui::pos2(x1, r.max.y)), egui::Rect::from_min_max(egui::pos2(u0 / tw, 0.0), egui::pos2(u1 / tw, 1.0)), Color32::WHITE);
                    };
                    slice(r.min.x, r.min.x + cut_l, 0.0, cut_l);
                    slice(r.min.x + cut_l, r.max.x - (tw - cut_r), cut_l, cut_r);
                    slice(r.max.x - (tw - cut_r), r.max.x, cut_r, tw);
                    let dark = Color32::from_rgb(30, 30, 40);
                    let field = |ui: &mut egui::Ui, rect: egui::Rect, text: &mut String, hint: &str| {
                        ui.put(rect, egui::TextEdit::singleline(text).frame(egui::Frame::NONE).hint_text(hint).text_color(dark).font(egui::FontId::proportional(12.5)))
                    };
                    let whisper = egui::Rect::from_min_max(egui::pos2(r.min.x + 6.0, r.min.y + 4.0), egui::pos2(r.min.x + 96.0, r.max.y - 4.0));
                    field(ui, whisper, &mut self.whisper_to, "Whisper").on_hover_text("Name to whisper to; empty for public chat");
                    let msg = egui::Rect::from_min_max(egui::pos2(r.min.x + cut_l + 4.0, r.min.y + 4.0), egui::pos2(r.max.x - 34.0, r.max.y - 4.0));
                    field(ui, msg, &mut self.input, "Press Enter to chat")
                } else {
                    ui.add(egui::TextEdit::singleline(&mut self.input).hint_text("Press Enter to chat").desired_width(f32::INFINITY))
                };
                if self.want_focus {
                    edit.request_focus();
                    self.want_focus = false;
                    // Caret after what is already typed.
                    let mut state = egui::TextEdit::load_state(ui.ctx(), edit.id).unwrap_or_default();
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(self.input.chars().count()))));
                    state.store(ui.ctx(), edit.id);
                }
                if edit.has_focus() && !self.history.is_empty() {
                    let (up, down) = ui.input(|i| (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown)));
                    let last = self.history.len() - 1;
                    let recall = match (up, down, self.recall) {
                        (true, _, None) => Some(Some(last)),
                        (true, _, Some(i)) => Some(Some(i.saturating_sub(1))),
                        (_, true, Some(i)) if i < last => Some(Some(i + 1)),
                        (_, true, Some(_)) => Some(None),
                        _ => None,
                    };
                    if let Some(r) = recall {
                        self.recall = r;
                        self.input = r.map(|i| self.history[i].clone()).unwrap_or_default();
                        // Caret to the end of the recalled line.
                        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), edit.id) {
                            let end = egui::text::CCursor::new(self.input.chars().count());
                            state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                            state.store(ui.ctx(), edit.id);
                        }
                    }
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let text = std::mem::take(&mut self.input);
                    self.recall = None;
                    if !text.trim().is_empty() {
                        if self.history.last() != Some(&text) {
                            self.history.push(text.clone());
                            if self.history.len() > 100 {
                                self.history.remove(0);
                            }
                        }
                        // A whisper name in the classic bar makes plain lines whispers.
                        let to = self.whisper_to.trim();
                        sent = Some(if classic.is_some() && !to.is_empty() && !text.starts_with('/') { format!("/w \"{to}\" {text}") } else { text });
                    }
                }
            });
        sent
    }
}

// ---------------------------------------------------------------- NPC dialog

/// What the dialog window currently offers.
#[derive(Clone, PartialEq)]
pub enum DialogMode {
    /// Text is still arriving.
    Reading,
    Next,
    Close,
    Menu(Vec<String>),
    Number(String),
    Text(String),
}

pub struct Dialog {
    pub npc: u32,
    pub title: String,
    pub lines: Vec<String>,
    pub mode: DialogMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DialogPos {
    /// Top-left corner in screen pixels.
    Pixels(i32, i32),
    /// Centre as a percentage of the screen.
    Percent(i32, i32),
}

/// What `setdialogalign` / `setdialogsize` / `setdialogpos` asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DialogLayout {
    pub align: u8,
    pub size: Option<(i32, i32)>,
    pub pos: Option<DialogPos>,
}

/// An NPC illustration ("cutin") and where the script wants it.
pub struct Cutin {
    pub texture: egui::TextureHandle,
    /// 0 bottom-left, 1 bottom-centre, 2 bottom-right, 3 middle-right, 4 middle-centre.
    pub position: u8,
}

/// The colour NPC text starts in (and ^000000 returns to).
fn npc_default_color() -> Color32 {
    if crate::classic::ink(Color32::WHITE) == Color32::WHITE { Color32::from_rgb(230, 230, 230) } else { Color32::BLACK }
}

/// Links without a colour of their own.
const LINK_COLOR: Color32 = Color32::from_rgb(120, 200, 255);

/// One span's look; links are underlined.
fn span_format(s: &crate::npcmarkup::Span, size: f32) -> egui::TextFormat {
    let color = s.color.unwrap_or(if s.link.is_some() { LINK_COLOR } else { npc_default_color() });
    let mut f = egui::TextFormat::simple(FontId::proportional(size * s.scale), color);
    if s.link.is_some() {
        f.underline = egui::Stroke::new(1.0, color);
    }
    f
}

/// What an item link shows when the item's name isn't known here.
fn span_text(s: &crate::npcmarkup::Span) -> std::borrow::Cow<'_, str> {
    match &s.link {
        Some(crate::npcmarkup::Action::Item(link)) if s.text.is_empty() => format!("<Item #{}>", link.id).into(),
        _ => s.text.as_str().into(),
    }
}

/// NPC text with its markup (colours, font tags, links drawn but not clickable; see
/// `npcmarkup`).
pub(crate) fn npc_text(text: &str, size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for s in crate::npcmarkup::parse(text) {
        job.append(&span_text(&s), 0.0, span_format(&s, size));
    }
    job
}

impl Hud {
    /// Shows the NPC dialog and cutin; returns the player's answer.
    pub fn show_dialog(&mut self, ctx: &egui::Context, dialog: &mut Option<Dialog>, cutin: Option<&Cutin>) -> Option<(u32, ro_net::NpcReply)> {
        use ro_net::NpcReply;

        if let Some((lines, color, at)) = &self.banner {
            const SHOW: f32 = 6.0;
            let t = at.elapsed().as_secs_f32();
            if t > SHOW {
                self.banner = None;
            } else {
                let alpha = ((SHOW - t) / 1.0).min(1.0);
                egui::Area::new(egui::Id::unique("special popup")).order(egui::Order::Foreground).anchor(Align2::CENTER_TOP, Vec2::new(0.0, 140.0)).interactable(false).show(ctx, |ui| {
                    Frame::new().fill(Color32::from_black_alpha((150.0 * alpha) as u8)).inner_margin(Margin::symmetric(18, 10)).corner_radius(CornerRadius::same(6)).show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            for l in lines {
                                ui.label(RichText::new(l).size(16.0).strong().color(color.gamma_multiply(alpha)));
                            }
                        });
                    });
                });
                ctx.request_repaint();
            }
        }

        if let Some(c) = cutin {
            let size = c.texture.ui_size();
            let (align, offset) = match c.position {
                0 => (Align2::LEFT_BOTTOM, Vec2::new(20.0, -220.0)),
                1 => (Align2::CENTER_BOTTOM, Vec2::new(0.0, -220.0)),
                3 => (Align2::RIGHT_CENTER, Vec2::new(-20.0, 0.0)),
                4 => (Align2::CENTER_CENTER, Vec2::ZERO),
                _ => (Align2::RIGHT_BOTTOM, Vec2::new(-20.0, -20.0)),
            };
            egui::Area::new(egui::Id::unique("cutin")).anchor(align, offset).interactable(false).show(ctx, |ui| {
                ui.image((c.texture.id(), size));
            });
        }

        let Some(d) = dialog.as_mut() else {
            self.dialog_layout = DialogLayout::default();
            return None;
        };
        let mut reply = None;
        let layout = self.dialog_layout;
        let (width, text_height) = layout.size.map_or((440.0, 220.0), |(w, h)| ((w as f32 - 16.0).max(160.0), (h as f32 - 90.0).max(40.0)));
        let screen = ctx.content_rect();
        let window = egui::Window::new("npc dialog").title_bar(false).resizable(false).frame(popup_frame(ctx));
        let window = match layout.pos {
            Some(DialogPos::Pixels(x, y)) => window.fixed_pos(egui::pos2(x as f32, y as f32)),
            Some(DialogPos::Percent(x, y)) => window.pivot(Align2::CENTER_CENTER).fixed_pos(egui::pos2(screen.width() * x as f32 / 100.0, screen.height() * y as f32 / 100.0)),
            None => window.anchor(Align2::CENTER_CENTER, Vec2::new(0.0, 60.0)),
        };
        window
            .show(ctx, |ui| {
                if crate::classic::skin(ui.ctx()).is_none() {
                    ui.style_mut().visuals = egui::Visuals::dark();
                }
                ui.set_width(width);
                if !d.title.is_empty() {
                    ui.label(RichText::new(&d.title).strong().size(14.0).color(crate::classic::ink(Color32::from_rgb(160, 225, 255))));
                    ui.add_space(4.0);
                }
                let (h, v) = match layout.align {
                    1 => (egui::Align::Max, egui::Align::Min),
                    2 => (egui::Align::Center, egui::Align::Min),
                    4 => (egui::Align::Min, egui::Align::Center),
                    5 => (egui::Align::Min, egui::Align::Max),
                    _ => (egui::Align::Min, egui::Align::Min),
                };
                egui::ScrollArea::vertical().max_height(text_height).stick_to_bottom(true).show(ui, |ui| {
                    // A fixed size keeps the text box that tall, so vertical alignment means something.
                    let min_h = if layout.size.is_some() { text_height } else { 0.0 };
                    ui.allocate_ui_with_layout(Vec2::new(width, min_h), egui::Layout::top_down(h).with_main_align(v).with_main_justify(min_h > 0.0 && v != egui::Align::Min), |ui| {
                        ui.set_min_height(min_h);
                        for line in &d.lines {
                            let spans = crate::npcmarkup::parse(line);
                            if !crate::npcmarkup::has_links(&spans) {
                                ui.label(npc_text(line, 14.0));
                                continue;
                            }
                            // Links: <NAVI> guides there, <URL> opens the page, items show their description.
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for s in &spans {
                                    let text = match &s.link {
                                        Some(crate::npcmarkup::Action::Item(link)) => link_label(&self.link_names, &mut self.missing_links, link),
                                        _ => s.text.clone(),
                                    };
                                    let mut job = egui::text::LayoutJob::default();
                                    job.append(&text, 0.0, span_format(s, 14.0));
                                    match &s.link {
                                        Some(a) if !matches!(a, crate::npcmarkup::Action::Tip(_)) => {
                                            let r = ui.add(egui::Label::new(job).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand);
                                            if r.clicked() {
                                                self.dialog_link = Some(a.clone());
                                            }
                                        }
                                        _ => {
                                            ui.label(job);
                                        }
                                    }
                                }
                            });
                        }
                    });
                });
                ui.add_space(8.0);
                let nav = self.pad_nav.take();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter)) || nav == Some(PadNav::Confirm);
                match &mut d.mode {
                    DialogMode::Reading => {}
                    DialogMode::Next => {
                        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Next").clicked() || enter {
                                reply = Some(NpcReply::Next);
                            }
                        });
                    }
                    DialogMode::Close => {
                        ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Close").clicked() || enter || nav == Some(PadNav::Cancel) {
                                reply = Some(NpcReply::Close);
                            }
                        });
                    }
                    DialogMode::Menu(options) => {
                        // Empty options are hidden but keep their index.
                        let shown: Vec<usize> = (0..options.len()).filter(|&i| !options[i].is_empty()).collect();
                        let mut at = shown.iter().position(|&i| i == self.menu_cursor).unwrap_or(0);
                        match nav {
                            Some(PadNav::Up) => at = at.saturating_sub(1),
                            Some(PadNav::Down) => at = (at + 1).min(shown.len().saturating_sub(1)),
                            _ => {}
                        }
                        self.menu_cursor = shown.get(at).copied().unwrap_or(0);
                        for &i in &shown {
                            let button = egui::Button::new(npc_text(&options[i], 13.5))
                                .min_size(Vec2::new(ui.available_width(), 24.0))
                                .selected(self.pad_used && i == self.menu_cursor);
                            if ui.add(button).clicked() || (i == self.menu_cursor && nav == Some(PadNav::Confirm)) {
                                reply = Some(NpcReply::Menu((i + 1).min(254) as u8));
                            }
                        }
                        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) || nav == Some(PadNav::Cancel) {
                            reply = Some(NpcReply::Menu(255));
                        }
                    }
                    DialogMode::Number(value) => {
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(value).desired_width(200.0));
                            edit.request_focus();
                            if ui.button("OK").clicked() || (edit.lost_focus() && enter) {
                                reply = Some(NpcReply::Number(value.trim().parse().unwrap_or(0)));
                            }
                        });
                    }
                    DialogMode::Text(value) => {
                        ui.horizontal(|ui| {
                            let edit = ui.add(egui::TextEdit::singleline(value).desired_width(300.0));
                            edit.request_focus();
                            if ui.button("OK").clicked() || (edit.lost_focus() && enter) {
                                reply = Some(NpcReply::Text(value.clone()));
                            }
                        });
                    }
                }
            });

        let reply = reply?;
        self.menu_cursor = 0;
        let npc = d.npc;
        match reply {
            // Close ends the conversation; other answers wait for the script's next step.
            NpcReply::Close => *dialog = None,
            NpcReply::Menu(255) => *dialog = None,
            NpcReply::Next | NpcReply::Menu(_) => {
                d.mode = DialogMode::Reading;
                d.lines.clear();
            }
            _ => d.mode = DialogMode::Reading,
        }
        Some((npc, reply))
    }
}

/// The map's thumbnail and the cell extents it covers.
pub struct Minimap {
    pub texture: egui::TextureHandle,
    pub cells: (f32, f32),
}

/// Minimap under the menu bar: `dots` are cells with a colour, `me` is drawn last and larger.
pub fn show_minimap(ctx: &egui::Context, map: &Minimap, name: &str, me: (f32, f32), dots: &[((f32, f32), Color32)], marks: &[((f32, f32), Color32)]) {
    const SIZE: f32 = 160.0;
    egui::Area::new(egui::Id::unique("minimap")).anchor(Align2::RIGHT_TOP, Vec2::new(-12.0, 52.0)).interactable(false).show(ctx, |ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(SIZE), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.image(map.texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        // Cell y grows north; the image's y grows down.
        let at = |c: (f32, f32)| rect.min + Vec2::new(c.0 / map.cells.0 * SIZE, (1.0 - c.1 / map.cells.1) * SIZE);
        for &(c, color) in dots {
            painter.circle_filled(at(c), 2.0, color);
        }
        painter.circle(at(me), 3.5, Color32::WHITE, egui::Stroke::new(1.0, Color32::BLACK));
        // Navigation stops and script marks (viewpoint): pulsing rings.
        if !marks.is_empty() {
            let pulse = 4.0 + ((ctx.input(|i| i.time) * 4.0).sin() as f32 + 1.0) * 1.5;
            for &(c, color) in marks {
                painter.circle_stroke(at(c), pulse, egui::Stroke::new(2.0, color));
            }
            ctx.request_repaint();
        }
        // Map name and coordinates over the image's bottom edge, outlined for contrast.
        let text = format!("{name}  {}, {}", me.0 as i32, me.1 as i32);
        let pos = rect.center_bottom() - Vec2::new(0.0, 3.0);
        let font = FontId::proportional(12.0);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            painter.text(pos + Vec2::new(dx, dy), Align2::CENTER_BOTTOM, &text, font.clone(), Color32::BLACK);
        }
        painter.text(pos, Align2::CENTER_BOTTOM, &text, font, Color32::WHITE);
    });
}

/// A bobbing arrow over the controller's target (orange while aiming a skill).
pub fn target_marker(ctx: &egui::Context, feet: glam::Vec2, ppp: f32, aiming: bool) {
    let painter = ctx.layer_painter(egui::LayerId::background());
    let bob = (ctx.input(|i| i.time) * 5.0).sin() as f32 * 3.0;
    let tip = egui::pos2(feet.x / ppp, feet.y / ppp - 112.0 + bob);
    let color = if aiming { Color32::from_rgb(255, 150, 60) } else { Color32::from_rgb(255, 225, 90) };
    let pts = vec![tip, tip + Vec2::new(-9.0, -12.0), tip + Vec2::new(9.0, -12.0)];
    painter.add(egui::Shape::convex_polygon(pts, color, Stroke::new(1.5, Color32::BLACK)));
    ctx.request_repaint();
}

/// Shown while dead, like the original client's restart window: return to the save point,
/// or wait here for a resurrection. Returns true when the player chose to return.
pub enum DeathChoice {
    SavePoint,
    /// Get up where we fell (CZ_STANDING_RESURRECTION).
    Revive,
}

/// The death window. `revive` names what can revive us on the spot (a Token of Siegfried,
/// Light of Regene), which adds a button for it as in the original.
pub fn death_window(ctx: &egui::Context, pad: Option<crate::padglyph::Style>, revive: Option<&str>) -> Option<DeathChoice> {
    let mut go = None;
    egui::Window::new("death")
        .title_bar(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::new(0.0, -190.0))
        .frame(popup_frame(ctx))
        .show(ctx, |ui| {
                if crate::classic::skin(ui.ctx()).is_none() {
                    ui.style_mut().visuals = egui::Visuals::dark();
                }
            ui.set_width(260.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("You have died.").size(16.0).strong().color(Color32::from_rgb(235, 110, 100)));
                ui.add_space(4.0);
                ui.label(RichText::new("Wait for a resurrection, or return to your save point.").size(12.5).color(Color32::from_gray(200)));
                ui.add_space(10.0);
                let back = ui.add(egui::Button::new(RichText::new("Return to Save Point").size(14.0)).min_size(Vec2::new(220.0, 30.0)));
                // With a controller, its confirm button drawn in the button's corner.
                if let Some(style) = pad {
                    let h = 18.0;
                    let at = back.rect.right_center() - Vec2::new(crate::padglyph::width(crate::padglyph::Btn::South, h) + 6.0, h / 2.0);
                    crate::padglyph::paint_at(ui.painter(), at, crate::padglyph::Btn::South, style, h);
                }
                if back.clicked() {
                    go = Some(DeathChoice::SavePoint);
                }
                if let Some(with) = revive {
                    ui.add_space(4.0);
                    if ui.add(egui::Button::new(RichText::new(format!("Revive ({with})")).size(14.0)).min_size(Vec2::new(220.0, 30.0))).clicked() {
                        go = Some(DeathChoice::Revive);
                    }
                }
            });
        });
    go
}

/// The game's mouse cursors (data\sprite\cursors.spr/act), drawn in place of the system
/// cursor. Actions: 0 arrow, 1 talk, 4 rotate camera, 5 attack, 7 warp, 8 not allowed,
/// 9 pick up, 10 skill target.
pub struct CursorSprite {
    images: Vec<egui::TextureHandle>,
    act: ro_formats::act::Act,
}

pub mod cursor {
    pub const ARROW: usize = 0;
    pub const TALK: usize = 1;
    pub const ROTATE: usize = 4;
    pub const ATTACK: usize = 5;
    pub const PICK_UP: usize = 9;
    pub const TARGET: usize = 10;
}

impl CursorSprite {
    pub fn load(ctx: &egui::Context, vfs: &ro_vfs::Vfs) -> Option<Self> {
        let spr = ro_formats::spr::Spr::parse(&vfs.read(r"data\sprite\cursors.spr").ok()??).ok()?;
        let act = ro_formats::act::Act::parse(&vfs.read(r"data\sprite\cursors.act").ok()??).ok()?;
        let (indexed, rgba) = spr.images(None);
        let images = indexed
            .into_iter()
            .chain(rgba)
            .enumerate()
            .map(|(i, img)| {
                let color = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.pixels);
                ctx.load_texture(format!("cursor{i}"), color, egui::TextureOptions::NEAREST)
            })
            .collect();
        Some(Self { images, act })
    }

    /// Draws cursor `action` with its hotspot at `pos` (UI points), `time` seconds in.
    pub fn draw(&self, ctx: &egui::Context, pos: egui::Pos2, action: usize, time: f64) {
        let Some(a) = self.act.actions.get(action).or_else(|| self.act.actions.first()) else { return };
        if a.frames.is_empty() {
            return;
        }
        let frame = &a.frames[(time * 1000.0 / a.delay_ms.max(1.0) as f64) as usize % a.frames.len()];
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::unique("game cursor")));
        for l in &frame.layers {
            let Some(tex) = usize::try_from(l.sprite).ok().and_then(|i| self.images.get(i)) else { continue };
            let size = tex.ui_size();
            let min = pos + Vec2::new(l.x as f32, l.y as f32) - size / 2.0;
            let uv = if l.mirror { egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0)) } else { egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)) };
            painter.image(tex.id(), egui::Rect::from_min_size(min, size), uv, Color32::WHITE);
        }
        ctx.request_repaint();
    }
}

/// What the Esc game menu chose.
pub enum EscChoice {
    CharSelect,
    Options,
    Shortcuts,
    Keys,
    Exit,
    Back,
}

/// The Esc game menu, like the original client's: Character Select, Options, Shortcut
/// Information, Shortcut Settings, Exit, Return to Game.
pub fn esc_menu(ctx: &egui::Context) -> Option<EscChoice> {
    let mut choice = None;
    egui::Window::new("game menu")
        .title_bar(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(popup_frame(ctx))
        .show(ctx, |ui| {
                if crate::classic::skin(ui.ctx()).is_none() {
                    ui.style_mut().visuals = egui::Visuals::dark();
                }
            ui.set_width(200.0);
            ui.vertical_centered(|ui| {
                let button = |ui: &mut egui::Ui, text: &str| ui.add(egui::Button::new(RichText::new(text).size(15.0)).min_size(Vec2::new(190.0, 32.0))).clicked();
                if button(ui, "Character Select") {
                    choice = Some(EscChoice::CharSelect);
                }
                if button(ui, "Options") {
                    choice = Some(EscChoice::Options);
                }
                if button(ui, &crate::msgstr::get(3880, &[]).unwrap_or_else(|| "Shortcut Information".into())) {
                    choice = Some(EscChoice::Shortcuts);
                }
                if button(ui, &crate::msgstr::get(1494, &[]).unwrap_or_else(|| "Shortcut Settings".into())) {
                    choice = Some(EscChoice::Keys);
                }
                if button(ui, "Exit") {
                    choice = Some(EscChoice::Exit);
                }
                ui.add_space(4.0);
                if button(ui, "Return to Game") {
                    choice = Some(EscChoice::Back);
                }
            });
        });
    choice
}

#[cfg(test)]
mod chat_log_tests {
    use super::*;

    #[test]
    fn logged_lines_name_their_item_links() {
        let dir = std::env::temp_dir().join(format!("tenseiga-hudlog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut hud = Hud::default();
        hud.push_chat(ChatKind::Public, "before the log".into());
        hud.flush_log(|_| None);
        hud.log = Some(crate::chatlog::ChatLog::open(&dir, "Doofy").unwrap());
        hud.push_chat(ChatKind::Public, "Doofy : <ITEML>000020135'00)00)00)00)00</ITEML> for sale".into());
        hud.flush_log(|id| (id == 4035).then(|| ("Hydra Card".to_string(), 0)));
        let path = hud.log.as_ref().unwrap().path().to_path_buf();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.ends_with("] Doofy : <Hydra Card> for sale\n"), "{text}");
        assert!(!text.contains("before the log"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
