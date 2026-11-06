//! Photo mode: the world holds still, the interface steps aside, and the camera goes where
//! it likes (further out, closer in, lower and higher than in play) with the look's knobs to
//! hand, for screenshots. Ctrl+P (Shortcut Settings) goes in and out; Esc leaves.

use std::collections::HashSet;

use winit::keyboard::{Key, NamedKey};

pub struct Photo {
    /// Where the camera looks (it no longer follows the player).
    pub target: glam::Vec3,
    /// Keys held for moving it.
    held: HashSet<Move>,
    /// The panel on show (H hides it, for a clear view while framing).
    pub panel: bool,
    /// The look while in photo mode.
    pub dof: f32,
    pub exposure: f32,
    pub saturation: f32,
    pub warmth: f32,
    pub vignette: f32,
    /// Field of view (degrees; the game's is 20).
    pub fov: f32,
    /// The camera as it was, to go back to.
    was: ro_render::Camera,
    /// The panel's Leave was pressed (handled on the next tick).
    pub leaving: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Move {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
}

impl Photo {
    /// Into photo mode from the camera as it is and the look as Settings has it.
    pub fn new(camera: ro_render::Camera, look: &ro_render::PostSettings) -> Self {
        Self {
            target: camera.target,
            held: HashSet::new(),
            panel: true,
            dof: look.dof_strength,
            exposure: look.exposure,
            saturation: look.saturation,
            warmth: look.warmth,
            vignette: look.vignette,
            fov: camera.fovy.to_degrees(),
            was: camera,
            leaving: false,
        }
    }

    /// The camera to go back to on leaving.
    pub fn camera_before(&self) -> ro_render::Camera {
        self.was
    }

    /// A key pressed or let go: true if photo mode took it (moving, H for the panel).
    pub fn key(&mut self, key: &Key, pressed: bool) -> bool {
        let m = match key {
            Key::Character(c) => match c.to_lowercase().as_str() {
                "w" => Move::Forward,
                "s" => Move::Back,
                "a" => Move::Left,
                "d" => Move::Right,
                "e" => Move::Up,
                "q" => Move::Down,
                "h" => {
                    if pressed {
                        self.panel = !self.panel;
                    }
                    return true;
                }
                _ => return false,
            },
            Key::Named(NamedKey::ArrowUp) => Move::Forward,
            Key::Named(NamedKey::ArrowDown) => Move::Back,
            Key::Named(NamedKey::ArrowLeft) => Move::Left,
            Key::Named(NamedKey::ArrowRight) => Move::Right,
            Key::Named(NamedKey::PageUp) => Move::Up,
            Key::Named(NamedKey::PageDown) => Move::Down,
            _ => return false,
        };
        if pressed {
            self.held.insert(m);
        } else {
            self.held.remove(&m);
        }
        true
    }

    /// Moves the camera's target by the keys held over `dt_ms`: along the ground as the
    /// camera faces (faster the further out it is), and up and down.
    pub fn fly(&mut self, dt_ms: f32, cam: &mut ro_render::Camera) {
        let (sy, cy) = cam.yaw.sin_cos();
        // Away from the camera along the ground, and to its right on screen.
        let ahead = glam::Vec3::new(-sy, 0.0, cy);
        let right = glam::Vec3::new(cy, 0.0, sy);
        let mut dir = glam::Vec3::ZERO;
        for m in &self.held {
            dir += match m {
                Move::Forward => ahead,
                Move::Back => -ahead,
                Move::Left => -right,
                Move::Right => right,
                Move::Up => glam::Vec3::Y,
                Move::Down => -glam::Vec3::Y,
            };
        }
        let speed = cam.distance * 0.8 / 1000.0;
        self.target += dir.normalize_or_zero() * speed * dt_ms;
        cam.target = self.target;
        cam.fovy = self.fov.to_radians();
    }

    /// The look with photo mode's knobs.
    pub fn look(&self, mut p: ro_render::PostSettings) -> ro_render::PostSettings {
        p.dof_strength = self.dof;
        p.exposure = self.exposure;
        p.saturation = self.saturation;
        p.warmth = self.warmth;
        p.vignette = self.vignette;
        p
    }

    /// The panel: the knobs, and what the keys do. What was asked for.
    pub fn show(&mut self, ctx: &egui::Context) -> Ask {
        let mut ask = Ask::None;
        if !self.panel {
            return ask;
        }
        egui::Window::new("Photo mode").anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0]).resizable(false).collapsible(false).show(ctx, |ui| {
            egui::Grid::new("photo").num_columns(2).show(ui, |ui| {
                ui.label("Depth of field");
                ui.add(egui::Slider::new(&mut self.dof, 0.0..=1.0));
                ui.end_row();
                ui.label("Exposure");
                ui.add(egui::Slider::new(&mut self.exposure, 0.3..=2.5));
                ui.end_row();
                ui.label("Saturation");
                ui.add(egui::Slider::new(&mut self.saturation, 0.0..=2.0));
                ui.end_row();
                ui.label("Warmth");
                ui.add(egui::Slider::new(&mut self.warmth, -1.0..=1.0));
                ui.end_row();
                ui.label("Vignette");
                ui.add(egui::Slider::new(&mut self.vignette, 0.0..=1.0));
                ui.end_row();
                ui.label("Field of view");
                ui.add(egui::Slider::new(&mut self.fov, 8.0..=60.0).suffix("°"));
                ui.end_row();
            });
            ui.separator();
            ui.label("WASD or arrows move, Q and E down and up; right-drag turns, the wheel zooms");
            ui.label("H hides this panel; Print Screen or the button saves the picture");
            ui.horizontal(|ui| {
                if ui.button("Save photo").clicked() {
                    ask = Ask::Save;
                }
                if ui.button("Leave (Esc)").clicked() {
                    ask = Ask::Leave;
                }
            });
        });
        ask
    }
}

/// What the photo panel asked for.
pub enum Ask {
    None,
    Save,
    Leave,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_move_the_camera_and_letting_go_stops_it() {
        let mut cam = ro_render::Camera::default();
        let mut p = Photo::new(cam, &ro_render::PostSettings::default());
        assert!(p.key(&Key::Character("w".into()), true));
        p.fly(1000.0, &mut cam);
        assert!(cam.target.length() > 1.0, "moved");
        let at = cam.target;
        p.key(&Key::Character("w".into()), false);
        p.fly(1000.0, &mut cam);
        assert_eq!(cam.target, at, "stopped");
        // Other keys are not photo mode's.
        assert!(!p.key(&Key::Character("z".into()), true));
        assert!(p.key(&Key::Character("h".into()), true) && !p.panel);
    }
}
