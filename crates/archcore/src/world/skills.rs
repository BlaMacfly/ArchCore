//! Compétences : catalogue, compétences des personnages, maître des compétences, lancement.
//!
//! ```text
//!   Données : skill_spec (SKILL_SPEC.txt, nature de la compétence) et skill_level (SKILL_CONST.txt, valeurs
//!             par niveau : classes admises, niveau requis, points, prix, coûts, temps, portée, dégâts).
//!   Entrée en jeu : chaque compétence part en AGPMSKILL op 0 (identifiant, propriétaire, facteurs « dirt »
//!             avec le niveau, TID), dans le champ 8 de la vue optimisée op 2. Tout personnage connaît
//!             « Normal Attack » (TID 222), l'attaque de base.
//!   Points de compétence : le niveau du personnage (plus 1 par niveau de 50 à 70), moins les points exigés
//!             par chaque niveau appris ; envoyés dans les facteurs « dirt » du personnage (indice 5).
//!   Maître des compétences (AGPMEVENTSKILLMASTER 0x3B) :
//!     client → op 4 (événement) ← op 5, résultat 0 : ouvre la fenêtre
//!     client → op 1 (TID au champ 4) : apprendre ← AGPMSKILL op 0 puis op 6, résultat 0 (réussi), 2 (déjà
//!             apprise), 3 (pas assez de points), 4 (niveau trop bas), 5 (classe), 1 (autre, dont l'argent)
//!     client → op 2 (identifiant au champ 3) : améliorer ← AGPMSKILL op 2 puis op 3
//!   Lancement : client → AGPMSKILL op 7 (identifiant, action : cible). Le serveur vérifie la compétence, son
//!             délai de relance, les PM et PV, approche la cible si besoin, puis diffuse op 7 (action 1 :
//!             durées d'incantation, d'effet et de relance) ; au bout de l'incantation, op 8 (action 2 :
//!             facteurs du résultat, dégâts en négatif) aux joueurs qui voient la cible.
//! ```
//! Effets : dégâts immédiats sur la cible et, pour les compétences de zone (champ `range` de SKILL_SPEC :
//! sphère autour de la cible ou du lanceur, rayon `area_r` ; boîte vers la cible, `area_f1` × `area_f2`),
//! sur les monstres voisins ; effets durables, dégâts sur la durée et soins (`effects.rs`) ; passives,
//! appliquées tant que la compétence est connue ; états spéciaux (étourdissement, sommeil…), posés sur la
//! cible selon leur chance, et effets de combat (critiques, blocage, dégâts changés en PV ou PM, contre-attaque).
//! Les compétences amies visent leur lanceur ; celles de groupe (zone 303 de SKILL_SPEC), aussi les membres de
//! son groupe dans leur rayon `area_r`. Maîtrises : une compétence des arbres supérieur et héroïque
//! (SkillMastery_High.txt, SkillMastery_Heroic.txt) ne s'apprend qu'avec les compétences qu'elle exige à leur
//! niveau et assez de points dépensés en tout.

use super::combat::{self, Attacker, Defender};
use super::effects::{self, DotSpec, Effect, Heal, Mods};
use super::monsters::Monster;
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::event_function;
use super::{send_system, Player, WorldCtx, WorldState};
use crate::character::{factors_mini, Factors, POINT_HP};
use crate::db::{col, Databases};
use crate::{debug, info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet, Vec3};
use rand::Rng;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// TID de « Normal Attack », connue de tous.
pub const NORMAL_ATTACK: u32 = 222;
/// Compétences au plus par personnage.
pub const MAX_SKILLS: usize = 64;
/// Type d'objet « personnage » dans SKILL_BASE.
const BASE_CHARACTER: i64 = 3;
/// Indice des points de compétence dans les facteurs « dirt » (durée, intensité, portée, cible, niveau,
/// points, expérience, points héroïques).
pub const DIRT_SKILL_LEVEL: usize = 4;
pub const DIRT_SKILL_POINT: usize = 5;
pub const DIRT_HEROIC_POINT: usize = 7;
/// Marge ajoutée à la portée, pour les positions arrondies du client.
const REACH_TOLERANCE: f64 = 50.0;

pub mod op {
    pub const ADD: i64 = 0;
    pub const UPDATE: i64 = 2;
    pub const CAST: i64 = 7;
    pub const CAST_RESULT: i64 = 8;
}

pub mod action {
    pub const CAST: i64 = 1;
    pub const RESULT: i64 = 2;
}

pub mod master_op {
    pub const LEARN: i64 = 1;
    pub const REQUEST_UPGRADE: i64 = 2;
    pub const RESPONSE_UPGRADE: i64 = 3;
    pub const REQUEST_EVENT: i64 = 4;
    pub const RESPONSE_EVENT: i64 = 5;
    pub const LEARN_RESULT: i64 = 6;
}

pub mod learn_result {
    pub const SUCCESS: i64 = 0;
    pub const FAIL: i64 = 1;
    pub const ALREADY_LEARNED: i64 = 2;
    pub const NOT_ENOUGH_POINTS: i64 = 3;
    pub const LOW_LEVEL: i64 = 4;
    pub const WRONG_CLASS: i64 = 5;
    /// Codes du contrôle du client (`Archonia.exe`), que sa fenêtre d'apprentissage n'affiche pas : un message
    /// système les accompagne.
    pub const NOT_ENOUGH_HEROIC_POINTS: i64 = 6;
    pub const NOT_ENOUGH_CHARISMA: i64 = 7;
}

/// Résultats d'une amélioration (AGPMEVENTSKILLMASTER op 3, champ 5), affichés par le client.
pub mod upgrade_result {
    pub const SUCCESS: i64 = 0;
    pub const NOT_ENOUGH_MONEY: i64 = 1;
    pub const NOT_ENOUGH_POINTS: i64 = 2;
    pub const NOT_ENOUGH_HEROIC_POINTS: i64 = 3;
    pub const NOT_ENOUGH_CHARISMA: i64 = 4;
}

/// Colonnes « require_class_* » de SKILL_CONST, dans l'ordre.
pub const CLASS_COLUMNS: [&str; 13] =
    ["knight", "archer", "wizard", "berserker", "hunter", "sorcerer", "swashbuckler", "ranger", "elementaler", "slayer", "obiter", "summoner", "scion"];

/// Éléments des dégâts magiques : colonne de SKILL_CONST, indice des dégâts du personnage (attributs), indice
/// dans le résultat (facteurs « points » : 10 magie … 17 glace).
const SPIRIT: [(&str, usize, usize); 8] = [
    ("magic", 1, 10),
    ("water", 2, 11),
    ("fire", 3, 12),
    ("earth", 4, 13),
    ("air", 5, 14),
    ("poison", 6, 15),
    ("thunder", 7, 16),
    ("ice", 8, 17),
];
/// Indice des dégâts physiques dans le résultat.
const RESULT_PHYSICAL: usize = 9;

/// Colonne de classe d'un personnage (race 1 humain, 2 orc, 3 elfe de lune, 4 scion ; classe 1 guerrier,
/// 2 archer, 3 scion, 4 mage).
pub fn class_column(race: i32, class: i32) -> Option<usize> {
    let col = match (race, class) {
        (1, 1) => "knight",
        (1, 2) => "archer",
        (1, 4) => "wizard",
        (2, 1) => "berserker",
        (2, 2) => "hunter",
        (2, 4) => "sorcerer",
        (3, 1) => "swashbuckler",
        (3, 2) => "ranger",
        (3, 4) => "elementaler",
        (4, 1) => "slayer",
        (4, 2) => "obiter",
        (4, 4) => "summoner",
        (4, _) => "scion",
        _ => return None,
    };
    CLASS_COLUMNS.iter().position(|c| *c == col)
}

/// Points de compétence gagnés à un niveau : un par niveau, plus un par niveau de 50 à 70.
pub fn points_for_level(level: i32) -> i32 {
    level + (level - 49).clamp(0, 21)
}

/// Points héroïques gagnés à un niveau : 1 au niveau 90, puis 2 par niveau jusqu'au 110 et 3 par niveau
/// jusqu'au 120.
pub fn heroic_points_for_level(level: i32) -> i32 {
    if level < 90 {
        return 0;
    }
    1 + (level.min(110) - 90) * 2 + (level.min(120) - 110).max(0) * 3
}

/// Valeurs d'un niveau de compétence (SKILL_CONST).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillLevel {
    /// Bit n : la classe `CLASS_COLUMNS[n]` peut apprendre ce niveau.
    pub classes: u16,
    pub require_level: i32,
    pub require_point: i32,
    /// Points héroïques exigés (compétences héroïques).
    pub require_heroic: i32,
    /// Charisme exigé.
    pub require_charisma: i32,
    pub learn_cost: i64,
    pub upgrade_cost: i64,
    pub cost_hp: i32,
    pub cost_mp: i32,
    pub cast_ms: u64,
    pub recast_ms: u64,
    pub duration_ms: u64,
    /// Portée en unités du jeu (la table l'exprime en mètres) ; 0 : portée d'attaque du personnage.
    pub range: f64,
    pub damage: f64,
    pub damage_pct: f64,
    /// Dégâts élémentaires (fixes, pour cent), dans l'ordre de `SPIRIT`.
    pub spirit: [(f64, f64); 8],
    /// Zone : rayon d'une sphère, longueur et largeur d'une boîte (unités du jeu), cibles au plus.
    pub area_r: f64,
    pub area_f1: f64,
    pub area_f2: f64,
    pub max_targets: usize,
    /// Effet durable : modificateurs, soin à l'application, dégâts sur la durée.
    pub mods: Mods,
    pub heal: Heal,
    pub dot: DotSpec,
    /// États spéciaux posés (bits de `effects::status`), chance sur une cible (0 : toujours) et durée.
    pub status: u64,
    pub status_rate: f64,
    pub status_ms: u64,
    /// Chance de la compétence (`skill_rate`) et affaiblissement (`buff_type` 1) : servent aux compétences des
    /// monstres sans SKILL_SPEC, posées sur la cible d'un coup.
    pub rate: f64,
    pub debuff: bool,
    /// Purge (bits de `effects::dispel`) et, pour une purge répétée, sa période (`interval`, 1000 ms à défaut).
    pub dispel: u64,
    pub purge_every_ms: Option<u64>,
}

/// Champs de SKILL_SPEC qui règlent les valeurs d'un niveau.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spec {
    pub melee_attack: i64,
    pub reflect_melee_attack: i64,
    pub convert_type: i64,
    /// defense_melee_attack (2 : contre-attaque).
    pub defense_melee_attack: i64,
    pub status: u64,
    pub offensive: bool,
    /// divide (partage des dégâts) et dispel_magic (purge), bruts.
    pub divide: i64,
    pub dispel_magic: i64,
}

/// Bits de la colonne reflect_melee_attack (au-delà des valeurs exactes 1, renvoi, et 2, bouclier).
pub mod reflect {
    pub const IGNORE_DEFENSE: i64 = 0x4;
    pub const IGNORE_RESIST: i64 = 0x8;
}

/// Bits de la colonne divide : nature des dégâts partagés et qui les prend.
pub mod divide {
    pub const SPIRIT: i64 = 0x1;
    pub const PHYSICAL: i64 = 0x2;
    /// Le lanceur prend la part des membres de son groupe (Divine Protection, Martyr's Mark).
    pub const CASTER: i64 = 0x10;
}

impl SkillLevel {
    #[cfg(test)]
    pub fn from_consts(c: &serde_json::Value) -> Self {
        Self::with_spec(c, &Spec::default())
    }

