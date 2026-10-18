//! Conteneur des animations `.ean` observées dans les packs Archonia.
//!
//! Les fichiers sont des suites de blocs RenderWare. Dans le pack analysé, les blocs `0x1b`
//! contiennent un préfixe de 20 octets suivi d'enregistrements de 36 octets. Leur disposition
//! correspond aux clés HAnim sérialisées ; le mot de liaison reste opaque jusqu'à validation de
//! ses valeurs dans le runtime Archonia.

use crate::rw::{Reader, RwError};
use glam::Quat;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct EanFile {
    pub chunks: Vec<EanChunk>,
}

#[derive(Debug, Clone)]
pub struct EanChunk {
    pub id: u32,
    pub version: u32,
    /// Octets du contenu tels qu'ils apparaissent dans le fichier.
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimationLayout {
    /// Les cinq mots initiaux restent opaques jusqu'à validation par le binaire.
    pub prefix: [u32; 5],
    pub key_count: usize,
}

/// Disposition sérialisée des clés HAnim : temps, quaternion, translation, puis lien précédent.
/// Le lien est gardé brut : les valeurs observées ne sont pas des indices de tableau.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HAnimKey {
    pub previous: u32,
    pub time: f32,
    /// Quaternion RenderWare (x, y, z, w).
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HAnimSample {
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}

impl EanChunk {
    /// Reconnaît uniquement la disposition vérifiée par la taille du bloc.
    pub fn animation_layout(&self) -> Option<AnimationLayout> {
        if self.id != 0x1b || self.payload.len() < 20 {
            return None;
        }
        let mut prefix = [0; 5];
        for (i, word) in prefix.iter_mut().enumerate() {
            *word = u32::from_le_bytes(self.payload[i * 4..i * 4 + 4].try_into().ok()?);
        }
        let key_bytes = self.payload.len() - 20;
        if key_bytes % 36 != 0 {
            return None;
        }
        Some(AnimationLayout {
            prefix,
            key_count: key_bytes / 36,
        })
    }

    /// Itère sur les enregistrements sans attribuer de sens aux champs inconnus.
    pub fn raw_keys(&self) -> Option<impl Iterator<Item = &'_ [u8; 36]>> {
        self.animation_layout()?;
        let keys = self.payload[20..].chunks_exact(36);
        Some(keys.map(|key| <&[u8; 36]>::try_from(key).expect("chunks_exact garantit 36 octets")))
    }

    pub fn h_anim_keys(&self) -> Option<impl Iterator<Item = HAnimKey> + '_> {
        let keys = self.raw_keys()?;
        Some(keys.map(|raw| {
            let word =
                |offset: usize| u32::from_le_bytes(raw[offset..offset + 4].try_into().unwrap());
            let float = |offset: usize| f32::from_bits(word(offset));
            HAnimKey {
                time: float(0),
                rotation: [float(4), float(8), float(12), float(16)],
                translation: [float(20), float(24), float(28)],
                previous: word(32),
            }
        }))
    }

    /// Regroupe les clés dont le mot final référence une clé antérieure par son décalage en octets.
    /// Les références externes ou non alignées commencent une nouvelle piste et restent opaques.
    pub fn linked_tracks(&self) -> Option<Vec<Vec<usize>>> {
        let keys: Vec<_> = self.h_anim_keys()?.collect();
        let mut roots = Vec::with_capacity(keys.len());
        let mut tracks = BTreeMap::<usize, Vec<usize>>::new();
        for (index, key) in keys.iter().enumerate() {
            let previous = key.previous as usize;
            let root = if previous % 36 == 0 && previous < keys.len() * 36 {
                let previous_index = previous / 36;
                if previous_index < index {
                    roots[previous_index]
                } else {
                    index
                }
            } else {
                index
            };
            roots.push(root);
            tracks.entry(root).or_default().push(index);
        }
        Some(tracks.into_values().collect())
    }

    /// Échantillonne chaque piste candidate à `time` (secondes), avec interpolation linéaire et
    /// slerp. Les clés hors intervalle sont tenues en pose extrême ; la boucle est au choix appelant.
    pub fn sample_linked_tracks(&self, time: f32) -> Option<Vec<HAnimSample>> {
        if !time.is_finite() {
            return None;
        }
        let keys: Vec<_> = self.h_anim_keys()?.collect();
        let tracks = self.linked_tracks()?;
        Some(
            tracks
                .iter()
                .map(|track| {
                    let mut ordered = track.clone();
                    ordered.sort_by(|a, b| keys[*a].time.total_cmp(&keys[*b].time).then(a.cmp(b)));
                    let upper = ordered.partition_point(|&index| keys[index].time <= time);
                    if upper == 0 {
                        return sample_from_key(keys[ordered[0]]);
                    }
                    let left = keys[ordered[upper - 1]];
                    if upper == ordered.len() {
                        return sample_from_key(left);
                    }
                    let right = keys[ordered[upper]];
                    let span = right.time - left.time;
                    if span <= 0.0 || !span.is_finite() {
                        return sample_from_key(left);
                    }
                    let t = ((time - left.time) / span).clamp(0.0, 1.0);
                    let a = normalized_quat(left.rotation);
                    let b = normalized_quat(right.rotation);
                    let q = a.slerp(b, t).normalize();
                    HAnimSample {
                        rotation: q.to_array(),
                        translation: [
                            left.translation[0] + (right.translation[0] - left.translation[0]) * t,
                            left.translation[1] + (right.translation[1] - left.translation[1]) * t,
                            left.translation[2] + (right.translation[2] - left.translation[2]) * t,
                        ],
                    }
                })
                .collect(),
        )
    }
}

fn normalized_quat(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(q[0], q[1], q[2], q[3]);
    if q.is_finite() && q.length_squared() > 1e-12 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

fn sample_from_key(key: HAnimKey) -> HAnimSample {
    HAnimSample {
        rotation: normalized_quat(key.rotation).to_array(),
        translation: key.translation,
    }
}

impl EanFile {
    pub fn parse(bytes: &[u8]) -> Result<Self, RwError> {
        let mut reader = Reader::new(bytes);
        let mut chunks = Vec::new();
        while reader.remaining() != 0 {
            if reader.remaining() < 12 {
                return Err(RwError::Truncated("en-tête de bloc EAN"));
            }
            let chunk = reader.chunk()?;
            let payload = reader.bytes(chunk.size)?.to_vec();
            chunks.push(EanChunk {
                id: chunk.id,
                version: chunk.version,
                payload,
            });
        }
        Ok(Self { chunks })
    }
}
