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
//!             d'AGPMCHARACTER (op 1). Code 9 : halte (ni compétence ni attaque), 10 : invisibilité ; codes 26 à 33
//!             (ajouts d'Archonia) : valeur EP8 de la compétence (`ep8_special_status`). Interruption (`disturb`) :
//!             attaque, déplacement, objet (bit propre au serveur), compétence. Règles des états : `status_rules.rs`.
//!   Combat : melee_attack 106, 109, 110 ajoutent « skill_rate » à la chance de coup critique et « critical »
//!             aux dégâts critiques (en pour cent) ; reflect_melee_attack 2 (bouclier) ajoute « skill_rate » à
//!             la chance de bloquer, comme « point.physical_block » ; convert_type 1 et 2 changent un coup reçu
//!             en PV ou en PM (chance « skill_rate », sinon « special_status_rate » ; part « damage_convert_* ») ;
//!             defense_melee_attack 2 (contre-attaque) renvoie à son auteur, avec la chance « skill_rate », les
//!             dégâts d'un coup de monstre reçu.
//!             Résistance aux critiques (« critical_defence_percent2 » de Skill_Const2, sauf la contre-attaque ;
//!             « Critical_Defense_Rate » des options) : ôtée de la chance de coup critique de l'assaillant (déduit).
//!             Résistance à l'étourdissement (« magnify.stun_resistance » de SKILL_CONST, « StunResistance » des
//!             options) : colonnes propres à Archonia, absentes des tables de l'épisode 8 : plus lues ; la case du
//!             bloc des caractéristiques secondaires (0xbc) reste à 0. L'étourdissement suit « stun_protect ».
//!   Renforcements (`point.*` de SKILL_CONST, épisode 8) : dégâts élémentaires « dmg_min_* », « dmg_max_* » ajoutés à ceux
//!             du porteur ; « physical_resistance » (réduction des dégâts physiques reçus, en pour cent) et
//!             « skill_block » (chance de bloquer une compétence) ajoutés aux facteurs de défense 0 et 10, comme les
//!             options ; « atk_range » (en mètres, ×100 comme la portée des compétences) à la portée d'attaque ;
//!             « skill_cast » et « skill_delay » (pour cent d'incantation et de relance en moins) aux facteurs
//!             d'attaque 3 et 4, comme les options du même nom (`Archonia.exe` range celles-ci dans ces facteurs) ;
//!             « cost_mp_decrease » (pour cent, négatif : PM coûtés en moins), « bonus_money » (or en plus, même unité
//!             que « bonus_exp »). Les colonnes « percent.* » de ces facteurs ne sont pas lues (une compétence au
//!             plus chacune).
//!   Paquets : AGPMSKILL op 4 (effet ajouté : identifiant de la compétence, porteur, champ 8 = mini SKILL_BUFF
//!             TID, durée, TID du lanceur) et op 5 (effet retiré : TID), aux joueurs qui voient le porteur.
//!             AGPMSKILL op 3 (porteur, champ 9 = bloc de 220 octets) au joueur seul, à l'entrée en jeu et à chaque
//!             changement : part des compétences de ses caractéristiques secondaires (i32 en 0x08 taux critique,
//!             0x0c dégâts critiques, 0x2c et 0x3c coup reçu changé en PV, 0xa4 défense ignorée, 0xa8 résistance
//!             ignorée, 0xb0 résistance aux critiques, 0xbc à l'étourdissement), sans l'équipement, que le client
//!             ajoute lui-même.
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
    /// Données de combat des compétences (champ 9) ; le client n'a pas d'autre traitement pour cette opération.
    pub const UPDATE_ATTACH_DATA: i64 = 3;
    pub const ADD_BUFF: i64 = 4;
    pub const REMOVE_BUFF: i64 = 5;
    /// Liste complète des effets d'un personnage (champ 8) : le client remplace ou ajoute chacun.
    pub const ALL_BUFFED_SKILL: i64 = 6;
    /// Bonus de niveau de toutes les compétences (champ 1, moins de 31).
    pub const MODIFIED_SKILL_LEVEL: i64 = 14;
    /// Recharge d'une compétence remise à zéro (1 identifiant, 4 TID).
    pub const INIT_COOLTIME: i64 = 15;
}

/// Bonus de niveau des compétences accepté par le client (op 14).
pub const MAX_SKILL_LEVEL_BONUS: i64 = 30;

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
    /// Créatures invoquées (`summons.rs`) : immobile (créature fixe), invoquée, apprivoisée. Le client n'accepte une
    /// invocation (AgpmSummons op 0) que si la créature porte déjà l'état de son type.
    pub const DONT_MOVE: u64 = 0x400;
    pub const SUMMONER: u64 = 0x800;
    pub const TAME: u64 = 0x1000;
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
    /// Halte (code 9 de special_status, bit 0x400 du masque de la compétence chez Archonia, qui le nomme ainsi) : ni
    /// compétence ni attaque, comme les deux bits du client qu'ArchCore envoie pour elle.
    pub const HALT: u64 = DISABLE_SKILL | DISABLE_NORMAL_ATK;
    /// Invisibilité (code 10, bit 0x200 du masque d'Archonia) : l'état « transparent » du personnage.
    pub const INVISIBLE: u64 = TRANSPARENT;
    /// Objets interdits (`disturb` 4, `skill_extra.rs`) : bit propre au serveur, jamais envoyé au client (`for_client`).
    pub const NO_ITEM: u64 = 1 << 62;
    /// Bits propres au serveur.
    pub const SERVER_ONLY: u64 = NO_ITEM;
    /// Ni déplacement ni action.
    pub const HELPLESS: u64 = STUN | FREEZE | SLEEP;
    pub const NO_MOVE: u64 = HELPLESS | HOLD;
    pub const NO_ATTACK: u64 = HELPLESS | DISABLE_NORMAL_ATK | FEAR;
    pub const NO_SKILL: u64 = HELPLESS | DISABLE_SKILL | FEAR;
    /// Hors d'atteinte des monstres.
    pub const UNTOUCHABLE: u64 = INVINCIBLE | TRANSPARENT;
}