    pub fn with_spec(c: &serde_json::Value, sp: &Spec) -> Self {
        let num = |k: &str| c.get(k).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let mut classes = 0u16;
        for (i, name) in CLASS_COLUMNS.iter().enumerate() {
            if num(&format!("require_class_{name}")) != 0.0 {
                classes |= 1 << i;
            }
        }
        let mut spirit = [(0.0, 0.0); 8];
        for (i, (name, _, _)) in SPIRIT.iter().enumerate() {
            spirit[i] = (num(&format!("damage_{name}")), num(&format!("damage_{name}_PERCENT")));
        }
        SkillLevel {
            classes,
            require_level: num("require_level") as i32,
            require_point: num("require_point") as i32,
            require_heroic: num("require_heroic_point") as i32,
            require_charisma: num("require_charisma_point") as i32,
            learn_cost: num("skill_cost") as i64,
            upgrade_cost: num("skill_upgrade_cost") as i64,
            cost_hp: num("cost_hp") as i32,
            cost_mp: num("cost_mp") as i32,
            cast_ms: num("cast_time").max(0.0) as u64,
            recast_ms: num("recast_time").max(0.0) as u64,
            duration_ms: num("duration").max(0.0) as u64,
            range: num("range") * 100.0,
            damage: num("damage_a"),
            damage_pct: num("damage_a_PERCENT"),
            spirit,
            area_r: num("area_r") * 100.0,
            area_f1: num("area_f1") * 100.0,
            area_f2: num("area_f2") * 100.0,
            max_targets: num("max_target_num").max(0.0) as usize,
            mods: combat_mods(c, sp),
            heal: Heal::from_consts(c),
            dot: DotSpec::from_consts(c),
            status: sp.status,
            status_rate: if sp.offensive { num("skill_rate") } else { 0.0 },
            status_ms: if sp.status & effects::status::STUN != 0 && num("stun_time") > 0.0 { num("stun_time") } else { num("duration").max(0.0) } as u64,
            rate: num("skill_rate"),
            debuff: num("buff_type") == 1.0,
            dispel: effects::parse_dispel(sp.dispel_magic).0,
            purge_every_ms: effects::parse_dispel(sp.dispel_magic).1.then(|| if num("interval") > 0.0 { num("interval") as u64 } else { 1000 }),
        }
    }

    pub fn allows(&self, class_col: Option<usize>) -> bool {
        class_col.is_some_and(|c| self.classes & (1 << c) != 0)
    }

    /// Laisse un effet durable sur ses cibles.
    pub fn lasting(&self) -> bool {
        self.duration_ms > 0 && (!self.mods.is_empty() || !self.dot.is_empty() || self.purge_every_ms.is_some() && self.dispel != 0)
    }

    /// Laisse un effet, compte tenu des états effectivement posés.
    pub fn leaves_effect(&self, status: u64) -> bool {
        self.lasting() || status != 0 && self.status_ms > 0
    }
}

/// Modificateurs d'un niveau, avec les effets de combat que règle SKILL_SPEC.
fn combat_mods(c: &serde_json::Value, sp: &Spec) -> Mods {
    let num = |k: &str| c.get(k).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0);
    let rate = num("skill_rate");
    let mut m = Mods::from_consts(c);
    if matches!(sp.melee_attack, 106 | 109 | 110) {
        m.crit_rate += rate;
        m.crit_dmg += num("critical");
    }
    if sp.reflect_melee_attack == 2 {
        m.block += rate;
    }
    let convert_rate = if rate > 0.0 { rate } else { num("special_status_rate") };
    match sp.convert_type {
        1 => (m.to_hp_rate, m.to_hp) = (convert_rate, num("damage_convert_hp")),
        2 => (m.to_mp_rate, m.to_mp) = (convert_rate, num("damage_convert_mp")),
        _ => {}
    }
    if sp.status & effects::status::STUN_PROTECT != 0 {
        m.stun_protect += rate;
    }
    if sp.defense_melee_attack == 2 {
        m.counter += rate;
    }
    // Défense ignorée : montants de Skill_Const2.txt (colonnes après FACTOR_PERCENT2, d'où le préfixe).
    if sp.reflect_melee_attack > 2 {
        if sp.reflect_melee_attack & reflect::IGNORE_DEFENSE != 0 {
            m.ignore_defense += num("percent2.ignore_physical_defence_percent2");
        }
        if sp.reflect_melee_attack & reflect::IGNORE_RESIST != 0 {
            m.ignore_resist += num("percent2.ignore_attribute_defence_percent2");
        }
    }
    if sp.divide & divide::CASTER != 0 {
        let share = num("divide_rate");
        if sp.divide & divide::PHYSICAL != 0 {
            m.share_physical += share;
        }
        if sp.divide & divide::SPIRIT != 0 {
            m.share_spirit += share;
        }
    }
    m
}

/// Forme de la zone d'une compétence (champ `range` de SKILL_SPEC : au-delà de 1000, centrée sur la cible ;
/// centaines 3 sphère, 4 boîte).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Single,
    Sphere { around_target: bool },
    Line,
}

pub fn area(range_code: i64) -> Area {
    let around_target = range_code > 1000;
    let c = range_code % 1000;
    match if c >= 100 { c / 100 } else { c } {
        3 => Area::Sphere { around_target },
        4 => Area::Line,
        _ => Area::Single,
    }
}

/// Monstres touchés par une zone : sphère de rayon `r` autour de `center`, ou boîte de `from` vers
/// `toward` (longueur, largeur).
pub fn in_area(shape: Area, m: Vec3, center: Vec3, from: Vec3, toward: Vec3, lv: &SkillLevel) -> bool {
    match shape {
        Area::Single => false,
        Area::Sphere { .. } => lv.area_r > 0.0 && combat::dist2d(m, center) <= lv.area_r,
        Area::Line => {
            let (length, width) = (if lv.area_f1 > 0.0 { lv.area_f1 } else { 600.0 }, if lv.area_f2 > 0.0 { lv.area_f2 } else { 300.0 });
            let (dx, dz) = ((toward.x - from.x) as f64, (toward.z - from.z) as f64);
            let n = (dx * dx + dz * dz).sqrt();
            if n == 0.0 {
                return false;
            }
            let (ux, uz) = (dx / n, dz / n);
            let (vx, vz) = ((m.x - from.x) as f64, (m.z - from.z) as f64);
            let along = vx * ux + vz * uz;
            let across = (vx * uz - vz * ux).abs();
            (0.0..=length).contains(&along) && across <= width / 2.0
        }
    }
}

/// Une compétence du catalogue (SKILL_SPEC et ses niveaux).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillTemplate {
    pub tid: u32,
    pub name: String,
    /// Champs bruts de SKILL_SPEC utiles ici.
    pub attribute: i64,
    pub upgradable: i64,
    pub target: i64,
    /// Champ `range` de SKILL_SPEC (forme de la zone).
    pub range_code: i64,
    pub melee_attack: i64,
    pub auto_attack: bool,
    /// États spéciaux (colonne special_status).
    pub status: u64,
    /// Niveau n à l'indice n - 1.
    pub levels: Vec<SkillLevel>,
}

/// Nature d'une compétence, d'après le champ `attribute` de SKILL_SPEC : chiffres de poids décroissant
/// (dizaines de milliers : sauvegarde, milliers : magie…), dizaines : renforcement (1) ou affaiblissement (2),
/// unités : dégâts physiques (1), magiques (2 à 6), production (8), passive (9).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nature {
    pub buff: bool,
    pub debuff: bool,
    pub passive: bool,
    pub damage: bool,
}

pub fn nature(attribute: i64) -> Nature {
    let tens = (attribute % 100) / 10;
    let units = attribute % 10;
    Nature { buff: tens == 1, debuff: tens >= 2, passive: units == 9, damage: (1..=6).contains(&units) }
}

impl SkillTemplate {
    pub fn level(&self, n: u32) -> Option<&SkillLevel> {
        self.levels.get((n as usize).checked_sub(1)?)
    }
    pub fn nature(&self) -> Nature {
        nature(self.attribute)
    }
    /// Dégâts immédiats à l'impact (colonne melee_attack : types 101, 102, 103, 108 et variantes critiques).
    pub fn instant_damage(&self) -> bool {
        matches!(self.melee_attack, 101 | 102 | 103 | 108 | 109 | 110)
    }
    /// Vise des ennemis (unités du champ target) ; sinon, son lanceur.
    pub fn offensive(&self) -> bool {
        self.target % 10 == 1
    }
    /// Peut viser un autre joueur (dizaines du champ target : Assistance, Disentangle).
    pub fn on_friend(&self) -> bool {
        self.target / 10 % 10 == 1
    }
    /// Touche aussi son lanceur (centaines du champ target), en plus de ses cibles.
    pub fn on_self(&self) -> bool {
        self.target / 100 % 10 == 1
    }
    pub fn area(&self) -> Area {
        area(self.range_code)
    }
    /// Se lance sans cible désignée : zone autour du lanceur, ou compétence non offensive.
    pub fn needs_target(&self) -> bool {
        self.offensive() && self.area() != Area::Sphere { around_target: false }
    }
    pub fn can_upgrade(&self) -> bool {
        matches!(self.upgradable, 1 | 3)
    }
    /// Compétence de groupe : amie, en sphère autour du lanceur réservée aux membres de son groupe (zone 303).
    pub fn party(&self) -> bool {
        !self.offensive() && self.range_code % 1000 == 303
    }
}

/// Place d'une compétence dans un arbre de maîtrise (SkillMastery_High.txt, SkillMastery_Heroic.txt).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mastery {
    /// Classe de l'arbre (indice de `CLASS_COLUMNS`), `None` si son modèle est inconnu.
    pub class: Option<usize>,
    pub heroic: bool,
    /// Compétences exigées : (TID, points), les points d'une compétence étant son niveau.
    pub requires: Vec<(u32, u32)>,
    /// Points dépensés en tout : points de compétence, ou points héroïques dans l'arbre héroïque.
    pub total: i32,
}

#[derive(Default)]
pub struct SkillCatalog {
    skills: HashMap<u32, SkillTemplate>,
    /// Arbres de maîtrise, par TID de compétence (une compétence peut figurer dans l'arbre de plusieurs classes).
    masteries: HashMap<u32, Vec<Mastery>>,
}

impl SkillCatalog {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let Ok(specs) = db.world.rows("SELECT tid, name, spec FROM skill_spec", ()).await else {
            warn!("compétences non chargées : lancer `archcore db-init` puis `archcore import`");
            return Ok(SkillCatalog::default());
        };
        let mut skills: HashMap<u32, SkillTemplate> = HashMap::new();
        let mut spec_of: HashMap<u32, Spec> = HashMap::new();
        for r in &specs {
            let spec = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "spec")).unwrap_or_default();
            let int = |k: &str| spec.get(k).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0);
            let tid: u32 = col(r, "tid");
            let raw_status = spec.get("special_status").and_then(|v| v.as_str()).unwrap_or("");
            spec_of.insert(
                tid,
                Spec {
                    melee_attack: int("melee_attack"),
                    reflect_melee_attack: int("reflect_melee_attack"),
                    convert_type: int("convert_type"),
                    defense_melee_attack: int("defense_melee_attack"),
                    status: effects::parse_status(raw_status),
                    offensive: int("target") % 10 == 1,
                    divide: int("divide"),
                    dispel_magic: int("dispel_magic"),
                },
            );
            skills.insert(
                tid,
                SkillTemplate {
                    tid,
                    name: col(r, "name"),
                    attribute: int("attribute"),
                    upgradable: int("upgradable"),
                    target: int("target"),
                    range_code: int("range"),
                    melee_attack: int("melee_attack"),
                    auto_attack: int("auto_attack") != 0,
                    status: effects::parse_status(raw_status),
                    levels: Vec::new(),
                },
            );
        }
        for r in db.world.rows("SELECT tid, level, consts FROM skill_level ORDER BY tid, level", ()).await? {
            let Some(t) = skills.get_mut(&col::<u32>(&r, "tid")) else { continue };
            let level: u32 = col(&r, "level");
            let consts = serde_json::from_str::<serde_json::Value>(&col::<String>(&r, "consts")).unwrap_or_default();
            if level as usize == t.levels.len() + 1 {
                t.levels.push(SkillLevel::with_spec(&consts, &spec_of.get(&t.tid).copied().unwrap_or_default()));
            }
        }
        let damage = skills.values().filter(|s| s.instant_damage()).count();
        // Maîtrises : la classe de l'arbre vient de la race et de la classe de son modèle (CharacterDataTable).
        let mut masteries: HashMap<u32, Vec<Mastery>> = HashMap::new();
        let sql = "SELECT m.skill_tid, m.tree, m.requires, m.total_points, JSON_VALUE(d.data, '$.Race') AS race, JSON_VALUE(d.data, '$.Class') AS class \
                   FROM skill_mastery m LEFT JOIN character_data_table d ON d.tid = m.class_tid";
        match db.world.rows(sql, ()).await {
            Ok(rows) => {
                for r in &rows {
                    let int = |k: &str| col::<Option<String>>(r, k).and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(0);
                    let requires: Vec<(u32, u32)> = serde_json::from_str(&col::<String>(r, "requires")).unwrap_or_default();
                    let m = Mastery { class: class_column(int("race"), int("class")), heroic: col::<i32>(r, "tree") == 2, requires, total: col(r, "total_points") };
                    masteries.entry(col(r, "skill_tid")).or_default().push(m);
                }
            }
            Err(_) => warn!("maîtrises non chargées : lancer `archcore db-init` puis `archcore import`"),
        }
        info!("compétences : {} ({damage} à dégâts immédiats), {} dans les arbres de maîtrise", skills.len(), masteries.len());
        Ok(SkillCatalog { skills, masteries })
    }

    pub fn get(&self, tid: u32) -> Option<&SkillTemplate> {
        self.skills.get(&tid)
    }

    /// TID d'une compétence d'après son nom (le plus petit si le nom revient).
    pub fn by_name(&self, name: &str) -> Option<u32> {
        self.skills.values().filter(|t| t.name == name).map(|t| t.tid).min()
    }

    /// Place de la compétence dans l'arbre de la classe `class_col` (ou dans le seul arbre qui la porte).
    pub fn mastery(&self, tid: u32, class_col: Option<usize>) -> Option<&Mastery> {
        let list = self.masteries.get(&tid)?;
        list.iter().find(|m| m.class.is_some() && m.class == class_col).or_else(|| (list.len() == 1).then(|| &list[0]))
    }

    #[cfg(test)]
    pub fn insert_mastery(&mut self, tid: u32, m: Mastery) {
        self.masteries.entry(tid).or_default().push(m);
    }

    #[cfg(test)]
    pub fn insert(&mut self, t: SkillTemplate) {
        self.skills.insert(t.tid, t);
    }
}

