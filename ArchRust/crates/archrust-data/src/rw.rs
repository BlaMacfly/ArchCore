//! Flux RenderWare 3.7 (bibliothèque sur laquelle le client est bâti : `RpClump`,
//! `RwTexDictionary`, `RpHAnimHierarchy`…).
//!
//! Un flux est une suite de blocs `u32 type, u32 taille, u32 version` imbriqués. Les types
//! rencontrés dans le client :
//!
//! | Type   | Nom                 | Rôle |
//! |--------|---------------------|------|
//! | 0x01   | Struct              | données brutes du bloc parent |
//! | 0x03   | Extension           | greffons (HAnim, Bin Mesh, Skin, nom de frame…) |
//! | 0x06   | Texture             | référence de texture d'un matériau |
//! | 0x07   | Material            | couleur, texture |
//! | 0x08   | Material List       | |
//! | 0x0E   | Frame List          | hiérarchie de transformations |
//! | 0x0F   | Geometry            | sommets, triangles, UV, couleurs |
//! | 0x10   | Clump               | un modèle (`.dff` / `.ecl`) |
//! | 0x14   | Atomic              | géométrie attachée à une frame |
//! | 0x15   | Texture Native      | raster D3D9 (DXT1/3/5, ARGB…) |
//! | 0x16   | Texture Dictionary  | ensemble de textures (`.txd` / `.tx1`) |
//! | 0x1A   | Geometry List       | |
//! | 0x11E  | HAnim PLG           | identifiants d'os d'une frame |
//! | 0x116  | Skin PLG            | poids de peau et matrices inverses |
//! | 0x50E  | Bin Mesh PLG        | listes/bandes de triangles par matériau |
//! | 0x253F2FE | Frame Node Name  | nom d'une frame |
//!
//! Le lecteur est tolérant : les blocs inconnus sont ignorés, les tailles sont vérifiées.

use glam::{Mat4, Vec3};

pub const ID_STRUCT: u32 = 0x01;
pub const ID_STRING: u32 = 0x02;
pub const ID_EXTENSION: u32 = 0x03;
pub const ID_TEXTURE: u32 = 0x06;
pub const ID_MATERIAL: u32 = 0x07;
pub const ID_MATERIAL_LIST: u32 = 0x08;
pub const ID_FRAME_LIST: u32 = 0x0e;
pub const ID_GEOMETRY: u32 = 0x0f;
pub const ID_CLUMP: u32 = 0x10;
pub const ID_ATOMIC: u32 = 0x14;
pub const ID_TEXTURE_NATIVE: u32 = 0x15;
pub const ID_TEXTURE_DICTIONARY: u32 = 0x16;
pub const ID_GEOMETRY_LIST: u32 = 0x1a;
pub const ID_ANIMATION: u32 = 0x1b;
pub const ID_HANIM_PLG: u32 = 0x11e;
pub const ID_SKIN_PLG: u32 = 0x116;
pub const ID_BIN_MESH_PLG: u32 = 0x50e;
pub const ID_FRAME_NODE_NAME: u32 = 0x253f2fe;

