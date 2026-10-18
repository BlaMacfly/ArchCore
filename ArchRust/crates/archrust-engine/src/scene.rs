//! Conversion des clumps RenderWare en maillages GPU et entités du monde.

use crate::assets::Assets;
use crate::render::{Mesh, Renderer, SubMesh, Vertex};
use anyhow::Result;
use archrust_data::rw::{Clump, Geometry};
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

/// Construit les maillages d'un clump : un maillage par atomic, dans l'espace de sa frame.
/// Les textures référencées sont chargées (packs ou patch HD) et téléversées à la demande.
pub fn build_clump_meshes(renderer: &mut Renderer, assets: &Assets, clump: &Clump) -> Vec<(Arc<Mesh>, Mat4)> {
    let world = clump.world_matrices();
    let mut out = Vec::new();
    for atomic in &clump.atomics {
        let Some(geom) = clump.geometries.get(atomic.geometry as usize) else { continue };
        if geom.vertices.is_empty() {
            continue;
        }
        let mesh = build_geometry_mesh(renderer, assets, geom);
        let model = world.get(atomic.frame as usize).copied().unwrap_or(Mat4::IDENTITY);
        out.push((Arc::new(mesh), model));
    }
    out
}

/// Maillage d'une géométrie : les triangles sont regroupés par matériau.
pub fn build_geometry_mesh(renderer: &mut Renderer, assets: &Assets, geom: &Geometry) -> Mesh {
    let n = geom.vertices.len();
    let uv0 = geom.uv_sets.first();
    let mut vertices: Vec<Vertex> = Vec::with_capacity(n);
    for i in 0..n {
        vertices.push(Vertex {
            position: geom.vertices[i].to_array(),
            normal: geom.normals.get(i).copied().unwrap_or(Vec3::Y).to_array(),
            uv: uv0.and_then(|s| s.get(i)).copied().unwrap_or([0.0, 0.0]),
            color: geom.colors.get(i).map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]).unwrap_or([1.0; 4]),
        });
    }
    if geom.normals.is_empty() {
        compute_normals(&mut vertices, &geom.triangles);
    }
    // Indices par matériau (listes de triangles ; les bandes du Bin Mesh sont dépliées si besoin).
    let mut by_material: HashMap<u16, Vec<u32>> = HashMap::new();
    if !geom.meshes.is_empty() {
        for m in &geom.meshes {
            let list = by_material.entry(m.material as u16).or_default();
            if geom.tristrip {
                for w in m.indices.windows(3).enumerate() {
                    let (i, tri) = w;
                    if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                        continue;
                    }
                    if i % 2 == 0 {
                        list.extend_from_slice(&[tri[0], tri[1], tri[2]]);
                    } else {
                        list.extend_from_slice(&[tri[1], tri[0], tri[2]]);
                    }
                }
            } else {
                list.extend_from_slice(&m.indices);
            }
        }
    } else {
        for t in &geom.triangles {
            by_material.entry(t.material).or_default().extend_from_slice(&[t.v[0] as u32, t.v[1] as u32, t.v[2] as u32]);
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    let mut parts = Vec::new();
    let mut mats: Vec<_> = by_material.into_iter().collect();
    mats.sort_by_key(|(m, _)| *m);
    for (mat_id, list) in mats {
        let material = geom.materials.get(mat_id as usize);
        let texture = material.and_then(|m| m.texture.as_ref()).map(|t| t.name.clone());
        if let Some(name) = &texture {
            ensure_texture(renderer, assets, name);
        }
        let color = material.map(|m| [m.color[0] as f32 / 255.0, m.color[1] as f32 / 255.0, m.color[2] as f32 / 255.0, m.color[3] as f32 / 255.0]).unwrap_or([1.0; 4]);
        parts.push(SubMesh { index_start: indices.len() as u32, index_count: list.len() as u32, texture, color });
        indices.extend(list.into_iter().filter(|&i| (i as usize) < n));
        if let Some(p) = parts.last_mut() {
            p.index_count = indices.len() as u32 - p.index_start;
        }
    }
    renderer.create_mesh(&vertices, &indices, parts)
}

/// Charge et téléverse une texture si elle n'est pas déjà sur le GPU.
pub fn ensure_texture(renderer: &mut Renderer, assets: &Assets, name: &str) {
    if renderer.texture(name).is_some() {
        return;
    }
    match assets.texture(name) {
        Ok(img) => {
            renderer.upload_texture(name, img.width, img.height, &img.data);
        }
        Err(e) => {
            log::debug!("texture {name} : {e}");
            renderer.textures.insert(name.to_ascii_lowercase(), renderer.white.clone());
        }
    }
}

fn compute_normals(vertices: &mut [Vertex], triangles: &[archrust_data::rw::Triangle]) {
    let mut acc = vec![Vec3::ZERO; vertices.len()];
    for t in triangles {
        let [a, b, c] = t.v.map(|i| i as usize);
        if a >= vertices.len() || b >= vertices.len() || c >= vertices.len() {
            continue;
        }
        let pa = Vec3::from_array(vertices[a].position);
        let pb = Vec3::from_array(vertices[b].position);
        let pc = Vec3::from_array(vertices[c].position);
        let nrm = (pb - pa).cross(pc - pa);
        acc[a] += nrm;
        acc[b] += nrm;
        acc[c] += nrm;
    }
    for (v, n) in vertices.iter_mut().zip(acc) {
        v.normal = n.normalize_or(Vec3::Y).to_array();
    }
}

/// Maillage de secours (boîte) pour les entités sans modèle connu.
pub fn box_mesh(renderer: &Renderer, half: Vec3, color: [f32; 4]) -> Mesh {
    let p = |x: f32, y: f32, z: f32| [x * half.x, y * half.y + half.y, z * half.z];
    let faces: [([f32; 3], [[f32; 3]; 4]); 6] = [
        ([0.0, 0.0, 1.0], [p(-1.0, -1.0, 1.0), p(1.0, -1.0, 1.0), p(1.0, 1.0, 1.0), p(-1.0, 1.0, 1.0)]),
        ([0.0, 0.0, -1.0], [p(1.0, -1.0, -1.0), p(-1.0, -1.0, -1.0), p(-1.0, 1.0, -1.0), p(1.0, 1.0, -1.0)]),
        ([1.0, 0.0, 0.0], [p(1.0, -1.0, 1.0), p(1.0, -1.0, -1.0), p(1.0, 1.0, -1.0), p(1.0, 1.0, 1.0)]),
        ([-1.0, 0.0, 0.0], [p(-1.0, -1.0, -1.0), p(-1.0, -1.0, 1.0), p(-1.0, 1.0, 1.0), p(-1.0, 1.0, -1.0)]),
        ([0.0, 1.0, 0.0], [p(-1.0, 1.0, 1.0), p(1.0, 1.0, 1.0), p(1.0, 1.0, -1.0), p(-1.0, 1.0, -1.0)]),
        ([0.0, -1.0, 0.0], [p(-1.0, -1.0, -1.0), p(1.0, -1.0, -1.0), p(1.0, -1.0, 1.0), p(-1.0, -1.0, 1.0)]),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (normal, quad) in faces {
        let base = vertices.len() as u32;
        for (i, pos) in quad.iter().enumerate() {
            vertices.push(Vertex { position: *pos, normal, uv: [(i & 1) as f32, (i >> 1) as f32], color });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let count = indices.len() as u32;
    renderer.create_mesh(&vertices, &indices, vec![SubMesh { index_start: 0, index_count: count, texture: None, color: [1.0; 4] }])
}

/// Sol de secours (grille plate) quand aucun secteur n'est chargé.
pub fn ground_mesh(renderer: &Renderer, size: f32) -> Mesh {
    let h = size / 2.0;
    let vertices = vec![
        Vertex { position: [-h, 0.0, -h], normal: [0.0, 1.0, 0.0], uv: [0.0, 0.0], color: [0.35, 0.5, 0.3, 1.0] },
        Vertex { position: [h, 0.0, -h], normal: [0.0, 1.0, 0.0], uv: [size / 500.0, 0.0], color: [0.35, 0.5, 0.3, 1.0] },
        Vertex { position: [h, 0.0, h], normal: [0.0, 1.0, 0.0], uv: [size / 500.0, size / 500.0], color: [0.35, 0.5, 0.3, 1.0] },
        Vertex { position: [-h, 0.0, h], normal: [0.0, 1.0, 0.0], uv: [0.0, size / 500.0], color: [0.35, 0.5, 0.3, 1.0] },
    ];
    renderer.create_mesh(&vertices, &[0, 2, 1, 0, 3, 2], vec![SubMesh { index_start: 0, index_count: 6, texture: None, color: [1.0; 4] }])
}

/// Entité du monde affichée.
pub struct Entity {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    pub position: Vec3,
    pub destination: Vec3,
    pub meshes: Vec<(Arc<Mesh>, Mat4)>,
    pub is_player: bool,
}

impl Entity {
    pub fn model_matrix(&self) -> Mat4 {
        Mat4::from_translation(self.position)
    }
}

/// Résolution TID → fichier de modèle d'après `charactertemplateclient.ini`.
///
/// La table est positionnelle : la section d'en-tête `[]` nomme les colonnes (`0=ModuleData1`,
/// `1=DFF`, `2=DEFAULT_ARMOUR_DFF`…) et chaque section `[TID]` donne `colonne=valeur`. La colonne
/// `DFF` contient le numéro du clump : `1` → `a0000001.ecl` du pack `character`.
pub fn model_for_tid(assets: &Assets, tid: i32) -> Option<String> {
    let table = assets.table("ini", "charactertemplateclient.ini").ok()?;
    let header = table.section("")?;
    let dff_col = header.entries.iter().find(|(_, v)| v.eq_ignore_ascii_case("DFF")).map(|(k, _)| k.as_str())?;
    let sec = table.section(&tid.to_string())?;
    let value = sec.get(dff_col)?.trim();
    if let Ok(n) = value.parse::<u32>() {
        return Some(format!("a{n:07}.ecl"));
    }
    let base = value.rsplit(['\\', '/']).next().unwrap_or(value);
    let base = base.rsplit_once('.').map(|(b, _)| b).unwrap_or(base);
    Some(format!("{}.ecl", base.to_ascii_lowercase()))
}

/// Charge (ou fabrique) les maillages d'une entité.
pub fn entity_meshes(renderer: &mut Renderer, assets: &Assets, tid: i32, fallback: &Arc<Mesh>) -> Result<Vec<(Arc<Mesh>, Mat4)>> {
    if let Some(file) = model_for_tid(assets, tid) {
        if let Ok(clump) = assets.clump("character", &file) {
            let meshes = build_clump_meshes(renderer, assets, &clump);
            if !meshes.is_empty() {
                return Ok(meshes);
            }
        }
    }
    Ok(vec![(fallback.clone(), Mat4::IDENTITY)])
}
