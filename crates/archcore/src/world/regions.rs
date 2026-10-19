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
    points: HashMap<u16, Vec<ResurrectionPoint>>,
}

impl Regions {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let mut r = Regions::default();
        let Ok(rows) = db.world.rows("SELECT id, safety FROM region_template", ()).await else {
            crate::warn!("régions non chargées : lancer `archcore db-init` puis `archcore import`");
            return Ok(r);
        };
        for row in &rows {
            r.safety.insert(col::<u32>(row, "id") as u16, col(row, "safety"));
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

    pub fn is_safe(&self, region: u16) -> bool {
        self.safety.get(&region) == Some(&SAFE)
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
        assert_eq!((default_region(1), default_region(2), default_region(3)), (9, 5, 26));
    }
}
