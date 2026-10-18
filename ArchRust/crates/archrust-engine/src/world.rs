//! Terrain : secteurs du monde chargés autour du joueur.
//!
//! Le monde est découpé en secteurs carrés de 6400 unités ; l'indice d'un secteur est sa
//! coordonnée divisée par 6400, décalée de 400 (vérifié : `D272,272` couvre x,z ∈ [-819200, -812800]).
//! Seize secteurs par axe forment une tuile `(sx/16)*100 + sy/16` :
//!
//! * `world/b%06dx.ma2` → `D{sx},{sy}.dff` : géométrie (1536 sommets, 512 triangles, 6 jeux d'UV) ;
//! * `world/a%06dx.ma2` → `R{sx},{sy}.dff` : dictionnaire de textures du secteur (128×128 DXT1) ;
//! * `world/a%06dx.ma1` → `C{sx},{sy}.amf` : grille d'attributs 16×16 (collision, zones).

use crate::assets::Assets;
use crate::render::{DrawItem, Mesh, Renderer};
use crate::scene::build_geometry_mesh;
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::magpack::MagPack;
use archrust_data::rw::{SectorGeometry, TextureDictionary};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SECTOR_SIZE: f32 = 6400.0;
pub const SECTOR_OFFSET: i32 = 400;

/// Indice de secteur d'une position monde.
pub fn sector_of(x: f32, z: f32) -> (i32, i32) {
    ((x / SECTOR_SIZE).floor() as i32 + SECTOR_OFFSET, (z / SECTOR_SIZE).floor() as i32 + SECTOR_OFFSET)
}

/// Numéro de tuile d'un secteur.
pub fn tile_of(sx: i32, sy: i32) -> u32 {
    ((sx / 16) * 100 + sy / 16) as u32
}

/// Origine monde (coin bas-gauche) d'un secteur.
pub fn sector_origin(sx: i32, sy: i32) -> Vec3 {
    Vec3::new((sx - SECTOR_OFFSET) as f32 * SECTOR_SIZE, 0.0, (sy - SECTOR_OFFSET) as f32 * SECTOR_SIZE)
}

pub struct Terrain {
    pub radius: i32,
    sectors: HashMap<(i32, i32), Arc<Mesh>>,
    failed: HashSet<(i32, i32)>,
    tiles_a: HashMap<u32, Option<Arc<MagPack>>>,
    tiles_b: HashMap<u32, Option<Arc<MagPack>>>,
    pub loads_per_frame: usize,
    pub last_center: Option<(i32, i32)>,
}

impl Default for Terrain {
    fn default() -> Self {
        Self::new(3)
    }
}

impl Terrain {
    pub fn new(radius: i32) -> Self {
        Self { radius, sectors: HashMap::new(), failed: HashSet::new(), tiles_a: HashMap::new(), tiles_b: HashMap::new(), loads_per_frame: 2, last_center: None }
    }

    pub fn loaded_count(&self) -> usize {
        self.sectors.len()
    }

    fn tile(&mut self, assets: &Assets, prefix: char, tile: u32) -> Option<Arc<MagPack>> {
        let cache = if prefix == 'a' { &mut self.tiles_a } else { &mut self.tiles_b };
        if let Some(t) = cache.get(&tile) {
            return t.clone();
        }
        let loaded = match assets.magpack(prefix, tile, "ma2") {
            Ok(p) => Some(Arc::new(p)),
            Err(e) => {
                log::debug!("tuile {prefix}{tile:06}x.ma2 : {e}");
                None
            }
        };
        cache.insert(tile, loaded.clone());
        loaded
    }

