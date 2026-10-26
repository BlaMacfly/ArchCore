//! IA des monstres : riposte et poursuite.
//!
//! Un monstre ne s'en prend qu'à ceux qui l'ont frappé. Il retient chaque agresseur avec les dégâts reçus et
//! l'oublie 30 s après son dernier coup. Toutes les 500 ms, il vise celui qui lui a fait le plus de dégâts
//! parmi les joueurs vivants, en jeu et à moins de 5000 unités ; il le rejoint, puis le frappe au rythme de
//! sa vitesse d'attaque. Sans agresseur, il rentre à son point d'apparition ; s'il s'en est éloigné de plus
//! de 10 000 unités, il y est ramené d'un coup avec tous ses PV.
//!
//! Marche : un monstre avance pas à pas vers sa destination à chaque tour, comme les joueurs ; il entre dans la
//! vue d'un joueur, ou en sort, quand il change vraiment de secteur. Un monstre posé sur le sol (point
//! d'apparition sans hauteur) suit la hauteur du terrain du client.
//!
//! États spéciaux : étourdi, gelé ou endormi, un monstre ne bouge ni ne frappe ; immobilisé, il ne poursuit
//! pas ; sous « attaque interdite » ou la peur, il ne frappe pas. Il ignore les joueurs invincibles ou
//! invisibles. Le coup d'un monstre peut être esquivé, bloqué ou changé en PV ou PM selon les effets du
//! joueur (`effects::guard`), et réveille un joueur endormi. Un joueur qui contre-attaque (Counter Attack) lui
//! renvoie, avec sa chance, les dégâts du coup reçu (résultat 12). Le monstre lance aussi les compétences de son
//! IA (`monster_skills.rs`) à la place d'un coup normal ou de la poursuite.
//!
//! Patrouille : un monstre (type « monstre ») au repos, vu par au moins un joueur et dont le point a un rayon,
//! marche toutes les 15 à 45 s vers un endroit tiré au hasard dans ce rayon, hors sol infranchissable ; au plus
//! 16 départs par tour.

use super::combat::{self, Attacker, Defender};
use super::effects::{self, status, Guard};
use super::map::WorldMap;
use super::monster_skills;
use super::monsters::{Aggro, Monster, Walk};
use super::{WorldCtx, WorldState};
use crate::debug;
use archcore_net::schema;
use archcore_net::{Fields, Vec3};
use rand::Rng;
use std::time::{Duration, Instant};

/// Un agresseur est oublié ce délai après son dernier coup.
pub const AGGRO_TIME: Duration = Duration::from_secs(30);
/// Fréquence du choix de la cible.
pub const SCAN_INTERVAL: Duration = Duration::from_millis(500);
/// Distance au-delà de laquelle un agresseur n'est plus poursuivi.
pub const CHASE_LIMIT: f64 = 5000.0;
/// Distance au point d'apparition au-delà de laquelle le monstre y est ramené.
pub const LEASH: f64 = 10_000.0;
/// Rayon du corps d'un joueur, ajouté à la portée du monstre.
pub const PLAYER_HIT_RANGE: f64 = 50.0;
/// Marge de portée, pour les positions arrondies.
const REACH_TOLERANCE: f64 = 50.0;
/// Vitesses d'un personnage sans vitesse connue, en dixièmes d'unité par seconde, comme le client Archonia
/// (4000 à la marche, 12000 à la course) ; déplacement le plus long compté.
const DEFAULT_WALK: i32 = 4_000;
const DEFAULT_RUN: i32 = 12_000;
const MAX_TRAVEL: Duration = Duration::from_secs(4);
/// Patrouille : intervalle tiré entre ces bornes, départs au plus par tour, essais pour trouver un endroit libre.
const PATROL_MIN: Duration = Duration::from_secs(15);
const PATROL_MAX: Duration = Duration::from_secs(45);
const MAX_PATROL_PER_TICK: usize = 16;
const PATROL_TRIES: usize = 8;
/// Bit « monstre » du type de personnage.
const TYPE_MONSTER: i64 = 0x4;

