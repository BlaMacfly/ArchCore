//! Zones du monde : région d'une position, musique et ambiance de la région, heure du jeu.
//!
//! Chaîne reconstituée sur les données du client et sur Archonia (module ciel `AgcmSkySet`) :
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
//! 3. `ini/skyset.ini` : section `[SkySet]`, six tranches horaires fixes `00`, `05`, `07`, `13`,
//!    `19`, `21` (Archonia : 0–4 h, 5–6 h, 7–12 h, 13–17 h, 18–20 h, 21–23 h), chacune avec :
//!    * `HH_BackGroundMusic` : MP3 de `sound/bgm/` (vide = la tranche précédente continue) ;
//!    * `HH_EffectIndex_01..05` : jeux d'effets (`effect/ini/new/efffile_set.eff`) dont les effets
//!      **son** nomment un MP3 de `sound/stream/` ; Archonia les joue tous en boucle au changement
//!      de tranche ou de ciel (`HH_EffectRate_NN` ne sert qu'au tirage de l'effet visuel). Pour le
//!      `SkySet` 13 (Anchorville, nord de Limelight) : 3828 « 휴먼-심야 » `A_GRN_M.mp3` de 0 à 6 h,
//!      3825 « 휴먼-새벽 » `A_GRN_E.mp3` de 7 à 12 h, 3826 « 휴먼-낮 » `A_GRN_D.mp3` de 13 à 17 h,
//!      3827 « 휴먼-저녁 » `A_GRN_N.mp3` ensuite. Vérifié en jeu sur ArchCore à 7 h UTC :
//!      région 0 → `hft_008.mp3` + `a_grn_e.mp3`.
//! 4. l'heure est celle du jeu : `(temps_ms / 3 600 000) % 24` sur le temps du paquet
//!    `AGPMTIMER` (champ 1, millisecondes) entretenu par le client ([`GameClock`]) ; sans serveur,
//!    l'heure locale.

use crate::assets::Assets;
use crate::world::{sector_of, sector_origin, tile_of, CELLS, CELL_SIZE};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Taille d'une grille `.amf` : en-tête + 256 cellules.
const AMF_LEN: usize = 16 + CELLS * CELLS * 4;

/// Préfixes des six tranches horaires de `skyset.ini`, dans l'ordre de la journée.
pub const SKY_SLOTS: [&str; 6] = ["00", "05", "07", "13", "19", "21"];

/// Tranche horaire (indice dans [`SKY_SLOTS`]) d'une heure 0–23, comme Archonia.
pub fn sky_slot(hour: u32) -> usize {
    match hour % 24 {
        0..=4 => 0,
        5..=6 => 1,
        7..=12 => 2,
        13..=17 => 3,
        18..=20 => 4,
        _ => 5,
    }
}

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

/// Musique d'une section `skyset.ini` à l'heure `hour` (0–23) : `HH_BackGroundMusic` de la tranche
/// de l'heure, une tranche vide reconduisant la précédente (minuit passé : la dernière remplie).
pub fn sky_music(section: &archrust_data::ini::IniSection, hour: u32) -> Option<String> {
    let slot = sky_slot(hour);
    let value = |i: usize| {
        section
            .get(&format!("{}_BackGroundMusic", SKY_SLOTS[i]))
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "-")
    };
    let pick = (0..=slot)
        .rev()
        .chain((slot + 1..SKY_SLOTS.len()).rev())
        .find_map(value)?;
    Some(format!("sound/bgm/{}", pick.to_ascii_lowercase()))
}

