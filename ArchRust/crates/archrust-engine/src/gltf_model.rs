//! Modèles glTF 2.0 / GLB fournis par les patchs (`[[models]]`).
//!
//! Chargement en deux temps, pour rester testable sans GPU :
//!
//! 1. [`GltfModel::parse`] lit le fichier (binaire GLB ou JSON `.gltf` avec tampons et images
//!    voisins ou en `data:` URI) : positions, normales, UV0, couleurs de sommet, indices, couleur
//!    de base et texture de chaque matériau, matrices monde des nœuds de la scène par défaut ;
//! 2. [`upload`] crée les maillages et textures GPU via [`Renderer::create_mesh`] et
//!    [`Renderer::upload_texture`], et renvoie des paires (maillage, matrice) comme
//!    [`crate::scene::build_clump_meshes`].
//!
//! Repère : glTF est Y vers le haut comme ArchLord ; seule l'échelle diffère (glTF en mètres,
//! ArchLord en unités ≈ centimètre), d'où la clé `scale` des `[[models]]`.
//!
//! [`sample_cube_glb`] fabrique un GLB de test (deux cubes texturés) utilisé par les tests et par
//! l'exemple `cube_glb` qui écrit un patch complet.

use crate::assets::placeholder_texture;
use crate::render::{Mesh, Renderer, SubMesh, Vertex};
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use glam::{Mat4, Vec3};
use std::sync::Arc;

/// Image RGBA8 décodée d'un glTF.
#[derive(Debug, Clone)]
pub struct GltfImage {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Primitive triangulée : sommets déjà au format du moteur, indices en liste de triangles.
#[derive(Debug, Clone)]
pub struct GltfPrimitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// `baseColorFactor` du matériau (déjà multiplié dans la couleur des sommets).
    pub base_color: [f32; 4],
    /// Index dans [`GltfModel::images`] de la texture de couleur de base.
    pub image: Option<usize>,
    pub material: String,
}

#[derive(Debug, Clone)]
pub struct GltfMesh {
    pub name: String,
    pub primitives: Vec<GltfPrimitive>,
}

/// Nœud de la scène par défaut portant un maillage, avec sa matrice monde.
#[derive(Debug, Clone)]
pub struct GltfInstance {
    pub node: String,
    pub mesh: usize,
    pub matrix: Mat4,
}

#[derive(Debug, Clone, Default)]
pub struct GltfModel {
    pub generator: String,
    pub version: String,
    pub extensions_used: Vec<String>,
    pub meshes: Vec<GltfMesh>,
    pub images: Vec<GltfImage>,
    pub instances: Vec<GltfInstance>,
    /// Primitives ignorées (points, lignes).
    pub skipped: usize,
}

/// Lecture d'un fichier voisin (`.bin`, image) : chemin relatif au glTF, déjà décodé (`%20` → espace).
pub type Neighbour<'a> = dyn Fn(&str) -> Result<Vec<u8>> + 'a;

