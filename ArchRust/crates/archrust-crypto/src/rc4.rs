//! RC4 tel que dérivé par CryptoAPI dans le client.
//!
//! Le client appelle `CryptCreateHash(MD5)` → `CryptHashData(clé)` → `CryptDeriveKey(CALG_RC4)`.
//! Sur le fournisseur de base Microsoft, la clé RC4 effective est constituée des 16 octets du MD5
//! (5 octets de clé + 11 octets de sel issus du même hachage). Vérifié sur les tables `ini`
//! (clé « 1111 ») et les textures `.tx1` (clé « asdfqwer »).

use md5::{Digest, Md5};

/// Flux RC4 (ARC4) classique.
pub struct Rc4 {
    s: [u8; 256],
    i: u8,
    j: u8,
}

impl Rc4 {
    /// Planifie la clé (KSA). `key` doit faire de 1 à 256 octets.
    pub fn new(key: &[u8]) -> Self {
        assert!(!key.is_empty() && key.len() <= 256, "cle RC4 de 1 a 256 octets");
        let mut s = [0u8; 256];
        for (n, v) in s.iter_mut().enumerate() {
            *v = n as u8;
        }
        let mut j: u8 = 0;
        for i in 0..256 {
            j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
            s.swap(i, j as usize);
        }
        Self { s, i: 0, j: 0 }
    }

    /// Chiffre ou déchiffre `data` en place (RC4 est symétrique).
    pub fn apply(&mut self, data: &mut [u8]) {
        for b in data.iter_mut() {
            self.i = self.i.wrapping_add(1);
            self.j = self.j.wrapping_add(self.s[self.i as usize]);
            self.s.swap(self.i as usize, self.j as usize);
            let k = self.s[(self.s[self.i as usize].wrapping_add(self.s[self.j as usize])) as usize];
            *b ^= k;
        }
    }
}

/// Clé RC4 effective de CryptoAPI pour une clé texte : les 16 octets de son MD5.
pub fn capi_rc4_key(key_text: &[u8]) -> [u8; 16] {
    let digest = Md5::digest(key_text);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}

/// Chiffre/déchiffre `data` comme `CryptEncrypt`/`CryptDecrypt` du client avec la clé texte donnée.
pub fn capi_crypt(key_text: &str, data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    Rc4::new(&capi_rc4_key(key_text.as_bytes())).apply(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rc4_rfc6229_vector() {
        // Vecteur de test public (RFC 6229) : clé 0102030405, premiers octets du flux.
        let mut rc4 = Rc4::new(&[1, 2, 3, 4, 5]);
        let mut buf = [0u8; 16];
        rc4.apply(&mut buf);
        assert_eq!(&buf[..8], &[0xb2, 0x39, 0x63, 0x05, 0xf0, 0x3d, 0xc0, 0x27]);
    }

    #[test]
    fn capi_key_is_md5() {
        // MD5("1111") = b59c67bf196a4758191e42f76670ceba
        let k = capi_rc4_key(b"1111");
        assert_eq!(k[0], 0xb5);
        assert_eq!(k[15], 0xba);
    }

    #[test]
    fn symmetric() {
        let plain = b"Archonia.ini";
        let enc = capi_crypt("1111", plain);
        assert_ne!(&enc[..], &plain[..]);
        assert_eq!(capi_crypt("1111", &enc), plain);
    }
}
