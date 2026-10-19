//! Packs de sons `sound/effect/*.pk` et leur index `sound/soundpak.info`.
//!
//! Un `.pk` est une simple suite de fichiers WAV (PCM 16 bits) mis bout à bout, sans en-tête.
//! `soundpak.info` les décrit, en petit-boutiste :
//!
//! ```text
//! groupe (40 octets) : u32 version (= 1), char[8] nom ("EFFECT"), 6 × u32 réservés, u32 nb_packs
//!   pack (20 octets) : u8 drapeau, char[7] fichier ("SD.pk"), char[4] préfixe ("SD"),
//!                      u32 nb_entrées, u32 dernier_index
//!     entrée (28 octets) : u32 index, char[12] nom ("SD0000.WAV"), u32 décalage, u32 taille, u32 id
//! ```
//!
//! Plusieurs groupes peuvent se suivre jusqu'à la fin du fichier. Les packs absents de l'index
//! (dossier `sound/effect/mono/`) se découpent par balayage des en-têtes `RIFF….WAVE`
//! ([`scan_riff`]) ; leurs entrées sont alors nommées `<pack><numéro sur 4 chiffres>.wav`.
//!
//! Les noms sont comparés sans tenir compte de la casse (le client tourne sous Windows).

use std::fmt;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SoundPakError {
    #[error("index tronqué à l'octet {0}")]
    Truncated(usize),
    #[error("en-tête de groupe inattendu à {offset:#x} (version {version}, groupe {group:?})")]
    BadGroup {
        offset: usize,
        version: u32,
        group: String,
    },
    #[error("entrée {name} hors du pack (décalage {offset}, taille {size}, pack de {len} octets)")]
    OutOfRange {
        name: String,
        offset: usize,
        size: usize,
        len: usize,
    },
    #[error("entrée {0} : pas un fichier WAV (en-tête RIFF absent)")]
    NotRiff(String),
}

/// Un WAV dans un pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundEntry {
    pub index: u32,
    /// Nom tel qu'écrit dans l'index (`SD0000.WAV`) ou généré par le balayage (`maa0003.wav`).
    pub name: String,
    pub offset: usize,
    pub size: usize,
    pub id: u32,
}

impl SoundEntry {
    /// Octets du WAV dans les données du pack.
    pub fn slice<'a>(&self, pack: &'a [u8]) -> Result<&'a [u8], SoundPakError> {
        let data = self
            .offset
            .checked_add(self.size)
            .and_then(|end| pack.get(self.offset..end))
            .ok_or_else(|| SoundPakError::OutOfRange {
                name: self.name.clone(),
                offset: self.offset,
                size: self.size,
                len: pack.len(),
            })?;
        if !data.starts_with(b"RIFF") {
            return Err(SoundPakError::NotRiff(self.name.clone()));
        }
        Ok(data)
    }

    /// Les noms d'entrées se comparent sans la casse.
    pub fn is_named(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// Un fichier `.pk` décrit par l'index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundPack {
    pub flag: u8,
    /// Nom du fichier (`SD.pk`).
    pub file: String,
    /// Préfixe des noms d'entrées (`SD`).
    pub prefix: String,
    pub last_index: u32,
    pub entries: Vec<SoundEntry>,
}

impl SoundPack {
    /// Nom du fichier sans extension, en minuscules (`sd`).
    pub fn stem(&self) -> String {
        let f = self.file.to_ascii_lowercase();
        f.strip_suffix(".pk").unwrap_or(&f).to_string()
    }

    pub fn entry(&self, name: &str) -> Option<&SoundEntry> {
        self.entries.iter().find(|e| e.is_named(name))
    }

    /// Somme des tailles déclarées (taille minimale attendue du `.pk`).
    pub fn declared_len(&self) -> usize {
        self.entries
            .iter()
            .map(|e| e.offset + e.size)
            .max()
            .unwrap_or(0)
    }
}

/// Un groupe de packs (`EFFECT`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundGroup {
    pub version: u32,
    pub name: String,
    pub reserved: [u32; 6],
    pub packs: Vec<SoundPack>,
}

/// Contenu de `soundpak.info`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundIndex {
    pub groups: Vec<SoundGroup>,
}