/// Le monstre `cid` vient d'être frappé par le joueur `attacker` (CID) pour `damage` PV.
pub fn add_aggro(st: &mut WorldState, cid: u32, attacker: u32, damage: u64, now: Instant) {
    let Some(m) = st.monsters.get_mut(cid) else { return };
    if !m.is_alive() {
        return;
    }
    match m.aggro.iter_mut().find(|a| a.cid == attacker) {
        Some(a) => {
            a.damage += damage;
            a.until = now + AGGRO_TIME;
        }
        None => m.aggro.push(Aggro { cid: attacker, damage, until: now + AGGRO_TIME }),
    }
    st.monsters.engaged.insert(cid);
}

/// Agresseur à viser : le plus de dégâts parmi les joueurs vivants, en jeu et assez proches.
pub fn choose_target(st: &WorldState, m: &Monster) -> Option<u32> {
    m.aggro
        .iter()
        .filter(|a| {
            st.by_cid.get(&a.cid).and_then(|sid| st.players.get(sid)).is_some_and(|p| {
                p.in_game
                    && !p.dead
                    && p.skills.status_sent & status::UNTOUCHABLE == 0
                    && combat::dist2d(p.ch.pos(), m.pos) <= CHASE_LIMIT
            })
        })
        .max_by_key(|a| a.damage)
        .map(|a| a.cid)
}

/// Vitesse en dixièmes d'unité par seconde : celle du monstre, sinon celle d'Archonia par défaut.
fn tenths(speed: i32, fast: bool) -> i32 {
    if speed > 0 { speed } else if fast { DEFAULT_RUN } else { DEFAULT_WALK }
}

/// Temps de trajet d'un monstre sur `distance` unités : `10000 / vitesse × distance` ms, comme le client.
fn travel(distance: f64, speed: i32, fast: bool) -> Duration {
    Duration::from_secs_f64(distance / (tenths(speed, fast) as f64 / 10.0)).min(MAX_TRAVEL)
}

/// Déplace le monstre jusqu'à `dest` (chez les joueurs qui le voient), en courant ou en marchant ; rend le
/// temps de trajet, d'après la vitesse donnée.
fn walk(map: &WorldMap, st: &mut WorldState, cid: u32, dest: Vec3, run: i32) -> Duration {
    go(map, st, cid, dest, run, true)
}

/// Hauteur du sol sous `p` pour un monstre posé sur le sol, sinon `p` tel quel.
fn on_ground(map: &WorldMap, st: &WorldState, cid: u32, mut p: Vec3) -> Vec3 {
    if st.monsters.grounded.contains(&cid) {
        if let Some(y) = map.height(p.x as f64, p.z as f64) {
            p.y = y;
        }
    }
    p
}

fn go(map: &WorldMap, st: &mut WorldState, cid: u32, dest: Vec3, speed: i32, fast: bool) -> Duration {
    let Some(from) = st.monsters.get(cid).map(|m| m.pos) else { return Duration::ZERO };
    let dest = on_ground(map, st, cid, dest);
    let mv = &schema::OPT_CHAR_MOVE;
    let flags = if fast { combat::MOVE_FAST } else { 0 };
    let fields = Fields::new(mv).with(0, cid).with(1, from).with(2, dest).with(5, flags).with(6, 0);
    for sid in combat::viewers(st, cid) {
        st.players[&sid].send(mv, fields.clone());
    }
    st.monsters.walking.insert(cid, Walk { dest, speed: tenths(speed, fast) as f64, at: Instant::now(), fast });
    travel(combat::dist2d(from, dest), speed, fast)
}

