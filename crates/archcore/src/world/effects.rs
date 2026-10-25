//! Effets durables des compétences : renforcements, affaiblissements, dégâts sur la durée, passives.
//!
//! ```text
//!   Valeurs : colonnes de SKILL_CONST rangées par l'import en « point.* » (ajout) et « percent.* » (pour cent) :
//!             caractéristiques, PV et PM max, dégâts et défense physiques, précision, esquive, vitesses de
//!             déplacement et d'attaque. « point.hp », « percent.hp » (et mp) soignent à l'application.
//!   Dégâts sur la durée : « dot_dmg_* » (dégâts fixes par élément) toutes les « dot_damage_time » ms.
//!   Un effet dure « duration » ms ; une passive dure tant que la compétence est connue. Un nouvel effet de la
//!   même compétence remplace l'ancien.
//!   États spéciaux : colonne special_status de SKILL_SPEC (étourdissement, gel, ralentissement, sommeil,
//!             attaques ou compétences interdites, invincibilités…), portés par l'effet. Sur une cible, ils
//!             prennent avec la chance « skill_rate » (toujours sans elle) et durent « stun_time » ms, sinon
//!             « duration ». L'état d'un personnage, union de ceux de ses effets, part dans le champ 17
//!             d'AGPMCHARACTER (op 1).
//!   Combat : melee_attack 106, 109, 110 ajoutent « skill_rate » à la chance de coup critique et « critical »
//!             aux dégâts critiques (en pour cent) ; reflect_melee_attack 2 (bouclier) ajoute « skill_rate » à
//!             la chance de bloquer, comme « point.physical_block » ; convert_type 1 et 2 changent un coup reçu
//!             en PV ou en PM (chance « skill_rate », sinon « special_status_rate » ; part « damage_convert_* ») ;
//!             defense_melee_attack 2 (contre-attaque) renvoie à son auteur, avec la chance « skill_rate », les
//!             dégâts d'un coup de monstre reçu.
//!   Paquets : AGPMSKILL op 4 (effet ajouté : identifiant de la compétence, porteur, champ 8 = mini SKILL_BUFF
//!             TID, durée, TID du lanceur) et op 5 (effet retiré : TID), aux joueurs qui voient le porteur.
//! ```
//! Sur un joueur, les effets s'ajoutent aux facteurs (modèle + équipement) ; sur un monstre, ils modifient
//! sa défense, ses dégâts et ses vitesses d'attaque et de déplacement.

use super::combat::result;
use crate::character::{Factors, POINT_HP};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Value};
use std::time::{Duration, Instant};

/// Effets au plus sur un personnage.
pub const MAX_EFFECTS: usize = 32;
/// Type d'objet « personnage » dans SKILL_BASE.
const BASE_CHARACTER: i64 = 3;

pub mod op {
    pub const ADD_BUFF: i64 = 4;
    pub const REMOVE_BUFF: i64 = 5;
}

/// États spéciaux d'un personnage (bits du champ 17 d'AGPMCHARACTER).
pub mod status {
    pub const STUN: u64 = 0x1;
    pub const FREEZE: u64 = 0x2;
    pub const SLOW: u64 = 0x4;
    pub const INVINCIBLE: u64 = 0x8;
    pub const ATTRIBUTE_INVINCIBLE: u64 = 0x10;
    pub const NOT_ADD_AGGRO: u64 = 0x20;
    pub const HIDE_AGGRO: u64 = 0x40;
    pub const STUN_PROTECT: u64 = 0x80;
    pub const TRANSPARENT: u64 = 0x200;
    pub const HALF_TRANSPARENT: u64 = 0x20_0000;
    pub const NORMAL_ATK_INVINCIBLE: u64 = 0x2000_0000;
    pub const SKILL_ATK_INVINCIBLE: u64 = 0x4000_0000;
    pub const DISABLE_SKILL: u64 = 0x8000_0000;
    pub const DISABLE_NORMAL_ATK: u64 = 0x1_0000_0000;
    pub const SLEEP: u64 = 0x2_0000_0000;
    pub const SLOW_INVINCIBLE: u64 = 0x4_0000_0000;
    pub const DISARMAMENT: u64 = 0x20_0000_0000;
    pub const DISABLE_CHATTING: u64 = 0x80_0000_0000;
    pub const HOLD: u64 = 0x100_0000_0000;
    pub const CONFUSION: u64 = 0x200_0000_0000;
    pub const FEAR: u64 = 0x400_0000_0000;
    pub const BERSERK: u64 = 0x800_0000_0000;
    pub const SHRINK: u64 = 0x1000_0000_0000;
    pub const DISEASE: u64 = 0x2000_0000_0000;
    /// Ni déplacement ni action.
    pub const HELPLESS: u64 = STUN | FREEZE | SLEEP;
    pub const NO_MOVE: u64 = HELPLESS | HOLD;
    pub const NO_ATTACK: u64 = HELPLESS | DISABLE_NORMAL_ATK | FEAR;
    pub const NO_SKILL: u64 = HELPLESS | DISABLE_SKILL | FEAR;
    /// Hors d'atteinte des monstres.
    pub const UNTOUCHABLE: u64 = INVINCIBLE | TRANSPARENT;
}