/// Jeux d'effets (`HH_EffectIndex_01..05`, non nuls) de la tranche de l'heure.
pub fn sky_effect_sets(section: &archrust_data::ini::IniSection, hour: u32) -> Vec<u32> {
    let prefix = SKY_SLOTS[sky_slot(hour)];
    (1..=5)
        .filter_map(|n| section.get_i64(&format!("{prefix}_EffectIndex_{n:02}")))
        .filter(|&id| id > 0)
        .map(|id| id as u32)
        .collect()
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

/// Horloge du jeu : temps du serveur (paquet `AGPMTIMER`, millisecondes) avancé localement.
/// Archonia en tire l'heure du ciel par `(ms / 3 600 000) % 24`, sans fuseau : ArchCore envoie par
/// défaut l'époque Unix décalée du fuseau local du serveur (`world.timeZone`), l'heure du jeu est
/// donc l'heure locale du serveur.
#[derive(Debug, Clone, Copy, Default)]
pub struct GameClock {
    base: Option<(u64, Instant)>,
}

impl GameClock {
    /// Temps reçu du serveur (`WorldEvent::GameTime`).
    pub fn set(&mut self, server_ms: u64) {
        self.base = Some((server_ms, Instant::now()));
    }

    pub fn clear(&mut self) {
        self.base = None;
    }

    pub fn is_set(&self) -> bool {
        self.base.is_some()
    }

    /// Temps du jeu courant en millisecondes, si le serveur l'a donné.
    pub fn now_ms(&self) -> Option<u64> {
        self.base
            .map(|(ms, at)| ms.saturating_add(at.elapsed().as_millis() as u64))
    }

    /// Heure du jeu (0–23) : celle du serveur, sinon l'heure locale.
    pub fn hour(&self) -> u32 {
        self.now_ms().map_or_else(local_hour, hour_of_ms)
    }
}

/// Heure (0–23) d'un temps en millisecondes, comme Archonia (`AgpmEventNature`).
pub fn hour_of_ms(ms: u64) -> u32 {
    ((ms / 3_600_000) % 24) as u32
}

/// Ce qu'une position commande à l'audio.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ZoneAudio {
    pub region: u16,
    /// Musique (`sound/bgm/…`) de la région à l'heure donnée.
    pub music: Option<String>,
    /// Ambiances (`sound/stream/…`) de la région à l'heure donnée, dans l'ordre des
    /// `EffectIndex` ; Archonia les joue toutes, ArchRust la première.
    pub ambience: Vec<String>,
}

