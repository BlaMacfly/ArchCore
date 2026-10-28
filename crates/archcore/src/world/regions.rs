//! Régions et points de résurrection.
//!
//! Chaque segment de la carte serveur porte un numéro de région (RegionTemplate.ini), dont le champ Type
//! dit si elle est sûre (bits 2-3 à 0), libre ou dangereuse. Un joueur est rattaché à la dernière région
//! sûre qu'il a traversée ; à l'entrée en jeu, à celle où il se trouve si elle est sûre, sinon à celle de sa
//! race (Anchorville, Golundo, Norine). Il ressuscite, ou rentre par parchemin, au point de résurrection de
//! cette région le plus proche de lui (objets à événement 19 « binding » de type 1 des obj*.ini), parmi ceux
//! que sa race et sa classe autorisent.

use crate::db::{col, Databases};
use crate::info;
use archcore_net::Vec3;
use std::collections::HashMap;

/// Sûreté d'une région (bits 2-3 du champ Type).
pub const SAFE: i64 = 0;

/// Région de rattachement par défaut, selon la race (1 humain, 2 orc, 3 elfe de lune).
pub fn default_region(race: i32) -> u16 {
    match race {
        2 => 5,
        3 => 26,
        _ => 9,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResurrectionPoint {
    pub pos: Vec3,
    /// Race et classe exigées (0 : toutes).
    pub race: i64,
    pub class: i64,
}

#[derive(Default)]
pub struct Regions {
    safety: HashMap<u16, i64>,
    /// Champ Type de chaque région (bits 0x10 : montures permises).
    type_bits: HashMap<u16, u32>,
    points: HashMap<u16, Vec<ResurrectionPoint>>,
    /// Région parente (ParentIndex), quand elle en a une autre qu'elle-même.
    parents: HashMap<u16, u16>,
    /// Niveaux d'entrée (LevelMin, LevelLimit ; 0 : aucun) des régions qui en ont : étages de la Battle Square.
    levels: HashMap<u16, (i32, i32)>,
}

impl Regions {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let mut r = Regions::default();
        let Ok(rows) = db.world.rows("SELECT id, parent_id, safety, type_bits, level_min, level_limit FROM region_template", ()).await else {
            crate::warn!("régions non chargées : lancer `archcore db-init` puis `archcore import`");
            return Ok(r);
        };
        for row in &rows {
            r.safety.insert(col::<u32>(row, "id") as u16, col(row, "safety"));
            let id = col::<u32>(row, "id") as u16;
            r.type_bits.insert(id, col(row, "type_bits"));
            let parent: i64 = col(row, "parent_id");
            if parent > 0 && parent != id as i64 {
                r.parents.insert(id, parent as u16);
            }
            let (min, limit): (i32, i32) = (col(row, "level_min"), col(row, "level_limit"));
            if min > 0 || limit > 0 {
                r.levels.insert(id, (min, limit));
            }
        }
        for row in db.world.rows("SELECT region_id, pos_x, pos_y, pos_z, race, class FROM resurrection_point", ()).await? {
            let pos = Vec3::new(col::<f64>(&row, "pos_x") as f32, col::<f64>(&row, "pos_y") as f32, col::<f64>(&row, "pos_z") as f32);
            r.points.entry(col::<u32>(&row, "region_id") as u16).or_default().push(ResurrectionPoint { pos, race: col(&row, "race"), class: col(&row, "class") });
        }
        let safe = r.safety.values().filter(|s| **s == SAFE).count();
        info!("régions : {} ({safe} sûre(s)), {} point(s) de résurrection", r.safety.len(), r.points.values().map(Vec::len).sum::<usize>());
        Ok(r)
    }

    #[cfg(test)]
    pub fn insert(&mut self, region: u16, safety: i64, points: Vec<ResurrectionPoint>) {
        self.safety.insert(region, safety);
        if !points.is_empty() {
            self.points.insert(region, points);
        }
    }

    /// Sûreté d'une région : 0 sûre (aussi pour une région inconnue), 1 libre, 2 combat.
    pub fn safety(&self, region: u16) -> i64 {
        self.safety.get(&region).copied().unwrap_or(SAFE)
    }

    pub fn is_safe(&self, region: u16) -> bool {
        self.safety.get(&region) == Some(&SAFE)
    }

    #[cfg(test)]
    pub fn insert_levels(&mut self, region: u16, parent: Option<u16>, levels: Option<(i32, i32)>) {
        if let Some(p) = parent {
            self.parents.insert(region, p);
        }
        if let Some(l) = levels {
            self.levels.insert(region, l);
        }
    }

    /// Région parente, si elle en a une autre qu'elle-même.
    pub fn parent(&self, region: u16) -> Option<u16> {
        self.parents.get(&region).copied()
    }

    /// Niveaux d'entrée (minimum, limite ; 0 : aucun) de la région ou, à défaut, de sa parente.
    pub fn level_range(&self, region: u16) -> Option<(i32, i32)> {
        self.levels.get(&region).or_else(|| self.parent(region).and_then(|p| self.levels.get(&p))).copied()
    }

    /// Champ Type d'une région connue.
    pub fn type_bits(&self, region: u16) -> Option<u32> {
        self.type_bits.get(&region).copied()
    }

    /// Point de résurrection de la région le plus proche de `from`, permis à cette race, quelle que soit la classe : les
    /// points des zones de race (forts du Battleground, zones des Shrines) portent la race aussi dans leur colonne de
    /// classe, qui n'y désigne donc pas une classe.
    pub fn race_point(&self, region: u16, from: Vec3, race: i64) -> Option<Vec3> {
        let d = |p: &ResurrectionPoint| ((p.pos.x - from.x) as f64).powi(2) + ((p.pos.z - from.z) as f64).powi(2);
        self.points.get(&region)?.iter().filter(|p| p.race == 0 || p.race == race).min_by(|a, b| d(a).total_cmp(&d(b))).map(|p| p.pos)
    }

    /// Point de résurrection de la région le plus proche de `from`, permis à cette race et cette classe.
    pub fn resurrection(&self, region: u16, from: Vec3, race: i64, class: i64) -> Option<Vec3> {
        let allowed = |p: &&ResurrectionPoint| (p.race == 0 || p.race == race) && (p.class == 0 || p.class == class);
        let d = |p: &ResurrectionPoint| ((p.pos.x - from.x) as f64).powi(2) + ((p.pos.z - from.z) as f64).powi(2);
        self.points.get(&region)?.iter().filter(allowed).min_by(|a, b| d(a).total_cmp(&d(b))).map(|p| p.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_le_plus_proche_et_permis() {
        let mut r = Regions::default();
        let p = |x: f32, race: i64| ResurrectionPoint { pos: Vec3::new(x, 0.0, 0.0), race, class: 0 };
        r.insert(9, SAFE, vec![p(0.0, 0), p(1000.0, 2), p(5000.0, 1)]);
        r.insert(40, 2, vec![]);
        assert!(r.is_safe(9) && !r.is_safe(40) && !r.is_safe(77));
        assert_eq!(r.resurrection(9, Vec3::new(900.0, 0.0, 0.0), 1, 1), Some(Vec3::new(0.0, 0.0, 0.0)), "le point orc est exclu");
        assert_eq!(r.resurrection(9, Vec3::new(900.0, 0.0, 0.0), 2, 1), Some(Vec3::new(1000.0, 0.0, 0.0)));
        assert_eq!(r.resurrection(40, Vec3::new(0.0, 0.0, 0.0), 1, 1), None);
        // Zone de race : la colonne de classe répète la race.
        r.insert(245, 2, vec![ResurrectionPoint { pos: Vec3::new(7.0, 0.0, 0.0), race: 2, class: 2 }]);
        assert_eq!((r.resurrection(245, Vec3::default(), 2, 1), r.race_point(245, Vec3::default(), 2)), (None, Some(Vec3::new(7.0, 0.0, 0.0))));
        assert_eq!(r.race_point(245, Vec3::default(), 1), None);
        // Niveaux d'entrée : ceux de la région, sinon de sa parente (étages de la Battle Square).
        r.insert_levels(198, None, Some((20, 30)));
        r.insert_levels(195, Some(198), None);
        assert_eq!((r.level_range(195), r.level_range(198), r.level_range(9), r.parent(195)), (Some((20, 30)), Some((20, 30)), None, Some(198)));
        assert_eq!((default_region(1), default_region(2), default_region(3)), (9, 5, 26));
    }
}
