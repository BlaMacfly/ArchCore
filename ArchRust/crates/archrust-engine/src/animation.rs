//! Animation squelettique des personnages, d'après le client Archonia.
//!
//! **Choix des animations** — `ini/charactertemplateanimation.ini`, lu par Archonia.exe dans
//! `FUN_14050b1b0` (sha256 e9c7609b…) : chaque section `[TID]` est positionnelle (l'en-tête `[]`
//! nomme les colonnes, ex. `2=ANIMATION_NAME000`) et chaque valeur `ANIMATION_NAME…` vaut
//! `type:équipement:variante:fichier` (`sscanf "%d:%d:%d:%s"`). Le client range le fichier dans
//! une table `[type][équipement]`, la variante désignant une entrée de la liste ;
//! `ANIMATION_ACTIVE_RATE…` (`%d:%d:%d:%d`) donne le taux de tirage d'une variante. L'équipement est
//! converti par `FUN_1404addf0` selon `ANIMATION_TYPE2` ; 0 = sans arme. Les types 1 et 2 reçoivent
//! d'office le drapeau 1 (boucle).
//!
//! Sens des types, **déduit des données** (amplitude des clips de TID 96) : 0 = attente
//! (`a0060000` respire, 4° au plus ; `a0060002`, tiré à 20 %, est une attente animée de 7 s),
//! 1 = marche, 2 = course (`a0060200`, cycle de 0,8 s, membres jusqu'à 100°), 3 = attaque
//! (avec `ANIMATION_POINT`), 4 = coup reçu, 5 = mort.
//!
//! **Pose** — un clip `.ean` contient des clés HAnim standard (préfixe : version 0x100, type 1,
//! nombre de clés, drapeaux, durée en secondes). Les `n` premières clés ouvrent les pistes des `n`
//! nœuds de la hiérarchie, dans l'ordre du tableau de nœuds ; chaque clé suivante désigne la
//! précédente de sa piste par décalage en octets (`indice × 36`). Le quaternion s'applique tel quel
//! (convention glam), vérifié par la déformation : le conjugué écarte le torse de 140 unités.
//! La déformation suit RpSkin : voir `Clump::skin_matrices`.

