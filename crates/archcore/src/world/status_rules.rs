//! Règles des états spéciaux que le client affiche sans les jouer (special_status de SKILL_SPEC, codes de l'épisode 8).
//!
//! ```text
//!   5 invincibilité élémentaire   parts élémentaires (10 à 17) d'un coup reçu, dégâts sur la durée compris, à 0.
//!   6 aggro non ajoutée           les coups du porteur n'ajoutent rien à la liste d'un monstre où il n'est pas déjà :
//!                                 le monstre ne riposte pas.
//!   7 aggro cachée                à la pose, le porteur est retiré des listes des monstres voisins (et de leur cible) ;
//!                                 il n'y revient qu'en frappant ; les monstres préventifs l'ignorent.
//!   9 halte, 10 invisibilité      voir `effects::status` (9 : ni compétence ni attaque ; 10 : monstres aveugles,
//!                                 joueurs aussi sans détection, `skill_extra.rs`).
//!   18 désarmement                ni attaque normale ni compétence exigeant une arme (`requirement.rs`) ; les armes
//!                                 portées ne comptent plus dans les facteurs.
//!   19 mutisme                    discussion refusée (tous les canaux, message système).
//!   21 confusion                  un monstre frappe, toutes les `Rules.ConfusionIntervalMs` (1500), une cible tirée au
//!                                 hasard autour de lui (joueurs et monstres) ; un monstre ainsi frappé garde 1 PV.
//!   23 maladie                    pas de régénération de PV ni de PM (`recovery.rs`). « infect_around » : absent des
//!                                 tables de l'épisode 8, rien.
//!   24 berserk                    comme la confusion, mais la cible la plus proche ; insensible à la provocation.
//!   25 rétrécissement             portée d'attaque et dégâts physiques diminués de `Rules.ShrinkPercent` (30 %).
//! ```
//! Côté joueur, la confusion et le berserk ne font qu'envoyer leur bit d'état (le client règle ses commandes) : choix.
//! Règles proposées, rien ne les montre dans Archonia ; les états viennent des tables EP8 (`effects::ep8_special_status`).

use super::combat::{self, Attacker, Defender};
use super::effects::status;
use super::monsters::Monster;
use super::WorldState;
use crate::character::Factors;
use crate::debug;
use rand::Rng;
use std::time::{Duration, Instant};

/// Indices des parts élémentaires dans le résultat.
const ELEMENTS: std::ops::RangeInclusive<usize> = 10..=17;
/// Rayon dans lequel un monstre confus ou berserk cherche sa cible.
pub const FRENZY_RANGE: f64 = 600.0;

/// Invincibilité élémentaire : parts élémentaires retirées d'un coup reçu par un porteur de cet état.
pub fn shield(status: u64, parts: &mut Vec<(usize, i32)>) {
    if status & status::ATTRIBUTE_INVINCIBLE != 0 {
        parts.retain(|(i, _)| !ELEMENTS.contains(i));
    }
}

/// Aggro non ajoutée : le joueur `attacker` (CID) n'entre pas dans la liste du monstre `cid` s'il porte l'état 6 et
/// n'y est pas déjà.
pub fn adds_aggro(st: &WorldState, cid: u32, attacker: u32) -> bool {
    let quiet = st.by_cid.get(&attacker).and_then(|s| st.players.get(s)).is_some_and(|p| p.skills.status_sent & status::NOT_ADD_AGGRO != 0);
    !quiet || st.monsters.get(cid).is_some_and(|m| m.aggro.iter().any(|a| a.cid == attacker))
}

/// Aggro cachée posée : le joueur `sid` sort des listes des monstres qui le voient et de leur cible.
pub fn hide_aggro(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let (cid, seen): (u32, Vec<u32>) = (p.ch.id, p.seen_monsters.iter().copied().collect());
    let mut n = 0;
    for m in seen {
        let Some(x) = st.monsters.get_mut(m) else { continue };
        let before = x.aggro.len();
        x.aggro.retain(|a| a.cid != cid);
        if x.target == Some(cid) {
            x.target = None;
        }
        n += before - x.aggro.len();
    }
    debug!("[état] aggro cachée : {cid:#x} retiré de {n} liste(s)");
}

/// États qui viennent d'être posés sur un joueur (`old` → `new`) : aggro cachée.
pub fn on_player_status(st: &mut WorldState, sid: u32, old: u64, new: u64) {
    if new & !old & status::HIDE_AGGRO != 0 {
        hide_aggro(st, sid);
    }
}

/// Le joueur ne peut pas parler (mutisme).
pub fn muted(status: u64) -> bool {
    status & status::DISABLE_CHATTING != 0
}

/// Pas de régénération (maladie).
pub fn no_regen(status: u64) -> bool {
    status & status::DISEASE != 0
}

/// Part gardée par le rétrécissement (1 sans lui).
pub fn shrink_keep(status: u64) -> f64 {
    if status & status::SHRINK != 0 { (100.0 - crate::config::rules().shrink_percent) / 100.0 } else { 1.0 }
}

/// Rétrécissement d'un joueur : portée d'attaque et dégâts physiques réduits.
pub fn shrink_factors(f: &mut Factors, status: u64) {
    let k = shrink_keep(status);
    if k >= 1.0 {
        return;
    }
    f.attack[0] = ((f.attack[0] as f64 * k).round() as i32).max(50);
    f.damage_min[0] = (f.damage_min[0] as f64 * k).round() as i32;
    f.damage_max[0] = (f.damage_max[0] as f64 * k).round() as i32;
}