/// États d'un code de la colonne special_status (1 étourdissement… 25 rétrécissement) ; 9 halte (compétences et attaques
/// interdites), 10 invisibilité. Les codes 26 à 33 sont des ajouts d'Archonia, absents de l'épisode 8 : aucun état
/// (`ep8_special_status` les remplace par la valeur EP8 de la compétence quand elle en a une).
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
        9 => HALT,
        10 => INVISIBLE,
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

/// États de l'épisode 8 des compétences dont Archonia a changé la colonne special_status (codes 26 à 33 qu'il a
/// ajoutés, ou un autre code) : valeur de la table EP8 de la même compétence (vide : aucun état). Les tables EP8
/// importées ont déjà ces valeurs ; la correspondance sert quand la colonne vient encore du client.
pub const EP8_SPECIAL_STATUS: [(&str, &str); 20] = [
    ("Cure", ""),
    ("Righteous Smite", "25"),
    ("Scorpion Trap", "20"),
    ("Storm Bite", "25"),
    ("Soul Bane", ""),
    ("Cry of Disorder", "21"),
    ("Painting Attack", "21"),
    ("Crimson Eye", "24"),
    ("Zone of Terrible Corpses", "22"),
    ("Eye of the Red Ranger", "22"),
    ("Spectral Storm", "21"),
    ("Iron Protector", ""),
    ("Shrinking Punch", "25"),
    ("Dragon Boon", ""),
    ("Resounding Destroyer", "20"),
    ("Lightning Frenzy Assault", "24"),
    ("Frightful Wingover", "22"),
    ("Oppressive Blow", "25"),
    ("Arena Ban", ""),
    ("Stun Immunity", ""),
];

/// Colonne special_status d'une compétence ramenée à l'épisode 8 : la valeur EP8 de la compétence quand Archonia l'a
/// changée (`EP8_SPECIAL_STATUS`), sinon la colonne sans ses codes 26 à 33.
pub fn ep8_special_status(name: &str, raw: &str) -> String {
    if let Some((_, v)) = EP8_SPECIAL_STATUS.iter().find(|(n, _)| *n == name) {
        return (*v).to_string();
    }
    raw.split(';').map(str::trim).filter(|c| !c.parse::<i64>().is_ok_and(|v| (26..=33).contains(&v))).collect::<Vec<_>>().join(";")
}

/// États portés par la colonne `disturb` de SKILL_SPEC (interruption, `skill_extra.rs`) : bits 1 attaque normale, 2
/// déplacement, 4 objet, 8 compétence (valeur modulo 1000 ; au-delà, un réglage non lu, comme pour special_status).
pub fn disturb_status(v: i64) -> u64 {
    let v = v.max(0) % 1000;
    let mut out = 0;
    if v & 0x1 != 0 {
        out |= status::DISABLE_NORMAL_ATK;
    }
    if v & 0x2 != 0 {
        out |= status::HOLD;
    }
    if v & 0x4 != 0 {
        out |= status::NO_ITEM;
    }
    if v & 0x8 != 0 {
        out |= status::DISABLE_SKILL;
    }
    out
}

