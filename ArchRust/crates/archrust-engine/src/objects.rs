//! Objets statiques du monde (bâtiments, arbres, rochers, barrières…), d'après les données
//! d'Archonia.
//!
//! * **Placement** — `ini/obj%05d.ini`, une table par tuile du monde (numéro de tuile de
//!   [`tile_of`], ex. `obj02024.ini` pour Anchorville) ; une section par objet, clés nommées
//!   écrites par ApmObject (`FUN_140233e40`) : `TID`, `ObjectType`, `Scale=x,y,z`,
//!   `Position=x,y,z` (coordonnées monde), `DegreeX`, `DegreeY` (degrés), `OcTreeIDNum`…
//! * **Modèle** — `ini/objecttemplate.ini`, table positionnelle : colonne 10 `DFF` =
//!   `lod:fichier` (ex. `0:H003b100.ECL`, pack `object`), colonne 4 `Category`, et une seconde
//!   colonne 2 = `ObjectType` d'AgcmObject (drapeaux).
//! * **Exclusions** — les objets de catégorie « spawn » (lumières, effets, repères de monstres)
//!   sont les seuls à porter le bit `0x10000` des drapeaux AgcmObject : ce sont des repères
//!   d'éditeur, non dessinés. La catégorie `NPC` place des modèles de PNJ que le serveur envoie
//!   déjà comme personnages (ex. `<Garde> Yuan`) : ignorée pour ne pas les dédoubler.
//! * **Matrice** — `T(position) × RotY(DegreeY) × RotX(DegreeX) × S(scale)`, avec la convention
//!   d'angle des personnages (+Z envoyé sur (sin, 0, cos)). Elle **remplace** la frame racine du
//!   clump : celle-ci garde la position d'export du modèle (ex. `(1280, 5, 1790)` pour la barrière
//!   `h004ac00.ecl`, `(-3100, 15, -3654)` pour l'arbre `h003b100.ecl`), qu'il faut annuler.

use crate::assets::Assets;
use crate::render::{DrawItem, Mesh, Renderer};
use crate::scene::model_meshes;
use crate::world::{sector_of, tile_of};
use glam::{Mat4, Quat, Vec3};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Bit des drapeaux AgcmObject propre aux repères d'éditeur (catégorie « spawn »).
pub const AGCM_OBJECT_MARKER: u32 = 0x1_0000;

/// Objet placé, avant chargement de son modèle.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub tid: u32,
    pub position: Vec3,
    pub scale: Vec3,
    pub degree_x: f32,
    pub degree_y: f32,
}

impl Placement {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(
            self.scale,
            Quat::from_rotation_y(self.degree_y.to_radians())
                * Quat::from_rotation_x(self.degree_x.to_radians()),
            self.position,
        )
    }
}

fn vec3_of(text: &str) -> Option<Vec3> {
    let v: Vec<f32> = text
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    (v.len() == 3).then(|| Vec3::new(v[0], v[1], v[2]))
}

/// Placements d'une tuile (`ini/obj%05d.ini`), tous objets confondus.
pub fn tile_placements(assets: &Assets, tile: u32) -> Vec<Placement> {
    let Ok(table) = assets.table("ini", &format!("obj{tile:05}.ini")) else {
        return Vec::new();
    };
    table
        .sections
        .iter()
        .filter_map(|s| {
            Some(Placement {
                tid: s.get("TID")?.trim().parse().ok()?,
                position: vec3_of(s.get("Position")?)?,
                scale: s.get("Scale").and_then(vec3_of).unwrap_or(Vec3::ONE),
                degree_x: s.get_f32("DegreeX").unwrap_or(0.0),
                degree_y: s.get_f32("DegreeY").unwrap_or(0.0),
            })
        })
        .collect()
}

/// Portail : objet placé qui porte la fonction d'événement 8 (AgpmEventTeleport) et le nom de son
/// point de `TeleportPoint.ini` (`Function=8` puis `PointName=…` dans sa section).
#[derive(Debug, Clone, PartialEq)]
pub struct PortalObject {
    /// Section de l'objet : identifiant envoyé dans l'EVENT_BASE.
    pub id: u32,
    pub point: String,
    pub placement: Placement,
}

/// Portails d'une tuile (`ini/obj%05d.ini`).
pub fn tile_portals(assets: &Assets, tile: u32) -> Vec<PortalObject> {
    let Ok(table) = assets.table("ini", &format!("obj{tile:05}.ini")) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for s in &table.sections {
        let Ok(id) = s.name.trim().parse::<u32>() else { continue };
        // Chaque `Function=` ouvre une fonction ; le `PointName` qui suit appartient à la dernière.
        let mut teleport = false;
        let mut point = None;
        for (k, v) in &s.entries {
            if k.eq_ignore_ascii_case("Function") {
                teleport = v.trim() == "8";
            } else if teleport && k.eq_ignore_ascii_case("PointName") && !v.trim().is_empty() {
                point = Some(v.trim().to_string());
            }
        }
        let (Some(point), Some(position)) = (point, s.get("Position").and_then(vec3_of)) else { continue };
        let placement = Placement {
            tid: s.get("TID").and_then(|t| t.trim().parse().ok()).unwrap_or(0),
            position,
            scale: s.get("Scale").and_then(vec3_of).unwrap_or(Vec3::ONE),
            degree_x: s.get_f32("DegreeX").unwrap_or(0.0),
            degree_y: s.get_f32("DegreeY").unwrap_or(0.0),
        };
        out.push(PortalObject { id, point, placement });
    }
    out
}