#[derive(Debug, thiserror::Error)]
pub enum RwError {
    #[error("flux tronqué ({0})")]
    Truncated(&'static str),
    #[error("bloc tronqué à {offset:#x} : type {id:#x}, taille annoncée {size}, octets restants {remaining}")]
    TruncatedChunk { offset: usize, id: u32, size: usize, remaining: usize },
    #[error("bloc {expected:#x} attendu, {got:#x} trouvé")]
    Unexpected { expected: u32, got: u32 },
    #[error("format non pris en charge : {0}")]
    Unsupported(String),
}

/// Lecteur de blocs.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub data: &'a [u8],
    pub pos: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct Chunk {
    pub id: u32,
    pub size: usize,
    pub version: u32,
    /// Début des données du bloc dans le tampon.
    pub start: usize,
}

impl Chunk {
    pub fn end(&self) -> usize {
        self.start + self.size
    }
    /// Version RenderWare décodée (ex. 0x37002 pour 3.7.0.2).
    pub fn rw_version(&self) -> u32 {
        decode_version(self.version)
    }
}

/// Décode l'estampille de version (`0x1C02002D` → 3.7.0.2 soit 0x37002).
pub fn decode_version(stamp: u32) -> u32 {
    if stamp & 0xffff0000 != 0 {
        let v = ((stamp >> 14) & 0x3ff00) | ((stamp >> 16) & 0x3f) | 0x30000;
        v
    } else {
        stamp << 8
    }
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
    pub fn bytes(&mut self, n: usize) -> Result<&'a [u8], RwError> {
        if self.remaining() < n {
            return Err(RwError::Truncated("octets"));
        }
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, RwError> {
        Ok(self.bytes(1)?[0])
    }
    pub fn u16(&mut self) -> Result<u16, RwError> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, RwError> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, RwError> {
        Ok(self.u32()? as i32)
    }
    pub fn f32(&mut self) -> Result<f32, RwError> {
        Ok(f32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    pub fn vec3(&mut self) -> Result<Vec3, RwError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    /// Lit un en-tête de bloc.
    pub fn chunk(&mut self) -> Result<Chunk, RwError> {
        let id = self.u32()?;
        let size = self.u32()? as usize;
        let version = self.u32()?;
        if self.remaining() < size {
            return Err(RwError::TruncatedChunk { offset: self.pos - 12, id, size, remaining: self.remaining() });
        }
        Ok(Chunk { id, size, version, start: self.pos })
    }
    /// Lit un bloc et vérifie son type.
    pub fn expect(&mut self, id: u32) -> Result<Chunk, RwError> {
        let c = self.chunk()?;
        if c.id != id {
            return Err(RwError::Unexpected { expected: id, got: c.id });
        }
        Ok(c)
    }
    /// Lecteur restreint au contenu d'un bloc ; avance le lecteur courant après le bloc.
    pub fn enter(&mut self, c: &Chunk) -> Reader<'a> {
        let sub = Reader { data: &self.data[..c.end()], pos: c.start };
        self.pos = c.end();
        sub
    }
    pub fn skip(&mut self, c: &Chunk) {
        self.pos = c.end();
    }
    /// Chaîne d'un bloc `String` (terminée par 0, complétée à 4).
    pub fn string_chunk(&mut self) -> Result<String, RwError> {
        let c = self.expect(ID_STRING)?;
        let raw = &self.data[c.start..c.end()];
        self.pos = c.end();
        Ok(cstr(raw))
    }
}

fn cstr(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Clump (modèle)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Frame {
    pub rotation: [[f32; 3]; 3],
    pub position: Vec3,
    pub parent: i32,
    pub flags: u32,
    pub name: String,
    pub hanim: Option<HAnim>,
}

impl Frame {
    /// Matrice locale (colonnes = vecteurs de la base, convention RenderWare).
    pub fn local_matrix(&self) -> Mat4 {
        let r = self.rotation;
        Mat4::from_cols_array(&[
            r[0][0], r[0][1], r[0][2], 0.0,
            r[1][0], r[1][1], r[1][2], 0.0,
            r[2][0], r[2][1], r[2][2], 0.0,
            self.position.x, self.position.y, self.position.z, 1.0,
        ])
    }
}

#[derive(Debug, Clone, Default)]
pub struct HAnim {
    pub node_id: u32,
    pub nodes: Vec<HAnimNode>,
    pub flags: u32,
    pub keyframe_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct HAnimNode {
    pub id: u32,
    pub index: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Texture {
    pub name: String,
    pub mask: String,
    pub filter: u8,
    pub address_u: u8,
    pub address_v: u8,
}

#[derive(Debug, Clone, Default)]
pub struct Material {
    pub color: [u8; 4],
    pub texture: Option<Texture>,
    pub ambient: f32,
    pub specular: f32,
    pub diffuse: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Triangle {
    pub v: [u16; 3],
    pub material: u16,
}

#[derive(Debug, Clone, Default)]
pub struct Skin {
    pub bone_count: u8,
    pub used_bones: Vec<u8>,
    /// 4 indices d'os par sommet.
    pub indices: Vec<[u8; 4]>,
    /// 4 poids par sommet.
    pub weights: Vec<[f32; 4]>,
    pub inverse_bind: Vec<Mat4>,
}

#[derive(Debug, Clone)]
pub struct BinMesh {
    pub material: u32,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub flags: u32,
    pub vertices: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub colors: Vec<[u8; 4]>,
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub triangles: Vec<Triangle>,
    pub bound_center: Vec3,
    pub bound_radius: f32,
    pub materials: Vec<Material>,
    pub skin: Option<Skin>,
    /// `true` = bandes de triangles, `false` = listes.
    pub tristrip: bool,
    pub meshes: Vec<BinMesh>,
}

pub const GEOM_TRISTRIP: u32 = 0x01;
pub const GEOM_POSITIONS: u32 = 0x02;
pub const GEOM_TEXTURED: u32 = 0x04;
pub const GEOM_PRELIT: u32 = 0x08;
pub const GEOM_NORMALS: u32 = 0x10;
pub const GEOM_LIGHT: u32 = 0x20;
pub const GEOM_MODULATE: u32 = 0x40;
pub const GEOM_TEXTURED2: u32 = 0x80;
pub const GEOM_NATIVE: u32 = 0x0100_0000;

#[derive(Debug, Clone, Copy)]
pub struct Atomic {
    pub frame: u32,
    pub geometry: u32,
    pub flags: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Clump {
    pub frames: Vec<Frame>,
    pub geometries: Vec<Geometry>,
    pub atomics: Vec<Atomic>,
}

impl Clump {
    /// Analyse un `.dff` / `.ecl` complet.
    pub fn parse(data: &[u8]) -> Result<Clump, RwError> {
        let mut r = Reader::new(data);
        let c = r.expect(ID_CLUMP)?;
        let mut r = r.enter(&c);
        let st = r.expect(ID_STRUCT)?;
        let mut s = r.enter(&st);
        let num_atomics = s.u32()?;
        let mut clump = Clump::default();
        // Frame list
        let fl = r.expect(ID_FRAME_LIST)?;
        let mut fr = r.enter(&fl);
        clump.frames = parse_frame_list(&mut fr)?;
        // Geometry list
        let gl = r.expect(ID_GEOMETRY_LIST)?;
        let mut gr = r.enter(&gl);
        let gst = gr.expect(ID_STRUCT)?;
        let mut gs = gr.enter(&gst);
        let num_geom = gs.u32()?;
        for _ in 0..num_geom {
            let gc = gr.expect(ID_GEOMETRY)?;
            let mut g = gr.enter(&gc);
            clump.geometries.push(parse_geometry(&mut g, gc.rw_version())?);
        }
        // Atomics (puis lumières/caméras/extension ignorées)
        for _ in 0..num_atomics {
            let ac = match r.chunk() {
                Ok(c) => c,
                Err(_) => break,
            };
            if ac.id != ID_ATOMIC {
                r.skip(&ac);
                continue;
            }
            let mut a = r.enter(&ac);
            let st = a.expect(ID_STRUCT)?;
            let mut s = a.enter(&st);
            clump.atomics.push(Atomic { frame: s.u32()?, geometry: s.u32()?, flags: s.u32()? });
        }
        Ok(clump)
    }

    /// Matrice monde de chaque frame (composition parent → enfant).
    pub fn world_matrices(&self) -> Vec<Mat4> {
        let mut out = vec![Mat4::IDENTITY; self.frames.len()];
        for (i, f) in self.frames.iter().enumerate() {
            let local = f.local_matrix();
            out[i] = if f.parent >= 0 && (f.parent as usize) < i { out[f.parent as usize] * local } else { local };
        }
        out
    }
}

fn parse_frame_list(r: &mut Reader) -> Result<Vec<Frame>, RwError> {
    let st = r.expect(ID_STRUCT)?;
    let mut s = r.enter(&st);
    let n = s.u32()? as usize;
    let mut frames = Vec::with_capacity(n);
    for _ in 0..n {
        let mut rot = [[0f32; 3]; 3];
        for row in rot.iter_mut() {
            for v in row.iter_mut() {
                *v = s.f32()?;
            }
        }
        let position = s.vec3()?;
        let parent = s.i32()?;
        let flags = s.u32()?;
        frames.push(Frame { rotation: rot, position, parent, flags, name: String::new(), hanim: None });
    }
    for frame in frames.iter_mut() {
        let Ok(ext) = r.chunk() else { break };
        if ext.id != ID_EXTENSION {
            r.skip(&ext);
            continue;
        }
        let mut e = r.enter(&ext);
        while e.remaining() >= 12 {
            let c = e.chunk()?;
            match c.id {
                ID_FRAME_NODE_NAME => {
                    frame.name = cstr(&e.data[c.start..c.end()]);
                    e.skip(&c);
                }
                ID_HANIM_PLG => {
                    let mut h = e.enter(&c);
                    let _version = h.u32()?;
                    let node_id = h.u32()?;
                    let num_nodes = h.u32()? as usize;
                    let mut hanim = HAnim { node_id, ..Default::default() };
                    if num_nodes > 0 {
                        hanim.flags = h.u32()?;
                        hanim.keyframe_size = h.u32()?;
                        for _ in 0..num_nodes {
                            hanim.nodes.push(HAnimNode { id: h.u32()?, index: h.u32()?, flags: h.u32()? });
                        }
                    }
                    frame.hanim = Some(hanim);
                }
                _ => e.skip(&c),
            }
        }
    }
    Ok(frames)
}

fn parse_geometry(r: &mut Reader, version: u32) -> Result<Geometry, RwError> {
    let st = r.expect(ID_STRUCT)?;
    let mut s = r.enter(&st);
    let flags = s.u32()?;
    let num_tris = s.u32()? as usize;
    let num_verts = s.u32()? as usize;
    let num_morph = s.u32()? as usize;
    if version < 0x34000 {
        let _ambient = s.f32()?;
        let _specular = s.f32()?;
        let _diffuse = s.f32()?;
    }
    let mut g = Geometry { flags, tristrip: flags & GEOM_TRISTRIP != 0, ..Default::default() };
    let native = flags & GEOM_NATIVE != 0;
    if !native {
        if flags & GEOM_PRELIT != 0 {
            for _ in 0..num_verts {
                let b = s.bytes(4)?;
                g.colors.push([b[0], b[1], b[2], b[3]]);
            }
        }
        let mut num_uv = ((flags >> 16) & 0xff) as usize;
        if num_uv == 0 && flags & (GEOM_TEXTURED | GEOM_TEXTURED2) != 0 {
            num_uv = if flags & GEOM_TEXTURED2 != 0 { 2 } else { 1 };
        }
        for _ in 0..num_uv {
            let mut set = Vec::with_capacity(num_verts);
            for _ in 0..num_verts {
                set.push([s.f32()?, s.f32()?]);
            }
            g.uv_sets.push(set);
        }
        for _ in 0..num_tris {
            let v2 = s.u16()?;
            let v1 = s.u16()?;
            let mat = s.u16()?;
            let v3 = s.u16()?;
            g.triangles.push(Triangle { v: [v1, v2, v3], material: mat });
        }
    }
    for m in 0..num_morph {
        let center = s.vec3()?;
        let radius = s.f32()?;
        let has_verts = s.u32()? != 0;
        let has_normals = s.u32()? != 0;
        if m == 0 {
            g.bound_center = center;
            g.bound_radius = radius;
        }
        if has_verts {
            let mut v = Vec::with_capacity(num_verts);
            for _ in 0..num_verts {
                v.push(s.vec3()?);
            }
            if m == 0 {
                g.vertices = v;
            }
        }
        if has_normals {
            let mut n = Vec::with_capacity(num_verts);
            for _ in 0..num_verts {
                n.push(s.vec3()?);
            }
            if m == 0 {
                g.normals = n;
            }
        }
    }
    // Material list
    let ml = r.expect(ID_MATERIAL_LIST)?;
    let mut mr = r.enter(&ml);
    let mst = mr.expect(ID_STRUCT)?;
    let mut ms = mr.enter(&mst);
    let num_mat = ms.u32()? as usize;
    let mut indices = Vec::with_capacity(num_mat);
    for _ in 0..num_mat {
        indices.push(ms.i32()?);
    }
    let mut parsed: Vec<Material> = Vec::new();
    for idx in indices {
        if idx >= 0 {
            let m = parsed.get(idx as usize).cloned().unwrap_or_default();
            g.materials.push(m);
            continue;
        }
        // Certaines géométries de terrain déclarent 72 emplacements (tous à -1), mais ne
        // sérialisent que les matériaux réellement utilisés. Les emplacements restants
        // précèdent alors les extensions propres au jeu, pas d'autres chunks Material.
        let mut next = mr;
        if !matches!(next.chunk(), Ok(Chunk { id: ID_MATERIAL, .. })) {
            let mat = Material { color: [255; 4], ..Default::default() };
            parsed.push(mat.clone());
            g.materials.push(mat);
            continue;
        }
        let mc = mr.expect(ID_MATERIAL)?;
        let mut m = mr.enter(&mc);
        let st = m.expect(ID_STRUCT)?;
        let mut s = m.enter(&st);
        let _flags = s.u32()?;
        let c = s.bytes(4)?;
        let color = [c[0], c[1], c[2], c[3]];
        let _unused = s.u32()?;
        let textured = s.u32()? != 0;
        let (mut ambient, mut specular, mut diffuse) = (1.0, 1.0, 1.0);
        if mc.rw_version() > 0x30400 && s.remaining() >= 12 {
            ambient = s.f32()?;
            specular = s.f32()?;
            diffuse = s.f32()?;
        }
        let mut mat = Material { color, texture: None, ambient, specular, diffuse };
        if textured {
            let tc = m.expect(ID_TEXTURE)?;
            let mut t = m.enter(&tc);
            let tst = t.expect(ID_STRUCT)?;
            let mut ts = t.enter(&tst);
            let filter_flags = ts.u16()?;
            let _ = ts.u16()?;
            let name = t.string_chunk()?;
            let mask = t.string_chunk()?;
            mat.texture = Some(Texture {
                name,
                mask,
                filter: (filter_flags & 0xff) as u8,
                address_u: ((filter_flags >> 8) & 0xf) as u8,
                address_v: ((filter_flags >> 12) & 0xf) as u8,
            });
        }
        parsed.push(mat.clone());
        g.materials.push(mat);
    }
    // Extension de la géométrie : Bin Mesh, Skin…
    while r.remaining() >= 12 {
        // Le client ajoute parfois des données privées après les extensions RenderWare;
        // un en-tête qui ne tient pas dans le chunk géométrique marque la fin lisible.
        let Ok(ec) = r.chunk() else { break };
        if ec.id != ID_EXTENSION {
            r.skip(&ec);
            continue;
        }
        let mut e = r.enter(&ec);
        while e.remaining() >= 12 {
            let Ok(c) = e.chunk() else { break };
            match c.id {
                ID_BIN_MESH_PLG => {
                    let mut b = e.enter(&c);
                    let kind = b.u32()?;
                    g.tristrip = kind == 1;
                    let num_meshes = b.u32()? as usize;
                    let _total = b.u32()?;
                    for _ in 0..num_meshes {
                        let n = b.u32()? as usize;
                        let material = b.u32()?;
                        let mut idx = Vec::with_capacity(n);
                        if !native {
                            for _ in 0..n {
                                idx.push(b.u32()?);
                            }
                        }
                        g.meshes.push(BinMesh { material, indices: idx });
                    }
                }
                ID_SKIN_PLG => {
                    let mut k = e.enter(&c);
                    let bone_count = k.u8()?;
                    let used = k.u8()? as usize;
                    let _max_weights = k.u8()?;
                    let _pad = k.u8()?;
                    let mut skin = Skin { bone_count, ..Default::default() };
                    skin.used_bones = k.bytes(used)?.to_vec();
                    for _ in 0..num_verts {
                        let b = k.bytes(4)?;
                        skin.indices.push([b[0], b[1], b[2], b[3]]);
                    }
                    for _ in 0..num_verts {
                        skin.weights.push([k.f32()?, k.f32()?, k.f32()?, k.f32()?]);
                    }
                    for _ in 0..bone_count {
                        let mut m = [0f32; 16];
                        for v in m.iter_mut() {
                            *v = k.f32()?;
                        }
                        skin.inverse_bind.push(Mat4::from_cols_array(&m));
                    }
                    g.skin = Some(skin);
                }
                _ => e.skip(&c),
            }
        }
    }
    Ok(g)
}

// ---------------------------------------------------------------------------------------------
// Secteur de terrain (entrée `D%d,%d.dff` des archives `b*.ma2`)
// ---------------------------------------------------------------------------------------------

/// Géométrie d'un secteur de terrain : `u32` nombre d'atomics (1), puis un bloc `Atomic`
/// autonome (Struct de 24 octets, Geometry standard à 6 jeux d'UV, extension).
#[derive(Debug, Clone)]
pub struct SectorGeometry {
    /// Les 6 entiers du Struct de l'atomic (identifiant, drapeaux… non interprétés).
    pub atomic_struct: Vec<u32>,
    pub geometry: Geometry,
}

impl SectorGeometry {
    pub fn parse(data: &[u8]) -> Result<SectorGeometry, RwError> {
        if data.len() < 16 {
            return Err(RwError::Truncated("secteur"));
        }
        let mut r = Reader::new(&data[4..]);
        let ac = r.expect(ID_ATOMIC)?;
        let mut a = r.enter(&ac);
        let st = a.expect(ID_STRUCT)?;
        let mut s = a.enter(&st);
        let mut atomic_struct = Vec::new();
        while s.remaining() >= 4 {
            atomic_struct.push(s.u32()?);
        }
        let gc = a.expect(ID_GEOMETRY)?;
        let mut g = a.enter(&gc);
        let geometry = parse_geometry(&mut g, gc.rw_version())?;
        Ok(SectorGeometry { atomic_struct, geometry })
    }

    /// Bornes des sommets (min, max).
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for v in &self.geometry.vertices {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
        (lo, hi)
    }
}

// ---------------------------------------------------------------------------------------------
// Dictionnaire de textures
// ---------------------------------------------------------------------------------------------

/// Formats de raster D3D9 rencontrés.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RasterFormat {
    Dxt1,
    Dxt3,
    Dxt5,
    /// B8G8R8A8 (D3DFMT_A8R8G8B8).
    Bgra8,
    /// B8G8R8X8.
    Bgrx8,
    /// Indexé 8 bits avec palette BGRA.
    Pal8,
    Other(u32),
}

#[derive(Debug, Clone)]
pub struct MipLevel {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TextureNative {
    pub name: String,
    pub mask: String,
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    pub format: RasterFormat,
    pub has_alpha: bool,
    pub palette: Option<Vec<[u8; 4]>>,
    pub mips: Vec<MipLevel>,
}

#[derive(Debug, Clone, Default)]
pub struct TextureDictionary {
    pub textures: Vec<TextureNative>,
}

const FOURCC_DXT1: u32 = 0x3154_5844;
const FOURCC_DXT3: u32 = 0x3354_5844;
const FOURCC_DXT5: u32 = 0x3554_5844;
const RASTER_PAL8: u32 = 0x2000;

impl TextureDictionary {
    /// Analyse un `.txd` (ou un `.tx1` déjà déchiffré).
    pub fn parse(data: &[u8]) -> Result<Self, RwError> {
        let mut r = Reader::new(data);
        let c = r.expect(ID_TEXTURE_DICTIONARY)?;
        let mut r = r.enter(&c);
        let st = r.expect(ID_STRUCT)?;
        let mut s = r.enter(&st);
        let count = s.u16()? as usize;
        let _device = s.u16()?;
        let mut dict = TextureDictionary::default();
        for _ in 0..count {
            let tc = r.expect(ID_TEXTURE_NATIVE)?;
            let mut t = r.enter(&tc);
            dict.textures.push(parse_texture_native(&mut t)?);
        }
        Ok(dict)
    }

    pub fn get(&self, name: &str) -> Option<&TextureNative> {
        self.textures.iter().find(|t| t.name.eq_ignore_ascii_case(name))
    }
}

fn parse_texture_native(r: &mut Reader) -> Result<TextureNative, RwError> {
    let st = r.expect(ID_STRUCT)?;
    let mut s = r.enter(&st);
    let platform = s.u32()?;
    if platform != 9 && platform != 8 {
        return Err(RwError::Unsupported(format!("plateforme de texture {platform}")));
    }
    let _filter = s.u32()?;
    let name = cstr(s.bytes(32)?);
    let mask = cstr(s.bytes(32)?);
    let raster_format = s.u32()?;
    let (d3d_format, d3d8_alpha) = if platform == 9 { (s.u32()?, false) } else { (0, s.u32()? != 0) };
    let width = s.u16()? as u32;
    let height = s.u16()? as u32;
    let depth = s.u8()?;
    let num_levels = s.u8()? as usize;
    let _raster_type = s.u8()?;
    let flags = s.u8()?;
    let (format, has_alpha, compressed) = if platform == 9 {
        let compressed = flags & 0x8 != 0;
        let f = if compressed {
            match d3d_format {
                FOURCC_DXT1 => RasterFormat::Dxt1,
                FOURCC_DXT3 => RasterFormat::Dxt3,
                FOURCC_DXT5 => RasterFormat::Dxt5,
                other => RasterFormat::Other(other),
            }
        } else if raster_format & RASTER_PAL8 != 0 {
            RasterFormat::Pal8
        } else {
            match d3d_format {
                21 => RasterFormat::Bgra8,
                22 => RasterFormat::Bgrx8,
                other => RasterFormat::Other(other),
            }
        };
        (f, flags & 0x1 != 0, compressed)
    } else {
        let f = match flags {
            1 => RasterFormat::Dxt1,
            3 => RasterFormat::Dxt3,
            5 => RasterFormat::Dxt5,
            _ if raster_format & RASTER_PAL8 != 0 => RasterFormat::Pal8,
            _ => RasterFormat::Bgra8,
        };
        (f, d3d8_alpha, flags != 0)
    };
    let _ = compressed;
    let palette = if format == RasterFormat::Pal8 {
        let raw = s.bytes(256 * 4)?;
        Some(raw.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect())
    } else {
        None
    };
    let mut mips = Vec::with_capacity(num_levels);
    let (mut w, mut h) = (width, height);
    for _ in 0..num_levels {
        let size = s.u32()? as usize;
        let data = s.bytes(size)?.to_vec();
        mips.push(MipLevel { width: w.max(1), height: h.max(1), data });
        w /= 2;
        h /= 2;
    }
    Ok(TextureNative { name, mask, width, height, depth, format, has_alpha, palette, mips })
}

impl TextureNative {
    /// Décode le niveau 0 en RGBA8 (DXT1/3/5, BGRA8, BGRX8, PAL8).
    pub fn to_rgba8(&self) -> Result<Vec<u8>, RwError> {
        let mip = self.mips.first().ok_or(RwError::Truncated("niveau de mipmap"))?;
        let (w, h) = (mip.width as usize, mip.height as usize);
        let mut out = vec![0u8; w * h * 4];
        match self.format {
            RasterFormat::Bgra8 | RasterFormat::Bgrx8 => {
                for (i, px) in mip.data.chunks_exact(4).take(w * h).enumerate() {
                    out[i * 4] = px[2];
                    out[i * 4 + 1] = px[1];
                    out[i * 4 + 2] = px[0];
                    out[i * 4 + 3] = if self.format == RasterFormat::Bgrx8 { 255 } else { px[3] };
                }
            }
            RasterFormat::Pal8 => {
                let pal = self.palette.as_ref().ok_or(RwError::Truncated("palette"))?;
                for (i, &idx) in mip.data.iter().take(w * h).enumerate() {
                    let c = pal[idx as usize];
                    out[i * 4..i * 4 + 4].copy_from_slice(&[c[2], c[1], c[0], c[3]]);
                }
            }
            RasterFormat::Dxt1 => decode_dxt(&mip.data, w, h, 1, &mut out),
            RasterFormat::Dxt3 => decode_dxt(&mip.data, w, h, 3, &mut out),
            RasterFormat::Dxt5 => decode_dxt(&mip.data, w, h, 5, &mut out),
            RasterFormat::Other(f) => return Err(RwError::Unsupported(format!("format D3D {f:#x}"))),
        }
        Ok(out)
    }
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 0x1f) as u32;
    let g = ((c >> 5) & 0x3f) as u32;
    let b = (c & 0x1f) as u32;
    [((r * 255 + 15) / 31) as u8, ((g * 255 + 31) / 63) as u8, ((b * 255 + 15) / 31) as u8]
}

/// Décodeur DXT1/DXT3/DXT5 (S3TC, format public) vers RGBA8.
fn decode_dxt(data: &[u8], w: usize, h: usize, variant: u8, out: &mut [u8]) {
    let block_size = if variant == 1 { 8 } else { 16 };
    let bw = w.div_ceil(4);
    let bh = h.div_ceil(4);
    for by in 0..bh {
        for bx in 0..bw {
            let off = (by * bw + bx) * block_size;
            if off + block_size > data.len() {
                return;
            }
            let block = &data[off..off + block_size];
            let (alpha_block, color_block) = if variant == 1 { (&[][..], block) } else { (&block[..8], &block[8..]) };
            let c0 = u16::from_le_bytes([color_block[0], color_block[1]]);
            let c1 = u16::from_le_bytes([color_block[2], color_block[3]]);
            let p0 = rgb565(c0);
            let p1 = rgb565(c1);
            let mut colors = [[0u8; 4]; 4];
            colors[0] = [p0[0], p0[1], p0[2], 255];
            colors[1] = [p1[0], p1[1], p1[2], 255];
            if variant != 1 || c0 > c1 {
                for i in 0..3 {
                    colors[2][i] = ((2 * p0[i] as u32 + p1[i] as u32) / 3) as u8;
                    colors[3][i] = ((p0[i] as u32 + 2 * p1[i] as u32) / 3) as u8;
                }
                colors[2][3] = 255;
                colors[3][3] = 255;
            } else {
                for i in 0..3 {
                    colors[2][i] = ((p0[i] as u32 + p1[i] as u32) / 2) as u8;
                }
                colors[2][3] = 255;
                colors[3] = [0, 0, 0, 0];
            }
            let bits = u32::from_le_bytes([color_block[4], color_block[5], color_block[6], color_block[7]]);
            // Alpha DXT5 : 2 valeurs de référence + 16 index de 3 bits.
            let mut a5 = [0u8; 8];
            let mut a5bits: u64 = 0;
            if variant == 5 {
                a5[0] = alpha_block[0];
                a5[1] = alpha_block[1];
                if a5[0] > a5[1] {
                    for i in 1..7 {
                        a5[i + 1] = (((7 - i) as u32 * a5[0] as u32 + i as u32 * a5[1] as u32) / 7) as u8;
                    }
                } else {
                    for i in 1..5 {
                        a5[i + 1] = (((5 - i) as u32 * a5[0] as u32 + i as u32 * a5[1] as u32) / 5) as u8;
                    }
                    a5[6] = 0;
                    a5[7] = 255;
                }
                for (i, &b) in alpha_block[2..8].iter().enumerate() {
                    a5bits |= (b as u64) << (8 * i);
                }
            }
            for py in 0..4 {
                for px in 0..4 {
                    let (x, y) = (bx * 4 + px, by * 4 + py);
                    if x >= w || y >= h {
                        continue;
                    }
                    let i = py * 4 + px;
                    let ci = ((bits >> (2 * i)) & 3) as usize;
                    let mut c = colors[ci];
                    match variant {
                        3 => {
                            // 16 demi-octets d'alpha dans 8 octets : le pixel i occupe l'octet i/2,
                            // demi-octet bas pour i pair, haut pour i impair.
                            let b = alpha_block[i / 2];
                            let nib = if i % 2 == 0 { b & 0xf } else { b >> 4 };
                            c[3] = nib * 17;
                        }
                        5 => {
                            let ai = ((a5bits >> (3 * i)) & 7) as usize;
                            c[3] = a5[ai];
                        }
                        _ => {}
                    }
                    let o = (y * w + x) * 4;
                    out[o..o + 4].copy_from_slice(&c);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_decode() {
        assert_eq!(decode_version(0x1c02002d), 0x37002);
        assert_eq!(decode_version(0x1803ffff), 0x36003);
    }

    #[test]
    fn dxt1_solid_block() {
        // Bloc DXT1 : c0 = c1 = rouge pur (0xF800), index tous à 0.
        let mut data = vec![0u8; 8];
        data[0] = 0x00;
        data[1] = 0xf8;
        data[2] = 0x00;
        data[3] = 0xf8;
        let mut out = vec![0u8; 4 * 4 * 4];
        decode_dxt(&data, 4, 4, 1, &mut out);
        assert_eq!(&out[..4], &[255, 0, 0, 255]);
        assert_eq!(&out[60..64], &[255, 0, 0, 255]);
    }

    #[test]
    fn chunk_reader_rejects_truncated() {
        let mut r = Reader::new(&[0x10, 0, 0, 0, 0xff, 0, 0, 0, 0x2d, 0, 2, 0x1c]);
        assert!(r.chunk().is_err());
    }
}
