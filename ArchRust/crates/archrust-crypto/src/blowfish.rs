//! Blowfish du protocole réseau ArchLord.
//!
//! L'algorithme est le Blowfish standard. La particularité du jeu est la disposition des mots :
//! le client caste directement les octets du tampon en `uint32` natifs x86, donc chaque bloc de
//! 8 octets est lu comme deux mots **petit-boutistes** (L = octets 0..3, R = octets 4..7), là où
//! la définition de référence lit des mots gros-boutistes. On s'appuie sur une implémentation
//! standard et on inverse chaque mot avant et après.

use blowfish::Blowfish;
use cipher::{BlockDecrypt, BlockEncrypt, KeyInit};

/// Chiffreur Blowfish en mode ECB avec mots petit-boutistes.
#[derive(Clone)]
pub struct BlowfishLe {
    inner: Blowfish,
}

impl BlowfishLe {
    /// Clé de 4 à 56 octets (32 octets dans le jeu).
    pub fn new(key: &[u8]) -> Result<Self, cipher::InvalidLength> {
        let inner = Blowfish::new_from_slice(key)?;
        Ok(Self { inner })
    }

    fn swap_words(block: &mut [u8; 8]) {
        block[..4].reverse();
        block[4..].reverse();
    }

    /// Chiffre un bloc de 8 octets en place.
    pub fn encrypt_block(&self, block: &mut [u8; 8]) {
        Self::swap_words(block);
        self.inner.encrypt_block(block.into());
        Self::swap_words(block);
    }

    /// Déchiffre un bloc de 8 octets en place.
    pub fn decrypt_block(&self, block: &mut [u8; 8]) {
        Self::swap_words(block);
        self.inner.decrypt_block(block.into());
        Self::swap_words(block);
    }

    /// Chiffre tout un tampon (longueur multiple de 8) en place, mode ECB du jeu.
    pub fn encrypt_buffer(&self, buf: &mut [u8]) {
        assert!(buf.len() % 8 == 0, "Blowfish : longueur non multiple de 8");
        for chunk in buf.chunks_exact_mut(8) {
            let block: &mut [u8; 8] = chunk.try_into().unwrap();
            self.encrypt_block(block);
        }
    }

    /// Déchiffre tout un tampon (longueur multiple de 8) en place.
    pub fn decrypt_buffer(&self, buf: &mut [u8]) {
        assert!(buf.len() % 8 == 0, "Blowfish : longueur non multiple de 8");
        for chunk in buf.chunks_exact_mut(8) {
            let block: &mut [u8; 8] = chunk.try_into().unwrap();
            self.decrypt_block(block);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let bf = BlowfishLe::new(&[7u8; 32]).unwrap();
        let mut data = *b"ArchLordPacket!!";
        let copy = data;
        bf.encrypt_buffer(&mut data);
        assert_ne!(data, copy);
        bf.decrypt_buffer(&mut data);
        assert_eq!(data, copy);
    }

    #[test]
    fn standard_vector_through_le_layout() {
        // Vecteur de test public : clé 00..00, bloc 00..00 -> 4EF997456198DD78 (mots gros-boutistes).
        // En disposition petit-boutiste, chaque mot est inversé : 45 97 F9 4E 78 DD 98 61.
        let bf = BlowfishLe::new(&[0u8; 8]).unwrap();
        let mut block = [0u8; 8];
        bf.encrypt_block(&mut block);
        assert_eq!(block, [0x45, 0x97, 0xf9, 0x4e, 0x78, 0xdd, 0x98, 0x61]);
    }
}