/// Fichier de modèle d'un modèle d'objet, ou `None` s'il ne doit pas être dessiné.
pub fn template_model(assets: &Assets, tid: u32) -> Option<String> {
    let table = assets.table("ini", "objecttemplate.ini").ok()?;
    let t = table.section(&tid.to_string())?;
    if t.get("4").is_some_and(|c| c.eq_ignore_ascii_case("NPC")) {
        return None;
    }
    let flags = t
        .get_all("2")
        .nth(1)
        .and_then(|v| v.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if flags & AGCM_OBJECT_MARKER != 0 {
        return None;
    }
    // `DFF` = `lod:fichier` ; le niveau 0 suffit pour l'instant.
    let dff = t.get_all("10").find_map(|v| {
        let (lod, file) = v.split_once(':')?;
        (lod.trim() == "0" && !file.trim().is_empty()).then(|| file.trim().to_ascii_lowercase())
    })?;
    Some(dff)
}

/// Inverse de la frame racine d'un clump d'objet du client (identité pour un glTF de patch, posé
/// tel quel) : la matrice de l'objet prend la place de cette frame.
pub fn root_inverse(assets: &Assets, file: &str) -> Mat4 {
    if assets.patched_model(&format!("object/{file}")).is_some() {
        return Mat4::IDENTITY;
    }
    assets
        .clump("object", file)
        .ok()
        .and_then(|c| c.frames.first().map(|f| f.local_matrix().inverse()))
        .unwrap_or(Mat4::IDENTITY)
}

/// Objets du monde chargés autour du joueur, par secteur.
#[derive(Default)]
pub struct WorldObjects {
    /// Tuiles dont les placements ont été lus.
    tiles: HashSet<u32>,
    /// Placements des tuiles lues, par secteur.
    placements: HashMap<(i32, i32), Vec<Placement>>,
    /// Maillages prêts, par secteur.
    sectors: HashMap<(i32, i32), Vec<(Arc<Mesh>, Mat4)>>,
    /// Modèles déjà convertis (`None` = introuvable ou à ne pas dessiner).
    models: HashMap<u32, Option<Arc<Vec<(Arc<Mesh>, Mat4)>>>>,
    pub radius: i32,
    pub sectors_per_frame: usize,
}

impl WorldObjects {
    pub fn new(radius: i32) -> Self {
        Self {
            radius,
            sectors_per_frame: 1,
            ..Default::default()
        }
    }

    pub fn loaded_count(&self) -> usize {
        self.sectors.values().map(Vec::len).sum()
    }

    /// Hauteur du plus haut sol d'objet sous `(x, z)` et au-dessous de `below` : rayon vertical
    /// contre les triangles des objets placés dans le secteur (géométrie du client, matrice de
    /// l'objet, frame racine annulée). Sert à poser les personnages de la scène de connexion sur
    /// le dallage de la salle du trône. `None` si la tuile n'est pas lue ou si aucun triangle ne
    /// passe sous le point.
    pub fn floor_height(&self, assets: &Assets, x: f32, z: f32, below: f32) -> Option<f32> {
        // Un grand bâtiment est rangé dans le secteur de son pivot : on regarde aussi les voisins.
        let (sx, sy) = sector_of(x, z);
        let placements: Vec<&Placement> = (-1..=1)
            .flat_map(|dy| (-1..=1).map(move |dx| (sx + dx, sy + dy)))
            .filter_map(|k| self.placements.get(&k))
            .flatten()
            .collect();
        if placements.is_empty() {
            return None;
        }
        let mut best: Option<f32> = None;
        for p in placements {
            let Some(file) = template_model(assets, p.tid) else {
                continue;
            };
            let Ok(clump) = assets.clump("object", &file) else {
                continue;
            };
            let root = root_inverse(assets, &file);
            let world = clump.world_matrices();
            let object = p.matrix();
            for a in &clump.atomics {
                let (Some(g), Some(frame)) = (
                    clump.geometries.get(a.geometry as usize),
                    world.get(a.frame as usize),
                ) else {
                    continue;
                };
                let m = object * root * *frame;
                let verts: Vec<Vec3> = g.vertices.iter().map(|v| m.transform_point3(*v)).collect();
                for t in &g.triangles {
                    let [i, j, k] = t.v.map(|i| i as usize);
                    let (Some(a), Some(b), Some(c)) = (verts.get(i), verts.get(j), verts.get(k))
                    else {
                        continue;
                    };
                    // Coordonnées barycentriques dans le plan XZ.
                    let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
                    if d.abs() < 1e-6 {
                        continue;
                    }
                    let l1 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / d;
                    let l2 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / d;
                    let l3 = 1.0 - l1 - l2;
                    if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                        continue;
                    }
                    let y = l1 * a.y + l2 * b.y + l3 * c.y;
                    if y <= below && best.is_none_or(|h| y > h) {
                        best = Some(y);
                    }
                }
            }
        }
        best
    }

    fn model(
        &mut self,
        renderer: &mut Renderer,
        assets: &Assets,
        tid: u32,
    ) -> Option<Arc<Vec<(Arc<Mesh>, Mat4)>>> {
        if let Some(m) = self.models.get(&tid) {
            return m.clone();
        }
        let loaded = template_model(assets, tid).and_then(|file| {
            match model_meshes(renderer, assets, "object", &file) {
                Ok(m) if !m.is_empty() => {
                    let root = root_inverse(assets, &file);
                    Some(Arc::new(
                        m.into_iter()
                            .map(|(mesh, local)| (mesh, root * local))
                            .collect(),
                    ))
                }
                Ok(_) => None,
                Err(e) => {
                    log::debug!("objet TID {tid} ({file}) : {e:#}");
                    None
                }
            }
        });
        self.models.insert(tid, loaded.clone());
        loaded
    }

    /// Lit les tuiles autour de `center`, prépare les secteurs proches (quelques-uns par image)
    /// et oublie les lointains.
    pub fn update(&mut self, renderer: &mut Renderer, assets: &Assets, center: Vec3) {
        if !assets.has_client() {
            return;
        }
        let (cx, cy) = sector_of(center.x, center.z);
        let r = self.radius;
        for sy in cy - r..=cy + r {
            for sx in cx - r..=cx + r {
                let tile = tile_of(sx, sy);
                if self.tiles.insert(tile) {
                    let placements = tile_placements(assets, tile);
                    log::debug!("tuile {tile} : {} objets placés", placements.len());
                    for p in placements {
                        let key = sector_of(p.position.x, p.position.z);
                        self.placements.entry(key).or_default().push(p);
                    }
                }
            }
        }
        let keep = r + 2;
        self.sectors
            .retain(|&(sx, sy), _| (sx - cx).abs() <= keep && (sy - cy).abs() <= keep);
        let mut wanted: Vec<(i32, i32)> = self
            .placements
            .keys()
            .copied()
            .filter(|&(sx, sy)| (sx - cx).abs() <= r && (sy - cy).abs() <= r)
            .filter(|k| !self.sectors.contains_key(k))
            .collect();
        wanted.sort_by_key(|&(sx, sy)| (sx - cx).abs().max((sy - cy).abs()));
        for key in wanted.into_iter().take(self.sectors_per_frame) {
            let Some(placements) = self.placements.get(&key).cloned() else {
                continue;
            };
            let mut meshes = Vec::new();
            for p in &placements {
                if let Some(model) = self.model(renderer, assets, p.tid) {
                    let world = p.matrix();
                    meshes.extend(model.iter().map(|(m, local)| (m.clone(), world * *local)));
                }
            }
            self.sectors.insert(key, meshes);
        }
    }

    /// Sphère englobante (centre, rayon) d'un objet placé, en coordonnées monde, si son modèle est
    /// déjà chargé.
    pub fn placed_bounds(&self, p: &Placement) -> Option<(Vec3, f32)> {
        let meshes = self.models.get(&p.tid)?.as_ref()?;
        let world = p.matrix();
        let scale = p.scale.max_element();
        let spheres: Vec<(Vec3, f32)> = meshes.iter().map(|(m, local)| (world.transform_point3(local.transform_point3(m.bounds_center)), m.bounds_radius * scale)).collect();
        let center = spheres.iter().map(|s| s.0).sum::<Vec3>() / spheres.len().max(1) as f32;
        let radius = spheres.iter().map(|(c, r)| c.distance(center) + r).fold(0.0, f32::max);
        (!spheres.is_empty()).then_some((center, radius))
    }

    /// Objets à dessiner à moins de `max_distance` de `eye`.
    pub fn draw_items(&self, eye: Vec3, max_distance: f32) -> Vec<DrawItem> {
        let mut out = Vec::new();
        for meshes in self.sectors.values() {
            for (mesh, model) in meshes {
                let center = model.transform_point3(mesh.bounds_center);
                if center.distance(eye) - mesh.bounds_radius > max_distance {
                    continue;
                }
                out.push(DrawItem {
                    mesh: mesh.clone(),
                    model: *model,
                    tint: [1.0; 4],
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_matrix_scales_turns_then_translates() {
        let p = Placement {
            tid: 1,
            position: Vec3::new(100.0, 5.0, -50.0),
            scale: Vec3::splat(2.0),
            degree_x: 0.0,
            degree_y: 90.0,
        };
        let m = p.matrix();
        // +Z du modèle, mis à l'échelle ×2 puis tourné de 90° autour de Y : vers +X.
        let got = m.transform_point3(Vec3::Z);
        assert!(
            (got - Vec3::new(102.0, 5.0, -50.0)).length() < 1e-4,
            "{got:?}"
        );
        assert_eq!(vec3_of("1.5, -2,3"), Some(Vec3::new(1.5, -2.0, 3.0)));
        assert_eq!(vec3_of("1,2"), None);
    }
}
