//! Combat au corps à corps contre les monstres.
//!
//! Le client désigne sa cible par AGPMOPTIMIZEDCHARACTION (0x41, drapeaux 2 : 0 attaquant, 1 cible,
//! 6 attaque forcée). Le serveur porte ensuite les coups, au rythme de la vitesse d'attaque ; hors de portée,
//! il fait d'abord marcher le joueur jusqu'à la cible. Un déplacement demandé par le client interrompt
//! l'attaque.
//!
//! Chaque coup est diffusé aux joueurs qui voient la cible par le même paquet : 2 résultat, 3 facteurs,
//! 5 combo, 6 attaque forcée. Le client lit le champ 3 avec le schéma complet AGPMCHAR_FACTORS : le
//! sous-paquet 0 (« résultat ») porte les dégâts affichés, le sous-paquet 3 les PV restants de la cible.
//!
//! Toucher : rapport des niveaux et de la précision (AR) de l'assaillant à l'esquive (DR) de la cible, celle d'un monstre
//! comprise (« DR » de CharacterDataTable, plus les « evade_rate » de ses effets). Dégâts physiques : jet de l'arme et de
//! la caractéristique, moins la défense, puis moins la résistance physique de la cible en pour cent (« PhysicalAttackResist »
//! des options et « physical_resistance » des compétences pour un joueur, 80 % au plus ; « Physical_Resistance » de
//! CharacterDataTable pour un monstre, 100 % au plus), diminuée de l'« Ignore_PhysicalAttackResist » de l'assaillant
//! (points ôtés). Règle choisie : les tables de l'épisode 8 portent la résistance, aucun binaire ne montre son calcul.
//!
//! Mort d'un monstre : statut d'action « mort », expérience au tueur, cadavre retiré au bout de 20 s, puis
//! réapparition à sa place après le délai de son point d'apparition. Les monstres frappés ripostent
//! (`ai.rs`). Mort d'un joueur : statut « mort » ; AGPMCHARACTER op 9 (résurrection au village) le ramène
//! au village de départ de sa race avec tous ses PV, invincible 10 s.

use super::ai;
use super::monsters::{Life, Monster, MonsterRegistry};
use super::{character_remove_packet, send_system, Player, WorldCtx, WorldState};
use crate::character::{factors_mini, POINT_HP};
use crate::{debug, info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, Fields, Packet, Vec3};
use rand::Rng;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Résultat d'une action (champ 2).
pub mod result {
    pub const MISS: i64 = 1;
    pub const SUCCESS: i64 = 5;
    pub const CRITICAL: i64 = 6;
    pub const EVADE: i64 = 7;
    pub const BLOCK: i64 = 9;
    pub const COUNTER_ATTACK: i64 = 12;
}

/// Classes (champ « type » des facteurs, 3e valeur) ; les autres, dont le guerrier (1), frappent à la force.
pub mod class {
    pub const ARCHER: i32 = 2;
    pub const MAGE: i32 = 4;
}

/// Opération AGPMCHARACTER « montée de niveau ».
const CHARACTER_LEVEL_UP: i64 = 5;
/// Opération AGPMCHARACTER du client « ressusciter au village ».
pub const REQUEST_RESURRECTION_TOWN: i64 = 9;
/// AGPMCHARACTER champ 11 : statut d'action « mort ».
const ACTION_STATUS_DEAD: i64 = 2;
/// Cadence de la boucle de combat.
pub const TICK: Duration = Duration::from_millis(100);
/// Durée d'affichage d'un cadavre.
pub const CORPSE_TIME: Duration = Duration::from_secs(20);
/// Réapparition au plus tôt, même si le point d'apparition indique moins.
const MIN_RESPAWN_SEC: i64 = 5;
/// Marge ajoutée à la portée, pour les positions arrondies du client.
pub(super) const REACH_TOLERANCE: f64 = 50.0;
/// Durée au bout de laquelle une approche est revue (la cible a pu bouger).
const MAX_APPROACH: Duration = Duration::from_secs(4);
/// Drapeau de déplacement « course ».
pub const MOVE_FAST: i64 = 0x10;
/// Niveau maximal d'un personnage.
const MAX_LEVEL: i32 = 120;
/// Invincibilité après une résurrection.
pub const RESURRECTION_GRACE: Duration = Duration::from_secs(10);

/// Attaque en cours d'un joueur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attack {
    pub target: u32,
    /// Prochain coup possible (fin du coup précédent ou de l'approche).
    pub ready_at: Instant,
    /// Attaque forcée (Ctrl, champ 6 de l'action) : nécessaire contre un joueur qui n'est pas déjà un ennemi.
    pub forced: bool,
}

/// Ce qui compte pour toucher et blesser, côté attaquant.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Attacker {
    pub level: i32,
    pub ar: i32,
    pub class: i32,
    pub str_: f64,
    pub dex: f64,
    pub int: f64,
    pub min_damage: f64,
    pub max_damage: f64,
    /// Défense physique et résistances élémentaires de la cible ignorées (pour cent).
    pub ignore_defense: f64,
    pub ignore_resist: f64,
    /// Résistance physique et blocage des compétences de la cible ignorés (points de pour cent ôtés).
    pub ignore_phys_resist: f64,
    pub ignore_skill_block: f64,
    /// Dégâts héroïques (min, max) et nature de l'assaillant : la résistance héroïque de la cible ne vaut que contre un
    /// joueur (`heroic.rs`).
    pub heroic: (f64, f64),
    pub player: bool,
}

/// Côté défenseur.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Defender {
    pub level: i32,
    pub dr: i32,
    pub defense: f64,
    /// Résistances élémentaires (pour cent, dans l'ordre des attributs 1 à 8), déjà plafonnées : 80 pour un
    /// joueur, 100 pour un monstre.
    pub resist: [i32; 8],
    /// Résistance physique (pour cent des dégâts physiques ôtés, plafonnée comme les résistances) et chance de bloquer
    /// une compétence (pour cent).
    pub phys_resist: f64,
    pub skill_block: f64,
    /// Défense héroïque et résistances héroïques mêlée, distance, magie (pour cent, `heroic.rs`).
    pub heroic_def: f64,
    pub heroic_resist: [f64; 3],
}

impl Defender {
    /// Un joueur : niveau, esquive (DR), défense physique, résistances (80 % au plus), résistance physique (facteur de
    /// défense 0, 80 % au plus) et blocage des compétences (facteur 10) : options et effets compris.
    pub fn of_player(f: &crate::character::Factors) -> Self {
        Defender {
            level: f.status[7],
            dr: f.point_max[9],
            defense: f.defense_point[0] as f64,
            resist: spirit_resist(f).map(|r| r.clamp(0, 80)),
            phys_resist: f.defense_rate[0].clamp(0, 80) as f64,
            skill_block: f.defense_rate[10].max(0) as f64,
            heroic_def: super::heroic::defense_of(f).0,
            heroic_resist: super::heroic::defense_of(f).1,
        }
    }

    /// Un monstre : esquive (« DR », plus les « evade_rate » de ses effets), défense physique sous ses effets, résistances
    /// de CharacterDataTable (100 % au plus), résistance physique (« Physical_Resistance ») et blocage des compétences
    /// (« Skill_Block »), effets compris.
    pub fn of_monster(level: i32, s: &super::monsters::MonsterStats, m: &super::effects::Mods) -> Self {
        Defender {
            level,
            dr: (s.dr as f64 + m.evade).max(0.0) as i32,
            defense: super::effects::monster_defense(s.ac, m),
            resist: s.resist.map(|r| r.clamp(0, 100)),
            phys_resist: (s.phys_resist as f64 + m.phys_resist).clamp(0.0, 100.0),
            skill_block: (s.skill_block as f64 + m.skill_block).max(0.0),
            heroic_def: (s.heroic_def as f64 + m.heroic_def).max(0.0),
            heroic_resist: m.heroic_resist,
        }
    }
}

