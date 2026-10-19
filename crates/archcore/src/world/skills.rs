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
//! Première étape : les compétences de dégâts immédiats sur une cible. Les effets durables (renforcements,
//! affaiblissements, dégâts sur la durée), les zones et les compétences passives viendront ensuite : ces
//! compétences se lancent (animation, coût, délai) sans effet.

use super::combat::{self, Attacker, Defender};
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

/// Valeurs d'un niveau de compétence (SKILL_CONST).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillLevel {
    /// Bit n : la classe `CLASS_COLUMNS[n]` peut apprendre ce niveau.
    pub classes: u16,
    pub require_level: i32,
    pub require_point: i32,
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
}

impl SkillLevel {
    pub fn from_consts(c: &serde_json::Value) -> Self {
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
        }
    }

    pub fn allows(&self, class_col: Option<usize>) -> bool {
        class_col.is_some_and(|c| self.classes & (1 << c) != 0)
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
    pub melee_attack: i64,
    pub auto_attack: bool,
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
    /// Ne vise que son lanceur (centaines du champ target).
    pub fn self_only(&self) -> bool {
        self.target >= 100
    }
    /// Vise un ennemi.
    pub fn offensive(&self) -> bool {
        !self.self_only() && (self.instant_damage() || self.nature().debuff || self.nature().damage)
    }
    pub fn can_upgrade(&self) -> bool {
        matches!(self.upgradable, 1 | 3)
    }
}

#[derive(Default)]
pub struct SkillCatalog {
    skills: HashMap<u32, SkillTemplate>,
}

impl SkillCatalog {
    pub async fn load(db: &Databases) -> anyhow::Result<Self> {
        let Ok(specs) = db.world.rows("SELECT tid, name, spec FROM skill_spec", ()).await else {
            warn!("compétences non chargées : lancer `archcore db-init` puis `archcore import`");
            return Ok(SkillCatalog::default());
        };
        let mut skills: HashMap<u32, SkillTemplate> = HashMap::new();
        for r in &specs {
            let spec = serde_json::from_str::<serde_json::Value>(&col::<String>(r, "spec")).unwrap_or_default();
            let int = |k: &str| spec.get(k).and_then(|v| v.as_str()).and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(0);
            let tid: u32 = col(r, "tid");
            skills.insert(
                tid,
                SkillTemplate {
                    tid,
                    name: col(r, "name"),
                    attribute: int("attribute"),
                    upgradable: int("upgradable"),
                    target: int("target"),
                    melee_attack: int("melee_attack"),
                    auto_attack: int("auto_attack") != 0,
                    levels: Vec::new(),
                },
            );
        }
        for r in db.world.rows("SELECT tid, level, consts FROM skill_level ORDER BY tid, level", ()).await? {
            let Some(t) = skills.get_mut(&col::<u32>(&r, "tid")) else { continue };
            let level: u32 = col(&r, "level");
            let consts = serde_json::from_str::<serde_json::Value>(&col::<String>(&r, "consts")).unwrap_or_default();
            if level as usize == t.levels.len() + 1 {
                t.levels.push(SkillLevel::from_consts(&consts));
            }
        }
        let damage = skills.values().filter(|s| s.instant_damage()).count();
        info!("compétences : {} ({damage} à dégâts immédiats)", skills.len());
        Ok(SkillCatalog { skills })
    }

