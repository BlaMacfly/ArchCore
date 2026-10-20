//! Hauteurs du terrain, lues dans la géométrie du sol du client (`Game/world/b%06dx.ma2`).
//!
//! ```text
//!   Archive « MagPack Ver 0.1a » (une tuile = 16 × 16 secteurs) :
//!     50 octets d'en-tête, puis la table des matières répétée jusqu'à un octet nul :
//!       u8 longueur du nom, nom (« D272,272.dff »), u32 taille ^ 0x6969
//!     puis les données de chaque entrée, suivies d'un u32 nul.
//!   Entrée : u32 taille décompressée ^ 0x6969, puis un flux de la famille aPLib.
//!   Secteur « D<x>,<z>.dff » : 4 octets, puis un Atomic RenderWare (0x14) dont la géométrie (0x0F)
//!     porte les sommets du sol ; chaque sommet est un coin de cellule (16 × 16 cellules de 400 unités).
//! ```
//! Les hauteurs sont rangées par secteur dans `map_height` : 17 × 17 coins, i16 LE, ligne Z puis colonne X.

use std::path::Path;

/// Côté d'un secteur, cellules par côté, coins par côté.
pub const SECTOR_SIZE: f32 = 6400.0;
pub const CELLS: usize = 16;
pub const CORNERS: usize = CELLS + 1;
const CELL_SIZE: f32 = SECTOR_SIZE / CELLS as f32;
const SECTOR_OFFSET: i32 = 400;
const MAGIC: &[u8] = b"MagPack Ver 0.1a";
const HEADER_LEN: usize = 0x32;
const SIZE_MASK: u32 = 0x6969;

/// Entrées d'une archive MagPack : (nom, données brutes).
pub fn magpack_entries(bytes: &[u8]) -> anyhow::Result<Vec<(String, &[u8])>> {
    anyhow::ensure!(bytes.len() >= HEADER_LEN && &bytes[..MAGIC.len()] == MAGIC, "signature MagPack absente");
    let mut pos = HEADER_LEN;
    let mut toc = Vec::new();
    loop {
        let len = *bytes.get(pos).ok_or_else(|| anyhow::anyhow!("archive tronquée"))? as usize;
        pos += 1;
        if len == 0 {
            break;
        }
        anyhow::ensure!(pos + len + 4 <= bytes.len(), "archive tronquée");
        let name: String = bytes[pos..pos + len].iter().map(|&b| b as char).collect();
        let size = u32::from_le_bytes(bytes[pos + len..pos + len + 4].try_into().unwrap()) ^ SIZE_MASK;
        toc.push((name, size as usize));
        pos += len + 4;
    }
    let mut out = Vec::with_capacity(toc.len());
    for (name, size) in toc {
        anyhow::ensure!(pos + size + 4 <= bytes.len(), "archive tronquée");
        out.push((name, &bytes[pos..pos + size]));
        pos += size + 4;
    }
    Ok(out)
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    tag: u8,
    left: u8,
}

impl Bits<'_> {
    fn byte(&mut self) -> anyhow::Result<u8> {
        let b = *self.data.get(self.pos).ok_or_else(|| anyhow::anyhow!("flux compressé tronqué"))?;
        self.pos += 1;
        Ok(b)
    }
    fn bit(&mut self) -> anyhow::Result<usize> {
        if self.left == 0 {
            self.tag = self.byte()?;
            self.left = 8;
        }
        let b = (self.tag >> 7) & 1;
        self.tag <<= 1;
        self.left -= 1;
        Ok(b as usize)
    }
    fn gamma(&mut self) -> anyhow::Result<usize> {
        let mut g = 1usize;
        loop {
            g = (g << 1) + self.bit()?;
            if self.bit()? == 0 {
                return Ok(g);
            }
            anyhow::ensure!(g <= 1 << 24, "flux compressé invalide");
        }
    }
}