/// États tels que le client les reçoit : sans les bits propres au serveur.
pub fn for_client(status: u64) -> u64 {
    status & !status::SERVER_ONLY
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
    /// Résistance aux critiques (pour cent, ôtée de la chance de coup critique de l'assaillant).
    pub crit_resist: f64,
    /// Contre-attaque : chance (pour cent) de renvoyer à son auteur les dégâts d'un coup reçu.
    pub counter: f64,
    /// Défense ignorée par les coups du porteur (pour cent) : défense physique, résistances élémentaires.
    pub ignore_defense: f64,
    pub ignore_resist: f64,
    /// Partage des dégâts : part (pour cent) des dégâts physiques et élémentaires reçus que prend le lanceur
    /// de l'effet.
    pub share_physical: f64,
    pub share_spirit: f64,
    /// Chance d'esquiver un coup (pour cent).
    pub dodge: f64,
    /// Charisme gagné en plus (pour cent, « bonus_charisma_add_rate » : Charisma Double…).
    pub charisma_bonus: f64,
    /// Défense de chaque élément (« point.def_point_* » : ragoûts…), dans l'ordre des attributs 1 à 8 (magie, eau, feu,
    /// terre, air, poison, foudre, glace), ajoutée aux résistances du porteur.
    pub spirit_defense: [f64; 8],
    /// Expérience et butin en plus (« bonus_exp », « bonus_drop_rate » : talismans de Janus, costumes), valeurs brutes de
    /// SKILL_CONST ; leur unité est réglable (`Skill.GameBonusDivisor`, `consumables.rs`).
    pub exp_bonus: f64,
    pub drop_bonus: f64,
    /// Or en plus (« bonus_money »), même unité que `exp_bonus`.
    pub gold_bonus: f64,
    /// Dégâts élémentaires (min, max) en plus (« point.dmg_min_* », « point.dmg_max_* »), ordre des attributs 1 à 8.
    pub spirit_dmg: [(f64, f64); 8],
    /// Réduction des dégâts physiques reçus (pour cent, « physical_resistance ») et chance de bloquer une compétence
    /// (« skill_block »).
    pub phys_resist: f64,
    pub skill_block: f64,
    /// Portée d'attaque en plus (unités du jeu ; la table l'exprime en mètres).
    pub atk_range: f64,
    /// Incantation et relance plus courtes (pour cent, « skill_cast », « skill_delay » ; négatif : plus longues), PM
    /// coûtés en plus (pour cent, « cost_mp_decrease » ; négatif : en moins).
    pub skill_cast: f64,
    pub skill_delay: f64,
    pub mp_cost_pct: f64,
    /// Pénétration des défenses de la cible (options « Ignore_* », points de pour cent ôtés de la chance ou de la
    /// réduction adverse) : résistance physique, blocage, blocage des compétences, esquive, résistance aux critiques,
    /// protection contre l'étourdissement.
    pub ignore_phys_resist: f64,
    pub ignore_block: f64,
    pub ignore_skill_block: f64,
    pub ignore_dodge: f64,
    pub ignore_crit_resist: f64,
    pub ignore_stun_protect: f64,
    /// Système héroïque (`heroic.rs`) : dégâts (min, max) en plus (« point.dmg_min_heroic », « point.dmg_max_heroic »),
    /// défense (« point.def_point_heroic ») et résistances mêlée, distance, magie (pour cent,
    /// « point.heroic_melee_Resistance »… : facteurs de défense 12 à 14).
    pub heroic_dmg: (f64, f64),
    pub heroic_def: f64,
    pub heroic_resist: [f64; 3],
    /// Niveaux de toutes les compétences en plus (« skill_levelup » d'une compétence de type 1, `skill_extra.rs`).
    pub skill_levels: f64,
}

/// Colonnes de défense élémentaire de SKILL_CONST, dans l'ordre des attributs 1 à 8.
pub const SPIRIT_DEFENSE_COLUMNS: [&str; 8] = ["magic", "water", "fire", "earth", "air", "poison", "thunder", "ice"];

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
/// … 17 glace, 19 héroïque : « dot_dmg_heroic », `heroic.rs`).
pub const DOT_COLUMNS: [(&str, usize); 10] =
    [("a", 9), ("magic", 10), ("water", 11), ("fire", 12), ("earth", 13), ("air", 14), ("poison", 15), ("thunder", 16), ("ice", 17), ("heroic", 19)];

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
            charisma_bonus: num(c, "bonus_charisma_add_rate"),
            spirit_defense: SPIRIT_DEFENSE_COLUMNS.map(|e| p(&format!("def_point_{e}"))),
            exp_bonus: num(c, "bonus_exp"),
            drop_bonus: num(c, "bonus_drop_rate"),
            gold_bonus: num(c, "bonus_money"),
            spirit_dmg: SPIRIT_DEFENSE_COLUMNS.map(|e| (p(&format!("dmg_min_{e}")), p(&format!("dmg_max_{e}")))),
            phys_resist: p("physical_resistance"),
            skill_block: p("skill_block"),
            atk_range: p("atk_range") * 100.0,
            skill_cast: p("skill_cast"),
            skill_delay: p("skill_delay"),
            mp_cost_pct: num(c, "cost_mp_decrease"),
            heroic_dmg: (p("dmg_min_heroic"), p("dmg_max_heroic")),
            heroic_def: p("def_point_heroic"),
            heroic_resist: [p("heroic_melee_Resistance"), p("heroic_ranged_Resistance"), p("heroic_magic_Resistance")],
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
        self.crit_resist += o.crit_resist;
        self.counter += o.counter;
        self.ignore_defense += o.ignore_defense;
        self.ignore_resist += o.ignore_resist;
        self.share_physical += o.share_physical;
        self.share_spirit += o.share_spirit;
        self.dodge += o.dodge;
        self.charisma_bonus += o.charisma_bonus;
        for i in 0..8 {
            self.spirit_defense[i] += o.spirit_defense[i];
        }
        self.exp_bonus += o.exp_bonus;
        self.drop_bonus += o.drop_bonus;
        self.gold_bonus += o.gold_bonus;
        for i in 0..8 {
            self.spirit_dmg[i].0 += o.spirit_dmg[i].0;
            self.spirit_dmg[i].1 += o.spirit_dmg[i].1;
        }
        self.phys_resist += o.phys_resist;
        self.skill_block += o.skill_block;
        self.atk_range += o.atk_range;
        self.skill_cast += o.skill_cast;
        self.skill_delay += o.skill_delay;
        self.mp_cost_pct += o.mp_cost_pct;
        self.ignore_phys_resist += o.ignore_phys_resist;
        self.ignore_block += o.ignore_block;
        self.ignore_skill_block += o.ignore_skill_block;
        self.ignore_dodge += o.ignore_dodge;
        self.ignore_crit_resist += o.ignore_crit_resist;
        self.ignore_stun_protect += o.ignore_stun_protect;
        self.heroic_dmg.0 += o.heroic_dmg.0;
        self.heroic_dmg.1 += o.heroic_dmg.1;
        self.heroic_def += o.heroic_def;
        for i in 0..3 {
            self.heroic_resist[i] += o.heroic_resist[i];
        }
        self.skill_levels += o.skill_levels;
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
    /// Session du lanceur (`MONSTER_CASTER` : un monstre, ou personne à qui revenir).
    pub caster_sid: u32,
    /// Purge répétée tant que dure l'effet (Cure).
    pub purge: Option<Purge>,
}