use crate::assets::Assets;
use crate::render::{Mesh, Renderer, Vertex};
use archrust_data::ean::{EanFile, HAnimSample};
use archrust_data::rw::Clump;
use glam::{Mat3, Quat, Vec3};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Type d'animation d'Archonia (premier nombre de `ANIMATION_NAME`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimType {
    Wait = 0,
    Walk = 1,
    Run = 2,
    Attack = 3,
    Hit = 4,
    Death = 5,
}

/// Géométrie skinnée d'un clump du client, liée à son maillage GPU.
pub struct SkinBinding {
    pub clump: Arc<Clump>,
    pub geometry: usize,
    /// Sommets de la pose de bind (espace de la frame de l'atomic).
    pub base: Vec<Vertex>,
}

/// Clip `.ean` prêt à échantillonner : pistes triées par temps, une par nœud HAnim.
pub struct Clip {
    pub name: String,
    pub duration: f32,
    tracks: Vec<Vec<(f32, Quat, Vec3)>>,
}

impl Clip {
    pub fn from_ean(name: &str, ean: &EanFile) -> Option<Clip> {
        let chunk = ean.chunks.iter().find(|c| c.id == 0x1b)?;
        let layout = chunk.animation_layout()?;
        let keys: Vec<_> = chunk.h_anim_keys()?.collect();
        let mut tracks: Vec<Vec<(f32, Quat, Vec3)>> = chunk
            .linked_tracks()?
            .into_iter()
            .map(|t| {
                t.into_iter()
                    .map(|i| {
                        let k = keys[i];
                        let q = Quat::from_array(k.rotation);
                        let q = if q.is_finite() && q.length_squared() > 1e-12 {
                            q.normalize()
                        } else {
                            Quat::IDENTITY
                        };
                        (k.time, q, Vec3::from_array(k.translation))
                    })
                    .collect()
            })
            .collect();
        for t in &mut tracks {
            t.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let declared = f32::from_bits(layout.prefix[4]);
        let last = keys
            .iter()
            .map(|k| k.time)
            .filter(|t| t.is_finite())
            .fold(0.0, f32::max);
        let duration = if declared.is_finite() && declared > 0.0 {
            declared
        } else {
            last
        };
        Some(Clip {
            name: name.to_string(),
            duration: duration.max(1e-3),
            tracks,
        })
    }

    /// Pose à l'instant `time` (secondes, ramené dans la durée du clip).
    pub fn sample(&self, time: f32) -> Vec<HAnimSample> {
        let t = time.rem_euclid(self.duration);
        self.tracks
            .iter()
            .map(|track| {
                let upper = track.partition_point(|k| k.0 <= t);
                let (q, p) = if upper == 0 {
                    (track[0].1, track[0].2)
                } else if upper == track.len() {
                    (track[upper - 1].1, track[upper - 1].2)
                } else {
                    let (a, b) = (track[upper - 1], track[upper]);
                    let span = b.0 - a.0;
                    let f = if span > 0.0 {
                        ((t - a.0) / span).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    (a.1.slerp(b.1, f).normalize(), a.2.lerp(b.2, f))
                };
                HAnimSample {
                    rotation: q.to_array(),
                    translation: p.to_array(),
                }
            })
            .collect()
    }
}

/// Variante d'animation : fichier et taux de tirage (`ANIMATION_ACTIVE_RATE`, 0 si absent).
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    pub file: String,
    pub rate: u32,
}

/// Animations d'un TID, sans arme, par type.
#[derive(Debug, Clone, Default)]
pub struct AnimationSet {
    pub by_type: HashMap<u32, Vec<Variant>>,
}

impl AnimationSet {
    /// Lit la section `[tid]` de `charactertemplateanimation.ini` (équipement 0 uniquement).
    pub fn for_tid(assets: &Assets, tid: i32) -> AnimationSet {
        let mut set = AnimationSet::default();
        let Ok(table) = assets.table("ini", "charactertemplateanimation.ini") else {
            return set;
        };
        let (Some(header), Some(section)) = (table.section(""), table.section(&tid.to_string()))
        else {
            return set;
        };
        let column = |key: &str| header.get(key).unwrap_or("");
        let mut rates: HashMap<(u32, u32), u32> = HashMap::new();
        let mut names: Vec<(u32, u32, String)> = Vec::new();
        for (key, value) in &section.entries {
            let col = column(key);
            let mut it = value.splitn(4, ':');
            let (Some(t), Some(e), Some(v), Some(rest)) =
                (it.next(), it.next(), it.next(), it.next())
            else {
                continue;
            };
            let (Ok(t), Ok(e), Ok(v)) = (
                t.trim().parse::<u32>(),
                e.trim().parse::<u32>(),
                v.trim().parse::<u32>(),
            ) else {
                continue;
            };
            if e != 0 {
                continue;
            }
            if col.starts_with("ANIMATION_NAME") {
                if !rest.trim().is_empty() && rest.trim() != "0" {
                    names.push((t, v, rest.trim().to_ascii_lowercase()));
                }
            } else if col.starts_with("ANIMATION_ACTIVE_RATE") {
                if let Ok(r) = rest.trim().parse::<u32>() {
                    rates.insert((t, v), r);
                }
            }
        }
        names.sort();
        for (t, v, file) in names {
            let rate = rates.get(&(t, v)).copied().unwrap_or(0);
            set.by_type.entry(t).or_default().push(Variant {
                file: format!("{file}.ean"),
                rate,
            });
        }
        set
    }

    /// Variante à jouer pour un tirage `roll` ∈ [0, 1) : pondérée par les taux, sinon la première.
    pub fn pick(&self, kind: u32, roll: f32) -> Option<&Variant> {
        let list = self.by_type.get(&kind)?;
        let total: u32 = list.iter().map(|v| v.rate).sum();
        if total == 0 {
            return list.first();
        }
        let mut target = roll.clamp(0.0, 0.999_999) * total as f32;
        for v in list {
            if v.rate == 0 {
                continue;
            }
            if target < v.rate as f32 {
                return Some(v);
            }
            target -= v.rate as f32;
        }
        list.iter().rev().find(|v| v.rate > 0)
    }
}

fn clip_cache() -> &'static Mutex<HashMap<String, Option<Arc<Clip>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<Clip>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Clip `character\animation\<file>`, analysé une fois puis gardé en cache.
pub fn load_clip(assets: &Assets, file: &str) -> Option<Arc<Clip>> {
    if let Some(c) = clip_cache().lock().unwrap().get(file) {
        return c.clone();
    }
    let clip = match assets
        .read_packed("character/animation", file)
        .map_err(anyhow::Error::from)
        .and_then(|b| EanFile::parse(&b).map_err(anyhow::Error::from))
    {
        Ok(ean) => Clip::from_ean(file, &ean).map(Arc::new),
        Err(e) => {
            log::debug!("animation {file} : {e:#}");
            None
        }
    };
    clip_cache()
        .lock()
        .unwrap()
        .insert(file.to_string(), clip.clone());
    clip
}

/// Animation d'une entité : maillages skinnés, clips du TID et état courant.
pub struct Animator {
    parts: Vec<(Arc<Mesh>, Arc<SkinBinding>)>,
    set: AnimationSet,
    kind: u32,
    clip: Option<Arc<Clip>>,
    time: f32,
    since_update: f32,
    seed: u32,
    /// Clip ponctuel en cours (attaque, coup reçu) : joué une fois, puis retour à l'attente ou à la
    /// course.
    action: bool,
    /// Mort : le clip de mort est joué une fois puis sa dernière pose est gardée.
    dead: bool,
    /// Identité des maillages animés, pour reconstruire l'animateur s'ils changent.
    pub signature: usize,
}

impl Animator {
    /// Animateur d'une entité, ou `None` si aucun de ses maillages n'est skinné.
    pub fn new(
        assets: &Assets,
        tid: i32,
        meshes: &[(Arc<Mesh>, glam::Mat4)],
        seed: u32,
    ) -> Option<Animator> {
        let parts: Vec<_> = meshes
            .iter()
            .filter_map(|(m, _)| m.skin.clone().map(|s| (m.clone(), s)))
            .collect();
        if parts.is_empty() {
            return None;
        }
        let set = AnimationSet::for_tid(assets, tid);
        if set.by_type.is_empty() {
            return None;
        }
        let signature = signature(meshes);
        let mut a = Animator {
            parts,
            set,
            kind: u32::MAX,
            clip: None,
            time: 0.0,
            since_update: f32::MAX,
            seed: seed.max(1),
            action: false,
            dead: false,
            signature,
        };
        a.switch(assets, AnimType::Wait as u32);
        Some(a)
    }

    fn roll(&mut self) -> f32 {
        // xorshift32 : tirage des variantes, déterministe par entité.
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn switch(&mut self, assets: &Assets, kind: u32) {
        // Sans course, la marche ; sans marche, l'attente (le TID 96 n'a pas de type 1).
        let kind = [
            kind,
            AnimType::Walk as u32,
            AnimType::Run as u32,
            AnimType::Wait as u32,
        ]
        .into_iter()
        .find(|k| self.set.by_type.contains_key(k))
        .unwrap_or(kind);
        if kind == self.kind {
            return;
        }
        self.kind = kind;
        let roll = self.roll();
        self.clip = self
            .set
            .pick(kind, roll)
            .and_then(|v| load_clip(assets, &v.file));
        self.time = 0.0;
        self.since_update = f32::MAX;
    }

    /// Avance l'animation de `dt` et réécrit les sommets si nécessaire (au plus `max_hz` fois par
    /// seconde). `moving` choisit la course, sinon l'attente.
    pub fn update(
        &mut self,
        assets: &Assets,
        renderer: &Renderer,
        dt: f32,
        moving: bool,
        max_hz: f32,
    ) {
        let wanted = if moving {
            AnimType::Run
        } else {
            AnimType::Wait
        } as u32;
        // Un clip ponctuel se termine à sa fin, ou dès que le personnage se remet en marche.
        if self.action
            && !self.dead
            && (moving || self.clip.as_ref().is_none_or(|c| self.time >= c.duration - 2e-3))
        {
            self.action = false;
            self.kind = u32::MAX;
        }
        if !self.action && !self.dead {
            self.switch(assets, wanted);
        }
        let Some(clip) = self.clip.clone() else {
            return;
        };
        self.time += dt;
        if self.dead || self.action {
            // Pose finale gardée (mort) ou clip joué une seule fois (attaque, coup reçu).
            self.time = self.time.min(clip.duration - 1e-3).max(0.0);
        }
        if self.time >= clip.duration && self.kind == AnimType::Wait as u32 {
            // Fin d'un cycle d'attente : nouveau tirage parmi les variantes, comme les taux le prévoient.
            self.time -= clip.duration;
            let roll = self.roll();
            if let Some(next) = self
                .set
                .pick(self.kind, roll)
                .and_then(|v| load_clip(assets, &v.file))
            {
                self.clip = Some(next);
            }
        }
        self.since_update += dt;
        if self.since_update < 1.0 / max_hz {
            return;
        }
        self.since_update = 0.0;
        let Some(clip) = self.clip.clone() else {
            return;
        };
        let pose = clip.sample(self.time);
        for (mesh, binding) in &self.parts {
            if let Some(vertices) = deform(binding, &pose) {
                renderer.write_vertices(mesh, &vertices);
            }
        }
    }

    /// Joue une fois un clip du type `kind` (3 attaque, 4 coup reçu), s'il existe pour ce modèle.
    /// Un coup reçu n'interrompt pas une attaque en cours ; rien ne se joue sur un mort.
    pub fn play_once(&mut self, assets: &Assets, kind: AnimType) -> bool {
        let kind = kind as u32;
        if self.dead || !self.set.by_type.contains_key(&kind) {
            return false;
        }
        if kind == AnimType::Hit as u32 && self.action && self.kind == AnimType::Attack as u32 {
            return false;
        }
        let roll = self.roll();
        let Some(clip) = self.set.pick(kind, roll).and_then(|v| load_clip(assets, &v.file)) else {
            return false;
        };
        // Plusieurs variantes d'attaque sans taux : on les alterne d'un coup à l'autre.
        let clip = match self.set.by_type.get(&kind) {
            Some(list) if list.len() > 1 && list.iter().all(|v| v.rate == 0) => list
                .get((roll * list.len() as f32) as usize % list.len())
                .and_then(|v| load_clip(assets, &v.file))
                .unwrap_or(clip),
            _ => clip,
        };
        self.kind = kind;
        self.clip = Some(clip);
        self.time = 0.0;
        self.since_update = f32::MAX;
        self.action = true;
        true
    }

    /// Mort (clip de type 5 joué une fois, dernière pose gardée) ou retour à la vie. `at_end` pose
    /// directement la fin du clip (cadavre déjà à terre quand il entre dans la vue).
    pub fn set_dead(&mut self, assets: &Assets, dead: bool, at_end: bool) {
        if dead == self.dead {
            return;
        }
        self.dead = dead;
        self.action = false;
        self.since_update = f32::MAX;
        if dead {
            let kind = AnimType::Death as u32;
            let roll = self.roll();
            if let Some(clip) = self.set.pick(kind, roll).and_then(|v| load_clip(assets, &v.file)) {
                self.time = if at_end { clip.duration } else { 0.0 };
                self.clip = Some(clip);
                self.kind = kind;
            }
        } else {
            self.kind = u32::MAX;
            self.switch(assets, AnimType::Wait as u32);
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    pub fn clip_name(&self) -> Option<&str> {
        self.clip.as_ref().map(|c| c.name.as_str())
    }
}

/// Identité d'un ensemble de maillages (adresses des `Arc`).
pub fn signature(meshes: &[(Arc<Mesh>, glam::Mat4)]) -> usize {
    meshes.iter().fold(meshes.len(), |acc, (m, _)| {
        acc.wrapping_mul(31).wrapping_add(Arc::as_ptr(m) as usize)
    })
}

/// Sommets d'une géométrie skinnée dans une pose : positions et normales mélangées par os.
pub fn deform(binding: &SkinBinding, pose: &[HAnimSample]) -> Option<Vec<Vertex>> {
    let geometry = binding.clump.geometries.get(binding.geometry)?;
    let skin = geometry.skin.as_ref()?;
    let matrices = binding.clump.skin_matrices(binding.geometry, pose).ok()?;
    if skin.indices.len() != binding.base.len() {
        return None;
    }
    let normals: Vec<Mat3> = matrices.iter().map(|m| Mat3::from_mat4(*m)).collect();
    let mut out = binding.base.clone();
    for (i, v) in out.iter_mut().enumerate() {
        let p = Vec3::from_array(binding.base[i].position);
        let n = Vec3::from_array(binding.base[i].normal);
        let (mut pp, mut nn) = (Vec3::ZERO, Vec3::ZERO);
        let mut total = 0.0;
        for lane in 0..4 {
            let w = skin.weights[i][lane];
            if w == 0.0 {
                continue;
            }
            let b = skin.indices[i][lane] as usize;
            let (Some(m), Some(r)) = (matrices.get(b), normals.get(b)) else {
                continue;
            };
            pp += m.transform_point3(p) * w;
            nn += (*r * n) * w;
            total += w;
        }
        if total > 0.0 {
            v.position = pp.to_array();
            v.normal = nn.normalize_or_zero().to_array();
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_weighted_by_rate() {
        let mut set = AnimationSet::default();
        set.by_type.insert(
            0,
            vec![
                Variant {
                    file: "a0060000.ean".into(),
                    rate: 80,
                },
                Variant {
                    file: "a0060001.ean".into(),
                    rate: 0,
                },
                Variant {
                    file: "a0060002.ean".into(),
                    rate: 20,
                },
            ],
        );
        assert_eq!(set.pick(0, 0.0).unwrap().file, "a0060000.ean");
        assert_eq!(set.pick(0, 0.79).unwrap().file, "a0060000.ean");
        assert_eq!(set.pick(0, 0.81).unwrap().file, "a0060002.ean");
        assert!(set.pick(2, 0.5).is_none());
    }
}
