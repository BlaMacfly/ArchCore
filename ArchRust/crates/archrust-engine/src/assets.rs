//! Gestionnaire de ressources : patchs HD d'ArchRust, puis système de fichiers virtuel du client.
//!
//! Ordre de résolution d'une ressource :
//! 1. un patch actif (`archrust-patch`, dont `free-assets/`) qui la fournit ;
//! 2. le système de fichiers virtuel du client, reproduit d'après Archonia.exe (voir [`Assets::open`]).
//!
//! Logique du client reprise ici (relevée avec REA/Ghidra, sha256 e9c7609b…) :
//! * au démarrage, chaque dossier contenant un `Reference.Dat` devient un pack, indexé par son
//!   chemin relatif en minuscules (`character`, `texture\character\high`…) ;
//! * ouvrir `dossier\fichier` : si un pack existe pour `dossier`, seule son entrée `fichier` compte
//!   (pas de repli sur le disque) ; sinon le fichier en clair est lu sur le disque ;
//! * en cas d'échec, les chargeurs de modèles réessaient dans le pack de surcharge `etc` sous le nom
//!   complet `dossier\fichier` ; ce qui vient de `etc` est chiffré AES-GCM (seconde clé) ;
//! * un modèle `X.ecl` est d'abord cherché sous la forme `X.glb` dans le même dossier ;
//! * une texture est cherchée, sous le nom `<nom>.tx1`, dans les dossiers de l'ordre officiel
//!   (effets, interface, personnages, objets, monde, compétences, objets d'inventaire, divers,
//!   minicarte, non empaquetés…), avec les sous-dossiers `high`/`medium`/`low` choisis par
//!   `TexCharacter`, `TexObject`, `TexEffect`, `TexWorld` de `ini/COption.ini`.
//!
//! Les modèles glTF/GLB (patchs `[[models]]` ou `.glb` natifs du client) sont analysés une fois
//! ([`Assets::patched_model`]) puis téléversés par `scene::patched_model_meshes`.

use crate::gltf_model::GltfModel;
use anyhow::{anyhow, Context, Result};
use archrust_crypto::{capi_crypt, decrypt_overlay_entry, KEY_TEXTURE};
use archrust_data::ini::{CodePage, IniTable};
use archrust_data::magpack::MagPack;
use archrust_data::pack::PackIndex;
use archrust_data::{
    ean::EanFile,
    rw::{Clump, TextureDictionary},
};
use archrust_patch::{PatchStack, Resolved, ResourceKind};
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

/// Modèle glTF fourni par un patch (`[[models]]`), analysé côté CPU et mis en cache.
#[derive(Debug)]
pub struct PatchedModel {
    pub model: GltfModel,
    pub patch_id: String,
    pub file: String,
    /// Facteur d'échelle du manifeste (glTF en mètres → unités ArchLord).
    pub scale: f32,
}

impl PatchedModel {
    /// Préfixe des noms de textures GPU de ce modèle (uniques par patch et fichier).
    pub fn texture_prefix(&self) -> String {
        format!("gltf:{}:{}", self.patch_id, self.file).to_ascii_lowercase()
    }
}

/// Groupes de textures empaquetés du client (outils d'inspection). Le chargeur, lui, suit
/// [`texture_search_path`], l'ordre exact du client.
pub const TEXTURE_GROUPS: &[&str] = &[
    "character",
    "object",
    "world",
    "item",
    "effect",
    "skill",
    "ui",
    "etc",
];

/// Dossier du pack de surcharge chiffré (racine `packed_client`).
pub const OVERLAY_DIR: &str = "etc";

/// Qualité d'une famille de textures (`TexCharacter`… de `COption.ini`, `[Patch_Option]`).
/// Valeurs du client : 0 = dossier de base, 1 = `High\`, 2 = `Medium\`, 3 = `Low\`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    #[default]
    Base,
    High,
    Medium,
    Low,
}

impl Quality {
    pub fn from_option(v: i64) -> Quality {
        match v {
            1 => Quality::High,
            2 => Quality::Medium,
            3 => Quality::Low,
            _ => Quality::Base,
        }
    }
    /// Sous-dossier ajouté au dossier de la famille (`""`, `"\\high"`…), en minuscules.
    pub fn suffix(self) -> &'static str {
        match self {
            Quality::Base => "",
            Quality::High => "\\high",
            Quality::Medium => "\\medium",
            Quality::Low => "\\low",
        }
    }
}

/// Qualités par famille, comme les quatre réglages du client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextureQuality {
    pub character: Quality,
    pub object: Quality,
    pub effect: Quality,
    pub world: Quality,
}