const GROUP_HEADER: usize = 40;
/// Tailles des enregistrements (pack, entrée) ; voir l'en-tête du module.
#[allow(dead_code)]
const PACK_HEADER: usize = 20;
#[allow(dead_code)]
const ENTRY_SIZE: usize = 28;

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], SoundPakError> {
        let s = self
            .bytes
            .get(self.pos..self.pos + n)
            .ok_or(SoundPakError::Truncated(self.pos))?;
        self.pos += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, SoundPakError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, SoundPakError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// Chaîne de `n` octets terminée par 0 (ou pleine).
    fn cstr(&mut self, n: usize) -> Result<String, SoundPakError> {
        let s = self.take(n)?;
        let end = s.iter().position(|&b| b == 0).unwrap_or(n);
        Ok(String::from_utf8_lossy(&s[..end]).into_owned())
    }
}

impl SoundIndex {
    /// Analyse `soundpak.info`.
    pub fn parse(bytes: &[u8]) -> Result<Self, SoundPakError> {
        let mut c = Cursor { bytes, pos: 0 };
        let mut groups = Vec::new();
        while c.pos + GROUP_HEADER <= bytes.len() {
            let offset = c.pos;
            let version = c.u32()?;
            let name = c.cstr(8)?;
            let mut reserved = [0u32; 6];
            for r in &mut reserved {
                *r = c.u32()?;
            }
            let count = c.u32()? as usize;
            if version != 1 || name.is_empty() {
                return Err(SoundPakError::BadGroup {
                    offset,
                    version,
                    group: name,
                });
            }
            let mut packs = Vec::with_capacity(count);
            for _ in 0..count {
                let flag = c.u8()?;
                let file = c.cstr(7)?;
                let prefix = c.cstr(4)?;
                let entries_n = c.u32()? as usize;
                let last_index = c.u32()?;
                let mut entries = Vec::with_capacity(entries_n.min(1 << 16));
                for _ in 0..entries_n {
                    let index = c.u32()?;
                    let name = c.cstr(12)?;
                    let offset = c.u32()? as usize;
                    let size = c.u32()? as usize;
                    let id = c.u32()?;
                    entries.push(SoundEntry {
                        index,
                        name,
                        offset,
                        size,
                        id,
                    });
                }
                packs.push(SoundPack {
                    flag,
                    file,
                    prefix,
                    last_index,
                    entries,
                });
            }
            groups.push(SoundGroup {
                version,
                name,
                reserved,
                packs,
            });
        }
        Ok(Self { groups })
    }

    pub fn packs(&self) -> impl Iterator<Item = &SoundPack> {
        self.groups.iter().flat_map(|g| g.packs.iter())
    }

    pub fn entry_count(&self) -> usize {
        self.packs().map(|p| p.entries.len()).sum()
    }

    /// Pack par nom de fichier (`sd.pk`, `SD`, `effect/sd.pk`…), sans la casse.
    pub fn pack(&self, file: &str) -> Option<&SoundPack> {
        let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let stem = base
            .strip_suffix(".pk")
            .or_else(|| base.strip_suffix(".PK"))
            .unwrap_or(base);
        self.packs().find(|p| p.stem().eq_ignore_ascii_case(stem))
    }

    /// Entrée par nom (`sd0000.wav`), dans tous les packs.
    pub fn find(&self, name: &str) -> Option<(&SoundPack, &SoundEntry)> {
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        self.packs()
            .find_map(|p| p.entry(base).map(|e| (p, e)))
    }
}

/// Découpe un `.pk` sans index en balayant les en-têtes `RIFF….WAVE`. Les entrées sont nommées
/// `<prefix><numéro sur 4 chiffres>.wav`, dans l'ordre du fichier.
pub fn scan_riff(bytes: &[u8], prefix: &str) -> Vec<SoundEntry> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 12 <= bytes.len() {
        if &bytes[pos..pos + 4] == b"RIFF" && &bytes[pos + 8..pos + 12] == b"WAVE" {
            let declared = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let size = declared.saturating_add(8).min(bytes.len() - pos);
            let n = out.len() as u32;
            out.push(SoundEntry {
                index: n,
                name: format!("{prefix}{n:04}.wav"),
                offset: pos,
                size,
                id: n,
            });
            pos += size.max(12);
        } else {
            pos += 1;
        }
    }
    out
}

