//! Zones du monde : région d'une position et musique de la région.
//!
//! Chaîne reconstituée sur les données du client :
//!
//! 1. la grille d'attributs du secteur `C{sx},{sy}.amf` (archive `world/a%06dx.ma1`, 16 octets
//!    d'en-tête puis 16 × 16 `u32`) porte dans les 16 bits hauts de chaque cellule l'indice de la
//!    région (`RegionTemplate.ini`) ; les 16 bits bas sont les attributs de cellule (collision…).
//!    Vérifié : le secteur d'Anchorville (`C325,389`) ne contient que des cellules `0x0009xxxx`
//!    et la région 9 est « 앵커빌 » (Anchorville) ; la salle du trône de la scène de connexion
//!    (`C272,272`) donne la région 22 « 3D_UI 휴먼컨셉 ». L'ordre des cellules (ligne = z,
//!    colonne = x, comme la grille de hauteurs) est une hypothèse sans effet sur la musique tant que
//!    le secteur est d'une seule région ;
//! 2. `ini/RegionTemplate.ini` : section `[région]`, clés `Name`, `SkySet`, `WorldMap`, `Type` ;
//! 3. `ini/skyset.ini` : section `[SkySet]`, clés `HH_BackGroundMusic` (tranches propres à chaque
//!    jeu : `00`, `05`, `07`, `13`, `19`, `21` pour le `SkySet` 13) : nom du MP3 de `sound/bgm/`
//!    par tranche horaire. Une valeur vide reconduit la tranche précédente ; l'heure retenue est
//!    l'heure locale de la machine. Vérifié en jeu : Anchorville à 9 h → `HFT_008.mp3`.
//!
//! À faire : l'heure du jeu (serveur) plutôt que l'heure locale ; les ambiances (`sound/stream/`),
//! dont la table n'a pas encore été identifiée (les `SkySet` « 엠비언스 » n'ont pas de musique).

use crate::assets::Assets;
use crate::world::{sector_of, sector_origin, tile_of, CELLS, CELL_SIZE};
use std::collections::HashMap;
use std::sync::Arc;

/// Taille d'une grille `.amf` : en-tête + 256 cellules.
const AMF_LEN: usize = 16 + CELLS * CELLS * 4;

/// Indice de région porté par une cellule d'attributs.
pub fn cell_region(cell: u32) -> u16 {
    (cell >> 16) as u16
}

/// Grille de 256 cellules d'un `.amf` (sans l'en-tête), ou `None` si la taille ne correspond pas.
pub fn parse_amf(bytes: &[u8]) -> Option<Vec<u32>> {
    if bytes.len() < AMF_LEN {
        return None;
    }
    Some(
        bytes[16..AMF_LEN]
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect(),
    )
}

/// Musique d'une section `skyset.ini` à l'heure `hour` (0–23) : la tranche `HH_BackGroundMusic`
/// la plus tardive qui ne dépasse pas l'heure, sinon la dernière de la journée (minuit passé).
pub fn sky_music(section: &archrust_data::ini::IniSection, hour: u32) -> Option<String> {
    let mut slots: Vec<(u32, &str)> = section
        .entries
        .iter()
        .filter_map(|(k, v)| {
            let h: u32 = k.strip_suffix("_BackGroundMusic")?.parse().ok()?;
            Some((h, v.trim()))
        })
        .collect();
    slots.sort_by_key(|(h, _)| *h);
    // Une tranche vide reconduit la précédente.
    let mut last = "";
    let mut filled: Vec<(u32, &str)> = Vec::with_capacity(slots.len());
    for (h, v) in slots {
        if !v.is_empty() && v != "-" {
            last = v;
        }
        filled.push((h, last));
    }
    let pick = filled
        .iter()
        .rev()
        .find(|(h, v)| *h <= hour % 24 && !v.is_empty())
        .or_else(|| filled.iter().rev().find(|(_, v)| !v.is_empty()))?;
    Some(format!("sound/bgm/{}", pick.1.to_ascii_lowercase()))
}

/// Heure locale (0–23) ; hors Windows, heure UTC.
pub fn local_hour() -> u32 {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        let mut t = SYSTEMTIME {
            wYear: 0,
            wMonth: 0,
            wDayOfWeek: 0,
            wDay: 0,
            wHour: 0,
            wMinute: 0,
            wSecond: 0,
            wMilliseconds: 0,
        };
        // SAFETY : `GetLocalTime` remplit la structure fournie, valide et correctement alignée.
        unsafe { GetLocalTime(&mut t) };
        return (t.wHour as u32) % 24;
    }
    #[cfg(not(windows))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        ((secs / 3600) % 24) as u32
    }
}

/// Région et musique d'une position, avec cache des grilles `.amf` lues.
#[derive(Default)]
pub struct Zones {
    /// Grille par secteur (`None` = tuile ou entrée absente, on ne réessaie pas).
    grids: HashMap<(i32, i32), Option<Arc<Vec<u32>>>>,
}

