//! Primitives cryptographiques relevées dans le client ArchLord 64 bits.
//!
//! | Usage                         | Algorithme                                   | Module       |
//! |-------------------------------|----------------------------------------------|--------------|
//! | Tables `ini`, serveurs        | RC4, clé = MD5("1111") (CryptoAPI)           | [`rc4`]      |
//! | Textures `.tx1`               | RC4, clé = MD5("asdfqwer")                   | [`rc4`]      |
//! | Compte / mot de passe (login) | RC4, clé = MD5(clé de 8 car. du serveur)     | [`rc4`]      |
//! | Trames réseau                 | Blowfish ECB, mots 32 bits petit-boutistes   | [`blowfish`] |
//! | Index `Reference.Dat`         | AES-128-GCM (CryptoPP), étiquette finale     | [`pack`]     |
//!
//! Tout est écrit à partir des observations faites sur l'exécutable (Ghidra via REA) ; aucun code
//! tiers du jeu n'est repris.

pub mod blowfish;
pub mod pack;
pub mod rc4;

pub use blowfish::BlowfishLe;
pub use pack::{
    decrypt_overlay_entry, decrypt_pack_index, encrypt_overlay_entry, overlay_key_iv,
    pack_index_key_iv, PackCryptoError,
};
pub use rc4::{capi_crypt, capi_rc4_key, Rc4};

/// Clé texte des tables `ini` et du fichier de serveurs.
pub const KEY_INI: &str = "1111";
/// Clé texte des textures `.tx1` (dictionnaires RenderWare).
pub const KEY_TEXTURE: &str = "asdfqwer";