impl Attacker {
    /// Cible vue par cet attaquant : défense physique et résistances diminuées de la part qu'il ignore.
    pub fn pierce(&self, d: &Defender) -> Defender {
        let keep = |pct: f64| (100.0 - pct.clamp(0.0, 100.0)) / 100.0;
        Defender {
            defense: d.defense * keep(self.ignore_defense),
            resist: d.resist.map(|r| (r as f64 * keep(self.ignore_resist)).round() as i32),
            phys_resist: super::effects::pierced(d.phys_resist, self.ignore_phys_resist),
            skill_block: super::effects::pierced(d.skill_block, self.ignore_skill_block),
            ..*d
        }
    }

    pub fn of_player(p: &Player) -> Self {
        let f = &p.factors;
        let m = super::effects::of_player(p);
        Attacker {
            ignore_defense: m.ignore_defense,
            ignore_resist: m.ignore_resist,
            ignore_phys_resist: m.ignore_phys_resist,
            ignore_skill_block: m.ignore_skill_block,
            level: f.status[7],
            ar: f.point_max[8],
            class: f.ftype[2],
            str_: f.status[1] as f64 / 100.0,
            dex: f.status[3] as f64 / 100.0,
            int: f.status[2] as f64 / 100.0,
            min_damage: f.damage_min[0] as f64,
            max_damage: f.damage_max[0] as f64,
            heroic: super::heroic::damage_of(f),
            player: true,
        }
    }

    /// Un monstre (ou une invocation) : niveau, précision, dégâts physiques et héroïques sous ses effets.
    pub fn of_monster(level: i32, s: &super::monsters::MonsterStats, m: &super::effects::Mods) -> Self {
        let (min_damage, max_damage) = super::effects::monster_damage(s.min_damage, s.max_damage, m);
        let heroic = ((s.heroic_min as f64 + m.heroic_dmg.0).max(0.0), (s.heroic_max as f64 + m.heroic_dmg.1).max(0.0));
        Attacker { level, ar: s.ar, min_damage, max_damage, heroic, ..Default::default() }
    }

    /// Fourchette de dégâts physiques : arme, plus la caractéristique principale de la classe.
    pub fn damage_range(&self) -> (f64, f64) {
        let (lo, hi) = match self.class {
            class::ARCHER => {
                let x = self.dex * 2.0 / 9.0 + self.str_ / 9.0;
                (x, x * 1.05)
            }
            class::MAGE => (self.int * 0.4, self.int * 0.42),
            _ => (self.str_ / 3.0, self.str_ * 0.35),
        };
        let (lo, hi) = ((self.min_damage + lo).max(0.0), (self.max_damage + hi).max(0.0));
        (lo.min(hi), hi)
    }
}

/// Chance de toucher, en pour cent (10 à 95) : rapport des niveaux et de la précision (AR) à l'esquive
/// (DR) ; une cible de niveau supérieur fait perdre 7 points par niveau d'écart.
pub fn hit_chance(a: &Attacker, d: &Defender) -> i32 {
    let (al, dl) = (a.level.max(1) as f64, d.level.max(1) as f64);
    let mut chance = if a.ar + d.dr > 0 { (al / (al + dl) * (a.ar as f64 / (a.ar + d.dr) as f64) * 290.0) as i32 } else { 0 };
    if d.level > a.level {
        chance = 90 - 7 * (d.level - a.level);
    }
    chance.clamp(10, 95)
}

/// Chance de toucher d'un monstre (30 à 95 %) : même rapport, mais une cible de niveau supérieur garde au
/// moins 85 - 3 × l'écart.
pub fn monster_hit_chance(a: &Attacker, d: &Defender) -> i32 {
    let (al, dl) = (a.level.max(1) as f64, d.level.max(1) as f64);
    let mut chance = if a.ar + d.dr > 0 { (al / (al + dl) * (a.ar as f64 / (a.ar + d.dr) as f64) * 290.0) as i32 } else { 0 };
    if d.level > a.level {
        chance = chance.max(85 - 3 * (d.level - a.level));
    }
    chance.clamp(30, 95)
}

/// Un coup de joueur : (résultat, dégâts). Les dégâts touchent au moins 1 PV.
pub fn strike(rng: &mut impl Rng, a: &Attacker, d: &Defender) -> (i64, i32) {
    strike_at(rng, hit_chance(a, d), a, d)
}

/// Un coup avec une chance de toucher donnée (en pour cent).
pub fn strike_at(rng: &mut impl Rng, chance: i32, a: &Attacker, d: &Defender) -> (i64, i32) {
    if rng.gen_range(0..100) >= chance {
        return (result::MISS, 0);
    }
    let (lo, hi) = a.damage_range();
    let raw = if hi > lo { rng.gen_range(lo..=hi) } else { lo };
    (result::SUCCESS, physical_hit(raw - d.defense, d))
}

/// Dégâts physiques après la défense, diminués de la résistance physique de la cible ; au moins 1.
pub fn physical_hit(after_defense: f64, d: &Defender) -> i32 {
    ((after_defense * (100.0 - d.phys_resist.clamp(0.0, 100.0)) / 100.0).round() as i32).max(1)
}

/// Un coup bloqué ou esquivé par un joueur que frappe un autre joueur (combat entre joueurs, de guildes) : sa chance
/// d'esquive (options et effets, moins l'« Ignore_EvadeRate » / « Ignore_DodgeRate » de l'assaillant) puis de blocage
/// (compétences, « BlockRate » des options, moins l'« Ignore_BlockRate »), 80 % au plus chacune comme contre un monstre.
pub fn player_guard(rng: &mut impl Rng, target: &super::effects::Mods, block_factor: i32, attacker: &super::effects::Mods) -> Option<i64> {
    let dodge = super::effects::pierced(target.dodge, attacker.ignore_dodge).min(80.0);
    let block = super::effects::pierced(target.block + block_factor as f64, attacker.ignore_block).min(80.0);
    if dodge > 0.0 && rng.gen_range(0.0..100.0) < dodge {
        return Some(result::EVADE);
    }
    (block > 0.0 && rng.gen_range(0.0..100.0) < block).then_some(result::BLOCK)
}

/// Coup normal d'un joueur sur un autre (combat entre joueurs, de guildes) : toucher, esquive et blocage de la cible
/// (`player_guard`), critique (résistance de la cible moins l'« Ignore_CriticalDefenseRate » de l'assaillant), part
/// élémentaire. Rend (résultat, dégâts par nature).
pub fn versus(rng: &mut impl Rng, p: &Player, t: &Player) -> (i64, Vec<(usize, i32)>) {
    let attacker = Attacker::of_player(p);
    let defender = attacker.pierce(&Defender::of_player(&t.factors));
    let (mine, theirs) = (super::effects::of_player(p), super::effects::of_player(t));
    let (outcome, damage) = strike(rng, &attacker, &defender);
    if outcome == result::SUCCESS {
        if let Some(r) = player_guard(rng, &theirs, t.factors.defense_rate[9], &mine) {
            return (r, Vec::new());
        }
    }
    let (outcome, damage) = super::effects::critical(rng, &mine, theirs.crit_resist - mine.ignore_crit_resist.max(0.0), outcome, damage);
    let mut parts = vec![(RESULT_PHYSICAL, damage)];
    if damage > 0 {
        let chance = spirit_chance(true, attacker.level, defender.level);
        parts.extend(spirit_hits(rng, chance, &spirit_ranges(&p.factors), &defender.resist, true));
        // Part héroïque (heroic.rs), selon la classe de l'assaillant.
        super::heroic::push(&mut parts, super::heroic::hit(rng, &attacker, &defender));
    }
    (outcome, parts)
}

/// Champ 3 du paquet d'action : facteurs complets avec le résultat (PV, PM, dégâts affichés en négatif)
/// et les PV restants.
pub fn damage_factors(hp: i32, mp: i32, damage: i32) -> Vec<u8> {
    parts_factors(hp, mp, &[(RESULT_PHYSICAL, damage)])
}

