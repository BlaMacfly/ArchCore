//! Préférences du joueur sur les packs : activé ou non, priorité choisie. Fichier `packs.toml` du
//! dossier des SavedVariables, à côté de `options.toml` :
//!
//! ```toml
//! [[pack]]
//! id = "interface-hd"
//! enabled = false
//!
//! [[pack]]
//! id = "sons-fantasy"
//! priority = 200        # facultatif : remplace la priorité du manifeste
//! ```
//!
//! Un pack absent du fichier garde son état par défaut (activé, priorité du manifeste). Supprimer le
//! fichier rétablit la configuration par défaut.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "packs.toml";

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PackChoice {
    pub id: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PackSettings {
    #[serde(default, rename = "pack")]
    pub packs: Vec<PackChoice>,
}

impl PackSettings {
    pub fn path(saved_dir: &Path) -> PathBuf {
        saved_dir.join(FILE_NAME)
    }

    /// Préférences enregistrées, ou aucune (fichier absent ; illisible : journalisé).
    pub fn load(saved_dir: &Path) -> PackSettings {
        let p = Self::path(saved_dir);
        match std::fs::read_to_string(&p) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                log::warn!("{} illisible ({e}) : configuration des packs par défaut", p.display());
                PackSettings::default()
            }),
            Err(_) => PackSettings::default(),
        }
    }

    pub fn save(&self, saved_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(saved_dir)?;
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(
            Self::path(saved_dir),
            format!("# Packs de ressources d'ArchRust : activés ou non, priorité (supprimer ce fichier rétablit la configuration par défaut).\n{text}"),
        )
    }

    /// Rétablit la configuration par défaut (supprime le fichier).
    pub fn reset(saved_dir: &Path) -> std::io::Result<()> {
        match std::fs::remove_file(Self::path(saved_dir)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    pub fn get(&self, id: &str) -> Option<&PackChoice> {
        self.packs.iter().find(|c| c.id.eq_ignore_ascii_case(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_defaults_and_reset() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(PackSettings::load(tmp.path()), PackSettings::default());
        let s = PackSettings {
            packs: vec![
                PackChoice { id: "a".into(), enabled: false, priority: None },
                PackChoice { id: "b".into(), enabled: true, priority: Some(200) },
            ],
        };
        s.save(tmp.path()).unwrap();
        assert_eq!(PackSettings::load(tmp.path()), s);
        assert_eq!(PackSettings::load(tmp.path()).get("B").and_then(|c| c.priority), Some(200));
        std::fs::write(PackSettings::path(tmp.path()), "[[pack]]\nid = \"c\"\n").unwrap();
        assert!(PackSettings::load(tmp.path()).get("c").unwrap().enabled, "activé par défaut");
        std::fs::write(PackSettings::path(tmp.path()), "pas du toml =").unwrap();
        assert_eq!(PackSettings::load(tmp.path()), PackSettings::default());
        PackSettings::reset(tmp.path()).unwrap();
        PackSettings::reset(tmp.path()).unwrap();
        assert!(!PackSettings::path(tmp.path()).exists());
    }
}
