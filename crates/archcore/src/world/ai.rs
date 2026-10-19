//! IA des monstres : riposte et poursuite.
//!
//! Un monstre ne s'en prend qu'à ceux qui l'ont frappé. Il retient chaque agresseur avec les dégâts reçus et
//! l'oublie 30 s après son dernier coup. Toutes les 500 ms, il vise celui qui lui a fait le plus de dégâts
//! parmi les joueurs vivants, en jeu et à moins de 5000 unités ; il le rejoint, puis le frappe au rythme de
//! sa vitesse d'attaque. Sans agresseur, il rentre à son point d'apparition ; s'il s'en est éloigné de plus
//! de 10 000 unités, il y est ramené d'un coup avec tous ses PV.

use super::combat::{self, Attacker, Defender};
use super::monsters::{Aggro, Monster};
use super::{WorldCtx, WorldState};
use crate::debug;
use archcore_net::schema;
use archcore_net::{Fields, Vec3};
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
/// Vitesse d'un monstre sans vitesse connue (unités par seconde) ; déplacement le plus long compté.
const DEFAULT_SPEED: f64 = 300.0;
const MAX_TRAVEL: Duration = Duration::from_secs(4);

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
                p.in_game && !p.dead && combat::dist2d(p.dest.unwrap_or_else(|| p.ch.pos()), m.pos) <= CHASE_LIMIT
            })
        })
        .max_by_key(|a| a.damage)
        .map(|a| a.cid)
}

/// Temps de trajet d'un monstre sur `distance` unités, d'après sa vitesse de course.
fn travel(distance: f64, run: i32) -> Duration {
    let speed = if run > 0 { run as f64 / 10.0 } else { DEFAULT_SPEED };
    Duration::from_secs_f64(distance / speed).min(MAX_TRAVEL)
}

/// Fait marcher le monstre jusqu'à `dest` (chez les joueurs qui le voient) ; rend le temps de trajet.
fn walk(st: &mut WorldState, cid: u32, dest: Vec3, run: i32) -> Duration {
    let Some(from) = st.monsters.get(cid).map(|m| m.pos) else { return Duration::ZERO };
    let mv = &schema::OPT_CHAR_MOVE;
    let fields = Fields::new(mv).with(0, cid).with(1, from).with(2, dest).with(5, 0).with(6, 0);
    for sid in combat::viewers(st, cid) {
        st.players[&sid].send(mv, fields.clone());
    }
    st.monsters.relocate(cid, dest);
    travel(combat::dist2d(from, dest), run)
}

/// Un pas d'IA pour tous les monstres engagés.
pub fn step(_ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    let engaged: Vec<u32> = st.monsters.engaged.iter().copied().collect();
    for cid in engaged {
        monster_step(st, cid, now);
    }
}

fn disengage(st: &mut WorldState, cid: u32) {
    if let Some(m) = st.monsters.get_mut(cid) {
        m.aggro.clear();
        m.target = None;
    }
    st.monsters.engaged.remove(&cid);
}

fn monster_step(st: &mut WorldState, cid: u32, now: Instant) {
    let Some(m) = st.monsters.get(cid).cloned() else {
        st.monsters.engaged.remove(&cid);
        return;
    };
    if !m.is_alive() {
        return disengage(st, cid);
    }
    let stats = st.monsters.stats_of(m.tid);
    // Trop loin de chez lui : ramené d'un coup, soigné.
    if combat::dist2d(m.pos, m.home) > LEASH {
        disengage(st, cid);
        walk(st, cid, m.home, stats.run);
        if let Some(m) = st.monsters.get_mut(cid) {
            m.hp = m.max_hp;
        }
        debug!("[ia] {} ({cid:#x}) trop loin de son point : ramené", m.name);
        return;
    }
    if now < m.ready_at {
        return;
    }
    let aggro: Vec<Aggro> = m.aggro.iter().copied().filter(|a| a.until > now).collect();
    if aggro.is_empty() {
        // Plus personne à poursuivre : il rentre.
        disengage(st, cid);
        if combat::dist2d(m.pos, m.home) > 1.0 {
            let t = walk(st, cid, m.home, stats.run);
            st.monsters.get_mut(cid).unwrap().ready_at = now + t;
        }
        return;
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
    let Some(tcid) = target else { return };
    let sid = st.by_cid[&tcid];
    let p = &st.players[&sid];
    let ppos = p.dest.unwrap_or_else(|| p.ch.pos());
    let reach = stats.range.max(50) as f64 + PLAYER_HIT_RANGE;
    let distance = combat::dist2d(m.pos, ppos);
    if distance > reach + REACH_TOLERANCE {
        // Poursuite : jusqu'au bord de la portée.
        let k = ((distance - reach * 0.8) / distance).max(0.0) as f32;
        let dest = Vec3::new(m.pos.x + (ppos.x - m.pos.x) * k, ppos.y, m.pos.z + (ppos.z - m.pos.z) * k);
        let t = walk(st, cid, dest, stats.run);
        st.monsters.get_mut(cid).unwrap().ready_at = now + t;
        return;
    }
    if p.invincible_until.is_some_and(|t| now < t) {
        st.monsters.get_mut(cid).unwrap().ready_at = now + SCAN_INTERVAL;
        return;
    }
    let attacker = Attacker {
        level: m.level as i32,
        ar: stats.ar,
        min_damage: stats.min_damage as f64,
        max_damage: stats.max_damage as f64,
        ..Default::default()
    };
    let defender = Defender { level: p.factors.status[7], dr: p.factors.point_max[9], defense: p.factors.defense_point[0] as f64 };
    let (outcome, damage) = combat::strike_at(&mut rand::thread_rng(), combat::monster_hit_chance(&attacker, &defender), &attacker, &defender);
    let speed = stats.attack_speed.max(1) as u64;
    st.monsters.get_mut(cid).unwrap().ready_at = now + Duration::from_millis(60_000 / speed);
    combat::hit_player(st, sid, cid, outcome, damage);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duree_de_trajet() {
        assert_eq!(travel(385.0, 3850), Duration::from_secs(1));
        assert_eq!(travel(300.0, 0), Duration::from_secs(1), "vitesse inconnue");
        assert_eq!(travel(1e9, 3850), MAX_TRAVEL);
    }
}