/// Même chose avec des dégâts par nature : (indice dans le résultat, dégâts).
pub fn parts_factors(hp: i32, mp: i32, parts: &[(usize, i32)]) -> Vec<u8> {
    let pt = &mini::FACTOR_POINT;
    let mut shown = Fields::new(pt).with(0, hp).with(1, mp);
    for (i, d) in parts {
        shown.set(*i, -*d);
    }
    let cf = &mini::CHAR_FACTORS;
    let result = build_mini(cf, &Fields::new(cf).with(3, build_mini(pt, &shown)));
    let current = build_mini(pt, &Fields::new(pt).with(0, hp).with(1, mp));
    build_mini(cf, &Fields::new(cf).with(0, result).with(3, current))
}

/// Indice des dégâts physiques dans le résultat (facteurs « points ») ; les éléments suivent, de 10 (magie)
/// à 17 (glace), dans l'ordre des attributs 1 à 8.
pub const RESULT_PHYSICAL: usize = 9;

/// Fourchettes de dégâts élémentaires (attributs 1 à 8) de facteurs.
pub fn spirit_ranges(f: &crate::character::Factors) -> [(f64, f64); 8] {
    std::array::from_fn(|i| (f.damage_min[i + 1] as f64, f.damage_max[i + 1].max(f.damage_min[i + 1]) as f64))
}

/// Chance qu'un élément porte, en pour cent : 70 pour un joueur ; pour un monstre, 80 plus l'écart de
/// niveau avec sa cible, 60 au moins.
pub fn spirit_chance(attacker_is_player: bool, attacker_level: i32, target_level: i32) -> u32 {
    if attacker_is_player { 70 } else { (80 + attacker_level - target_level).max(60) as u32 }
}

/// Part élémentaire d'un coup qui touche : pour chaque élément dont l'attaquant a des dégâts, avec la
/// chance `chance`, un jet entre le minimum et le maximum, doublé, moins la résistance de la cible en
/// pour cent (sa défense de l'élément, 80 au plus pour un joueur, 100 pour un monstre). Rend
/// (indice dans le résultat, dégâts) pour les éléments qui portent.
pub fn spirit_hits(rng: &mut impl Rng, chance: u32, ranges: &[(f64, f64); 8], resist: &[i32; 8], target_is_player: bool) -> Vec<(usize, i32)> {
    let cap = if target_is_player { 80 } else { 100 };
    let mut out = Vec::new();
    for (i, &(lo, hi)) in ranges.iter().enumerate() {
        if hi <= 0.0 || rng.gen_range(0..100) >= chance {
            continue;
        }
        let raw = if lo < hi { rng.gen_range(lo..=hi) } else { hi };
        let res = resist[i].clamp(0, cap) as f64;
        let d = (2.0 * raw * (100.0 - res) / 100.0) as i32;
        if d > 0 {
            out.push((RESULT_PHYSICAL + 1 + i, d));
        }
    }
    out
}

/// Résistances élémentaires (attributs 1 à 8) de facteurs.
pub fn spirit_resist(f: &crate::character::Factors) -> [i32; 8] {
    std::array::from_fn(|i| f.defense_point[i + 1])
}

/// Paquet AGPMOPTIMIZEDCHARACTION d'un coup porté.
pub fn action_packet(attacker: u32, target: u32, outcome: i64, factors: Option<Vec<u8>>) -> Vec<u8> {
    let s = &schema::OPT_CHAR_ACTION;
    let f = Fields::new(s).with(0, attacker).with(1, target).with(2, outcome).with(5, 1).with(6, 1);
    build_packet(s, &match factors {
        Some(b) => f.with(3, b),
        None => f,
    })
}

/// Ajoute de l'expérience ; l'expérience se compte par niveau (`needed[n]` pour passer le niveau n).
/// Rend (niveau, expérience, niveaux gagnés).
pub fn gain_exp(level: i32, exp: u64, gain: u64, needed: &[u64]) -> (i32, u64, i32) {
    let (mut level, mut exp, mut ups) = (level, exp + gain, 0);
    while level < MAX_LEVEL {
        match needed.get(level as usize).copied() {
            Some(n) if n > 0 && exp >= n => {
                exp -= n;
                level += 1;
                ups += 1;
            }
            _ => break,
        }
    }
    (level, exp, ups)
}

pub fn dist2d(a: Vec3, b: Vec3) -> f64 {
    let (dx, dz) = ((a.x - b.x) as f64, (a.z - b.z) as f64);
    (dx * dx + dz * dz).sqrt()
}

/// AGPMOPTIMIZEDCHARACTION reçu : le joueur prend un monstre, ou un joueur qu'il peut attaquer, pour cible.
pub fn on_action(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let target = p.int(1).unwrap_or(0) as u32;
    let forced = p.int(6).unwrap_or(0) != 0;
    // Forme d'animal (Type 8, morph.rs) : n'attaque personne.
    if super::morph::harmless(st, sid) {
        debug!("[combat] #{sid} : pas d'attaque sous une forme d'animal");
        return;
    }
    let alive = st.monsters.get(target).is_some_and(|m| m.is_alive() && st.monsters.attackable(m));
    // Objet de siège : seulement pendant le combat, selon le camp et l'ordre d'attaque (siege/war.rs).
    if alive && !super::siege::war::may_hit(ctx, st, sid, target) {
        debug!("[combat] #{sid} : objet de siège {target:#x} pas attaquable maintenant");
        return;
    }
    // Invocation d'un autre joueur : règles du combat entre joueurs envers son maître (summons.rs).
    if alive {
        if let Err(r) = super::summons::may_attack(ctx, st, sid, target, forced) {
            super::pvp::refuse(st, sid, r);
            return;
        }
    }
    // Joueur : combat de guildes ou règles du combat entre joueurs (pvp.rs).
    let foe = !alive
        && match st.by_cid.get(&target).copied() {
            Some(tsid) => match super::pvp::check(ctx, st, sid, tsid, forced) {
                Ok(()) => true,
                Err(r) => {
                    super::pvp::refuse(st, sid, r);
                    false
                }
            },
            None => false,
        };
    let Some(player) = st.players.get_mut(&sid) else { return };
    if !player.in_game || player.dead {
        return;
    }
    if !alive && !foe {
        debug!("[combat] {} vise {target:#x}, qui n'est pas un monstre vivant et attaquable", player.ch.name);
        return;
    }
    if player.attack.map(|a| a.target) != Some(target) {
        debug!("[combat] {} attaque {target:#x}", player.ch.name);
        // Anti-triche : une nouvelle cible (ou une reprise après un déplacement) ne donne pas de coup avant
        // l'intervalle de la vitesse d'attaque, tolérance comprise, depuis le coup précédent (anticheat.rs).
        let now = Instant::now();
        let ac = &ctx.config.world.anticheat;
        let (ready_at, early) = if ac.enabled { player.hits.start(now, ac.attack_tolerance / 100.0) } else { (now, false) };
        player.attack = Some(Attack { target, ready_at, forced });
        if early && player.hits.early == super::anticheat::EARLY_ATTACKS {
            let details = format!(
                "{} demandes d'attaque avant le coup suivant (vitesse d'attaque {}, coup retardé de {} ms)",
                player.hits.early,
                player.factors.attack[2],
                ready_at.saturating_duration_since(now).as_millis()
            );
            super::anticheat::report(ctx, st, sid, super::anticheat::kind::CADENCE, &details);
        }
    }
}

/// Boucle de combat : coups des joueurs, cadavres, réapparitions.
pub fn spawn_ticker(world: Arc<super::World>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        loop {
            tick.tick().await;
            let mut st = world.state.lock().await;
            if let Err(e) = step(&world.ctx, &mut st, Instant::now()).await {
                warn!("[combat] {e:#}");
            }
        }
    });
}

