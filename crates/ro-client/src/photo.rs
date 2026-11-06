//! Photo mode: the world holds still, the interface steps aside, and the camera goes where
//! it likes (further out, closer in, lower and higher than in play) with the look's knobs to
//! hand, for screenshots. Ctrl+P (Shortcut Settings) goes in and out; Esc leaves. The
//! character can be posed: any of its actions (an attack swing, the battle stance with the
//! weapon drawn, casting, sitting...) held on a chosen frame, facing a chosen way.

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
    /// The character's pose.
    pub posing: Posing,
}

/// The actions a character can be posed in: (name, ACT action group). Standing and sitting
/// have no animation; their three frames are the head turned straight, right and left.
/// "Attack" is the swing the character's weapon is drawn in (`ATTACK`); the numbered three
/// are all of them (a weapon shows in only one).
pub const POSES: [(&str, usize); 14] = [
    ("Standing", 0),
    ("Walking", 1),
    ("Sitting", 2),
    ("Battle stance", 4),
    ("Attack", ATTACK),
    ("Casting", 12),
    ("Picking up", 3),
    ("Hurt", 6),
    ("Fallen", 8),
    ("Frozen", 7),
    ("Frozen 2", 9),
    ("Attack 1", 5),
    ("Attack 2", 10),
    ("Attack 3", 11),
];

/// In `POSES`: the weapon's own attack motion (given to `Posing::apply`).
const ATTACK: usize = usize::MAX;

/// `--pose name,frame,facing` (frame from 1, facing a sprite direction 0-7, 0 toward the
/// camera): (index into `POSES`, frame from 0, facing).
pub fn parse_pose(arg: &str) -> Option<(usize, usize, usize)> {
    let mut parts = arg.split(',').map(str::trim);
    let name = parts.next()?.to_lowercase();
    let pose = POSES.iter().position(|(n, _)| n.to_lowercase() == name)?;
    let frame = parts.next().and_then(|f| f.parse::<usize>().ok()).unwrap_or(1).max(1) - 1;
    let facing = parts.next().and_then(|f| f.parse::<usize>().ok()).unwrap_or(0) % 8;
    Some((pose, frame, facing))
}

/// The ways a character can face, as seen from the camera (the sprite directions).
const FACINGS: [&str; 8] = ["Toward the camera", "Front left", "Left", "Back left", "Away", "Back right", "Right", "Front right"];

/// Posing the character in photo mode: off, it is drawn as in play (standing still, as the
/// world is); on, in the chosen action, held on one frame or playing.
pub struct Posing {
    pub on: bool,
    /// Into `POSES`.
    pub pose: usize,
    /// The frame held (or for standing and sitting, the head turn).
    pub frame: usize,
    /// The action plays (looping) rather than holding a frame.
    pub play: bool,
    /// Into `FACINGS` (a sprite direction).
    pub facing: usize,
    /// Frames in the chosen action, as last seen (for the slider).
    pub frames: usize,
    /// Time into the playing action (real time: the world itself stands still).
    pub clock_ms: f32,
}

impl Default for Posing {
    fn default() -> Self {
        // (Battle stance: the weapon drawn, the pose most asked for.)
        Self { on: false, pose: 3, frame: 0, play: false, facing: 0, frames: 1, clock_ms: 0.0 }
    }
}

impl Posing {
    /// Whether the pose's frames are head turns rather than an animation.
    fn head_turns(&self) -> bool {
        matches!(POSES[self.pose].1, 0 | 2)
    }

    /// Poses `a` (the character as play would draw it). `attack` is the action group its
    /// weapon swings in; `frames(index)` gives the frame count and per-frame time of an ACT
    /// action index (action * 8 + direction) for its body.
    pub fn apply(&mut self, a: &mut ro_render::Actor, attack: usize, frames: impl Fn(usize) -> (usize, f32)) {
        if !self.on {
            // (Posing starts facing as the character does.)
            self.facing = a.dir as usize;
            return;
        }
        let action = match POSES[self.pose].1 {
            ATTACK => attack,
            g => g,
        };
        a.action = action;
        a.dir = ro_render::Dir::from_index(self.facing);
        if self.head_turns() {
            self.frames = 3;
            self.frame = self.frame.min(2);
            a.head_dir = self.frame as u8;
            a.time_ms = 0.0;
            a.once = false;
            return;
        }
        a.head_dir = 0;
        let (n, delay) = frames(action * 8 + self.facing);
        self.frames = n.max(1);
        self.frame = self.frame.min(self.frames - 1);
        if self.play {
            a.time_ms = self.clock_ms;
            a.once = false;
        } else {
            // (Midway into the frame, so rounding never shows its neighbour.)
            a.time_ms = (self.frame as f32 + 0.5) * delay;
            a.once = true;
        }
    }