/// Une compétence connue d'un joueur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Owned {
    /// Identifiant attribué à l'entrée en jeu (unique sur le serveur).
    pub id: u32,
    pub tid: u32,
    pub level: u32,
    /// Fin du délai de relance.
    pub ready_at: Option<Instant>,
}

/// Lancement en cours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cast {
    pub skill_id: u32,
    pub target: u32,
    pub stage: Stage,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// En route vers la cible, portée vérifiée à cet instant.
    Approach { check_at: Instant },
    /// Incantation, effet à cet instant.
    Casting { complete_at: Instant },
}

/// Compétences d'un joueur.
#[derive(Clone, Debug, Default)]
pub struct PlayerSkills {
    pub list: Vec<Owned>,
    pub cast: Option<Cast>,
    /// Effets portés (renforcements, affaiblissements, passives).
    pub effects: Vec<Effect>,
    /// États spéciaux annoncés aux clients.
    pub status_sent: u64,
}

impl PlayerSkills {
    pub fn by_id(&self, id: u32) -> Option<&Owned> {
        self.list.iter().find(|s| s.id == id)
    }
    pub fn by_tid(&self, tid: u32) -> Option<&Owned> {
        self.list.iter().find(|s| s.tid == tid)
    }

    /// Points dépensés : somme des points exigés par chaque niveau appris.
    pub fn spent(&self, catalog: &SkillCatalog) -> i32 {
        self.list
            .iter()
            .filter_map(|s| catalog.get(s.tid).map(|t| (1..=s.level).filter_map(|n| t.level(n)).map(|l| l.require_point).sum::<i32>()))
            .sum()
    }

    /// Points héroïques dépensés, de la même façon.
    pub fn spent_heroic(&self, catalog: &SkillCatalog) -> i32 {
        self.list
            .iter()
            .filter_map(|s| catalog.get(s.tid).map(|t| (1..=s.level).filter_map(|n| t.level(n)).map(|l| l.require_heroic).sum::<i32>()))
            .sum()
    }
}

/// Ce qui manque pour apprendre une compétence de son arbre de maîtrise, sinon `None`.
pub fn mastery_unmet(catalog: &SkillCatalog, skills: &PlayerSkills, m: &Mastery) -> Option<String> {
    for &(tid, points) in &m.requires {
        if skills.by_tid(tid).is_none_or(|s| s.level < points) {
            let name = catalog.get(tid).map(|t| t.name.clone()).unwrap_or_else(|| format!("la compétence {tid}"));
            return Some(format!("il faut {name} au niveau {points}"));
        }
    }
    let (spent, what) = if m.heroic { (skills.spent_heroic(catalog), "points héroïques") } else { (skills.spent(catalog), "points de compétence") };
    (spent < m.total).then(|| format!("il faut {} {what} dépensés en tout ({spent} pour l'instant)", m.total))
}

/// Compétences d'un personnage : « Normal Attack » puis celles de la base.
pub async fn load(db: &Databases, catalog: &SkillCatalog, character_id: u32, next_id: &mut u32) -> anyhow::Result<PlayerSkills> {
    let rows = db.characters.rows("SELECT skill_tid, level FROM character_skills WHERE character_id = ? ORDER BY skill_tid", (character_id,)).await?;
    let mut skills = PlayerSkills::default();
    let mut add = |tid: u32, level: u32| {
        *next_id += 1;
        skills.list.push(Owned { id: *next_id, tid, level, ready_at: None });
    };
    if catalog.get(NORMAL_ATTACK).is_some() {
        add(NORMAL_ATTACK, 1);
    }
    for r in &rows {
        let tid: u32 = col(r, "skill_tid");
        if tid != NORMAL_ATTACK && catalog.get(tid).is_some() {
            add(tid, col::<u32>(r, "level").max(1));
        }
    }
    Ok(skills)
}

/// Recrée les effets des compétences passives connues (niveau courant). Rend vrai si la liste a changé.
pub fn sync_passives(catalog: &SkillCatalog, skills: &mut PlayerSkills, caster_tid: u32) -> bool {
    let before: Vec<(u32, u32)> = skills.effects.iter().filter(|e| e.ends_at.is_none()).map(|e| (e.tid, e.level)).collect();
    skills.effects.retain(|e| e.ends_at.is_some());
    for s in &skills.list {
        let Some(t) = catalog.get(s.tid).filter(|t| t.nature().passive) else { continue };
        let Some(lv) = t.level(s.level).filter(|lv| !lv.mods.is_empty()) else { continue };
        effects::push(
            &mut skills.effects,
            Effect { skill_id: s.id, tid: s.tid, level: s.level, caster_tid, duration_ms: 0, ends_at: None, mods: lv.mods, dot: None, status: 0, status_until: None, caster_sid: effects::MONSTER_CASTER, purge: None },
        );
    }
    let after: Vec<(u32, u32)> = skills.effects.iter().filter(|e| e.ends_at.is_none()).map(|e| (e.tid, e.level)).collect();
    before != after
}

/// Charisme d'un personnage : caractéristique CHA des facteurs, en points bruts (pas × 100 comme les autres).
pub fn charisma_points(f: &Factors) -> i32 {
    f.status[4]
}

/// Points de compétence et points héroïques restants, recopiés dans les facteurs du personnage.
pub fn refresh_points(catalog: &SkillCatalog, skills: &PlayerSkills, factors: &mut Factors) {
    factors.dirt[DIRT_SKILL_POINT] = (points_for_level(factors.status[7]) - skills.spent(catalog)).max(0);
    factors.dirt[DIRT_HEROIC_POINT] = (heroic_points_for_level(factors.status[7]) - skills.spent_heroic(catalog)).max(0);
}

// ---------------------------------------------------------------------------------------------
// Paquets
// ---------------------------------------------------------------------------------------------

fn base(id: u32) -> Vec<u8> {
    let s = &mini::SKILL_BASE;
    build_mini(s, &Fields::new(s).with(0, BASE_CHARACTER).with(1, id))
}

/// Facteurs d'une compétence : sous-paquet « dirt » avec son niveau et ses points.
fn skill_factors(level: u32) -> Vec<u8> {
    let d = &mini::FACTOR_DIRT;
    let dirt = build_mini(d, &Fields::new(d).with(DIRT_SKILL_LEVEL, level as i64).with(DIRT_SKILL_POINT, level as i64).with(6, 0));
    let cf = &mini::CHAR_FACTORS;
    build_mini(cf, &Fields::new(cf).with(11, dirt))
}

/// AGPMSKILL op 0 : une compétence du personnage `owner`.
pub fn add_packet(owner: u32, s: &Owned) -> Vec<u8> {
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::ADD).with(1, s.id).with(2, base(owner)).with(3, skill_factors(s.level)).with(4, s.tid).with(5, 0))
}

/// AGPMSKILL op 2 : niveau d'une compétence changé.
pub fn update_packet(owner: u32, s: &Owned) -> Vec<u8> {
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::UPDATE).with(1, s.id).with(2, base(owner)).with(3, skill_factors(s.level)))
}

/// AGPMSKILL op 7 : début du lancement, diffusé autour du lanceur.
pub fn cast_packet(caster: u32, s: &Owned, target: u32, target_pos: Vec3, lv: &SkillLevel) -> Vec<u8> {
    let a = &mini::SKILL_ACTION;
    let act = build_mini(
        a,
        &Fields::new(a)
            .with(0, action::CAST)
            .with(1, base(target))
            .with(3, target_pos)
            .with(5, lv.cast_ms as i64)
            .with(6, lv.duration_ms as i64)
            .with(7, lv.recast_ms as i64)
            .with(8, s.level as i64)
            .with(9, 1),
    );
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::CAST).with(1, s.id).with(2, base(caster)).with(4, s.tid).with(6, act))
}

/// Facteurs du résultat : PV et PM restants, dégâts en négatif aux indices de leur nature.
pub fn result_factors(hp: i32, mp: i32, damages: &[(usize, i32)]) -> Vec<u8> {
    let pt = &mini::FACTOR_POINT;
    let mut shown = Fields::new(pt).with(0, hp).with(1, mp);
    for (i, d) in damages {
        shown.set(*i, -*d);
    }
    let cf = &mini::CHAR_FACTORS;
    let result = build_mini(cf, &Fields::new(cf).with(3, build_mini(pt, &shown)));
    let current = build_mini(pt, &Fields::new(pt).with(0, hp).with(1, mp));
    build_mini(cf, &Fields::new(cf).with(0, result).with(3, current))
}

/// AGPMSKILL op 8 : effet sur la cible.
pub fn result_packet(caster: u32, s: &Owned, target: u32, factors: Vec<u8>) -> Vec<u8> {
    let a = &mini::SKILL_ACTION;
    let act = build_mini(a, &Fields::new(a).with(0, action::RESULT).with(1, base(target)).with(2, factors).with(8, s.level as i64).with(9, 1).with(10, 0));
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::CAST_RESULT).with(1, s.id).with(2, base(caster)).with(4, s.tid).with(6, act))
}

/// Champ 8 de la vue optimisée op 2 : toutes les compétences du joueur.
pub fn all_packets(p: &Player) -> Vec<u8> {
    p.skills.list.iter().flat_map(|s| add_packet(p.ch.id, s)).collect()
}

// ---------------------------------------------------------------------------------------------
// Dégâts
// ---------------------------------------------------------------------------------------------

/// Dégâts d'une compétence : (indice dans le résultat, dégâts). La part physique reprend le jet d'attaque
/// du personnage, majoré du pourcentage et du bonus du niveau ; chaque élément reprend les dégâts
/// élémentaires du personnage de la même façon, doublés, moins la résistance de la cible à l'élément. Une
/// cible plus forte retire 5 % par niveau d'écart (80 % au plus) ; l'armure ne réduit que la part physique. Chaque part fait au moins 1.
pub fn skill_damage(rng: &mut impl Rng, a: &Attacker, spirit_ranges: &[(f64, f64); 8], d: &Defender, lv: &SkillLevel) -> Vec<(usize, i32)> {
    let penalty = (5 * (d.level - a.level).max(0)).min(80) as f64;
    let keep = (100.0 - penalty) / 100.0;
    let roll = |rng: &mut dyn rand::RngCore, (lo, hi): (f64, f64)| if hi > lo { rng.gen_range(lo..=hi) } else { lo };
    let mut out = Vec::new();
    let has_spirit = lv.spirit.iter().any(|(f, p)| *f != 0.0 || *p != 0.0);
    if lv.damage != 0.0 || lv.damage_pct != 0.0 || !has_spirit {
        let raw = roll(rng, a.damage_range());
        let x = (raw * (100.0 + lv.damage_pct) / 100.0 + lv.damage) * keep - d.defense;
        out.push((RESULT_PHYSICAL, (x.round() as i32).max(1)));
    }
    for (i, (flat, pct)) in lv.spirit.iter().enumerate() {
        if *flat == 0.0 && *pct == 0.0 {
            continue;
        }
        let raw = roll(rng, spirit_ranges[i]);
        let x = (raw * (100.0 + pct) / 100.0 + flat) * keep * 2.0 * (100 - d.resist[i]).max(0) as f64 / 100.0;
        out.push((SPIRIT[i].2, (x.round() as i32).max(1)));
    }
    out
}

