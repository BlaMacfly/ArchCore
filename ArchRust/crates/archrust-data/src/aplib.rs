//! Décompresseur des entrées MagPack (secteurs du monde).
//!
//! Relevé dans le client (`CMagPack::GetFile` → décompression) : un flux de bits de la famille
//! **aPLib** (Jørgen Ibsen), réimplémenté ici d'après le comportement observé :
//!
//! * le premier octet est copié tel quel ;
//! * bit `0` : octet littéral ;
//! * bits `10` : code gamma `g` ; si `g == 2`, répétition du dernier décalage avec une longueur
//!   gamma ; sinon décalage `((g - 3) << 8) | octet`, longueur gamma ajustée (+2 si décalage
//!   ≥ 32000, +1 si ≥ 1280, +2 si < 128) ;
//! * bits `110` : octet `b` ; décalage `b >> 1` (0 = fin du flux), longueur `2 + (b & 1)` ;
//! * bits `111` : décalage sur 4 bits, un seul octet (0 = octet nul).
//!
//! Les bits sont lus de l'octet de drapeaux du plus fort au plus faible. Un code gamma vaut
//! `1`, puis pour chaque paire de bits `(b, suite)` : `g = g * 2 + b`, tant que `suite == 1`.
//!
//! Dans une entrée MagPack, les 4 premiers octets donnent la taille décompressée XOR `0x6969`.

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ApLibError {
    #[error("flux compressé tronqué")]
    Truncated,
    #[error("décalage invalide ({offset}) à la position {pos}")]
    BadOffset { offset: usize, pos: usize },
    #[error("taille décompressée {got} différente de l'annonce {expected}")]
    SizeMismatch { got: usize, expected: usize },
}

struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    tag: u8,
    bits_left: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, tag: 0, bits_left: 0 }
    }
    fn byte(&mut self) -> Result<u8, ApLibError> {
        let b = *self.data.get(self.pos).ok_or(ApLibError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }
    fn bit(&mut self) -> Result<u32, ApLibError> {
        if self.bits_left == 0 {
            self.tag = self.byte()?;
            self.bits_left = 8;
        }
        let bit = (self.tag >> 7) & 1;
        self.tag <<= 1;
        self.bits_left -= 1;
        Ok(bit as u32)
    }
    fn gamma(&mut self) -> Result<usize, ApLibError> {
        let mut g: usize = 1;
        loop {
            g = (g << 1) + self.bit()? as usize;
            if self.bit()? == 0 {
                return Ok(g);
            }
            if g > (1 << 24) {
                return Err(ApLibError::Truncated);
            }
        }
    }
}

/// Décompresse un flux (sans l'en-tête de taille). `expected` borne la sortie.
pub fn depack(src: &[u8], expected: usize) -> Result<Vec<u8>, ApLibError> {
    let mut r = BitReader::new(src);
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    out.push(r.byte()?);
    let mut last_offset: usize = 0;
    let copy = |out: &mut Vec<u8>, offset: usize, len: usize| -> Result<(), ApLibError> {
        if offset == 0 || offset > out.len() {
            return Err(ApLibError::BadOffset { offset, pos: out.len() });
        }
        for _ in 0..len {
            let b = out[out.len() - offset];
            out.push(b);
        }
        Ok(())
    };
    loop {
        if out.len() > expected + 64 {
            return Err(ApLibError::SizeMismatch { got: out.len(), expected });
        }
        if r.bit()? == 0 {
            let b = r.byte()?;
            out.push(b);
            continue;
        }
        if r.bit()? == 0 {
            let g = r.gamma()?;
            let (offset, len) = if g == 2 {
                (last_offset, r.gamma()?)
            } else {
                let offset = ((g - 3) << 8) | r.byte()? as usize;
                let mut len = r.gamma()?;
                if offset >= 32000 {
                    len += 2;
                } else if offset >= 1280 {
                    len += 1;
                } else if offset < 128 {
                    len += 2;
                }
                (offset, len)
            };
            copy(&mut out, offset, len)?;
            last_offset = offset;
            continue;
        }
        if r.bit()? == 0 {
            let b = r.byte()?;
            let offset = (b >> 1) as usize;
            if offset == 0 {
                break;
            }
            let len = 2 + (b & 1) as usize;
            copy(&mut out, offset, len)?;
            last_offset = offset;
            continue;
        }
        let mut offset = 0usize;
        for _ in 0..4 {
            offset = (offset << 1) | r.bit()? as usize;
        }
        if offset == 0 {
            out.push(0);
        } else {
            if offset > out.len() {
                return Err(ApLibError::BadOffset { offset, pos: out.len() });
            }
            let b = out[out.len() - offset];
            out.push(b);
        }
    }
    if out.len() != expected {
        return Err(ApLibError::SizeMismatch { got: out.len(), expected });
    }
    Ok(out)
}