impl TextureQuality {
    /// Lit `[Patch_Option]` de `ini/COption.ini` (fichier en clair). Absent ou illisible : base.
    pub fn from_coption(text: &str) -> TextureQuality {
        let table = IniTable::parse(text);
        let get = |k: &str| {
            table
                .section("Patch_Option")
                .and_then(|s| s.get_i64(k))
                .map(Quality::from_option)
                .unwrap_or_default()
        };
        TextureQuality {
            character: get("TexCharacter"),
            object: get("TexObject"),
            effect: get("TexEffect"),
            world: get("TexWorld"),
        }
    }
}

/// Chemin de recherche des textures du client (`RwImageSetPath`), dans l'ordre exact
/// d'Archonia.exe : `Texture\Effect\<q>;Texture\UI\;Texture\Character\<q>;Texture\Object\<q>;
/// Texture\World\<q>;Texture\SKILL\;Texture\Item\;Texture\Etc\;Texture\Minimap\;
/// Texture\NotPacked\;Texture\AmbOccl\;Map\Temp\Rough\` (dossiers en minuscules, sans `\` final).
pub fn texture_search_path(q: &TextureQuality) -> Vec<String> {
    vec![
        format!("texture\\effect{}", q.effect.suffix()),
        "texture\\ui".into(),
        format!("texture\\character{}", q.character.suffix()),
        format!("texture\\object{}", q.object.suffix()),
        format!("texture\\world{}", q.world.suffix()),
        "texture\\skill".into(),
        "texture\\item".into(),
        "texture\\etc".into(),
        "texture\\minimap".into(),
        "texture\\notpacked".into(),
        "texture\\amboccl".into(),
        "map\\temp\\rough".into(),
    ]
}

/// Normalise un chemin du client : séparateurs `\`, minuscules, sans `.\` ni `\` en tête.
pub fn normalize_client_path(path: &str) -> String {
    let p = path.replace('/', "\\").to_ascii_lowercase();
    let p = p.trim_start_matches(".\\").trim_start_matches('\\');
    p.to_string()
}

/// Sépare `dossier\fichier` ; un chemin sans dossier appartient au pack `root`.
pub fn split_client_path(path: &str) -> (String, String) {
    let p = normalize_client_path(path);
    match p.rsplit_once('\\') {
        Some((d, f)) => (d.to_string(), f.to_string()),
        None => ("root".to_string(), p),
    }
}

/// Dossiers où ArchRust cherche le client ArchLord quand `--game` n'est pas donné, dans l'ordre.
pub const CLIENT_DIR_CANDIDATES: &[&str] = &[
    "assets-archonia/Game",
    "assets-archonia",
    "../client/ArchLord/Game",
];

