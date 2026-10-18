//! Packs `Reference.Dat` + `Data.Dat` (dossiers `character`, `object`, `etc`, `images`,
//! `texture/*`, `world/octree|grass|water`, `effect/*`…).
//!
//! Relevé dans le chargeur du client (lecture de `Reference.Dat` puis déchiffrement AES-GCM) :
//!
//! ```text
//! u32  nombre d'entrées
//! u32  longueur du nom racine, puis le nom (terminé par 0, ex. "character", "\images")
//! pour chaque entrée :
//!     u32  longueur du nom, puis le nom (terminé par 0 ; le client le met en minuscules)
//!     u64  offset dans Data.Dat
//!     u64  taille
//! ```
//!
//! `Data.Dat` est la simple concaténation des fichiers (flux RenderWare `.ecl`/`.dff`, `.bm1`,
//! `.png`, `.dat`…) aux offsets indiqués.

use archrust_crypto::{decrypt_pack_index, PackCryptoError};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("chiffrement : {0}")]
    Crypto(#[from] PackCryptoError),
    #[error("index tronqué")]
    Truncated,
    #[error("index incohérent : {0}")]
    Invalid(&'static str),
    #[error("E/S : {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackEntry {
    /// Nom en minuscules, tel que le client le cherche (ex. `a0000001.ecl`, `character\a0001598.ecl`).
    pub name: String,
    pub offset: u64,
    pub size: u64,
}

/// Index d'un pack.
#[derive(Debug, Default, Clone)]
pub struct PackIndex {
    pub root: String,
    pub entries: Vec<PackEntry>,
    by_name: HashMap<String, usize>,
}

fn take<'a>(buf: &mut &'a [u8], n: usize) -> Result<&'a [u8], PackError> {
    if buf.len() < n {
        return Err(PackError::Truncated);
    }
    let (h, t) = buf.split_at(n);
    *buf = t;
    Ok(h)
}
fn u32le(buf: &mut &[u8]) -> Result<u32, PackError> {
    Ok(u32::from_le_bytes(take(buf, 4)?.try_into().unwrap()))
}
fn u64le(buf: &mut &[u8]) -> Result<u64, PackError> {
    Ok(u64::from_le_bytes(take(buf, 8)?.try_into().unwrap()))
}
fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    bytes[..end].iter().map(|&b| b as char).collect()
}

impl PackIndex {
    /// Analyse un index **déjà déchiffré**.
    pub fn parse_plain(mut buf: &[u8]) -> Result<Self, PackError> {
        let count = u32le(&mut buf)? as usize;
        if (count as u64) * 14 + 6 > buf.len() as u64 + 8 {
            return Err(PackError::Invalid("nombre d'entrées trop grand pour la taille"));
        }
        let root_len = u32le(&mut buf)? as usize;
        let root = cstr(take(&mut buf, root_len)?);
        let mut idx = PackIndex { root, entries: Vec::with_capacity(count), by_name: HashMap::with_capacity(count) };
        for _ in 0..count {
            let len = u32le(&mut buf)? as usize;
            let name = cstr(take(&mut buf, len)?).to_ascii_lowercase();
            let offset = u64le(&mut buf)?;
            let size = u64le(&mut buf)?;
            idx.by_name.insert(name.clone(), idx.entries.len());
            idx.entries.push(PackEntry { name, offset, size });
        }
        Ok(idx)
    }

    /// Déchiffre puis analyse un fichier `Reference.Dat`.
    pub fn parse_encrypted(raw: &[u8]) -> Result<Self, PackError> {
        let plain = decrypt_pack_index(raw)?;
        Self::parse_plain(&plain)
    }

    /// Entrée par nom (insensible à la casse, `/` et `\` équivalents).
    pub fn find(&self, name: &str) -> Option<&PackEntry> {
        let key = name.to_ascii_lowercase().replace('/', "\\");
        self.by_name.get(&key).map(|&i| &self.entries[i])
    }

    /// Taille totale des données référencées (fin de la dernière entrée).
    pub fn data_size(&self) -> u64 {
        self.entries.iter().map(|e| e.offset + e.size).max().unwrap_or(0)
    }

    /// Lit une entrée depuis un flux `Data.Dat` ouvert.
    pub fn read_entry<R: Read + Seek>(&self, data: &mut R, entry: &PackEntry) -> Result<Vec<u8>, PackError> {
        data.seek(SeekFrom::Start(entry.offset))?;
        let mut out = vec![0u8; entry.size as usize];
        data.read_exact(&mut out)?;
        Ok(out)
    }

    /// Sérialise l'index en clair (pour les outils d'empaquetage / patchs).
    pub fn to_plain(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        let root = self.root.as_bytes();
        out.extend_from_slice(&((root.len() + 1) as u32).to_le_bytes());
        out.extend_from_slice(root);
        out.push(0);
        for e in &self.entries {
            let n = e.name.as_bytes();
            out.extend_from_slice(&((n.len() + 1) as u32).to_le_bytes());
            out.extend_from_slice(n);
            out.push(0);
            out.extend_from_slice(&e.offset.to_le_bytes());
            out.extend_from_slice(&e.size.to_le_bytes());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_plain() {
        let mut idx = PackIndex { root: "character".into(), ..Default::default() };
        idx.entries.push(PackEntry { name: "a0000001.ecl".into(), offset: 0, size: 9932 });
        idx.entries.push(PackEntry { name: "a0000002.ecl".into(), offset: 9932, size: 6374 });
        let plain = idx.to_plain();
        let back = PackIndex::parse_plain(&plain).unwrap();
        assert_eq!(back.root, "character");
        assert_eq!(back.entries, idx.entries);
        assert_eq!(back.find("A0000002.ECL").unwrap().offset, 9932);
        assert_eq!(back.data_size(), 16306);
    }
}
