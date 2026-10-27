//! Monstres placés dans le monde (importés des tables du serveur d'origine dans `archcore_world.monster_spawn`).
//!
//! Chaque point d'apparition porte `count` monstres d'un même modèle, dispersés dans le carré de côté
//! 2 × rayon centré sur le point, hors des segments de sol infranchissable de la carte serveur (`map.rs`),
//! avec une orientation au hasard. Le tirage part d'une graine fixe
//! (identifiant du point) : un monstre réapparaît au même endroit d'un démarrage à l'autre. La hauteur est
//! celle de la table (souvent 0) : le client pose le monstre sur le terrain.
//!
//! Apparition côté client : la même vue optimisée (0x42, op 1) que les PNJ, sans fonctions d'événement, mais
//! avec les facteurs du monstre (niveau, vitesses, PV) tirés de CharacterDataTable.txt : sans eux le client
//! le croit à 0 PV.
//!
//! IA : chaque point d'apparition désigne une IA (`ai_tid`, table `monster_ai`) qui donne les compétences du
//! monstre (`monster_skills.rs`).

use super::npcs::Translator;
use crate::character::{factors_mini, Factors};
use crate::db::{col, Databases};
use crate::info;
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Vec3};
use rand::{Rng, SeedableRng};
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;

/// Les monstres reçoivent des CID dans une plage séparée de celles des joueurs et des PNJ.
pub const MONSTER_CID_BASE: u32 = 0x5000_0000;
/// Côté d'une case de la grille de recherche.
const CELL: f64 = 4096.0;
/// Drapeau de déplacement « arrêté ».
const MOVE_STOP: i64 = 0x04;
/// Tirages au plus pour trouver une position libre ; à défaut, le monstre est placé au centre du point.
const PLACEMENT_TRIES: usize = 16;
/// Statut d'action (champ 3 de la vue) d'un personnage mort.
const ACTION_STATUS_DEAD: i64 = 2;
/// Portée de touche d'un modèle sans « HitRange » connu.
pub const DEFAULT_HIT_RANGE: i32 = 100;

/// Bits du type de personnage (« Type » de CharacterDataTable.txt) : 0x4 monstre, 0x8 créature d'ambiance
/// (coqs, poules…), 0x10 garde, puis ceux-ci.
pub mod char_type {
    pub const ATTACKABLE: i64 = 0x1_0000;
    pub const TARGETABLE: i64 = 0x2_0000;
}

/// Caractéristiques d'un modèle de monstre (CharacterDataTable.txt).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonsterStats {
    pub race: i32,
    pub class: i32,
    pub max_hp: i32,
    pub max_mp: i32,
    pub max_sp: i32,
    pub walk: i32,
    pub run: i32,
    pub ar: i32,
    pub min_damage: i32,
    pub max_damage: i32,
    pub attack_speed: i32,
    pub range: i32,
    pub exp: i32,
    /// Défense physique (« AC »).
    pub ac: i32,
    /// Rayon du corps, ajouté à la portée de l'attaquant (« HitRange » de charactertemplate.ini).
    pub hit_range: i32,
    /// Type de personnage (bits `char_type`).
    pub char_type: i64,
    /// Or laissé à la mort (« Geld_Min », « Geld_Max »).
    pub geld_min: i32,
    pub geld_max: i32,
    /// Rang de butin (« DropRank ») et multiplicateur des groupes sensibles (« DropMeditation »).
    pub drop_rank: i32,
    pub drop_meditation: f64,
    /// Dégâts élémentaires (min, max), dans l'ordre des éléments des compétences : magie, eau, feu, terre, air,
    /// poison, foudre, glace.
    pub spirit: [(i32, i32); 8],
    /// Résistances élémentaires (pour cent), même ordre.
    pub resist: [i32; 8],
}

