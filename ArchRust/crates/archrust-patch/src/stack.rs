//! Pile de patchs : découverte, ordre par priorité, résolution des ressources, lecture depuis un
//! dossier ou une archive `.ahp`, construction et validation d'archives.

use crate::manifest::{glob_match, Category, Manifest};
use crate::settings::{PackChoice, PackSettings};
use crate::PatchError;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Texture,
    Model,
    File,
    Shader,
}

/// Origine physique d'un patch.
#[derive(Debug, Clone)]
pub enum PatchSource {
    Dir(PathBuf),
    Archive(PathBuf),
}

/// Version du client, comparée au `min_archrust` des packs.
pub const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone)]
pub struct LoadedPatch {
    pub manifest: Manifest,
    pub source: PatchSource,
    pub enabled: bool,
    /// Problèmes relevés au chargement (fichier manquant, version du client trop ancienne…), pour
    /// les afficher au joueur. Le pack reste utilisable pour ce qui est présent.
    pub problems: Vec<String>,
    /// Pack officiel du projet (`free-assets/`) : placé sous les packs de la communauté.
    pub official: bool,
    /// Priorité appliquée : celle du manifeste, ou celle choisie par le joueur.
    pub priority: i32,
}

impl LoadedPatch {
    /// Lit un fichier du patch.
    pub fn read(&self, file: &str) -> Result<Vec<u8>, PatchError> {
        let rel = file.replace('\\', "/");
        match &self.source {
            PatchSource::Dir(dir) => {
                let p = dir.join(&rel);
                std::fs::read(&p).map_err(|e| PatchError::Io(p, e))
            }
            PatchSource::Archive(path) => {
                let f = File::open(path).map_err(|e| PatchError::Io(path.clone(), e))?;
                let mut zip =
                    zip::ZipArchive::new(f).map_err(|e| PatchError::Zip(path.clone(), e))?;
                let mut entry = zip
                    .by_name(&rel)
                    .map_err(|e| PatchError::Zip(path.clone(), e))?;
                let mut out = Vec::with_capacity(entry.size() as usize);
                entry
                    .read_to_end(&mut out)
                    .map_err(|e| PatchError::Io(path.clone(), e))?;
                Ok(out)
            }
        }
    }

    /// Relève les problèmes du pack (fichiers manquants, version du client) dans `problems`.
    pub fn check(&mut self) {
        self.problems.clear();
        if !self.manifest.supports(CLIENT_VERSION) {
            self.problems.push(format!("demande ArchRust {} ou plus récent (client {CLIENT_VERSION})", self.manifest.patch.min_archrust));
        }
        for f in self.missing_files() {
            self.problems.push(format!("fichier {f} introuvable"));
        }
    }

    /// Fichiers référencés par le manifeste mais absents du pack.
    pub fn missing_files(&self) -> Vec<String> {
        let present: Vec<String> = match &self.source {
            PatchSource::Dir(dir) => walkdir::WalkDir::new(dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| e.path().strip_prefix(dir).ok().map(|p| p.to_string_lossy().replace('\\', "/")))
                .collect(),
            PatchSource::Archive(path) => match File::open(path).ok().and_then(|f| zip::ZipArchive::new(f).ok()) {
                Some(zip) => zip.file_names().map(|s| s.to_string()).collect(),
                None => return self.manifest.referenced_files().map(str::to_string).collect(),
            },
        };
        self.manifest
            .referenced_files()
            .filter(|f| {
                let want = f.replace('\\', "/");
                !present.iter().any(|p| p.eq_ignore_ascii_case(&want))
            })
            .map(str::to_string)
            .collect()
    }

    /// Vérifie que tous les fichiers référencés existent.
    pub fn validate(&self) -> Result<(), PatchError> {
        let present: Vec<String> = match &self.source {
            PatchSource::Dir(dir) => walkdir::WalkDir::new(dir)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .filter_map(|e| {
                    e.path()
                        .strip_prefix(dir)
                        .ok()
                        .map(|p| p.to_string_lossy().replace('\\', "/"))
                })
                .collect(),
            PatchSource::Archive(path) => {
                let f = File::open(path).map_err(|e| PatchError::Io(path.clone(), e))?;
                let zip = zip::ZipArchive::new(f).map_err(|e| PatchError::Zip(path.clone(), e))?;
                zip.file_names().map(|s| s.to_string()).collect()
            }
        };
        for f in self.manifest.referenced_files() {
            let want = f.replace('\\', "/");
            if !present.iter().any(|p| p.eq_ignore_ascii_case(&want)) {
                return Err(PatchError::MissingFile {
                    id: self.manifest.patch.id.clone(),
                    file: f.to_string(),
                });
            }
        }
        Ok(())
    }
}