/// Fait avancer les monstres en marche. Quand l'un change de secteur, il apparaît chez les joueurs qui voient
/// le nouveau (avec sa marche) et disparaît chez ceux qui ne le voient plus.
pub fn walk_step(map: &WorldMap, st: &mut WorldState, now: Instant) {
    let moving: Vec<(u32, Walk)> = st.monsters.walking.iter().map(|(c, w)| (*c, *w)).collect();
    for (cid, w) in moving {
        let Some(from) = st.monsters.get(cid).filter(|m| m.is_alive()).map(|m| m.pos) else {
            st.monsters.walking.remove(&cid);
            continue;
        };
        let (next, arrived) = super::presence::step_toward(from, w.dest, w.speed, now.saturating_duration_since(w.at).as_secs_f64());
        let next = on_ground(map, st, cid, next);
        st.monsters.relocate(cid, next);
        if arrived {
            st.monsters.walking.remove(&cid);
        } else if let Some(x) = st.monsters.walking.get_mut(&cid) {
            x.at = now;
        }
        if super::sector::of(from) == super::sector::of(next) {
            continue;
        }
        let Some(m) = st.monsters.get(cid) else { continue };
        let view = super::monsters::MonsterRegistry::view_packet(&st.monsters, m);
        let mv = &schema::OPT_CHAR_MOVE;
        let fields = Fields::new(mv).with(0, cid).with(1, next).with(2, w.dest).with(5, if arrived { 0x04 } else if w.fast { combat::MOVE_FAST } else { 0 }).with(6, 0);
        for p in st.players.values_mut().filter(|p| p.in_game) {
            if super::sector::sees(p.view_pos, next) && p.seen_monsters.insert(cid) {
                p.send_raw(view.clone());
                if !arrived {
                    p.send(mv, fields.clone());
                }
            } else if !super::sector::keeps(p.view_pos, next) && p.seen_monsters.remove(&cid) {
                p.send_raw(super::character_remove_packet(cid));
            }
        }
    }
}

/// Promenades des monstres au repos que voit au moins un joueur.
pub fn patrol_step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let mut watched: Vec<u32> = st.players.values().filter(|p| p.in_game).flat_map(|p| p.seen_monsters.iter().copied()).collect();
    watched.sort_unstable();
    watched.dedup();
    let mut rng = rand::thread_rng();
    let mut started = 0;
    for cid in watched {
        if started >= MAX_PATROL_PER_TICK {
            break;
        }
        let Some(m) = st.monsters.get(cid) else { continue };
        if !m.is_alive() || m.target.is_some() || !m.aggro.is_empty() || m.roam <= 0.0 || now < m.patrol_at || now < m.ready_at {
            continue;
        }
        if st.monsters.status(cid, now) & status::NO_MOVE != 0 {
            continue;
        }
        let stats = st.monsters.stats_of(m.tid);
        let next = now + Duration::from_millis(rng.gen_range(PATROL_MIN.as_millis() as u64..=PATROL_MAX.as_millis() as u64));
        let (home, roam) = (m.home, m.roam);
        st.monsters.get_mut(cid).unwrap().patrol_at = next;
        if stats.char_type & TYPE_MONSTER == 0 {
            continue;
        }
        let spot = (0..PATROL_TRIES).map(|_| (rng.gen_range(-roam..roam), rng.gen_range(-roam..roam))).find(|(dx, dz)| !ctx.map.is_blocked(home.x as f64 + dx, home.z as f64 + dz));
        let Some((dx, dz)) = spot else { continue };
        let dest = Vec3::new(home.x + dx as f32, home.y, home.z + dz as f32);
        let t = go(&ctx.map, st, cid, dest, stats.walk, false);
        st.monsters.get_mut(cid).unwrap().ready_at = now + t;
        started += 1;
    }
}

/// Un pas d'IA pour tous les monstres engagés. Rend les monstres tués par une contre-attaque : (session du
/// joueur, monstre), à achever par `combat::kill`.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) -> Vec<(u32, u32)> {
    monster_skills::step(ctx, st, now);
    let engaged: Vec<u32> = st.monsters.engaged.iter().copied().collect();
    let mut killed = Vec::new();
    for cid in engaged {
        if let Some(k) = monster_step(ctx, st, cid, now) {
            killed.push(k);
        }
    }
    killed
}

fn disengage(st: &mut WorldState, cid: u32) {
    if let Some(m) = st.monsters.get_mut(cid) {
        m.aggro.clear();
        m.target = None;
    }
    st.monsters.engaged.remove(&cid);
}