/// Fourchettes de dégâts élémentaires d'un joueur, dans l'ordre de `SPIRIT` (attributs 1 à 8).
fn spirit_ranges(f: &Factors) -> [(f64, f64); 8] {
    super::combat::spirit_ranges(f)
}

// ---------------------------------------------------------------------------------------------
// Lancement
// ---------------------------------------------------------------------------------------------

/// AGPMSKILL reçu du client.
pub fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    match p.int(0) {
        Some(op::CAST) => request_cast(ctx, st, sid, p),
        other => debug!("[compétence] #{sid} AGPMSKILL op {other:?} (ignoré)"),
    }
}

fn request_cast(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let skill_id = p.int(1).unwrap_or(0) as u32;
    let target = p
        .bytes(6)
        .and_then(|b| decode_mini(&mini::SKILL_ACTION, b).ok())
        .and_then(|a| a.bytes(1).and_then(|b| decode_mini(&mini::SKILL_BASE, b).ok()))
        .and_then(|b| b.int(1))
        .unwrap_or(0) as u32;
    let now = Instant::now();
    let Some(player) = st.players.get(&sid) else { return };
    if !player.in_game || player.dead {
        return;
    }
    let name = player.ch.name.clone();
    let Some(owned) = player.skills.by_id(skill_id).copied() else {
        warn!("[compétence] {name} lance la compétence {skill_id}, qu'il ne connaît pas");
        return;
    };
    let Some(t) = ctx.skills.get(owned.tid) else { return };
    if t.auto_attack || t.nature().passive {
        return;
    }
    if matches!(player.skills.cast, Some(Cast { stage: Stage::Casting { .. }, .. })) {
        debug!("[compétence] {name} : incantation déjà en cours");
        return;
    }
    if effects::active_status(&player.skills.effects, now) & effects::status::NO_SKILL != 0 {
        debug!("[compétence] {name} : compétences interdites (état spécial)");
        return;
    }
    if owned.ready_at.is_some_and(|r| now < r) {
        debug!("[compétence] {name} : {} pas encore prête", t.name);
        return;
    }
    let alive = st.monsters.get(target).is_some_and(|m| m.is_alive() && st.monsters.stats_of(m.tid).is_attackable());
    let target = if t.offensive() && alive {
        target
    } else if t.needs_target() {
        debug!("[compétence] {name} : {} vise {target:#x}, qui n'est pas un monstre vivant", t.name);
        return;
    } else if t.on_friend() && st.by_cid.get(&target).and_then(|s| st.players.get(s)).is_some_and(|f| f.in_game && !f.dead) {
        target
    } else {
        player.ch.id
    };
    let player = st.players.get_mut(&sid).unwrap();
    player.skills.cast = Some(Cast { skill_id, target, stage: Stage::Approach { check_at: now } });
    debug!("[compétence] {name} prépare {} niveau {} sur {target:#x}", t.name, owned.level);
}

/// Avance les lancements : approche, début d'incantation, effet.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    for sid in st.sids() {
        let Some(cast) = st.players.get(&sid).and_then(|p| p.skills.cast) else { continue };
        match cast.stage {
            Stage::Approach { check_at } if now >= check_at => begin(ctx, st, sid, cast, now),
            Stage::Casting { complete_at } if now >= complete_at => complete(ctx, st, sid, cast, now).await?,
            _ => {}
        }
    }
    effects_step(ctx, st, now).await
}

fn stop(st: &mut WorldState, sid: u32) {
    if let Some(p) = st.players.get_mut(&sid) {
        p.skills.cast = None;
    }
}

fn begin(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cast: Cast, now: Instant) {
    let p = &st.players[&sid];
    let Some(owned) = p.skills.by_id(cast.skill_id).copied() else { return stop(st, sid) };
    let Some((t, lv)) = ctx.skills.get(owned.tid).and_then(|t| Some((t, t.level(owned.level)?))) else { return stop(st, sid) };
    if p.dead {
        return stop(st, sid);
    }
    let me = p.ch.pos();
    let friend = st.by_cid.get(&cast.target).filter(|s| **s != sid).and_then(|s| st.players.get(s)).filter(|f| f.in_game && !f.dead).map(|f| f.ch.pos());
    let target: Option<Monster> = if cast.target == p.ch.id || friend.is_some() { None } else { st.monsters.get(cast.target).filter(|m| m.is_alive()).cloned() };
    if cast.target != p.ch.id && target.is_none() && friend.is_none() {
        return stop(st, sid);
    }
    // Joueur ami visé (Assistance, Disentangle) : à portée, sinon refusé.
    if let Some(at) = friend {
        let range = if lv.range > 0.0 { lv.range } else { p.factors.attack[0].max(50) as f64 };
        if combat::dist2d(me, at) > range + REACH_TOLERANCE {
            send_system(st, sid, "Cible trop éloignée.");
            return stop(st, sid);
        }
    }
    if let Some(m) = &target {
        let hit_range = st.monsters.stats_of(m.tid).hit_range as f64;
        let range = if lv.range > 0.0 { lv.range } else { p.factors.attack[0].max(50) as f64 };
        let distance = combat::dist2d(me, m.pos);
        if distance > range + hit_range + REACH_TOLERANCE {
            let travel = combat::approach(st, sid, m, hit_range, now);
            if let Some(c) = st.players.get_mut(&sid).and_then(|p| p.skills.cast.as_mut()) {
                c.stage = Stage::Approach { check_at: now + travel };
            }
            return;
        }
    }
    let f = &p.factors;
    if f.point[1] < lv.cost_mp {
        send_system(st, sid, "Pas assez de PM.");
        return stop(st, sid);
    }
    if lv.cost_hp > 0 && f.point[POINT_HP] <= lv.cost_hp {
        send_system(st, sid, "Pas assez de PV.");
        return stop(st, sid);
    }
    let name = t.name.clone();
    let p = st.players.get_mut(&sid).unwrap();
    p.factors.point[1] -= lv.cost_mp;
    p.factors.point[POINT_HP] -= lv.cost_hp;
    let complete_at = now + Duration::from_millis(lv.cast_ms);
    if let Some(s) = p.skills.list.iter_mut().find(|s| s.id == owned.id) {
        s.ready_at = Some(now + Duration::from_millis(lv.recast_ms.max(lv.cast_ms)));
    }
    p.skills.cast = Some(Cast { stage: Stage::Casting { complete_at }, ..cast });
    if let Some(a) = p.attack.as_mut() {
        a.ready_at = a.ready_at.max(complete_at);
    }
    if target.is_some() {
        super::recovery::in_combat(&mut p.timers, now);
    }
    let cid = p.ch.id;
    let target_pos = target.as_ref().map(|m| m.pos).or(friend).unwrap_or(me);
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, cid).with(7, factors_mini(&p.factors)));
    let pkt = cast_packet(cid, &owned, cast.target, target_pos, lv);
    combat::send_around(st, sid, &pkt);
    debug!("[compétence] {} lance {name} (incantation {} ms, relance {} ms)", st.players[&sid].ch.name, lv.cast_ms, lv.recast_ms);
}

async fn complete(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cast: Cast, now: Instant) -> anyhow::Result<()> {
    stop(st, sid);
    let p = &st.players[&sid];
    let Some(owned) = p.skills.by_id(cast.skill_id).copied() else { return Ok(()) };
    let Some((t, lv)) = ctx.skills.get(owned.tid).and_then(|t| Some((t, t.level(owned.level)?))) else { return Ok(()) };
    if p.dead {
        return Ok(());
    }
    let (caster_tid, me) = (p.ch.template_id, p.dest.unwrap_or_else(|| p.ch.pos()));
    if !t.offensive() {
        // Compétence amie : son lanceur (soin, renforcement) et, pour une compétence de groupe, les membres de son
        // groupe dans la zone.
        let mut friends = vec![st.by_cid.get(&cast.target).copied().filter(|s| st.players.contains_key(s)).unwrap_or(sid)];
        if t.party() {
            friends.extend(party_near(st, sid, me, lv.area_r));
        }
        for friend in friends {
            if !lv.heal.is_empty() {
                effects::heal(&mut st.players.get_mut(&friend).unwrap().factors, &lv.heal);
                super::equipment::refresh_factors(ctx, st, friend);
            }
            if lv.dispel != 0 {
                purge_player(ctx, st, friend, lv.dispel);
            }
            if lv.leaves_effect(lv.status) {
                add_player_effect(ctx, st, friend, Effect { caster_sid: sid, ..new_effect(&owned, lv, caster_tid, lv.status, None, now) });
            }
        }
        return Ok(());
    }
    // Cibles : la cible désignée, puis les monstres de la zone.
    let primary = st.monsters.get(cast.target).filter(|m| m.is_alive()).map(|m| (m.cid, m.pos));
    let center = match (t.area(), primary) {
        (Area::Sphere { around_target: true }, Some((_, pos))) | (Area::Line, Some((_, pos))) => pos,
        _ => me,
    };
    let toward = primary.map(|(_, pos)| pos).unwrap_or(me);
    let mut targets: Vec<u32> = primary.map(|(c, _)| vec![c]).unwrap_or_default();
    let limit = if lv.max_targets > 0 { lv.max_targets } else { 12 };
    if t.area() != Area::Single {
        let radius = lv.area_r.max(lv.area_f1).max(600.0);
        let mut near: Vec<(f64, u32)> = st
            .monsters
            .near(center, radius)
            .filter(|m| m.is_alive() && !targets.contains(&m.cid) && st.monsters.stats_of(m.tid).is_attackable())
            .filter(|m| in_area(t.area(), m.pos, center, me, toward, lv))
            .map(|m| (combat::dist2d(m.pos, center), m.cid))
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        debug!("[compétence] {} : zone {:?} autour de {:.0}, {:.0} (rayon {:.0}) : {} autre(s) monstre(s)", t.name, t.area(), center.x, center.z, lv.area_r, near.len());
        targets.extend(near.into_iter().map(|(_, c)| c));
    }
    targets.truncate(limit);
    let skill_name = t.name.clone();
    for target in targets {
        hit_monster(ctx, st, sid, &owned, t, lv, target, caster_tid, now).await?;
    }
    // Purge qui touche aussi son lanceur (Bow of Meditation : champ target 111).
    if lv.dispel != 0 && t.on_self() {
        purge_player(ctx, st, sid, lv.dispel);
    }
    if let (Some((target, _)), Some(p)) = (primary, st.players.get_mut(&sid)) {
        super::recovery::in_combat(&mut p.timers, now);
        if p.attack.is_none() && st.monsters.get(target).is_some_and(|m| m.is_alive()) {
            p.attack = Some(combat::Attack { target, ready_at: now });
        }
    }
    debug!("[compétence] {} : {skill_name} terminée", st.players.get(&sid).map(|p| p.ch.name.as_str()).unwrap_or("?"));
    Ok(())
}

/// Effet d'une compétence offensive sur un monstre : dégâts immédiats, effet durable.
#[allow(clippy::too_many_arguments)]
async fn hit_monster(ctx: &WorldCtx, st: &mut WorldState, sid: u32, owned: &Owned, t: &SkillTemplate, lv: &SkillLevel, cid: u32, caster_tid: u32, now: Instant) -> anyhow::Result<()> {
    let Some(m) = st.monsters.get(cid).filter(|m| m.is_alive()).cloned() else { return Ok(()) };
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let caster = p.ch.id;
    let stats = st.monsters.stats_of(m.tid);
    let mods = st.monsters.mods(cid);
    let (attacker, spirit) = (Attacker::of_player(p), spirit_ranges(&p.factors));
    let defender = attacker.pierce(&Defender::of_monster(m.level as i32, &stats, &mods));
    let damages = if t.instant_damage() { skill_damage(&mut rand::thread_rng(), &attacker, &spirit, &defender, lv) } else { Vec::new() };
    let total: i32 = damages.iter().map(|(_, d)| *d).sum();
    let target = st.monsters.get_mut(cid).unwrap();
    target.hp = (target.hp - total).max(0);
    let hp = target.hp;
    if total > 0 {
        st.monsters.wake(cid);
    }
    let mut watchers = combat::viewers(st, cid);
    if !watchers.contains(&sid) {
        watchers.push(sid);
    }
    combat::broadcast(st, &watchers, &result_packet(caster, owned, cid, result_factors(hp, stats.max_mp, &damages)));
    super::ai::add_aggro(st, cid, caster, total.max(1) as u64, now);
    debug!("[compétence] {} → {} : {total} dégât(s), {hp}/{} PV", t.name, m.name, m.max_hp);
    if hp == 0 {
        return combat::kill(ctx, st, sid, cid, now).await;
    }
    if lv.dispel != 0 {
        let n = purge_monster(ctx, st, cid, lv.dispel, now);
        debug!("[compétence] {} → {} : purge ({:#x}), {n} effet(s) retiré(s)", t.name, m.name, lv.dispel);
    }
    let status = roll_status(&mut rand::thread_rng(), lv, &mods);
    if lv.leaves_effect(status) {
        let penalty = (5 * (m.level as i32 - attacker.level).max(0)).min(80) as f64;
        let dot = (!lv.dot.is_empty()).then(|| effects::Dot {
            damages: lv.dot.damages.iter().map(|(i, d)| (*i, ((d * (100.0 - penalty) / 100.0).round() as i32).max(1))).collect(),
            interval: Duration::from_millis(lv.dot.interval_ms),
            next_at: now + Duration::from_millis(lv.dot.interval_ms),
            caster_sid: sid,
        });
        let e = Effect { caster_sid: sid, ..new_effect(owned, lv, caster_tid, status, dot, now) };
        let list = st.monsters.effects.entry(cid).or_default();
        if let Some(old) = effects::push(list, e.clone()) {
            combat::broadcast(st, &watchers, &effects::remove_packet(cid, &old));
        }
        combat::broadcast(st, &watchers, &effects::add_packet(cid, &e));
        if status != 0 {
            debug!("[compétence] {} → {} : état {status:#x} pour {} ms", t.name, m.name, lv.status_ms);
        }
        sync_monster_status(st, cid, now);
    }
    Ok(())
}