/// États d'un code de la colonne special_status (1 étourdissement… 25 rétrécissement) ; 9 (« halte »)
/// interdit compétences et attaques.
pub fn status_of_code(code: i64) -> u64 {
    use status::*;
    match code {
        1 => STUN,
        2 => FREEZE,
        3 => SLOW,
        4 => INVINCIBLE,
        5 => ATTRIBUTE_INVINCIBLE,
        6 => NOT_ADD_AGGRO,
        7 => HIDE_AGGRO,
        8 => STUN_PROTECT,
        9 => DISABLE_SKILL | DISABLE_NORMAL_ATK,
        10 => TRANSPARENT,
        11 => HALF_TRANSPARENT,
        12 => NORMAL_ATK_INVINCIBLE,
        13 => SKILL_ATK_INVINCIBLE,
        14 => DISABLE_SKILL,
        15 => DISABLE_NORMAL_ATK,
        16 => SLEEP,
        17 => STUN_PROTECT | SLOW_INVINCIBLE,
        18 => DISARMAMENT,
        19 => DISABLE_CHATTING,
        20 => HOLD,
        21 => CONFUSION,
        22 => FEAR,
        23 => DISEASE,
        24 => BERSERK,
        25 => SHRINK,
        _ => 0,
    }
}

/// Champ special_status de SKILL_SPEC : codes séparés par « ; » (ceux à partir de 1000 règlent autre chose).
pub fn parse_status(s: &str) -> u64 {
    s.split(';').filter_map(|v| v.trim().parse::<i64>().ok()).filter(|v| *v < 1000).fold(0, |acc, v| acc | status_of_code(v))
}

/// Modificateurs de caractéristiques d'un effet (ou leur somme).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mods {
    /// Constitution, force, intelligence, dextérité, sagesse.
    pub stats: [f64; 5],
    pub max_hp: f64,
    pub max_hp_pct: f64,
    pub max_mp: f64,
    pub max_mp_pct: f64,
    pub dmg_min: f64,
    pub dmg_max: f64,
    pub dmg_pct: f64,
    pub defense: f64,
    pub defense_pct: f64,
    pub hit: f64,
    pub evade: f64,
    pub movement_pct: f64,
    pub atk_speed_pct: f64,
    /// Coup critique : chance (pour cent) et dégâts (pour cent des dégâts normaux).
    pub crit_rate: f64,
    pub crit_dmg: f64,
    /// Chance de bloquer un coup (pour cent, 80 au plus).
    pub block: f64,
    /// Coup reçu changé en PV ou en PM : chance, part des dégâts (pour cent).
    pub to_hp_rate: f64,
    pub to_hp: f64,
    pub to_mp_rate: f64,
    pub to_mp: f64,
    /// Chance d'échapper à l'étourdissement (pour cent).
    pub stun_protect: f64,
    /// Contre-attaque : chance (pour cent) de renvoyer à son auteur les dégâts d'un coup reçu.
    pub counter: f64,
}