pub async fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> anyhow::Result<()> {
    for sid in st.sids() {
        attack_step(ctx, st, sid, now).await?;
    }
    super::presence::move_step(ctx, st, now);
    ai::walk_step(&ctx.map, st, now);
    for (sid, cid) in ai::step(ctx, st, now) {
        // Tué par une contre-attaque (une seule fois, même si plusieurs joueurs ont contre-attaqué).
        if st.monsters.get(cid).is_some_and(|m| m.is_alive() && m.hp == 0) {
            kill(ctx, st, sid, cid, now).await?;
        }
    }
    // Invocations : fin, suivi du maître, combat (expérience et butin à leur maître).
    super::summons::step(ctx, st, now).await?;
    // Suiveurs, invocations et renforts des monstres ; zones au sol ; objets à durée.
    super::ai_kin::step(ctx, st, now);
    super::skill_extra::fields_step(ctx, st, now).await?;
    super::skill_extra::pending_step(ctx, st, now).await?;
    super::lifetime::step(ctx, st, now).await?;
    ai::patrol_step(ctx, st, now);
    ai::preemptive_step(st, now);
    super::pvp::step(ctx, st, now);
    life_step(ctx, st, now);
    super::loot::expire(st, now);
    super::recovery::regen_step(st, now);
    super::skills::step(ctx, st, now).await?;
    super::portal::step(st);
    // Zone d'effet et bonus des groupes (op 2).
    super::party::step(ctx, st, now);
    super::guild::step(ctx, st, now).await?;
    // Combats de Shrine, compteur de dégâts des boss, horaire du Battleground.
    super::shrine::step(ctx, st, now).await?;
    super::boss::step(ctx, st, now);
    super::battlesquare::step(ctx, st, now);
    // Guerres de siège : calendrier, combat, carving.
    super::siege::step(ctx, st, now).await?;
    // Guerre de Rensphere et effets de l'ArchLord ; versements des taxes.
    super::archlord::step(ctx, st, now).await?;
    super::tax::step(ctx, st, now).await?;
    // Usure des coups de ce tour, descentes de monture forcées.
    super::repair::step(ctx, st).await?;
    super::ride::step(ctx, st).await?;
    // Métamorphoses échues ou de joueurs morts.
    super::morph::step(ctx, st, now);
    Ok(())
}

/// Joueurs qui voient le monstre `cid`.
pub fn viewers(st: &WorldState, cid: u32) -> Vec<u32> {
    st.players.iter().filter(|(_, p)| p.in_game && p.seen_monsters.contains(&cid)).map(|(sid, _)| *sid).collect()
}

pub(super) fn broadcast(st: &WorldState, sids: &[u32], plain: &[u8]) {
    for sid in sids {
        if let Some(p) = st.players.get(sid) {
            p.send_raw(plain.to_vec());
        }
    }
}

async fn attack_step(ctx: &WorldCtx, st: &mut WorldState, sid: u32, now: Instant) -> anyhow::Result<()> {
    // Métamorphose (morph.rs) : sous une forme d'animal, plus de coups ; un monstre changé en animal par une Mutation
    // n'est plus attaquable.
    let Some(target) = st.players.get(&sid).and_then(|p| p.attack).map(|a| a.target) else { return Ok(()) };
    if super::morph::harmless(st, sid) || st.monsters.get(target).is_some_and(|m| m.is_alive() && !st.monsters.attackable(m)) {
        st.players.get_mut(&sid).unwrap().attack = None;
        return Ok(());
    }
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let Some(attack) = p.attack else { return Ok(()) };
    if !p.in_game || p.dead || now < attack.ready_at {
        return Ok(());
    }
    // Attaques interdites, peur, désarmement (status_rules.rs).
    if super::effects::active_status(&p.skills.effects, now) & (super::effects::status::NO_ATTACK | super::effects::status::DISARMAMENT) != 0 {
        return Ok(());
    }
    // Munition de l'arme (arc, arbalète : ammo.rs), consommée à chaque coup porté.
    let ammo = super::ammo::needed(ctx, p).map(|(tid, _)| tid);
    let Some(m) = st.monsters.get(attack.target).filter(|m| m.is_alive()).cloned() else {
        if let Some(tsid) = st.by_cid.get(&attack.target).copied() {
            // Joueur invisible sans détection (skill_extra.rs) : plus de cible.
            if super::skill_extra::hidden_from(ctx, st, sid, tsid) {
                st.players.get_mut(&sid).unwrap().attack = None;
                return Ok(());
            }
            if super::ammo::refuse_attack(ctx, st, sid, attack.target) {
                return Ok(());
            }
            super::pvp::attack_step(ctx, st, sid, attack.target, attack.forced, now).await?;
            // Un coup est parti à cet instant : une munition de moins.
            if let Some(tid) = ammo.filter(|_| st.players.get(&sid).is_some_and(|p| p.hits.last.is_some_and(|(at, _)| at == now))) {
                super::ammo::consume(ctx, st, sid, tid, 1).await?;
            }
            return Ok(());
        }
        st.players.get_mut(&sid).unwrap().attack = None;
        return Ok(());
    };
    let stats = st.monsters.stats_for(&m);
    // Distance depuis la position que le serveur suit (marche en cours comprise) : on ne frappe qu'à portée.
    let reach = p.factors.attack[0].max(50) as f64 + stats.hit_range as f64;
    if dist2d(p.ch.pos(), m.pos) > reach + REACH_TOLERANCE {
        if p.dest.is_none() || now >= attack.ready_at {
            approach(st, sid, &m, stats.hit_range as f64, now);
        }
        return Ok(());
    }

    if super::ammo::refuse_attack(ctx, st, sid, m.cid) {
        return Ok(());
    }
    let p = &st.players[&sid];
    let attacker = Attacker::of_player(p);
    let defender = attacker.pierce(&Defender::of_monster(m.level as i32, &stats, &st.monsters.mods(m.cid)));
    let target_status = st.monsters.status(m.cid, now);
    let (outcome, parts) = {
        let mut rng = rand::thread_rng();
        let (outcome, damage) = strike(&mut rng, &attacker, &defender);
        let mine = super::effects::of_player(p);
        let crit_resist = st.monsters.mods(m.cid).crit_resist - mine.ignore_crit_resist.max(0.0);
        let (outcome, damage) = super::effects::critical(&mut rng, &mine, crit_resist, outcome, damage);
        let mut parts = vec![(RESULT_PHYSICAL, damage)];
        if damage > 0 {
            let chance = spirit_chance(true, attacker.level, m.level as i32);
            parts.extend(spirit_hits(&mut rng, chance, &spirit_ranges(&p.factors), &defender.resist, false));
            super::heroic::push(&mut parts, super::heroic::hit(&mut rng, &attacker, &defender));
        }
        // Invincibilité élémentaire de la cible (status_rules.rs).
        super::status_rules::shield(target_status, &mut parts);
        // Death Strike (skill_extra.rs) : le monstre ordinaire touché tombe d'un coup, résultat 13.
        if damage > 0 && super::skill_extra::death_strike(ctx, st, p, &m, &mut rng) {
            (super::skill_extra::RESULT_DEATH_STRIKE, vec![(RESULT_PHYSICAL, m.hp.max(1))])
        } else {
            (outcome, parts)
        }
    };
    let damage: i32 = parts.iter().map(|(_, d)| d).sum();
    let speed = p.factors.attack[2].max(1) as u64;
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let me = st.players.get_mut(&sid).unwrap();
    me.attack = Some(Attack { target: m.cid, ready_at: now + Duration::from_millis(60_000 / speed), forced: false });
    me.hits.hit(now, speed as i32);
    // Qui frappe s'arrête, comme le client au début de l'animation du coup.
    me.dest = None;
    super::recovery::in_combat(&mut me.timers, now);

    let target = st.monsters.get_mut(m.cid).unwrap();
    target.hp = (target.hp - damage).max(0);
    let hp = target.hp;
    if damage > 0 {
        st.monsters.wake(m.cid);
    }
    let factors = (damage > 0).then(|| parts_factors(hp, stats.max_mp, &parts));
    let watchers = viewers(st, m.cid);
    broadcast(st, &watchers, &action_packet(cid, m.cid, outcome, factors));
    if damage > 0 {
        ai::add_aggro(st, m.cid, cid, damage as u64, now);
        super::repair::hit_dealt(st, sid);
    }
    debug!("[combat] {name} → {} : résultat {outcome}, {damage} dégât(s), {hp}/{} PV", m.name, m.max_hp);
    if let Some(tid) = ammo {
        super::ammo::consume(ctx, st, sid, tid, 1).await?;
    }
    if damage > 0 {
        // Effets d'un seul coup (Lens Stone) consommés.
        super::skill_extra::spend_one_hit(ctx, st, sid, m.cid);
        // PV absorbés (Life Extinction, skill_extra.rs).
        let drain = st.players.get(&sid).map(effects_absorb).unwrap_or((0.0, 0.0));
        super::skill_extra::absorb(ctx, st, sid, m.cid, damage, drain);
    }
    if hp == 0 {
        kill(ctx, st, sid, m.cid, now).await?;
    } else if damage > 0 {
        super::skills::gear_procs(ctx, st, sid, m.cid, now).await?;
    }
    Ok(())
}

