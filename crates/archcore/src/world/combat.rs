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
}

impl Defender {
    /// Un joueur : niveau, esquive (DR), défense physique, résistances (80 % au plus).
    pub fn of_player(f: &crate::character::Factors) -> Self {
        Defender { level: f.status[7], dr: f.point_max[9], defense: f.defense_point[0] as f64, resist: spirit_resist(f).map(|r| r.clamp(0, 80)) }
    }

    /// Un monstre : défense physique sous ses effets, résistances de CharacterDataTable (100 % au plus).
    pub fn of_monster(level: i32, s: &super::monsters::MonsterStats, m: &super::effects::Mods) -> Self {
        Defender { level, dr: 0, defense: super::effects::monster_defense(s.ac, m), resist: s.resist.map(|r| r.clamp(0, 100)) }
    }
}

impl Attacker {
    /// Cible vue par cet attaquant : défense physique et résistances diminuées de la part qu'il ignore.
    pub fn pierce(&self, d: &Defender) -> Defender {
        let keep = |pct: f64| (100.0 - pct.clamp(0.0, 100.0)) / 100.0;
        Defender {
            defense: d.defense * keep(self.ignore_defense),
            resist: d.resist.map(|r| (r as f64 * keep(self.ignore_resist)).round() as i32),
            ..*d
        }
    }