/// Soin à l'application : PV et PM fixes et pour cent du maximum.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Heal {
    pub hp: f64,
    pub hp_pct: f64,
    pub mp: f64,
    pub mp_pct: f64,
}

impl Heal {
    pub fn is_empty(&self) -> bool {
        *self == Heal::default()
    }
}

/// Éléments des dégâts sur la durée : colonne de SKILL_CONST et indice dans le résultat (9 physique, 10 magie
/// … 17 glace).
pub const DOT_COLUMNS: [(&str, usize); 9] =
    [("a", 9), ("magic", 10), ("water", 11), ("fire", 12), ("earth", 13), ("air", 14), ("poison", 15), ("thunder", 16), ("ice", 17)];

fn num(c: &serde_json::Value, k: &str) -> f64 {
    c.get(k).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<f64>().ok()).unwrap_or(0.0)
}

impl Mods {
    pub fn from_consts(c: &serde_json::Value) -> Self {
        let p = |k: &str| num(c, &format!("point.{k}"));
        let pc = |k: &str| num(c, &format!("percent.{k}"));
        Mods {
            stats: [p("con"), p("str"), p("int"), p("dex"), p("wis")],
            max_hp: p("max_hp"),
            max_hp_pct: pc("max_hp"),
            max_mp: p("max_mp"),
            max_mp_pct: pc("max_mp"),
            dmg_min: p("dmg_min_physical"),
            dmg_max: p("dmg_max_physical"),
            dmg_pct: pc("dmg_max_physical").max(pc("dmg_min_physical")),
            defense: p("def_point_physical"),
            defense_pct: pc("def_point_physical"),
            hit: p("hit_rate"),
            evade: p("evade_rate") + p("dodge_rate"),
            movement_pct: pc("movement"),
            atk_speed_pct: pc("atk_speed"),
            block: p("physical_block"),
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Mods::default()
    }

    pub fn add(&mut self, o: &Mods) {
        for i in 0..5 {
            self.stats[i] += o.stats[i];
        }
        self.max_hp += o.max_hp;
        self.max_hp_pct += o.max_hp_pct;
        self.max_mp += o.max_mp;
        self.max_mp_pct += o.max_mp_pct;
        self.dmg_min += o.dmg_min;
        self.dmg_max += o.dmg_max;
        self.dmg_pct += o.dmg_pct;
        self.defense += o.defense;
        self.defense_pct += o.defense_pct;
        self.hit += o.hit;
        self.evade += o.evade;
        self.movement_pct += o.movement_pct;
        self.atk_speed_pct += o.atk_speed_pct;
        self.crit_rate += o.crit_rate;
        self.crit_dmg += o.crit_dmg;
        self.block += o.block;
        self.to_hp_rate += o.to_hp_rate;
        self.to_hp += o.to_hp;
        self.to_mp_rate += o.to_mp_rate;
        self.to_mp += o.to_mp;
        self.stun_protect += o.stun_protect;
        self.counter += o.counter;
    }
}

impl Heal {
    pub fn from_consts(c: &serde_json::Value) -> Self {
        Heal { hp: num(c, "point.hp"), hp_pct: num(c, "percent.hp"), mp: num(c, "point.mp"), mp_pct: num(c, "percent.mp") }
    }
}

/// Dégâts sur la durée d'un niveau de compétence : (indice dans le résultat, dégâts par coup), période.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DotSpec {
    pub damages: Vec<(usize, f64)>,
    pub interval_ms: u64,
}

impl DotSpec {
    pub fn from_consts(c: &serde_json::Value) -> Self {
        let damages = DOT_COLUMNS.iter().filter_map(|(k, i)| Some((*i, num(c, &format!("dot_dmg_{k}")))).filter(|(_, d)| *d > 0.0)).collect();
        let interval = num(c, "dot_damage_time");
        DotSpec { damages, interval_ms: if interval > 0.0 { interval as u64 } else { 1000 } }
    }
    pub fn is_empty(&self) -> bool {
        self.damages.is_empty()
    }
}