/// Fait courir le joueur, depuis sa position actuelle, jusqu'au bord de la cible (OPT_CHAR_MOVE à lui et aux
/// joueurs qui le voient) ; le serveur suit cette marche. Rend la durée du trajet (revue au plus tard après
/// `MAX_APPROACH`).
pub(super) fn approach(st: &mut WorldState, sid: u32, m: &Monster, hit_range: f64, now: Instant) -> Duration {
    let p = st.players.get_mut(&sid).unwrap();
    let from = p.ch.pos();
    let distance = dist2d(from, m.pos).max(1.0);
    let k = ((distance - hit_range / 2.0) / distance).max(0.0) as f32;
    let dest = Vec3::new(from.x + (m.pos.x - from.x) * k, m.pos.y, from.z + (m.pos.z - from.z) * k);
    let speed = p.factors.status[9].max(1) as f64 / 10.0;
    let travel = Duration::from_secs_f64(distance / speed).min(MAX_APPROACH);
    p.dest = Some(dest);
    p.move_flags = MOVE_FAST;
    p.moved_at = now;
    p.dirty = true;
    if let Some(a) = p.attack.as_mut() {
        a.ready_at = now + travel;
    }
    let cid = p.ch.id;
    let mv = &schema::OPT_CHAR_MOVE;
    let fields = Fields::new(mv).with(0, cid).with(1, from).with(2, dest).with(5, MOVE_FAST).with(6, 0);
    p.send(mv, fields.clone());
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
        o.send(mv, fields.clone());
    }
    debug!("[combat] approche de {} sur {:.0} unités", m.name, distance);
    travel
}

/// PV absorbés par les coups normaux du joueur : chance et part des dégâts de ses effets.
fn effects_absorb(p: &super::Player) -> (f64, f64) {
    let m = super::effects::total(&p.skills.effects);
    (m.absorb_rate, m.absorb)
}

/// Fait courir le joueur vers une position visée au sol, jusqu'à portée `range` (même paquet que `approach`). Rend la durée
/// du trajet.
pub(super) fn approach_point(st: &mut WorldState, sid: u32, at: Vec3, range: f64, now: Instant) -> Duration {
    let p = st.players.get_mut(&sid).unwrap();
    let from = p.ch.pos();
    let distance = dist2d(from, at).max(1.0);
    let k = ((distance - range * 0.9) / distance).max(0.0) as f32;
    let dest = Vec3::new(from.x + (at.x - from.x) * k, at.y, from.z + (at.z - from.z) * k);
    let speed = p.factors.status[9].max(1) as f64 / 10.0;
    let travel = Duration::from_secs_f64(distance * k as f64 / speed).min(MAX_APPROACH);
    p.dest = Some(dest);
    p.move_flags = MOVE_FAST;
    p.moved_at = now;
    p.dirty = true;
    let cid = p.ch.id;
    let mv = &schema::OPT_CHAR_MOVE;
    let fields = Fields::new(mv).with(0, cid).with(1, from).with(2, dest).with(5, MOVE_FAST).with(6, 0);
    p.send(mv, fields.clone());
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
        o.send(mv, fields.clone());
    }
    travel
}

/// Mort d'un monstre : statut « mort » chez ceux qui le voient, expérience au tueur, cadavre programmé.
pub(super) async fn kill(ctx: &WorldCtx, st: &mut WorldState, killer_sid: u32, cid: u32, now: Instant) -> anyhow::Result<()> {
    // Invocation : elle disparaît, sans expérience ni butin (summons.rs).
    if st.monsters.owner_of(cid).is_some() {
        super::summons::died(st, cid);
        return Ok(());
    }
    // Chef : ses invocations, suiveurs et renfort disparaissent ; créature appelée : retirée de sa bande (ai_kin.rs).
    super::ai_kin::on_death(st, cid);
    st.extra.provoked.remove(&cid);
    // Dégâts de chaque agresseur, pour la répartition de l'expérience d'un groupe ; ceux d'une invocation comptent pour
    // son maître.
    let damage = super::summons::damage_by_owner(st, cid);
    let target = st.monsters.get_mut(cid).unwrap();
    let pos = target.pos;
    target.life = Life::Dead { remove_at: now + CORPSE_TIME };
    target.aggro.clear();
    target.target = None;
    st.monsters.engaged.remove(&cid);
    st.monsters.effects.remove(&cid);
    // Mutation : la forme se perd sans op 12, l'immunité aussi (morph.rs).
    st.monsters.morphs.remove(&cid);
    st.monsters.morph_immune.remove(&cid);
    // Compétences de l'IA : relances et lancements comptés repartent de zéro à la réapparition.
    st.monsters.skill_state.remove(&cid);
    st.monsters.casts.remove(&cid);
    let target = st.monsters.get_mut(cid).unwrap();
    let (tid, name) = (target.tid, target.name.clone());
    st.monsters.pending.insert(cid);
    // Boss de la fenêtre des boss d'Archonia : classement du combat, liste renvoyée.
    super::boss::on_kill(ctx, st, tid, cid, &damage);
    let c = &schema::CHARACTER;
    let dead = build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(11, ACTION_STATUS_DEAD));
    let watchers = viewers(st, cid);
    broadcast(st, &watchers, &dead);
    // Objet de siège détruit : variantes, étapes du siège ; ni expérience ni butin (siege/war.rs).
    if super::siege::war::on_kill(ctx, st, killer_sid, cid, now) {
        if let Some(p) = st.players.get_mut(&killer_sid) {
            p.attack = None;
        }
        return Ok(());
    }
    let base = st.monsters.stats_of(tid).exp.max(0) as f64;
    let gain = (base * st.rates.get("exp")).round().max(0.0) as u64;
    let Some(p) = st.players.get_mut(&killer_sid) else { return Ok(()) };
    p.attack = None;
    info!("[combat] {} tue {name} ({cid:#x}) : +{gain} d'expérience", p.ch.name);
    for (sid, part) in super::party::share_exp(st, killer_sid, gain, pos, &damage) {
        // Meurtrier de palier 3 : expérience des monstres réduite de moitié.
        let rogue = st.players.get(&sid).is_some_and(|x| super::pvp::murderer_level(x.ch.murderer) >= 3);
        // Bonus d'expérience des effets (talismans de Janus, costumes : consumables.rs).
        let part = super::consumables::with_exp_bonus(ctx, st, sid, if rogue { part / 2 } else { part });
        reward(ctx, st, sid, part).await?;
    }
    super::loot::on_kill(ctx, st, killer_sid, cid, now);
    super::quests::on_kill(ctx, st, killer_sid, tid).await?;
    // Objectifs de titre de collecte, si les morts comptent (`Title.CollectCountsKills`).
    super::titles::on_kill(ctx, st, killer_sid, tid).await?;
    Ok(())
}

/// Expérience gagnée : facteurs mis à jour, montée de niveau éventuelle, sauvegarde.
pub(super) async fn reward(ctx: &WorldCtx, st: &mut WorldState, sid: u32, gain: u64) -> anyhow::Result<()> {
    if gain == 0 {
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    let (level, exp, ups) = gain_exp(p.ch.level, p.ch.exp, gain, &ctx.level_exp);
    p.ch.level = level;
    p.ch.exp = exp;
    p.factors.status[7] = level;
    p.factors.point[3] = (exp % (1u64 << 32)) as u32 as i32;
    p.factors.point[4] = (exp >> 32) as i32;
    if ups > 0 {
        // Montée de niveau : facteurs du nouveau niveau (croissance, équipement, effets), PV et PM restaurés,
        // nouveaux points de compétence.
        super::equipment::recompute_factors(ctx, p);
        p.factors.point[POINT_HP] = p.factors.point_max[0];
        p.factors.point[1] = p.factors.point_max[1];
        super::skills::refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    }
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, cid).with(7, factors_mini(&p.factors)));
    if ups > 0 {
        let up = build_packet(c, &Fields::new(c).with(0, CHARACTER_LEVEL_UP).with(1, cid));
        p.send_raw(up.clone());
        for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
            o.send_raw(up.clone());
        }
        info!("[combat] {name} passe au niveau {level}");
    }
    send_system(st, sid, &format!("Vous gagnez {gain} point(s) d'expérience."));
    ctx.db.characters.exec("UPDATE characters SET level = ?, exp = ? WHERE id = ?", (level, exp, cid)).await?;
    // Objectifs de titre de niveau (Advanced).
    if ups > 0 {
        super::titles::refresh(ctx, st, sid).await?;
    }
    Ok(())
}