/// Résout le dossier du client : le chemin explicite s'il est donné, sinon le dossier de
/// l'exécutable s'il ressemble à un dossier `Game` (présence de `character/` ou d'`ini/` :
/// `ArchRust.exe` posé à côté d'`Archonia.exe`), sinon le premier candidat qui y ressemble.
/// `None` = mode sans client.
pub fn find_client_dir(explicit: Option<&Path>) -> Option<PathBuf> {
    let looks_like_client = |p: &Path| p.join("character").is_dir() || p.join("ini").is_dir();
    if let Some(p) = explicit {
        return Some(p.to_path_buf());
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    beside_exe
        .into_iter()
        .chain(CLIENT_DIR_CANDIDATES.iter().map(PathBuf::from))
        .find(|p| looks_like_client(p))
}

pub struct Assets {
    /// Dossier `Game` du client, ou `None` en mode sans client (substituts et patchs seulement).
    pub game_dir: Option<PathBuf>,
    pub patches: PatchStack,
    /// Qualité des textures lue dans `ini/COption.ini` du client.
    pub quality: TextureQuality,
    /// Packs ouverts (ou absents : `None`), indexés par dossier normalisé.
    packs: Mutex<HashMap<String, Option<Arc<Pack>>>>,
    textures: Mutex<HashMap<String, Arc<RgbaImage>>>,
    clumps: Mutex<HashMap<String, Arc<Clump>>>,
    animations: Mutex<HashMap<String, Arc<EanFile>>>,
    tables: Mutex<HashMap<String, Arc<IniTable>>>,
    models: Mutex<HashMap<String, Arc<PatchedModel>>>,
    /// Jeux d'effets (`effect/ini/new/efffile_*.eff`), lus à la première demande.
    effect_sets: Mutex<Option<Arc<archrust_data::EffectSets>>>,
}

impl Assets {
    /// `patch_dirs` : un dossier contenant `patch.toml` est chargé comme un patch (ex. `free-assets/`),
    /// sinon comme un dossier de patchs (`patches/`). La pile reste triée par priorité.
    pub fn new(game_dir: Option<PathBuf>, patch_dirs: &[PathBuf]) -> Self {
        let mut patches = PatchStack::new();
        for dir in patch_dirs {
            if dir.join("patch.toml").is_file() {
                match PatchStack::load_one(dir) {
                    Ok(p) => {
                        if let Err(e) = p.validate() {
                            log::warn!("patch {} : {e}", p.manifest.patch.id);
                        }
                        patches.push(p);
                    }
                    Err(e) => log::warn!("patch {} ignoré : {e}", dir.display()),
                }
            } else if dir.is_dir() {
                for p in PatchStack::load_dir(dir).patches {
                    patches.push(p);
                }
            }
        }
        if !patches.is_empty() {
            log::info!(
                "patchs : {}",
                patches
                    .patches
                    .iter()
                    .map(|p| format!("{} ({})", p.manifest.patch.id, p.manifest.patch.priority))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        match &game_dir {
            Some(g) => log::info!("client : {}", g.display()),
            None => log::warn!("mode sans client : seules les ressources des patchs sont utilisées, le reste est remplacé par des substituts"),
        }
        let quality = game_dir
            .as_ref()
            .and_then(|g| std::fs::read(g.join("ini").join("COption.ini")).ok())
            .map(|b| {
                TextureQuality::from_coption(&archrust_data::ini::decode_text(
                    &b,
                    CodePage::Windows1252,
                ))
            })
            .unwrap_or_default();
        if game_dir.is_some() {
            log::info!("qualité des textures (COption.ini) : {quality:?}");
        }
        Self {
            game_dir,
            patches,
            quality,
            packs: Mutex::new(HashMap::new()),
            textures: Mutex::new(HashMap::new()),
            clumps: Mutex::new(HashMap::new()),
            animations: Mutex::new(HashMap::new()),
            tables: Mutex::new(HashMap::new()),
            models: Mutex::new(HashMap::new()),
            effect_sets: Mutex::new(None),
        }
    }

    /// Jeux d'effets du client (`effect/ini/new/efffile_set.eff` + `efffile_var.eff`), en cache.
    /// Sans client ou sans ces fichiers : table vide (un message de débogage).
    pub fn effect_sets(&self) -> Arc<archrust_data::EffectSets> {
        let mut slot = self.effect_sets.lock().unwrap();
        if let Some(s) = &*slot {
            return s.clone();
        }
        let loaded = (|| -> Result<archrust_data::EffectSets> {
            let set = self.read_file("effect/ini/new/efffile_set.eff")?;
            let var = self.read_file("effect/ini/new/efffile_var.eff")?;
            Ok(archrust_data::EffectSets::parse(&set, &var))
        })();
        let sets = match loaded {
            Ok(s) => {
                log::debug!("jeux d'effets : {} fiches", s.len());
                s
            }
            Err(e) => {
                log::debug!("jeux d'effets indisponibles ({e:#}) : table vide");
                archrust_data::EffectSets::default()
            }
        };
        let sets = Arc::new(sets);
        *slot = Some(sets.clone());
        sets
    }

    /// Raccourci : client + dossiers `free-assets/` et `patches/` du dépôt.
    pub fn with_defaults(game_dir: Option<PathBuf>) -> Self {
        Self::new(
            game_dir,
            &[PathBuf::from("free-assets"), PathBuf::from("patches")],
        )
    }

    pub fn has_client(&self) -> bool {
        self.game_dir.is_some()
    }

    fn game_dir(&self) -> Result<&Path> {
        self.game_dir
            .as_deref()
            .ok_or_else(|| anyhow!("aucun client ArchLord (mode sans client)"))
    }

    /// Pack du dossier `dir` (chemin du client, normalisé ici), ou `None` s'il n'y a pas de
    /// `Reference.Dat` dans ce dossier. Le résultat, présent ou absent, est mis en cache.
    fn pack_opt(&self, dir: &str) -> Result<Option<Arc<Pack>>> {
        let key = normalize_client_path(dir);
        if let Some(p) = self.packs.lock().unwrap().get(&key) {
            return Ok(p.clone());
        }
        let disk = if key == "root" {
            self.game_dir()?.to_path_buf()
        } else {
            self.game_dir()?.join(key.replace('\\', "/"))
        };
        let find = |name: &str| {
            std::fs::read_dir(&disk)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .map(|f| f.to_string_lossy().eq_ignore_ascii_case(name))
                        .unwrap_or(false)
                })
        };
        let pack = match (find("Reference.Dat"), find("Data.Dat")) {
            (Some(ref_path), Some(data_path)) => {
                let index = PackIndex::parse_encrypted(&std::fs::read(&ref_path)?)
                    .with_context(|| format!("index {}", ref_path.display()))?;
                let data = File::open(&data_path)?;
                log::debug!("pack {key} : {} entrées", index.entries.len());
                Some(Arc::new(Pack {
                    index,
                    data: Mutex::new(data),
                }))
            }
            _ => None,
        };
        self.packs.lock().unwrap().insert(key, pack.clone());
        Ok(pack)
    }

    fn pack(&self, rel_dir: &str) -> Result<Arc<Pack>> {
        self.pack_opt(rel_dir)?
            .ok_or_else(|| anyhow!("pack {rel_dir} : Reference.Dat absent"))
    }

    fn read_entry(pack: &Pack, name: &str) -> Result<Option<Vec<u8>>> {
        let Some(entry) = pack.index.find(name).cloned() else {
            return Ok(None);
        };
        let mut f = pack.data.lock().unwrap();
        Ok(Some(pack.index.read_entry(&mut *f, &entry)?))
    }

    /// Ouvre un fichier par son chemin client (`character\a0000060.ecl`, `texture/world/x.tx1`…)
    /// en suivant le système de fichiers virtuel d'Archonia.exe :
    ///
    /// 1. si un pack existe pour le dossier, seule son entrée compte (aucun repli sur le disque) ;
    /// 2. sinon, le fichier en clair est lu sur le disque ;
    /// 3. si rien n'est trouvé et que `overlay` est vrai, l'entrée `dossier\fichier` du pack de
    ///    surcharge `etc` est lue et déchiffrée (AES-GCM, seconde clé).
    ///
    /// Un fichier demandé directement dans le dossier `etc` est lui aussi déchiffré, comme le fait
    /// le client. Les patchs d'ArchRust ne sont pas consultés ici. `Ok(None)` = introuvable.
    pub fn open(&self, path: &str, overlay: bool) -> Result<Option<Vec<u8>>> {
        let (dir, file) = split_client_path(path);
        let mut found = match self.pack_opt(&dir)? {
            Some(pack) => Self::read_entry(&pack, &file)?,
            None => {
                let rel = if dir == "root" {
                    file.clone()
                } else {
                    format!("{dir}\\{file}")
                };
                let disk = self.game_dir()?.join(rel.replace('\\', "/"));
                if disk.is_file() {
                    Some(std::fs::read(&disk)?)
                } else {
                    // Windows ignore la casse ; on fait de même sur les autres systèmes.
                    find_case_insensitive(self.game_dir()?, &rel)
                        .map(std::fs::read)
                        .transpose()?
                }
            }
        };
        if dir == OVERLAY_DIR {
            if let Some(raw) = found.take() {
                found = Some(
                    decrypt_overlay_entry(&raw).map_err(|e| anyhow!("{path} (pack etc) : {e}"))?,
                );
            }
        }
        if found.is_none() && overlay && dir != OVERLAY_DIR {
            if let Some(pack) = self.pack_opt(OVERLAY_DIR)? {
                if let Some(raw) = Self::read_entry(&pack, &format!("{dir}\\{file}"))? {
                    log::debug!("{dir}\\{file} servi par le pack de surcharge etc");
                    found = Some(
                        decrypt_overlay_entry(&raw)
                            .map_err(|e| anyhow!("{path} (pack etc) : {e}"))?,
                    );
                }
            }
        }
        Ok(found)
    }

    /// Lit une entrée d'un pack (`rel_dir` = dossier du pack, `name` = nom dans l'index), avec le
    /// repli du client sur le pack de surcharge `etc`.
    pub fn read_packed(&self, rel_dir: &str, name: &str) -> Result<Vec<u8>> {
        self.open(&format!("{rel_dir}\\{name}"), true)?
            .ok_or_else(|| anyhow!("{name} absent du pack {rel_dir}"))
    }

    /// Noms des entrées d'un pack.
    pub fn list_packed(&self, rel_dir: &str) -> Result<Vec<String>> {
        Ok(self
            .pack(rel_dir)?
            .index
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect())
    }

    /// Fichier brut du client, en passant par les patchs (`[[files]]`) puis le système de fichiers
    /// virtuel du client (packs, disque, surcharge `etc`).
    pub fn read_file(&self, rel_path: &str) -> Result<Vec<u8>> {
        if let Some(r) = self.patches.read(ResourceKind::File, rel_path) {
            let (bytes, res) = r?;
            log::debug!("{rel_path} fourni par le patch {}", res.patch_id);
            return Ok(bytes);
        }
        self.open(rel_path, true)?
            .ok_or_else(|| anyhow!("{rel_path} introuvable dans le client"))
    }

    /// Table `ini` déchiffrée (cache). `dir` = `ini`, `ini_fr`… Sans client, une table absente est
    /// vide plutôt qu'une erreur.
    pub fn table(&self, dir: &str, file: &str) -> Result<Arc<IniTable>> {
        let key = format!("{dir}/{file}").to_ascii_lowercase();
        if let Some(t) = self.tables.lock().unwrap().get(&key) {
            return Ok(t.clone());
        }
        let raw = match self.read_file(&format!("{dir}/{file}")) {
            Ok(r) => r,
            Err(e) if !self.has_client() => {
                log::debug!("table {dir}/{file} absente sans client ({e}) : table vide");
                let empty = Arc::new(IniTable::default());
                self.tables.lock().unwrap().insert(key, empty.clone());
                return Ok(empty);
            }
            Err(e) => return Err(e),
        };
        let cp = CodePage::for_dir(dir);
        // Les tables de langue (`ini_fr`…) sont parfois déjà en clair.
        let looks_plain = raw
            .iter()
            .take(256)
            .filter(|&&b| b < 0x20 && b != b'\r' && b != b'\n' && b != b'\t')
            .count()
            * 20
            < raw.len().min(256);
        let text = if looks_plain {
            archrust_data::ini::decode_text(&raw, cp)
        } else {
            archrust_data::ini::decrypt_ini_with(&raw, cp)
        };
        let table = Arc::new(IniTable::parse(&text));
        self.tables.lock().unwrap().insert(key, table.clone());
        Ok(table)
    }

    /// Texture par nom RenderWare (ex. `A00005AA`), décodée en RGBA8. Une texture introuvable
    /// (patchs puis client) est remplacée par un substitut généré, pour que la scène reste lisible.
    pub fn texture(&self, name: &str) -> Result<Arc<RgbaImage>> {
        let key = name.to_ascii_lowercase();
        if let Some(t) = self.textures.lock().unwrap().get(&key) {
            return Ok(t.clone());
        }
        let img = match self.load_texture_uncached(&key) {
            Ok(i) => i,
            Err(e) => {
                log::debug!("texture {name} : {e} → substitut");
                placeholder_texture(&key)
            }
        };
        let img = Arc::new(img);
        self.textures.lock().unwrap().insert(key, img.clone());
        Ok(img)
    }

    fn load_texture_uncached(&self, name: &str) -> Result<RgbaImage> {
        // 1. Patch HD
        if let Some(r) = self.patches.read(ResourceKind::Texture, name) {
            let (bytes, res) = r?;
            let img = image::load_from_memory(&bytes)
                .with_context(|| format!("image du patch {} pour {name}", res.patch_id))?
                .to_rgba8();
            return Ok(RgbaImage {
                width: img.width(),
                height: img.height(),
                data: img.into_raw(),
                uv_scale: res.scale,
                from_patch: Some(res.patch_id),
            });
        }
        // 2. Chemin de recherche du client, dans son ordre, avec la qualité de COption.ini. Le repli
        //    sur le pack `etc` est appliqué à chaque dossier : il contient des textures sous
        //    `texture\character\high\…`, ce qui n'a de sens que si le lecteur d'images l'utilise.
        self.game_dir()?;
        let file = format!("{name}.tx1");
        let mut raw = None;
        for dir in texture_search_path(&self.quality) {
            if let Some(b) = self.open(&format!("{dir}\\{file}"), true)? {
                raw = Some(b);
                break;
            }
        }
        let Some(raw) = raw else {
            // Pas de `.tx1` : certaines textures du monde sont des images ordinaires du pack, comme
            // le masque de fondu du sol `a0001c0t.tif` (TIFF 256×256 à quatre échantillons) dans
            // `texture\world`.
            for ext in ["tif", "png", "bmp", "dds"] {
                let file = format!("{name}.{ext}");
                for dir in texture_search_path(&self.quality) {
                    if let Some(bytes) = self.open(&format!("{dir}\\{file}"), true)? {
                        // TIFF : lecteur propre, qui garde le 4ᵉ échantillon (alpha des masques).
                        let (width, height, data) = if ext == "tif" {
                            let t = archrust_data::tiff::decode(&bytes)
                                .with_context(|| format!("image {dir}\\{file}"))?;
                            (t.width, t.height, t.rgba)
                        } else {
                            let img = image::load_from_memory(&bytes)
                                .with_context(|| format!("image {dir}\\{file}"))?
                                .to_rgba8();
                            (img.width(), img.height(), img.into_raw())
                        };
                        return Ok(RgbaImage {
                            width,
                            height,
                            data,
                            uv_scale: 1.0,
                            from_patch: None,
                        });
                    }
                }
            }
            return Err(anyhow!("texture {name} introuvable"));
        };
        let plain = capi_crypt(KEY_TEXTURE, &raw);
        let dict =
            TextureDictionary::parse(&plain).with_context(|| format!("dictionnaire {name}"))?;
        let tex = dict
            .textures
            .first()
            .ok_or_else(|| anyhow!("dictionnaire {name} vide"))?;
        let data = tex.to_rgba8()?;
        let mip = &tex.mips[0];
        Ok(RgbaImage {
            width: mip.width,
            height: mip.height,
            data,
            uv_scale: 1.0,
            from_patch: None,
        })
    }

    /// Image PNG/BMP/DDS d'un pack ou du disque (interface, icônes, cartes). Un patch `[[files]]`
    /// déclaré pour `rel_dir/name` (ex. `texture/ui/ac_base.png`) l'emporte sur le pack.
    pub fn image(&self, rel_dir: &str, name: &str) -> Result<RgbaImage> {
        let rel_path = format!("{rel_dir}/{name}");
        let (bytes, from_patch) = match self.patches.read(ResourceKind::File, &rel_path) {
            Some(r) => {
                let (bytes, res) = r?;
                log::debug!("{rel_path} fournie par le patch {}", res.patch_id);
                (bytes, Some(res.patch_id))
            }
            None => (
                self.open(&rel_path, true)?
                    .ok_or_else(|| anyhow!("{rel_path} introuvable dans le client"))?,
                None,
            ),
        };
        let img = image::load_from_memory(&bytes)
            .with_context(|| format!("décodage de {rel_path}"))?
            .to_rgba8();
        Ok(RgbaImage {
            width: img.width(),
            height: img.height(),
            data: img.into_raw(),
            uv_scale: 1.0,
            from_patch,
        })
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

    /// Animation `.ean` par chemin client, avec le même système de ressources que modèles et
    /// textures (patchs, packs, disque et repli `etc`).
    pub fn animation(&self, name: &str) -> Result<Arc<EanFile>> {
        let path = if name.contains('/') || name.contains('\\') {
            normalize_client_path(name)
        } else {
            format!("character/animation/{}", name.to_ascii_lowercase())
        };
        let key = path.to_ascii_lowercase();
        if let Some(animation) = self.animations.lock().unwrap().get(&key) {
            return Ok(animation.clone());
        }
        let bytes = self.read_file(&path)?;
        let animation =
            Arc::new(EanFile::parse(&bytes).with_context(|| format!("animation {path}"))?);
        self.animations
            .lock()
            .unwrap()
            .insert(key, animation.clone());
        Ok(animation)
    }

    /// Modèle glTF/GLB remplaçant la clé logique `character/a0000060.ecl`, `world/D272,272.dff`…
    ///
    /// 1. un `[[models]]` d'un patch actif ;
    /// 2. sinon, comme `AgcmCharacter::LoadClump` du client : `<dossier>\<nom>.glb` à côté du
    ///    `.ecl`, dans le système de fichiers virtuel (packs, disque, surcharge `etc`).
    ///
    /// `None` si rien ne remplace le modèle ; `Some(Err)` si le fichier est illisible ou invalide.
    /// Le résultat est mis en cache.
    pub fn patched_model(&self, key: &str) -> Option<Result<Arc<PatchedModel>>> {
        if let Some(res) = self.patches.resolve(ResourceKind::Model, key).cloned() {
            let cache_key = format!("{}:{}", res.patch_id, res.file).to_ascii_lowercase();
            if let Some(m) = self.models.lock().unwrap().get(&cache_key) {
                return Some(Ok(m.clone()));
            }
            return Some(self.load_patched_model(&res).map(|m| {
                let m = Arc::new(m);
                self.models.lock().unwrap().insert(cache_key, m.clone());
                m
            }));
        }
        self.client_gltf(key)
    }

    /// `.glb` natif du client remplaçant un `.ecl`/`.dff` (même dossier, même nom de base).
    fn client_gltf(&self, key: &str) -> Option<Result<Arc<PatchedModel>>> {
        if !self.has_client() {
            return None;
        }
        let path = normalize_client_path(key);
        let (stem, ext) = path.rsplit_once('.')?;
        if ext == "glb" || ext == "gltf" {
            return None;
        }
        let glb = format!("{stem}.glb");
        let cache_key = format!("client:{glb}");
        if let Some(m) = self.models.lock().unwrap().get(&cache_key) {
            return Some(Ok(m.clone()));
        }
        let bytes = match self.open(&glb, true) {
            Ok(Some(b)) => b,
            Ok(None) => return None,
            Err(e) => return Some(Err(e)),
        };
        let dir = glb
            .rsplit_once('\\')
            .map(|(d, _)| d.to_string())
            .unwrap_or_default();
        let neighbour = |rel: &str| -> Result<Vec<u8>> {
            if rel.split(['/', '\\']).any(|c| c == "..") {
                return Err(anyhow!("{rel} sort du dossier du modèle"));
            }
            self.open(&format!("{dir}\\{rel}"), true)?
                .ok_or_else(|| anyhow!("{rel} introuvable à côté de {glb}"))
        };
        Some(
            GltfModel::parse(&bytes, &neighbour)
                .with_context(|| format!("glTF du client {glb}"))
                .map(|model| {
                    log::info!(
                        "modèle glTF du client {glb} : {} maillage(s)",
                        model.meshes.len()
                    );
                    let m = Arc::new(PatchedModel {
                        model,
                        patch_id: "client".into(),
                        file: glb.clone(),
                        scale: 1.0,
                    });
                    self.models.lock().unwrap().insert(cache_key, m.clone());
                    m
                }),
        )
    }

    fn load_patched_model(&self, res: &Resolved) -> Result<PatchedModel> {
        let patch = &self.patches.patches[res.patch_index];
        let bytes = patch.read(&res.file).map_err(|e| anyhow!("{e}"))?;
        // Les tampons et images d'un `.gltf` JSON sont lus à côté de lui, dans le même patch.
        let dir = res
            .file
            .replace('\\', "/")
            .rsplit_once('/')
            .map(|(d, _)| d.to_string());
        let neighbour = |rel: &str| -> Result<Vec<u8>> {
            if rel.split(['/', '\\']).any(|c| c == "..") {
                return Err(anyhow!("{rel} sort du patch"));
            }
            let path = match &dir {
                Some(d) => format!("{d}/{rel}"),
                None => rel.to_string(),
            };
            patch.read(&path).map_err(|e| anyhow!("{e}"))
        };
        let model = GltfModel::parse(&bytes, &neighbour)
            .with_context(|| format!("glTF {} du patch {}", res.file, res.patch_id))?;
        log::info!(
            "modèle {} du patch {} : {} maillage(s), {} image(s), {} nœud(s), échelle {}",
            res.file,
            res.patch_id,
            model.meshes.len(),
            model.images.len(),
            model.instances.len(),
            res.scale
        );
        Ok(PatchedModel {
            model,
            patch_id: res.patch_id.clone(),
            file: res.file.clone(),
            scale: res.scale,
        })
    }

    /// Archive MagPack d'une tuile du monde (`a`/`b` + numéro + `x.ma1`/`.ma2`).
    pub fn magpack(&self, prefix: char, tile: u32, ext: &str) -> Result<MagPack> {
        let name = format!("world/{prefix}{tile:06}x.{ext}");
        let bytes = self.read_file(&name)?;
        Ok(MagPack::parse(&bytes).with_context(|| name)?)
    }
}

/// Cherche `rel` (séparateurs `\`) sous `root` sans tenir compte de la casse, composant par
/// composant, comme le système de fichiers de Windows sur lequel le client est conçu.
fn find_case_insensitive(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = root.to_path_buf();
    for comp in rel.split('\\').filter(|c| !c.is_empty()) {
        let direct = cur.join(comp);
        if direct.exists() {
            cur = direct;
            continue;
        }
        cur = std::fs::read_dir(&cur)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .map(|f| f.to_string_lossy().eq_ignore_ascii_case(comp))
                    .unwrap_or(false)
            })?;
    }
    cur.is_file().then_some(cur)
}