/// Dégâts sur la durée en cours.
#[derive(Clone, Debug, PartialEq)]
pub struct Dot {
    /// (indice dans le résultat, dégâts par coup).
    pub damages: Vec<(usize, i32)>,
    pub interval: Duration,
    pub next_at: Instant,
    /// Session du lanceur, à qui revient la victime (`MONSTER_CASTER` : un monstre).
    pub caster_sid: u32,
}

/// Lanceur d'un effet posé par un monstre (aucune session n'a ce numéro).
pub const MONSTER_CASTER: u32 = 0;

/// Un effet porté par un personnage ou un monstre.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub skill_id: u32,
    pub tid: u32,
    pub level: u32,
    pub caster_tid: u32,
    pub duration_ms: u64,
    /// Fin de l'effet ; `None` : passive.
    pub ends_at: Option<Instant>,
    pub mods: Mods,
    pub dot: Option<Dot>,
    /// États spéciaux posés, jusqu'à `status_until` (`None` : tant que dure l'effet).
    pub status: u64,
    pub status_until: Option<Instant>,
}

/// Somme des modificateurs d'une liste d'effets.
pub fn total(effects: &[Effect]) -> Mods {
    let mut m = Mods::default();
    for e in effects {
        m.add(&e.mods);
    }
    m
}

/// États spéciaux en cours d'une liste d'effets.
pub fn active_status(effects: &[Effect], now: Instant) -> u64 {
    effects.iter().filter(|e| e.status_until.is_none_or(|t| now < t)).fold(0, |acc, e| acc | e.status)
}

/// Retire des états (sommeil rompu par un coup, par exemple) ; rend vrai si l'un d'eux était posé.
pub fn lift_status(effects: &mut [Effect], bits: u64) -> bool {
    let mut lifted = false;
    for e in effects.iter_mut().filter(|e| e.status & bits != 0) {
        e.status &= !bits;
        lifted = true;
    }
    lifted
}

/// Coup critique sur un coup réussi : (résultat, dégâts).
pub fn critical(rng: &mut impl rand::Rng, m: &Mods, outcome: i64, damage: i32) -> (i64, i32) {
    if outcome != result::SUCCESS || m.crit_rate <= 0.0 || rng.gen_range(0.0..100.0) >= m.crit_rate {
        return (outcome, damage);
    }
    (result::CRITICAL, (damage as f64 * m.crit_dmg.max(100.0) / 100.0).round() as i32)
}

/// Sort d'un coup de monstre réussi sur un joueur, d'après ses effets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// Coup porté.
    Hit,
    /// Esquivé (invincible aux attaques normales).
    Evade,
    /// Bloqué.
    Block,
    /// Changé en PV ou en PM rendus.
    ToHp(i32),
    ToMp(i32),
}

pub fn guard(rng: &mut impl rand::Rng, m: &Mods, status: u64, damage: i32) -> Guard {
    if status & status::NORMAL_ATK_INVINCIBLE != 0 {
        return Guard::Evade;
    }
    let mut roll = |rate: f64| rate > 0.0 && rng.gen_range(0.0..100.0) < rate.min(80.0);
    if roll(m.block) {
        return Guard::Block;
    }
    if roll(m.to_hp_rate) {
        return Guard::ToHp((damage as f64 * m.to_hp / 100.0) as i32);
    }
    if roll(m.to_mp_rate) {
        return Guard::ToMp((damage as f64 * m.to_mp / 100.0) as i32);
    }
    Guard::Hit
}

/// Contre-attaque déclenchée par un coup reçu, selon la chance des effets.
pub fn counter(rng: &mut impl rand::Rng, m: &Mods) -> bool {
    m.counter > 0.0 && rng.gen_range(0.0..100.0) < m.counter
}

/// AGPMCHARACTER op 1 : états spéciaux de `cid`.
pub fn status_packet(cid: u32, status: u64) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(17, status as i64))
}

/// Ajoute un effet : remplace celui de la même compétence ; refusé si la liste est pleine.
pub fn push(effects: &mut Vec<Effect>, e: Effect) -> Option<Effect> {
    let replaced = effects.iter().position(|x| x.tid == e.tid).map(|i| effects.remove(i));
    if effects.len() >= MAX_EFFECTS {
        return replaced;
    }
    effects.push(e);
    replaced
}