fn monster_step(ctx: &WorldCtx, st: &mut WorldState, cid: u32, now: Instant) -> Option<(u32, u32)> {
    let map = &ctx.map;
    let Some(m) = st.monsters.get(cid).cloned() else {
        st.monsters.engaged.remove(&cid);
        return None;
    };
    if !m.is_alive() {
        disengage(st, cid);
        return None;
    }
    let stats = st.monsters.stats_of(m.tid);
    // Trop loin de chez lui : ramené d'un coup, soigné.
    if combat::dist2d(m.pos, m.home) > LEASH {
        disengage(st, cid);
        walk(map, st, cid, m.home, stats.run);
        if let Some(m) = st.monsters.get_mut(cid) {
            m.hp = m.max_hp;
        }
        debug!("[ia] {} ({cid:#x}) trop loin de son point : ramené", m.name);
        return None;
    }
    if now < m.ready_at {
        return None;
    }
    let state = st.monsters.status(cid, now);
    if state & status::HELPLESS != 0 {
        return None;
    }
    let aggro: Vec<Aggro> = m.aggro.iter().copied().filter(|a| a.until > now).collect();
    if aggro.is_empty() {
        // Plus personne à poursuivre : il rentre.
        disengage(st, cid);
        if combat::dist2d(m.pos, m.home) > 1.0 {
            let t = walk(map, st, cid, m.home, stats.run);
            st.monsters.get_mut(cid).unwrap().ready_at = now + t;
        }
        return None;
    }
    let mut target = m.target;
    let valid = |t: u32| st.by_cid.get(&t).and_then(|sid| st.players.get(sid)).is_some_and(|p| p.in_game && !p.dead);
    if now >= m.scan_at || !target.is_some_and(valid) {
        let mut probe = m.clone();
        probe.aggro = aggro.clone();
        target = choose_target(st, &probe);
    }
    {
        let mm = st.monsters.get_mut(cid).unwrap();
        mm.aggro = aggro;
        mm.target = target;
        if now >= mm.scan_at {
            mm.scan_at = now + SCAN_INTERVAL;
        }
    }
    let Some(tcid) = target else { return None };
    let sid = st.by_cid[&tcid];
    let p = &st.players[&sid];
    let shielded = p.invincible_until.is_some_and(|t| now < t);
    // Position actuelle de la cible (suivie par le serveur), pas sa destination : le monstre vise le joueur, pas
    // l'endroit où il va.
    let ppos = p.ch.pos();
    let reach = stats.range.max(50) as f64 + PLAYER_HIT_RANGE;
    let distance = combat::dist2d(m.pos, ppos);
    if distance > reach + REACH_TOLERANCE {
        // Une compétence qui porte déjà jusqu'à la cible part à la place de la poursuite.
        if let Some(slot) = (!shielded).then(|| monster_skills::ready_skill(ctx, st, &m, distance, now)).flatten() {
            monster_skills::begin(ctx, st, cid, slot, tcid, ppos, now);
            return None;
        }
        if state & status::NO_MOVE != 0 {
            return None;
        }
        // Poursuite : jusqu'au bord de la portée.
        let k = ((distance - reach * 0.8) / distance).max(0.0) as f32;
        let dest = Vec3::new(m.pos.x + (ppos.x - m.pos.x) * k, ppos.y, m.pos.z + (ppos.z - m.pos.z) * k);
        let run = super::effects::speed(stats.run, st.monsters.mods(cid).movement_pct);
        let t = walk(map, st, cid, dest, run);
        debug!("[ia] {} ({cid:#x}) poursuit : {distance:.0} unités, vers {:.0}, {:.0} en {:.1} s", m.name, dest.x, dest.z, t.as_secs_f64());
        st.monsters.get_mut(cid).unwrap().ready_at = now + t;
        return None;
    }
    if let Some(slot) = (!shielded).then(|| monster_skills::ready_skill(ctx, st, &m, distance, now)).flatten() {
        monster_skills::begin(ctx, st, cid, slot, tcid, ppos, now);
        return None;
    }
    let normal = st.monsters.ai_of(cid).is_none_or(|a| a.normal_attack);
    if shielded || state & status::NO_ATTACK != 0 || !normal {
        st.monsters.get_mut(cid).unwrap().ready_at = now + SCAN_INTERVAL;
        return None;
    }
    let mods = st.monsters.mods(cid);
    let (min_damage, max_damage) = super::effects::monster_damage(stats.min_damage, stats.max_damage, &mods);
    let attacker = Attacker { level: m.level as i32, ar: stats.ar, min_damage, max_damage, ..Default::default() };
    let defender = Defender::of_player(&p.factors);
    let mut rng = rand::thread_rng();
    let (mut outcome, physical) = combat::strike_at(&mut rng, combat::monster_hit_chance(&attacker, &defender), &attacker, &defender);
    let mut parts = vec![(combat::RESULT_PHYSICAL, physical)];
    if outcome == combat::result::SUCCESS {
        let spirit = stats.spirit.map(|(lo, hi)| (lo as f64, hi.max(lo) as f64));
        let chance = combat::spirit_chance(false, m.level as i32, p.factors.status[7]);
        parts.extend(combat::spirit_hits(&mut rng, chance, &spirit, &combat::spirit_resist(&p.factors), true));
    }
    let mut damage: i32 = parts.iter().map(|(_, d)| d).sum();
    let speed = super::effects::speed(stats.attack_speed, mods.atk_speed_pct).max(1) as u64;
    st.monsters.get_mut(cid).unwrap().ready_at = now + Duration::from_millis(60_000 / speed);
    // Qui frappe s'arrête (le client arrête aussi le monstre au début de son coup).
    st.monsters.walking.remove(&cid);
    let mut countered = 0;
    if outcome == combat::result::SUCCESS {
        let p = st.players.get_mut(&sid).unwrap();
        let mods = effects::total(&p.skills.effects);
        let guard = effects::guard(&mut rng, &mods, p.skills.status_sent, damage);
        if matches!(guard, Guard::Hit | Guard::Block) && effects::counter(&mut rng, &mods) {
            countered = damage;
        }
        match guard {
            Guard::Hit => {
                effects::lift_status(&mut p.skills.effects, status::SLEEP);
            }
            Guard::Evade => outcome = combat::result::EVADE,
            Guard::Block => outcome = combat::result::BLOCK,
            Guard::ToHp(n) | Guard::ToMp(n) => {
                let (i, max) = if matches!(guard, Guard::ToHp(_)) { (crate::character::POINT_HP, 0) } else { (1, 1) };
                p.factors.point[i] = (p.factors.point[i] + n.max(0)).min(p.factors.point_max[max]);
                outcome = combat::result::MISS;
                let c = &archcore_net::schema::CHARACTER;
                let (pcid, f) = (p.ch.id, crate::character::factors_mini(&p.factors));
                p.send(c, Fields::new(c).with(0, 1).with(1, pcid).with(7, f));
            }
        }
        if guard != Guard::Hit {
            damage = 0;
            parts.clear();
            debug!("[ia] {} ({cid:#x}) : coup {guard:?}", m.name);
        }
    }
    debug!("[ia] {} ({cid:#x}) frappe : résultat {outcome}, {damage} dégât(s)", m.name);
    combat::hit_player(st, sid, cid, outcome, &parts);
    if outcome == combat::result::SUCCESS && damage > 0 {
        monster_skills::on_hit(ctx, st, cid, sid, now);
    }
    if countered > 0 {
        return counter(st, sid, cid, countered, now);
    }
    None
}

