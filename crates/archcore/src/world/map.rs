//! Carte serveur du monde (`archcore_world.map_segment`, importée de `node_data`).
//!
//! Le monde est découpé en 800 × 800 secteurs de 6400 unités à partir de (-2 560 000, -2 560 000), chacun en
//! 16 × 16 segments de 400 unités. Un segment tient sur 4 octets : type de case, blocage (bit 0 : sol
//! infranchissable, bit 1 : ciel) et numéro de région (RegionTemplate.ini). Un secteur absent n'est pas
//! bloquant.

use crate::db::{col, Databases};
use crate::info;
use std::collections::HashMap;

pub const SECTOR_SIZE: f64 = 6400.0;
pub const SEGMENT_SIZE: f64 = 400.0;
pub const SECTOR_COUNT: i64 = 800;
pub const SEGMENTS: usize = 16;
pub const WORLD_START: f64 = -(SECTOR_COUNT as f64 / 2.0) * SECTOR_SIZE;
/// Bit « sol infranchissable » de l'octet de blocage.
pub const BLOCK_GROUND: u32 = 1;

#[derive(Default)]
pub struct WorldMap {
    sectors: HashMap<(u16, u16), Box<[u32; SEGMENTS * SEGMENTS]>>,
}

/// Secteur et segment d'une position X/Z, hors du monde → `None`.
pub fn locate(x: f64, z: f64) -> Option<((u16, u16), usize)> {
    let (fx, fz) = ((x - WORLD_START) / SECTOR_SIZE, (z - WORLD_START) / SECTOR_SIZE);
    if !(0.0..SECTOR_COUNT as f64).contains(&fx) || !(0.0..SECTOR_COUNT as f64).contains(&fz) {
        return None;
    }
    let (sx, sz) = (fx.floor(), fz.floor());
    let gx = (((x - WORLD_START) - sx * SECTOR_SIZE) / SEGMENT_SIZE).floor() as usize;
    let gz = (((z - WORLD_START) - sz * SECTOR_SIZE) / SEGMENT_SIZE).floor() as usize;
    Some(((sx as u16, sz as u16), gz.min(SEGMENTS - 1) * SEGMENTS + gx.min(SEGMENTS - 1)))
}

impl WorldMap {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let rows = match db.world.rows("SELECT sector_x, sector_z, segments FROM map_segment", ()).await {
            Ok(rows) => rows,
            Err(e) => {
                crate::warn!("carte du monde non chargée ({e:#}) : lancer `archcore db-init` puis `archcore import --server …`");
                Vec::new()
            }
        };
        let mut map = WorldMap::default();
        for r in &rows {
            map.insert(col::<u16>(r, "sector_x"), col::<u16>(r, "sector_z"), &col::<Vec<u8>>(r, "segments"));
        }
        info!("carte du monde : {} secteur(s)", map.sectors.len());
        Ok(map)
    }

    /// Ajoute un secteur à partir de ses 1024 octets (256 segments u32 LE, ligne Z puis colonne X).
    pub fn insert(&mut self, sx: u16, sz: u16, bytes: &[u8]) {
        let mut segs = Box::new([0u32; SEGMENTS * SEGMENTS]);
        for (i, c) in bytes.chunks_exact(4).take(SEGMENTS * SEGMENTS).enumerate() {
            segs[i] = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        self.sectors.insert((sx, sz), segs);
    }

    pub fn segment(&self, x: f64, z: f64) -> Option<u32> {
        let (sector, i) = locate(x, z)?;
        self.sectors.get(&sector).map(|s| s[i])
    }

    /// Sol infranchissable (ou hors du monde).
    pub fn is_blocked(&self, x: f64, z: f64) -> bool {
        match locate(x, z) {
            None => true,
            Some((sector, i)) => self.sectors.get(&sector).is_some_and(|s| (s[i] >> 8) & BLOCK_GROUND != 0),
        }
    }

    /// Région (RegionTemplate.ini) du segment, s'il est connu.
    pub fn region(&self, x: f64, z: f64) -> Option<u16> {
        self.segment(x, z).map(|v| (v >> 16) as u16)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoupage_en_secteurs_et_segments() {
        // Départ d'Anchorville : secteur (325, 389).
        let ((sx, sz), i) = locate(-475955.0, -66219.0).unwrap();
        assert_eq!((sx, sz), (325, 389));
        let (gx, gz) = (i % SEGMENTS, i / SEGMENTS);
        assert_eq!((gx, gz), (((-475955.0 - WORLD_START) - 325.0 * SECTOR_SIZE) as usize / 400, ((-66219.0 - WORLD_START) - 389.0 * SECTOR_SIZE) as usize / 400));
        assert!(locate(WORLD_START - 1.0, 0.0).is_none());
        assert!(locate(0.0, -WORLD_START).is_none());
    }

    #[test]
    fn blocage_et_region() {
        let mut map = WorldMap::default();
        let mut bytes = vec![0u8; 1024];
        // Segment (gx 1, gz 0) : sol bloqué, région 9 ; segment (gx 0, gz 1) : libre, région 7.
        bytes[4..8].copy_from_slice(&(0x0009_0100u32).to_le_bytes());
        bytes[64..68].copy_from_slice(&(0x0007_0000u32).to_le_bytes());
        map.insert(400, 400, &bytes);
        assert!(map.is_blocked(450.0, 10.0));
        assert_eq!(map.region(450.0, 10.0), Some(9));
        assert!(!map.is_blocked(10.0, 450.0));
        assert_eq!(map.region(10.0, 450.0), Some(7));
        assert!(!map.is_blocked(10_000.0, 10.0), "secteur absent : libre");
        assert!(map.is_blocked(-3_000_000.0, 0.0), "hors du monde : bloqué");
    }
}