/// Retire les effets échus ; les rend.
pub fn expire(effects: &mut Vec<Effect>, now: Instant) -> Vec<Effect> {
    let (gone, keep): (Vec<Effect>, Vec<Effect>) = std::mem::take(effects).into_iter().partition(|e| e.ends_at.is_some_and(|t| now >= t));
    *effects = keep;
    gone
}

fn pct(v: f64, p: f64) -> f64 {
    v * (100.0 + p) / 100.0
}

/// Applique des modificateurs à des facteurs de joueur (modèle + équipement).
pub fn apply(f: &mut Factors, m: &Mods) {
    if m.is_empty() {
        return;
    }
    // Caractéristiques stockées × 100 : constitution, force, intelligence, dextérité, puis sagesse (indice 6).
    for (i, idx) in [0usize, 1, 2, 3, 6].iter().enumerate() {
        f.status[*idx] = (f.status[*idx] as f64 + m.stats[i] * 100.0).max(0.0) as i32;
    }
    f.point_max[0] = pct(f.point_max[0] as f64 + m.max_hp, m.max_hp_pct).max(1.0) as i32;
    f.point_max[1] = pct(f.point_max[1] as f64 + m.max_mp, m.max_mp_pct).max(0.0) as i32;
    f.damage_min[0] = pct(f.damage_min[0] as f64 + m.dmg_min, m.dmg_pct).max(0.0) as i32;
    f.damage_max[0] = pct(f.damage_max[0] as f64 + m.dmg_max, m.dmg_pct).max(f.damage_min[0] as f64) as i32;
    f.defense_point[0] = pct(f.defense_point[0] as f64 + m.defense, m.defense_pct).max(0.0) as i32;
    f.point_max[8] = (f.point_max[8] as f64 + m.hit).max(0.0) as i32;
    f.point_max[9] = (f.point_max[9] as f64 + m.evade).max(0.0) as i32;
    f.status[8] = pct(f.status[8] as f64, m.movement_pct).max(0.0) as i32;
    f.status[9] = pct(f.status[9] as f64, m.movement_pct).max(0.0) as i32;
    f.attack[2] = pct(f.attack[2] as f64, m.atk_speed_pct).max(1.0) as i32;
    f.point[POINT_HP] = f.point[POINT_HP].min(f.point_max[0]);
    f.point[1] = f.point[1].min(f.point_max[1]);
}

/// Soin appliqué à des PV et PM courants (bornés par le maximum ; jamais en dessous de 1 PV).
pub fn heal(f: &mut Factors, h: &Heal) {
    let hp = f.point[POINT_HP] as f64 + h.hp + f.point_max[0] as f64 * h.hp_pct / 100.0;
    let mp = f.point[1] as f64 + h.mp + f.point_max[1] as f64 * h.mp_pct / 100.0;
    f.point[POINT_HP] = (hp as i32).clamp(1, f.point_max[0].max(1));
    f.point[1] = (mp as i32).clamp(0, f.point_max[1].max(0));
}

/// Défense physique d'un monstre sous effets.
pub fn monster_defense(ac: i32, m: &Mods) -> f64 {
    pct(ac as f64 + m.defense, m.defense_pct).max(0.0)
}

/// Dégâts (min, max) d'un monstre sous effets.
pub fn monster_damage(min: i32, max: i32, m: &Mods) -> (f64, f64) {
    let lo = pct(min as f64 + m.dmg_min, m.dmg_pct).max(0.0);
    (lo, pct(max as f64 + m.dmg_max, m.dmg_pct).max(lo))
}

/// Vitesse (déplacement ou attaque) sous un pourcentage d'effets ; au moins 10 % de la vitesse d'origine.
pub fn speed(v: i32, p: f64) -> i32 {
    pct(v as f64, p.max(-90.0)).round() as i32
}

fn base(cid: u32) -> Vec<u8> {
    let s = &mini::SKILL_BASE;
    build_mini(s, &Fields::new(s).with(0, BASE_CHARACTER).with(1, cid))
}

