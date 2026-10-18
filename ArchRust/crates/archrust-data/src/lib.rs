//! Formats de données du client ArchLord 64 bits, reconstruits par rétro-ingénierie.
//!
//! | Fichier                          | Format                                               | Module      |
//! |----------------------------------|------------------------------------------------------|-------------|
//! | `ini/*.ini`, `Archonia.ini`      | texte CP949 chiffré RC4 (clé « 1111 »)               | [`ini`]     |
//! | `*/Reference.Dat` + `Data.Dat`   | index AES-GCM (nom → offset, taille) + flux concaténés| [`pack`]    |
//! | `world/*.ma1`, `world/*.ma2`     | archive « MagPack Ver 0.1a », 256 entrées            | [`magpack`] |
//! | `.dff`, `.ecl`, `.txd`, `.tx1`   | flux RenderWare 3.7 (clump, géométrie, textures)     | [`rw`]      |
//!
//! Les lecteurs sont purement en mémoire : l'appelant fournit les octets, les modules rendent des
//! structures Rust. Aucun fichier du jeu n'est embarqué.

pub mod aplib;
pub mod ini;
pub mod magpack;
pub mod pack;
pub mod rw;

pub use ini::{decrypt_ini, IniTable};
pub use magpack::MagPack;
pub use pack::{PackEntry, PackIndex};
