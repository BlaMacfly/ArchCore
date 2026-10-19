//! Aligne l'encodage d'images retravaillées (`<nom>_upgrade.<ext>`) sur celui des originaux
//! (`<nom>.<ext>`, dans le même dossier), sans toucher aux dimensions.
//!
//! ```text
//! cargo run -p archrust-data --example reformat_like -- <dossier>                 # rapport seul
//! cargo run -p archrust-data --example reformat_like -- <dossier> --apply [--backup <dossier>]
//! ```
//!
//! Règles appliquées, par conteneur :
//!
//! * **PNG** : même type de couleur et même profondeur que l'original (8 bits RGBA, RGB, gris…),
//!   non entrelacé ; les chunks auxiliaires de l'original (`gAMA`, `cHRM`, `sRGB`, `iCCP`, `pHYs`,
//!   `bKGD`, `tEXt`, `iTXt`, `zTXt`, `tIME`, `sBIT`) sont recopiés tels quels après `IHDR`, ceux
//!   de la version retravaillée sont abandonnés ;
//! * **TIFF** : même compression (en pratique aucune), même nombre d'échantillons (RGB ou RGBA 8 bits) ;
//! * **BMP** : même profondeur (24 ou 32 bits), en-tête `BITMAPINFOHEADER`, sans compression.
//!
//! Un fichier déjà conforme n'est pas réécrit. Les `_upgrade` sans original sont listés et ignorés.

use anyhow::{anyhow, bail, Context, Result};
use image::{DynamicImage, ExtendedColorType, ImageEncoder};
use std::collections::BTreeMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const SUFFIX: &str = "_upgrade";
/// Chunks PNG auxiliaires recopiés depuis l'original (ordre de l'original conservé).
const COPIED_CHUNKS: &[&[u8; 4]] = &[b"gAMA", b"cHRM", b"sRGB", b"iCCP", b"sBIT", b"pHYs", b"bKGD", b"tEXt", b"iTXt", b"zTXt", b"tIME"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Format {
    Png { color_type: u8, bit_depth: u8, interlace: u8, ancillary: Vec<String> },
    Tiff { compression: u16, bits: Vec<u16>, samples: u16, photometric: u16 },
    Bmp { bpp: u16, header: u32, compression: u32 },
}

impl Format {
    fn describe(&self) -> String {
        match self {
            Format::Png { color_type, bit_depth, interlace, ancillary } => format!("PNG type {} {} bits{}{}", png_color_name(*color_type), bit_depth, if *interlace != 0 { " entrelacé" } else { "" }, if ancillary.is_empty() { String::new() } else { format!(" [{}]", ancillary.join(" ")) }),
            Format::Tiff { compression, bits, samples, photometric } => format!("TIFF compression {} ({} échantillons × {:?} bits, photométrie {photometric})", tiff_compression_name(*compression), samples, bits),
            Format::Bmp { bpp, header, compression } => format!("BMP {bpp} bits, en-tête {header} octets, compression {compression}"),
        }
    }
}

fn png_color_name(t: u8) -> &'static str {
    match t {
        0 => "gris",
        2 => "RGB",
        3 => "palette",
        4 => "gris+alpha",
        6 => "RGBA",
        _ => "?",
    }
}

