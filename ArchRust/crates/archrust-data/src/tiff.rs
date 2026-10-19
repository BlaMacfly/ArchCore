//! Lecteur TIFF minimal : images non compressées, 8 bits par échantillon, en bandes.
//!
//! Le client range ainsi le masque de fondu du sol (`texture\world\a0001c0t.tif`, 256×256, quatre
//! échantillons RGB + un échantillon supplémentaire déclaré « non précisé », `ExtraSamples = 0`).
//! Ce quatrième échantillon porte les formes des masques ; les décodeurs génériques l'ignorent et
//! rendent un alpha à 255. Ici, un quatrième échantillon est toujours lu comme alpha.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TiffError {
    #[error("TIFF tronqué")]
    Truncated,
    #[error("en-tête TIFF invalide")]
    BadHeader,
    #[error("TIFF non pris en charge : {0}")]
    Unsupported(String),
}

/// Image décodée en RGBA8.
#[derive(Debug, Clone)]
pub struct TiffImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

struct Bytes<'a> {
    data: &'a [u8],
    big: bool,
}

impl Bytes<'_> {
    fn u16(&self, at: usize) -> Result<u16, TiffError> {
        let b = self.data.get(at..at + 2).ok_or(TiffError::Truncated)?;
        Ok(if self.big {
            u16::from_be_bytes([b[0], b[1]])
        } else {
            u16::from_le_bytes([b[0], b[1]])
        })
    }
    fn u32(&self, at: usize) -> Result<u32, TiffError> {
        let b = self.data.get(at..at + 4).ok_or(TiffError::Truncated)?;
        let b = [b[0], b[1], b[2], b[3]];
        Ok(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
}

/// Décode la première image d'un fichier TIFF.
pub fn decode(data: &[u8]) -> Result<TiffImage, TiffError> {
    let big = match data.get(0..2) {
        Some(b"II") => false,
        Some(b"MM") => true,
        _ => return Err(TiffError::BadHeader),
    };
    let r = Bytes { data, big };
    if r.u16(2)? != 42 {
        return Err(TiffError::BadHeader);
    }
    let ifd = r.u32(4)? as usize;
    let count = r.u16(ifd)? as usize;
    let (mut width, mut height, mut samples, mut compression) = (0u32, 0u32, 1u32, 1u32);
    let mut bits = 8u32;
    let mut rows_per_strip = u32::MAX;
    let mut offsets: Vec<u32> = Vec::new();
    let mut counts: Vec<u32> = Vec::new();
    for i in 0..count {
        let e = ifd + 2 + i * 12;
        let tag = r.u16(e)?;
        let kind = r.u16(e + 2)?;
        let n = r.u32(e + 4)? as usize;
        // Valeurs (SHORT = 3, LONG = 4) en place si elles tiennent dans 4 octets.
        let size = match kind {
            3 => 2,
            4 => 4,
            _ => 0,
        };
        let values = |r: &Bytes| -> Result<Vec<u32>, TiffError> {
            if size == 0 {
                return Ok(Vec::new());
            }
            let base = if n * size <= 4 {
                e + 8
            } else {
                r.u32(e + 8)? as usize
            };
            (0..n)
                .map(|k| {
                    if size == 2 {
                        r.u16(base + k * 2).map(u32::from)
                    } else {
                        r.u32(base + k * 4)
                    }
                })
                .collect()
        };
        let first = |r: &Bytes| -> Result<u32, TiffError> {
            values(r)?.first().copied().ok_or(TiffError::Truncated)
        };
        match tag {
            256 => width = first(&r)?,
            257 => height = first(&r)?,
            258 => bits = values(&r)?.into_iter().max().unwrap_or(8),
            259 => compression = first(&r)?,
            273 => offsets = values(&r)?,
            277 => samples = first(&r)?,
            278 => rows_per_strip = first(&r)?,
            279 => counts = values(&r)?,
            _ => {}
        }
    }
    if compression != 1 || bits != 8 || !(1..=4).contains(&samples) {
        return Err(TiffError::Unsupported(format!(
            "compression {compression}, {bits} bits, {samples} échantillons"
        )));
    }
    if width == 0 || height == 0 || offsets.is_empty() {
        return Err(TiffError::BadHeader);
    }
    let row = (width * samples) as usize;
    let mut pixels = Vec::with_capacity(row * height as usize);
    for (k, &off) in offsets.iter().enumerate() {
        let rows = rows_per_strip.min(height - (k as u32 * rows_per_strip).min(height));
        let len = counts
            .get(k)
            .map(|&c| c as usize)
            .unwrap_or(row * rows as usize);
        let strip = data
            .get(off as usize..off as usize + len)
            .ok_or(TiffError::Truncated)?;
        pixels.extend_from_slice(strip);
    }
    if pixels.len() < row * height as usize {
        return Err(TiffError::Truncated);
    }
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for px in pixels[..row * height as usize].chunks_exact(samples as usize) {
        match samples {
            1 => rgba.extend_from_slice(&[px[0], px[0], px[0], 255]),
            2 => rgba.extend_from_slice(&[px[0], px[0], px[0], px[1]]),
            3 => rgba.extend_from_slice(&[px[0], px[1], px[2], 255]),
            _ => rgba.extend_from_slice(&[px[0], px[1], px[2], px[3]]),
        }
    }
    Ok(TiffImage {
        width,
        height,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_unspecified_extra_sample_as_alpha() {
        // 2×1, RGBA, une bande, petit-boutiste.
        let mut f = b"II*\0".to_vec();
        f.extend_from_slice(&8u32.to_le_bytes());
        let pixels: [u8; 8] = [255, 255, 255, 0, 255, 255, 255, 200];
        let entries: [(u16, u16, u32, u32); 7] = [
            (256, 3, 1, 2),
            (257, 3, 1, 1),
            (258, 3, 1, 8),
            (259, 3, 1, 1),
            (273, 4, 1, 0), // corrigé plus bas
            (277, 3, 1, 4),
            (279, 4, 1, 8),
        ];
        let data_off = 8 + 2 + entries.len() as u32 * 12 + 4;
        f.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, kind, n, v) in entries {
            f.extend_from_slice(&tag.to_le_bytes());
            f.extend_from_slice(&kind.to_le_bytes());
            f.extend_from_slice(&n.to_le_bytes());
            let v = if tag == 273 { data_off } else { v };
            if kind == 3 {
                f.extend_from_slice(&(v as u16).to_le_bytes());
                f.extend_from_slice(&[0, 0]);
            } else {
                f.extend_from_slice(&v.to_le_bytes());
            }
        }
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&pixels);
        let img = decode(&f).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgba, pixels.to_vec());
    }
}