/// TID des effets posés par les options à déclenchement : aucun paquet d'effet ne les annonce (l'étourdissement
/// part dans l'état du monstre, la blessure dans ses PV).
pub const GEAR_STUN: u32 = u32::MAX;
pub const GEAR_WOUND: u32 = u32::MAX - 1;

/// L'effet est annoncé aux clients (AGPMSKILL op 4 et 5).
pub fn announced(e: &Effect) -> bool {
    e.tid < GEAR_WOUND
}

/// Déclenchements de l'équipement après un coup normal qui touche le monstre `cid` : options qui étourdissent
/// ou blessent, chacune avec sa chance, puis compétences des runes serties (« Skill_Rate »), lancées sur la
/// cible comme une compétence : dégâts élémentaires fixes du niveau (moins la résistance de la cible) et effet
/// (ralentissement, étourdissement, poison).
pub async fn gear_procs(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, now: Instant) -> anyhow::Result<()> {
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    if p.gear.stuns.is_empty() && p.gear.wounds.is_empty() && p.gear.rune_skills.is_empty() {
        return Ok(());
    }
    let gear = p.gear.clone();
    let caster_tid = p.ch.template_id;
    let mods = st.monsters.mods(cid);
    let (put, runes) = {
        let mut rng = rand::thread_rng();
        let mut put = Vec::new();
        for (rate, ms) in &gear.stuns {
            if *ms > 0 && rng.gen_range(0.0..100.0) < *rate && !(mods.stun_protect > 0.0 && rng.gen_range(0.0..100.0) < mods.stun_protect) {
                let until = now + Duration::from_millis(*ms);
                put.push(Effect {
                    skill_id: 0,
                    tid: GEAR_STUN,
                    level: 1,
                    caster_tid,
                    duration_ms: *ms,
                    ends_at: Some(until),
                    mods: Mods::default(),
                    dot: None,
                    status: effects::status::STUN,
                    status_until: Some(until),
                    caster_sid: sid,
                    purge: None,
                });
            }
        }
        for w in &gear.wounds {
            if w.duration_ms > 0 && rng.gen_range(0.0..100.0) < w.rate {
                let every = Duration::from_millis(1000);
                put.push(Effect {
                    skill_id: 0,
                    tid: GEAR_WOUND,
                    level: 1,
                    caster_tid,
                    duration_ms: w.duration_ms,
                    ends_at: Some(now + Duration::from_millis(w.duration_ms)),
                    mods: Mods::default(),
                    dot: Some(effects::Dot { damages: w.damages.clone(), interval: every, next_at: now + every, caster_sid: sid }),
                    status: 0,
                    status_until: None,
                    caster_sid: sid,
                    purge: None,
                });
            }
        }
        let runes: Vec<(u32, u32)> = gear
            .rune_skills
            .iter()
            .filter(|(tid, level, rate)| {
                let rate = if *rate > 0.0 { *rate } else { ctx.skills.get(*tid).and_then(|t| t.level(*level)).map(|lv| lv.rate).unwrap_or(0.0) };
                rng.gen_range(0.0..100.0) < rate
            })
            .map(|(tid, level, _)| (*tid, *level))
            .collect();
        (put, runes)
    };
    if !put.is_empty() {
        let list = st.monsters.effects.entry(cid).or_default();
        for e in put {
            debug!("[équipement] déclenchement sur {cid:#x} : {}", if e.tid == GEAR_STUN { "étourdissement" } else { "blessure" });
            effects::push(list, e);
        }
        sync_monster_status(st, cid, now);
    }
    for (tid, level) in runes {
        if !st.monsters.get(cid).is_some_and(|m| m.is_alive()) {
            break;
        }
        rune_skill(ctx, st, sid, cid, tid, level, now).await?;
    }
    Ok(())
}

/// Compétence d'une rune sur le monstre `cid` : AGPMSKILL op 8 (dégâts élémentaires fixes du niveau, moins la
/// résistance de la cible), puis l'effet du niveau, posé comme celui d'une compétence.
async fn rune_skill(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, tid: u32, level: u32, now: Instant) -> anyhow::Result<()> {
    let Some((t, lv)) = ctx.skills.get(tid).and_then(|t| Some((t, t.level(level)?))) else { return Ok(()) };
    let Some(m) = st.monsters.get(cid).filter(|m| m.is_alive()).cloned() else { return Ok(()) };
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let (caster, caster_tid, attacker) = (p.ch.id, p.ch.template_id, Attacker::of_player(p));
    let stats = st.monsters.stats_of(m.tid);
    let defender = attacker.pierce(&Defender::of_monster(m.level as i32, &stats, &st.monsters.mods(cid)));
    let damages: Vec<(usize, i32)> = lv
        .spirit
        .iter()
        .enumerate()
        .filter(|(_, (flat, _))| *flat > 0.0)
        .map(|(i, (flat, _))| (SPIRIT[i].2, ((flat * (100 - defender.resist[i]).max(0) as f64 / 100.0).round() as i32).max(1)))
        .collect();
    let total: i32 = damages.iter().map(|(_, d)| *d).sum();
    let target = st.monsters.get_mut(cid).unwrap();
    target.hp = (target.hp - total).max(0);
    let hp = target.hp;
    let owned = Owned { id: 0, tid, level, ready_at: None };
    let mut watchers = combat::viewers(st, cid);
    if !watchers.contains(&sid) {
        watchers.push(sid);
    }
    combat::broadcast(st, &watchers, &result_packet(caster, &owned, cid, result_factors(hp, stats.max_mp, &damages)));
    super::ai::add_aggro(st, cid, caster, total.max(1) as u64, now);
    debug!("[équipement] rune : {} → {} ({total} dégât(s), {hp} PV)", t.name, m.name);
    if hp == 0 {
        return combat::kill(ctx, st, sid, cid, now).await;
    }
    let status = roll_status(&mut rand::thread_rng(), &SkillLevel { status_rate: 0.0, ..lv.clone() }, &st.monsters.mods(cid));
    if lv.leaves_effect(status) {
        let dot = (!lv.dot.is_empty()).then(|| effects::Dot {
            damages: lv.dot.damages.iter().map(|(i, d)| (*i, (*d as i32).max(1))).collect(),
            interval: Duration::from_millis(lv.dot.interval_ms),
            next_at: now + Duration::from_millis(lv.dot.interval_ms),
            caster_sid: sid,
        });
        let e = Effect { caster_sid: sid, ..new_effect(&owned, lv, caster_tid, status, dot, now) };
        let list = st.monsters.effects.entry(cid).or_default();
        if let Some(old) = effects::push(list, e.clone()) {
            combat::broadcast(st, &watchers, &effects::remove_packet(cid, &old));
        }
        combat::broadcast(st, &watchers, &effects::add_packet(cid, &e));
        sync_monster_status(st, cid, now);
    }
    Ok(())
}

/// Un effet affaiblit son porteur : compétence offensive ou d'affaiblissement.
pub fn hostile(catalog: &SkillCatalog, e: &Effect) -> bool {
    catalog.get(e.tid).is_some_and(|t| t.offensive() || t.nature().debuff || t.level(e.level).is_some_and(|lv| lv.debuff))
}

/// Purge des effets d'un joueur : retirés (AGPMSKILL op 5 autour de lui), facteurs et états recalculés.
pub fn purge_player(ctx: &WorldCtx, st: &mut WorldState, sid: u32, kinds: u64) -> usize {
    let Some(p) = st.players.get_mut(&sid) else { return 0 };
    let gone = effects::purge(&mut p.skills.effects, kinds, |e| hostile(&ctx.skills, e));
    if gone.is_empty() {
        return 0;
    }
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    for e in &gone {
        combat::send_around(st, sid, &effects::remove_packet(cid, e));
    }
    super::equipment::refresh_factors(ctx, st, sid);
    sync_player_status(st, sid, Instant::now());
    debug!("[compétence] purge de {name} ({kinds:#x}) : {} effet(s) retiré(s)", gone.len());
    gone.len()
}

/// Purge des effets d'un monstre (Antimagic) : retirés chez ceux qui le voient, états recalculés.
pub fn purge_monster(ctx: &WorldCtx, st: &mut WorldState, cid: u32, kinds: u64, now: Instant) -> usize {
    let Some(list) = st.monsters.effects.get_mut(&cid) else { return 0 };
    let gone = effects::purge(list, kinds, |e| hostile(&ctx.skills, e));
    if list.is_empty() {
        st.monsters.effects.remove(&cid);
    }
    let watchers = combat::viewers(st, cid);
    for e in gone.iter().filter(|e| announced(e)) {
        combat::broadcast(st, &watchers, &effects::remove_packet(cid, e));
    }
    sync_monster_status(st, cid, now);
    gone.len()
}

/// États qu'une compétence pose sur une cible : selon sa chance, et l'étourdissement selon la protection.
pub fn roll_status(rng: &mut impl Rng, lv: &SkillLevel, target: &Mods) -> u64 {
    let mut status = lv.status;
    if status != 0 && lv.status_rate > 0.0 && rng.gen_range(0.0..100.0) >= lv.status_rate {
        status = 0;
    }
    if status & effects::status::STUN != 0 && target.stun_protect > 0.0 && rng.gen_range(0.0..100.0) < target.stun_protect {
        status &= !effects::status::STUN;
    }
    status
}

/// Effet d'un niveau de compétence : il dure le plus long de sa durée et de celle de ses états.
pub fn new_effect(owned: &Owned, lv: &SkillLevel, caster_tid: u32, status: u64, dot: Option<effects::Dot>, now: Instant) -> Effect {
    let status_ms = if status != 0 { lv.status_ms } else { 0 };
    let life = lv.duration_ms.max(status_ms);
    Effect {
        skill_id: owned.id,
        tid: owned.tid,
        level: owned.level,
        caster_tid,
        duration_ms: life,
        ends_at: Some(now + Duration::from_millis(life)),
        mods: lv.mods,
        dot,
        status,
        status_until: (status != 0).then(|| now + Duration::from_millis(status_ms)),
        caster_sid: effects::MONSTER_CASTER,
        purge: lv.purge_every_ms.filter(|_| lv.dispel != 0).map(|every| effects::Purge {
            kinds: lv.dispel,
            rate: if lv.rate > 0.0 { lv.rate } else { 100.0 },
            interval: Duration::from_millis(every),
            next_at: now + Duration::from_millis(every),
        }),
    }
}

/// Annonce les états spéciaux d'un joueur s'ils ont changé.
pub fn sync_player_status(st: &mut WorldState, sid: u32, now: Instant) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let status = effects::active_status(&p.skills.effects, now);
    if status == p.skills.status_sent {
        return;
    }
    p.skills.status_sent = status;
    let cid = p.ch.id;
    combat::send_around(st, sid, &effects::status_packet(cid, status));
}

