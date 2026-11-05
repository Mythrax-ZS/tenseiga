//! Full-screen art from the client data, as the original shows it: the age-rating splash
//! at start-up, the title illustration behind the login and character screens, and a
//! random loading screen (texture\유저인터페이스\loadingNN.jpg) with a progress bar while
//! a map loads.

use crate::uitex::UiSize;
use egui::{Color32, Pos2, Rect, Vec2};

const UI: &str = "data\\texture\\유저인터페이스\\";
/// How long the age-rating splash stays, and the fades around screens (seconds).
const SPLASH_SECS: f64 = 2.0;
const FADE_SECS: f64 = 0.4;

pub struct Splash {
    title: Option<egui::TextureHandle>,
    grade: Option<egui::TextureHandle>,
    loading_names: Vec<String>,
    /// The loading screen, when it was first drawn, and when the map was in.
    loading: Option<(egui::TextureHandle, Option<f64>, Option<f64>)>,
    /// Why the map didn't load, shown on the loading screen (it then stays up).
    failed: Option<String>,
    started: Option<f64>,
}

fn load(ctx: &egui::Context, vfs: &ro_vfs::Vfs, path: &str) -> Option<egui::TextureHandle> {
    crate::uitex::load(ctx, vfs, path, egui::TextureOptions::LINEAR)
}

/// Draws `tex` whole and undistorted, centred (4:3 art on wide screens keeps its shape);
/// the bars beside it show a dim, enlarged copy instead of plain black.
fn cover(ctx: &egui::Context, tex: &egui::TextureHandle, alpha: f32) {
    let screen = ctx.content_rect();
    let size = tex.ui_size();
    let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    let painter = ctx.layer_painter(egui::LayerId::background());
    painter.rect_filled(screen, 0.0, Color32::BLACK);
    let a = alpha.clamp(0.0, 1.0);
    let fill = (screen.width() / size.x).max(screen.height() / size.y);
    painter.image(tex.id(), Rect::from_center_size(screen.center(), size * fill), uv, Color32::from_gray((a * 60.0) as u8));
    let fit = (screen.width() / size.x).min(screen.height() / size.y);
    painter.image(tex.id(), Rect::from_center_size(screen.center(), size * fit), uv, Color32::from_white_alpha((a * 255.0) as u8));
}

impl Splash {
    pub fn new(ctx: &egui::Context, vfs: Option<&ro_vfs::Vfs>) -> Self {
        let Some(vfs) = vfs else {
            return Self { title: None, grade: None, loading_names: vec![], loading: None, failed: None, started: None };
        };
        // The original ships up to 36 loading screens; use whichever exist.
        let loading_names = (0..40).map(|i| format!("{UI}loading{i:02}.jpg")).filter(|p| vfs.read(p).ok().flatten().is_some()).collect();
        Self {
            title: load(ctx, vfs, &format!("{UI}bgi_temp.bmp")),
            grade: load(ctx, vfs, &format!("{UI}t_gamegrade.tga")),
            loading_names,
            loading: None,
            failed: None,
            started: None,
        }
    }

    /// True while the start-up splash is still showing (input waits).
    pub fn intro(&mut self, ctx: &egui::Context) -> bool {
        let now = ctx.input(|i| i.time);
        let t = now - *self.started.get_or_insert(now);
        if t >= SPLASH_SECS || self.grade.is_none() {
            return false;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("intro")));
        painter.rect_filled(screen, 0.0, Color32::BLACK);
        if let Some(g) = &self.grade {
            // Fade in, hold, fade out.
            let a = (t / FADE_SECS).min(1.0).min((SPLASH_SECS - t) / FADE_SECS) as f32;
            let size = g.ui_size() * (screen.height() / 540.0).max(1.0);
            let rect = Rect::from_center_size(screen.center(), size);
            painter.image(g.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha((a * 255.0) as u8));
        }
        ctx.request_repaint();
        true
    }

    /// The title illustration behind the login and character screens.
    pub fn title(&self, ctx: &egui::Context) {
        if let Some(t) = &self.title {
            cover(ctx, t, 1.0);
        }
    }

    /// Picks a random loading screen (call when a map starts loading).
    pub fn begin_loading(&mut self, ctx: &egui::Context, vfs: Option<&ro_vfs::Vfs>) {
        let Some(vfs) = vfs else { return };
        if self.loading_names.is_empty() {
            return;
        }
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
        let path = &self.loading_names[seed % self.loading_names.len()];
        if let Some(tex) = load(ctx, vfs, path) {
            self.loading = Some((tex, None, None));
            self.failed = None;
        }
    }

    /// The map didn't load: say why on the loading screen.
    pub fn fail(&mut self, why: String) {
        self.failed = Some(why);
    }

    /// Draws the loading screen (`done` once the map is in; `loaded`, 0..1, how far the map
    /// loader is). Returns false once it has faded away.
    pub fn loading(&mut self, ctx: &egui::Context, done: bool, loaded: f32) -> bool {
        // A map that failed to load is as done as it will get: fade, so the game can be used to leave.
        let done = done || self.failed.is_some();
        let Some((tex, since, done_at)) = &mut self.loading else { return false };
        let now = ctx.input(|i| i.time);
        // Timed from its first frame (a map loaded on the frame loop is in before that).
        let since = *since.get_or_insert(now);
        let done_at = if done { Some(*done_at.get_or_insert(now)) } else { None };
        // The bar follows the loader; once the map is in it fills over the screen's minimum
        // showing time (at once, after a slow load).
        let following = 0.1 + 0.8 * loaded.clamp(0.0, 1.0);
        let progress = if done { following.max(((now - since) / 0.6) as f32) } else { following };
        // Stay at least a moment so it doesn't just flash, then fade out once loaded.
        let fade_from = done_at.map(|d| d.max(since + 0.6));
        let alpha = match fade_from {
            Some(f) if now > f => 1.0 - ((now - f) / FADE_SECS) as f32,
            _ => 1.0,
        };
        if alpha <= 0.0 {
            self.loading = None;
            return false;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("loading screen")));
        let size = tex.ui_size();
        let scale = (screen.width() / size.x).min(screen.height() / size.y);
        let rect = Rect::from_center_size(screen.center(), size * scale);
        let a = (alpha * 255.0) as u8;
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(a));
        painter.image(tex.id(), rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::from_white_alpha(a));
        // Progress bar along the bottom of the art, like the original.
        let bar = Rect::from_min_size(Pos2::new(rect.left() + rect.width() * 0.1, rect.bottom() - rect.height() * 0.06), Vec2::new(rect.width() * 0.8, 10.0));
        painter.rect_filled(bar.expand(2.0), 3.0, Color32::from_black_alpha((a as f32 * 0.7) as u8));
        let mut fill = bar;
        fill.set_width(bar.width() * progress.clamp(0.0, 1.0));
        painter.rect_filled(fill, 2.0, Color32::from_rgba_unmultiplied(120, 190, 255, a));
        if let Some(why) = &self.failed {
            let at = Pos2::new(bar.center().x, bar.top() - 14.0);
            painter.text(at + Vec2::splat(1.0), egui::Align2::CENTER_BOTTOM, why, egui::FontId::proportional(16.0), Color32::BLACK);
            painter.text(at, egui::Align2::CENTER_BOTTOM, why, egui::FontId::proportional(16.0), Color32::from_rgb(255, 120, 110));
        }
        ctx.request_repaint();
        true
    }
}