/// Décompresse une entrée MagPack (u32 taille ^ 0x6969, puis le flux).
pub fn depack_entry(entry: &[u8]) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(entry.len() >= 5, "entrée tronquée");
    let expected = (u32::from_le_bytes(entry[..4].try_into().unwrap()) ^ SIZE_MASK) as usize;
    let mut r = Bits { data: &entry[4..], pos: 0, tag: 0, left: 0 };
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    out.push(r.byte()?);
    let mut last = 0usize;
    fn copy(out: &mut Vec<u8>, offset: usize, len: usize) -> anyhow::Result<()> {
        anyhow::ensure!(offset != 0 && offset <= out.len(), "décalage invalide");
        for _ in 0..len {
            out.push(out[out.len() - offset]);
        }
        Ok(())
    }
    loop {
        anyhow::ensure!(out.len() <= expected + 64, "taille décompressée inattendue");
        if r.bit()? == 0 {
            let b = r.byte()?;
            out.push(b);
            continue;
        }
        if r.bit()? == 0 {
            let g = r.gamma()?;
            let (offset, len) = if g == 2 {
                (last, r.gamma()?)
            } else {
                let offset = ((g - 3) << 8) | r.byte()? as usize;
                let len = r.gamma()?
                    + if offset >= 32000 {
                        2
                    } else if offset >= 1280 {
                        1
                    } else if offset < 128 {
                        2
                    } else {
                        0
                    };
                (offset, len)
            };
            copy(&mut out, offset, len)?;
            last = offset;
            continue;
        }
        if r.bit()? == 0 {
            let b = r.byte()?;
            let offset = (b >> 1) as usize;
            if offset == 0 {
                break;
            }
            copy(&mut out, offset, 2 + (b & 1) as usize)?;
            last = offset;
            continue;
        }
        let mut offset = 0usize;
        for _ in 0..4 {
            offset = (offset << 1) | r.bit()?;
        }
        if offset == 0 {
            out.push(0);
        } else {
            copy(&mut out, offset, 1)?;
        }
    }
    anyhow::ensure!(out.len() == expected, "taille décompressée {} au lieu de {expected}", out.len());
    Ok(out)
}

struct Rw<'a> {
    data: &'a [u8],
    pos: usize,
}