impl MonsterStats {
    /// Lit une ligne de `character_data_table.data` (clés de l'en-tête de CharacterDataTable.txt).
    pub fn from_data(v: &serde_json::Value) -> Self {
        let n = |k: &str| -> i32 {
            match v.get(k) {
                Some(serde_json::Value::String(t)) => t.trim().parse::<f64>().map(|x| x as i32).unwrap_or(0),
                Some(serde_json::Value::Number(x)) => x.as_f64().unwrap_or(0.0) as i32,
                _ => 0,
            }
        };
        MonsterStats {
            race: n("Race"),
            class: n("Class"),
            max_hp: n("MAX_HP"),
            max_mp: n("MAX_MP"),
            max_sp: n("MAX_SP"),
            walk: n("Movement Walk"),
            run: n("Movement Run"),
            ar: n("AR"),
            min_damage: n("Min_Damage"),
            max_damage: n("Max_Damage"),
            attack_speed: n("ATK Speed"),
            range: n("Range"),
            exp: n("Exp"),
            ac: n("AC"),
            hit_range: DEFAULT_HIT_RANGE,
            char_type: n("Type") as i64,
            geld_min: n("Geld_Min"),
            geld_max: n("Geld_Max"),
            drop_rank: n("DropRank"),
            drop_meditation: match v.get("DropMeditation") {
                Some(serde_json::Value::String(t)) => t.trim().parse().unwrap_or(1.0),
                _ => 1.0,
            },
            spirit: ["Magic", "Water", "Fire", "Earth", "Air", "Poison", "Thunder", "Ice"].map(|e| (n(&format!("{e} Min_Damage")), n(&format!("{e} Max_Damage")))),
            resist: ["Magic", "Water", "Fire", "Earth", "Air", "Poison", "Thunder", "Ice"].map(|e| n(&format!("{e} Res."))),
        }
    }

    /// Un joueur peut le prendre pour cible et l'attaquer.
    pub fn is_attackable(&self) -> bool {
        let needed = char_type::ATTACKABLE | char_type::TARGETABLE;
        self.char_type & needed == needed
    }
}

/// Facteurs d'un monstre envoyés avec sa vue : niveau et vitesses, type, PV/PM/PE, attaque.
pub fn monster_factors(level: i64, s: &MonsterStats, hp: i32) -> Factors {
    let mut f = Factors::default();
    f.status[7] = level as i32;
    f.status[8] = s.walk;
    f.status[9] = s.run;
    f.ftype = [s.race, 0, s.class];
    f.point[0] = hp;
    f.point[1] = s.max_mp;
    f.point[2] = s.max_sp;
    f.point_max = [s.max_hp, s.max_mp, s.max_sp, 0, s.exp, 0, 0, 0, s.ar, 0, 0];
    f.attack = [s.range, 50, s.attack_speed, 0, 0, 0, 0, 0];
    f.damage_min[0] = s.min_damage;
    f.damage_max[0] = s.max_damage;
    f
}

/// Cycle de vie d'un monstre : vivant, cadavre visible, puis retiré jusqu'à sa réapparition.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Life {
    Alive,
    Dead { remove_at: Instant },
    Gone { respawn_at: Instant },
}

/// Un agresseur retenu par un monstre : dégâts cumulés, oublié 30 s après son dernier coup.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aggro {
    /// CID du joueur.
    pub cid: u32,
    pub damage: u64,
    pub until: Instant,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Monster {
    pub cid: u32,
    pub spawn_id: u32,
    pub tid: u32,
    pub level: i64,
    pub name: String,
    pub pos: Vec3,
    pub degree_x: f64,
    pub degree_y: f64,
    pub hp: i32,
    pub max_hp: i32,
    /// Délai de réapparition après le retrait du cadavre, en secondes ; tiré entre `respawn_sec` et
    /// `respawn_max_sec` quand celui-ci est plus grand (boss de la fenêtre des boss).
    pub respawn_sec: i64,
    pub respawn_max_sec: i64,
    pub life: Life,
    /// Position d'apparition, où il revient quand il abandonne.
    pub home: Vec3,
    pub aggro: Vec<Aggro>,
    /// Joueur visé (CID).
    pub target: Option<u32>,
    /// Prochaine action possible (coup ou fin de déplacement) et prochain choix de cible.
    pub ready_at: Instant,
    pub scan_at: Instant,
    /// Rayon de promenade autour du point d'apparition, et prochaine promenade.
    pub roam: f64,
    pub patrol_at: Instant,
    /// IA du point d'apparition (0 : aucune).
    pub ai_tid: u32,
}

