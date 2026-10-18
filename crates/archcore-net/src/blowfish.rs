//! Blowfish tel qu'utilisé par le protocole ArchLord.
//!
//! L'algorithme est le Blowfish standard (P-array / S-boxes de Schneier, planification de clé
//! standard). La seule particularité du jeu est la disposition des mots : le client caste directement
//! les octets du tampon en `u32` natifs x86, donc chaque bloc de 8 octets est lu comme deux mots
//! **little-endian** (L = octets 0..3, R = octets 4..7).

use crate::blowfish_tables::{P_INIT, S_INIT};

#[derive(Clone)]
pub struct Blowfish {
    p: [u32; 18],
    s: [[u32; 256]; 4],
}

impl Blowfish {
    /// Clé de 4 à 56 octets (32 octets dans le jeu).
    pub fn new(key: &[u8]) -> Result<Self, crate::Error> {
        if key.len() < 4 || key.len() > 56 {
            return Err(crate::Error::Invalid(format!("Blowfish : clé de 4 à 56 octets attendue, reçu {}", key.len())));
        }
        let mut bf = Blowfish { p: P_INIT, s: S_INIT };
        let mut j = 0;
        for i in 0..18 {
            let mut data = 0u32;
            for _ in 0..4 {
                data = (data << 8) | key[j] as u32;
                j = (j + 1) % key.len();
            }
            bf.p[i] ^= data;
        }
        let (mut l, mut r) = (0u32, 0u32);
        for i in (0..18).step_by(2) {
            (l, r) = bf.encrypt_words(l, r);
            bf.p[i] = l;
            bf.p[i + 1] = r;
        }
        for b in 0..4 {
            for i in (0..256).step_by(2) {
                (l, r) = bf.encrypt_words(l, r);
                bf.s[b][i] = l;
                bf.s[b][i + 1] = r;
            }
        }
        Ok(bf)
    }

    #[inline]
    fn f(&self, x: u32) -> u32 {
        let a = self.s[0][(x >> 24) as usize].wrapping_add(self.s[1][((x >> 16) & 0xff) as usize]);
        (a ^ self.s[2][((x >> 8) & 0xff) as usize]).wrapping_add(self.s[3][(x & 0xff) as usize])
    }

    /// Chiffre un bloc exprimé en deux mots 32 bits.
    pub fn encrypt_words(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        for i in (0..16).step_by(2) {
            l ^= self.p[i];
            r ^= self.f(l);
            r ^= self.p[i + 1];
            l ^= self.f(r);
        }
        l ^= self.p[16];
        r ^= self.p[17];
        (r, l)
    }

    /// Déchiffre un bloc exprimé en deux mots 32 bits.
    pub fn decrypt_words(&self, mut l: u32, mut r: u32) -> (u32, u32) {
        let mut i = 17;
        while i > 1 {
            l ^= self.p[i];
            r ^= self.f(l);
            r ^= self.p[i - 1];
            l ^= self.f(r);
            i -= 2;
        }
        l ^= self.p[1];
        r ^= self.p[0];
        (r, l)
    }

    fn apply(&self, buf: &mut [u8], decrypt: bool) -> Result<(), crate::Error> {
        if buf.len() % 8 != 0 {
            return Err(crate::Error::Invalid("Blowfish : longueur non multiple de 8".into()));
        }
        for block in buf.chunks_exact_mut(8) {
            let l = u32::from_le_bytes(block[0..4].try_into().unwrap());
            let r = u32::from_le_bytes(block[4..8].try_into().unwrap());
            let (l, r) = if decrypt { self.decrypt_words(l, r) } else { self.encrypt_words(l, r) };
            block[0..4].copy_from_slice(&l.to_le_bytes());
            block[4..8].copy_from_slice(&r.to_le_bytes());
        }
        Ok(())
    }

    /// Chiffre en place un tampon (longueur multiple de 8), mode ECB du jeu.
    pub fn encrypt_le(&self, buf: &mut [u8]) -> Result<(), crate::Error> {
        self.apply(buf, false)
    }

    /// Déchiffre en place un tampon (longueur multiple de 8), mode ECB du jeu.
    pub fn decrypt_le(&self, buf: &mut [u8]) -> Result<(), crate::Error> {
        self.apply(buf, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_vectors::*;

    #[test]
    fn identique_a_l_implementation_node() {
        let bf = Blowfish::new(&BF_KEY).unwrap();
        let mut buf = BF_PLAIN;
        bf.encrypt_le(&mut buf).unwrap();
        assert_eq!(buf, BF_CIPHER);
        bf.decrypt_le(&mut buf).unwrap();
        assert_eq!(buf, BF_PLAIN);
    }
}
