//! IA des monstres : invocations d'autres monstres, suiveurs et renfort en danger (colonnes d'`aidatatable.txt` des
//! tables de l'épisode 8, rien dans Archonia : l'IA est au serveur).
//!
//! ```text
//!   Invocation (« SummonTID » liste « ; », « SummonMaxCount ») : quand le chef entre en combat, SummonMaxCount monstres
//!     tirés dans la liste apparaissent à côté de lui, avec ses agresseurs ; pas d'autre invocation pendant ce combat.
//!     Ils disparaissent à sa mort ou à son retour au point d'apparition (fin du combat).
//!   Suiveurs (« FollowerTID », « FollowerNUM », « Max_CumulativeFollower », « Min_UpkeepFollower ») : quand un joueur voit
//!     le chef, FollowerNUM suiveurs apparaissent autour de lui ; ils le suivent et partagent ses agresseurs (et lui les
//!     leurs). Quand il en reste moins de Min_UpkeepFollower, il en refait jusqu'à FollowerNUM, sans dépasser
//!     Max_CumulativeFollower créés pendant sa vie.
//!   Renfort en danger (« PinchMonster ») : sous `Rules.PinchHpPercent` (30 %) de ses PV, une fois par vie, le monstre
//!     indiqué apparaît à côté de lui avec ses agresseurs.
//!   Mort ou fin du chef : ses invocations, suiveurs et renfort disparaissent, sans expérience ni butin. Tués, ils donnent
//!     expérience et butin comme les autres, mais ne réapparaissent pas. À la réapparition du chef, tout recommence.
//! ```
//! Règles proposées (aucune trace de leur calcul) ; `Rules.MonsterKin` (1). « Visibility » (60, Archdémon) : non lu.

use super::monsters::{Life, Monster};
use super::{combat, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::Vec3;
use rand::Rng;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Les monstres appelés par un autre reçoivent des CID dans une plage à part (entre les monstres du monde et les
/// invocations des joueurs).
pub const KIN_CID_BASE: u32 = 0x5400_0000;
const KIN_CID_SPAN: u32 = 0x0400_0000;
/// Cadence des décisions.
const STEP: Duration = Duration::from_millis(500);
/// Distance des créatures au chef à l'apparition, marge avant qu'un suiveur ne le rejoigne.
const SPREAD: f64 = 250.0;
const FOLLOW_SLACK: f64 = 400.0;

/// Colonnes d'une IA qui font apparaître d'autres monstres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KinAi {
    pub summon_tids: Vec<u32>,
    pub summon_max: u32,
    pub follower_tids: Vec<u32>,
    pub follower_num: u32,
    pub max_cumulative: u32,
    pub min_upkeep: u32,
    pub pinch_tid: u32,
}

impl KinAi {
    /// Lit les colonnes d'une ligne de `monster_ai.data`.
    pub fn from_data(d: &serde_json::Value) -> Self {
        let text = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("").trim().to_string();
        let num = |k: &str| text(k).parse::<u32>().unwrap_or(0);
        let list = |k: &str| text(k).split(';').filter_map(|t| t.trim().parse::<u32>().ok()).filter(|t| *t > 0).collect::<Vec<_>>();
        KinAi {
            summon_tids: list("SummonTID"),
            summon_max: num("SummonMaxCount"),
            follower_tids: list("FollowerTID"),
            follower_num: num("FollowerNUM"),
            max_cumulative: num("Max_CumulativeFollower"),
            min_upkeep: num("Min_UpkeepFollower"),
            pinch_tid: list("PinchMonster").first().copied().unwrap_or(0),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.summon_tids.is_empty() && self.follower_tids.is_empty() && self.pinch_tid == 0
    }

    /// Suiveurs à faire apparaître : `alive` vivants, `created` déjà créés pendant la vie du chef. Rien tant qu'il en
    /// reste au moins `min_upkeep` (sauf la première fois) ; jamais plus que `max_cumulative` en tout (0 : sans limite).
    pub fn followers_due(&self, alive: u32, created: u32) -> u32 {
        if self.follower_tids.is_empty() || self.follower_num == 0 || created > 0 && alive >= self.min_upkeep.max(1) {
            return 0;
        }
        let want = self.follower_num.saturating_sub(alive);
        if self.max_cumulative > 0 { want.min(self.max_cumulative.saturating_sub(created)) } else { want }
    }
}

/// Créatures d'un chef.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Band {
    pub summoned: Vec<u32>,
    pub followers: Vec<u32>,
    pub pinch: Vec<u32>,
    /// Suiveurs créés pendant la vie du chef, invocation faite pendant ce combat, renfort fait pendant cette vie.
    pub created: u32,
    pub summoned_now: bool,
    pub pinch_done: bool,
}

impl Band {
    fn all(&self) -> impl Iterator<Item = u32> + '_ {
        self.summoned.iter().chain(&self.followers).chain(&self.pinch).copied()
    }
}