impl Rw<'_> {
    fn u32(&mut self) -> anyhow::Result<u32> {
        let b = self.data.get(self.pos..self.pos + 4).ok_or_else(|| anyhow::anyhow!("géométrie tronquée"))?;
        self.pos += 4;
        Ok(u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn f32(&mut self) -> anyhow::Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn skip(&mut self, n: usize) -> anyhow::Result<()> {
        anyhow::ensure!(self.pos + n <= self.data.len(), "géométrie tronquée");
        self.pos += n;
        Ok(())
    }
    /// En-tête de bloc : (type, taille, version RenderWare décodée).
    fn chunk(&mut self, id: u32) -> anyhow::Result<(usize, u32)> {
        let (got, size, stamp) = (self.u32()?, self.u32()? as usize, self.u32()?);
        anyhow::ensure!(got == id, "bloc {got:#x} au lieu de {id:#x}");
        let version = if stamp & 0xffff_0000 != 0 { ((stamp >> 14) & 0x3ff00) | ((stamp >> 16) & 0x3f) | 0x30000 } else { stamp << 8 };
        Ok((size, version))
    }
}

/// Sommets du sol d'un secteur (`D<x>,<z>.dff` décompressé).
pub fn sector_vertices(dff: &[u8]) -> anyhow::Result<Vec<[f32; 3]>> {
    const ATOMIC: u32 = 0x14;
    const STRUCT: u32 = 0x01;
    const GEOMETRY: u32 = 0x0f;
    const PRELIT: u32 = 0x08;
    const TEXTURED: u32 = 0x04;
    const TEXTURED2: u32 = 0x80;
    const NATIVE: u32 = 0x0100_0000;
    anyhow::ensure!(dff.len() >= 16, "secteur tronqué");
    let mut r = Rw { data: &dff[4..], pos: 0 };
    r.chunk(ATOMIC)?;
    let (size, _) = r.chunk(STRUCT)?;
    r.skip(size)?;
    let (_, version) = r.chunk(GEOMETRY)?;
    r.chunk(STRUCT)?;
    let flags = r.u32()?;
    let (tris, verts, morphs) = (r.u32()? as usize, r.u32()? as usize, r.u32()? as usize);
    if version < 0x34000 {
        r.skip(12)?;
    }
    if flags & NATIVE == 0 {
        if flags & PRELIT != 0 {
            r.skip(4 * verts)?;
        }
        let mut uv = ((flags >> 16) & 0xff) as usize;
        if uv == 0 && flags & (TEXTURED | TEXTURED2) != 0 {
            uv = if flags & TEXTURED2 != 0 { 2 } else { 1 };
        }
        r.skip(uv * verts * 8 + tris * 8)?;
    }
    anyhow::ensure!(morphs > 0, "géométrie sans sommets");
    r.skip(16)?;
    let has_verts = r.u32()? != 0;
    r.u32()?;
    anyhow::ensure!(has_verts, "géométrie sans sommets");
    (0..verts).map(|_| Ok([r.f32()?, r.f32()?, r.f32()?])).collect()
}

/// Hauteurs aux 17 × 17 coins de cellule du secteur (`sx`, `sz`) : le sommet le plus haut tombant sur chaque
/// coin ; un coin jamais atteint prend la hauteur d'un voisin.
pub fn heightfield(sx: i32, sz: i32, vertices: &[[f32; 3]]) -> Vec<i16> {
    let (ox, oz) = ((sx - SECTOR_OFFSET) as f32 * SECTOR_SIZE, (sz - SECTOR_OFFSET) as f32 * SECTOR_SIZE);
    let mut grid = vec![0f32; CORNERS * CORNERS];
    let mut filled = vec![false; CORNERS * CORNERS];
    for v in vertices {
        let i = ((v[0] - ox) / CELL_SIZE).round().clamp(0.0, CELLS as f32) as usize;
        let j = ((v[2] - oz) / CELL_SIZE).round().clamp(0.0, CELLS as f32) as usize;
        let k = j * CORNERS + i;
        if !filled[k] || v[1] > grid[k] {
            grid[k] = v[1];
            filled[k] = true;
        }
    }
    for k in 0..CORNERS * CORNERS {
        if filled[k] {
            continue;
        }
        let (i, j) = (k % CORNERS, k / CORNERS);
        let near = [(i.wrapping_sub(1), j), (i + 1, j), (i, j.wrapping_sub(1)), (i, j + 1)];
        if let Some(m) = near.iter().filter(|(a, b)| *a < CORNERS && *b < CORNERS).map(|(a, b)| b * CORNERS + a).find(|&m| filled[m]) {
            grid[k] = grid[m];
        }
    }
    grid.iter().map(|h| h.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16).collect()
}

/// Lit toutes les tuiles du sol (`world/b*x.ma2` sous `game_dir`) : (secteur X, secteur Z, 17 × 17 hauteurs i16
/// LE). Rend aussi le nombre de secteurs illisibles.
pub fn read_world(game_dir: &Path) -> anyhow::Result<(Vec<(u16, u16, Vec<u8>)>, usize)> {
    let dir = game_dir.join("world");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| anyhow::anyhow!("{} : {e}", dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.to_ascii_lowercase().starts_with('b') && n.to_ascii_lowercase().ends_with("x.ma2")))
        .collect();
    files.sort();
    let (mut out, mut bad) = (Vec::new(), 0);
    for f in files {
        let bytes = std::fs::read(&f)?;
        let Ok(entries) = magpack_entries(&bytes) else {
            bad += 1;
            continue;
        };
        for (name, data) in entries {
            let Some((sx, sz)) = name.strip_prefix(['D', 'd']).and_then(|n| n.split('.').next()).and_then(|n| n.split_once(',')).and_then(|(a, b)| Some((a.parse::<i32>().ok()?, b.parse::<i32>().ok()?))) else {
                continue;
            };
            if !(0..800).contains(&sx) || !(0..800).contains(&sz) {
                continue;
            }
            match depack_entry(data).and_then(|d| sector_vertices(&d)) {
                Ok(v) if !v.is_empty() => {
                    let bytes: Vec<u8> = heightfield(sx, sz, &v).iter().flat_map(|h| h.to_le_bytes()).collect();
                    out.push((sx as u16, sz as u16, bytes));
                }
                _ => bad += 1,
            }
        }
    }
    Ok((out, bad))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grille_de_hauteurs() {
        // Secteur (272, 272) : coins (0, 0) à 1000, (1, 0) à 2000 ; les autres reprennent un voisin.
        let o = (272 - 400) as f32 * SECTOR_SIZE;
        let h = heightfield(272, 272, &[[o, 1000.0, o], [o + 400.0, 2000.0, o], [o + 400.0, 1500.0, o]]);
        assert_eq!(h.len(), CORNERS * CORNERS);
        assert_eq!((h[0], h[1], h[CORNERS]), (1000, 2000, 1000), "le plus haut sommet d'un coin, voisin repris");
    }

    #[test]
    fn archive_et_flux() {
        // Archive à une entrée : flux aPLib « a », drapeaux 0 0 0 (trois littéraux) puis 110 et l'octet 0 (fin).
        let stream = [b'a', 0b0001_1000, b'b', b'c', b'd', 0x00];
        let mut entry = (4u32 ^ SIZE_MASK).to_le_bytes().to_vec();
        entry.extend_from_slice(&stream);
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[..MAGIC.len()].copy_from_slice(MAGIC);
        bytes.push(4);
        bytes.extend_from_slice(b"D1,2");
        bytes.extend_from_slice(&((entry.len() as u32) ^ SIZE_MASK).to_le_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&entry);
        bytes.extend_from_slice(&[0, 0, 0, 0]);
        let e = magpack_entries(&bytes).unwrap();
        assert_eq!(e[0].0, "D1,2");
        assert_eq!(depack_entry(e[0].1).unwrap(), b"abcd");
    }
}
