//! RC4 tel que dérivé par CryptoAPI dans le client ArchLord 64 bits :
//! `CryptCreateHash(MD5)` → `CryptHashData(clé texte)` → `CryptDeriveKey(CALG_RC4, CRYPT_CREATE_SALT)`.
//! Sur le « Microsoft Base Cryptographic Provider », la clé RC4 effective est le MD5 complet (16 octets).
//! Utilisé par le client pour chiffrer compte et mot de passe au login (clé = chaîne HASHKEY du
//! serveur) et pour les tables ini du client (clé « 1111 »).

use md5::{Digest, Md5};

pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j = 0u8;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
        })
        .collect()
}

/// Chiffre ou déchiffre (RC4 est symétrique) comme `CryptEncrypt`/`CryptDecrypt` du client.
pub fn capi_crypt(key_string: &[u8], data: &[u8]) -> Vec<u8> {
    let key = Md5::digest(key_string);
    rc4(&key, data)
}

#[cfg(test)]
mod tests {
    #[test]
    fn identique_a_l_implementation_node() {
        assert_eq!(super::capi_crypt(b"hfIGkhCr", b"sima"), crate::test_vectors::RC4_SIMA);
        assert_eq!(super::capi_crypt(b"hfIGkhCr", &crate::test_vectors::RC4_SIMA), b"sima");
    }
}
