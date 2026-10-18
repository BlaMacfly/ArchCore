//! Conversion des clumps RenderWare en maillages GPU et entités du monde.
//!
//! Tout modèle passe par [`model_meshes`] : si un patch déclare un `[[models]]` glTF/GLB pour la
//! clé logique (`character/a0000060.ecl`…), c'est lui qui est affiché, avec ses propres textures ;
//! sinon le clump du client est converti.

use crate::assets::Assets;
use crate::gltf_model;
use crate::render::{Mesh, Renderer, SubMesh, Vertex};
use anyhow::Result;
use archrust_data::rw::{Clump, Geometry};
use archrust_netclient::world::EquipmentView;
use glam::{Mat4, Vec3};
use std::collections::HashMap;
use std::sync::Arc;

/// Maillages d'un modèle logique `rel_dir/file` (ex. `character/a0000060.ecl`) : un patch
/// `[[models]]` fournissant un glTF/GLB l'emporte sur le clump du client, et ses textures
/// remplacent celles du client.
pub fn model_meshes(
    renderer: &mut Renderer,
    assets: &Assets,
    rel_dir: &str,
    file: &str,
) -> Result<Vec<(Arc<Mesh>, Mat4)>> {
    let key = format!("{rel_dir}/{file}");
    if let Some(meshes) = patched_model_meshes(renderer, assets, &key) {
        return Ok(meshes);
    }
    let clump = assets.clump(rel_dir, file)?;
    Ok(build_clump_meshes(renderer, assets, &clump))
}

/// Maillages GPU du glTF qu'un patch fournit pour `key`, s'il y en a un. Un glTF illisible est
/// signalé puis ignoré : le modèle du client reprend la main.
pub fn patched_model_meshes(
    renderer: &mut Renderer,
    assets: &Assets,
    key: &str,
) -> Option<Vec<(Arc<Mesh>, Mat4)>> {
    match assets.patched_model(key)? {
        Ok(pm) => Some(gltf_model::upload(
            renderer,
            &pm.model,
            &pm.texture_prefix(),
            pm.scale,
        )),
        Err(e) => {
            log::warn!("{key} : {e:#}");
            None
        }
    }
}