impl GltfModel {
    /// Analyse un GLB ou un glTF JSON. `neighbour` lit les fichiers référencés par URI relative.
    pub fn parse(bytes: &[u8], neighbour: &Neighbour) -> Result<GltfModel> {
        let gltf::Gltf { document, mut blob } =
            gltf::Gltf::from_slice(bytes).context("glTF invalide")?;

        // Tampons : binaire GLB, data: URI ou fichier voisin.
        let mut buffers: Vec<Vec<u8>> = Vec::with_capacity(document.buffers().len());
        for buffer in document.buffers() {
            let data = match buffer.source() {
                gltf::buffer::Source::Bin => blob
                    .take()
                    .ok_or_else(|| anyhow!("tampon {} : bloc BIN absent", buffer.index()))?,
                gltf::buffer::Source::Uri(uri) => load_uri(uri, neighbour)
                    .with_context(|| format!("tampon {}", buffer.index()))?,
            };
            if data.len() < buffer.length() {
                bail!(
                    "tampon {} : {} octets lus, {} attendus",
                    buffer.index(),
                    data.len(),
                    buffer.length()
                );
            }
            buffers.push(data);
        }

        // Images : vue de tampon ou URI ; une image indécodable devient un substitut (damier).
        let mut images = Vec::with_capacity(document.images().len());
        for image in document.images() {
            let name = image
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("image{}", image.index()));
            let bytes: Result<Vec<u8>> = match image.source() {
                gltf::image::Source::View { view, .. } => {
                    let buf = buffers
                        .get(view.buffer().index())
                        .ok_or_else(|| anyhow!("vue {} : tampon manquant", view.index()))?;
                    let end = view.offset() + view.length();
                    buf.get(view.offset()..end)
                        .map(|s| s.to_vec())
                        .ok_or_else(|| anyhow!("vue {} hors du tampon", view.index()))
                }
                gltf::image::Source::Uri { uri, .. } => load_uri(uri, neighbour),
            };
            let decoded = bytes.and_then(|b| {
                image::load_from_memory(&b)
                    .map(|i| i.to_rgba8())
                    .map_err(|e| anyhow!("{e}"))
            });
            match decoded {
                Ok(rgba) => images.push(GltfImage {
                    name,
                    width: rgba.width(),
                    height: rgba.height(),
                    data: rgba.into_raw(),
                }),
                Err(e) => {
                    log::warn!("glTF : image {name} : {e} → substitut");
                    let p = placeholder_texture(&name);
                    images.push(GltfImage {
                        name,
                        width: p.width,
                        height: p.height,
                        data: p.data,
                    });
                }
            }
        }