/// Purge répétée : sortes d'effets retirés (bits de `dispel`), chance à chaque fois, période.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Purge {
    pub kinds: u64,
    pub rate: f64,
    pub interval: Duration,
    pub next_at: Instant,
}

/// Sortes d'effets qu'une purge retire (colonne dispel_magic de SKILL_SPEC, modulo 10000 ; 10000 et plus :
/// purge répétée).
pub mod dispel {
    /// 2 (valeur exacte) : affaiblissements.
    pub const DEBUFFS: u64 = 0x2;
    pub const STUN: u64 = 0x4;
    pub const SLOW: u64 = 0x8;
    /// Tous les effets, renforcements comme affaiblissements.
    pub const ALL: u64 = 0x10;
    pub const TRANSPARENT: u64 = 0x20;
    /// Effets qui posent un état spécial.
    pub const SPECIAL: u64 = 0x40;
    /// Valeur de la colonne au-delà de laquelle la purge se répète.
    pub const REPEAT: i64 = 10000;
}

/// Sortes d'une valeur de dispel_magic ; vrai si la purge se répète.
pub fn parse_dispel(v: i64) -> (u64, bool) {
    let repeat = v >= dispel::REPEAT;
    let v = (v % dispel::REPEAT).max(0) as u64;
    (if v == dispel::DEBUFFS { dispel::DEBUFFS } else { v & !dispel::DEBUFFS }, repeat)
}

/// Un effet est retiré par une purge de ces sortes ; `hostile` : il affaiblit son porteur. Les passives restent.
pub fn dispels(e: &Effect, kinds: u64, hostile: bool) -> bool {
    use dispel::*;
    if e.ends_at.is_none() || kinds == 0 {
        return false;
    }
    kinds & ALL != 0
        || kinds & DEBUFFS != 0 && hostile
        || kinds & STUN != 0 && e.status & status::STUN != 0
        || kinds & SLOW != 0 && (e.status & status::SLOW != 0 || hostile && e.mods.movement_pct < 0.0)
        || kinds & TRANSPARENT != 0 && e.status & (status::TRANSPARENT | status::HALF_TRANSPARENT) != 0
        || kinds & SPECIAL != 0 && e.status != 0
}

/// Retire d'une liste les effets qu'une purge retire ; les rend.
pub fn purge(effects: &mut Vec<Effect>, kinds: u64, hostile: impl Fn(&Effect) -> bool) -> Vec<Effect> {
    let (gone, keep): (Vec<Effect>, Vec<Effect>) = std::mem::take(effects).into_iter().partition(|e| dispels(e, kinds, hostile(e)));
    *effects = keep;
    gone
}

/// Part d'un coup reçu que prend un autre personnage (partage des dégâts) : (dégâts gardés, dégâts pris).
pub fn share(parts: &[(usize, i32)], physical_pct: f64, spirit_pct: f64) -> (Vec<(usize, i32)>, i32) {
    let mut taken = 0;
    let kept = parts
        .iter()
        .map(|&(i, d)| {
            let pct = if i == super::combat::RESULT_PHYSICAL { physical_pct } else { spirit_pct }.clamp(0.0, 100.0);
            let t = (d as f64 * pct / 100.0) as i32;
            taken += t;
            (i, d - t)
        })
        .collect();
    (kept, taken)
}

/// Somme des modificateurs d'une liste d'effets.
pub fn total(effects: &[Effect]) -> Mods {
    let mut m = Mods::default();
    for e in effects {
        m.add(&e.mods);
    }
    m
}

