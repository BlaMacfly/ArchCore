//! Terrain : secteurs du monde chargés autour du joueur.
//!
//! Le monde est découpé en secteurs carrés de 6400 unités ; l'indice d'un secteur est sa
//! coordonnée divisée par 6400, décalée de 400 (vérifié : `D272,272` couvre x,z ∈ [-819200, -812800]).
//! Seize secteurs par axe forment une tuile `(sx/16)*100 + sy/16` :
//!
//! * `world/b%06dx.ma2` → `D{sx},{sy}.dff` : géométrie (1536 sommets, 512 triangles, 6 jeux d'UV),
//!   matériaux multi-couches (extension `0xfffff001`) dessinés par `terrain.wgsl` ;
//! * `world/a%06dx.ma2` → `R{sx},{sy}.dff` : dictionnaire de textures du secteur (128×128 DXT1) ;
//! * `world/a%06dx.ma1` → `C{sx},{sy}.amf` : grille d'attributs 16×16 (collision, zones).
//!
//! Un patch peut remplacer un secteur entier par un glTF (`[[models]] match = "world/D272,272.dff"`),
//! posé à l'origine du secteur ; la hauteur du sol est alors tirée de ses sommets.

use crate::assets::Assets;
use crate::render::{DrawItem, Mesh, Renderer, TerrainMesh, TerrainVertex, TERRAIN_STAGES};
use crate::scene::{ensure_texture, patched_model_meshes};
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::magpack::MagPack;
use archrust_data::rw::{Geometry, SectorGeometry, TextureDictionary};
use glam::{Mat4, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub const SECTOR_SIZE: f32 = 6400.0;
pub const SECTOR_OFFSET: i32 = 400;

/// Indice de secteur d'une position monde.
pub fn sector_of(x: f32, z: f32) -> (i32, i32) {
    (
        (x / SECTOR_SIZE).floor() as i32 + SECTOR_OFFSET,
        (z / SECTOR_SIZE).floor() as i32 + SECTOR_OFFSET,
    )
}

/// Numéro de tuile d'un secteur.
pub fn tile_of(sx: i32, sy: i32) -> u32 {
    ((sx / 16) * 100 + sy / 16) as u32
}

/// Origine monde (coin bas-gauche) d'un secteur.
pub fn sector_origin(sx: i32, sy: i32) -> Vec3 {
    Vec3::new(
        (sx - SECTOR_OFFSET) as f32 * SECTOR_SIZE,
        0.0,
        (sy - SECTOR_OFFSET) as f32 * SECTOR_SIZE,
    )
}

/// Cellules par secteur et taille d'une cellule (6400 / 16).
pub const CELLS: usize = 16;
pub const CELL_SIZE: f32 = SECTOR_SIZE / CELLS as f32;

/// Hauteurs aux 17 × 17 coins de cellule d'un secteur, construites à partir des sommets de sa
/// géométrie (chaque sommet est un coin de cellule).
#[derive(Debug, Clone)]
pub struct HeightField {
    pub origin: Vec3,
    grid: Vec<f32>,
    filled: Vec<bool>,
}

impl HeightField {
    pub fn from_vertices(sx: i32, sy: i32, vertices: &[Vec3]) -> Self {
        let origin = sector_origin(sx, sy);
        let n = CELLS + 1;
        let mut grid = vec![0f32; n * n];
        let mut filled = vec![false; n * n];
        for v in vertices {
            let i = ((v.x - origin.x) / CELL_SIZE)
                .round()
                .clamp(0.0, CELLS as f32) as usize;
            let j = ((v.z - origin.z) / CELL_SIZE)
                .round()
                .clamp(0.0, CELLS as f32) as usize;
            let k = j * n + i;
            if !filled[k] || v.y > grid[k] {
                grid[k] = v.y;
                filled[k] = true;
            }
        }
        // Les coins jamais visés prennent la hauteur d'un voisin rempli.
        let mut hf = HeightField {
            origin,
            grid,
            filled,
        };
        for k in 0..n * n {
            if !hf.filled[k] {
                let (i, j) = (k % n, k / n);
                let neighbours = [
                    (i.wrapping_sub(1), j),
                    (i + 1, j),
                    (i, j.wrapping_sub(1)),
                    (i, j + 1),
                ];
                if let Some(h) = neighbours
                    .iter()
                    .filter(|(a, b)| *a < n && *b < n)
                    .map(|(a, b)| b * n + a)
                    .find(|&m| hf.filled[m])
                    .map(|m| hf.grid[m])
                {
                    hf.grid[k] = h;
                }
            }
        }
        hf
    }

    fn corner(&self, i: usize, j: usize) -> f32 {
        self.grid[j.min(CELLS) * (CELLS + 1) + i.min(CELLS)]
    }

    /// Hauteur interpolée (bilinéaire) en un point du secteur.
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        let fx = ((x - self.origin.x) / CELL_SIZE).clamp(0.0, CELLS as f32 - 0.0001);
        let fz = ((z - self.origin.z) / CELL_SIZE).clamp(0.0, CELLS as f32 - 0.0001);
        let (i, j) = (fx.floor() as usize, fz.floor() as usize);
        let (tx, tz) = (fx - i as f32, fz - j as f32);
        let h00 = self.corner(i, j);
        let h10 = self.corner(i + 1, j);
        let h01 = self.corner(i, j + 1);
        let h11 = self.corner(i + 1, j + 1);
        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * tz
    }
}