impl Monster {
    pub fn is_alive(&self) -> bool {
        self.life == Life::Alive
    }
    /// Visible des joueurs : vivant ou cadavre.
    pub fn is_visible(&self) -> bool {
        !matches!(self.life, Life::Gone { .. })
    }
}

/// Point d'apparition tel que lu dans `monster_spawn`.
#[derive(Clone, Debug)]
pub struct SpawnPoint {
    pub id: u32,
    pub tid: u32,
    pub name: String,
    pub level: i64,
    pub count: i64,
    pub pos: Vec3,
    pub degree_x: f64,
    pub radius: f64,
    pub max_hp: i32,
    pub respawn_sec: i64,
    pub respawn_max_sec: i64,
    pub ai_tid: u32,
}

/// Marche d'un monstre : destination, vitesse (dixièmes d'unité par seconde), instant du dernier pas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walk {
    pub dest: Vec3,
    pub speed: f64,
    pub at: Instant,
    /// À la course (drapeau 0x10), sinon à la marche.
    pub fast: bool,
}

/// Monstres du monde, avec une grille X/Z pour retrouver vite ceux qui sont à portée.
#[derive(Default)]
pub struct MonsterRegistry {
    pub monsters: Vec<Monster>,
    /// Caractéristiques par modèle.
    pub stats: HashMap<u32, MonsterStats>,
    /// Monstres morts ou retirés, à faire avancer dans leur cycle de vie.
    pub pending: BTreeSet<u32>,
    /// Monstres qui ont des agresseurs (IA active).
    pub engaged: BTreeSet<u32>,
    /// Prochain examen des monstres préventifs (ai::preemptive_step).
    pub preemptive_at: Option<Instant>,
    /// Effets des compétences portés par des monstres.
    pub effects: HashMap<u32, Vec<super::effects::Effect>>,
    /// États spéciaux annoncés aux clients (monstres qui en ont).
    pub status_sent: HashMap<u32, u64>,
    /// Marches en cours, avancées à chaque tour (`ai::walk_step`).
    pub walking: HashMap<u32, Walk>,
    /// Monstres posés sur le sol (point d'apparition sans hauteur) : leur hauteur suit le terrain.
    pub grounded: std::collections::HashSet<u32>,
    /// IA par TID (`monster_ai`), état de leurs compétences pendant la vie de chaque monstre, incantations.
    pub ai: HashMap<u32, super::monster_skills::MonsterAi>,
    pub skill_state: HashMap<u32, super::monster_skills::SkillState>,
    pub casts: HashMap<u32, super::monster_skills::MonsterCast>,
    by_cid: HashMap<u32, usize>,
    grid: HashMap<(i64, i64), Vec<usize>>,
}

fn cell_of(x: f64, z: f64) -> (i64, i64) {
    ((x / CELL).floor() as i64, (z / CELL).floor() as i64)
}

/// Nombre de monstres d'un point sous un multiplicateur de densité : arrondi, au moins 1 s'il en avait.
pub fn scaled_count(count: i64, density: f64) -> i64 {
    if count <= 0 {
        return 0;
    }
    ((count as f64 * density.max(0.0)).round() as i64).max(1)
}