/// En-tête d'un WAV (bloc `fmt `), pour décrire une entrée sans la décoder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    /// Code de format (`1` = PCM, `3` = flottant, `0xfffe` = extensible).
    pub format: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Taille du bloc `data`.
    pub data_len: usize,
}

impl WavInfo {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return None;
        }
        let mut pos = 12;
        let mut fmt = None;
        let mut data_len = None;
        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let body = pos + 8;
            match id {
                b"fmt " if body + 16 <= bytes.len() => {
                    let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
                    fmt = Some((
                        u16_at(body),
                        u16_at(body + 2),
                        u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap()),
                        u16_at(body + 14),
                    ));
                }
                b"data" => {
                    data_len = Some(len.min(bytes.len().saturating_sub(body)));
                    if fmt.is_some() {
                        break;
                    }
                }
                _ => {}
            }
            pos = body + len + (len & 1);
        }
        let (format, channels, sample_rate, bits_per_sample) = fmt?;
        Some(Self {
            format,
            channels,
            sample_rate,
            bits_per_sample,
            data_len: data_len.unwrap_or(0),
        })
    }

    /// Durée en secondes (PCM entier).
    pub fn duration_secs(&self) -> f32 {
        let frame = self.channels as usize * (self.bits_per_sample as usize / 8).max(1);
        if self.sample_rate == 0 || frame == 0 {
            return 0.0;
        }
        (self.data_len / frame) as f32 / self.sample_rate as f32
    }
}

