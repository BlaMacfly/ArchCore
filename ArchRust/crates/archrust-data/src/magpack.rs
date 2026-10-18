//! Archives « MagPack Ver 0.1a » des tuiles du monde (`world/a%06dx.ma1`, `a%06dx.ma2`,
//! `b%06dx.ma2`).
//!
//! Relevé dans le chargeur `CMagPack` du client :
//!
//! ```text
//! 50 octets : "MagPack Ver 0.1a" complété de zéros
//! table des matières, répétée jusqu'à un octet de longueur nul :
//!     u8   longueur du nom
//!     nom  (ex. "C272,272.amf", "R272,272.dff", "D272,272.dff")
//!     u32  taille des données XOR 0x6969
//! données : pour chaque entrée, `taille` octets puis un u32 nul (séparateur)
//! ```
//!
//! Le client exige exactement 256 entrées (une tuile = 16 × 16 secteurs) ; `.ma1` contient les
//! secteurs `C` (collision/attributs `.amf`), `.ma2` les géométries `R`/`D` (RenderWare `.dff`).

#[derive(Debug, thiserror::Error)]
pub enum MagPackError {
    #[error("signature MagPack absente")]
    BadMagic,
    #[error("archive tronquée")]
    Truncated,
    #[error("séparateur non nul après l'entrée {0}")]
    BadSeparator(usize),
}

pub const MAGIC: &[u8] = b"MagPack Ver 0.1a";
pub const HEADER_LEN: usize = 0x32;
const SIZE_MASK: u32 = 0x6969;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MagEntry {
    pub name: String,
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct MagPack {
    pub entries: Vec<MagEntry>,
    data: Vec<u8>,
}

impl MagPack {
    pub fn parse(bytes: &[u8]) -> Result<Self, MagPackError> {
        if bytes.len() < HEADER_LEN || &bytes[..MAGIC.len()] != MAGIC {
            return Err(MagPackError::BadMagic);
        }
        let mut pos = HEADER_LEN;
        let mut toc: Vec<(String, usize)> = Vec::new();
        loop {
            let len = *bytes.get(pos).ok_or(MagPackError::Truncated)? as usize;
            pos += 1;
            if len == 0 {
                break;
            }
            let end = pos + len + 4;
            if end > bytes.len() {
                return Err(MagPackError::Truncated);
            }
            let name: String = bytes[pos..pos + len].iter().map(|&b| b as char).collect();
            let size = u32::from_le_bytes(bytes[pos + len..end].try_into().unwrap()) ^ SIZE_MASK;
            toc.push((name, size as usize));
            pos = end;
        }
        let mut entries = Vec::with_capacity(toc.len());
        for (i, (name, size)) in toc.into_iter().enumerate() {
            if pos + size + 4 > bytes.len() {
                return Err(MagPackError::Truncated);
            }
            entries.push(MagEntry {
                name,
                offset: pos,
                size,
            });
            pos += size;
            if bytes[pos..pos + 4] != [0, 0, 0, 0] {
                return Err(MagPackError::BadSeparator(i));
            }
            pos += 4;
        }
        Ok(Self {
            entries,
            data: bytes.to_vec(),
        })
    }

    pub fn get(&self, name: &str) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
            .map(|e| &self.data[e.offset..e.offset + e.size])
    }

    pub fn entry_data(&self, e: &MagEntry) -> &[u8] {
        &self.data[e.offset..e.offset + e.size]
    }

    /// Données décompressées d'une entrée (`u32` taille ^ 0x6969 + flux aPLib).
    pub fn unpack(&self, e: &MagEntry) -> Result<Vec<u8>, crate::aplib::ApLibError> {
        crate::aplib::depack_entry(self.entry_data(e))
    }

    /// Entrée décompressée par nom.
    pub fn unpack_named(&self, name: &str) -> Option<Result<Vec<u8>, crate::aplib::ApLibError>> {
        let e = self
            .entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))?;
        Some(self.unpack(e))
    }

    /// Construit une archive (pour les outils). Les tailles sont masquées comme dans le client.
    pub fn build(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_LEN];
        out[..MAGIC.len()].copy_from_slice(MAGIC);
        for (name, data) in entries {
            out.push(name.len() as u8);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(&((data.len() as u32) ^ SIZE_MASK).to_le_bytes());
        }
        out.push(0);
        for (_, data) in entries {
            out.extend_from_slice(data);
            out.extend_from_slice(&[0, 0, 0, 0]);
        }
        out
    }
}

/// Coordonnées d'un secteur à partir d'un nom `C272,272.amf` / `R272,272.dff`.
pub fn sector_coords(name: &str) -> Option<(char, i32, i32)> {
    let kind = name.chars().next()?;
    let body = name.get(1..)?.split('.').next()?;
    let (x, y) = body.split_once(',')?;
    Some((kind, x.parse().ok()?, y.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_and_parse() {
        let bytes = MagPack::build(&[("C272,272.amf", b"abc"), ("C273,272.amf", b"defgh")]);
        let pack = MagPack::parse(&bytes).unwrap();
        assert_eq!(pack.entries.len(), 2);
        assert_eq!(pack.get("c273,272.amf").unwrap(), b"defgh");
        assert_eq!(sector_coords("R275,273.dff"), Some(('R', 275, 273)));
    }
}
