//! Gestionnaire de ressources : packs du client + patchs HD, avec cache.
//!
//! Ordre de résolution d'une ressource :
//! 1. un patch HD actif (`archrust-patch`) qui la fournit ;
//! 2. un fichier en clair sous `Game/` (ex. `texture/notpacked/<nom>.tx1`) ;
//! 3. l'entrée correspondante d'un pack `Reference.Dat` + `Data.Dat`.

use anyhow::{anyhow, Context, Result};
use archrust_crypto::{capi_crypt, KEY_TEXTURE};
use archrust_data::ini::{CodePage, IniTable};
use archrust_data::magpack::MagPack;
use archrust_data::pack::PackIndex;
use archrust_data::rw::{Clump, TextureDictionary};
use archrust_patch::{PatchStack, ResourceKind};
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Image RGBA8 décodée.
#[derive(Debug, Clone)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// Facteur d'échelle UV demandé par un patch (1 = inchangé).
    pub uv_scale: f32,
    pub from_patch: Option<String>,
}

struct Pack {
    index: PackIndex,
    data: Mutex<File>,
}

/// Groupes de textures du client, dans l'ordre de recherche.
pub const TEXTURE_GROUPS: &[&str] = &["character", "object", "world", "item", "effect", "skill", "ui", "etc"];

pub struct Assets {
    pub game_dir: PathBuf,
    pub patches: PatchStack,
    packs: Mutex<HashMap<String, Arc<Pack>>>,
    textures: Mutex<HashMap<String, Arc<RgbaImage>>>,
    clumps: Mutex<HashMap<String, Arc<Clump>>>,
    tables: Mutex<HashMap<String, Arc<IniTable>>>,
}

impl Assets {
    pub fn new(game_dir: impl Into<PathBuf>, patches_dir: Option<&Path>) -> Self {
        let patches = patches_dir.map(PatchStack::load_dir).unwrap_or_default();
        if !patches.is_empty() {
            log::info!("patchs HD : {}", patches.patches.iter().map(|p| format!("{} ({})", p.manifest.patch.id, p.manifest.patch.priority)).collect::<Vec<_>>().join(", "));
        }
        Self {
            game_dir: game_dir.into(),
            patches,
            packs: Mutex::new(HashMap::new()),
            textures: Mutex::new(HashMap::new()),
            clumps: Mutex::new(HashMap::new()),
            tables: Mutex::new(HashMap::new()),
        }
    }