/// Chefs et créatures appelées.
#[derive(Debug, Default)]
pub struct Kin {
    pub bands: HashMap<u32, Band>,
    /// Créature → chef.
    pub leader_of: HashMap<u32, u32>,
    next_cid: u32,
    step_at: Option<Instant>,
}

impl Kin {
    /// Le monstre a été appelé par un autre (il ne réapparaît pas).
    pub fn is_kin(&self, cid: u32) -> bool {
        self.leader_of.contains_key(&cid)
    }
}

/// Fait apparaître le monstre `tid` près de `at` : registre, vue chez ceux qui voient sa place. Rend son CID.
fn spawn(ctx: &WorldCtx, st: &mut WorldState, tid: u32, at: Vec3, now: Instant) -> Option<u32> {
    let stats = st.monsters.stats.get(&tid).cloned()?;
    let cid = loop {
        st.kin.next_cid = (st.kin.next_cid + 1) % KIN_CID_SPAN;
        let c = KIN_CID_BASE + st.kin.next_cid;
        if st.monsters.get(c).is_none() {
            break c;
        }
    };
    let angle = rand::thread_rng().gen_range(0.0..std::f64::consts::TAU);
    let mut pos = Vec3::new(at.x + (SPREAD * angle.cos()) as f32, at.y, at.z + (SPREAD * angle.sin()) as f32);
    if ctx.map.is_blocked(pos.x as f64, pos.z as f64) {
        pos = at;
    }
    let grounded = match ctx.map.height(pos.x as f64, pos.z as f64) {
        Some(y) => {
            pos.y = y;
            true
        }
        None => false,
    };
    let name = st.monsters.names.get(&tid).cloned().unwrap_or_else(|| format!("Monstre {tid}"));
    st.monsters.insert(Monster {
        cid,
        spawn_id: 0,
        tid,
        level: stats.level.max(1) as i64,
        name,
        pos,
        degree_x: 0.0,
        degree_y: 0.0,
        hp: stats.max_hp.max(1),
        max_hp: stats.max_hp.max(1),
        respawn_sec: 0,
        respawn_max_sec: 0,
        life: Life::Alive,
        home: pos,
        zone: pos,
        aggro: Vec::new(),
        target: None,
        ready_at: now,
        scan_at: now,
        roam: 0.0,
        patrol_at: now,
        ai_tid: 0,
        owner: None,
    });
    if grounded {
        st.monsters.grounded.insert(cid);
    }
    let Some(m) = st.monsters.get(cid) else { return None };
    for p in st.players.values_mut().filter(|p| p.in_game) {
        if super::sector::sees(p.view_pos, pos) && p.seen_monsters.insert(cid) {
            let name = ctx.locale.resolve(crate::locale::kind::MONSTER_NAME, m.tid, &p.language, &m.name);
            p.send_raw(st.monsters.view_packet_named(m, &name));
        }
    }
    Some(cid)
}

