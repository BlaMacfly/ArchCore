//! Caméra orbitale autour d'une cible (le personnage), façon MMO.
//!
//! Le monde ArchLord utilise des coordonnées en centimètres-ish (positions de l'ordre de 10⁵),
//! Y vers le haut ; on garde ce repère.

use glam::{Mat4, Vec3};

#[derive(Debug, Clone)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            yaw: 0.6,
            pitch: 0.45,
            distance: 1500.0,
            fov_y: 55f32.to_radians(),
            near: 10.0,
            far: 200_000.0,
        }
    }
}

impl OrbitCamera {
    pub fn eye(&self) -> Vec3 {
        let dir = Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        );
        self.target + dir * self.distance
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Y)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, aspect.max(0.01), self.near, self.far)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    pub fn rotate(&mut self, dx: f32, dy: f32) {
        self.yaw += dx * 0.005;
        self.pitch = (self.pitch + dy * 0.005).clamp(-1.2, 1.5);
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (1.0 - delta * 0.1)).clamp(100.0, 50_000.0);
    }

    /// Déplace la cible dans le plan horizontal, relativement à l'orientation de la caméra.
    pub fn pan(&mut self, forward: f32, right: f32) {
        let f = Vec3::new(-self.yaw.cos(), 0.0, -self.yaw.sin());
        let r = Vec3::new(-f.z, 0.0, f.x);
        self.target += f * forward + r * right;
    }
}