impl fmt::Display for WavInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.format {
            1 => "PCM",
            3 => "flottant",
            0xfffe => "extensible",
            _ => "format inconnu",
        };
        write!(
            f,
            "{kind} {} bits, {} Hz, {}, {:.2} s",
            self.bits_per_sample,
            self.sample_rate,
            match self.channels {
                1 => "mono".to_string(),
                2 => "stéréo".to_string(),
                n => format!("{n} canaux"),
            },
            self.duration_secs()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WAV PCM 16 bits mono 22 050 Hz contenant `samples`.
    pub(crate) fn wav(samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&22_050u32.to_le_bytes());
        v.extend_from_slice(&(22_050u32 * 2).to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(&data);
        v
    }

    fn fixed(s: &str, n: usize) -> Vec<u8> {
        let mut v = s.as_bytes().to_vec();
        v.resize(n, 0);
        v
    }

    /// Index synthétique : un groupe `EFFECT`, un pack `SD.pk` et ses entrées.
    fn index_bytes(entries: &[(&str, u32, u32)]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&1u32.to_le_bytes());
        v.extend_from_slice(&fixed("EFFECT", 8));
        for r in [7u32, 0, 0, 0, 0, 0] {
            v.extend_from_slice(&r.to_le_bytes());
        }
        v.extend_from_slice(&1u32.to_le_bytes()); // nb_packs
        v.push(0x3a);
        v.extend_from_slice(&fixed("SD.pk", 7));
        v.extend_from_slice(&fixed("SD", 4));
        v.extend_from_slice(&(entries.len() as u32).to_le_bytes());
        v.extend_from_slice(&(entries.len() as u32 - 1).to_le_bytes());
        for (i, (name, off, size)) in entries.iter().enumerate() {
            v.extend_from_slice(&(i as u32).to_le_bytes());
            v.extend_from_slice(&fixed(name, 12));
            v.extend_from_slice(&off.to_le_bytes());
            v.extend_from_slice(&size.to_le_bytes());
            v.extend_from_slice(&(100 + i as u32).to_le_bytes());
        }
        v
    }

    #[test]
    fn parses_index_and_reads_entries_from_pack() {
        let a = wav(&[1, 2, 3, 4]);
        let b = wav(&[-5; 10]);
        let mut pack = a.clone();
        pack.extend_from_slice(&b);
        let idx = SoundIndex::parse(&index_bytes(&[
            ("SD0000.WAV", 0, a.len() as u32),
            ("SD0001.WAV", a.len() as u32, b.len() as u32),
        ]))
        .unwrap();
        assert_eq!(idx.groups.len(), 1);
        assert_eq!(idx.groups[0].name, "EFFECT");
        assert_eq!(idx.groups[0].reserved[0], 7);
        assert_eq!(idx.entry_count(), 2);
        let p = idx.pack("effect/SD.PK").expect("pack");
        assert_eq!(p.stem(), "sd");
        assert_eq!(p.prefix, "SD");
        assert_eq!(p.flag, 0x3a);
        assert_eq!(p.last_index, 1);
        assert_eq!(p.declared_len(), pack.len());
        let (p2, e) = idx.find("sound/effect/sd0001.wav").expect("entrée");
        assert_eq!(p2.file, "SD.pk");
        assert_eq!(e.index, 1);
        assert_eq!(e.id, 101);
        assert_eq!(e.slice(&pack).unwrap(), &b[..]);
        let info = WavInfo::parse(e.slice(&pack).unwrap()).unwrap();
        assert_eq!((info.channels, info.sample_rate, info.bits_per_sample), (1, 22_050, 16));
        assert_eq!(info.data_len, 20);
        assert!(idx.find("sd0002.wav").is_none());
        assert!(idx.pack("xx.pk").is_none());
    }

    #[test]
    fn rejects_truncated_index_and_bad_header() {
        let full = index_bytes(&[("SD0000.WAV", 0, 10)]);
        assert!(matches!(
            SoundIndex::parse(&full[..full.len() - 3]),
            Err(SoundPakError::Truncated(_))
        ));
        let mut bad = full.clone();
        bad[0] = 2;
        assert!(matches!(
            SoundIndex::parse(&bad),
            Err(SoundPakError::BadGroup { version: 2, .. })
        ));
        // Un fichier vide est un index vide ; une queue trop courte pour un groupe est ignorée.
        assert_eq!(SoundIndex::parse(&[]).unwrap().entry_count(), 0);
        assert_eq!(SoundIndex::parse(&[0u8; 39]).unwrap().groups.len(), 0);
    }

    #[test]
    fn entry_out_of_range_or_not_riff_is_an_error() {
        let pack = wav(&[0; 4]);
        let e = SoundEntry {
            index: 0,
            name: "x.wav".into(),
            offset: 4,
            size: pack.len(),
            id: 0,
        };
        assert!(matches!(e.slice(&pack), Err(SoundPakError::OutOfRange { .. })));
        let e = SoundEntry {
            offset: 4,
            size: 8,
            ..e
        };
        assert!(matches!(e.slice(&pack), Err(SoundPakError::NotRiff(_))));
    }

    #[test]
    fn riff_scan_splits_a_pack_without_index() {
        let a = wav(&[1, 2, 3]);
        let b = wav(&[9; 7]);
        let c = wav(&[]);
        let mut pack = Vec::new();
        pack.extend_from_slice(&[0xffu8; 5]); // déchets avant le premier WAV
        pack.extend_from_slice(&a);
        pack.extend_from_slice(&b);
        pack.extend_from_slice(&c);
        let entries = scan_riff(&pack, "maa");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "maa0000.wav");
        assert_eq!(entries[2].name, "maa0002.wav");
        assert_eq!(entries[0].slice(&pack).unwrap(), &a[..]);
        assert_eq!(entries[1].slice(&pack).unwrap(), &b[..]);
        assert_eq!(entries[2].slice(&pack).unwrap(), &c[..]);
        // Taille RIFF mensongère : l'entrée est bornée au fichier.
        let mut lying = wav(&[1]);
        lying[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        let e = scan_riff(&lying, "x");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].size, lying.len());
    }

    #[test]
    fn wav_info_duration_and_display() {
        let info = WavInfo::parse(&wav(&[0; 22_050])).unwrap();
        assert!((info.duration_secs() - 1.0).abs() < 1e-6);
        assert_eq!(info.to_string(), "PCM 16 bits, 22050 Hz, mono, 1.00 s");
        assert!(WavInfo::parse(b"RIFX").is_none());
    }
}