fn tiff_compression_name(c: u16) -> String {
    match c {
        1 => "aucune".into(),
        5 => "LZW".into(),
        7 => "JPEG".into(),
        8 | 32946 => "Deflate".into(),
        32773 => "PackBits".into(),
        other => format!("{other}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Analyse des conteneurs

struct PngChunk {
    kind: [u8; 4],
    data: Vec<u8>,
}

fn png_chunks(bytes: &[u8]) -> Result<Vec<PngChunk>> {
    if bytes.len() < 8 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        bail!("signature PNG absente");
    }
    let mut out = Vec::new();
    let mut i = 8;
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
        let kind: [u8; 4] = bytes[i + 4..i + 8].try_into().unwrap();
        let end = i + 8 + len;
        if end + 4 > bytes.len() {
            bail!("chunk {} tronqué", String::from_utf8_lossy(&kind));
        }
        out.push(PngChunk { kind, data: bytes[i + 8..end].to_vec() });
        i = end + 4;
        if &kind == b"IEND" {
            break;
        }
    }
    Ok(out)
}

fn png_format(bytes: &[u8]) -> Result<Format> {
    let chunks = png_chunks(bytes)?;
    let ihdr = chunks.iter().find(|c| &c.kind == b"IHDR").ok_or_else(|| anyhow!("IHDR absent"))?;
    if ihdr.data.len() < 13 {
        bail!("IHDR trop court");
    }
    let ancillary = chunks.iter().filter(|c| COPIED_CHUNKS.contains(&&c.kind)).map(|c| String::from_utf8_lossy(&c.kind).into_owned()).collect();
    Ok(Format::Png { bit_depth: ihdr.data[8], color_type: ihdr.data[9], interlace: ihdr.data[12], ancillary })
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn png_write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_in = Vec::with_capacity(4 + data.len());
    crc_in.extend_from_slice(kind);
    crc_in.extend_from_slice(data);
    out.extend_from_slice(&crc_in);
    out.extend_from_slice(&crc32(&crc_in).to_be_bytes());
}

fn tiff_format(bytes: &[u8]) -> Result<Format> {
    if bytes.len() < 8 {
        bail!("TIFF trop court");
    }
    let le = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => bail!("signature TIFF absente"),
    };
    let u16_at = |o: usize| -> Result<u16> {
        let s: [u8; 2] = bytes.get(o..o + 2).ok_or_else(|| anyhow!("TIFF tronqué"))?.try_into().unwrap();
        Ok(if le { u16::from_le_bytes(s) } else { u16::from_be_bytes(s) })
    };
    let u32_at = |o: usize| -> Result<u32> {
        let s: [u8; 4] = bytes.get(o..o + 4).ok_or_else(|| anyhow!("TIFF tronqué"))?.try_into().unwrap();
        Ok(if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    };
    let ifd = u32_at(4)? as usize;
    let n = u16_at(ifd)? as usize;
    let (mut compression, mut samples, mut photometric, mut bits) = (1u16, 0u16, 0u16, Vec::new());
    for k in 0..n {
        let e = ifd + 2 + 12 * k;
        let tag = u16_at(e)?;
        let typ = u16_at(e + 2)?;
        let count = u32_at(e + 4)? as usize;
        let short_values = |count: usize| -> Result<Vec<u16>> {
            if typ != 3 {
                return Ok(Vec::new());
            }
            if count <= 2 {
                (0..count).map(|i| u16_at(e + 8 + 2 * i)).collect()
            } else {
                let off = u32_at(e + 8)? as usize;
                (0..count).map(|i| u16_at(off + 2 * i)).collect()
            }
        };
        match tag {
            259 => compression = short_values(1)?.first().copied().unwrap_or(1),
            262 => photometric = short_values(1)?.first().copied().unwrap_or(0),
            277 => samples = short_values(1)?.first().copied().unwrap_or(0),
            258 => bits = short_values(count)?,
            _ => {}
        }
    }
    if samples == 0 {
        samples = bits.len().max(1) as u16;
    }
    Ok(Format::Tiff { compression, bits, samples, photometric })
}

/// Pixels RGBA 8 bits d'un TIFF non compressé, entrelacé par pixel (chunky), à 4 échantillons :
/// les bandes sont concaténées. Sert à récupérer l'alpha que `image` ignore faute d'`ExtraSamples`.
fn tiff_raw_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let le = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => bail!("signature TIFF absente"),
    };
    let u16_at = |o: usize| -> Result<u16> {
        let s: [u8; 2] = bytes.get(o..o + 2).ok_or_else(|| anyhow!("TIFF tronqué"))?.try_into().unwrap();
        Ok(if le { u16::from_le_bytes(s) } else { u16::from_be_bytes(s) })
    };
    let u32_at = |o: usize| -> Result<u32> {
        let s: [u8; 4] = bytes.get(o..o + 4).ok_or_else(|| anyhow!("TIFF tronqué"))?.try_into().unwrap();
        Ok(if le { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    };
    let ifd = u32_at(4)? as usize;
    let n = u16_at(ifd)? as usize;
    let (mut w, mut h, mut samples, mut compression, mut planar) = (0u32, 0u32, 0u16, 1u16, 1u16);
    let (mut offsets, mut counts): (Vec<u32>, Vec<u32>) = (Vec::new(), Vec::new());
    for k in 0..n {
        let e = ifd + 2 + 12 * k;
        let (tag, typ, count) = (u16_at(e)?, u16_at(e + 2)?, u32_at(e + 4)? as usize);
        // Valeurs SHORT ou LONG, en ligne (≤ 4 octets) ou déportées.
        let values = |count: usize| -> Result<Vec<u32>> {
            let size = if typ == 3 { 2 } else { 4 };
            let base = if size * count <= 4 { e + 8 } else { u32_at(e + 8)? as usize };
            (0..count).map(|i| if typ == 3 { u16_at(base + 2 * i).map(u32::from) } else { u32_at(base + 4 * i) }).collect()
        };
        match tag {
            256 => w = values(1)?[0],
            257 => h = values(1)?[0],
            259 => compression = values(1)?[0] as u16,
            277 => samples = values(1)?[0] as u16,
            284 => planar = values(1)?[0] as u16,
            273 => offsets = values(count)?,
            279 => counts = values(count)?,
            _ => {}
        }
    }
    if compression != 1 || planar != 1 || samples != 4 {
        bail!("TIFF brut non géré (compression {compression}, planar {planar}, {samples} échantillons)");
    }
    let expected = (w as usize) * (h as usize) * 4;
    let mut data = Vec::with_capacity(expected);
    if counts.len() != offsets.len() {
        counts = vec![expected as u32; offsets.len().max(1)];
    }
    for (o, c) in offsets.iter().zip(&counts) {
        let (o, c) = (*o as usize, *c as usize);
        data.extend_from_slice(bytes.get(o..o + c).ok_or_else(|| anyhow!("bande hors du fichier"))?);
    }
    if data.len() < expected {
        bail!("données TIFF incomplètes : {} octets pour {expected}", data.len());
    }
    data.truncate(expected);
    Ok((w, h, data))
}

fn bmp_format(bytes: &[u8]) -> Result<Format> {
    if bytes.len() < 34 || &bytes[..2] != b"BM" {
        bail!("signature BMP absente");
    }
    let header = u32::from_le_bytes(bytes[14..18].try_into().unwrap());
    let bpp = u16::from_le_bytes(bytes[28..30].try_into().unwrap());
    let compression = u32::from_le_bytes(bytes[30..34].try_into().unwrap());
    Ok(Format::Bmp { bpp, header, compression })
}

fn detect(path: &Path, bytes: &[u8]) -> Result<Format> {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("png") => png_format(bytes),
        Some("tif") | Some("tiff") => tiff_format(bytes),
        Some("bmp") => bmp_format(bytes),
        other => bail!("extension non gérée : {other:?}"),
    }
}