    /// The pose's part of the panel.
    fn show(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.on, "Pose the character");
        if !self.on {
            return;
        }
        egui::Grid::new("pose").num_columns(2).show(ui, |ui| {
            ui.label("Action");
            egui::ComboBox::from_id_salt("pose action").selected_text(POSES[self.pose].0).show_ui(ui, |ui| {
                for (i, (name, _)) in POSES.iter().enumerate() {
                    if ui.selectable_value(&mut self.pose, i, *name).changed() {
                        self.frame = 0;
                        self.clock_ms = 0.0;
                    }
                }
            });
            ui.end_row();
            if self.head_turns() {
                ui.label("Head");
                ui.horizontal(|ui| {
                    for (i, name) in ["Straight", "Right", "Left"].iter().enumerate() {
                        ui.selectable_value(&mut self.frame, i, *name);
                    }
                });
            } else {
                ui.label("Frame");
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!self.play, |ui| {
                        let mut shown = self.frame + 1;
                        let n = self.frames.max(1);
                        if ui.add(egui::Slider::new(&mut shown, 1..=n).suffix(format!(" of {n}"))).changed() {
                            self.frame = shown - 1;
                        }
                    });
                    ui.checkbox(&mut self.play, "Play");
                });
            }
            ui.end_row();
            ui.label("Facing");
            ui.horizontal(|ui| {
                if ui.button("<").on_hover_text("Turn left").clicked() {
                    self.facing = (self.facing + 7) % 8;
                }
                if ui.button(">").on_hover_text("Turn right").clicked() {
                    self.facing = (self.facing + 1) % 8;
                }
                ui.label(FACINGS[self.facing]);
            });
            ui.end_row();
        });
    }
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
            posing: Posing::default(),
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
        self.posing.clock_ms += dt_ms;
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
            self.posing.show(ui);
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

    fn actor() -> ro_render::Actor {
        ro_render::Actor {
            pos: glam::Vec3::ZERO,
            body: ro_render::SheetId::NONE,
            head: Some(ro_render::SheetId::NONE),
            headgear: vec![],
            weapon: vec![],
            shadow: None,
            action: 0,
            dir: ro_render::Dir::W,
            head_dir: 1,
            time_ms: 1234.0,
            once: false,
            alpha: 1.0,
            scale: 1.0,
            tint: [1.0; 3],
            silhouette: None,
        }
    }

    #[test]
    fn a_pose_holds_the_chosen_frame_of_the_chosen_action_facing_the_chosen_way() {
        let mut p = Posing::default();
        let frames = |index: usize| if index / 8 == 10 { (6, 100.0) } else { (4, 80.0) };
        // Off: drawn as in play, and posing would start facing the same way.
        let mut a = actor();
        p.apply(&mut a, 10, frames);
        assert_eq!((a.action, a.time_ms, p.facing), (0, 1234.0, ro_render::Dir::W as usize));
        // Attack (the weapon swings in group 10), its fourth frame, facing away.
        p.on = true;
        p.pose = POSES.iter().position(|(n, _)| *n == "Attack").unwrap();
        p.frame = 3;
        p.facing = ro_render::Dir::N as usize;
        p.apply(&mut a, 10, frames);
        assert_eq!((a.action, a.dir, a.head_dir), (10, ro_render::Dir::N, 0));
        assert_eq!((a.time_ms / 100.0) as usize, 3, "the fourth frame");
        assert!(a.once, "held");
        // A frame past the end is pulled back to the last.
        p.frame = 99;
        p.apply(&mut a, 10, frames);
        assert_eq!((p.frame, (a.time_ms / 100.0) as usize), (5, 5));
        // Playing: the action runs on real time.
        p.play = true;
        p.clock_ms = 250.0;
        p.apply(&mut a, 10, frames);
        assert_eq!((a.time_ms, a.once), (250.0, false));
        // Sitting: the frames are head turns.
        p.pose = POSES.iter().position(|(n, _)| *n == "Sitting").unwrap();
        p.frame = 2;
        p.apply(&mut a, 10, frames);
        assert_eq!((a.action, a.head_dir), (2, 2));
        // A katar swings in group 11.
        p.pose = POSES.iter().position(|(n, _)| *n == "Attack").unwrap();
        p.apply(&mut a, 11, frames);
        assert_eq!(a.action, 11);
    }

    #[test]
    fn the_pose_flag_names_an_action_its_frame_and_facing() {
        let attack = POSES.iter().position(|(n, _)| *n == "Attack").unwrap();
        assert_eq!(parse_pose("attack,3,4"), Some((attack, 2, 4)));
        assert_eq!(parse_pose("Battle stance"), Some((3, 0, 0)));
        assert_eq!(parse_pose("dancing"), None);
    }
}