    pub fn of_player(p: &Player) -> Self {
        let f = &p.factors;
        let m = super::effects::of_player(p);
        Attacker {
            ignore_defense: m.ignore_defense,
            ignore_resist: m.ignore_resist,
            level: f.status[7],
            ar: f.point_max[8],
            class: f.ftype[2],
            str_: f.status[1] as f64 / 100.0,
            dex: f.status[3] as f64 / 100.0,
            int: f.status[2] as f64 / 100.0,
            min_damage: f.damage_min[0] as f64,
            max_damage: f.damage_max[0] as f64,
        }
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
    (result::SUCCESS, ((raw - d.defense).round() as i32).max(1))
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
    let alive = st.monsters.get(target).is_some_and(|m| m.is_alive() && st.monsters.stats_of(m.tid).is_attackable());
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
        player.attack = Some(Attack { target, ready_at: Instant::now(), forced });
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
    ai::patrol_step(ctx, st, now);
    ai::preemptive_step(st, now);
    super::pvp::step(ctx, st, now);
    life_step(ctx, st, now);
    super::loot::expire(st, now);
    super::recovery::regen_step(st, now);
    super::skills::step(ctx, st, now).await?;
    super::portal::step(st);
    super::guild::step(ctx, st, now).await?;
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
    let Some(p) = st.players.get(&sid) else { return Ok(()) };
    let Some(attack) = p.attack else { return Ok(()) };
    if !p.in_game || p.dead || now < attack.ready_at {
        return Ok(());
    }
    if super::effects::active_status(&p.skills.effects, now) & super::effects::status::NO_ATTACK != 0 {
        return Ok(());
    }
    let Some(m) = st.monsters.get(attack.target).filter(|m| m.is_alive()).cloned() else {
        if st.by_cid.contains_key(&attack.target) {
            return super::pvp::attack_step(ctx, st, sid, attack.target, attack.forced, now).await;
        }
        st.players.get_mut(&sid).unwrap().attack = None;
        return Ok(());
    };
    let stats = st.monsters.stats_of(m.tid);
    // Distance depuis la position que le serveur suit (marche en cours comprise) : on ne frappe qu'à portée.
    let reach = p.factors.attack[0].max(50) as f64 + stats.hit_range as f64;
    if dist2d(p.ch.pos(), m.pos) > reach + REACH_TOLERANCE {
        if p.dest.is_none() || now >= attack.ready_at {
            approach(st, sid, &m, stats.hit_range as f64, now);
        }
        return Ok(());
    }

    let attacker = Attacker::of_player(p);
    let defender = attacker.pierce(&Defender::of_monster(m.level as i32, &stats, &st.monsters.mods(m.cid)));
    let (outcome, parts) = {
        let mut rng = rand::thread_rng();
        let (outcome, damage) = strike(&mut rng, &attacker, &defender);
        let (outcome, damage) = super::effects::critical(&mut rng, &super::effects::of_player(p), outcome, damage);
        let mut parts = vec![(RESULT_PHYSICAL, damage)];
        if damage > 0 {
            let chance = spirit_chance(true, attacker.level, m.level as i32);
            parts.extend(spirit_hits(&mut rng, chance, &spirit_ranges(&p.factors), &defender.resist, false));
        }
        (outcome, parts)
    };
    let damage: i32 = parts.iter().map(|(_, d)| d).sum();
    let speed = p.factors.attack[2].max(1) as u64;
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let me = st.players.get_mut(&sid).unwrap();
    me.attack = Some(Attack { target: m.cid, ready_at: now + Duration::from_millis(60_000 / speed), forced: false });
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
    }
    debug!("[combat] {name} → {} : résultat {outcome}, {damage} dégât(s), {hp}/{} PV", m.name, m.max_hp);
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

/// Mort d'un monstre : statut « mort » chez ceux qui le voient, expérience au tueur, cadavre programmé.
pub(super) async fn kill(ctx: &WorldCtx, st: &mut WorldState, killer_sid: u32, cid: u32, now: Instant) -> anyhow::Result<()> {
    let target = st.monsters.get_mut(cid).unwrap();
    // Dégâts de chaque agresseur, pour la répartition de l'expérience d'un groupe.
    let damage: std::collections::HashMap<u32, u64> = target.aggro.iter().map(|a| (a.cid, a.damage)).collect();
    let pos = target.pos;
    target.life = Life::Dead { remove_at: now + CORPSE_TIME };
    target.aggro.clear();
    target.target = None;
    st.monsters.engaged.remove(&cid);
    st.monsters.effects.remove(&cid);
    // Compétences de l'IA : relances et lancements comptés repartent de zéro à la réapparition.
    st.monsters.skill_state.remove(&cid);
    st.monsters.casts.remove(&cid);
    let target = st.monsters.get_mut(cid).unwrap();
    let (tid, name) = (target.tid, target.name.clone());
    st.monsters.pending.insert(cid);
    let c = &schema::CHARACTER;
    let dead = build_packet(c, &Fields::new(c).with(0, 1).with(1, cid).with(11, ACTION_STATUS_DEAD));
    let watchers = viewers(st, cid);
    broadcast(st, &watchers, &dead);
    let base = st.monsters.stats_of(tid).exp.max(0) as f64;
    let gain = (base * st.rates.get("exp")).round().max(0.0) as u64;
    let Some(p) = st.players.get_mut(&killer_sid) else { return Ok(()) };
    p.attack = None;
    info!("[combat] {} tue {name} ({cid:#x}) : +{gain} d'expérience", p.ch.name);
    for (sid, part) in super::party::share_exp(st, killer_sid, gain, pos, &damage) {
        // Meurtrier de palier 3 : expérience des monstres réduite de moitié.
        let rogue = st.players.get(&sid).is_some_and(|x| super::pvp::murderer_level(x.ch.murderer) >= 3);
        reward(ctx, st, sid, if rogue { part / 2 } else { part }).await?;
    }
    super::loot::on_kill(ctx, st, killer_sid, cid, now);
    super::quests::on_kill(ctx, st, killer_sid, tid).await?;
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
        let Some(m) = st.monsters.get_mut(cid) else {
            st.monsters.pending.remove(&cid);
            continue;
        };
        match m.life {
            Life::Dead { .. } => {
                m.life = Life::Gone { respawn_at: now + Duration::from_secs(respawn_delay(m.respawn_sec, m.respawn_max_sec, &mut rand::thread_rng()) as u64) };
                let gone = character_remove_packet(cid);
                for p in st.players.values_mut() {
                    if p.seen_monsters.remove(&cid) {
                        p.send_raw(gone.clone());
                    }
                }
                // Cadavre parti : sa récolte (product.rs) est oubliée.
                st.harvests.remove(&cid);
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
    let parts = &divert(st, sid, parts);
    let damage: i32 = parts.iter().map(|(_, d)| d).sum();
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
    if let Some(d) = p.dest.take() {
        p.ch.set_pos(d);
    }
    p.factors.point[POINT_HP] = 0;
    let plain = status_packet(p, ACTION_STATUS_DEAD);
    info!("[combat] {} meurt", p.ch.name);
    send_around(st, sid, &plain);
    // Les effets des compétences tombent à la mort (pas les passives) ; facteurs recalculés à la résurrection.
    super::skills::clear_player_effects(st, sid);
}

/// AGPMCHARACTER op 9 : résurrection au village de départ de la race, tous PV et PM, invincible 10 s.
pub async fn resurrect(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.dead) {
        return Ok(());
    }
    super::equipment::refresh_factors(ctx, st, sid);
    let p = st.players.get_mut(&sid).unwrap();
    p.dead = false;
    p.invincible_until = Some(Instant::now() + RESURRECTION_GRACE);
    p.factors.point[POINT_HP] = p.factors.point_max[0];
    p.factors.point[1] = p.factors.point_max[1];
    let (x, y, z) = super::travel::home_village(ctx, p);
    let plain = status_packet(p, 0);
    info!("[combat] {} ressuscite au village", p.ch.name);
    send_around(st, sid, &plain);
    super::teleport(ctx, st, sid, x, y, z).await?;
    Ok(())
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
        let boar = Defender { level: 5, dr: 0, defense: 20.0, resist };
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