    pub fn get(&self, tid: u32) -> Option<&SkillTemplate> {
        self.skills.get(&tid)
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

/// Points de compétence restants, recopiés dans les facteurs du personnage.
pub fn refresh_points(catalog: &SkillCatalog, skills: &PlayerSkills, factors: &mut Factors) {
    factors.dirt[DIRT_SKILL_POINT] = (points_for_level(factors.status[7]) - skills.spent(catalog)).max(0);
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
/// élémentaires du personnage de la même façon, doublés. Une cible plus forte retire 5 % par niveau
/// d'écart (80 % au plus) ; l'armure ne réduit que la part physique. Chaque part fait au moins 1.
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
        let x = (raw * (100.0 + pct) / 100.0 + flat) * keep * 2.0;
        out.push((SPIRIT[i].2, (x.round() as i32).max(1)));
    }
    out
}

/// Fourchettes de dégâts élémentaires d'un joueur, dans l'ordre de `SPIRIT`.
fn spirit_ranges(f: &Factors) -> [(f64, f64); 8] {
    let mut r = [(0.0, 0.0); 8];
    for (i, (_, attr, _)) in SPIRIT.iter().enumerate() {
        r[i] = (f.damage_min[*attr] as f64, f.damage_max[*attr].max(f.damage_min[*attr]) as f64);
    }
    r
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
    if owned.ready_at.is_some_and(|r| now < r) {
        debug!("[compétence] {name} : {} pas encore prête", t.name);
        return;
    }
    let target = if t.offensive() {
        let alive = st.monsters.get(target).is_some_and(|m| m.is_alive() && st.monsters.stats_of(m.tid).is_attackable());
        if !alive {
            debug!("[compétence] {name} : {} vise {target:#x}, qui n'est pas un monstre vivant", t.name);
            return;
        }
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
    Ok(())
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
    let me = p.dest.unwrap_or_else(|| p.ch.pos());
    let target: Option<Monster> = if cast.target == p.ch.id { None } else { st.monsters.get(cast.target).filter(|m| m.is_alive()).cloned() };
    if cast.target != p.ch.id && target.is_none() {
        return stop(st, sid);
    }
    if let Some(m) = &target {
        let hit_range = st.monsters.stats_of(m.tid).hit_range as f64;
        let range = if lv.range > 0.0 { lv.range } else { p.factors.attack[0].max(50) as f64 };
        let distance = combat::dist2d(me, m.pos);
        if distance > range + hit_range + REACH_TOLERANCE {
            let travel = combat::approach(st, sid, me, m, hit_range, distance, now);
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
    let target_pos = target.as_ref().map(|m| m.pos).unwrap_or(me);
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
    if p.dead || cast.target == p.ch.id || !t.instant_damage() {
        // Effets durables : pas encore appliqués.
        return Ok(());
    }
    let Some(m) = st.monsters.get(cast.target).filter(|m| m.is_alive()).cloned() else { return Ok(()) };
    let stats = st.monsters.stats_of(m.tid);
    let attacker = Attacker::of_player(p);
    let defender = Defender { level: m.level as i32, dr: 0, defense: stats.ac as f64 };
    let damages = skill_damage(&mut rand::thread_rng(), &attacker, &spirit_ranges(&p.factors), &defender, lv);
    let total: i32 = damages.iter().map(|(_, d)| *d).sum();
    let (cid, name, skill_name) = (p.ch.id, p.ch.name.clone(), t.name.clone());
    let target = st.monsters.get_mut(m.cid).unwrap();
    target.hp = (target.hp - total).max(0);
    let hp = target.hp;
    let pkt = result_packet(cid, &owned, m.cid, result_factors(hp, stats.max_mp, &damages));
    let mut watchers = combat::viewers(st, m.cid);
    if !watchers.contains(&sid) {
        watchers.push(sid);
    }
    combat::broadcast(st, &watchers, &pkt);
    super::ai::add_aggro(st, m.cid, cid, total as u64, now);
    if let Some(p) = st.players.get_mut(&sid) {
        super::recovery::in_combat(&mut p.timers, now);
        if p.attack.is_none() {
            p.attack = Some(combat::Attack { target: m.cid, ready_at: now });
        }
    }
    debug!("[compétence] {name} : {skill_name} → {} : {total} dégât(s), {hp}/{} PV", m.name, m.max_hp);
    if hp == 0 {
        combat::kill(ctx, st, sid, m.cid, now).await?;
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
            if upgrade(ctx, st, sid, skill_id).await? {
                if let Some(player) = st.players.get(&sid) {
                    player.send(s, reply(master_op::RESPONSE_UPGRADE).with(3, skill_id).with(5, 0));
                }
            }
        }
        op => warn!("[maître] #{sid} op {op} inattendu"),
    }
    Ok(())
}

/// Ce qui manque pour apprendre ou améliorer à ce niveau, sinon `None`.
fn missing(p: &Player, lv: &SkillLevel, price: i64) -> Option<(i64, &'static str)> {
    if (p.ch.level) < lv.require_level {
        return Some((learn_result::LOW_LEVEL, "niveau trop bas"));
    }
    if p.factors.dirt[DIRT_SKILL_POINT] < lv.require_point {
        return Some((learn_result::NOT_ENOUGH_POINTS, "pas assez de points de compétence"));
    }
    if (p.ch.money as i64) < price {
        return Some((learn_result::FAIL, "pas assez d'argent"));
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
    if let Some((code, why)) = missing(p, lv, lv.learn_cost) {
        info!("[maître] {} ne peut pas apprendre {} : {why}", p.ch.name, t.name);
        return Ok(code);
    }
    let money = p.ch.money as i64 - lv.learn_cost;
    let character_id = p.ch.id;
    ctx.db.characters.exec("INSERT INTO character_skills (character_id, skill_tid, level) VALUES (?, ?, 1) ON DUPLICATE KEY UPDATE level = 1", (character_id, tid)).await?;
    st.next_skill_id += 1;
    let owned = Owned { id: st.next_skill_id, tid, level: 1, ready_at: None };
    let p = st.players.get_mut(&sid).unwrap();
    p.skills.list.push(owned);
    refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    send_points(p);
    p.send_raw(add_packet(p.ch.id, &owned));
    info!("[maître] {} apprend {} ({} or)", p.ch.name, t.name, lv.learn_cost);
    super::set_money(ctx, st, sid, money).await?;
    Ok(learn_result::SUCCESS)
}

/// Améliore une compétence d'un niveau.
pub async fn upgrade(ctx: &WorldCtx, st: &mut WorldState, sid: u32, skill_id: u32) -> anyhow::Result<bool> {
    let p = &st.players[&sid];
    let Some(owned) = p.skills.by_id(skill_id).copied() else { return Ok(false) };
    let Some(t) = ctx.skills.get(owned.tid).filter(|t| t.can_upgrade() && !t.auto_attack) else { return Ok(false) };
    let Some(lv) = t.level(owned.level + 1).filter(|lv| lv.upgrade_cost > 0) else {
        send_system(st, sid, &format!("{} est déjà à son niveau maximal.", t.name));
        return Ok(false);
    };
    if let Some((_, why)) = missing(p, lv, lv.upgrade_cost) {
        info!("[maître] {} ne peut pas améliorer {} : {why}", p.ch.name, t.name);
        send_system(st, sid, &format!("Impossible d'améliorer {} : {why}.", t.name));
        return Ok(false);
    }
    let money = p.ch.money as i64 - lv.upgrade_cost;
    let level = owned.level + 1;
    ctx.db.characters.exec("UPDATE character_skills SET level = ? WHERE character_id = ? AND skill_tid = ?", (level, p.ch.id, owned.tid)).await?;
    let p = st.players.get_mut(&sid).unwrap();
    let Some(s) = p.skills.list.iter_mut().find(|s| s.id == skill_id) else { return Ok(false) };
    s.level = level;
    let updated = *s;
    refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    send_points(p);
    p.send_raw(update_packet(p.ch.id, &updated));
    info!("[maître] {} passe {} au niveau {level} ({} or)", p.ch.name, t.name, lv.upgrade_cost);
    super::set_money(ctx, st, sid, money).await?;
    Ok(true)
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
            auto_attack: false,
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
        assert!(SkillTemplate { target: 100, attribute: 10, ..Default::default() }.self_only());
    }

    #[test]
    fn points_de_competence() {
        assert_eq!(points_for_level(1), 1);
        assert_eq!(points_for_level(49), 49);
        assert_eq!(points_for_level(55), 61);
        assert_eq!(points_for_level(99), 120);
        let mut catalog = SkillCatalog::default();
        catalog.insert(dual_slash());
        let skills = PlayerSkills { list: vec![Owned { id: 1, tid: 1, level: 2, ready_at: None }], cast: None };
        assert_eq!(skills.spent(&catalog), 2);
        let mut f = Factors::default();
        f.status[7] = 10;
        refresh_points(&catalog, &skills, &mut f);
        assert_eq!(f.dirt[DIRT_SKILL_POINT], 8);
    }

    #[test]
    fn degats_d_une_competence() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let a = Attacker { level: 10, class: 1, str_: 30.0, min_damage: 10.0, max_damage: 10.0, ..Default::default() };
        let lv = dual_slash().levels[0].clone();
        // Jet : 10 + 30/3 = 20 (fourchette 20..20,5) ; × 2,1 = 42 ; armure 2.
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 10, dr: 0, defense: 2.0 }, &lv);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, RESULT_PHYSICAL);
        assert!((40..=42).contains(&d[0].1), "{d:?}");
        // Cible de 4 niveaux de plus : -20 %.
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 14, dr: 0, defense: 0.0 }, &lv);
        assert!((33..=35).contains(&d[0].1), "{d:?}");
        // Élément seul (feu fixe 4, sans dégâts de feu du personnage) : (0 + 4) × 2 = 8, pas de part physique.
        let mut fire = SkillLevel::default();
        fire.spirit[2] = (4.0, 70.0);
        let d = skill_damage(&mut rng, &a, &[(0.0, 0.0); 8], &Defender { level: 10, dr: 0, defense: 100.0 }, &fire);
        assert_eq!(d, vec![(12, 8)]);
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