/// Contenu affiché d'un secteur : sol du client (multi-couches) ou modèles d'un patch glTF.
enum SectorMeshes {
    Ground(Arc<TerrainMesh>),
    Models(Vec<(Arc<Mesh>, Mat4)>),
}

pub struct Terrain {
    pub radius: i32,
    sectors: HashMap<(i32, i32), SectorMeshes>,
    heights: HashMap<(i32, i32), HeightField>,
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
        Self {
            radius,
            sectors: HashMap::new(),
            heights: HashMap::new(),
            failed: HashSet::new(),
            tiles_a: HashMap::new(),
            tiles_b: HashMap::new(),
            loads_per_frame: 2,
            last_center: None,
        }
    }

    pub fn loaded_count(&self) -> usize {
        self.sectors.len()
    }

    fn tile(&mut self, assets: &Assets, prefix: char, tile: u32) -> Option<Arc<MagPack>> {
        let cache = if prefix == 'a' {
            &mut self.tiles_a
        } else {
            &mut self.tiles_b
        };
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

    fn load_sector(
        &mut self,
        renderer: &mut Renderer,
        assets: &Assets,
        sx: i32,
        sy: i32,
    ) -> Option<SectorMeshes> {
        // 1. Un patch fournit un glTF pour ce secteur : posé à l'origine du secteur, hauteurs tirées de ses sommets.
        let key = format!("world/D{sx},{sy}.dff");
        if let Some(meshes) = patched_model_meshes(renderer, assets, &key) {
            let root = Mat4::from_translation(sector_origin(sx, sy));
            if let Some(Ok(pm)) = assets.patched_model(&key) {
                let pts = pm.model.world_positions(
                    root * Mat4::from_scale(Vec3::splat(if pm.scale > 0.0 {
                        pm.scale
                    } else {
                        1.0
                    })),
                );
                self.heights
                    .insert((sx, sy), HeightField::from_vertices(sx, sy, &pts));
            }
            return Some(SectorMeshes::Models(
                meshes
                    .into_iter()
                    .map(|(m, local)| (m, root * local))
                    .collect(),
            ));
        }
        // 2. Géométrie du client.
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
                    let txd_plain = if txd_plain.len() > 4 && txd_plain[0] == 0x16 {
                        txd_plain
                    } else {
                        capi_crypt(KEY_TEXTURE, &txd_plain)
                    };
                    if let Ok(dict) = TextureDictionary::parse(&txd_plain) {
                        if let Some(t) = dict.textures.first() {
                            if let Ok(rgba) = t.to_rgba8() {
                                renderer.upload_texture(
                                    &r_name,
                                    t.mips[0].width,
                                    t.mips[0].height,
                                    &rgba,
                                );
                            }
                        }
                    }
                }
            }
        }
        let mut geometry = sector.geometry;
        self.heights.insert(
            (sx, sy),
            HeightField::from_vertices(sx, sy, &geometry.vertices),
        );
        // Un matériau sans texture de base (aucun observé depuis la lecture de l'extension
        // 0xfffff001) reçoit la texture du secteur. Elle couvre tout le secteur alors que les UV
        // sont par cellule : ce n'est qu'un repli.
        for m in geometry.materials.iter_mut() {
            if m.texture.is_none() && renderer.texture(&r_name).is_some() {
                m.texture = Some(archrust_data::rw::Texture {
                    name: r_name.clone(),
                    ..Default::default()
                });
            }
        }
        Some(SectorMeshes::Ground(Arc::new(build_terrain_mesh(
            renderer, assets, &geometry,
        ))))
    }

    /// Charge progressivement les secteurs autour de `center`, décharge les lointains.
    pub fn update(&mut self, renderer: &mut Renderer, assets: &Assets, center: Vec3) {
        let (cx, cy) = sector_of(center.x, center.z);
        if self.last_center != Some((cx, cy)) {
            self.last_center = Some((cx, cy));
            let r = self.radius + 2;
            self.sectors
                .retain(|&(sx, sy), _| (sx - cx).abs() <= r && (sy - cy).abs() <= r);
            self.heights
                .retain(|&(sx, sy), _| (sx - cx).abs() <= r && (sy - cy).abs() <= r);
            self.failed
                .retain(|&(sx, sy)| (sx - cx).abs() <= r && (sy - cy).abs() <= r);
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
                Some(meshes) => {
                    self.sectors.insert((sx, sy), meshes);
                }
                None => {
                    self.failed.insert((sx, sy));
                }
            }
            budget -= 1;
        }
    }

    pub fn draw_items(&self) -> Vec<DrawItem> {
        self.sectors
            .values()
            .filter_map(|s| match s {
                SectorMeshes::Models(m) => Some(m),
                SectorMeshes::Ground(_) => None,
            })
            .flatten()
            .map(|(m, model)| DrawItem {
                mesh: m.clone(),
                model: *model,
                tint: [1.0; 4],
            })
            .collect()
    }

    /// Secteurs de sol du client, dessinés par le pipeline multi-couches.
    pub fn ground_meshes(&self) -> Vec<Arc<TerrainMesh>> {
        self.sectors
            .values()
            .filter_map(|s| match s {
                SectorMeshes::Ground(g) => Some(g.clone()),
                SectorMeshes::Models(_) => None,
            })
            .collect()
    }

    /// Hauteur du terrain sous (x, z), si le secteur est chargé.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        self.heights
            .get(&sector_of(x, z))
            .map(|h| h.height_at(x, z))
    }

    /// Premier point du terrain touché par un rayon (marche à pas fixe puis dichotomie).
    pub fn raycast(&self, origin: Vec3, dir: Vec3, max_dist: f32) -> Option<Vec3> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let step = 100.0;
        let mut t = 0.0;
        let mut prev_above: Option<(f32, f32)> = None; // (t, écart au sol)
        while t <= max_dist {
            let p = origin + dir * t;
            if let Some(h) = self.height_at(p.x, p.z) {
                let gap = p.y - h;
                if gap <= 0.0 {
                    let (mut lo, mut hi) = (prev_above.map(|(t0, _)| t0).unwrap_or(t), t);
                    for _ in 0..16 {
                        let mid = (lo + hi) / 2.0;
                        let q = origin + dir * mid;
                        match self.height_at(q.x, q.z) {
                            Some(hm) if q.y - hm <= 0.0 => hi = mid,
                            _ => lo = mid,
                        }
                    }
                    let q = origin + dir * hi;
                    return Some(Vec3::new(q.x, self.height_at(q.x, q.z).unwrap_or(q.y), q.z));
                }
                prev_above = Some((t, gap));
            } else {
                prev_above = None;
            }
            t += step;
        }
        None
    }
}

