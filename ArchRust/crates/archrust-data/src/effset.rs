//! Jeux d'effets du client : `effect/ini/new/efffile_set.eff` (fiches) + `efffile_var.eff` (blocs).
//!
//! Format reconstitué sur les données et sur le chargeur d'Archonia (`AgcdEffSetMng`,
//! `efffile_set.eff` lu fiche par fiche en onze `fread` consécutifs) :
//!
//! ```text
//! efffile_set.eff : u32 nombre de fiches, puis fiches de 124 octets :
//!   u32 id (EffectIndex de skyset.ini, EFFECT des tables d'objets…)
//!   char[32] nom (CP949, terminé par 0)
//!   u32 ?, u32 ?, u32 ?, 16 octets, 16 octets, 24 octets (boîte, durée…),
//!   u32 nb_bases, u32 nb_effets, u32 ?,
//!   u32 décalage, u32 taille : bloc du jeu dans efffile_var.eff
//! efffile_var.eff : blocs mis bout à bout, chacun = enregistrements des bases puis des effets
//!   du jeu, écrits par la méthode virtuelle de chaque type d'effet (tailles propres au type).
//! ```
//!
//! Seul l'effet **son** (type 6, 196 octets) est décodé ici : `u32 type = 6`, 52 octets de
//! paramètres (observés : `1` en 0x2c, `1` en 0x32, `2` en 0x38), `char[128]` nom du fichier
//! (`A_CID_D.mp3`, joué par Archonia depuis `SOUND\STREAM\` en boucle), `f32` volume (1,1).
//! Un bloc qui n'est pas une suite d'effets son est balayé à la recherche de noms `*.mp3`
//! terminés par 0 (repli, signalé par [`EffectSet::scanned`]).

use std::collections::HashMap;

/// Taille d'une fiche de `efffile_set.eff`.
pub const SET_RECORD_LEN: usize = 124;
/// Taille d'un enregistrement d'effet son dans `efffile_var.eff`.
pub const SOUND_RECORD_LEN: usize = 196;
/// Type d'effet « son » (`AgcdEffSet`).
pub const EFFECT_TYPE_SOUND: u32 = 6;

/// Un jeu d'effets et les sons qu'il déclenche.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectSet {
    pub id: u32,
    pub name: String,
    /// Décalage et taille du bloc dans `efffile_var.eff`.
    pub offset: u32,
    pub len: u32,
    /// Noms des fichiers son (tels qu'écrits : `A_CID_D.mp3`), dans l'ordre du bloc.
    pub sounds: Vec<String>,
    /// `true` si les sons viennent d'un balayage du bloc (effets d'autres types mêlés), `false`
    /// si le bloc n'est fait que d'effets son décodés exactement.
    pub scanned: bool,
}

/// Table des jeux d'effets, indexée par `id`.
#[derive(Debug, Default, Clone)]
pub struct EffectSets {
    sets: HashMap<u32, EffectSet>,
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4).map(|c| u32::from_le_bytes(c.try_into().unwrap()))
}

fn cstr(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..end]
}

/// Sons d'un bloc : suite d'effets son décodés, sinon balayage des `*.mp3`.
pub fn block_sounds(block: &[u8]) -> (Vec<String>, bool) {
    if !block.is_empty()
        && block.len().is_multiple_of(SOUND_RECORD_LEN)
        && block
            .chunks_exact(SOUND_RECORD_LEN)
            .all(|r| u32_at(r, 0) == Some(EFFECT_TYPE_SOUND))
    {
        let sounds = block
            .chunks_exact(SOUND_RECORD_LEN)
            .map(|r| String::from_utf8_lossy(cstr(&r[0x3c..0xbc])).into_owned())
            .filter(|s| !s.is_empty())
            .collect();
        return (sounds, false);
    }
    // Repli : tout nom `….mp3` terminé par 0 dont le début suit un 0.
    let mut sounds = Vec::new();
    let lower: Vec<u8> = block.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut i = 0;
    while i + 5 <= lower.len() {
        if &lower[i..i + 4] == b".mp3" && lower.get(i + 4) == Some(&0) {
            let start = block[..i].iter().rposition(|&c| c == 0).map_or(0, |p| p + 1);
            let name = &block[start..i + 4];
            if name.iter().all(|c| c.is_ascii_graphic()) {
                sounds.push(String::from_utf8_lossy(name).into_owned());
            }
            i += 5;
        } else {
            i += 1;
        }
    }
    (sounds, true)
}