/// Retire une créature appelée (chef mort ou rentré) : elle disparaît de la vue, sans expérience ni butin.
fn vanish(st: &mut WorldState, cid: u32) {
    st.kin.leader_of.remove(&cid);
    let gone = super::character_remove_packet(cid);
    for p in st.players.values_mut() {
        if p.seen_monsters.remove(&cid) {
            p.send_raw(gone.clone());
        }
    }
    st.monsters.remove(cid);
}

/// Donne à `cid` les agresseurs de `from` (1 point de haine chacun, s'il ne les a pas déjà).
fn share_aggro(st: &mut WorldState, from: u32, cid: u32, now: Instant) {
    let Some(list) = st.monsters.get(from).map(|m| m.aggro.iter().filter(|a| a.until > now).map(|a| a.cid).collect::<Vec<_>>()) else { return };
    for a in list {
        if !st.monsters.get(cid).is_some_and(|m| m.is_alive() && m.aggro.iter().any(|x| x.cid == a)) {
            super::ai::add_aggro(st, cid, a, 1, now);
        }
    }
}

/// Appelle `n` monstres tirés dans `tids` près du chef, avec ses agresseurs. Rend leurs CID.
fn call(ctx: &WorldCtx, st: &mut WorldState, leader: u32, tids: &[u32], n: u32, now: Instant) -> Vec<u32> {
    let Some(at) = st.monsters.get(leader).map(|m| m.pos) else { return Vec::new() };
    let mut out = Vec::new();
    for _ in 0..n {
        if tids.is_empty() {
            break;
        }
        let tid = tids[rand::thread_rng().gen_range(0..tids.len())];
        if let Some(cid) = spawn(ctx, st, tid, at, now) {
            st.kin.leader_of.insert(cid, leader);
            share_aggro(st, leader, cid, now);
            out.push(cid);
        }
    }
    out
}

/// Un pas : invocations à l'entrée en combat, suiveurs, renfort, suivi du chef, agresseurs partagés.
pub fn step(ctx: &WorldCtx, st: &mut WorldState, now: Instant) {
    if st.kin.step_at.is_some_and(|t| now < t) || !crate::config::rules().monster_kin {
        return;
    }
    st.kin.step_at = Some(now + STEP);
    // Chefs à suivre : ceux qui ont déjà une bande, et ceux que voit un joueur.
    let mut leaders: Vec<u32> = st.kin.bands.keys().copied().collect();
    leaders.extend(st.players.values().filter(|p| p.in_game).flat_map(|p| p.seen_monsters.iter().copied()));
    leaders.sort_unstable();
    leaders.dedup();
    for leader in leaders {
        let Some(m) = st.monsters.get(leader).cloned() else {
            disband(st, leader);
            continue;
        };
        let Some(ai) = st.monsters.ai_of(leader).map(|a| a.kin.clone()).filter(|k| !k.is_empty()) else { continue };
        if !m.is_alive() {
            disband(st, leader);
            continue;
        }
        let mut band = st.kin.bands.remove(&leader).unwrap_or_default();
        let alive = |st: &WorldState, list: &mut Vec<u32>| list.retain(|c| st.monsters.get(*c).is_some_and(|x| x.is_alive()));
        alive(st, &mut band.summoned);
        alive(st, &mut band.followers);
        alive(st, &mut band.pinch);
        let engaged = st.monsters.engaged.contains(&leader);
        // Invocation : à l'entrée en combat ; fin du combat : elles partent.
        if engaged && !band.summoned_now && !ai.summon_tids.is_empty() {
            band.summoned_now = true;
            band.summoned = call(ctx, st, leader, &ai.summon_tids, ai.summon_max.max(1), now);
            info!("[ia] {} ({leader:#x}) appelle {} monstre(s)", m.name, band.summoned.len());
        } else if !engaged && band.summoned_now {
            band.summoned_now = false;
            for c in std::mem::take(&mut band.summoned) {
                vanish(st, c);
            }
        }
        // Suiveurs.
        let due = ai.followers_due(band.followers.len() as u32, band.created);
        if due > 0 {
            let new = call(ctx, st, leader, &ai.follower_tids, due, now);
            band.created += new.len() as u32;
            debug!("[ia] {} ({leader:#x}) : {} suiveur(s) de plus ({} en tout)", m.name, new.len(), band.created);
            band.followers.extend(new);
        }
        // Renfort en danger.
        if ai.pinch_tid != 0 && !band.pinch_done && (m.hp as f64) < m.max_hp as f64 * crate::config::rules().pinch_hp_percent / 100.0 {
            band.pinch_done = true;
            band.pinch = call(ctx, st, leader, &[ai.pinch_tid], 1, now);
            info!("[ia] {} ({leader:#x}) en danger : renfort {}", m.name, ai.pinch_tid);
        }
        // Agresseurs partagés entre le chef et ses suiveurs ; suiveurs au repos ramenés près du chef.
        for f in band.followers.clone() {
            share_aggro(st, leader, f, now);
            share_aggro(st, f, leader, now);
            let Some(fm) = st.monsters.get(f).cloned() else { continue };
            if fm.aggro.is_empty() && combat::dist2d(fm.pos, m.pos) > SPREAD + FOLLOW_SLACK && !st.monsters.walking.contains_key(&f) {
                let run = st.monsters.stats_of(fm.tid).run;
                let dest = super::skill_extra::contact(m.pos, fm.pos, SPREAD);
                super::ai::walk(&ctx.map, st, f, dest, run);
                if let Some(x) = st.monsters.get_mut(f) {
                    x.home = dest;
                }
            }
        }
        st.kin.bands.insert(leader, band);
    }
}