/// Monstres d'un point, dispersés uniformément dans le carré [x ± rayon] × [z ± rayon] en évitant les
/// positions où `blocked(x, z)`, chacun tourné au hasard (orientation Y entre -180 et 180°).
pub fn spawn_point(p: &SpawnPoint, first_cid: u32, blocked: &dyn Fn(f64, f64) -> bool) -> Vec<Monster> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(p.id as u64);
    let r = p.radius.max(0.0);
    let (cx, cz) = (p.pos.x as f64, p.pos.z as f64);
    (0..p.count.max(0) as u32)
        .map(|i| {
            let mut offset = (0.0, 0.0);
            if r > 0.0 {
                for _ in 0..PLACEMENT_TRIES {
                    let (dx, dz): (f64, f64) = (rng.gen_range(-r..r), rng.gen_range(-r..r));
                    if !blocked(cx + dx, cz + dz) {
                        offset = (dx, dz);
                        break;
                    }
                }
            }
            let (dx, dz) = offset;
            let pos = Vec3::new(p.pos.x + dx as f32, p.pos.y, p.pos.z + dz as f32);
            let now = Instant::now();
            let degree_y = rng.gen_range(-180..180) as f64;
            Monster {
                cid: first_cid + i,
                spawn_id: p.id,
                tid: p.tid,
                level: p.level,
                name: p.name.clone(),
                pos,
                degree_x: p.degree_x,
                degree_y,
                hp: p.max_hp,
                max_hp: p.max_hp,
                respawn_sec: p.respawn_sec,
                respawn_max_sec: p.respawn_max_sec,
                life: Life::Alive,
                home: pos,
                aggro: Vec::new(),
                target: None,
                ready_at: now,
                scan_at: now,
                roam: r,
                patrol_at: now + std::time::Duration::from_millis(rng.gen_range(0..45_000)),
                ai_tid: p.ai_tid,
            }
        })
        .collect()
}