/// Annonce les états spéciaux d'un monstre s'ils ont changé.
pub fn sync_monster_status(st: &mut WorldState, cid: u32, now: Instant) {
    let status = st.monsters.status(cid, now);
    let sent = st.monsters.status_sent.get(&cid).copied().unwrap_or(0);
    if status == sent {
        return;
    }
    if status == 0 {
        st.monsters.status_sent.remove(&cid);
    } else {
        st.monsters.status_sent.insert(cid, status);
    }
    let watchers = combat::viewers(st, cid);
    combat::broadcast(st, &watchers, &effects::status_packet(cid, status));
}

/// Ajoute un effet à un joueur : annoncé autour de lui, facteurs recalculés.
pub fn add_player_effect(ctx: &WorldCtx, st: &mut WorldState, sid: u32, e: Effect) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let cid = p.ch.id;
    if let Some(old) = effects::push(&mut p.skills.effects, e.clone()) {
        combat::send_around(st, sid, &effects::remove_packet(cid, &old));
    }
    combat::send_around(st, sid, &effects::add_packet(cid, &e));
    super::equipment::refresh_factors(ctx, st, sid);
}

/// Sessions des autres membres du groupe de `sid`, en jeu, vivants et à moins de `radius` de `center`.
pub fn party_near(st: &WorldState, sid: u32, center: Vec3, radius: f64) -> Vec<u32> {
    let Some(cid) = st.players.get(&sid).map(|p| p.ch.id) else { return Vec::new() };
    let Some(party) = st.parties.party_of(cid) else { return Vec::new() };
    party
        .members
        .iter()
        .filter(|m| **m != cid)
        .filter_map(|m| st.by_cid.get(m).copied())
        .filter(|s| st.players.get(s).is_some_and(|p| p.in_game && !p.dead && combat::dist2d(p.dest.unwrap_or_else(|| p.ch.pos()), center) <= radius))
        .collect()
}

/// Retire les effets non passifs d'un joueur (mort) ; rend vrai s'il en avait.
pub fn clear_player_effects(st: &mut WorldState, sid: u32) -> bool {
    let Some(p) = st.players.get_mut(&sid) else { return false };
    let cid = p.ch.id;
    let gone: Vec<Effect> = p.skills.effects.iter().filter(|e| e.ends_at.is_some()).cloned().collect();
    p.skills.effects.retain(|e| e.ends_at.is_none());
    for e in &gone {
        combat::send_around(st, sid, &effects::remove_packet(cid, e));
    }
    !gone.is_empty()
}

/// Effets échus des joueurs et des monstres ; coups des dégâts sur la durée.
async fn effects_step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    for sid in st.sids() {
        let Some(p) = st.players.get_mut(&sid) else { continue };
        if p.skills.effects.is_empty() && p.skills.status_sent == 0 {
            continue;
        }
        let cid = p.ch.id;
        // Dégâts sur la durée posés par un monstre : coups dus avant l'expiration.
        let mut ticks: Vec<Vec<(usize, i32)>> = Vec::new();
        // Purges répétées (Cure) : sortes à retirer, selon leur chance à chaque période.
        let mut purge_kinds = 0u64;
        for e in p.skills.effects.iter_mut() {
            let end = e.ends_at.unwrap_or(now);
            if let Some(d) = e.dot.as_mut() {
                while d.next_at <= now && d.next_at <= end {
                    ticks.push(d.damages.clone());
                    d.next_at += d.interval;
                }
            }
            if let Some(g) = e.purge.as_mut() {
                while g.next_at <= now && g.next_at <= end {
                    if rand::thread_rng().gen_range(0.0..100.0) < g.rate {
                        purge_kinds |= g.kinds;
                    }
                    g.next_at += g.interval;
                }
            }
        }
        if purge_kinds != 0 {
            purge_player(ctx, st, sid, purge_kinds);
        }
        let Some(p) = st.players.get_mut(&sid) else { continue };
        let gone = effects::expire(&mut p.skills.effects, now);
        if !gone.is_empty() {
            for e in &gone {
                combat::send_around(st, sid, &effects::remove_packet(cid, e));
            }
            super::equipment::refresh_factors(ctx, st, sid);
        }
        for damages in ticks {
            let Some(p) = st.players.get_mut(&sid).filter(|p| !p.dead) else { break };
            let total: i32 = damages.iter().map(|(_, d)| *d).sum();
            let hp = (p.factors.point[POINT_HP] - total).max(0);
            p.factors.point[POINT_HP] = hp;
            super::recovery::in_combat(&mut p.timers, now);
            let mp = p.factors.point[1];
            let c = &schema::CHARACTER;
            combat::send_around(st, sid, &build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(7, result_factors(hp, mp, &damages))));
            if hp == 0 {
                combat::player_death(st, sid);
            }
        }
        sync_player_status(st, sid, now);
    }
    let mut affected: Vec<u32> = st.monsters.effects.keys().chain(st.monsters.status_sent.keys()).copied().collect();
    affected.sort_unstable();
    affected.dedup();
    for cid in affected {
        if !st.monsters.get(cid).is_some_and(|m| m.is_alive()) {
            st.monsters.effects.remove(&cid);
            st.monsters.status_sent.remove(&cid);
            continue;
        }
        let watchers = combat::viewers(st, cid);
        let Some(list) = st.monsters.effects.get_mut(&cid) else {
            sync_monster_status(st, cid, now);
            continue;
        };
        // Coups dus avant l'expiration.
        let mut ticks: Vec<(u32, Vec<(usize, i32)>)> = Vec::new();
        for e in list.iter_mut() {
            let end = e.ends_at.unwrap_or(now);
            if let Some(d) = e.dot.as_mut() {
                while d.next_at <= now && d.next_at <= end {
                    ticks.push((d.caster_sid, d.damages.clone()));
                    d.next_at += d.interval;
                }
            }
        }
        let gone = effects::expire(list, now);
        if list.is_empty() {
            st.monsters.effects.remove(&cid);
        }
        for e in gone.iter().filter(|e| announced(e)) {
            combat::broadcast(st, &watchers, &effects::remove_packet(cid, e));
        }
        sync_monster_status(st, cid, now);
        for (caster_sid, damages) in ticks {
            let total: i32 = damages.iter().map(|(_, d)| *d).sum();
            let caster_online = st.players.get(&caster_sid).is_some_and(|p| p.in_game);
            let Some(m) = st.monsters.get_mut(cid).filter(|m| m.is_alive()) else { break };
            // Sans lanceur pour en profiter, l'effet ne tue pas.
            m.hp = (m.hp - total).max(if caster_online { 0 } else { 1 });
            let (hp, tid) = (m.hp, m.tid);
            let max_mp = st.monsters.stats_of(tid).max_mp;
            let c = &schema::CHARACTER;
            let pkt = build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(7, result_factors(hp, max_mp, &damages)));
            combat::broadcast(st, &watchers, &pkt);
            if hp == 0 {
                combat::kill(ctx, st, caster_sid, cid, now).await?;
                break;
            }
            if let Some(caster) = st.players.get(&caster_sid).map(|p| p.ch.id) {
                super::ai::add_aggro(st, cid, caster, total as u64, now);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Maître des compétences
// ---------------------------------------------------------------------------------------------

/// AGPMEVENTSKILLMASTER reçu du client.
pub async fn on_master_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some((npc, _)) = resolve(&ctx.npcs, &base) else {
        warn!("[maître] #{sid} maître des compétences inconnu");
        return Ok(());
    };
    if !npc.events.iter().any(|e| e.function == event_function::SKILL_MASTER) {
        warn!("[maître] #{sid} {} n'enseigne pas de compétences", npc.name);
        return Ok(());
    }
    if npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > MAX_NPC_DISTANCE {
        warn!("[maître] #{sid} {} trop loin de {}", player.ch.name, npc.name);
        return Ok(());
    }
    let event = build_mini(&mini::EVENT_BASE, &base);
    let cid = player.ch.id;
    let s = &schema::EVENT_SKILL_MASTER;
    let reply = |op: i64| Fields::new(s).with(0, op).with(1, event.clone()).with(2, cid);
    match p.int(0).unwrap_or(-1) {
        master_op::REQUEST_EVENT => {
            info!("[maître] {} consulte {}", player.ch.name, npc.name);
            player.send(s, reply(master_op::RESPONSE_EVENT).with(5, 0).with(7, 0));
        }
        master_op::LEARN => {
            let tid = p.int(4).unwrap_or(0) as u32;
            let result = learn(ctx, st, sid, tid).await?;
            if let Some(player) = st.players.get(&sid) {
                player.send(s, reply(master_op::LEARN_RESULT).with(4, tid).with(5, result));
            }
        }
        master_op::REQUEST_UPGRADE => {
            let skill_id = p.int(3).unwrap_or(0) as u32;
            if let Some(result) = upgrade(ctx, st, sid, skill_id).await? {
                if let Some(player) = st.players.get(&sid) {
                    player.send(s, reply(master_op::RESPONSE_UPGRADE).with(3, skill_id).with(5, result));
                }
            }
        }
        op => warn!("[maître] #{sid} op {op} inattendu"),
    }
    Ok(())
}

/// Ce qui manque pour apprendre ou améliorer à ce niveau : codes d'apprentissage et d'amélioration (`None` :
/// pas de code d'amélioration), raison ; sinon `None`.
fn missing(p: &Player, lv: &SkillLevel, price: i64) -> Option<(i64, Option<i64>, &'static str)> {
    if (p.ch.level) < lv.require_level {
        return Some((learn_result::LOW_LEVEL, None, "niveau trop bas"));
    }
    if p.factors.dirt[DIRT_SKILL_POINT] < lv.require_point {
        return Some((learn_result::NOT_ENOUGH_POINTS, Some(upgrade_result::NOT_ENOUGH_POINTS), "pas assez de points de compétence"));
    }
    if p.factors.dirt[DIRT_HEROIC_POINT] < lv.require_heroic {
        return Some((learn_result::NOT_ENOUGH_HEROIC_POINTS, Some(upgrade_result::NOT_ENOUGH_HEROIC_POINTS), "pas assez de points héroïques"));
    }
    if charisma_points(&p.factors) < lv.require_charisma {
        return Some((learn_result::NOT_ENOUGH_CHARISMA, Some(upgrade_result::NOT_ENOUGH_CHARISMA), "pas assez de charisme"));
    }
    if (p.ch.money as i64) < price {
        return Some((learn_result::FAIL, Some(upgrade_result::NOT_ENOUGH_MONEY), "pas assez d'argent"));
    }
    None
}

/// Apprend une compétence : rend le résultat à renvoyer au client.
pub async fn learn(ctx: &WorldCtx, st: &mut WorldState, sid: u32, tid: u32) -> anyhow::Result<i64> {
    let p = &st.players[&sid];
    let Some(t) = ctx.skills.get(tid).filter(|t| !t.auto_attack) else { return Ok(learn_result::FAIL) };
    let class_col = class_column(p.factors.ftype[0], p.factors.ftype[2]);
    let Some(lv) = t.level(1).filter(|lv| lv.allows(class_col)) else {
        info!("[maître] {} ne peut pas apprendre {} (classe)", p.ch.name, t.name);
        return Ok(learn_result::WRONG_CLASS);
    };
    if p.skills.by_tid(tid).is_some() {
        return Ok(learn_result::ALREADY_LEARNED);
    }
    if p.skills.list.len() >= MAX_SKILLS {
        return Ok(learn_result::FAIL);
    }
    if let Some(why) = ctx.skills.mastery(tid, class_col).and_then(|m| mastery_unmet(&ctx.skills, &p.skills, m)) {
        info!("[maître] {} ne peut pas apprendre {} : {why}", p.ch.name, t.name);
        send_system(st, sid, &format!("Impossible d'apprendre {} : {why}.", t.name));
        return Ok(learn_result::FAIL);
    }
    if let Some((code, _, why)) = missing(p, lv, lv.learn_cost) {
        info!("[maître] {} ne peut pas apprendre {} : {why}", p.ch.name, t.name);
        if matches!(code, learn_result::NOT_ENOUGH_HEROIC_POINTS | learn_result::NOT_ENOUGH_CHARISMA) {
            send_system(st, sid, &format!("Impossible d'apprendre {} : {why}.", t.name));
        }
        return Ok(code);
    }
    let money = p.ch.money as i64 - lv.learn_cost;
    let character_id = p.ch.id;
    ctx.db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE level = 1", (character_id, tid)).await?;
    spend_charisma(ctx, st, sid, lv.require_charisma).await?;
    st.next_skill_id += 1;
    let owned = Owned { id: st.next_skill_id, tid, level: 1, ready_at: None };
    let p = st.players.get_mut(&sid).unwrap();
    p.skills.list.push(owned);
    refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    p.send_raw(add_packet(p.ch.id, &owned));
    let caster_tid = p.ch.template_id;
    if sync_passives(&ctx.skills, &mut p.skills, caster_tid) {
        super::equipment::refresh_factors(ctx, st, sid);
    } else {
        send_points(p);
    }
    let p = &st.players[&sid];
    info!("[maître] {} apprend {} ({} or)", p.ch.name, t.name, lv.learn_cost);
    super::set_money(ctx, st, sid, money).await?;
    Ok(learn_result::SUCCESS)
}

/// Améliore une compétence d'un niveau.
/// Rend le résultat à renvoyer (AGPMEVENTSKILLMASTER op 3), `None` quand le client n'en a pas pour ce refus
/// (niveau maximal, niveau trop bas : message système seul).
pub async fn upgrade(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_id: u32) -> anyhow::Result<Option<i64>> {
    let p = &st.players[&sid];
    let Some(owned) = p.skills.by_id(skill_id).copied() else { return Ok(None) };
    let Some(t) = ctx.skills.get(owned.tid).filter(|t| t.can_upgrade() && !t.auto_attack) else { return Ok(None) };
    let Some(lv) = t.level(owned.level + 1).filter(|lv| lv.upgrade_cost > 0) else {
        send_system(st, sid, &format!("{} est déjà à son niveau maximal.", t.name));
        return Ok(None);
    };
    if let Some((_, code, why)) = missing(p, lv, lv.upgrade_cost) {
        info!("[maître] {} ne peut pas améliorer {} : {why}", p.ch.name, t.name);
        // Le client affiche lui-même le refus d'un code connu.
        if code.is_none() {
            send_system(st, sid, &format!("Impossible d'améliorer {} : {why}.", t.name));
        }
        return Ok(code);
    }
    let money = p.ch.money as i64 - lv.upgrade_cost;
    let level = owned.level + 1;
    ctx.db.characters.exec("UPDATE character_skills SET level = ? WHERE character_id = ? AND skill_tid = ?", (level, p.ch.id, owned.tid)).await?;
    spend_charisma(ctx, st, sid, lv.require_charisma).await?;
    let p = st.players.get_mut(&sid).unwrap();
    let Some(s) = p.skills.list.iter_mut().find(|s| s.id == skill_id) else { return Ok(None) };
    s.level = level;
    let updated = *s;
    refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    p.send_raw(update_packet(p.ch.id, &updated));
    let caster_tid = p.ch.template_id;
    if sync_passives(&ctx.skills, &mut p.skills, caster_tid) {
        super::equipment::refresh_factors(ctx, st, sid);
    } else {
        send_points(p);
    }
    let p = &st.players[&sid];
    info!("[maître] {} passe {} au niveau {level} ({} or)", p.ch.name, t.name, lv.upgrade_cost);
    super::set_money(ctx, st, sid, money).await?;
    Ok(Some(upgrade_result::SUCCESS))
}

/// Charisme dépensé par un niveau de compétence héroïque (« require_charisma_point ») : retiré, enregistré, facteurs
/// renvoyés au joueur.
async fn spend_charisma(ctx: &WorldCtx, st: &mut WorldState, sid: u32, points: i32) -> anyhow::Result<()> {
    if points <= 0 {
        return Ok(());
    }
    let left = (st.players[&sid].ch.charisma as i64 - points as i64).max(0);
    set_charisma(ctx, st, sid, left as u32).await
}

/// Charisme d'un joueur fixé : enregistré, recopié dans ses facteurs (CHA), envoyé à lui seul.
pub async fn set_charisma(ctx: &WorldCtx, st: &mut WorldState, sid: u32, points: u32) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let points = points.min(crate::character::MAX_CHARISMA);
    p.ch.charisma = points;
    p.factors.status[4] = points as i32;
    send_points(p);
    ctx.db.characters.exec("UPDATE characters SET charisma = ? WHERE id = ?", (points, p.ch.id)).await?;
    Ok(())
}