/// Contre-attaque : les dégâts du coup reçu reviennent au monstre, annoncés comme un coup du joueur
/// (AGPMOPTIMIZEDCHARACTION, résultat 12) ; rend (session, monstre) si le monstre en meurt.
fn counter(st: &mut WorldState, sid: u32, cid: u32, damage: i32, now: Instant) -> Option<(u32, u32)> {
    let pcid = st.players.get(&sid).filter(|p| p.in_game && !p.dead)?.ch.id;
    let m = st.monsters.get_mut(cid).filter(|m| m.is_alive())?;
    m.hp = (m.hp - damage).max(0);
    let (hp, tid, name) = (m.hp, m.tid, m.name.clone());
    st.monsters.wake(cid);
    let max_mp = st.monsters.stats_of(tid).max_mp;
    let watchers = combat::viewers(st, cid);
    combat::broadcast(st, &watchers, &combat::action_packet(pcid, cid, combat::result::COUNTER_ATTACK, Some(combat::damage_factors(hp, max_mp, damage))));
    add_aggro(st, cid, pcid, damage as u64, now);
    debug!("[ia] contre-attaque sur {name} ({cid:#x}) : {damage} dégât(s) renvoyé(s), {hp} PV");
    (hp == 0).then_some((sid, cid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duree_de_trajet() {
        assert_eq!(travel(385.0, 3850, true), Duration::from_secs(1));
        assert_eq!(travel(1200.0, 0, true), Duration::from_secs(1), "course inconnue : 12000");
        assert_eq!(travel(400.0, 0, false), Duration::from_secs(1), "marche inconnue : 4000");
        assert_eq!(travel(1e9, 3850, true), MAX_TRAVEL);
    }
}