impl MonsterRegistry {
    /// `density` multiplie le nombre de monstres de chaque point (arrondi, au moins 1).
    pub async fn load(db: &Databases, translator: &Translator, map: &super::map::WorldMap, density: f64) -> anyhow::Result<Self> {
        // Toutes les créatures, y compris celles des guerres de siège ; la table peut manquer si l'import n'a
        // pas eu les tables du serveur.
        let rows = match db
            .world
            .rows("SELECT id, tid, display_name, level, count, respawn_sec, respawn_max_sec, ai_tid, pos_x, pos_y, pos_z, degree_x, radius FROM monster_spawn WHERE count > 0 ORDER BY id", ())
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                crate::warn!("monstres non chargés ({e:#}) : lancer `archcore db-init` puis `archcore import --server …`");
                Vec::new()
            }
        };
        let mut stats = HashMap::new();
        for r in db.world.rows("SELECT tid, data FROM character_data_table", ()).await? {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&col::<String>(&r, "data")) {
                stats.insert(col::<u32>(&r, "tid"), MonsterStats::from_data(&v));
            }
        }
        // Rayon du corps : dans charactertemplate.ini, quand le modèle y figure.
        for r in db.world.rows("SELECT tid, JSON_VALUE(raw, '$.HitRange') AS hit_range FROM character_template", ()).await? {
            let hit: i32 = col::<Option<String>>(&r, "hit_range").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
            if let Some(s) = stats.get_mut(&col::<u32>(&r, "tid")).filter(|_| hit > 0) {
                s.hit_range = hit;
            }
        }
        let points: Vec<SpawnPoint> = rows
            .iter()
            .map(|r| SpawnPoint {
                id: col(r, "id"),
                tid: col(r, "tid"),
                name: translator.translate(&col::<String>(r, "display_name")),
                level: col(r, "level"),
                count: scaled_count(col(r, "count"), density),
                pos: Vec3::new(col::<f64>(r, "pos_x") as f32, col::<f64>(r, "pos_y") as f32, col::<f64>(r, "pos_z") as f32),
                degree_x: col(r, "degree_x"),
                radius: col(r, "radius"),
                max_hp: stats.get(&col::<u32>(r, "tid")).map(|s| s.max_hp.max(1)).unwrap_or(1),
                respawn_sec: col(r, "respawn_sec"),
                respawn_max_sec: col(r, "respawn_max_sec"),
                ai_tid: col::<i64>(r, "ai_tid").max(0) as u32,
            })
            .collect();
        let without = points.iter().filter(|p| !stats.contains_key(&p.tid)).count();
        let mut reg = Self::from_points(&points, &|x, z| map.is_blocked(x, z));
        reg.stats = stats;
        // IA des points (aidatatable.txt) ; la table manque tant que `archcore import --server` ne l'a pas remplie.
        match db.world.rows("SELECT tid, data FROM monster_ai", ()).await {
            Ok(rows) => {
                for r in &rows {
                    let data = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "data")).unwrap_or_default();
                    reg.ai.insert(col(r, "tid"), super::monster_skills::MonsterAi::from_data(&data));
                }
            }
            Err(e) => crate::warn!("IA des monstres non chargée ({e:#}) : lancer `archcore db-init` puis `archcore import --server …`"),
        }
        let skilled = reg.monsters.iter().filter(|m| reg.ai.get(&m.ai_tid).is_some_and(|a| !a.skills.is_empty())).count();
        info!("IA des monstres : {} ; {skilled} monstre(s) avec des compétences", reg.ai.len());
        // Points sans hauteur (la plupart) : chaque monstre est posé sur le sol du client.
        let flat: std::collections::HashSet<u32> = points.iter().filter(|p| p.pos.y == 0.0).map(|p| p.id).collect();
        for m in reg.monsters.iter_mut().filter(|m| flat.contains(&m.spawn_id)) {
            if let Some(y) = map.height(m.pos.x as f64, m.pos.z as f64) {
                m.pos.y = y;
                m.home.y = y;
                reg.grounded.insert(m.cid);
            }
        }
        info!("{} monstre(s) chargé(s) sur {} point(s) d'apparition ({without} point(s) sans caractéristiques, densité × {density}, {} posé(s) sur le sol)", reg.monsters.len(), points.len(), reg.grounded.len());
        Ok(reg)
    }

    pub fn from_points(points: &[SpawnPoint], blocked: &dyn Fn(f64, f64) -> bool) -> Self {
        let mut monsters = Vec::new();
        for p in points {
            let first = MONSTER_CID_BASE + monsters.len() as u32;
            monsters.extend(spawn_point(p, first, blocked));
        }
        Self::from_monsters(monsters)
    }

    pub fn from_monsters(monsters: Vec<Monster>) -> Self {
        let mut reg = MonsterRegistry { monsters, ..Default::default() };
        for (i, m) in reg.monsters.iter().enumerate() {
            reg.by_cid.insert(m.cid, i);
            reg.grid.entry(cell_of(m.pos.x as f64, m.pos.z as f64)).or_default().push(i);
        }
        reg
    }

    pub fn get(&self, cid: u32) -> Option<&Monster> {
        self.by_cid.get(&cid).map(|&i| &self.monsters[i])
    }

    pub fn get_mut(&mut self, cid: u32) -> Option<&mut Monster> {
        self.by_cid.get(&cid).map(|&i| &mut self.monsters[i])
    }

    /// Déplace un monstre et met la grille de recherche à jour.
    pub fn relocate(&mut self, cid: u32, pos: Vec3) {
        let Some(&i) = self.by_cid.get(&cid) else { return };
        let old = self.monsters[i].pos;
        let (from, to) = (cell_of(old.x as f64, old.z as f64), cell_of(pos.x as f64, pos.z as f64));
        if from != to {
            if let Some(v) = self.grid.get_mut(&from) {
                v.retain(|&j| j != i);
            }
            self.grid.entry(to).or_default().push(i);
        }
        self.monsters[i].pos = pos;
    }

    /// IA d'un monstre, si son point en a une.
    pub fn ai_of(&self, cid: u32) -> Option<&super::monster_skills::MonsterAi> {
        self.get(cid).and_then(|m| self.ai.get(&m.ai_tid))
    }

    /// Somme des modificateurs des effets portés par un monstre.
    pub fn mods(&self, cid: u32) -> super::effects::Mods {
        self.effects.get(&cid).map(|l| super::effects::total(l)).unwrap_or_default()
    }

    /// États spéciaux en cours d'un monstre.
    pub fn status(&self, cid: u32, now: Instant) -> u64 {
        self.effects.get(&cid).map(|l| super::effects::active_status(l, now)).unwrap_or(0)
    }

    /// Un coup réveille le monstre endormi.
    pub fn wake(&mut self, cid: u32) {
        if let Some(l) = self.effects.get_mut(&cid) {
            super::effects::lift_status(l, super::effects::status::SLEEP);
        }
    }

    pub fn stats_of(&self, tid: u32) -> MonsterStats {
        self.stats.get(&tid).cloned().unwrap_or_default()
    }

    /// Monstres (même retirés) dont la distance horizontale (X/Z) à `pos` est au plus `radius`.
    pub fn near(&self, pos: Vec3, radius: f64) -> impl Iterator<Item = &Monster> {
        let (x, z) = (pos.x as f64, pos.z as f64);
        let (lo, hi) = (cell_of(x - radius, z - radius), cell_of(x + radius, z + radius));
        let r2 = radius * radius;
        (lo.0..=hi.0)
            .flat_map(move |cx| (lo.1..=hi.1).map(move |cz| (cx, cz)))
            .filter_map(|c| self.grid.get(&c))
            .flatten()
            .map(|&i| &self.monsters[i])
            .filter(move |m| {
                let (dx, dz) = (m.pos.x as f64 - x, m.pos.z as f64 - z);
                dx * dx + dz * dz <= r2
            })
    }

    /// Paquet d'apparition d'un monstre (vue optimisée, opération 1) : immobile (drapeau d'arrêt, pas de
    /// destination), avec ses facteurs.
    pub fn view_packet(&self, m: &Monster) -> Vec<u8> {
        self.view_packet_named(m, &m.name)
    }

    /// Comme `view_packet`, avec le nom à afficher (localisé pour le joueur, voir `locale.rs`).
    pub fn view_packet_named(&self, m: &Monster, name: &str) -> Vec<u8> {
        let clamp16 = |v: f64| v.round().clamp(-32768.0, 32767.0) as i64;
        let stats = self.stats.get(&m.tid).cloned().unwrap_or_default();
        let vs = &mini::OPT_VIEW_CHAR;
        let view = build_mini(
            vs,
            &Fields::new(vs)
                .with(0, m.cid)
                .with(1, m.tid)
                .with(2, archcore_net::text::encode(name))
                .with(3, if m.is_alive() { 0 } else { ACTION_STATUS_DEAD })
                .with(5, factors_mini(&monster_factors(m.level, &stats, m.hp)))
                .with(6, MOVE_STOP)
                .with(7, 0)
                .with(8, m.pos)
                .with(12, clamp16(m.degree_x))
                .with(13, clamp16(m.degree_y)),
        );
        let ov = &schema::OPT_VIEW;
        build_packet(ov, &Fields::new(ov).with(0, 1).with(1, view))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_mini, decode_packet};

    fn point(id: u32, x: f32, z: f32, count: i64, radius: f64) -> SpawnPoint {
        SpawnPoint { id, tid: 450, name: "Ver vert".into(), level: 1, count, pos: Vec3::new(x, 0.0, z), degree_x: 0.0, radius, max_hp: 136, respawn_sec: 8, respawn_max_sec: 0, ai_tid: 4500 }
    }

    #[test]
    fn densite() {
        assert_eq!((scaled_count(3, 1.0), scaled_count(3, 2.5), scaled_count(1, 0.2), scaled_count(0, 3.0)), (3, 8, 1, 0));
    }

    #[test]
    fn dispersion_stable_dans_le_carre() {
        let p = point(7, 1000.0, -2000.0, 50, 2500.0);
        let a = spawn_point(&p, MONSTER_CID_BASE, &|_, _| false);
        assert_eq!(a.len(), 50);
        let placed = |v: &[Monster]| v.iter().map(|m| (m.pos, m.degree_y)).collect::<Vec<_>>();
        assert_eq!(placed(&a), placed(&spawn_point(&p, MONSTER_CID_BASE, &|_, _| false)), "même graine, mêmes positions");
        for m in &a {
            assert!((m.pos.x - 1000.0).abs() <= 2500.0 && (m.pos.z + 2000.0).abs() <= 2500.0);
            assert!((-180.0..180.0).contains(&m.degree_y));
            assert_eq!(m.hp, 136);
        }
        assert_eq!(spawn_point(&point(1, 0.0, 0.0, 3, 0.0), 0, &|_, _| false).iter().filter(|m| m.pos == Vec3::new(0.0, 0.0, 0.0)).count(), 3);
    }

    #[test]
    fn evite_le_sol_bloque() {
        // Moitié ouest (x < 1000) bloquée : tous les monstres finissent à l'est.
        let a = spawn_point(&point(3, 1000.0, 0.0, 40, 2000.0), MONSTER_CID_BASE, &|x, _| x < 1000.0);
        assert!(a.iter().all(|m| m.pos.x >= 1000.0));
        // Tout bloqué : repli au centre du point.
        let b = spawn_point(&point(3, 1000.0, 0.0, 3, 2000.0), MONSTER_CID_BASE, &|_, _| true);
        assert!(b.iter().all(|m| m.pos == Vec3::new(1000.0, 0.0, 0.0)));
    }

    #[test]
    fn recherche_par_grille() {
        let reg = MonsterRegistry::from_points(&[point(1, 0.0, 0.0, 4, 100.0), point(2, 20_000.0, 0.0, 2, 0.0), point(3, -9000.0, -9000.0, 1, 0.0)], &|_, _| false);
        assert_eq!(reg.monsters.len(), 7);
        assert_eq!(reg.near(Vec3::new(0.0, 0.0, 0.0), 8000.0).count(), 4);
        assert_eq!(reg.near(Vec3::new(19_000.0, 0.0, 0.0), 1000.0).count(), 2);
        assert_eq!(reg.near(Vec3::new(-9000.0, 0.0, -9000.0), 10.0).count(), 1);
        // CID consécutifs, retrouvables.
        assert_eq!(reg.get(MONSTER_CID_BASE + 4).unwrap().spawn_id, 2);
        // Un monstre déplacé se retrouve à sa nouvelle place, plus à l'ancienne.
        let mut reg = reg;
        reg.relocate(MONSTER_CID_BASE + 6, Vec3::new(30_000.0, 0.0, 30_000.0));
        assert_eq!(reg.near(Vec3::new(-9000.0, 0.0, -9000.0), 10.0).count(), 0);
        assert_eq!(reg.near(Vec3::new(30_000.0, 0.0, 30_000.0), 10.0).count(), 1);
    }

    #[test]
    fn caracteristiques_depuis_la_table() {
        let v: serde_json::Value = serde_json::json!({"TID":"450","Race":"11","Class":"5","MAX_HP":"136","MAX_MP":"35","MAX_SP":"62",
            "Movement Walk":"450","Movement Run":"3850","AR":"21","Min_Damage":"15","Max_Damage":"15","ATK Speed":"32","Range":"170","Exp":"14"});
        let s = MonsterStats::from_data(&v);
        assert_eq!((s.race, s.class, s.max_hp, s.walk, s.run, s.range, s.exp), (11, 5, 136, 450, 3850, 170, 14));
        assert!(!s.is_attackable(), "sans type : non attaquable");
        assert!(MonsterStats { char_type: 196_612, ..s.clone() }.is_attackable());
        assert!(!MonsterStats { char_type: 8, ..s.clone() }.is_attackable(), "coq, poule");
        assert!(!MonsterStats { char_type: 65_540, ..s }.is_attackable(), "attaquable mais pas ciblable");
        let f = monster_factors(1, &s, 100);
        assert_eq!((f.status[7], f.status[8], f.status[9], f.point[0], f.point_max[0]), (1, 450, 3850, 100, 136));
    }

    #[test]
    fn paquet_de_vue() {
        let m = spawn_point(&point(1, 10.0, 20.0, 1, 0.0), MONSTER_CID_BASE, &|_, _| false).remove(0);
        let mut reg = MonsterRegistry::from_monsters(vec![m.clone()]);
        reg.stats.insert(450, MonsterStats { max_hp: 136, walk: 450, run: 3850, ..Default::default() });
        let p = decode_packet(&schema::OPT_VIEW, &reg.view_packet(&m)).unwrap();
        assert_eq!(p.int(0), Some(1));
        let v = decode_mini(&mini::OPT_VIEW_CHAR, p.bytes(1).unwrap()).unwrap();
        assert_eq!((v.int(0), v.int(1)), (Some(MONSTER_CID_BASE as i64), Some(450)));
        assert_eq!(v.vec3(8), Some(Vec3::new(10.0, 0.0, 20.0)));
        assert_eq!(v.int(6), Some(MOVE_STOP), "immobile");
        assert!(v.vec3(9).is_none(), "pas de destination");
        assert!(v.bytes(5).is_some_and(|b| !b.is_empty()), "facteurs présents");
        assert!(p.bytes(4).is_none(), "pas de fonctions d'événement");
    }
}