/// Ressource résolue dans un patch.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub patch_id: String,
    pub file: String,
    pub scale: f32,
    pub patch_index: usize,
}

#[derive(Debug, Default, Clone)]
pub struct PatchStack {
    pub patches: Vec<LoadedPatch>,
    /// Correspondances exactes (sans `*`), pour une résolution O(1).
    exact: HashMap<(ResourceKind, String), Resolved>,
    /// Motifs avec `*`, essayés dans l'ordre de priorité.
    patterns: Vec<(ResourceKind, String, Resolved)>,
}

fn norm(key: &str) -> String {
    key.replace('\\', "/").to_ascii_lowercase()
}

impl PatchStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Charge un patch (dossier ou `.ahp`).
    pub fn load_one(path: &Path) -> Result<LoadedPatch, PatchError> {
        let (manifest_text, source) = if path.is_dir() {
            let p = path.join("patch.toml");
            (
                std::fs::read_to_string(&p).map_err(|e| PatchError::Io(p, e))?,
                PatchSource::Dir(path.to_path_buf()),
            )
        } else {
            let f = File::open(path).map_err(|e| PatchError::Io(path.to_path_buf(), e))?;
            let mut zip =
                zip::ZipArchive::new(f).map_err(|e| PatchError::Zip(path.to_path_buf(), e))?;
            let mut entry = zip
                .by_name("patch.toml")
                .map_err(|e| PatchError::Zip(path.to_path_buf(), e))?;
            let mut s = String::new();
            entry
                .read_to_string(&mut s)
                .map_err(|e| PatchError::Io(path.to_path_buf(), e))?;
            (s, PatchSource::Archive(path.to_path_buf()))
        };
        let manifest = Manifest::parse(&manifest_text)
            .map_err(|e| PatchError::Manifest(path.to_path_buf(), e))?;
        let priority = manifest.patch.priority;
        let mut lp = LoadedPatch {
            manifest,
            source,
            enabled: true,
            problems: Vec::new(),
            official: false,
            priority,
        };
        lp.check();
        Ok(lp)
    }

    /// Découvre et charge tous les patchs d'un dossier, triés par priorité décroissante.
    pub fn load_dir(dir: &Path) -> Self {
        let mut stack = PatchStack::new();
        let Ok(rd) = std::fs::read_dir(dir) else {
            return stack;
        };
        let mut loaded = Vec::new();
        for e in rd.flatten() {
            let p = e.path();
            let is_patch = p.is_dir() && p.join("patch.toml").exists()
                || p.extension()
                    .map(|x| x.eq_ignore_ascii_case("ahp"))
                    .unwrap_or(false);
            if !is_patch {
                continue;
            }
            match Self::load_one(&p) {
                Ok(lp) => {
                    for problem in &lp.problems {
                        log::warn!("patch {} : {problem}", lp.manifest.patch.id);
                    }
                    loaded.push(lp);
                }
                Err(err) => log::warn!("patch {} ignoré : {err}", p.display()),
            }
        }
        for lp in loaded {
            stack.push(lp);
        }
        stack
    }

    pub fn push(&mut self, patch: LoadedPatch) {
        self.patches.push(patch);
        self.sort();
        self.rebuild();
    }

    /// Ordre de résolution : packs de la communauté puis officiels, priorité décroissante, puis
    /// identifiant (ordre déterministe à priorité égale).
    fn sort(&mut self) {
        self.patches.sort_by(|a, b| {
            a.official
                .cmp(&b.official)
                .then(b.priority.cmp(&a.priority))
                .then(a.manifest.patch.id.cmp(&b.manifest.patch.id))
        });
    }

    /// Applique les préférences du joueur (activé, priorité) ; un pack absent garde son état par défaut.
    pub fn apply_settings(&mut self, settings: &PackSettings) {
        for p in &mut self.patches {
            let choice = settings.get(&p.manifest.patch.id);
            p.enabled = choice.map(|c| c.enabled).unwrap_or(true);
            p.priority = choice.and_then(|c| c.priority).unwrap_or(p.manifest.patch.priority);
        }
        self.sort();
        self.rebuild();
    }

    /// État courant, à enregistrer dans `packs.toml` (seuls les écarts à l'état par défaut).
    pub fn settings(&self) -> PackSettings {
        PackSettings {
            packs: self
                .patches
                .iter()
                .filter(|p| !p.enabled || p.priority != p.manifest.patch.priority)
                .map(|p| PackChoice {
                    id: p.manifest.patch.id.clone(),
                    enabled: p.enabled,
                    priority: (p.priority != p.manifest.patch.priority).then_some(p.priority),
                })
                .collect(),
        }
    }

    /// Indices des packs d'une catégorie (« mixtes » compris), dans l'ordre de résolution.
    pub fn in_category(&self, category: Category) -> Vec<usize> {
        self.patches
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let c = p.manifest.category();
                c == category || c == Category::Mixed || category == Category::Mixed
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Monte (`up`) ou descend un pack parmi ceux du même groupe (communauté ou officiel) : les
    /// priorités du groupe sont renumérotées (10, 20, 30…) dans le nouvel ordre. Rend faux s'il est
    /// déjà en bout de liste.
    pub fn move_pack(&mut self, id: &str, up: bool) -> bool {
        let Some(at) = self.patches.iter().position(|p| p.manifest.patch.id == id) else { return false };
        let official = self.patches[at].official;
        let mut group: Vec<usize> = (0..self.patches.len()).filter(|&i| self.patches[i].official == official).collect();
        let Some(pos) = group.iter().position(|&i| i == at) else { return false };
        let other = if up { pos.checked_sub(1) } else { (pos + 1 < group.len()).then_some(pos + 1) };
        let Some(other) = other else { return false };
        group.swap(pos, other);
        let n = group.len() as i32;
        for (rank, &i) in group.iter().enumerate() {
            self.patches[i].priority = (n - rank as i32) * 10;
        }
        self.sort();
        self.rebuild();
        true
    }

    /// Marque un pack comme officiel (placé sous ceux de la communauté).
    pub fn set_official(&mut self, id: &str) {
        for p in &mut self.patches {
            if p.manifest.patch.id == id {
                p.official = true;
            }
        }
        self.sort();
        self.rebuild();
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        for p in &mut self.patches {
            if p.manifest.patch.id == id {
                p.enabled = enabled;
            }
        }
        self.rebuild();
    }

    fn rebuild(&mut self) {
        self.exact.clear();
        self.patterns.clear();
        for (i, p) in self.patches.iter().enumerate() {
            if !p.enabled {
                continue;
            }
            let id = p.manifest.patch.id.clone();
            let mut add = |kind: ResourceKind, pattern: &str, file: &str, scale: f32| {
                let r = Resolved {
                    patch_id: id.clone(),
                    file: file.to_string(),
                    scale,
                    patch_index: i,
                };
                if pattern.contains('*') {
                    self.patterns.push((kind, pattern.to_string(), r));
                } else {
                    self.exact.entry((kind, norm(pattern))).or_insert(r);
                }
            };
            for t in &p.manifest.textures {
                add(ResourceKind::Texture, &t.pattern, &t.file, t.scale);
            }
            for m in &p.manifest.models {
                add(ResourceKind::Model, &m.pattern, &m.file, m.scale);
            }
            for f in &p.manifest.files {
                add(ResourceKind::File, &f.path, &f.file, 1.0);
            }
            for s in &p.manifest.shaders {
                add(ResourceKind::Shader, &s.name, &s.file, 1.0);
            }
            // Un son est un fichier du client sous `sound/` : le lecteur audio passe par les fichiers.
            for s in &p.manifest.sounds {
                add(ResourceKind::File, &s.client_path(), &s.file, 1.0);
            }
        }
    }

    /// Patch le plus prioritaire fournissant la ressource.
    pub fn resolve(&self, kind: ResourceKind, key: &str) -> Option<&Resolved> {
        if let Some(r) = self.exact.get(&(kind, norm(key))) {
            return Some(r);
        }
        self.patterns
            .iter()
            .find(|(k, pat, _)| *k == kind && glob_match(pat, key))
            .map(|(_, _, r)| r)
    }

    /// Tous les packs actifs qui fournissent la ressource, du plus prioritaire au moins prioritaire
    /// (pour passer au suivant quand un fichier est absent ou illisible).
    pub fn candidates(&self, kind: ResourceKind, key: &str) -> Vec<Resolved> {
        let wanted = norm(key);
        let mut out = Vec::new();
        for (i, p) in self.patches.iter().enumerate().filter(|(_, p)| p.enabled) {
            let m = &p.manifest;
            let entries: Vec<(&str, &str, f32)> = match kind {
                ResourceKind::Texture => m.textures.iter().map(|t| (t.pattern.as_str(), t.file.as_str(), t.scale)).collect(),
                ResourceKind::Model => m.models.iter().map(|t| (t.pattern.as_str(), t.file.as_str(), t.scale)).collect(),
                ResourceKind::Shader => m.shaders.iter().map(|t| (t.name.as_str(), t.file.as_str(), 1.0)).collect(),
                ResourceKind::File => m.files.iter().map(|f| (f.path.as_str(), f.file.as_str(), 1.0)).collect(),
            };
            let hit = entries
                .iter()
                .find(|(pat, _, _)| if pat.contains('*') { glob_match(pat, key) } else { norm(pat) == wanted })
                .map(|(_, file, scale)| (file.to_string(), *scale))
                // Un son (`[[sounds]]`) est un fichier du client sous `sound/`, essayé après `[[files]]`.
                .or_else(|| (kind == ResourceKind::File).then(|| m.sounds.iter().find(|s| norm(&s.client_path()) == wanted).map(|s| (s.file.clone(), 1.0))).flatten());
            if let Some((file, scale)) = hit {
                out.push(Resolved { patch_id: m.patch.id.clone(), file, scale, patch_index: i });
            }
        }
        out
    }

    /// Lit le fichier d'une ressource résolue.
    pub fn read_resolved(&self, r: &Resolved) -> Result<Vec<u8>, PatchError> {
        self.patches[r.patch_index].read(&r.file)
    }

    /// Lit la ressource remplacée, si un patch la fournit.
    pub fn read(
        &self,
        kind: ResourceKind,
        key: &str,
    ) -> Option<Result<(Vec<u8>, Resolved), PatchError>> {
        let r = self.resolve(kind, key)?.clone();
        Some(self.patches[r.patch_index].read(&r.file).map(|b| (b, r)))
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    /// Construit une archive `.ahp` à partir d'un dossier de patch.
    pub fn build_archive(dir: &Path, out: &Path) -> Result<(), PatchError> {
        let lp = Self::load_one(dir)?;
        lp.validate()?;
        let f = File::create(out).map_err(|e| PatchError::Io(out.to_path_buf(), e))?;
        let mut zip = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for entry in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            let rel = entry
                .path()
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(rel, opts)
                .map_err(|e| PatchError::Zip(out.to_path_buf(), e))?;
            let bytes = std::fs::read(entry.path())
                .map_err(|e| PatchError::Io(entry.path().to_path_buf(), e))?;
            zip.write_all(&bytes)
                .map_err(|e| PatchError::Io(out.to_path_buf(), e))?;
        }
        zip.finish()
            .map_err(|e| PatchError::Zip(out.to_path_buf(), e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_patch(root: &Path, id: &str, priority: i32, tex: &str) -> PathBuf {
        let dir = root.join(id);
        std::fs::create_dir_all(dir.join("tex")).unwrap();
        std::fs::write(
            dir.join("patch.toml"),
            format!("[patch]\nid = \"{id}\"\nname = \"{id}\"\npriority = {priority}\n\n[[textures]]\nmatch = \"{tex}\"\nfile = \"tex/{id}.png\"\n\n[[files]]\npath = \"ini/npc.ini\"\nfile = \"tex/{id}.png\"\n"),
        )
        .unwrap();
        std::fs::write(dir.join(format!("tex/{id}.png")), id.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn official_last_settings_and_moves() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("patches");
        let off = write_patch(&root, "officiel", 1000, "a0057aab");
        write_patch(&root, "commu", 1, "a0057aab");
        write_patch(&root, "autre", 5, "a0057aab");
        let mut stack = PatchStack::new();
        for p in PatchStack::load_dir(&root).patches {
            stack.push(p);
        }
        stack.set_official("officiel");
        let ids = |s: &PatchStack| s.patches.iter().map(|p| p.manifest.patch.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&stack), ["autre", "commu", "officiel"], "communauté d'abord, malgré la priorité 1000");
        assert_eq!(stack.resolve(ResourceKind::Texture, "a0057aab").unwrap().patch_id, "autre");
        // Monter « commu » au-dessus d'« autre » ; le pack officiel reste dernier.
        assert!(stack.move_pack("commu", true));
        assert!(!stack.move_pack("commu", true), "déjà en tête");
        assert_eq!(ids(&stack), ["commu", "autre", "officiel"]);
        assert!(!stack.move_pack("officiel", true), "seul de son groupe");
        // Préférences : désactiver « commu », enregistrer, recharger.
        stack.set_enabled("commu", false);
        let saved = stack.settings();
        assert_eq!(saved.packs.len(), 2, "{saved:?}");
        let mut fresh = PatchStack::new();
        for p in PatchStack::load_dir(&root).patches {
            fresh.push(p);
        }
        fresh.set_official("officiel");
        fresh.apply_settings(&saved);
        assert_eq!(ids(&fresh), ["commu", "autre", "officiel"]);
        assert_eq!(fresh.resolve(ResourceKind::Texture, "a0057aab").unwrap().patch_id, "autre", "commu désactivé");
        fresh.apply_settings(&PackSettings::default());
        assert_eq!(ids(&fresh), ["autre", "commu", "officiel"], "configuration par défaut");
        assert!(fresh.settings().packs.is_empty());
        assert_eq!(fresh.in_category(Category::Graphics).len(), 3);
        assert!(fresh.in_category(Category::Sounds).is_empty());
        let _ = off;
    }

    #[test]
    fn candidates_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("patches");
        write_patch(&root, "bas", 1, "a0057*");
        write_patch(&root, "haut", 100, "a0057aab");
        let mut stack = PatchStack::load_dir(&root);
        let ids = |v: Vec<Resolved>| v.into_iter().map(|r| r.patch_id).collect::<Vec<_>>();
        assert_eq!(ids(stack.candidates(ResourceKind::Texture, "A0057AAB")), ["haut", "bas"]);
        assert_eq!(ids(stack.candidates(ResourceKind::Texture, "a0057zzz")), ["bas"]);
        assert_eq!(ids(stack.candidates(ResourceKind::File, "ini/npc.ini")), ["haut", "bas"]);
        stack.set_enabled("haut", false);
        assert_eq!(ids(stack.candidates(ResourceKind::Texture, "a0057aab")), ["bas"]);
        let r = &stack.candidates(ResourceKind::Texture, "a0057aab")[0];
        assert_eq!(stack.read_resolved(r).unwrap(), b"bas");
    }

    #[test]
    fn sounds_and_problems() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sons");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("patch.toml"),
            "[patch]\nid = \"sons\"\nname = \"Sons\"\nmin_archrust = \"99.0\"\n\n[[sounds]]\npath = \"effect/sd0000.wav\"\nfile = \"sd0000.wav\"\n\n[[sounds]]\npath = \"ui/absent.wav\"\nfile = \"absent.wav\"\n",
        )
        .unwrap();
        std::fs::write(dir.join("sd0000.wav"), b"RIFF").unwrap();
        let mut stack = PatchStack::new();
        stack.push(PatchStack::load_one(&dir).unwrap());
        let p = &stack.patches[0];
        assert_eq!(p.manifest.category(), crate::Category::Sounds);
        assert_eq!(p.problems.len(), 2, "{:?}", p.problems);
        assert!(p.problems[0].contains("99.0") && p.problems[1].contains("absent.wav"));
        let (bytes, r) = stack.read(ResourceKind::File, "sound\\effect\\SD0000.wav").unwrap().unwrap();
        assert_eq!((bytes.as_slice(), r.patch_id.as_str()), (&b"RIFF"[..], "sons"));
    }

    #[test]
    fn priority_and_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("patches");
        write_patch(&root, "low", 1, "a0057*");
        let hi = write_patch(&root, "high", 100, "a0057aab");
        PatchStack::build_archive(&hi, &root.join("high.ahp")).unwrap();
        std::fs::remove_dir_all(&hi).unwrap();

        let stack = PatchStack::load_dir(&root);
        assert_eq!(stack.patches.len(), 2);
        assert_eq!(stack.patches[0].manifest.patch.id, "high");
        let r = stack.resolve(ResourceKind::Texture, "A0057AAB").unwrap();
        assert_eq!(r.patch_id, "high");
        let (bytes, _) = stack
            .read(ResourceKind::Texture, "a0057aab")
            .unwrap()
            .unwrap();
        assert_eq!(bytes, b"high");
        assert_eq!(
            stack
                .resolve(ResourceKind::Texture, "a0057zzz")
                .unwrap()
                .patch_id,
            "low"
        );
        assert!(stack.resolve(ResourceKind::Texture, "b0001").is_none());
        assert!(stack.patches.iter().all(|p| p.problems.is_empty()), "packs complets");
        assert_eq!(
            stack
                .resolve(ResourceKind::File, "ini\\NPC.ini")
                .unwrap()
                .patch_id,
            "high"
        );

        let mut stack = stack;
        stack.set_enabled("high", false);
        assert_eq!(
            stack
                .resolve(ResourceKind::Texture, "a0057aab")
                .unwrap()
                .patch_id,
            "low"
        );
    }
}