/// D'où vient un gain de charisme. Archonia n'en définit aucune règle de gain : le serveur d'origine le donnait en
/// combat entre joueurs et, selon les règles de région (`MobCharismaDrop`, `PVPCharismaDrop` de
/// `RegionPerculiarity.xml`, aucune active dans le fichier du client), sur les monstres. Seule la commande MJ existe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharismaSource {
    Gm,
    Boss,
}

/// Bonus de charisme (pour cent) des effets d'un joueur qui portent sur un gain de cette source : Charisma Double et
/// Event Additional Charisma sur tout gain, Boss Additional Charisma sur ceux d'un boss seulement.
pub fn charisma_bonus(catalog: &SkillCatalog, effects: &[Effect], source: CharismaSource) -> f64 {
    effects
        .iter()
        .filter(|e| e.mods.charisma_bonus != 0.0)
        .filter(|e| source == CharismaSource::Boss || !catalog.get(e.tid).is_some_and(|t| t.name.starts_with("Boss ")))
        .map(|e| e.mods.charisma_bonus)
        .sum()
}

/// Charisme gagné : `base` multiplié par le taux du serveur (`Rate.Charisma`, `/SetCharisma`) et majoré des bonus
/// des effets du joueur qui portent sur cette source (« bonus_charisma_add_rate »). Rend le gain.
pub async fn gain_charisma(ctx: &WorldCtx, st: &mut WorldState, sid: u32, base: u32, source: CharismaSource) -> anyhow::Result<u32> {
    let Some(p) = st.players.get(&sid) else { return Ok(0) };
    let gain = charisma_gain(base, st.rates.get("charisma"), charisma_bonus(&ctx.skills, &p.skills.effects, source));
    let total = p.ch.charisma.saturating_add(gain);
    set_charisma(ctx, st, sid, total).await?;
    Ok(gain)
}

/// Gain de charisme : base × taux × (100 + bonus) / 100, arrondi.
pub fn charisma_gain(base: u32, rate: f64, bonus_pct: f64) -> u32 {
    (base as f64 * rate.max(0.0) * (100.0 + bonus_pct.max(0.0)) / 100.0).round().min(crate::character::MAX_CHARISMA as f64) as u32
}

