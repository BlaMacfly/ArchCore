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
    /// Longueur du bras laissée libre par le terrain (voir [`OrbitCamera::collide`]) ; infinie
    /// quand rien ne l'a raccourci.
    pub collision: f32,
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
            collision: f32::INFINITY,
        }
    }
}

impl OrbitCamera {
    /// Direction de la cible vers l'œil.
    fn direction(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
    }

    pub fn eye(&self) -> Vec3 {
        self.target + self.direction() * self.distance.min(self.collision)
    }

    /// Raccourcit le bras quand le terrain passe entre la cible et l'œil, puis le rallonge en
    /// douceur une fois la vue dégagée. Logique reprise de benilla (`player/camera.rs`,
    /// `seat_camera`, MIT/Apache-2.0) : rentrée immédiate, sortie exponentielle à 6 par seconde.
    /// Seul le terrain est testé (`ground` rend sa hauteur en x, z), pas les objets.
    pub fn collide(&mut self, dt: f32, ground: impl Fn(f32, f32) -> Option<f32>) {
        const STEPS: usize = 48;
        const MARGIN: f32 = 40.0;
        const RETURN_RATE: f32 = 6.0;
        let dir = self.direction();
        let blocked = |d: f32| {
            let p = self.target + dir * d;
            ground(p.x, p.z).is_some_and(|h| p.y < h + MARGIN)
        };
        let mut open = self.distance;
        let step = self.distance / STEPS as f32;
        if let Some(i) = (1..=STEPS).find(|i| blocked(step * *i as f32)) {
            // Dichotomie entre le dernier point libre et le premier point sous le terrain.
            let (mut lo, mut hi) = (step * (i - 1) as f32, step * i as f32);
            for _ in 0..8 {
                let mid = (lo + hi) / 2.0;
                if blocked(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            open = lo.max(self.near * 2.0);
        }
        self.collision = if open < self.collision {
            open
        } else {
            let t = 1.0 - (-RETURN_RATE * dt).exp();
            self.collision + (open - self.collision) * t
        };
        if self.collision >= self.distance - 0.5 {
            self.collision = f32::INFINITY;
        }
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
        // Un zoom avant part de la distance réellement vue, pas de celle demandée.
        if delta > 0.0 && self.collision < self.distance {
            self.distance = self.collision;
        }
        self.distance = (self.distance * (1.0 - delta * 0.1)).clamp(100.0, 50_000.0);
    }

    /// Déplace la cible dans le plan horizontal, relativement à l'orientation de la caméra.
    pub fn pan(&mut self, forward: f32, right: f32) {
        let f = Vec3::new(-self.yaw.cos(), 0.0, -self.yaw.sin());
        let r = Vec3::new(-f.z, 0.0, f.x);
        self.target += f * forward + r * right;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terrain_shortens_the_boom_then_releases_it() {
        let mut cam = OrbitCamera {
            target: Vec3::new(0.0, 100.0, 0.0),
            yaw: 0.0,
            pitch: 0.2,
            distance: 1500.0,
            ..OrbitCamera::default()
        };
        // Colline à partir de x = 600 : l'œil (x ≈ 1470) serait dessous.
        cam.collide(0.016, |x, _| Some(if x > 600.0 { 1000.0 } else { 0.0 }));
        let d = cam.collision;
        assert!(d < 700.0 && d > 500.0, "bras raccourci : {d}");
        // Terrain plat : le bras revient progressivement, puis entièrement.
        cam.collide(0.016, |_, _| Some(0.0));
        assert!(cam.collision > d && cam.collision < 1500.0);
        for _ in 0..300 {
            cam.collide(0.016, |_, _| Some(0.0));
        }
        assert!(cam.collision.is_infinite());
        assert!((cam.eye() - cam.target).length() > 1499.0);
    }
}
