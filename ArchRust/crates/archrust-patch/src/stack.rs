//! Pile de patchs : découverte, ordre par priorité, résolution des ressources, lecture depuis un
//! dossier ou une archive `.ahp`, construction et validation d'archives.

use crate::manifest::{glob_match, Manifest};
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

#[derive(Debug, Clone)]
pub struct LoadedPatch {
    pub manifest: Manifest,
    pub source: PatchSource,
    pub enabled: bool,
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

#[derive(Debug, Default)]
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
        Ok(LoadedPatch {
            manifest,
            source,
            enabled: true,
        })
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
                    if let Err(err) = lp.validate() {
                        log::warn!("patch {} : {err}", lp.manifest.patch.id);
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
        self.patches.sort_by(|a, b| {
            b.manifest
                .patch
                .priority
                .cmp(&a.manifest.patch.priority)
                .then(a.manifest.patch.id.cmp(&b.manifest.patch.id))
        });
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