/// Substitut de texture : damier 64 × 64 dont les deux couleurs dérivent du nom, pour distinguer
/// les ressources manquantes les unes des autres sans fichier.
pub fn placeholder_texture(name: &str) -> RgbaImage {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    let base = [
        96 + (h & 0x3f) as u8,
        96 + ((h >> 8) & 0x3f) as u8,
        96 + ((h >> 16) & 0x3f) as u8,
    ];
    let light = base.map(|c| c.saturating_add(70));
    let (w, hgt) = (64u32, 64u32);
    let mut data = Vec::with_capacity((w * hgt * 4) as usize);
    for y in 0..hgt {
        for x in 0..w {
            let c = if ((x / 8) + (y / 8)) % 2 == 0 {
                base
            } else {
                light
            };
            data.extend_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    RgbaImage {
        width: w,
        height: hgt,
        data,
        uv_scale: 1.0,
        from_patch: Some("substitut".into()),
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
    fn no_client_mode_gives_placeholders() {
        let assets = Assets::new(None, &[]);
        assert!(!assets.has_client());
        let t = assets.texture("a0057aab").unwrap();
        assert_eq!((t.width, t.height), (64, 64));
        assert_eq!(t.from_patch.as_deref(), Some("substitut"));
        assert!(assets
            .table("ini", "charactertemplateclient.ini")
            .unwrap()
            .is_empty());
        assert!(assets.clump("character", "a0000060.ecl").is_err());
        assert!(assets.read_packed("character", "x").is_err());
    }

    #[test]
    fn client_paths_and_quality() {
        assert_eq!(
            split_client_path("Character\\A0000060.ecl"),
            ("character".into(), "a0000060.ecl".into())
        );
        assert_eq!(
            split_client_path("texture/world/high/x.tx1"),
            ("texture\\world\\high".into(), "x.tx1".into())
        );
        assert_eq!(
            split_client_path(".\\Base.dat"),
            ("root".into(), "base.dat".into())
        );
        let q = TextureQuality::from_coption(
            "[Patch_Option]\nTexCharacter=1\nTexObject=2\nTexEffect=3\nTexWorld=0\n",
        );
        assert_eq!(
            q,
            TextureQuality {
                character: Quality::High,
                object: Quality::Medium,
                effect: Quality::Low,
                world: Quality::Base
            }
        );
        let path = texture_search_path(&q);
        assert_eq!(path[0], "texture\\effect\\low");
        assert_eq!(path[1], "texture\\ui");
        assert_eq!(path[2], "texture\\character\\high");
        assert_eq!(path[3], "texture\\object\\medium");
        assert_eq!(path[4], "texture\\world");
        assert_eq!(path[9], "texture\\notpacked");
        assert_eq!(path[10], "texture\\amboccl");
        assert_eq!(path.len(), 12);
    }

    #[test]
    fn placeholder_colours_depend_on_name() {
        let a = placeholder_texture("a");
        let b = placeholder_texture("b");
        assert_ne!(a.data, b.data);
    }

    #[test]
    fn tile_numbering() {
        // a001717x contient C272,272 … C287,287 : 272/16 = 17.
        assert_eq!(tile_of_sector(272, 272), 1717);
        assert_eq!(tile_of_sector(287, 272), 1717);
        assert_eq!(tile_of_sector(288, 272), 1817);
    }

    /// Un `[[files]]` de patch l'emporte sur le pack pour une image d'interface, et le TIFF RGBA
    /// non compressé (cadres `et_*`/`tb_*` du client) est décodé.
    #[test]
    fn patched_ui_image_tiff_wins_over_pack() {
        use image::ImageEncoder;
        let dir = std::env::temp_dir().join(format!("archrust-ui-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("files")).unwrap();
        std::fs::write(dir.join("patch.toml"), "[patch]\nid = \"t\"\nname = \"t\"\npriority = 5\n\n[[files]]\npath = \"texture/ui/tb_back.tif\"\nfile = \"files/tb_back.tif\"\n").unwrap();
        let rgba: Vec<u8> = (0..4u8).flat_map(|i| [10 * i, 20 * i, 30 * i, 100 + i]).collect();
        let mut cursor = std::io::Cursor::new(Vec::new());
        image::codecs::tiff::TiffEncoder::new(&mut cursor).write_image(&rgba, 2, 2, image::ExtendedColorType::Rgba8).unwrap();
        std::fs::write(dir.join("files/tb_back.tif"), cursor.into_inner()).unwrap();

        let assets = Assets::new(None, &[dir.clone()]);
        let img = assets.image("texture/ui", "TB_BACK.TIF").unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        assert_eq!(img.from_patch.as_deref(), Some("t"));
        assert_eq!(img.data, rgba);
        assert!(assets.image("texture/ui", "absent.png").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