/// Modificateurs de combat d'un joueur : ses effets et ceux de son équipement (options, runes).
pub fn of_player(p: &super::Player) -> Mods {
    let mut m = total(&p.skills.effects);
    m.add(&p.gear.mods);
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

/// Coup critique sur un coup réussi : (résultat, dégâts). `resist` : résistance aux critiques de la cible, ôtée
/// de la chance de l'assaillant (règle déduite : la fenêtre du personnage montre les deux en pour cent, côte à côte ;
/// le client n'en dit pas plus).
pub fn critical(rng: &mut impl rand::Rng, m: &Mods, resist: f64, outcome: i64, damage: i32) -> (i64, i32) {
    let rate = m.crit_rate - resist;
    if outcome != result::SUCCESS || rate <= 0.0 || rng.gen_range(0.0..100.0) >= rate {
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
    if roll(m.dodge) {
        return Guard::Evade;
    }
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
    build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(17, for_client(status) as i64))
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
    for (i, d) in m.spirit_defense.iter().enumerate() {
        f.defense_point[i + 1] = (f.defense_point[i + 1] as f64 + d) as i32;
    }
    for (i, (lo, hi)) in m.spirit_dmg.iter().enumerate() {
        f.damage_min[i + 1] = (f.damage_min[i + 1] as f64 + lo).max(0.0) as i32;
        f.damage_max[i + 1] = (f.damage_max[i + 1] as f64 + hi).max(f.damage_min[i + 1] as f64) as i32;
    }
    // Facteurs de défense « taux » : 0 résistance physique, 10 blocage des compétences (ceux des options aussi).
    f.defense_rate[0] = (f.defense_rate[0] as f64 + m.phys_resist) as i32;
    f.defense_rate[10] = (f.defense_rate[10] as f64 + m.skill_block) as i32;
    if m.atk_range != 0.0 {
        f.attack[0] = (f.attack[0] as f64 + m.atk_range).max(50.0) as i32;
    }
    // Héroïque (`heroic.rs`) : dégâts et défense d'attribut 11, résistances mêlée, distance, magie (12 à 14).
    f.damage_min[super::heroic::ATTR] = (f.damage_min[super::heroic::ATTR] as f64 + m.heroic_dmg.0).max(0.0) as i32;
    f.damage_max[super::heroic::ATTR] = (f.damage_max[super::heroic::ATTR] as f64 + m.heroic_dmg.1).max(0.0) as i32;
    f.defense_point[super::heroic::ATTR] = (f.defense_point[super::heroic::ATTR] as f64 + m.heroic_def).max(0.0) as i32;
    for (i, r) in m.heroic_resist.iter().enumerate() {
        f.defense_rate[super::heroic::ATTR + 1 + i] = (f.defense_rate[super::heroic::ATTR + 1 + i] as f64 + r) as i32;
    }
    f.attack[3] = (f.attack[3] as f64 + m.skill_cast) as i32;
    f.attack[4] = (f.attack[4] as f64 + m.skill_delay) as i32;
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

/// Dégâts élémentaires (min, max) d'un monstre sous effets, ordre des attributs 1 à 8.
pub fn monster_spirit(spirit: &[(i32, i32); 8], m: &Mods) -> [(f64, f64); 8] {
    std::array::from_fn(|i| {
        let lo = (spirit[i].0 as f64 + m.spirit_dmg[i].0).max(0.0);
        (lo, (spirit[i].1.max(spirit[i].0) as f64 + m.spirit_dmg[i].1).max(lo))
    })
}

/// Allonge de `extra_ms` un affaiblissement posé (option « Add_Debuff_Duration » de son lanceur) : durée, fin et fin de
/// ses états.
pub fn lengthen(e: &mut Effect, extra_ms: u64) {
    if extra_ms == 0 || e.ends_at.is_none() {
        return;
    }
    let extra = Duration::from_millis(extra_ms);
    e.duration_ms += extra_ms;
    e.ends_at = e.ends_at.map(|t| t + extra);
    e.status_until = e.status_until.map(|t| t + extra);
}

/// Chance (pour cent) d'une défense de la cible diminuée de la pénétration de l'assaillant, jamais négative.
pub fn pierced(rate: f64, ignore: f64) -> f64 {
    (rate - ignore.max(0.0)).max(0.0)
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

/// Mini SKILL_BUFF d'un effet : TID, durée restante (ms), TID du lanceur.
fn buff_mini(e: &Effect, now: Instant) -> Vec<u8> {
    let b = &mini::SKILL_BUFF;
    let left = e.ends_at.map_or(e.duration_ms, |t| t.saturating_duration_since(now).as_millis() as u64);
    build_mini(b, &Fields::new(b).with(0, e.tid).with(1, left as i64).with(2, e.caster_tid))
}

/// AGPMSKILL op 6 : tous les effets annoncés de `owner` (à l'entrée en jeu, à son apparition chez un autre joueur).
pub fn all_buffs_packet(owner: u32, list: &[&Effect], now: Instant) -> Vec<u8> {
    let buffs: Vec<Value> = list.iter().take(96).map(|e| Value::Bytes(buff_mini(e, now))).collect();
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::ALL_BUFFED_SKILL).with(2, base(owner)).with(8, buffs))
}

/// AGPMSKILL op 15 : la recharge de la compétence `skill_id` (TID `tid`) de `owner` est remise à zéro.
pub fn init_cooltime_packet(owner: u32, skill_id: u32, tid: u32) -> Vec<u8> {
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::INIT_COOLTIME).with(1, skill_id).with(2, base(owner)).with(4, tid))
}

/// AGPMSKILL op 14 : bonus de niveau de toutes les compétences de `owner` (options « skill_levelup » portées).
pub fn skill_level_bonus_packet(owner: u32, bonus: i64) -> Vec<u8> {
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::MODIFIED_SKILL_LEVEL).with(1, bonus.clamp(0, MAX_SKILL_LEVEL_BONUS)).with(2, base(owner)))
}