    fn pack(&self, rel_dir: &str) -> Result<Arc<Pack>> {
        if let Some(p) = self.packs.lock().unwrap().get(rel_dir) {
            return Ok(p.clone());
        }
        let dir = self.game_dir.join(rel_dir);
        let find = |name: &str| std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_name().map(|f| f.to_string_lossy().eq_ignore_ascii_case(name)).unwrap_or(false));
        let ref_path = find("Reference.Dat").ok_or_else(|| anyhow!("pack {rel_dir} : Reference.Dat absent"))?;
        let data_path = find("Data.Dat").ok_or_else(|| anyhow!("pack {rel_dir} : Data.Dat absent"))?;
        let index = PackIndex::parse_encrypted(&std::fs::read(&ref_path)?).with_context(|| format!("index {}", ref_path.display()))?;
        let data = File::open(&data_path)?;
        let pack = Arc::new(Pack { index, data: Mutex::new(data) });
        log::debug!("pack {rel_dir} : {} entrées", pack.index.entries.len());
        self.packs.lock().unwrap().insert(rel_dir.to_string(), pack.clone());
        Ok(pack)
    }

    /// Lit une entrée d'un pack (`rel_dir` = dossier du pack, `name` = nom dans l'index).
    pub fn read_packed(&self, rel_dir: &str, name: &str) -> Result<Vec<u8>> {
        let pack = self.pack(rel_dir)?;
        let entry = pack.index.find(name).ok_or_else(|| anyhow!("{name} absent du pack {rel_dir}"))?.clone();
        let mut f = pack.data.lock().unwrap();
        Ok(pack.index.read_entry(&mut *f, &entry)?)
    }

    /// Noms des entrées d'un pack.
    pub fn list_packed(&self, rel_dir: &str) -> Result<Vec<String>> {
        Ok(self.pack(rel_dir)?.index.entries.iter().map(|e| e.name.clone()).collect())
    }

    /// Fichier brut du client, en passant par les patchs (`[[files]]`) puis le disque.
    pub fn read_file(&self, rel_path: &str) -> Result<Vec<u8>> {
        if let Some(r) = self.patches.read(ResourceKind::File, rel_path) {
            let (bytes, res) = r?;
            log::debug!("{rel_path} fourni par le patch {}", res.patch_id);
            return Ok(bytes);
        }
        let p = self.game_dir.join(rel_path.replace('\\', "/"));
        std::fs::read(&p).with_context(|| format!("lecture de {}", p.display()))
    }

    /// Table `ini` déchiffrée (cache). `dir` = `ini`, `ini_fr`…
    pub fn table(&self, dir: &str, file: &str) -> Result<Arc<IniTable>> {
        let key = format!("{dir}/{file}").to_ascii_lowercase();
        if let Some(t) = self.tables.lock().unwrap().get(&key) {
            return Ok(t.clone());
        }
        let raw = self.read_file(&format!("{dir}/{file}"))?;
        let cp = CodePage::for_dir(dir);
        // Les tables de langue (`ini_fr`…) sont parfois déjà en clair.
        let looks_plain = raw.iter().take(256).filter(|&&b| b < 0x20 && b != b'\r' && b != b'\n' && b != b'\t').count() * 20 < raw.len().min(256);
        let text = if looks_plain { archrust_data::ini::decode_text(&raw, cp) } else { archrust_data::ini::decrypt_ini_with(&raw, cp) };
        let table = Arc::new(IniTable::parse(&text));
        self.tables.lock().unwrap().insert(key, table.clone());
        Ok(table)
    }

    /// Texture par nom RenderWare (ex. `A00005AA`), décodée en RGBA8.
    pub fn texture(&self, name: &str) -> Result<Arc<RgbaImage>> {
        let key = name.to_ascii_lowercase();
        if let Some(t) = self.textures.lock().unwrap().get(&key) {
            return Ok(t.clone());
        }
        let img = Arc::new(self.load_texture_uncached(&key)?);
        self.textures.lock().unwrap().insert(key, img.clone());
        Ok(img)
    }

    fn load_texture_uncached(&self, name: &str) -> Result<RgbaImage> {
        // 1. Patch HD
        if let Some(r) = self.patches.read(ResourceKind::Texture, name) {
            let (bytes, res) = r?;
            let img = image::load_from_memory(&bytes).with_context(|| format!("image du patch {} pour {name}", res.patch_id))?.to_rgba8();
            return Ok(RgbaImage { width: img.width(), height: img.height(), data: img.into_raw(), uv_scale: res.scale, from_patch: Some(res.patch_id) });
        }
        // 2. Fichier en clair
        let file = format!("{name}.tx1");
        let notpacked = self.game_dir.join("texture/notpacked").join(&file);
        let raw = if notpacked.exists() {
            Some(std::fs::read(&notpacked)?)
        } else {
            // 3. Packs par groupe
            let mut found = None;
            for g in TEXTURE_GROUPS {
                let dir = format!("texture/{g}");
                if let Ok(pack) = self.pack(&dir) {
                    if pack.index.find(&file).is_some() {
                        found = Some(self.read_packed(&dir, &file)?);
                        break;
                    }
                }
            }
            found
        };
        let raw = raw.ok_or_else(|| anyhow!("texture {name} introuvable"))?;
        let plain = capi_crypt(KEY_TEXTURE, &raw);
        let dict = TextureDictionary::parse(&plain).with_context(|| format!("dictionnaire {name}"))?;
        let tex = dict.textures.first().ok_or_else(|| anyhow!("dictionnaire {name} vide"))?;
        let data = tex.to_rgba8()?;
        let mip = &tex.mips[0];
        Ok(RgbaImage { width: mip.width, height: mip.height, data, uv_scale: 1.0, from_patch: None })
    }

    /// Image PNG/BMP/DDS d'un pack ou du disque (interface, cartes).
    pub fn image(&self, rel_dir: &str, name: &str) -> Result<RgbaImage> {
        let bytes = match self.read_packed(rel_dir, name) {
            Ok(b) => b,
            Err(_) => self.read_file(&format!("{rel_dir}/{name}"))?,
        };
        let img = image::load_from_memory(&bytes).with_context(|| format!("décodage de {rel_dir}/{name}"))?.to_rgba8();
        Ok(RgbaImage { width: img.width(), height: img.height(), data: img.into_raw(), uv_scale: 1.0, from_patch: None })
    }

    /// Modèle RenderWare d'un pack (`character`, `object`, `effect/clump`…).
    pub fn clump(&self, rel_dir: &str, name: &str) -> Result<Arc<Clump>> {
        let key = format!("{rel_dir}/{name}").to_ascii_lowercase();
        if let Some(c) = self.clumps.lock().unwrap().get(&key) {
            return Ok(c.clone());
        }
        let bytes = self.read_packed(rel_dir, name)?;
        let clump = Arc::new(Clump::parse(&bytes).with_context(|| format!("clump {key}"))?);
        self.clumps.lock().unwrap().insert(key, clump.clone());
        Ok(clump)
    }

    /// Archive MagPack d'une tuile du monde (`a`/`b` + numéro + `x.ma1`/`.ma2`).
    pub fn magpack(&self, prefix: char, tile: u32, ext: &str) -> Result<MagPack> {
        let name = format!("world/{prefix}{tile:06}x.{ext}");
        let bytes = self.read_file(&name)?;
        Ok(MagPack::parse(&bytes).with_context(|| name)?)
    }
}

/// Numéro de tuile (`a001717x`) à partir de coordonnées de secteur (`C272,272`) : 16 secteurs par
/// tuile, les tuiles sont numérotées `yy` `xx` sur deux chiffres chacune.
pub fn tile_of_sector(sx: i32, sy: i32) -> u32 {
    let tx = (sx / 16) as u32;
    let ty = (sy / 16) as u32;
    tx * 100 + ty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_numbering() {
        // a001717x contient C272,272 … C287,287 : 272/16 = 17.
        assert_eq!(tile_of_sector(272, 272), 1717);
        assert_eq!(tile_of_sector(287, 272), 1717);
        assert_eq!(tile_of_sector(288, 272), 1817);
    }
}