/// Région, musique et ambiance d'une position, avec cache des grilles `.amf` lues.
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

    /// Ambiances (`sound/stream/…`) d'une région à l'heure donnée : jeux d'effets
    /// `HH_EffectIndex_NN` de `skyset.ini` → effets son de `efffile_var.eff`.
    pub fn ambience_for_region(assets: &Assets, region: u16, hour: u32) -> Vec<String> {
        let Some(sky) = Self::sky_set(assets, region) else {
            return Vec::new();
        };
        let Ok(t) = assets.table("ini", "skyset.ini") else {
            return Vec::new();
        };
        let Some(section) = t.section(&sky.to_string()) else {
            return Vec::new();
        };
        let ids = sky_effect_sets(section, hour);
        if ids.is_empty() {
            return Vec::new();
        }
        let sets = assets.effect_sets();
        let mut out = Vec::new();
        for id in ids {
            match sets.get(id) {
                Some(set) => {
                    if set.sounds.is_empty() {
                        log::debug!("jeu d'effets {id} ({}) sans son", set.name);
                    }
                    out.extend(
                        set.sounds
                            .iter()
                            .map(|s| format!("sound/stream/{}", s.to_ascii_lowercase())),
                    );
                }
                None => log::debug!("jeu d'effets {id} absent de efffile_set.eff"),
            }
        }
        out
    }

    /// Région, musique et ambiance d'une position à l'heure donnée.
    pub fn audio_at(&mut self, assets: &Assets, x: f32, z: f32, hour: u32) -> Option<ZoneAudio> {
        let region = self.region_at(assets, x, z)?;
        Some(ZoneAudio {
            region,
            music: Self::music_for_region(assets, region, hour),
            ambience: Self::ambience_for_region(assets, region, hour),
        })
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
    fn sky_slots_follow_archonia_periods() {
        assert_eq!(sky_slot(0), 0);
        assert_eq!(sky_slot(4), 0);
        assert_eq!(sky_slot(5), 1);
        assert_eq!(sky_slot(6), 1);
        assert_eq!(sky_slot(7), 2);
        assert_eq!(sky_slot(12), 2);
        assert_eq!(sky_slot(13), 3);
        assert_eq!(sky_slot(17), 3);
        assert_eq!(sky_slot(18), 4);
        assert_eq!(sky_slot(20), 4);
        assert_eq!(sky_slot(21), 5);
        assert_eq!(sky_slot(23), 5);
        assert_eq!(sky_slot(27), 0);
    }

    #[test]
    fn sky_music_picks_the_slot_for_the_hour() {
        let t = IniTable::parse(
            "[13]\nTID=13\n00_BackGroundMusic=HFT_001.mp3\n05_BackGroundMusic=HFT_006.mp3\n07_BackGroundMusic=\n13_BackGroundMusic=-\n19_BackGroundMusic=HFT_002.MP3\n21_BackGroundMusic=\n[3]\n00_BackGroundMusic=\n05_BackGroundMusic=\n",
        );
        let s = t.section("13").unwrap();
        assert_eq!(sky_music(s, 3).as_deref(), Some("sound/bgm/hft_001.mp3"));
        assert_eq!(sky_music(s, 5).as_deref(), Some("sound/bgm/hft_006.mp3"));
        // Tranches vides : la précédente continue.
        assert_eq!(sky_music(s, 10).as_deref(), Some("sound/bgm/hft_006.mp3"));
        assert_eq!(sky_music(s, 15).as_deref(), Some("sound/bgm/hft_006.mp3"));
        // 18 h appartient à la tranche « 19 » (18–20 h), comme Archonia.
        assert_eq!(sky_music(s, 18).as_deref(), Some("sound/bgm/hft_002.mp3"));
        assert_eq!(sky_music(s, 23).as_deref(), Some("sound/bgm/hft_002.mp3"));
        assert_eq!(sky_music(t.section("3").unwrap(), 12), None);
        assert!(local_hour() < 24);
    }

    #[test]
    fn sky_effect_sets_follow_the_slot() {
        let t = IniTable::parse(
            "[13]\n00_EffectIndex_01=3828\n00_EffectIndex_02=0\n07_EffectIndex_01=3825\n07_EffectIndex_02=3868\n07_EffectIndex_03=0\n13_EffectIndex_01=3826\n",
        );
        let s = t.section("13").unwrap();
        assert_eq!(sky_effect_sets(s, 2), vec![3828]);
        assert_eq!(sky_effect_sets(s, 9), vec![3825, 3868]);
        assert_eq!(sky_effect_sets(s, 13), vec![3826]);
        // Tranche sans clé : rien.
        assert!(sky_effect_sets(s, 22).is_empty());
    }

    #[test]
    fn game_clock_uses_the_server_time_when_known() {
        let mut clock = GameClock::default();
        assert!(!clock.is_set());
        assert_eq!(clock.hour(), local_hour());
        // 1970-01-01 15:30 UTC.
        clock.set(15 * 3_600_000 + 30 * 60_000);
        assert!(clock.is_set());
        assert_eq!(clock.hour(), 15);
        assert_eq!(hour_of_ms(24 * 3_600_000 + 5), 0);
        assert_eq!(hour_of_ms(u64::MAX), ((u64::MAX / 3_600_000) % 24) as u32);
        clock.clear();
        assert!(!clock.is_set());
    }

    #[test]
    fn no_client_means_no_region() {
        let assets = Assets::new(None, &[]);
        let mut z = Zones::default();
        assert_eq!(z.region_at(&assets, -478_000.0, -68_000.0), None);
        assert_eq!(Zones::music_for_region(&assets, 9, 12), None);
        assert!(Zones::ambience_for_region(&assets, 9, 12).is_empty());
        assert!(z.audio_at(&assets, 0.0, 0.0, 12).is_none());
        assert!(assets.effect_sets().is_empty());
    }
}