/// AGPMSKILL op 5 : effet retiré de `owner`.
pub fn remove_packet(owner: u32, e: &Effect) -> Vec<u8> {
    let b = &mini::SKILL_BUFF;
    let buff = build_mini(b, &Fields::new(b).with(0, e.tid));
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::REMOVE_BUFF).with(1, e.skill_id).with(2, base(owner)).with(8, vec![Value::Bytes(buff)]))
}

/// Longueur du bloc des données de combat des compétences (champ 9 d'AGPMSKILL) : le client en copie toujours
/// 220 octets, sans vérifier la longueur.
pub const ATTACH_LEN: usize = 220;
/// Décalages dans ce bloc (i32) des valeurs que lit la fenêtre du personnage, dans l'ordre de `attach_values`.
pub const ATTACH_OFFSETS: [usize; 8] = [0x08, 0x0c, 0x2c, 0x3c, 0xa4, 0xa8, 0xb0, 0xbc];

/// Part des compétences des caractéristiques secondaires : taux et dégâts critiques, coup reçu changé en PV
/// (chance, part), défense et résistances ignorées, résistance aux critiques ; la résistance à l'étourdissement (0xbc)
/// reste à 0 (colonne propre à Archonia, plus lue).
pub fn attach_values(m: &Mods) -> [i32; 8] {
    [m.crit_rate, m.crit_dmg, m.to_hp_rate, m.to_hp, m.ignore_defense, m.ignore_resist, m.crit_resist, 0.0].map(|v| v.round() as i32)
}

/// Bloc de 220 octets : les valeurs à leurs décalages, le reste à zéro.
pub fn attach_block(values: &[i32; 8]) -> Vec<u8> {
    let mut b = vec![0u8; ATTACH_LEN];
    for (off, v) in ATTACH_OFFSETS.iter().zip(values) {
        b[*off..*off + 4].copy_from_slice(&v.to_le_bytes());
    }
    b
}

/// AGPMSKILL op 3 : données de combat des compétences de `owner` (champ 9), que le client range telles quelles.
pub fn attach_packet(owner: u32, values: &[i32; 8]) -> Vec<u8> {
    let k = &schema::SKILL;
    build_packet(k, &Fields::new(k).with(0, op::UPDATE_ATTACH_DATA).with(2, base(owner)).with(9, attach_block(values)))
}

