//! Options du client (volumes…), persistées en TOML dans le dossier des SavedVariables
//! (`saved-variables/options.toml`), à côté des SavedVariables des AddOns et du compte chiffré.

use crate::audio::AudioSettings;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "options.toml";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClientOptions {
    pub audio: AudioSettings,
}

pub fn path(saved_dir: &Path) -> PathBuf {
    saved_dir.join(FILE_NAME)
}

/// Options enregistrées, ou les valeurs par défaut (fichier absent ou illisible, journalisé).
pub fn load(saved_dir: &Path) -> ClientOptions {
    let p = path(saved_dir);
    match std::fs::read_to_string(&p) {
        Ok(text) => match toml::from_str(&text) {
            Ok(o) => o,
            Err(e) => {
                log::warn!("{} illisible ({e}) : options par défaut", p.display());
                ClientOptions::default()
            }
        },
        Err(_) => ClientOptions::default(),
    }
}

pub fn save(saved_dir: &Path, options: &ClientOptions) -> Result<()> {
    std::fs::create_dir_all(saved_dir)?;
    let text = toml::to_string_pretty(options)?;
    let p = path(saved_dir);
    std::fs::write(&p, format!("# Options du client ArchRust (volumes de 0 à 1).\n{text}"))
        .with_context(|| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_defaults() {
        let tmp = std::env::temp_dir().join(format!("archrust-options-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        assert_eq!(load(&tmp), ClientOptions::default());
        let mut o = ClientOptions::default();
        o.audio.music = 0.25;
        o.audio.muted = true;
        save(&tmp, &o).unwrap();
        let text = std::fs::read_to_string(path(&tmp)).unwrap();
        assert!(text.contains("[audio]") && text.contains("music = 0.25"), "{text}");
        assert_eq!(load(&tmp), o);
        // Clé inconnue ou manquante : valeurs par défaut pour le reste.
        std::fs::write(path(&tmp), "[audio]\neffects = 0.1\nfutur = 3\n").unwrap();
        let o = load(&tmp);
        assert_eq!(o.audio.effects, 0.1);
        assert_eq!(o.audio.music, AudioSettings::default().music);
        std::fs::write(path(&tmp), "pas du toml =").unwrap();
        assert_eq!(load(&tmp), ClientOptions::default());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