/// Textures des étages d'un matériau de sol : base, puis les entrées de l'extension `0xfffff001`
/// dans l'ordre du fichier (paires masque de type 3 / couche de type 1). L'étage n lit le jeu d'UV n
/// (vérifié sur `D325,389` : les cellules à une couche ont un UV1 en pas de 0,25 — case de l'atlas
/// du masque — et un UV2 de 0 à 1 ; celles à deux couches ont en plus UV3 en pas de 0,25 et UV4 de
/// 0 à 1 ; les étages inutilisés gardent des UV par défaut en pas de 0,5).
pub fn stage_textures(material: &archrust_data::rw::Material) -> [Option<String>; TERRAIN_STAGES] {
    let mut stages: [Option<String>; TERRAIN_STAGES] = Default::default();
    stages[0] = material.texture.as_ref().map(|t| t.name.clone());
    for (i, layer) in material.layers.iter().enumerate() {
        if i + 1 >= TERRAIN_STAGES {
            log::debug!(
                "matériau de sol : {} couches, au-delà de 2 ignorées",
                material.layers.len()
            );
            break;
        }
        stages[i + 1] = layer.texture.as_ref().map(|t| t.name.clone());
    }
    stages
}

/// Maillage GPU d'un secteur du client : sommets en coordonnées monde, cinq jeux d'UV, triangles
/// regroupés par matériau (Bin Mesh s'il est présent, sinon la liste de triangles).
fn build_terrain_mesh(renderer: &mut Renderer, assets: &Assets, g: &Geometry) -> TerrainMesh {
    let n = g.vertices.len();
    let vertices: Vec<TerrainVertex> = (0..n)
        .map(|i| {
            let mut uv = [[0.0; 2]; TERRAIN_STAGES];
            for (k, set) in g.uv_sets.iter().take(TERRAIN_STAGES).enumerate() {
                uv[k] = set.get(i).copied().unwrap_or([0.0, 0.0]);
            }
            TerrainVertex {
                position: g.vertices[i].to_array(),
                normal: g.normals.get(i).copied().unwrap_or(Vec3::Y).to_array(),
                color: g
                    .colors
                    .get(i)
                    .map(|c| c.map(|v| v as f32 / 255.0))
                    .unwrap_or([1.0; 4]),
                uv,
            }
        })
        .collect();
    let mut by_material: HashMap<u32, Vec<u32>> = HashMap::new();
    if !g.meshes.is_empty() && !g.tristrip {
        for m in &g.meshes {
            by_material
                .entry(m.material)
                .or_default()
                .extend(m.indices.iter().copied().filter(|&i| (i as usize) < n));
        }
    } else {
        for t in &g.triangles {
            if t.v.iter().all(|&i| (i as usize) < n) {
                by_material
                    .entry(t.material as u32)
                    .or_default()
                    .extend(t.v.iter().map(|&i| i as u32));
            }
        }
    }
    let mut materials: Vec<_> = by_material.into_iter().collect();
    materials.sort_by_key(|(m, _)| *m);
    let mut indices = Vec::new();
    let mut parts = Vec::new();
    for (material, list) in materials {
        let stages = g
            .materials
            .get(material as usize)
            .map(stage_textures)
            .unwrap_or_default();
        for name in stages.iter().flatten() {
            ensure_texture(renderer, assets, name);
        }
        parts.push((indices.len() as u32, list.len() as u32, stages));
        indices.extend(list);
    }
    renderer.create_terrain_mesh(&vertices, &indices, parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heightfield_bilinear_and_raycast() {
        // Plan incliné : y = x - origine.x (sur 6400 unités).
        let origin = sector_origin(272, 272);
        let mut verts = Vec::new();
        for j in 0..=CELLS {
            for i in 0..=CELLS {
                verts.push(Vec3::new(
                    origin.x + i as f32 * CELL_SIZE,
                    i as f32 * CELL_SIZE,
                    origin.z + j as f32 * CELL_SIZE,
                ));
            }
        }
        let hf = HeightField::from_vertices(272, 272, &verts);
        assert!((hf.height_at(origin.x + 1000.0, origin.z + 3000.0) - 1000.0).abs() < 0.01);
        let mut t = Terrain::new(1);
        t.heights.insert((272, 272), hf);
        let hit = t
            .raycast(
                Vec3::new(origin.x + 2000.0, 5000.0, origin.z + 2000.0),
                Vec3::new(0.0, -1.0, 0.0),
                10000.0,
            )
            .unwrap();
        assert!((hit.y - 2000.0).abs() < 1.0, "{hit:?}");
        assert!(t
            .raycast(Vec3::new(0.0, 100.0, 0.0), Vec3::Y, 1000.0)
            .is_none());
    }

    #[test]
    fn mapping() {
        assert_eq!(sector_of(-819200.0, -819200.0), (272, 272));
        assert_eq!(sector_of(-812800.1, -812800.1), (272, 272));
        assert_eq!(sector_of(-475955.0, -66219.0), (325, 389));
        assert_eq!(tile_of(325, 389), 2024);
        assert_eq!(
            sector_origin(272, 272),
            Vec3::new(-819200.0, 0.0, -819200.0)
        );
    }
}