/// Les deux formats sont-ils équivalents pour le client ?
fn conforms(original: &Format, upgrade: &Format) -> bool {
    match (original, upgrade) {
        (Format::Png { color_type: a, bit_depth: b, interlace: c, ancillary: anc_o }, Format::Png { color_type: x, bit_depth: y, interlace: z, ancillary: anc_u }) => {
            let mut so = anc_o.clone();
            let mut su = anc_u.clone();
            so.sort();
            su.sort();
            so.dedup();
            su.dedup();
            a == x && b == y && c == z && so == su
        }
        (Format::Tiff { compression: a, bits: b, samples: c, .. }, Format::Tiff { compression: x, bits: y, samples: z, .. }) => a == x && b == y && c == z,
        (Format::Bmp { bpp: a, header: b, compression: c }, Format::Bmp { bpp: x, header: y, compression: z }) => a == x && b == y && c == z,
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// Ré-encodage

fn to_color(img: &DynamicImage, color_type: u8, bit_depth: u8) -> Result<(Vec<u8>, ExtendedColorType)> {
    if bit_depth != 8 {
        bail!("profondeur {bit_depth} bits non gérée (seulement 8)");
    }
    Ok(match color_type {
        0 => (img.to_luma8().into_raw(), ExtendedColorType::L8),
        2 => (img.to_rgb8().into_raw(), ExtendedColorType::Rgb8),
        4 => (img.to_luma_alpha8().into_raw(), ExtendedColorType::La8),
        3 | 6 => (img.to_rgba8().into_raw(), ExtendedColorType::Rgba8),
        other => bail!("type de couleur PNG {other} non géré"),
    })
}

/// Si l'original a une couche alpha et pas la version retravaillée, l'alpha de l'original est
/// redimensionné aux dimensions de la nouvelle image et greffé dessus (sinon tout serait opaque).
fn restore_alpha(img: DynamicImage, original_bytes: &[u8], original: &Format) -> Result<(DynamicImage, bool)> {
    let wants_alpha = match original {
        Format::Png { color_type, .. } => matches!(color_type, 4 | 6),
        Format::Tiff { samples, .. } => *samples == 4,
        Format::Bmp { bpp, .. } => *bpp == 32,
    };
    if !wants_alpha || img.color().has_alpha() {
        return Ok((img, false));
    }
    // Alpha de l'original : via `image`, ou, pour un TIFF à 4 échantillons sans `ExtraSamples`
    // (que la bibliothèque décode en RGB), directement dans les octets bruts.
    let orig_alpha: image::GrayImage = match original {
        Format::Tiff { .. } => {
            let (ow, oh, rgba) = tiff_raw_rgba(original_bytes).context("lecture brute du TIFF original")?;
            image::ImageBuffer::from_fn(ow, oh, |x, y| image::Luma([rgba[((y * ow + x) * 4 + 3) as usize]]))
        }
        _ => {
            let orig = image::ImageReader::new(Cursor::new(original_bytes)).with_guessed_format()?.decode().context("décodage de l'original pour son alpha")?;
            if !orig.color().has_alpha() {
                return Ok((img, false));
            }
            let rgba = orig.to_rgba8();
            image::ImageBuffer::from_fn(rgba.width(), rgba.height(), |x, y| image::Luma([rgba.get_pixel(x, y).0[3]]))
        }
    };
    let (w, h) = (img.width(), img.height());
    let alpha = DynamicImage::ImageLuma8(orig_alpha).resize_exact(w, h, image::imageops::FilterType::Triangle).to_luma8();
    let mut rgba = img.to_rgba8();
    for (p, a) in rgba.pixels_mut().zip(alpha.pixels()) {
        p.0[3] = a.0[0];
    }
    Ok((DynamicImage::ImageRgba8(rgba), true))
}

fn reencode(original_bytes: &[u8], original: &Format, upgrade_path: &Path, alpha_restored: &mut bool) -> Result<Vec<u8>> {
    let img = image::ImageReader::open(upgrade_path)?.with_guessed_format()?.decode().with_context(|| format!("décodage de {}", upgrade_path.display()))?;
    let (img, restored) = restore_alpha(img, original_bytes, original)?;
    *alpha_restored = restored;
    let (w, h) = (img.width(), img.height());
    match original {
        Format::Png { color_type, bit_depth, .. } => {
            // Une palette d'origine est rendue en RGBA : le client lit les deux.
            let (raw, ct) = to_color(&img, *color_type, *bit_depth)?;
            let mut encoded = Vec::new();
            image::codecs::png::PngEncoder::new_with_quality(&mut encoded, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive).write_image(&raw, w, h, ct)?;
            // Greffe des chunks auxiliaires de l'original juste après IHDR.
            let new_chunks = png_chunks(&encoded)?;
            let orig_chunks = png_chunks(original_bytes)?;
            let mut out = Vec::with_capacity(encoded.len() + 64);
            out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
            let ihdr = new_chunks.iter().find(|c| &c.kind == b"IHDR").ok_or_else(|| anyhow!("IHDR absent après encodage"))?;
            png_write_chunk(&mut out, b"IHDR", &ihdr.data);
            for c in orig_chunks.iter().filter(|c| COPIED_CHUNKS.contains(&&c.kind)) {
                png_write_chunk(&mut out, &c.kind, &c.data);
            }
            for c in new_chunks.iter().filter(|c| &c.kind != b"IHDR" && !COPIED_CHUNKS.contains(&&c.kind)) {
                png_write_chunk(&mut out, &c.kind, &c.data);
            }
            Ok(out)
        }
        Format::Tiff { compression, samples, bits, .. } => {
            if *compression != 1 {
                bail!("l'original est compressé ({}) : seul le TIFF non compressé est écrit", tiff_compression_name(*compression));
            }
            if bits.iter().any(|&b| b != 8) {
                bail!("TIFF {bits:?} bits non géré");
            }
            let (raw, ct) = match samples {
                1 => (img.to_luma8().into_raw(), ExtendedColorType::L8),
                3 => (img.to_rgb8().into_raw(), ExtendedColorType::Rgb8),
                4 => (img.to_rgba8().into_raw(), ExtendedColorType::Rgba8),
                other => bail!("TIFF à {other} échantillons non géré"),
            };
            let mut cursor = Cursor::new(Vec::new());
            image::codecs::tiff::TiffEncoder::new(&mut cursor).write_image(&raw, w, h, ct)?;
            Ok(cursor.into_inner())
        }
        Format::Bmp { bpp, compression, .. } => {
            if *compression != 0 {
                bail!("BMP compressé ({compression}) non géré");
            }
            let (raw, ct) = match bpp {
                24 => (img.to_rgb8().into_raw(), ExtendedColorType::Rgb8),
                32 => (img.to_rgba8().into_raw(), ExtendedColorType::Rgba8),
                other => bail!("BMP {other} bits non géré"),
            };
            let mut encoded = Vec::new();
            image::codecs::bmp::BmpEncoder::new(&mut encoded).encode(&raw, w, h, ct)?;
            Ok(encoded)
        }
    }
}

// ---------------------------------------------------------------------------------------------

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("lecture de {}", dir.display()))? {
        let p = entry?.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Diagnostic : `--probe <fichiers…>` affiche le format détecté et le type de couleur décodé.
    if args.first().map(String::as_str) == Some("--probe") {
        for p in &args[1..] {
            let path = Path::new(p);
            let bytes = fs::read(path)?;
            let detected = detect(path, &bytes).map(|f| f.describe()).unwrap_or_else(|e| format!("non détecté : {e}"));
            let decoded = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?.decode().map(|i| format!("{:?} {}x{}", i.color(), i.width(), i.height())).unwrap_or_else(|e| format!("décodage impossible : {e}"));
            println!("{p}\n  détecté : {detected}\n  décodé  : {decoded}");
        }
        return Ok(());
    }
    let Some(root) = args.first().map(PathBuf::from) else {
        bail!("usage : reformat_like <dossier> [--apply] [--backup <dossier>]");
    };
    let apply = args.iter().any(|a| a == "--apply");
    let backup = args.iter().position(|a| a == "--backup").and_then(|i| args.get(i + 1)).map(PathBuf::from);

    let mut files = Vec::new();
    walk(&root, &mut files)?;
    files.sort();
    let mut stats: BTreeMap<&str, usize> = BTreeMap::new();
    let mut orphans = Vec::new();
    let mut alpha_fixed = Vec::new();
    let mut errors = Vec::new();
    let mut changes: BTreeMap<String, usize> = BTreeMap::new();

    for up in &files {
        let Some(stem) = up.file_stem().and_then(|s| s.to_str()) else { continue };
        let Some(base) = stem.strip_suffix(SUFFIX) else { continue };
        let ext = up.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if !matches!(ext.as_str(), "png" | "tif" | "tiff" | "bmp") {
            continue;
        }
        let orig = up.with_file_name(format!("{base}.{}", up.extension().and_then(|e| e.to_str()).unwrap_or("")));
        let rel = up.strip_prefix(&root).unwrap_or(up).to_path_buf();
        if !orig.is_file() {
            orphans.push(rel);
            continue;
        }
        let result: Result<&str> = (|| {
            let ob = fs::read(&orig)?;
            let ub = fs::read(up)?;
            let of = detect(&orig, &ob).with_context(|| format!("original {}", orig.display()))?;
            let uf = detect(up, &ub).with_context(|| format!("upgrade {}", up.display()))?;
            if conforms(&of, &uf) {
                return Ok("conforme");
            }
            *changes.entry(format!("{}  ←  {}", of.describe(), uf.describe())).or_default() += 1;
            if !apply {
                return Ok("à convertir");
            }
            let mut alpha_restored = false;
            let bytes = reencode(&ob, &of, up, &mut alpha_restored)?;
            if alpha_restored {
                alpha_fixed.push(rel.clone());
            }
            let check = detect(up, &bytes)?;
            if !conforms(&of, &check) {
                bail!("après ré-encodage, toujours différent : {} vs {}", of.describe(), check.describe());
            }
            if let Some(b) = &backup {
                let dest = b.join(&rel);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::copy(up, &dest)?;
            }
            let tmp = up.with_extension(format!("{ext}.tmp"));
            fs::write(&tmp, &bytes)?;
            fs::rename(&tmp, up)?;
            Ok("converti")
        })();
        match result {
            Ok(k) => *stats.entry(k).or_default() += 1,
            Err(e) => {
                errors.push(format!("{} : {e:#}", rel.display()));
                *stats.entry("erreur").or_default() += 1;
            }
        }
    }

    println!("# {} — {}", root.display(), if apply { "conversion appliquée" } else { "rapport (aucune écriture)" });
    for (k, v) in &stats {
        println!("{v:6}  {k}");
    }
    println!("{:6}  sans original (ignorés)", orphans.len());
    if !changes.is_empty() {
        println!("\n## Écarts rencontrés (format d'origine ← format de l'upgrade)");
        let mut list: Vec<_> = changes.iter().collect();
        list.sort_by(|a, b| b.1.cmp(a.1));
        for (k, v) in list {
            println!("{v:6}  {k}");
        }
    }
    if !alpha_fixed.is_empty() {
        println!("\n## Couche alpha reprise de l'original (redimensionnée)");
        for a in &alpha_fixed {
            println!("  {}", a.display());
        }
    }
    if !orphans.is_empty() {
        println!("\n## Upgrades sans original");
        for o in &orphans {
            println!("  {}", o.display());
        }
    }
    if !errors.is_empty() {
        println!("\n## Erreurs");
        for e in &errors {
            println!("  {e}");
        }
    }
    Ok(())
}