/// Envoie au joueur la part des compétences de ses caractéristiques secondaires (effets, passives comprises,
/// sans l'équipement : le client ajoute lui-même les options de ses objets), si elle a changé ou si `force`.
pub fn sync_attach(p: &mut super::Player, force: bool) {
    let values = attach_values(&total(&p.skills.effects));
    if !force && values == p.skills.attach_sent {
        return;
    }
    p.skills.attach_sent = values;
    p.send_raw(attach_packet(p.ch.id, &values));
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn effect(tid: u32, mods: Mods, ends_at: Option<Instant>) -> Effect {
        Effect { skill_id: 1, tid, level: 1, caster_tid: 96, duration_ms: 1000, ends_at, mods, dot: None, status: 0, status_until: None, caster_sid: MONSTER_CASTER, purge: None }
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
    fn renforcements_de_l_episode_8() {
        // Ancient's Favour : +30 de feu ; Death Pact : +50 % de résistance physique et de blocage des compétences ;
        // portée +3 m ; incantation −10 %, relance +5 % ; Mana Effect : −10 % de PM.
        let c = serde_json::json!({ "point.dmg_min_fire": "30", "point.dmg_max_fire": "30", "point.physical_resistance": "50", "point.skill_block": "50",
            "point.atk_range": "3", "point.skill_cast": "10", "point.skill_delay": "-5", "cost_mp_decrease": "-10", "bonus_money": "20" });
        let m = Mods::from_consts(&c);
        assert_eq!((m.spirit_dmg[2], m.phys_resist, m.skill_block, m.atk_range), ((30.0, 30.0), 50.0, 50.0, 300.0));
        assert_eq!((m.skill_cast, m.skill_delay, m.mp_cost_pct, m.gold_bonus), (10.0, -5.0, -10.0, 20.0));
        let mut f = Factors::default();
        f.attack[0] = 180;
        f.defense_rate[0] = 5;
        f.damage_min[3] = 2;
        f.damage_max[3] = 4;
        apply(&mut f, &m);
        assert_eq!((f.damage_min[3], f.damage_max[3], f.defense_rate[0], f.defense_rate[10]), (32, 34, 55, 50));
        assert_eq!((f.attack[0], f.attack[3], f.attack[4]), (480, 10, -5));
        // Portée réduite de 40 m (Venom Spray) : 50 unités au moins.
        apply(&mut f, &Mods { atk_range: -4000.0, ..Default::default() });
        assert_eq!(f.attack[0], 50);
        // Monstre : ses dégâts élémentaires sous effets.
        let mut weak = Mods::default();
        weak.spirit_dmg[1] = (-50.0, -50.0);
        weak.spirit_dmg[2] = (5.0, 5.0);
        let spirit = monster_spirit(&[(0, 0), (10, 20), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)], &weak);
        assert_eq!((spirit[1], spirit[2]), ((0.0, 0.0), (5.0, 5.0)));
        assert_eq!((pierced(20.0, 5.0), pierced(3.0, 5.0), pierced(10.0, -4.0)), (15.0, 0.0, 10.0));
    }

    #[test]
    fn affaiblissement_allonge() {
        let now = Instant::now();
        let mut e = Effect { status: status::STUN, status_until: Some(now + Duration::from_secs(2)), ..effect(1, Mods::default(), Some(now + Duration::from_secs(5))) };
        lengthen(&mut e, 3000);
        assert_eq!((e.duration_ms, e.ends_at, e.status_until), (4000, Some(now + Duration::from_secs(8)), Some(now + Duration::from_secs(5))));
        let mut passive = effect(2, Mods::default(), None);
        lengthen(&mut passive, 3000);
        assert_eq!(passive.duration_ms, 1000, "une passive ne s'allonge pas");
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
        assert_eq!((parse_status("9"), parse_status("10")), (status::HALT, status::INVISIBLE), "halte, invisibilité");
        // Codes d'Archonia ramenés à l'épisode 8 : valeur EP8 de la compétence, sinon codes 26 à 33 écartés.
        assert_eq!(ep8_special_status("Scorpion Trap", "29"), "20");
        assert_eq!(ep8_special_status("Cry of Disorder", "24"), "21");
        assert_eq!(ep8_special_status("Storm Bite", "29"), "25");
        assert_eq!(ep8_special_status("Cure", "26"), "");
        assert_eq!(ep8_special_status("Crimson Eye", "24"), "24");
        assert_eq!(ep8_special_status("Autre", "1;31;1003"), "1;1003");
        assert_eq!(parse_status(&ep8_special_status("Scorpion Trap", "29")), status::HOLD);
        assert_eq!(parse_status("29;33"), 0, "codes d'Archonia : aucun état");
        // Interruption : 11 = attaque, déplacement, compétence ; 1002 : déplacement ; 4 : objets (bit propre au serveur).
        assert_eq!(disturb_status(11), status::DISABLE_NORMAL_ATK | status::HOLD | status::DISABLE_SKILL);
        assert_eq!(disturb_status(1002), status::HOLD);
        assert_eq!(for_client(disturb_status(15)), status::DISABLE_NORMAL_ATK | status::HOLD | status::DISABLE_SKILL);
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
        assert_eq!(critical(&mut rng, &sure, 0.0, result::SUCCESS, 10), (result::CRITICAL, 18));
        assert_eq!(critical(&mut rng, &sure, 0.0, result::MISS, 0), (result::MISS, 0));
        assert_eq!(critical(&mut rng, &Mods::default(), 0.0, result::SUCCESS, 10), (result::SUCCESS, 10));
        // Dégâts critiques sous 100 % : jamais moins qu'un coup normal.
        let weak = Mods { crit_rate: 100.0, crit_dmg: 10.0, ..Default::default() };
        assert_eq!(critical(&mut rng, &weak, 0.0, result::SUCCESS, 10), (result::CRITICAL, 10));
        let rare = Mods { crit_rate: 3.0, crit_dmg: 120.0, ..Default::default() };
        let n = (0..10_000).filter(|_| critical(&mut rng, &rare, 0.0, result::SUCCESS, 10).0 == result::CRITICAL).count();
        assert!((200..400).contains(&n), "{n} critiques sur 10 000 à 3 %");
    }

    #[test]
    fn resistance_aux_critiques() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(13);
        let sure = Mods { crit_rate: 100.0, crit_dmg: 150.0, ..Default::default() };
        assert_eq!(critical(&mut rng, &sure, 100.0, result::SUCCESS, 10), (result::SUCCESS, 10), "résistance égale à la chance");
        let half = Mods { crit_rate: 30.0, ..sure };
        let n = (0..10_000).filter(|_| critical(&mut rng, &half, 20.0, result::SUCCESS, 10).0 == result::CRITICAL).count();
        assert!((850..1150).contains(&n), "30 % moins 20 % : {n} critiques sur 10 000");
        // Résistance négative (affaiblissement de la cible) : la chance augmente.
        let n = (0..10_000).filter(|_| critical(&mut rng, &half, -20.0, result::SUCCESS, 10).0 == result::CRITICAL).count();
        assert!((4700..5300).contains(&n), "30 % plus 20 % : {n} critiques sur 10 000");
    }

    #[test]
    fn bloc_des_competences() {
        let m = Mods { crit_rate: 3.0, crit_dmg: 120.0, to_hp_rate: 5.0, to_hp: 10.0, ignore_defense: 4.0, ignore_resist: 2.0, crit_resist: 16.0, defense: 9.0, ..Default::default() };
        let v = attach_values(&m);
        assert_eq!(v, [3, 120, 5, 10, 4, 2, 16, 0], "résistance à l'étourdissement (Archonia) : 0");
        let p = archcore_net::decode_packet(&schema::SKILL, &attach_packet(0x5000_0001, &v)).unwrap();
        assert_eq!(p.int(0), Some(op::UPDATE_ATTACH_DATA));
        let owner = archcore_net::decode_mini(&mini::SKILL_BASE, p.bytes(2).unwrap()).unwrap();
        assert_eq!((owner.int(0), owner.int(1)), (Some(BASE_CHARACTER), Some(0x5000_0001)));
        let b = p.bytes(9).expect("bloc du champ 9");
        assert_eq!(b.len(), ATTACH_LEN);
        let at = |off: usize| i32::from_le_bytes(b[off..off + 4].try_into().unwrap());
        assert_eq!((at(0x08), at(0x0c), at(0x2c), at(0x3c)), (3, 120, 5, 10));
        assert_eq!((at(0xa4), at(0xa8), at(0xb0), at(0xbc)), (4, 2, 16, 0));
        let rest: u32 = (0..ATTACH_LEN).step_by(4).filter(|o| !ATTACH_OFFSETS.contains(o)).map(|o| at(o).unsigned_abs()).sum();
        assert_eq!(rest, 0, "le reste du bloc est nul");
        assert!(p.int(1).is_none() && p.list(8).is_none(), "ni compétence ni effets");
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
    fn purges() {
        assert_eq!(parse_dispel(2), (dispel::DEBUFFS, false), "Cure des monstres");
        assert_eq!(parse_dispel(10002), (dispel::DEBUFFS, true), "Cure : répétée");
        assert_eq!(parse_dispel(12), (dispel::STUN | dispel::SLOW, false), "Assistance, Disentangle");
        assert_eq!(parse_dispel(16), (dispel::ALL, false), "Antimagic");
        assert_eq!(parse_dispel(0), (0, false));
        let now = Instant::now();
        let later = Some(now + Duration::from_secs(9));
        let stun = Effect { status: status::STUN, ..effect(1, Mods::default(), later) };
        let slow = effect(2, Mods { movement_pct: -15.0, ..Default::default() }, later);
        let buff = effect(3, Mods { defense: 5.0, ..Default::default() }, later);
        let passive = effect(4, Mods { defense: 1.0, ..Default::default() }, None);
        let hostile = |e: &Effect| e.tid <= 2;
        let mut list = vec![stun.clone(), slow.clone(), buff.clone(), passive.clone()];
        assert_eq!(purge(&mut list, dispel::STUN, hostile).iter().map(|e| e.tid).collect::<Vec<_>>(), vec![1]);
        assert_eq!(purge(&mut list, dispel::SLOW, hostile).iter().map(|e| e.tid).collect::<Vec<_>>(), vec![2], "ralentissement sans état : par la vitesse");
        assert!(purge(&mut list, dispel::DEBUFFS, hostile).is_empty(), "plus d'affaiblissement");
        assert_eq!(purge(&mut list, dispel::ALL, hostile).iter().map(|e| e.tid).collect::<Vec<_>>(), vec![3], "la passive reste");
        assert_eq!(list.len(), 1);
        let mut list = vec![stun, slow, buff];
        assert_eq!(purge(&mut list, dispel::DEBUFFS, hostile).len(), 2, "Cure : les deux affaiblissements");
    }

    #[test]
    fn partage_des_degats() {
        // Martyr's Mark niveau 7 : 40 % des dégâts physiques ; Divine Protection niveau 1 : 10 % des élémentaires.
        let parts = [(super::super::combat::RESULT_PHYSICAL, 50), (12, 20)];
        assert_eq!(share(&parts, 40.0, 0.0), (vec![(9, 30), (12, 20)], 20));
        assert_eq!(share(&parts, 0.0, 10.0), (vec![(9, 50), (12, 18)], 2));
        assert_eq!(share(&parts, 0.0, 0.0), (parts.to_vec(), 0));
    }

    #[test]
    fn esquive_de_l_equipement() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(5);
        let agile = Mods { dodge: 30.0, ..Default::default() };
        let n = (0..10_000).filter(|_| guard(&mut rng, &agile, 0, 30) == Guard::Evade).count();
        assert!((2700..3300).contains(&n), "esquive de 30 % : {n} sur 10 000");
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

    #[test]
    fn liste_recharge_et_bonus_de_niveau() {
        let now = Instant::now();
        let (a, b) = (effect(20, Mods::default(), Some(now + Duration::from_secs(5))), effect(21, Mods::default(), Some(now + Duration::from_secs(9))));
        let p = archcore_net::decode_packet(&schema::SKILL, &all_buffs_packet(0x10, &[&a, &b], now)).unwrap();
        assert_eq!((p.int(0), p.list(8).map(|l| l.len())), (Some(op::ALL_BUFFED_SKILL), Some(2)));
        let p = archcore_net::decode_packet(&schema::SKILL, &all_buffs_packet(0x10, &[], now)).unwrap();
        assert_eq!(p.list(8).map(|l| l.len()).unwrap_or(0), 0, "liste vide");
        let p = archcore_net::decode_packet(&schema::SKILL, &init_cooltime_packet(0x10, 7, 1439)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(4)), (Some(15), Some(7), Some(1439)));
        let p = archcore_net::decode_packet(&schema::SKILL, &skill_level_bonus_packet(0x10, 45)).unwrap();
        assert_eq!((p.int(0), p.int(1)), (Some(14), Some(MAX_SKILL_LEVEL_BONUS)), "moins de 31");
    }
}