        // Maillages.
        let mut skipped = 0;
        let mut meshes = Vec::with_capacity(document.meshes().len());
        for mesh in document.meshes() {
            let name = mesh
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("mesh{}", mesh.index()));
            let mut primitives = Vec::new();
            for prim in mesh.primitives() {
                let mode = prim.mode();
                if !matches!(
                    mode,
                    gltf::mesh::Mode::Triangles
                        | gltf::mesh::Mode::TriangleStrip
                        | gltf::mesh::Mode::TriangleFan
                ) {
                    skipped += 1;
                    continue;
                }
                let reader = prim.reader(|b| buffers.get(b.index()).map(|v| v.as_slice()));
                let Some(positions) = reader.read_positions() else {
                    skipped += 1;
                    continue;
                };
                let material = prim.material();
                let pbr = material.pbr_metallic_roughness();
                let base_color = pbr.base_color_factor();
                let image = pbr.base_color_texture().and_then(|info| {
                    if info.tex_coord() != 0 {
                        log::warn!(
                            "glTF : {name} utilise TEXCOORD_{} (seul le jeu 0 est pris en charge)",
                            info.tex_coord()
                        );
                    }
                    let idx = info.texture().source().index();
                    (idx < images.len()).then_some(idx)
                });
                let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
                let uvs: Option<Vec<[f32; 2]>> =
                    reader.read_tex_coords(0).map(|t| t.into_f32().collect());
                let colors: Option<Vec<[f32; 4]>> =
                    reader.read_colors(0).map(|c| c.into_rgba_f32().collect());
                let mut vertices: Vec<Vertex> = positions
                    .enumerate()
                    .map(|(i, p)| {
                        let c = colors
                            .as_ref()
                            .and_then(|c| c.get(i))
                            .copied()
                            .unwrap_or([1.0; 4]);
                        Vertex {
                            position: p,
                            normal: normals
                                .as_ref()
                                .and_then(|n| n.get(i))
                                .copied()
                                .unwrap_or([0.0, 1.0, 0.0]),
                            uv: uvs
                                .as_ref()
                                .and_then(|u| u.get(i))
                                .copied()
                                .unwrap_or([0.0, 0.0]),
                            color: [
                                c[0] * base_color[0],
                                c[1] * base_color[1],
                                c[2] * base_color[2],
                                c[3] * base_color[3],
                            ],
                        }
                    })
                    .collect();
                let n = vertices.len() as u32;
                let raw: Vec<u32> = match reader.read_indices() {
                    Some(idx) => idx.into_u32().filter(|&i| i < n).collect(),
                    None => (0..n).collect(),
                };
                let indices = match mode {
                    gltf::mesh::Mode::TriangleStrip => unroll_strip(&raw),
                    gltf::mesh::Mode::TriangleFan => unroll_fan(&raw),
                    _ => raw,
                };
                if normals.is_none() {
                    compute_normals(&mut vertices, &indices);
                }
                primitives.push(GltfPrimitive {
                    vertices,
                    indices,
                    base_color,
                    image,
                    material: material.name().unwrap_or("(sans nom)").to_string(),
                });
            }
            meshes.push(GltfMesh { name, primitives });
        }

        // Nœuds de la scène par défaut (ou de la première scène) ; sans scène, chaque maillage à l'identité.
        let mut instances = Vec::new();
        match document
            .default_scene()
            .or_else(|| document.scenes().next())
        {
            Some(scene) => {
                for node in scene.nodes() {
                    visit(&node, Mat4::IDENTITY, &mut instances);
                }
            }
            None => {
                for (i, m) in meshes.iter().enumerate() {
                    instances.push(GltfInstance {
                        node: m.name.clone(),
                        mesh: i,
                        matrix: Mat4::IDENTITY,
                    });
                }
            }
        }

        Ok(GltfModel {
            generator: document
                .as_json()
                .asset
                .generator
                .clone()
                .unwrap_or_default(),
            version: document.as_json().asset.version.clone(),
            extensions_used: document.extensions_used().map(str::to_string).collect(),
            meshes,
            images,
            instances,
            skipped,
        })
    }

    /// Positions de tous les sommets instanciés, dans le repère `root * matrice du nœud`.
    pub fn world_positions(&self, root: Mat4) -> Vec<Vec3> {
        let mut out = Vec::new();
        for inst in &self.instances {
            let Some(mesh) = self.meshes.get(inst.mesh) else {
                continue;
            };
            let m = root * inst.matrix;
            for p in &mesh.primitives {
                out.extend(
                    p.vertices
                        .iter()
                        .map(|v| m.transform_point3(Vec3::from_array(v.position))),
                );
            }
        }
        out
    }

    /// Boîte englobante des sommets instanciés (repère du modèle), si le modèle en a.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let pts = self.world_positions(Mat4::IDENTITY);
        if pts.is_empty() {
            return None;
        }
        Some(pts.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        ))
    }

    pub fn vertex_count(&self) -> usize {
        self.meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.vertices.len())
            .sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.indices.len() / 3)
            .sum()
    }

    /// Description lisible (pour `archrust inspect gltf`).
    pub fn describe(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("glTF {} ({}) : {} maillage(s), {} image(s), {} nœud(s) instancié(s), {} sommets, {} triangles", self.version, if self.generator.is_empty() { "générateur inconnu" } else { &self.generator }, self.meshes.len(), self.images.len(), self.instances.len(), self.vertex_count(), self.triangle_count()));
        if !self.extensions_used.is_empty() {
            s.push_str(&format!(
                "\n  extensions : {}",
                self.extensions_used.join(", ")
            ));
        }
        if self.skipped > 0 {
            s.push_str(&format!(
                "\n  {} primitive(s) ignorée(s) (points ou lignes)",
                self.skipped
            ));
        }
        if let Some((lo, hi)) = self.bounds() {
            s.push_str(&format!("\n  boîte englobante : [{:.3} {:.3} {:.3}] → [{:.3} {:.3} {:.3}] (×100 pour des mètres → unités ArchLord)", lo.x, lo.y, lo.z, hi.x, hi.y, hi.z));
        }
        for (i, img) in self.images.iter().enumerate() {
            s.push_str(&format!(
                "\n  image {i} {:?} : {}x{}",
                img.name, img.width, img.height
            ));
        }
        for (i, m) in self.meshes.iter().enumerate() {
            for (j, p) in m.primitives.iter().enumerate() {
                s.push_str(&format!(
                    "\n  maillage {i} {:?} primitive {j} : {} sommets, {} triangles, matériau {:?} couleur [{:.2} {:.2} {:.2} {:.2}]{}",
                    m.name,
                    p.vertices.len(),
                    p.indices.len() / 3,
                    p.material,
                    p.base_color[0],
                    p.base_color[1],
                    p.base_color[2],
                    p.base_color[3],
                    p.image.map(|k| format!(" texture image {k}")).unwrap_or_default()
                ));
            }
        }
        for inst in &self.instances {
            let (scale, rot, tr) = inst.matrix.to_scale_rotation_translation();
            s.push_str(&format!("\n  nœud {:?} → maillage {} : translation [{:.3} {:.3} {:.3}] échelle [{:.3} {:.3} {:.3}] rotation {:?}", inst.node, inst.mesh, tr.x, tr.y, tr.z, scale.x, scale.y, scale.z, rot));
        }
        s
    }
}