/// Niveau fixé par un MJ : expérience remise à zéro, facteurs du niveau (croissance, équipement, effets),
/// PV et PM restaurés, points de compétence recalculés.
pub async fn set_level(ctx: &WorldCtx, st: &mut WorldState, sid: u32, level: i32) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let level = level.clamp(1, MAX_LEVEL);
    p.ch.level = level;
    p.ch.exp = 0;
    p.factors.status[7] = level;
    p.factors.point[3] = 0;
    p.factors.point[4] = 0;
    super::equipment::recompute_factors(ctx, p);
    p.factors.point[POINT_HP] = p.factors.point_max[0];
    p.factors.point[1] = p.factors.point_max[1];
    super::skills::refresh_points(&ctx.skills, &p.skills, &mut p.factors);
    let cid = p.ch.id;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, cid).with(7, factors_mini(&p.factors)));
    send_around(st, sid, &build_packet(c, &Fields::new(c).with(0, CHARACTER_LEVEL_UP).with(1, cid)));
    ctx.db.characters.exec("UPDATE characters SET level = ?, exp = 0 WHERE id = ?", (level, cid)).await?;
    super::titles::refresh(ctx, st, sid).await?;
    Ok(())
}

/// Cadavres retirés, monstres réapparus.
fn life_step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let due: Vec<u32> = st
        .monsters
        .pending
        .iter()
        .copied()
        .filter(|cid| match st.monsters.get(*cid).map(|m| m.life) {
            Some(Life::Dead { remove_at }) => remove_at <= now,
            Some(Life::Gone { respawn_at }) => respawn_at <= now,
            _ => true,
        })
        .collect();
    for cid in due {
        // Créature appelée par un chef (ai_kin.rs) : retirée au départ de son cadavre, sans réapparition.
        if matches!(st.monsters.get(cid).map(|m| m.life), Some(Life::Dead { .. })) && super::ai_kin::remove_corpse(st, cid) {
            st.harvests.remove(&cid);
            continue;
        }
        let Some(m) = st.monsters.get_mut(cid) else {
            st.monsters.pending.remove(&cid);
            continue;
        };
        match m.life {
            Life::Dead { .. } => {
                m.life = Life::Gone { respawn_at: now + Duration::from_secs(respawn_delay(m.respawn_sec, m.respawn_max_sec, &mut rand::thread_rng()) as u64) };
                let tid = m.tid;
                let gone = character_remove_packet(cid);
                for p in st.players.values_mut() {
                    if p.seen_monsters.remove(&cid) {
                        p.send_raw(gone.clone());
                    }
                }
                // Cadavre parti : sa récolte (product.rs) est oubliée.
                st.harvests.remove(&cid);
                // Boss suivi : prochaine apparition connue.
                super::boss::on_life_change(ctx, st, tid);
            }
            Life::Gone { .. } => {
                m.life = Life::Alive;
                m.hp = m.max_hp;
                m.aggro.clear();
                m.target = None;
                let home = m.home;
                st.monsters.relocate(cid, home);
                st.monsters.walking.remove(&cid);
                st.monsters.pending.remove(&cid);
                respawn_view(ctx, st, cid);
                if let Some(tid) = st.monsters.get(cid).map(|m| m.tid) {
                    super::boss::on_life_change(ctx, st, tid);
                }
            }
            Life::Alive => {
                st.monsters.pending.remove(&cid);
            }
        }
    }
}

/// Partage des dégâts : chaque effet du joueur `sid` posé par un autre joueur (Divine Protection, Martyr's Mark)
/// fait prendre à ce lanceur, s'il est en jeu et vivant, sa part des dégâts physiques ou élémentaires du coup.
/// Le lanceur perd ces PV (AGPMCHARACTER op 1, facteurs du résultat, à lui et à ceux qui le voient) et peut en
/// mourir. Rend les dégâts qui restent au joueur.
pub fn divert(st: &mut WorldState, sid: u32, parts: &[(usize, i32)]) -> Vec<(usize, i32)> {
    let Some(p) = st.players.get(&sid) else { return parts.to_vec() };
    let shares: Vec<(u32, f64, f64)> = p
        .skills
        .effects
        .iter()
        .filter(|e| e.caster_sid != sid && e.caster_sid != super::effects::MONSTER_CASTER && (e.mods.share_physical > 0.0 || e.mods.share_spirit > 0.0))
        .map(|e| (e.caster_sid, e.mods.share_physical, e.mods.share_spirit))
        .collect();
    let mut left = parts.to_vec();
    for (caster, physical, spirit) in shares {
        if !st.players.get(&caster).is_some_and(|c| c.in_game && !c.dead) {
            continue;
        }
        let (kept, taken) = super::effects::share(&left, physical, spirit);
        if taken <= 0 {
            continue;
        }
        left = kept;
        let c = st.players.get_mut(&caster).unwrap();
        super::recovery::in_combat(&mut c.timers, Instant::now());
        let hp = (c.factors.point[POINT_HP] - taken).max(0);
        c.factors.point[POINT_HP] = hp;
        let (ccid, mp, name) = (c.ch.id, c.factors.point[1], c.ch.name.clone());
        let cs = &schema::CHARACTER;
        send_around(st, caster, &build_packet(cs, &Fields::new(cs).with(0, 1).with(1, ccid).with(7, parts_factors(hp, mp, &[(RESULT_PHYSICAL, taken)]))));
        debug!("[combat] {name} prend {taken} dégât(s) d'un membre de son groupe : {hp} PV");
        if hp == 0 {
            player_death(st, caster);
        }
    }
    left
}

/// Le joueur `sid` reçoit le coup d'un monstre : part des dégâts prise par un protecteur (`divert`), PV,
/// diffusion (à lui et à ceux qui le voient), mort.
pub fn hit_player(st: &mut WorldState, sid: u32, monster: u32, outcome: i64, parts: &[(usize, i32)]) {
    // Invincibilité élémentaire (status_rules.rs) : parts élémentaires retirées.
    let mut parts = parts.to_vec();
    super::status_rules::shield(st.players.get(&sid).map(|p| p.skills.status_sent).unwrap_or(0), &mut parts);
    let parts = &divert(st, sid, &parts);
    let damage: i32 = parts.iter().map(|(_, d)| d).sum();
    // Usure : l'armure de la victime, l'arme de l'attaquant quand c'est un joueur (combat entre joueurs, de guildes).
    if damage > 0 {
        super::repair::hit_taken(st, sid);
        if let Some(attacker) = st.by_cid.get(&monster).copied() {
            super::repair::hit_dealt(st, attacker);
        }
    }
    let Some(p) = st.players.get_mut(&sid) else { return };
    super::recovery::in_combat(&mut p.timers, Instant::now());
    let hp = (p.factors.point[POINT_HP] - damage).max(0);
    p.factors.point[POINT_HP] = hp;
    debug!("[combat] {} reçoit {damage} dégât(s) (résultat {outcome}) : {hp}/{} PV", p.ch.name, p.factors.point_max[0]);
    let (cid, mp) = (p.ch.id, p.factors.point[1]);
    let factors = (outcome == result::SUCCESS).then(|| parts_factors(hp, mp, parts));
    let plain = action_packet(monster, cid, outcome, factors);
    send_around(st, sid, &plain);
    if hp == 0 {
        player_death(st, sid);
    }
}