/// AGPMSKILL op 4 : effet ajouté sur `owner`.
pub fn add_packet(owner: u32, e: &Effect) -> Vec<u8> {
    let b = &mini::SKILL_BUFF;
    let buff = build_mini(b, &Fields::new(b).with(0, e.tid).with(1, e.duration_ms as i64).with(2, e.caster_tid));
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::ADD_BUFF).with(1, e.skill_id).with(2, base(owner)).with(8, vec![Value::Bytes(buff)]))
}

/// AGPMSKILL op 5 : effet retiré de `owner`.
pub fn remove_packet(owner: u32, e: &Effect) -> Vec<u8> {
    let b = &mini::SKILL_BUFF;
    let buff = build_mini(b, &Fields::new(b).with(0, e.tid));
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::REMOVE_BUFF).with(1, e.skill_id).with(2, base(owner)).with(8, vec![Value::Bytes(buff)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn effect(tid: u32, mods: Mods, ends_at: Option<Instant>) -> Effect {
        Effect { skill_id: 1, tid, level: 1, caster_tid: 96, duration_ms: 1000, ends_at, mods, dot: None, status: 0, status_until: None }
    }

    #[test]
    fn lecture_des_modificateurs() {
        // Berserker niveau 1.
        let c = serde_json::json!({ "point.con": "10", "point.str": "15", "point.dex": "3", "point.dmg_min_physical": "3",
            "point.dmg_max_physical": "5", "point.def_point_physical": "4", "percent.movement": "16", "duration": "330000" });
        let m = Mods::from_consts(&c);
        assert_eq!((m.stats, m.dmg_min, m.dmg_max, m.defense, m.movement_pct), ([10.0, 15.0, 0.0, 3.0, 0.0], 3.0, 5.0, 4.0, 16.0));
        let d = DotSpec::from_consts(&serde_json::json!({ "dot_dmg_water": "70", "dot_damage_time": "1000" }));
        assert_eq!((d.damages, d.interval_ms), (vec![(11, 70.0)], 1000));
        assert!(Heal::from_consts(&serde_json::json!({ "percent.hp": "30" })).hp_pct == 30.0);
    }

    #[test]
    fn effets_sur_un_joueur() {
        let mut f = Factors::default();
        f.status[1] = 3000;
        f.status[8] = 500;
        f.point_max[0] = 200;
        f.point[POINT_HP] = 200;
        f.damage_min[0] = 10;
        f.damage_max[0] = 20;
        f.defense_point[0] = 5;
        f.attack[2] = 60;
        let m = Mods { stats: [0.0, 15.0, 0.0, 0.0, 0.0], max_hp_pct: -10.0, dmg_min: 3.0, dmg_max: 5.0, defense: 4.0, movement_pct: 16.0, atk_speed_pct: -20.0, ..Default::default() };
        apply(&mut f, &m);
        assert_eq!((f.status[1], f.status[8], f.point_max[0], f.point[POINT_HP]), (4500, 580, 180, 180));
        assert_eq!((f.damage_min[0], f.damage_max[0], f.defense_point[0], f.attack[2]), (13, 25, 9, 48));
        f.point[POINT_HP] = 50;
        heal(&mut f, &Heal { hp_pct: 50.0, ..Default::default() });
        assert_eq!(f.point[POINT_HP], 140);
    }

    #[test]
    fn remplacement_et_expiration() {
        let now = Instant::now();
        let mut list = Vec::new();
        assert!(push(&mut list, effect(5, Mods { defense: 1.0, ..Default::default() }, Some(now + Duration::from_secs(1)))).is_none());
        assert!(push(&mut list, effect(5, Mods { defense: 2.0, ..Default::default() }, Some(now + Duration::from_secs(5)))).is_some());
        push(&mut list, effect(9, Mods { defense: 3.0, ..Default::default() }, None));
        assert_eq!((list.len(), total(&list).defense), (2, 5.0));
        let gone = expire(&mut list, now + Duration::from_secs(10));
        assert_eq!((gone.len(), list.len(), list[0].tid), (1, 1, 9), "la passive reste");
        assert_eq!(monster_defense(10, &Mods { defense: -4.0, defense_pct: -50.0, ..Default::default() }), 3.0);
        assert_eq!(speed(100, -20.0), 80);
        assert_eq!(speed(100, -200.0), 10);
    }

    #[test]
    fn etats_speciaux() {
        assert_eq!(parse_status("1"), status::STUN);
        assert_eq!(parse_status("1001;3"), status::SLOW, "les codes de 1000 et plus ne sont pas des états");
        assert_eq!(parse_status("9"), status::DISABLE_SKILL | status::DISABLE_NORMAL_ATK);
        assert_eq!(parse_status("17"), status::STUN_PROTECT | status::SLOW_INVINCIBLE);
        assert_eq!(parse_status("0"), 0);
        let now = Instant::now();
        let mut list = vec![
            Effect { status: status::STUN, status_until: Some(now + Duration::from_secs(3)), ..effect(1, Mods::default(), Some(now + Duration::from_secs(8))) },
            Effect { status: status::SLEEP, ..effect(2, Mods::default(), Some(now + Duration::from_secs(8))) },
        ];
        assert_eq!(active_status(&list, now), status::STUN | status::SLEEP);
        assert_eq!(active_status(&list, now + Duration::from_secs(4)), status::SLEEP, "l'étourdissement est fini, l'effet dure encore");
        assert!(lift_status(&mut list, status::SLEEP));
        assert!(!lift_status(&mut list, status::SLEEP));
        assert_eq!(active_status(&list, now), status::STUN);
    }

    #[test]
    fn critiques() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let sure = Mods { crit_rate: 100.0, crit_dmg: 180.0, ..Default::default() };
        assert_eq!(critical(&mut rng, &sure, result::SUCCESS, 10), (result::CRITICAL, 18));
        assert_eq!(critical(&mut rng, &sure, result::MISS, 0), (result::MISS, 0));
        assert_eq!(critical(&mut rng, &Mods::default(), result::SUCCESS, 10), (result::SUCCESS, 10));
        // Dégâts critiques sous 100 % : jamais moins qu'un coup normal.
        let weak = Mods { crit_rate: 100.0, crit_dmg: 10.0, ..Default::default() };
        assert_eq!(critical(&mut rng, &weak, result::SUCCESS, 10), (result::CRITICAL, 10));
        let rare = Mods { crit_rate: 3.0, crit_dmg: 120.0, ..Default::default() };
        let n = (0..10_000).filter(|_| critical(&mut rng, &rare, result::SUCCESS, 10).0 == result::CRITICAL).count();
        assert!((200..400).contains(&n), "{n} critiques sur 10 000 à 3 %");
    }

    #[test]
    fn garde_d_un_joueur() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(11);
        assert_eq!(guard(&mut rng, &Mods::default(), status::NORMAL_ATK_INVINCIBLE, 30), Guard::Evade);
        assert_eq!(guard(&mut rng, &Mods::default(), 0, 30), Guard::Hit);
        let wall = Mods { block: 100.0, ..Default::default() };
        let n = (0..10_000).filter(|_| guard(&mut rng, &wall, 0, 30) == Guard::Block).count();
        assert!((7600..8400).contains(&n), "blocage plafonné à 80 % : {n} sur 10 000");
        let mana = Mods { to_mp_rate: 100.0, to_mp: 25.0, ..Default::default() };
        let got: Vec<Guard> = (0..50).map(|_| guard(&mut rng, &mana, 0, 40)).collect();
        assert!(got.contains(&Guard::ToMp(10)) && got.contains(&Guard::Hit));
    }

    #[test]
    fn paquet_d_etat() {
        let p = archcore_net::decode_packet(&schema::CHARACTER, &status_packet(0x5000_0001, status::STUN | status::SLEEP)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(17)), (Some(1), Some(0x5000_0001), Some(0x2_0000_0001)));
    }

    #[test]
    fn paquets_d_effet() {
        let e = effect(20, Mods::default(), None);
        let p = archcore_net::decode_packet(&schema::SKILL, &add_packet(0x5000_0001, &e)).unwrap();
        assert_eq!(p.int(0), Some(op::ADD_BUFF));
        let list = p.list(8).expect("liste des effets");
        assert_eq!(list.len(), 1);
    }
}