/// Facteurs du personnage (points de compétence) envoyés au joueur.
pub fn send_points(p: &Player) {
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn dual_slash() -> SkillTemplate {
        let consts = serde_json::json!({
            "require_class_knight": "1", "require_class_berserker": "1", "require_level": "4", "require_point": "1",
            "damage_a_PERCENT": "110", "cost_mp": "10", "cast_time": "2000", "recast_time": "7450", "range": "2", "skill_cost": "80"
        });
        SkillTemplate {
            tid: 1,
            name: "Dual Slash".into(),
            attribute: 1,
            upgradable: 1,
            target: 1,
            melee_attack: 101,
            range_code: 202,
            auto_attack: false,
            status: 0,
            levels: vec![SkillLevel::from_consts(&consts), SkillLevel { require_level: 7, require_point: 1, upgrade_cost: 480, ..SkillLevel::from_consts(&consts) }],
        }
    }

    #[test]
    fn lecture_d_un_niveau() {
        let t = dual_slash();
        let lv = t.level(1).unwrap();
        assert_eq!((lv.require_level, lv.require_point, lv.learn_cost, lv.cost_mp, lv.cast_ms, lv.recast_ms), (4, 1, 80, 10, 2000, 7450));
        assert_eq!((lv.range, lv.damage_pct), (200.0, 110.0));
        assert!(lv.allows(class_column(1, 1)) && lv.allows(class_column(2, 1)));
        assert!(!lv.allows(class_column(1, 4)) && !lv.allows(None));
        assert!(t.instant_damage() && t.offensive() && t.can_upgrade());
        assert!(t.level(3).is_none());
    }

    #[test]
    fn nature_des_competences() {
        assert_eq!(nature(1), Nature { damage: true, ..Default::default() });
        assert_eq!(nature(10), Nature { buff: true, ..Default::default() });
        assert_eq!(nature(21), Nature { debuff: true, damage: true, ..Default::default() });
        assert_eq!(nature(9), Nature { passive: true, ..Default::default() });
        assert_eq!(nature(20010), Nature { buff: true, ..Default::default() });
        let berserker = SkillTemplate { target: 100, attribute: 10, range_code: 1, ..Default::default() };
        assert!(!berserker.offensive() && !berserker.needs_target());
        let earthquake = SkillTemplate { target: 101, range_code: 1304, ..Default::default() };
        assert!(earthquake.offensive() && earthquake.needs_target());
        assert_eq!(earthquake.area(), Area::Sphere { around_target: true });
        let whirl = SkillTemplate { target: 1, range_code: 304, ..Default::default() };
        assert!(!whirl.needs_target(), "zone autour du lanceur");
        assert_eq!((area(202), area(1404), area(1)), (Area::Single, Area::Line, Area::Single));
    }

    #[test]
    fn points_de_competence() {
        assert_eq!(points_for_level(1), 1);
        assert_eq!(points_for_level(49), 49);
        assert_eq!(points_for_level(55), 61);
        assert_eq!(points_for_level(99), 120);
        let mut catalog = SkillCatalog::default();
        catalog.insert(dual_slash());
        let skills = PlayerSkills { list: vec![Owned { id: 1, tid: 1, level: 2, ready_at: None }], ..Default::default() };
        assert_eq!(skills.spent(&catalog), 2);
        let mut f = Factors::default();
        f.status[7] = 10;
        refresh_points(&catalog, &skills, &mut f);
        assert_eq!(f.dirt[DIRT_SKILL_POINT], 8);
    }

    #[test]
    fn zones() {
        let lv = SkillLevel { area_r: 500.0, area_f1: 1000.0, area_f2: 200.0, ..Default::default() };
        let o = Vec3::new(0.0, 0.0, 0.0);
        let east = Vec3::new(1000.0, 0.0, 0.0);
        let sphere = Area::Sphere { around_target: true };
        assert!(in_area(sphere, Vec3::new(1300.0, 0.0, 300.0), east, o, east, &lv));
        assert!(!in_area(sphere, Vec3::new(1600.0, 0.0, 0.0), east, o, east, &lv));
        assert!(in_area(Area::Line, Vec3::new(700.0, 0.0, 90.0), o, o, east, &lv));
        assert!(!in_area(Area::Line, Vec3::new(700.0, 0.0, 150.0), o, o, east, &lv), "trop à côté");
        assert!(!in_area(Area::Line, Vec3::new(-100.0, 0.0, 0.0), o, o, east, &lv), "derrière");
    }

    #[test]
    fn effets_de_combat_lus() {
        let bash = SkillLevel::with_spec(
            &serde_json::json!({ "damage_a_PERCENT": "110", "stun_time": "3000", "skill_rate": "30" }),
            &Spec { melee_attack: 101, status: effects::status::STUN, offensive: true, ..Default::default() },
        );
        assert_eq!((bash.status, bash.status_rate, bash.status_ms), (effects::status::STUN, 30.0, 3000));
        assert!(bash.leaves_effect(effects::status::STUN) && !bash.leaves_effect(0) && !bash.lasting());
        let snare = SkillLevel::with_spec(
            &serde_json::json!({ "percent.movement": "-20", "duration": "8000" }),
            &Spec { status: effects::status::SLOW, offensive: true, ..Default::default() },
        );
        assert_eq!((snare.status_rate, snare.status_ms, snare.mods.movement_pct), (0.0, 8000, -20.0), "ralentissement sans chance : toujours");
        let crit = SkillLevel::with_spec(&serde_json::json!({ "critical": "120", "skill_rate": "3" }), &Spec { melee_attack: 106, ..Default::default() });
        assert_eq!((crit.mods.crit_rate, crit.mods.crit_dmg), (3.0, 120.0));
        let hold = SkillLevel::with_spec(&serde_json::json!({ "point.def_point_physical": "20", "skill_rate": "10" }), &Spec { reflect_melee_attack: 2, ..Default::default() });
        assert_eq!((hold.mods.block, hold.mods.defense), (10.0, 20.0));
        let reflection = SkillLevel::with_spec(&serde_json::json!({ "special_status_rate": "14", "damage_convert_mp": "25" }), &Spec { convert_type: 2, ..Default::default() });
        assert_eq!((reflection.mods.to_mp_rate, reflection.mods.to_mp), (14.0, 25.0));
        let protect = SkillLevel::with_spec(&serde_json::json!({ "skill_rate": "100", "duration": "180000" }), &Spec { status: effects::status::STUN_PROTECT, ..Default::default() });
        assert_eq!((protect.mods.stun_protect, protect.status_rate), (100.0, 0.0), "sur soi, l'état prend toujours");
    }

    #[test]
    fn contre_attaque_et_groupe() {
        let counter = SkillLevel::with_spec(&serde_json::json!({ "skill_rate": "11", "duration": "5000" }), &Spec { defense_melee_attack: 2, ..Default::default() });
        assert_eq!(counter.mods.counter, 11.0, "Counter Attack niveau 10");
        assert_eq!(SkillLevel::with_spec(&serde_json::json!({ "skill_rate": "11" }), &Spec { defense_melee_attack: 4, ..Default::default() }).mods.counter, 0.0);
        let marching = SkillTemplate { target: 100, range_code: 303, ..Default::default() };
        assert!(marching.party() && !marching.needs_target());
        assert!(!SkillTemplate { target: 100, range_code: 1, ..Default::default() }.party(), "sur soi seul");
        assert!(!SkillTemplate { target: 1, range_code: 303, ..Default::default() }.party(), "offensive");
    }

    #[test]
    fn maitrises() {
        let mut catalog = SkillCatalog::default();
        let lv = |point: i32, heroic: i32| SkillLevel { require_point: point, require_heroic: heroic, ..Default::default() };
        catalog.insert(SkillTemplate { tid: 526, name: "Marching Song".into(), levels: vec![lv(1, 0); 5], ..Default::default() });
        catalog.insert(SkillTemplate { tid: 1437, name: "Shield of the Front".into(), levels: vec![lv(2, 2); 5], ..Default::default() });
        let knight = class_column(1, 1);
        catalog.insert_mastery(525, Mastery { class: knight, heroic: false, requires: vec![(526, 4)], total: 0 });
        catalog.insert_mastery(1440, Mastery { class: knight, heroic: true, requires: vec![(1437, 3)], total: 19 });
        let mut skills = PlayerSkills { list: vec![Owned { id: 1, tid: 526, level: 3, ready_at: None }], ..Default::default() };
        let capacitate = catalog.mastery(525, knight).unwrap();
        assert_eq!(mastery_unmet(&catalog, &skills, capacitate).as_deref(), Some("il faut Marching Song au niveau 4"));
        skills.list[0].level = 4;
        assert_eq!(mastery_unmet(&catalog, &skills, capacitate), None);
        // Arbre héroïque : Shield of the Front niveau 3 (6 points héroïques) ne suffit pas, il en faut 19 en tout.
        skills.list.push(Owned { id: 2, tid: 1437, level: 3, ready_at: None });
        let judgement = catalog.mastery(1440, knight).unwrap();
        assert_eq!(skills.spent_heroic(&catalog), 6);
        assert!(mastery_unmet(&catalog, &skills, judgement).is_some_and(|w| w.contains("19 points héroïques")));
        assert!(catalog.mastery(1440, class_column(2, 1)).is_some(), "seul arbre qui la porte");
        assert!(catalog.mastery(222, knight).is_none(), "hors des arbres");
    }

    #[test]
    fn pose_des_etats() {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let mut lv = SkillLevel { status: effects::status::STUN, status_rate: 30.0, status_ms: 3000, ..Default::default() };
        let n = (0..10_000).filter(|_| roll_status(&mut rng, &lv, &Mods::default()) != 0).count();
        assert!((2700..3300).contains(&n), "{n} étourdissements sur 10 000 à 30 %");
        assert_eq!(roll_status(&mut rng, &lv, &Mods { stun_protect: 100.0, ..Default::default() }), 0, "protégé");
        lv.status_rate = 0.0;
        assert_eq!(roll_status(&mut rng, &lv, &Mods::default()), effects::status::STUN, "sans chance : toujours");
        let now = Instant::now();
        let owned = Owned { id: 9, tid: 1444, level: 1, ready_at: None };
        lv.duration_ms = 1000;
        let e = new_effect(&owned, &lv, 96, effects::status::STUN, None, now);
        assert_eq!((e.duration_ms, e.ends_at, e.status_until), (3000, Some(now + Duration::from_millis(3000)), Some(now + Duration::from_millis(3000))));
        let e = new_effect(&owned, &lv, 96, 0, None, now);
        assert_eq!((e.duration_ms, e.status_until), (1000, None));
    }

    #[test]
    fn passives() {
        let mut catalog = SkillCatalog::default();
        let hold = serde_json::json!({ "require_class_knight": "1", "point.def_point_physical": "20" });
        catalog.insert(SkillTemplate { tid: 40, name: "Defense Hold".into(), attribute: 9, target: 111, levels: vec![SkillLevel::from_consts(&hold)], ..Default::default() });
        let mut skills = PlayerSkills { list: vec![Owned { id: 3, tid: 40, level: 1, ready_at: None }], ..Default::default() };
        assert!(sync_passives(&catalog, &mut skills, 96));
        assert!(!sync_passives(&catalog, &mut skills, 96), "inchangée");
        assert_eq!((skills.effects.len(), effects::total(&skills.effects).defense), (1, 20.0));
    }

    #[test]
    fn degats_d_une_competence() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let a = Attacker { level: 10, class: 1, str_: 30.0, min_damage: 10.0, max_damage: 10.0, ..Default::default() };
        let lv = dual_slash().levels[0].clone();
        // Jet : 10 + 30/3 = 20 (fourchette 20..20,5) ; × 2,1 = 42 ; armure 2.
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 10, dr: 0, defense: 2.0, ..Default::default() }, &lv);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, RESULT_PHYSICAL);
        assert!((40..=42).contains(&d[0].1), "{d:?}");
        // Cible de 4 niveaux de plus : -20 %.
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 14, dr: 0, defense: 0.0, ..Default::default() }, &lv);
        assert!((33..=35).contains(&d[0].1), "{d:?}");
        // Élément seul (feu fixe 4, sans dégâts de feu du personnage) : (0 + 4) × 2 = 8, pas de part physique.
        let mut fire = SkillLevel::default();
        fire.spirit[2] = (4.0, 70.0);
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 10, dr: 0, defense: 100.0, ..Default::default() }, &fire);
        assert_eq!(d, vec![(12, 8)]);
        // Résistance au feu de 50 % : moitié moins.
        let mut resist = [0; 8];
        resist[2] = 50;
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 10, resist, ..Default::default() }, &fire);
        assert_eq!(d, vec![(12, 4)]);
    }

    #[test]
    fn points_heroiques() {
        assert_eq!((heroic_points_for_level(89), heroic_points_for_level(90), heroic_points_for_level(100)), (0, 1, 21));
        assert_eq!((heroic_points_for_level(110), heroic_points_for_level(115), heroic_points_for_level(120)), (41, 56, 71));
        let mut catalog = SkillCatalog::default();
        let lv = SkillLevel { require_point: 3, require_heroic: 3, ..Default::default() };
        catalog.insert(SkillTemplate { tid: 1439, name: "Iron Shield".into(), levels: vec![lv.clone(), lv], ..Default::default() });
        let skills = PlayerSkills { list: vec![Owned { id: 1, tid: 1439, level: 2, ready_at: None }], ..Default::default() };
        let mut f = Factors::default();
        f.status[7] = 100;
        refresh_points(&catalog, &skills, &mut f);
        assert_eq!((f.dirt[DIRT_SKILL_POINT], f.dirt[DIRT_HEROIC_POINT]), (115, 15));
        f.status[4] = 250;
        assert_eq!(charisma_points(&f), 250);
        assert_eq!((charisma_gain(10, 1.0, 0.0), charisma_gain(10, 2.0, 100.0), charisma_gain(7, 1.0, 200.0)), (10, 40, 21), "taux et Charisma Double");
        // Boss Additional Charisma ne porte que sur les gains d'un boss ; Charisma Double sur tous.
        let mut catalog = SkillCatalog::default();
        catalog.insert(SkillTemplate { tid: 1619, name: "Charisma Double".into(), ..Default::default() });
        catalog.insert(SkillTemplate { tid: 4393, name: "Boss Additional Charisma".into(), ..Default::default() });
        let buff = |tid: u32, pct: f64| new_effect(&Owned { id: 1, tid, level: 1, ready_at: None }, &SkillLevel { duration_ms: 1000, mods: Mods { charisma_bonus: pct, ..Default::default() }, ..Default::default() }, 96, 0, None, Instant::now());
        let on = [buff(1619, 100.0), buff(4393, 200.0)];
        assert_eq!((charisma_bonus(&catalog, &on, CharismaSource::Gm), charisma_bonus(&catalog, &on, CharismaSource::Boss)), (100.0, 300.0));
    }

    #[test]
    fn defense_ignoree_partage_et_purges_lus() {
        // Lethal Attack (reflect_melee_attack 4) et Stunner (8) : montants de Skill_Const2.txt.
        let lethal = SkillLevel::with_spec(&serde_json::json!({ "percent2.ignore_physical_defence_percent2": "5" }), &Spec { reflect_melee_attack: 4, ..Default::default() });
        assert_eq!((lethal.mods.ignore_defense, lethal.mods.ignore_resist, lethal.mods.block), (5.0, 0.0, 0.0));
        let stunner = SkillLevel::with_spec(&serde_json::json!({ "percent2.ignore_attribute_defence_percent2": "10" }), &Spec { reflect_melee_attack: 8, ..Default::default() });
        assert_eq!(stunner.mods.ignore_resist, 10.0);
        // Divine Protection (divide 17 : élémentaires) et Martyr's Mark (18 : physiques), sans partage sans le bit 16.
        let c = serde_json::json!({ "divide_rate": "40", "duration": "660000" });
        let divine = SkillLevel::with_spec(&c, &Spec { divide: 17, ..Default::default() });
        let martyr = SkillLevel::with_spec(&c, &Spec { divide: 18, ..Default::default() });
        assert_eq!((divine.mods.share_spirit, divine.mods.share_physical, martyr.mods.share_physical), (40.0, 0.0, 40.0));
        assert!(divine.lasting());
        assert!(SkillLevel::with_spec(&c, &Spec { divide: 2, ..Default::default() }).mods.is_empty());
        // Cure (dispel_magic 10002) : purge répétée toutes les « interval » ms ; Antimagic (16) : une fois.
        let cure = SkillLevel::with_spec(&serde_json::json!({ "duration": "60000", "interval": "1000", "skill_rate": "100" }), &Spec { dispel_magic: 10002, ..Default::default() });
        assert_eq!((cure.dispel, cure.purge_every_ms), (effects::dispel::DEBUFFS, Some(1000)));
        assert!(cure.lasting());
        let e = new_effect(&Owned { id: 3, tid: 90, level: 1, ready_at: None }, &cure, 96, 0, None, Instant::now());
        assert_eq!(e.purge.map(|p| (p.kinds, p.rate, p.interval)), Some((effects::dispel::DEBUFFS, 100.0, Duration::from_millis(1000))));
        let anti = SkillLevel::with_spec(&serde_json::json!({}), &Spec { dispel_magic: 16, offensive: true, ..Default::default() });
        assert_eq!((anti.dispel, anti.purge_every_ms, anti.lasting()), (effects::dispel::ALL, None, false));
        // Cibles : Assistance (target 10) vise un ami ; Bow of Meditation (111) touche aussi son lanceur.
        assert!(SkillTemplate { target: 10, ..Default::default() }.on_friend());
        let bow = SkillTemplate { target: 111, ..Default::default() };
        assert!(bow.offensive() && bow.on_self() && bow.on_friend());
        assert!(!SkillTemplate { target: 1, ..Default::default() }.on_self());
    }

    #[test]
    fn paquets_de_competence() {
        use archcore_net::decode_mini;
        let s = Owned { id: 7, tid: 1, level: 3, ready_at: None };
        let add = archcore_net::decode_packet(&schema::SKILL, &add_packet(0x1234, &s)).unwrap();
        assert_eq!((add.int(0), add.int(1), add.int(4)), (Some(op::ADD), Some(7), Some(1)));
        let owner = decode_mini(&mini::SKILL_BASE, add.bytes(2).unwrap()).unwrap();
        assert_eq!((owner.int(0), owner.int(1)), (Some(3), Some(0x1234)));
        let f = result_factors(90, 5, &[(RESULT_PHYSICAL, 12), (12, 8)]);
        let cf = decode_mini(&mini::CHAR_FACTORS, archcore_net::packet::mini_payload(&f)).unwrap();
        let result = decode_mini(&mini::CHAR_FACTORS, cf.bytes(0).unwrap()).unwrap();
        let pt = decode_mini(&mini::FACTOR_POINT, result.bytes(3).unwrap()).unwrap();
        assert_eq!((pt.int(0), pt.int(9), pt.int(12)), (Some(90), Some(-12), Some(-8)));
    }
}