/// Envoie au joueur `sid` et aux joueurs qui le voient.
/// À un joueur et à ceux qui le voient.
pub(super) fn send_around(st: &WorldState, sid: u32, plain: &[u8]) {
    let Some(me) = st.players.get(&sid) else { return };
    let cid = me.ch.id;
    me.send_raw(plain.to_vec());
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
        o.send_raw(plain.to_vec());
    }
}

/// Le joueur `other` reçoit ce que `send_around` envoie autour de `sid` (lui-même ou un joueur qui le voit).
pub(super) fn send_around_contains(st: &WorldState, sid: u32, other: u32) -> bool {
    other == sid || st.players.get(&other).zip(st.players.get(&sid)).is_some_and(|(o, me)| o.seen_players.contains(&me.ch.id))
}

/// Paquet AGPMCHARACTER op 1 : facteurs et statut d'action d'un joueur.
fn status_packet(p: &Player, status: i64) -> Vec<u8> {
    let c = &schema::CHARACTER;
    build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)).with(11, status))
}

/// Mort d'un joueur : il cesse d'attaquer et de bouger ; les monstres le lâchent.
pub fn player_death(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    p.dead = true;
    p.attack = None;
    p.skills.cast = None;
    // Mort en combat entre joueurs : marquée ensuite par `pvp::on_kill` (perte d'expérience, consumables.rs).
    p.uses.pk_death = false;
    if let Some(d) = p.dest.take() {
        p.ch.set_pos(d);
    }
    p.factors.point[POINT_HP] = 0;
    let plain = status_packet(p, ACTION_STATUS_DEAD);
    info!("[combat] {} meurt", p.ch.name);
    send_around(st, sid, &plain);
    // Ses inimitiés tombent au prochain tour, après le bilan d'un meurtre (pvp.rs : op 4, op 6).
    if let Some(p) = st.players.get_mut(&sid) {
        p.pvp.reset_pending = true;
    }
    // Les effets des compétences tombent à la mort (pas les passives) ; facteurs recalculés à la résurrection.
    super::skills::clear_player_effects(st, sid);
    // Ses invocations et son familier disparaissent (pas les créatures fixes, qui vont au bout de leur durée).
    super::summons::on_owner_death(st, sid);
}

/// AGPMCHARACTER op 9 : résurrection au village de départ de la race, tous PV et PM, invincible 10 s ; perte
/// d'expérience selon la configuration (`consumables::town_penalty`, désactivée par défaut).
pub async fn resurrect(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    super::consumables::town_penalty(ctx, st, sid).await?;
    resurrect_at(ctx, st, sid, None).await
}

/// Résurrection en `at` (point de siège : siege/war.rs), sinon selon la zone, au point propre de la région
/// (ResurrectionX/Z de RegionTemplate.ini) ou au village.
pub async fn resurrect_at(ctx: &WorldCtx, st: &mut WorldState, sid: u32, at: Option<Vec3>) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.dead) {
        return Ok(());
    }
    // Battleground : fort de sa race ; Shrine en combat : zone de sa race (battlesquare.rs) ; point propre de la région où
    // il est mort (ResurrectionX/Z, regions.rs) ; sinon le village.
    let own = || {
        let p = &st.players[&sid];
        let (x, z) = ctx.regions.own_point(ctx.map.region(p.ch.pos_x, p.ch.pos_z))?;
        Some(Vec3::new(x as f32, ctx.map.height(x, z).unwrap_or(p.ch.pos_y as f32), z as f32))
    };
    let (x, y, z) = match at.or_else(|| super::battlesquare::resurrection_point(ctx, st, sid)).or_else(own) {
        Some(at) => (at.x as f64, Some(at.y as f64), at.z as f64),
        None => super::travel::home_village(ctx, &st.players[&sid]),
    };
    revive(ctx, st, sid);
    info!("[combat] {} ressuscite en {x:.0}, {z:.0}", st.players[&sid].ch.name);
    super::teleport(ctx, st, sid, x, y, z).await?;
    Ok(())
}

/// Un mort revient à la vie là où il est : facteurs recalculés, tous PV et PM, invincible 10 s, statut normal
/// (AGPMCHARACTER op 1 autour de lui). Sert au village, aux points de résurrection et à l'orbe (consumables.rs).
pub fn revive(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    if !st.players.get(&sid).is_some_and(|p| p.dead) {
        return;
    }
    super::equipment::refresh_factors(ctx, st, sid);
    let p = st.players.get_mut(&sid).unwrap();
    p.dead = false;
    p.invincible_until = Some(Instant::now() + RESURRECTION_GRACE);
    p.factors.point[POINT_HP] = p.factors.point_max[0];
    p.factors.point[1] = p.factors.point_max[1];
    let plain = status_packet(p, 0);
    send_around(st, sid, &plain);
    // Le droit de mettre son tueur à prix s'éteint.
    super::wanted::on_resurrect(st, sid);
}

/// Montre un monstre réapparu aux joueurs qui voient son secteur.
/// Délai de réapparition d'un monstre, en secondes : fixe, ou tiré dans la plage du point quand elle en a
/// une (boss de la fenêtre des boss d'Archonia) ; jamais moins de `MIN_RESPAWN_SEC`.
pub fn respawn_delay(min: i64, max: i64, rng: &mut impl Rng) -> i64 {
    let min = min.max(MIN_RESPAWN_SEC);
    if max > min { rng.gen_range(min..=max) } else { min }
}