/// Fin d'un chef (mort, retiré) : ses créatures disparaissent, sa bande est oubliée (elle repart à sa réapparition).
pub fn disband(st: &mut WorldState, leader: u32) {
    let Some(band) = st.kin.bands.remove(&leader) else { return };
    let all: Vec<u32> = band.all().collect();
    for c in all {
        vanish(st, c);
    }
}

/// Mort d'un monstre (`combat::kill`) : un chef perd sa bande ; une créature appelée quitte celle de son chef.
pub fn on_death(st: &mut WorldState, cid: u32) {
    disband(st, cid);
    if let Some(leader) = st.kin.leader_of.get(&cid).copied() {
        if let Some(b) = st.kin.bands.get_mut(&leader) {
            b.summoned.retain(|c| *c != cid);
            b.followers.retain(|c| *c != cid);
            b.pinch.retain(|c| *c != cid);
        }
    }
}

/// Cadavre d'une créature appelée parti : elle est retirée au lieu de réapparaître (`combat::life_step`). Rend vrai si
/// c'en était une.
pub fn remove_corpse(st: &mut WorldState, cid: u32) -> bool {
    if !st.kin.is_kin(cid) {
        return false;
    }
    vanish(st, cid);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_et_suiveurs() {
        let d = serde_json::json!({ "FollowerTID": "1992;1993;1994;1996;1997", "FollowerNUM": "5", "Max_CumulativeFollower": "5", "Min_UpkeepFollower": "3" });
        let k = KinAi::from_data(&d);
        assert_eq!((k.follower_tids.len(), k.follower_num, k.max_cumulative, k.min_upkeep), (5, 5, 5, 3));
        assert_eq!(k.followers_due(0, 0), 5, "à l'apparition");
        assert_eq!(k.followers_due(3, 5), 0, "au moins 3 vivants");
        assert_eq!(k.followers_due(2, 5), 0, "5 créés : plus aucun");
        let k8 = KinAi { max_cumulative: 8, ..k.clone() };
        assert_eq!(k8.followers_due(2, 5), 3, "jusqu'à 5, dans la limite de 8");
        assert_eq!(k8.followers_due(1, 7), 1);
        let s = KinAi::from_data(&serde_json::json!({ "SummonTID": "204;205", "SummonMaxCount": "1", "PinchMonster": "1996" }));
        assert_eq!((s.summon_tids, s.summon_max, s.pinch_tid), (vec![204, 205], 1, 1996));
        assert!(KinAi::from_data(&serde_json::json!({})).is_empty());
    }
}