/// Désarmement : armes des mains retirées d'une copie de l'inventaire, pour le calcul des facteurs.
pub fn disarmed(inv: &super::items::Inventory, catalog: &super::items::ItemCatalog) -> super::items::Inventory {
    let mut out = inv.clone();
    out.equipment.retain(|at, i| {
        !(matches!(*at, super::equipment::part::HAND_LEFT | super::equipment::part::HAND_RIGHT)
            && catalog.get(i.tid).and_then(|d| d.equip.as_ref()).is_some_and(|e| e.kind == super::equipment::KIND_WEAPON))
    });
    out
}

/// Cible d'un monstre confus (au hasard) ou berserk (la plus proche) : joueur vivant ou autre monstre vivant, dans
/// `FRENZY_RANGE`. Rend (CID, joueur ?).
pub fn frenzy_target(st: &WorldState, m: &Monster, berserk: bool, rng: &mut impl Rng) -> Option<(u32, bool)> {
    let mut near: Vec<(f64, u32, bool)> = st
        .players
        .values()
        .filter(|p| p.in_game && !p.dead && p.skills.status_sent & status::UNTOUCHABLE == 0)
        .map(|p| (combat::dist2d(p.ch.pos(), m.pos), p.ch.id, true))
        .filter(|(d, _, _)| *d <= FRENZY_RANGE)
        .collect();
    near.extend(st.monsters.near(m.pos, FRENZY_RANGE).filter(|o| o.cid != m.cid && o.is_alive() && o.owner.is_none()).map(|o| (combat::dist2d(o.pos, m.pos), o.cid, false)).filter(|(d, _, _)| *d <= FRENZY_RANGE));
    if near.is_empty() {
        return None;
    }
    if berserk {
        near.into_iter().min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, c, p)| (c, p))
    } else {
        let (_, c, p) = near[rng.gen_range(0..near.len())];
        Some((c, p))
    }
}

/// Coup d'un monstre confus ou berserk sur un autre monstre : coup normal, la cible garde au moins 1 PV (ni mort ni
/// expérience sans joueur). Rend les dégâts.
pub fn strike_monster(st: &mut WorldState, cid: u32, tcid: u32, now: Instant) -> i32 {
    let (Some(m), Some(t)) = (st.monsters.get(cid).cloned(), st.monsters.get(tcid).cloned()) else { return 0 };
    let (stats, tstats) = (st.monsters.stats_for(&m), st.monsters.stats_for(&t));
    let attacker = Attacker::of_monster(m.level as i32, &stats, &st.monsters.mods(cid));
    let defender = Defender::of_monster(t.level as i32, &tstats, &st.monsters.mods(tcid));
    let mut rng = rand::thread_rng();
    let (outcome, damage) = combat::strike_at(&mut rng, combat::monster_hit_chance(&attacker, &defender), &attacker, &defender);
    let target = st.monsters.get_mut(tcid).unwrap();
    target.hp = (target.hp - damage).max(1);
    let hp = target.hp;
    let factors = (damage > 0).then(|| combat::damage_factors(hp, tstats.max_mp, damage));
    let watchers = combat::viewers(st, tcid);
    combat::broadcast(st, &watchers, &combat::action_packet(cid, tcid, outcome, factors));
    if let Some(x) = st.monsters.get_mut(cid) {
        x.ready_at = now + Duration::from_millis(crate::config::rules().confusion_interval_ms);
    }
    debug!("[état] {} ({cid:#x}) frappe {tcid:#x} dans sa fureur : {damage} dégât(s)", m.name);
    damage
}

/// États d'un monstre qui lui font frapper n'importe qui : (confusion ou berserk, berserk ?).
pub fn frenzied(state: u64) -> Option<bool> {
    (state & (status::CONFUSION | status::BERSERK) != 0).then_some(state & status::BERSERK != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regles_des_etats() {
        let mut parts = vec![(9, 30), (12, 15), (19, 5)];
        shield(status::ATTRIBUTE_INVINCIBLE, &mut parts);
        assert_eq!(parts, vec![(9, 30), (19, 5)], "élémentaire retiré, physique et héroïque gardés");
        let mut parts = vec![(12, 15)];
        shield(0, &mut parts);
        assert_eq!(parts, vec![(12, 15)]);
        assert!(muted(status::DISABLE_CHATTING) && !muted(status::STUN));
        assert!(no_regen(status::DISEASE) && !no_regen(0));
        let mut f = Factors::default();
        f.attack[0] = 300;
        f.damage_min[0] = 100;
        f.damage_max[0] = 200;
        shrink_factors(&mut f, status::SHRINK);
        assert_eq!((f.attack[0], f.damage_min[0], f.damage_max[0]), (210, 70, 140), "− 30 %");
        assert_eq!((frenzied(status::CONFUSION), frenzied(status::BERSERK), frenzied(status::STUN)), (Some(false), Some(true), None));
    }

    #[test]
    fn aggro_non_ajoutee() {
        let mut st = WorldState::default();
        st.by_cid.insert(7, 1);
        assert!(adds_aggro(&st, 0x5000_0001, 7), "joueur sans session connue : rien ne change");
        assert!(adds_aggro(&st, 0x5000_0001, 8));
    }
}
