//! Patchs graphiques HD pour ArchRust.
//!
//! Un patch (ou pack de ressources) est un dossier `patches/<id>/` ou une archive ZIP
//! `patches/<id>.ahp` (ArchLord HD Patch) contenant un manifeste `patch.toml` et des fichiers de
//! remplacement. Le client empile les packs : ceux de la communauté (`patches/`) d'abord, puis les
//! packs officiels (`free-assets/`), chaque groupe par priorité décroissante. Pour chaque ressource
//! demandée, le premier pack actif qui la fournit l'emporte, sinon les données d'origine du jeu sont
//! utilisées. Le joueur peut désactiver un pack ou changer sa priorité ([`PackSettings`]).
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
//! file = "textures/a0057aab.png"   # PNG, DDS, BMP, JPEG, TGA ou TIFF ; toute taille
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
//! [[sounds]]                       # remplace un son (chemin sous `sound/`, entrée d'un `.pk` comprise)
//! path = "ui/click.wav"
//! file = "sons/click.wav"           # WAV ou MP3
//!
//! [[shaders]]                      # déclaré, pas encore appliqué par le moteur
//! name = "terrain"
//! file = "shaders/terrain.wgsl"
//! ```
//!
//! `[patch]` accepte aussi `category = "graphics" | "sounds" | "mixed"` ; absente, elle est déduite
//! du contenu. `min_archrust` est comparé à la version du client : un pack trop récent est signalé.
//!
//! Les clés de correspondance sont insensibles à la casse ; `/` et `\` sont équivalents.

pub mod manifest;
pub mod settings;
pub mod stack;

pub use manifest::{Category, Manifest, PatchInfo};
pub use settings::{PackChoice, PackSettings};
pub use stack::{PatchSource, PatchStack, Resolved, ResourceKind, CLIENT_VERSION};

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