fn visit(node: &gltf::Node, parent: Mat4, out: &mut Vec<GltfInstance>) {
    let matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        out.push(GltfInstance {
            node: node
                .name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("node{}", node.index())),
            mesh: mesh.index(),
            matrix,
        });
    }
    for child in node.children() {
        visit(&child, matrix, out);
    }
}

/// Résout une URI glTF : `data:` (base64) ou fichier voisin (percent-encoding décodé).
pub fn load_uri(uri: &str, neighbour: &Neighbour) -> Result<Vec<u8>> {
    if let Some(rest) = uri.strip_prefix("data:") {
        let (meta, payload) = rest
            .split_once(',')
            .ok_or_else(|| anyhow!("data: URI sans virgule"))?;
        if meta.ends_with(";base64") {
            return base64::engine::general_purpose::STANDARD
                .decode(payload.trim())
                .map_err(|e| anyhow!("base64 : {e}"));
        }
        return Ok(percent_decode(payload).into_bytes());
    }
    let rel = percent_decode(uri);
    neighbour(&rel).with_context(|| format!("fichier voisin {rel}"))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unroll_strip(idx: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(idx.len().saturating_sub(2) * 3);
    for (i, w) in idx.windows(3).enumerate() {
        if w[0] == w[1] || w[1] == w[2] || w[0] == w[2] {
            continue;
        }
        if i % 2 == 0 {
            out.extend_from_slice(&[w[0], w[1], w[2]]);
        } else {
            out.extend_from_slice(&[w[1], w[0], w[2]]);
        }
    }
    out
}

fn unroll_fan(idx: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(idx.len().saturating_sub(2) * 3);
    if let Some((&center, rest)) = idx.split_first() {
        for w in rest.windows(2) {
            out.extend_from_slice(&[center, w[0], w[1]]);
        }
    }
    out
}

fn compute_normals(vertices: &mut [Vertex], indices: &[u32]) {
    let mut acc = vec![Vec3::ZERO; vertices.len()];
    for t in indices.chunks_exact(3) {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let pa = Vec3::from_array(vertices[a].position);
        let pb = Vec3::from_array(vertices[b].position);
        let pc = Vec3::from_array(vertices[c].position);
        let n = (pb - pa).cross(pc - pa);
        acc[a] += n;
        acc[b] += n;
        acc[c] += n;
    }
    for (v, n) in vertices.iter_mut().zip(acc) {
        v.normal = n.normalize_or(Vec3::Y).to_array();
    }
}

/// Téléverse un modèle : une texture GPU par image (nommée `{texture_prefix}#{index}`, réutilisée
/// si déjà présente), un maillage GPU par maillage glTF (une sous-partie par primitive), puis une
/// paire (maillage, matrice) par nœud instancié. `scale` est appliqué avant les matrices des nœuds.
pub fn upload(
    renderer: &mut Renderer,
    model: &GltfModel,
    texture_prefix: &str,
    scale: f32,
) -> Vec<(Arc<Mesh>, Mat4)> {
    let tex_names: Vec<String> = (0..model.images.len())
        .map(|i| format!("{texture_prefix}#{i}"))
        .collect();
    for (img, name) in model.images.iter().zip(&tex_names) {
        if renderer.texture(name).is_none() {
            renderer.upload_texture(name, img.width, img.height, &img.data);
        }
    }
    let mut gpu_meshes: Vec<Option<Arc<Mesh>>> = vec![None; model.meshes.len()];
    let root = Mat4::from_scale(Vec3::splat(if scale > 0.0 { scale } else { 1.0 }));
    let mut out = Vec::with_capacity(model.instances.len());
    for inst in &model.instances {
        let Some(mesh) = model.meshes.get(inst.mesh) else {
            continue;
        };
        if mesh.primitives.is_empty() {
            continue;
        }
        let gpu = gpu_meshes[inst.mesh].get_or_insert_with(|| {
            let mut vertices = Vec::new();
            let mut indices = Vec::new();
            let mut parts = Vec::new();
            for p in &mesh.primitives {
                let base = vertices.len() as u32;
                let start = indices.len() as u32;
                vertices.extend_from_slice(&p.vertices);
                indices.extend(p.indices.iter().map(|i| i + base));
                parts.push(SubMesh {
                    index_start: start,
                    index_count: indices.len() as u32 - start,
                    texture: p.image.map(|k| tex_names[k].clone()),
                    color: p.base_color,
                });
            }
            Arc::new(renderer.create_mesh(&vertices, &indices, parts))
        });
        out.push((gpu.clone(), root * inst.matrix));
    }
    out
}

/// Assemble un GLB (en-tête, bloc JSON, bloc BIN) à partir d'un document JSON et de son tampon.
pub fn write_glb(json: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = serde_json::to_vec(json).expect("JSON sérialisable");
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while bin_bytes.len() % 4 != 0 {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // "glTF"
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin_bytes);
    out
}

/// GLB de test : un cube unitaire texturé (damier PNG embarqué, 64×64) instancié deux fois —
/// nœud `corps` (1 m, posé sur le sol) et nœud `tete` (0,5 m, teinté, au-dessus) — soit 1,5 m de
/// haut. Les indices sont en `u16` et les deux maillages partagent les mêmes accesseurs.
pub fn sample_cube_glb() -> Vec<u8> {
    // 24 sommets : 6 faces × 4, normales par face, UV [0,1]².
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
    ];
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    for (n, u, v) in faces {
        let base = positions.len() as u16;
        for (su, sv, uv) in [
            (-1.0, -1.0, [0.0, 1.0]),
            (1.0, -1.0, [1.0, 1.0]),
            (1.0, 1.0, [1.0, 0.0]),
            (-1.0, 1.0, [0.0, 0.0]),
        ] {
            positions.push([
                0.5 * (n[0] + su * u[0] + sv * v[0]),
                0.5 * (n[1] + su * u[1] + sv * v[1]),
                0.5 * (n[2] + su * u[2] + sv * v[2]),
            ]);
            normals.push(n);
            uvs.push(uv);
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    // Damier 64×64 orange / bleu nuit, bord clair pour voir l'orientation des UV.
    let mut img = image::RgbaImage::new(64, 64);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let edge = x < 2 || y < 2 || x >= 62 || y >= 62;
        *px = if edge {
            image::Rgba([240, 240, 230, 255])
        } else if ((x / 8) + (y / 8)) % 2 == 0 {
            image::Rgba([225, 120, 40, 255])
        } else {
            image::Rgba([40, 60, 120, 255])
        };
    }
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png)
        .expect("PNG");
    let png = png.into_inner();

    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut push = |bytes: &[u8], target: Option<u32>| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(bytes);
        let mut v =
            serde_json::json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(t) = target {
            v["target"] = serde_json::json!(t);
        }
        views.push(v);
        views.len() - 1
    };
    let pos_view = push(bytemuck::cast_slice(&positions), Some(34962));
    let nrm_view = push(bytemuck::cast_slice(&normals), Some(34962));
    let uv_view = push(bytemuck::cast_slice(&uvs), Some(34962));
    let idx_view = push(bytemuck::cast_slice(&indices), Some(34963));
    let img_view = push(&png, None);
    let count = positions.len();
    let json = serde_json::json!({
        "asset": { "version": "2.0", "generator": "ArchRust sample_cube_glb" },
        "buffers": [{ "byteLength": bin.len() }],
        "bufferViews": views,
        "accessors": [
            { "bufferView": pos_view, "componentType": 5126, "count": count, "type": "VEC3", "min": [-0.5, -0.5, -0.5], "max": [0.5, 0.5, 0.5] },
            { "bufferView": nrm_view, "componentType": 5126, "count": count, "type": "VEC3" },
            { "bufferView": uv_view, "componentType": 5126, "count": count, "type": "VEC2" },
            { "bufferView": idx_view, "componentType": 5123, "count": indices.len(), "type": "SCALAR" }
        ],
        "images": [{ "name": "damier", "bufferView": img_view, "mimeType": "image/png" }],
        "samplers": [{ "magFilter": 9728, "minFilter": 9728, "wrapS": 10497, "wrapT": 10497 }],
        "textures": [{ "source": 0, "sampler": 0 }],
        "materials": [
            { "name": "damier", "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0.0 } },
            { "name": "damier-teinte", "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "baseColorFactor": [1.0, 0.7, 0.4, 1.0], "metallicFactor": 0.0 } }
        ],
        "meshes": [
            { "name": "cube", "primitives": [{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 0 }] },
            { "name": "cube-teinte", "primitives": [{ "attributes": { "POSITION": 0, "NORMAL": 1, "TEXCOORD_0": 2 }, "indices": 3, "material": 1 }] }
        ],
        "nodes": [
            { "name": "corps", "mesh": 0, "translation": [0.0, 0.5, 0.0] },
            { "name": "tete", "mesh": 1, "translation": [0.0, 1.25, 0.0], "scale": [0.5, 0.5, 0.5] }
        ],
        "scenes": [{ "name": "scene", "nodes": [0, 1] }],
        "scene": 0
    });
    write_glb(&json, &bin)
}

/// Manifeste d'un patch d'exemple remplaçant le corps du personnage humain (TID 96) par le cube.
pub fn sample_patch_manifest() -> &'static str {
    r#"# Patch d'exemple généré par `cargo run -p archrust-engine --example cube_glb -- patches/exemple-gltf`.
# Remplace le corps du personnage humain par défaut (TID 96 → character/a0000060.ecl) par un cube
# texturé en GLB. Sert à vérifier le chargeur glTF : `archrust play --no-client --screenshot docs/captures/gltf.png`.

[patch]
id = "exemple-gltf"
name = "Exemple glTF (cube texturé)"
version = "0.1.0"
author = "ArchRust"
priority = 10
description = "Cube GLB généré, à la place du modèle du joueur"

[[models]]
match = "character/a0000060.ecl"
file = "models/cube.glb"
scale = 100.0   # le GLB est en mètres, ArchLord en unités ≈ centimètre
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_neighbour(name: &str) -> Result<Vec<u8>> {
        Err(anyhow!("aucun voisin ({name})"))
    }

    #[test]
    fn sample_cube_parses() {
        let glb = sample_cube_glb();
        assert_eq!(&glb[0..4], b"glTF");
        let model = GltfModel::parse(&glb, &no_neighbour).unwrap();
        assert_eq!(model.version, "2.0");
        assert_eq!(model.meshes.len(), 2);
        assert_eq!(model.images.len(), 1);
        assert_eq!((model.images[0].width, model.images[0].height), (64, 64));
        assert_eq!(model.instances.len(), 2);
        assert_eq!(model.vertex_count(), 48);
        assert_eq!(model.triangle_count(), 24);
        assert_eq!(model.skipped, 0);
        let body = &model.meshes[0].primitives[0];
        assert_eq!(body.image, Some(0));
        assert_eq!(body.vertices[0].color, [1.0; 4]);
        assert_eq!(body.vertices[0].uv, [0.0, 1.0]);
        let head = &model.meshes[1].primitives[0];
        assert_eq!(head.base_color, [1.0, 0.7, 0.4, 1.0]);
        assert!((head.vertices[0].color[1] - 0.7).abs() < 1e-6);
        let (scale, _, tr) = model.instances[1].matrix.to_scale_rotation_translation();
        assert!((tr - Vec3::new(0.0, 1.25, 0.0)).length() < 1e-6);
        assert!((scale - Vec3::splat(0.5)).length() < 1e-6);
        let (lo, hi) = model.bounds().unwrap();
        assert!((lo - Vec3::new(-0.5, 0.0, -0.5)).length() < 1e-5);
        assert!((hi - Vec3::new(0.5, 1.5, 0.5)).length() < 1e-5);
        let desc = model.describe();
        assert!(
            desc.contains("2 maillage(s)") && desc.contains("\"tete\""),
            "{desc}"
        );
        // Échelle 100 : un personnage de 150 unités.
        let hi100 = model
            .world_positions(Mat4::from_scale(Vec3::splat(100.0)))
            .iter()
            .map(|p| p.y)
            .fold(f32::MIN, f32::max);
        assert!((hi100 - 150.0).abs() < 1e-3);
    }

    #[test]
    fn gltf_json_with_data_uri_and_neighbour() {
        // Triangle sans normales ni indices : tampon en data: URI, image dans un fichier voisin.
        let positions: [[f32; 3]; 3] = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let bin: &[u8] = bytemuck::cast_slice(&positions);
        let b64 = base64::engine::general_purpose::STANDARD.encode(bin);
        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "buffers": [{ "byteLength": bin.len(), "uri": format!("data:application/octet-stream;base64,{b64}") }],
            "bufferViews": [{ "buffer": 0, "byteLength": bin.len() }],
            "accessors": [{ "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] }],
            "images": [{ "uri": "textures/mon%20damier.png" }],
            "textures": [{ "source": 0 }],
            "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }],
            "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "material": 0 }] }],
            "nodes": [{ "mesh": 0, "matrix": [2.0,0.0,0.0,0.0, 0.0,2.0,0.0,0.0, 0.0,0.0,2.0,0.0, 3.0,0.0,0.0,1.0] }],
            "scenes": [{ "nodes": [0] }],
            "scene": 0
        });
        let text = serde_json::to_vec(&json).unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let png = png.into_inner();
        let asked = std::cell::RefCell::new(Vec::new());
        let neighbour = |name: &str| -> Result<Vec<u8>> {
            asked.borrow_mut().push(name.to_string());
            if name == "textures/mon damier.png" {
                Ok(png.clone())
            } else {
                Err(anyhow!("inconnu"))
            }
        };
        let model = GltfModel::parse(&text, &neighbour).unwrap();
        assert_eq!(asked.borrow().as_slice(), ["textures/mon damier.png"]);
        assert_eq!(model.images[0].data, [255, 0, 0, 255].repeat(4));
        let p = &model.meshes[0].primitives[0];
        assert_eq!(p.indices, vec![0, 1, 2]);
        assert_eq!(p.vertices[0].normal, [0.0, 0.0, 1.0]);
        let (lo, hi) = model.bounds().unwrap();
        assert_eq!(
            (lo, hi),
            (Vec3::new(3.0, 0.0, 0.0), Vec3::new(5.0, 2.0, 0.0))
        );
    }

    #[test]
    fn missing_image_becomes_placeholder() {
        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "images": [{ "uri": "absente.png" }],
            "scenes": [{ "nodes": [] }]
        });
        let model = GltfModel::parse(&serde_json::to_vec(&json).unwrap(), &no_neighbour).unwrap();
        assert_eq!((model.images[0].width, model.images[0].height), (64, 64));
        assert!(model.instances.is_empty());
    }

    #[test]
    fn strips_fans_and_uris() {
        assert_eq!(unroll_strip(&[0, 1, 2, 3]), vec![0, 1, 2, 2, 1, 3]);
        assert!(unroll_strip(&[0, 1, 1, 2]).is_empty());
        assert_eq!(unroll_fan(&[0, 1, 2, 3]), vec![0, 1, 2, 0, 2, 3]);
        assert_eq!(percent_decode("a%20b%2Fc%"), "a b/c%");
        assert_eq!(
            load_uri("data:text/plain,abc", &no_neighbour).unwrap(),
            b"abc"
        );
        assert_eq!(
            load_uri("data:application/octet-stream;base64,AQID", &no_neighbour).unwrap(),
            [1, 2, 3]
        );
        assert!(load_uri("x.bin", &no_neighbour).is_err());
    }
}