fn respawn_view(_ctx: &WorldCtx, st: &mut WorldState, cid: u32) {
    let Some(m) = st.monsters.get(cid) else { return };
    let (pos, view) = (m.pos, MonsterRegistry::view_packet(&st.monsters, m));
    for p in st.players.values_mut().filter(|p| p.in_game) {
        if super::sector::sees(p.view_pos, pos) && p.seen_monsters.insert(cid) {
            p.send_raw(view.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::{decode_mini, decode_packet};
    use rand::SeedableRng;

    fn warrior() -> Attacker {
        Attacker { level: 1, ar: 19, class: 1, str_: 14.0, dex: 11.0, int: 7.0, ..Default::default() }
    }

    #[test]
    fn delai_de_reapparition() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        assert_eq!(respawn_delay(8, 0, &mut rng), 8, "délai fixe");
        assert_eq!(respawn_delay(0, 0, &mut rng), MIN_RESPAWN_SEC, "plancher");
        for _ in 0..100 {
            let d = respawn_delay(1800, 2700, &mut rng);
            assert!((1800..=2700).contains(&d), "30 à 45 minutes : {d}");
        }
    }

    #[test]
    fn chance_de_toucher() {
        let worm = Defender { level: 1, dr: 0, defense: 2.0, ..Default::default() };
        assert_eq!(hit_chance(&warrior(), &worm), 95, "plafond");
        assert_eq!(hit_chance(&warrior(), &Defender { level: 5, ..worm }), 62, "90 - 7 × 4");
        assert_eq!(hit_chance(&warrior(), &Defender { level: 30, ..worm }), 10, "plancher");
        assert_eq!(hit_chance(&Attacker { ar: 0, ..warrior() }, &worm), 10);
    }

    #[test]
    fn degats_par_classe() {
        let (lo, hi) = warrior().damage_range();
        assert!((lo - 14.0 / 3.0).abs() < 1e-9 && (hi - 4.9).abs() < 1e-9);
        let (lo, hi) = Attacker { class: class::MAGE, int: 16.0, ..warrior() }.damage_range();
        assert!((lo - 6.4).abs() < 1e-9 && (hi - 6.72).abs() < 1e-9);
        let (lo, _) = Attacker { class: class::ARCHER, dex: 15.0, str_: 12.0, ..warrior() }.damage_range();
        assert!((lo - (15.0 * 2.0 / 9.0 + 12.0 / 9.0)).abs() < 1e-9);
        // Défense supérieure aux dégâts : 1 PV quand même.
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let tough = Defender { level: 1, dr: 0, defense: 50.0, ..Default::default() };
        for _ in 0..50 {
            let (r, d) = strike(&mut rng, &warrior(), &tough);
            assert!(r == result::MISS && d == 0 || r == result::SUCCESS && d == 1);
        }
    }

    #[test]
    fn defense_ignoree() {
        let mut resist = [0; 8];
        resist[0] = 50;
        let boar = Defender { level: 5, dr: 0, defense: 20.0, resist, ..Default::default() };
        // Lethal Attack niveau 1 : 5 % de la défense ; Stunner niveau 1 : 10 % des résistances.
        let a = Attacker { ignore_defense: 5.0, ignore_resist: 10.0, ..warrior() };
        let seen = a.pierce(&boar);
        assert_eq!((seen.defense, seen.resist[0], seen.level), (19.0, 45, 5));
        assert_eq!(warrior().pierce(&boar), boar);
        assert_eq!(Attacker { ignore_defense: 300.0, ..warrior() }.pierce(&boar).defense, 0.0, "jamais en dessous de zéro");
        // Résistances d'un monstre (CharacterDataTable : « Magic Res. »…) et d'un joueur (80 % au plus).
        let stats = super::super::monsters::MonsterStats { ac: 7, resist: [50, 50, 50, 0, 0, 0, 0, 120], ..Default::default() };
        let d = Defender::of_monster(5, &stats, &super::super::effects::Mods::default());
        assert_eq!((d.defense, d.resist[0], d.resist[7]), (7.0, 50, 100));
        let mut f = crate::character::Factors::default();
        f.defense_point[2] = 95;
        assert_eq!(Defender::of_player(&f).resist[1], 80);
    }

    #[test]
    fn chance_de_toucher_d_un_monstre() {
        let worm = Attacker { level: 1, ar: 21, ..Default::default() };
        let player = Defender { level: 1, dr: 13, defense: 0.0, ..Default::default() };
        assert_eq!(monster_hit_chance(&worm, &player), 89, "1/2 × 21/34 × 290");
        assert_eq!(monster_hit_chance(&Attacker { ar: 0, ..worm }, &player), 30, "plancher");
        assert_eq!(monster_hit_chance(&Attacker { ar: 0, ..worm }, &Defender { level: 11, ..player }), 55, "85 - 3 × 10");
    }

    #[test]
    fn esquive_et_resistance_physique_des_monstres() {
        // Monstre : DR de CharacterDataTable (et les « evade_rate » de ses effets), résistance physique, blocage des
        // compétences.
        let stats = super::super::monsters::MonsterStats { ac: 10, dr: 237, phys_resist: 80, skill_block: 5, ..Default::default() };
        let d = Defender::of_monster(50, &stats, &super::super::effects::Mods { evade: -37.0, ..Default::default() });
        assert_eq!((d.dr, d.phys_resist, d.skill_block), (200, 80.0, 5.0));
        let a = Attacker { level: 50, ar: 300, ..warrior() };
        assert_eq!(hit_chance(&a, &d), 87, "1/2 × 300/500 × 290");
        assert_eq!(hit_chance(&a, &Defender { dr: 0, ..d }), 95, "sans esquive : plafond");
        // 100 après la défense, 80 % de résistance : 20 ; l'assaillant en ignore 30 points : 50.
        assert_eq!(physical_hit(100.0, &d), 20);
        let piercing = Attacker { ignore_phys_resist: 30.0, ignore_skill_block: 10.0, ..a };
        let seen = piercing.pierce(&d);
        assert_eq!((physical_hit(100.0, &seen), seen.skill_block), (50, 0.0));
        assert_eq!(physical_hit(0.4, &d), 1, "au moins 1");
        // Joueur : facteurs de défense 0 (résistance, 80 % au plus) et 10 (blocage des compétences).
        let mut f = crate::character::Factors::default();
        f.defense_rate[0] = 95;
        f.defense_rate[10] = 12;
        let p = Defender::of_player(&f);
        assert_eq!((p.phys_resist, p.skill_block), (80.0, 12.0));
    }

    #[test]
    fn garde_contre_un_joueur() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(21);
        let none = super::super::effects::Mods::default();
        assert_eq!(player_guard(&mut rng, &none, 0, &none), None);
        // 100 % d'esquive, plafonnés à 80 ; l'assaillant en ignore 30 : 50 %.
        let agile = super::super::effects::Mods { dodge: 100.0, ..Default::default() };
        let n = (0..10_000).filter(|_| player_guard(&mut rng, &agile, 0, &none) == Some(result::EVADE)).count();
        assert!((7600..8400).contains(&n), "{n}");
        let sharp = super::super::effects::Mods { ignore_dodge: 50.0, ..Default::default() };
        let n = (0..10_000).filter(|_| player_guard(&mut rng, &agile, 0, &sharp) == Some(result::EVADE)).count();
        assert!((4700..5300).contains(&n), "{n}");
        // Blocage des options (facteur 9) : 20 %, moins 20 points ignorés : jamais.
        let n = (0..10_000).filter(|_| player_guard(&mut rng, &none, 20, &none) == Some(result::BLOCK)).count();
        assert!((1700..2300).contains(&n), "{n}");
        let breaker = super::super::effects::Mods { ignore_block: 20.0, ..Default::default() };
        assert!((0..1000).all(|_| player_guard(&mut rng, &none, 20, &breaker).is_none()));
    }

    #[test]
    fn experience_par_niveau() {
        let needed = [0, 8, 26, 62];
        assert_eq!(gain_exp(1, 0, 5, &needed), (1, 5, 0));
        assert_eq!(gain_exp(1, 5, 5, &needed), (2, 2, 1));
        assert_eq!(gain_exp(1, 0, 8 + 26 + 10, &needed), (3, 10, 2));
        assert_eq!(gain_exp(3, 0, 1000, &needed), (4, 938, 1), "table épuisée : on s'arrête");
    }

    #[test]
    fn paquet_de_coup() {
        let plain = action_packet(1, 0x5000_0007, result::SUCCESS, Some(damage_factors(130, 35, 6)));
        let p = decode_packet(&schema::OPT_CHAR_ACTION, &plain).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(5), p.int(6)), (Some(1), Some(0x5000_0007), Some(5), Some(1), Some(1)));
        let f = decode_mini(&mini::CHAR_FACTORS, p.bytes(3).unwrap()).unwrap();
        let now = decode_mini(&mini::FACTOR_POINT, f.bytes(3).unwrap()).unwrap();
        assert_eq!((now.int(0), now.int(1), now.int(9)), (Some(130), Some(35), None));
        let res = decode_mini(&mini::CHAR_FACTORS, f.bytes(0).unwrap()).unwrap();
        let shown = decode_mini(&mini::FACTOR_POINT, res.bytes(3).unwrap()).unwrap();
        assert_eq!((shown.int(0), shown.int(9)), (Some(130), Some(-6)));
        // Coup manqué : pas de facteurs.
        let miss = decode_packet(&schema::OPT_CHAR_ACTION, &action_packet(1, 2, result::MISS, None)).unwrap();
        assert!(miss.bytes(3).is_none());
    }

    #[test]
    fn degats_elementaires() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        // 10 d'eau fixes : doublés, moins 25 % de résistance ; le feu sans dégâts ne compte pas.
        let mut ranges = [(0.0, 0.0); 8];
        ranges[1] = (10.0, 10.0);
        let mut resist = [0; 8];
        resist[1] = 25;
        assert_eq!(spirit_hits(&mut rng, 100, &ranges, &resist, true), vec![(11, 15)]);
        // Résistance plafonnée à 80 % pour un joueur, 100 % pour un monstre.
        resist[1] = 95;
        assert_eq!(spirit_hits(&mut rng, 100, &ranges, &resist, true), vec![(11, 4)]);
        assert_eq!(spirit_hits(&mut rng, 100, &ranges, &resist, false), vec![(11, 1)]);
        resist[1] = 100;
        assert!(spirit_hits(&mut rng, 100, &ranges, &resist, false).is_empty());
        assert!(spirit_hits(&mut rng, 0, &ranges, &[0; 8], false).is_empty());
        assert_eq!((spirit_chance(true, 1, 50), spirit_chance(false, 30, 20), spirit_chance(false, 10, 40)), (70, 90, 60));
    }
}