impl Zones {
    fn grid(&mut self, assets: &Assets, sx: i32, sy: i32) -> Option<Arc<Vec<u32>>> {
        if let Some(g) = self.grids.get(&(sx, sy)) {
            return g.clone();
        }
        let loaded = (|| {
            let pack = assets.magpack('a', tile_of(sx, sy), "ma1").ok()?;
            let bytes = pack.unpack_named(&format!("C{sx},{sy}.amf"))?.ok()?;
            parse_amf(&bytes).map(Arc::new)
        })();
        if loaded.is_none() {
            log::debug!("zones : grille d'attributs C{sx},{sy}.amf indisponible");
        }
        self.grids.insert((sx, sy), loaded.clone());
        loaded
    }

    /// Indice de région (`RegionTemplate.ini`) de la cellule contenant (x, z).
    pub fn region_at(&mut self, assets: &Assets, x: f32, z: f32) -> Option<u16> {
        let (sx, sy) = sector_of(x, z);
        let grid = self.grid(assets, sx, sy)?;
        let origin = sector_origin(sx, sy);
        let cx = (((x - origin.x) / CELL_SIZE).floor() as i64).clamp(0, CELLS as i64 - 1) as usize;
        let cz = (((z - origin.z) / CELL_SIZE).floor() as i64).clamp(0, CELLS as i64 - 1) as usize;
        Some(cell_region(grid[cz * CELLS + cx]))
    }

    /// Nom (coréen) d'une région.
    pub fn region_name(assets: &Assets, region: u16) -> Option<String> {
        let t = assets.table("ini", "RegionTemplate.ini").ok()?;
        t.section(&region.to_string())?.get("Name").map(str::to_string)
    }

    /// Jeu de ciel (`SkySet`) d'une région.
    pub fn sky_set(assets: &Assets, region: u16) -> Option<i64> {
        let t = assets.table("ini", "RegionTemplate.ini").ok()?;
        t.section(&region.to_string())?.get_i64("SkySet")
    }

    /// Musique (`sound/bgm/…`) d'une région à l'heure donnée, d'après `skyset.ini`.
    pub fn music_for_region(assets: &Assets, region: u16, hour: u32) -> Option<String> {
        let sky = Self::sky_set(assets, region)?;
        let t = assets.table("ini", "skyset.ini").ok()?;
        sky_music(t.section(&sky.to_string())?, hour)
    }

    /// Région et musique d'une position.
    pub fn music_at(&mut self, assets: &Assets, x: f32, z: f32, hour: u32) -> Option<(u16, Option<String>)> {
        let region = self.region_at(assets, x, z)?;
        Some((region, Self::music_for_region(assets, region, hour)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archrust_data::ini::IniTable;

    #[test]
    fn amf_cells_carry_the_region_in_the_high_half() {
        assert_eq!(cell_region(0x0009_000e), 9);
        assert_eq!(cell_region(0x0016_020e), 22);
        let mut bytes = vec![0u8; 16];
        for i in 0..256u32 {
            bytes.extend_from_slice(&((i << 16) | 4).to_le_bytes());
        }
        let grid = parse_amf(&bytes).unwrap();
        assert_eq!(grid.len(), 256);
        assert_eq!(cell_region(grid[17]), 17);
        assert!(parse_amf(&bytes[..100]).is_none());
    }

    #[test]
    fn sky_music_picks_the_slot_for_the_hour() {
        let t = IniTable::parse(
            "[13]\nTID=13\n00_BackGroundMusic=HFT_001.mp3\n05_BackGroundMusic=HFT_006.mp3\n08_BackGroundMusic=\n12_BackGroundMusic=-\n20_BackGroundMusic=HFT_002.MP3\n[3]\n00_BackGroundMusic=\n05_BackGroundMusic=\n",
        );
        let s = t.section("13").unwrap();
        assert_eq!(sky_music(s, 3).as_deref(), Some("sound/bgm/hft_001.mp3"));
        assert_eq!(sky_music(s, 5).as_deref(), Some("sound/bgm/hft_006.mp3"));
        // Tranches vides : la précédente continue.
        assert_eq!(sky_music(s, 10).as_deref(), Some("sound/bgm/hft_006.mp3"));
        assert_eq!(sky_music(s, 15).as_deref(), Some("sound/bgm/hft_006.mp3"));
        assert_eq!(sky_music(s, 23).as_deref(), Some("sound/bgm/hft_002.mp3"));
        assert_eq!(sky_music(t.section("3").unwrap(), 12), None);
        assert!(local_hour() < 24);
    }

    #[test]
    fn no_client_means_no_region() {
        let assets = Assets::new(None, &[]);
        let mut z = Zones::default();
        assert_eq!(z.region_at(&assets, -478_000.0, -68_000.0), None);
        assert_eq!(Zones::music_for_region(&assets, 9, 12), None);
    }
}