/// Décompresse une entrée MagPack complète (`u32` taille ^ 0x6969, puis le flux).
pub fn depack_entry(entry: &[u8]) -> Result<Vec<u8>, ApLibError> {
    if entry.len() < 5 {
        return Err(ApLibError::Truncated);
    }
    let expected = (u32::from_le_bytes(entry[..4].try_into().unwrap()) ^ 0x6969) as usize;
    depack(&entry[4..], expected)
}

/// Compresseur minimal (littéraux + répétitions courtes) suffisant pour produire des entrées
/// valides ; il n'est pas optimal mais reste lisible par le client.
pub fn pack(data: &[u8]) -> Vec<u8> {
    struct BitWriter {
        out: Vec<u8>,
        tag_pos: Option<usize>,
        bits: u8,
    }
    impl BitWriter {
        fn bit(&mut self, b: u32) {
            if self.bits == 0 {
                self.tag_pos = Some(self.out.len());
                self.out.push(0);
                self.bits = 8;
            }
            self.bits -= 1;
            if b != 0 {
                let p = self.tag_pos.unwrap();
                self.out[p] |= 1 << self.bits;
            }
        }
        fn byte(&mut self, b: u8) {
            self.out.push(b);
        }
        fn gamma(&mut self, mut v: usize) {
            // Émet les bits de v (hors bit de poids fort) avec un bit de continuation après chacun.
            let mut bits = Vec::new();
            while v > 1 {
                bits.push((v & 1) as u32);
                v >>= 1;
            }
            for (i, b) in bits.iter().rev().enumerate() {
                self.bit(*b);
                self.bit(if i + 1 < bits.len() { 1 } else { 0 });
            }
        }
    }
    let mut w = BitWriter { out: Vec::with_capacity(data.len() + 8), tag_pos: None, bits: 0 };
    if data.is_empty() {
        return w.out;
    }
    w.byte(data[0]);
    let mut i = 1;
    while i < data.len() {
        // Recherche d'une répétition de 3 à 255 octets dans les 1279 derniers (pas de bonus).
        let window = i.min(1279);
        let mut best = (0usize, 0usize);
        for offset in 128..=window {
            let mut len = 0;
            while i + len < data.len() && len < 255 && data[i + len] == data[i + len - offset] {
                len += 1;
            }
            if len > best.1 {
                best = (offset, len);
            }
        }
        if best.1 >= 4 {
            let (offset, len) = best;
            w.bit(1);
            w.bit(0);
            w.gamma((offset >> 8) + 3);
            w.byte((offset & 0xff) as u8);
            w.gamma(len);
            i += len;
            continue;
        }
        w.bit(0);
        w.byte(data[i]);
        i += 1;
    }
    // Fin : code 110 avec octet 0.
    w.bit(1);
    w.bit(1);
    w.bit(0);
    w.byte(0);
    w.out
}

/// Fabrique une entrée MagPack (en-tête de taille + flux).
pub fn pack_entry(data: &[u8]) -> Vec<u8> {
    let mut out = ((data.len() as u32) ^ 0x6969).to_le_bytes().to_vec();
    out.extend(pack(data));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_literals_and_matches() {
        let mut data = Vec::new();
        for i in 0..2000u32 {
            data.push((i % 7) as u8);
        }
        data.extend_from_slice(b"RenderWare sector ".repeat(40).as_slice());
        let packed = pack_entry(&data);
        assert!(packed.len() < data.len());
        let back = depack_entry(&packed).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn short_codes() {
        // Construit à la main : littéral 'A', puis code 111 décalage 1 (répète 'A'), puis fin.
        // Octet 0 : 'A'. Drapeaux : 1,1,1, 0,0,0,1 (décalage 1 = 0001), puis 1,1,0 (fin) → octet 0.
        let src = [b'A', 0b1110_0011, 0b1000_0000, 0x00];
        let out = depack(&src, 2).unwrap();
        assert_eq!(out, b"AA");
    }

    #[test]
    fn size_check() {
        let packed = pack_entry(b"hello hello hello hello hello hello");
        let mut bad = packed.clone();
        bad[0] ^= 1;
        assert!(matches!(depack_entry(&bad), Err(ApLibError::SizeMismatch { .. })));
    }
}
