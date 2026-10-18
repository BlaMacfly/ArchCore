//! Chiffrement de l'index des packs (`Reference.Dat`).
//!
//! Relevé dans l'exécutable (singleton `ApCrypto`, fonction de déchiffrement bâtie sur
//! `CryptoPP::GCM<Rijndael>` + `AuthenticatedDecryptionFilter`) : AES-128-GCM, clé de 16 octets,
//! IV de 16 octets, étiquette d'authentification de 16 octets **à la fin** du fichier.
//!
//! La clé et l'IV sont stockés obfusqués dans l'exécutable : 33 octets dont chaque octet `i` est
//! XORé avec `(i % 0x38) + 0x32`. Décodés, ils forment la chaîne
//! `AgcmUIOPTIONSOFAAgcmCharactersOf` : les 16 premiers octets sont la clé, les 16 suivants l'IV.
//! Vérifié sur tous les `Reference.Dat` du client (character, images, etc, world/octree…).

use aes_gcm::aead::consts::U16;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::aes::Aes128;
use aes_gcm::{AesGcm, Nonce};

/// AES-128-GCM avec nonce de 16 octets (CryptoPP accepte un IV de longueur quelconque via GHASH).
type Aes128Gcm16 = AesGcm<Aes128, U16>;

/// Blob obfusqué tel qu'il apparaît dans l'exécutable (33 octets, petit-boutiste).
const OBFUSCATED: [u8; 33] = [
    0x73, 0x54, 0x57, 0x58, 0x63, 0x7e, 0x77, 0x69, 0x6e, 0x72, 0x73, 0x73, 0x6d, 0x70, 0x06, 0x00,
    0x03, 0x24, 0x27, 0x28, 0x05, 0x2f, 0x29, 0x3b, 0x2b, 0x28, 0x38, 0x28, 0x3c, 0x3c, 0x1f, 0x37,
    0x52,
];

#[derive(Debug, thiserror::Error)]
pub enum PackCryptoError {
    #[error("index trop court ({0} octets)")]
    TooShort(usize),
    #[error("authentification AES-GCM échouée (index corrompu ou clé différente)")]
    Auth,
}

/// Décode la clé et l'IV AES de l'index des packs.
pub fn pack_index_key_iv() -> ([u8; 16], [u8; 16]) {
    let mut decoded = OBFUSCATED;
    for (i, b) in decoded.iter_mut().enumerate() {
        *b ^= ((i % 0x38) as u8).wrapping_add(0x32);
    }
    let mut key = [0u8; 16];
    let mut iv = [0u8; 16];
    key.copy_from_slice(&decoded[..16]);
    iv.copy_from_slice(&decoded[16..32]);
    (key, iv)
}

/// Déchiffre et authentifie un fichier `Reference.Dat` complet (étiquette de 16 octets en fin).
pub fn decrypt_pack_index(raw: &[u8]) -> Result<Vec<u8>, PackCryptoError> {
    if raw.len() < 16 {
        return Err(PackCryptoError::TooShort(raw.len()));
    }
    let (key, iv) = pack_index_key_iv();
    let cipher = Aes128Gcm16::new_from_slice(&key).expect("clé AES-128 de 16 octets");
    let nonce = Nonce::<U16>::from_slice(&iv);
    cipher
        .decrypt(nonce, Payload { msg: raw, aad: &[] })
        .map_err(|_| PackCryptoError::Auth)
}

/// Chiffre un index en clair (pour les outils d'empaquetage) : données puis étiquette.
pub fn encrypt_pack_index(plain: &[u8]) -> Vec<u8> {
    let (key, iv) = pack_index_key_iv();
    let cipher = Aes128Gcm16::new_from_slice(&key).expect("clé AES-128 de 16 octets");
    let nonce = Nonce::<U16>::from_slice(&iv);
    cipher
        .encrypt(nonce, Payload { msg: plain, aad: &[] })
        .expect("chiffrement AES-GCM")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_iv_decode() {
        let (key, iv) = pack_index_key_iv();
        assert_eq!(&key, b"AgcmUIOPTIONSOFA");
        assert_eq!(&iv, b"AgcmCharactersOf");
    }

    #[test]
    fn roundtrip() {
        let plain = b"\x01\x00\x00\x00\x05\x00\x00\x00root\x00";
        let enc = encrypt_pack_index(plain);
        assert_eq!(enc.len(), plain.len() + 16);
        assert_eq!(decrypt_pack_index(&enc).unwrap(), plain);
        let mut bad = enc.clone();
        bad[0] ^= 1;
        assert!(decrypt_pack_index(&bad).is_err());
    }
}
