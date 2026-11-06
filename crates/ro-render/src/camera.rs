use glam::{camera::rh, Mat4, Vec3};

/// World units per GAT cell (a GND tile is 2x2 cells).
pub const CELL: f32 = 5.0;

/// RO-style orbit camera. The vertical FOV is fixed, so wider aspect ratios
/// (21:9, 32:9) reveal more horizontally instead of stretching or cropping.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub target: Vec3,
    /// Radians around +Y.
    pub yaw: f32,
    /// Radians above the horizon.
    pub pitch: f32,
    pub distance: f32,
    pub fovy: f32,
    /// Photo mode: zoom and tilt go well past play's limits.
    pub free: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self { target: Vec3::ZERO, yaw: 0.0, pitch: 50f32.to_radians(), distance: 300.0, fovy: 20f32.to_radians(), free: false }
    }
}

impl Camera {
    pub const MIN_DISTANCE: f32 = 120.0;
    pub const MAX_DISTANCE: f32 = 650.0;

    /// Centers on a cell, the way the client follows the player.
    pub fn look_at_cell(&mut self, x: u16, y: u16) {
        self.target = Vec3::new((x as f32 + 0.5) * CELL, 0.0, (y as f32 + 0.5) * CELL);
    }

    pub fn zoom(&mut self, steps: f32) {
        let (lo, hi) = if self.free { (30.0, 1600.0) } else { (Self::MIN_DISTANCE, Self::MAX_DISTANCE) };
        self.distance = (self.distance * 0.9f32.powf(steps)).clamp(lo, hi);
    }

    pub fn rotate(&mut self, d_yaw: f32, d_pitch: f32) {
        self.yaw = (self.yaw + d_yaw).rem_euclid(std::f32::consts::TAU);
        let (lo, hi) = if self.free { (4f32, 89f32) } else { (25f32, 75f32) };
        self.pitch = (self.pitch + d_pitch).clamp(lo.to_radians(), hi.to_radians());
    }

    pub fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(sy * cp, sp, -cy * cp) * self.distance
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        // wgpu clip space is y-up with depth in 0..1, i.e. the D3D convention.
        let view = rh::view::look_at_mat4(self.eye(), self.target, Vec3::Y);
        let proj = rh::proj::directx::perspective(self.fovy, aspect.max(0.01), 10.0, 5000.0);
        // World X = east and Z = north (RO map axes) is a left-handed basis; mirror screen X
        // so east is on the right with the default north-up view.
        Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)) * proj * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_projects_to_center_at_any_aspect() {
        let mut c = Camera::default();
        c.look_at_cell(156, 191);
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0, 32.0 / 9.0] {
            let p = c.view_proj(aspect).project_point3(c.target);
            assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4, "{aspect}: {p}");
        }
    }

    #[test]
    fn north_up_east_right() {
        let mut c = Camera::default();
        c.look_at_cell(100, 100);
        let vp = c.view_proj(16.0 / 9.0);
        let east = vp.project_point3(c.target + Vec3::X * 20.0);
        let north = vp.project_point3(c.target + Vec3::Z * 20.0);
        assert!(east.x > 0.0, "east should be right of center: {east}");
        assert!(north.y > 0.0, "north should be above center: {north}");
    }

    #[test]
    fn wider_aspect_keeps_vertical_extent() {
        let c = Camera::default();
        let probe = c.target + Vec3::new(0.0, 0.0, 30.0);
        let a = c.view_proj(16.0 / 9.0).project_point3(probe);
        let b = c.view_proj(32.0 / 9.0).project_point3(probe);
        assert!((a.y - b.y).abs() < 1e-5);
    }
}