/// Construit les maillages d'un clump : un maillage par atomic, dans l'espace de sa frame.
/// Les textures référencées sont chargées (packs ou patch HD) et téléversées à la demande.
pub fn build_clump_meshes(
    renderer: &mut Renderer,
    assets: &Assets,
    clump: &Clump,
) -> Vec<(Arc<Mesh>, Mat4)> {
    let world = clump.world_matrices();
    let mut out = Vec::new();
    for atomic in &clump.atomics {
        let Some(geom) = clump.geometries.get(atomic.geometry as usize) else {
            continue;
        };
        if geom.vertices.is_empty() {
            continue;
        }
        let mesh = build_geometry_mesh(renderer, assets, geom);
        let model = world
            .get(atomic.frame as usize)
            .copied()
            .unwrap_or(Mat4::IDENTITY);
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
            color: geom
                .colors
                .get(i)
                .map(|c| {
                    [
                        c[0] as f32 / 255.0,
                        c[1] as f32 / 255.0,
                        c[2] as f32 / 255.0,
                        c[3] as f32 / 255.0,
                    ]
                })
                .unwrap_or([1.0; 4]),
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
            by_material
                .entry(t.material)
                .or_default()
                .extend_from_slice(&[t.v[0] as u32, t.v[1] as u32, t.v[2] as u32]);
        }
    }
    let mut indices: Vec<u32> = Vec::new();
    let mut parts = Vec::new();
    let mut mats: Vec<_> = by_material.into_iter().collect();
    mats.sort_by_key(|(m, _)| *m);
    for (mat_id, list) in mats {
        let material = geom.materials.get(mat_id as usize);
        let texture = material
            .and_then(|m| m.texture.as_ref())
            .map(|t| t.name.clone());
        if let Some(name) = &texture {
            ensure_texture(renderer, assets, name);
        }
        let color = material
            .map(|m| {
                [
                    m.color[0] as f32 / 255.0,
                    m.color[1] as f32 / 255.0,
                    m.color[2] as f32 / 255.0,
                    m.color[3] as f32 / 255.0,
                ]
            })
            .unwrap_or([1.0; 4]);
        parts.push(SubMesh {
            index_start: indices.len() as u32,
            index_count: list.len() as u32,
            texture,
            color,
        });
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
            renderer
                .textures
                .insert(name.to_ascii_lowercase(), renderer.white.clone());
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
        (
            [0.0, 0.0, 1.0],
            [
                p(-1.0, -1.0, 1.0),
                p(1.0, -1.0, 1.0),
                p(1.0, 1.0, 1.0),
                p(-1.0, 1.0, 1.0),
            ],
        ),
        (
            [0.0, 0.0, -1.0],
            [
                p(1.0, -1.0, -1.0),
                p(-1.0, -1.0, -1.0),
                p(-1.0, 1.0, -1.0),
                p(1.0, 1.0, -1.0),
            ],
        ),
        (
            [1.0, 0.0, 0.0],
            [
                p(1.0, -1.0, 1.0),
                p(1.0, -1.0, -1.0),
                p(1.0, 1.0, -1.0),
                p(1.0, 1.0, 1.0),
            ],
        ),
        (
            [-1.0, 0.0, 0.0],
            [
                p(-1.0, -1.0, -1.0),
                p(-1.0, -1.0, 1.0),
                p(-1.0, 1.0, 1.0),
                p(-1.0, 1.0, -1.0),
            ],
        ),
        (
            [0.0, 1.0, 0.0],
            [
                p(-1.0, 1.0, 1.0),
                p(1.0, 1.0, 1.0),
                p(1.0, 1.0, -1.0),
                p(-1.0, 1.0, -1.0),
            ],
        ),
        (
            [0.0, -1.0, 0.0],
            [
                p(-1.0, -1.0, -1.0),
                p(1.0, -1.0, -1.0),
                p(1.0, -1.0, 1.0),
                p(-1.0, -1.0, 1.0),
            ],
        ),
    ];
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (normal, quad) in faces {
        let base = vertices.len() as u32;
        for (i, pos) in quad.iter().enumerate() {
            vertices.push(Vertex {
                position: *pos,
                normal,
                uv: [(i & 1) as f32, (i >> 1) as f32],
                color,
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let count = indices.len() as u32;
    renderer.create_mesh(
        &vertices,
        &indices,
        vec![SubMesh {
            index_start: 0,
            index_count: count,
            texture: None,
            color: [1.0; 4],
        }],
    )
}

/// Sol de secours (grille plate) quand aucun secteur n'est chargé.
pub fn ground_mesh(renderer: &Renderer, size: f32) -> Mesh {
    let h = size / 2.0;
    let vertices = vec![
        Vertex {
            position: [-h, 0.0, -h],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            color: [0.35, 0.5, 0.3, 1.0],
        },
        Vertex {
            position: [h, 0.0, -h],
            normal: [0.0, 1.0, 0.0],
            uv: [size / 500.0, 0.0],
            color: [0.35, 0.5, 0.3, 1.0],
        },
        Vertex {
            position: [h, 0.0, h],
            normal: [0.0, 1.0, 0.0],
            uv: [size / 500.0, size / 500.0],
            color: [0.35, 0.5, 0.3, 1.0],
        },
        Vertex {
            position: [-h, 0.0, h],
            normal: [0.0, 1.0, 0.0],
            uv: [0.0, size / 500.0],
            color: [0.35, 0.5, 0.3, 1.0],
        },
    ];
    renderer.create_mesh(
        &vertices,
        &[0, 2, 1, 0, 3, 2],
        vec![SubMesh {
            index_start: 0,
            index_count: 6,
            texture: None,
            color: [1.0; 4],
        }],
    )
}

/// Entité du monde affichée.
pub struct Entity {
    pub cid: i32,
    pub tid: i32,
    pub name: String,
    /// Derniers PV connus, mis à jour par les facteurs d'action reçus.
    pub hp: Option<i32>,
    pub position: Vec3,
    pub destination: Vec3,
    pub meshes: Vec<(Arc<Mesh>, Mat4)>,
    pub equipment: Vec<EquipmentView>,
    pub is_player: bool,
}

/// Nom du clump de modèle associé à un TID d'objet (colonne `BASE_DFF` de ItemTemplateAll.ini).
pub fn item_model_for_tid(assets: &Assets, tid: i32) -> Option<String> {
    if tid <= 0 {
        return None;
    }
    let table = assets.table("ini", "ItemTemplateAll.ini").ok()?;
    let header = table.section("")?;
    let dff_column = header
        .entries
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case("BASE_DFF"))
        .map(|(index, _)| index.as_str())?;
    let item = table.section(&tid.to_string())?;
    let dff = item.get(dff_column)?.trim().parse::<u32>().ok()?;
    (dff > 0).then(|| format!("d{dff:07x}.ecl"))
}

/// Nom affiché de l'objet depuis la colonne `Name` de ItemTemplateAll.ini.
pub fn item_name_for_tid(assets: &Assets, tid: i32) -> Option<String> {
    if tid <= 0 {
        return None;
    }
    let table = assets.table("ini", "ItemTemplateAll.ini").ok()?;
    let header = table.section("")?;
    let name_column = header
        .entries
        .iter()
        .find(|(_, name)| name.eq_ignore_ascii_case("Name"))
        .map(|(index, _)| index.as_str())?;
    table
        .section(&tid.to_string())?
        .get(name_column)
        .map(str::to_owned)
}

/// Charge le corps du personnage puis les modèles statiques des objets dans sa vue réseau.
pub fn entity_meshes_with_equipment(
    renderer: &mut Renderer,
    assets: &Assets,
    tid: i32,
    equipment: &[EquipmentView],
    fallback: &Arc<Mesh>,
) -> Result<Vec<(Arc<Mesh>, Mat4)>> {
    let mut meshes = entity_meshes(renderer, assets, tid, fallback)?;
    for item in equipment {
        let Some(file) = item_model_for_tid(assets, item.template_id) else {
            continue;
        };
        match model_meshes(renderer, assets, "character", &file) {
            Ok(part) => meshes.extend(part),
            Err(error) => log::debug!("objet TID {} (modèle {file}) : {error:#}", item.template_id),
        }
    }
    Ok(meshes)
}

impl Entity {
    pub fn model_matrix(&self) -> Mat4 {
        Mat4::from_translation(self.position)
    }
}

/// Fichiers de modèle d'un personnage d'après `charactertemplateclient.ini`.
///
/// La table est positionnelle : la section d'en-tête `[]` nomme les colonnes (`0=ModuleData1`,
/// `1=DFF`, `2=DEFAULT_ARMOUR_DFF`, `3=PICK_DFF`…) et chaque section `[TID]` donne
/// `colonne=valeur`. Les numéros sont écrits en **hexadécimal** sur 7 chiffres dans le pack
/// `character` : `a%07x` = squelette et corps (DFF), `b%07x` = armure par défaut
/// (DEFAULT_ARMOUR_DFF), `c%07x` = volume de sélection (PICK_DFF). Vérifié : TID 584 →
/// `a0000248.ecl`, TID 96 → `a0000060.ecl` + `b0000060.ecl`.
pub fn models_for_tid(assets: &Assets, tid: i32) -> Vec<String> {
    let Ok(table) = assets.table("ini", "charactertemplateclient.ini") else {
        return Vec::new();
    };
    let Some(header) = table.section("") else {
        return fallback_models(assets, tid);
    };
    let col = |name: &str| {
        header
            .entries
            .iter()
            .find(|(_, v)| v.eq_ignore_ascii_case(name))
            .map(|(k, _)| k.clone())
    };
    let Some(sec) = table.section(&tid.to_string()) else {
        return fallback_models(assets, tid);
    };
    let mut out = Vec::new();
    for (column, prefix) in [("DFF", 'a'), ("DEFAULT_ARMOUR_DFF", 'b')] {
        let Some(key) = col(column) else { continue };
        if let Some(n) = sec.get(&key).and_then(|v| v.trim().parse::<u32>().ok()) {
            out.push(format!("{prefix}{n:07x}.ecl"));
        }
    }
    out
}

/// Sans client, la table est vide : on suppose que le numéro de DFF est le TID lui-même, ce qui
/// est vrai pour les modèles vérifiés (96 → `a0000060`, 584 → `a0000248`). Les patchs peuvent
/// ainsi viser `character/a{tid:07x}.ecl` en mode `--no-client`. Avec un client, un TID absent de
/// la table reste sans modèle.
fn fallback_models(assets: &Assets, tid: i32) -> Vec<String> {
    if assets.has_client() || tid <= 0 {
        return Vec::new();
    }
    vec![format!("a{tid:07x}.ecl")]
}

/// Charge (ou fabrique) les maillages d'une entité : corps puis armure par défaut.
pub fn entity_meshes(
    renderer: &mut Renderer,
    assets: &Assets,
    tid: i32,
    fallback: &Arc<Mesh>,
) -> Result<Vec<(Arc<Mesh>, Mat4)>> {
    let files = models_for_tid(assets, tid);
    if files.is_empty() {
        log::warn!("TID {tid} : aucun modèle dans charactertemplateclient.ini");
    }
    let mut meshes = Vec::new();
    for file in &files {
        match model_meshes(renderer, assets, "character", file) {
            Ok(part) => {
                if part.is_empty() {
                    log::debug!("TID {tid} : {file} sans géométrie");
                }
                meshes.extend(part);
            }
            Err(e) if !assets.has_client() => log::debug!("TID {tid} : {file} : {e:#} → substitut"),
            Err(e) => log::warn!("TID {tid} : {file} : {e:#}"),
        }
    }
    if meshes.is_empty() {
        meshes.push((fallback.clone(), Mat4::IDENTITY));
    }
    Ok(meshes)
}
