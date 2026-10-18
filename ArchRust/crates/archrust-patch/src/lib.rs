//! Patchs graphiques HD pour ArchRust.
//!
//! Un patch est un dossier `patches/<id>/` ou une archive ZIP `patches/<id>.ahp` (ArchLord HD
//! Patch) contenant un manifeste `patch.toml` et des fichiers de remplacement. Le client empile
//! les patchs par priorité décroissante ; pour chaque ressource demandée, le premier patch qui la
//! fournit l'emporte, sinon les données d'origine du jeu sont utilisées.
//!
//! ```toml
//! [patch]
//! id = "anchorville-hd"            # identifiant stable (nom du dossier/archive)
//! name = "Anchorville HD"
//! version = "1.0.0"
//! author = "Un dev"
//! priority = 100                   # plus grand = appliqué par-dessus
//! description = "Textures 4K du village humain"
//! min_archrust = "0.1"
//!
//! [[textures]]                     # remplace une texture RenderWare par son nom (sans extension)
//! match = "a0057aab"               # nom exact ou motif avec * (ex. "a0057*")
//! file = "textures/a0057aab.png"   # PNG, DDS ou KTX2 ; toute taille (HD, mipmaps libres)
//! scale = 4.0                      # facteur d'échelle UV facultatif (défaut 1)
//!
//! [[models]]                       # remplace un modèle (clump) par un glTF/GLB
//! match = "character/a0000001.ecl"
//! file = "models/a0000001.glb"
//! scale = 100.0                    # facteur d'échelle facultatif (glTF en mètres → unités ArchLord)
//!
//! [[files]]                        # surcharge brute d'un chemin du client (ini, sons, cartes…)
//! path = "sound/effect/aa.pk"
//! file = "sound/aa.wav"
//!
//! [[shaders]]                      # remplace un shader du moteur (WGSL)
//! name = "terrain"
//! file = "shaders/terrain.wgsl"
//! ```
//!
//! Les clés de correspondance sont insensibles à la casse ; `/` et `\` sont équivalents.

pub mod manifest;
pub mod stack;

pub use manifest::{Manifest, PatchInfo};
pub use stack::{PatchSource, PatchStack, Resolved, ResourceKind};

#[derive(Debug, thiserror::Error)]
pub enum PatchError {
    #[error("E/S {0} : {1}")]
    Io(std::path::PathBuf, std::io::Error),
    #[error("archive {0} : {1}")]
    Zip(std::path::PathBuf, zip::result::ZipError),
    #[error("manifeste {0} : {1}")]
    Manifest(std::path::PathBuf, String),
    #[error("patch {id} : fichier {file} introuvable")]
    MissingFile { id: String, file: String },
}