    fn load_sector(&mut self, renderer: &mut Renderer, assets: &Assets, sx: i32, sy: i32) -> Option<Arc<Mesh>> {
        let tile = tile_of(sx, sy);
        let b = self.tile(assets, 'b', tile)?;
        let name = format!("D{sx},{sy}.dff");
        let plain = match b.unpack_named(&name)? {
            Ok(p) => p,
            Err(e) => {
                log::warn!("secteur {name} : {e}");
                return None;
            }
        };
        let sector = match SectorGeometry::parse(&plain) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("secteur {name} : {e}");
                return None;
            }
        };
        // Texture propre au secteur (dictionnaire R) : téléversée sous le nom du secteur.
        let r_name = format!("R{sx},{sy}");
        if renderer.texture(&r_name).is_none() {
            if let Some(a) = self.tile(assets, 'a', tile) {
                if let Some(Ok(txd_plain)) = a.unpack_named(&format!("{r_name}.dff")) {
                    let txd_plain = if txd_plain.len() > 4 && txd_plain[0] == 0x16 { txd_plain } else { capi_crypt(KEY_TEXTURE, &txd_plain) };
                    if let Ok(dict) = TextureDictionary::parse(&txd_plain) {
                        if let Some(t) = dict.textures.first() {
                            if let Ok(rgba) = t.to_rgba8() {
                                renderer.upload_texture(&r_name, t.mips[0].width, t.mips[0].height, &rgba);
                            }
                        }
                    }
                }
            }
        }
        let mut geometry = sector.geometry;
        // Le matériau sans texture reçoit la texture du secteur (carte de couleurs).
        for m in geometry.materials.iter_mut() {
            if m.texture.is_none() && renderer.texture(&r_name).is_some() {
                m.texture = Some(archrust_data::rw::Texture { name: r_name.clone(), ..Default::default() });
            }
        }
        let mesh = Arc::new(build_geometry_mesh(renderer, assets, &geometry));
        Some(mesh)
    }

    /// Charge progressivement les secteurs autour de `center`, décharge les lointains.
    pub fn update(&mut self, renderer: &mut Renderer, assets: &Assets, center: Vec3) {
        let (cx, cy) = sector_of(center.x, center.z);
        if self.last_center != Some((cx, cy)) {
            self.last_center = Some((cx, cy));
            let r = self.radius + 2;
            self.sectors.retain(|&(sx, sy), _| (sx - cx).abs() <= r && (sy - cy).abs() <= r);
            self.failed.retain(|&(sx, sy)| (sx - cx).abs() <= r && (sy - cy).abs() <= r);
        }
        let mut budget = self.loads_per_frame;
        // Du plus proche au plus lointain.
        let mut wanted: Vec<(i32, i32)> = Vec::new();
        for dy in -self.radius..=self.radius {
            for dx in -self.radius..=self.radius {
                wanted.push((cx + dx, cy + dy));
            }
        }
        wanted.sort_by_key(|&(sx, sy)| (sx - cx).abs().max((sy - cy).abs()));
        for (sx, sy) in wanted {
            if budget == 0 {
                break;
            }
            if self.sectors.contains_key(&(sx, sy)) || self.failed.contains(&(sx, sy)) {
                continue;
            }
            match self.load_sector(renderer, assets, sx, sy) {
                Some(mesh) => {
                    self.sectors.insert((sx, sy), mesh);
                }
                None => {
                    self.failed.insert((sx, sy));
                }
            }
            budget -= 1;
        }
    }

    pub fn draw_items(&self) -> Vec<DrawItem> {
        self.sectors.values().map(|m| DrawItem { mesh: m.clone(), model: Mat4::IDENTITY, tint: [1.0; 4] }).collect()
    }

    /// Hauteur approximative du terrain sous (x, z) : plus haut sommet du secteur proche du point.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let key = sector_of(x, z);
        let mesh = self.sectors.get(&key)?;
        Some(mesh.bounds_center.y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping() {
        assert_eq!(sector_of(-819200.0, -819200.0), (272, 272));
        assert_eq!(sector_of(-812800.1, -812800.1), (272, 272));
        assert_eq!(sector_of(-475955.0, -66219.0), (325, 389));
        assert_eq!(tile_of(325, 389), 2024);
        assert_eq!(sector_origin(272, 272), Vec3::new(-819200.0, 0.0, -819200.0));
    }
}