impl EffectSets {
    /// Lit les deux fichiers ; les fiches tronquées ou hors du fichier de blocs sont ignorées.
    pub fn parse(set_file: &[u8], var_file: &[u8]) -> Self {
        let mut sets = HashMap::new();
        let Some(count) = u32_at(set_file, 0) else {
            return Self { sets };
        };
        for i in 0..count as usize {
            let Some(r) = set_file.get(4 + i * SET_RECORD_LEN..4 + (i + 1) * SET_RECORD_LEN) else {
                break;
            };
            let id = u32_at(r, 0).unwrap();
            let name = crate::ini::decode_cp949(cstr(&r[4..36]));
            let offset = u32_at(r, 0x74).unwrap();
            let len = u32_at(r, 0x78).unwrap();
            let (sounds, scanned) = match var_file.get(offset as usize..(offset + len) as usize) {
                Some(block) if len > 0 => block_sounds(block),
                _ => (Vec::new(), false),
            };
            sets.insert(
                id,
                EffectSet {
                    id,
                    name,
                    offset,
                    len,
                    sounds,
                    scanned,
                },
            );
        }
        Self { sets }
    }

    pub fn get(&self, id: u32) -> Option<&EffectSet> {
        self.sets.get(&id)
    }

    pub fn len(&self) -> usize {
        self.sets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// Jeux triés par `id`.
    pub fn iter(&self) -> impl Iterator<Item = &EffectSet> {
        let mut v: Vec<&EffectSet> = self.sets.values().collect();
        v.sort_by_key(|s| s.id);
        v.into_iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sound_record(name: &str) -> Vec<u8> {
        let mut r = vec![0u8; SOUND_RECORD_LEN];
        r[0] = EFFECT_TYPE_SOUND as u8;
        r[0x2c] = 1;
        r[0x32] = 1;
        r[0x38] = 2;
        r[0x3c..0x3c + name.len()].copy_from_slice(name.as_bytes());
        r[0xc0..0xc4].copy_from_slice(&1.1f32.to_le_bytes());
        r
    }

    fn set_record(id: u32, name: &str, offset: u32, len: u32) -> Vec<u8> {
        let mut r = vec![0u8; SET_RECORD_LEN];
        r[0..4].copy_from_slice(&id.to_le_bytes());
        r[4..4 + name.len()].copy_from_slice(name.as_bytes());
        r[0x74..0x78].copy_from_slice(&offset.to_le_bytes());
        r[0x78..0x7c].copy_from_slice(&len.to_le_bytes());
        r
    }

    #[test]
    fn sound_sets_are_decoded_exactly_and_mixed_blocks_are_scanned() {
        let mut var = sound_record("A_GRN_M.mp3");
        var.extend(sound_record("OE0000.mp3"));
        let mixed_at = var.len() as u32;
        // Bloc mêlé : un effet d'un autre type (type 3, 40 octets inventés) puis un son.
        let mut mixed = vec![0u8; 40];
        mixed[0] = 3;
        mixed.extend(b"\0WATER_FALL.mp3\0suite");
        var.extend(&mixed);
        let mut set = 3u32.to_le_bytes().to_vec();
        set.extend(set_record(3828, "field-dawn", 0, SOUND_RECORD_LEN as u32));
        set.extend(set_record(3806, "Water_Small1", SOUND_RECORD_LEN as u32, SOUND_RECORD_LEN as u32));
        set.extend(set_record(7, "Fall", mixed_at, mixed.len() as u32));
        let sets = EffectSets::parse(&set, &var);
        assert_eq!(sets.len(), 3);
        let s = sets.get(3828).unwrap();
        assert_eq!(s.name, "field-dawn");
        assert_eq!(s.sounds, vec!["A_GRN_M.mp3".to_string()]);
        assert!(!s.scanned);
        assert_eq!(sets.get(3806).unwrap().sounds, vec!["OE0000.mp3".to_string()]);
        let fall = sets.get(7).unwrap();
        assert_eq!(fall.sounds, vec!["WATER_FALL.mp3".to_string()]);
        assert!(fall.scanned);
        assert_eq!(sets.iter().map(|s| s.id).collect::<Vec<_>>(), vec![7, 3806, 3828]);
        assert!(sets.get(1).is_none());
        // Fiche hors du fichier de blocs : jeu sans son.
        let mut short = 1u32.to_le_bytes().to_vec();
        short.extend(set_record(9, "x", 10_000, 196));
        assert!(EffectSets::parse(&short, &var).get(9).unwrap().sounds.is_empty());
        assert!(EffectSets::parse(&[], &[]).is_empty());
    }
}
