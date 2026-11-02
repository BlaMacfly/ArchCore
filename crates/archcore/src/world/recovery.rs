//! Récupération : potions de vie et de mana, régénération hors combat.
//!
//! Le client utilise un objet par AGPMITEM op 3 (champ 2 = IID). Une potion (ItemType 1 avec « HP » ou « MP »
//! dans itemdatatable.txt) rend sa quantité de PV ou de PM, sans dépasser le maximum ; elle est refusée au
//! mort, à jauge pleine, ou tant que le délai de sa sorte (vie ou mana) court encore (« Use Interval »).
//! Une utilisation consomme un exemplaire (pile diminuée par op 2, ou objet retiré par op 1) et se confirme
//! par op 4 : TID au champ 3, utilisateur aux champs 4 et 12, délai de réutilisation (ms) au champ 18.
//!
//! Les autres consommables (compétence d'objet, orbe de résurrection, Gem of origin, objets allumés, feux d'artifice)
//! passent par `consumables.rs`, le Secret Safe par `bank.rs`, les orbes de rappel du groupe (usage 15) par `party.rs`.
//!
//! Hors combat (aucun coup donné ni reçu depuis 10 s), un joueur vivant regagne toutes les 10 s 1,5 % de ses
//! PV et PM maximum, plus son taux de récupération, plus 5. Les options de régénération de son équipement
//! (« regen_hp », « regen_mp ») lui rendent aussi leurs points toutes les 10 s, en combat compris.

use super::items::{item_remove, ItemCatalog};
use super::{WorldCtx, WorldState};
use crate::character::{factors_mini, Factors, POINT_HP};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use std::time::{Duration, Instant};

/// Opérations AGPMITEM de l'utilisation.
pub const USE_ITEM: i64 = 3;
pub const USE_ITEM_BY_TID: i64 = 4;
/// Recharge partagée des objets d'un modèle (serveur → client).
pub const UPDATE_COOLDOWN: i64 = 28;
/// Combat : durée après le dernier coup donné ou reçu.
pub const COMBAT_TIME: Duration = Duration::from_secs(10);
/// Régénération : intervalle.
pub const REGEN_INTERVAL: Duration = Duration::from_secs(10);
/// Délai d'une potion sans « Use Interval ».
const DEFAULT_POTION_DELAY: Duration = Duration::from_secs(2);
const POINT_MP: usize = 1;

/// Effet d'une potion.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Potion {
    pub hp: i32,
    pub mp: i32,
    pub delay: Duration,
}

impl Potion {
    /// Lit une ligne d'itemdatatable.txt : objet utilisable (ItemType 1) qui rend des PV ou des PM.
    pub fn from_raw(raw: &serde_json::Value) -> Option<Self> {
        let num = |k: &str| raw.get(k).and_then(|v| v.as_str()).and_then(|t| t.trim().parse::<f64>().ok()).unwrap_or(0.0);
        let (hp, mp) = (num("HP") as i32, num("MP") as i32);
        if num("ItemType") as i64 != 1 || (hp <= 0 && mp <= 0) {
            return None;
        }
        let ms = num("Use Interval") as u64;
        Some(Potion { hp, mp, delay: if ms > 0 { Duration::from_millis(ms) } else { DEFAULT_POTION_DELAY } })
    }
}

/// Quand chaque sorte de potion redevient utilisable, et état de combat, pour un joueur.
#[derive(Clone, Copy, Debug)]
pub struct Timers {
    pub hp_potion: Instant,
    pub mp_potion: Instant,
    /// En combat jusqu'à cet instant.
    pub combat_until: Instant,
    /// Prochaine régénération.
    pub regen_at: Instant,
}

impl Default for Timers {
    fn default() -> Self {
        let now = Instant::now();
        Timers { hp_potion: now, mp_potion: now, combat_until: now, regen_at: now + REGEN_INTERVAL }
    }
}

/// Le joueur vient de donner ou de recevoir un coup.
pub fn in_combat(t: &mut Timers, now: Instant) {
    t.combat_until = now + COMBAT_TIME;
}

/// Pourquoi une potion ne peut pas servir maintenant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Full,
    Cooldown,
}

/// Applique une potion aux facteurs : rend le délai à poser, ou le refus.
pub fn drink(f: &mut Factors, potion: &Potion, t: &mut Timers, now: Instant) -> Result<(), Refusal> {
    let (point, ready) = if potion.hp > 0 { (POINT_HP, &mut t.hp_potion) } else { (POINT_MP, &mut t.mp_potion) };
    let (amount, max) = if potion.hp > 0 { (potion.hp, f.point_max[0]) } else { (potion.mp, f.point_max[1]) };
    if f.point[point] >= max {
        return Err(Refusal::Full);
    }
    if now < *ready {
        return Err(Refusal::Cooldown);
    }
    f.point[point] = (f.point[point] + amount).min(max);
    *ready = now + potion.delay;
    Ok(())
}

/// Régénération d'une jauge : 1,5 % du maximum + taux de récupération + 5, sans dépasser le maximum.
pub fn regen_amount(max: i32, rate: i32) -> i32 {
    (max as f64 * 0.015) as i32 + rate + 5
}

/// Une régénération sur les facteurs ; rend vrai si quelque chose a changé.
pub fn regenerate(f: &mut Factors) -> bool {
    let mut changed = false;
    for (point, max_i, rate_i) in [(POINT_HP, 0, 0), (POINT_MP, 1, 1)] {
        let max = f.point_max[max_i];
        if f.point[point] < max {
            f.point[point] = (f.point[point] + regen_amount(max, f.recovery[rate_i])).min(max);
            changed = true;
        }
    }
    changed
}

/// AGPMCHARACTER op 1 avec les facteurs du joueur, à lui et à ceux qui le voient.
fn send_factors(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let c = &schema::CHARACTER;
    let plain = build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)));
    p.send_raw(plain.clone());
    let cid = p.ch.id;
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&cid)) {
        o.send_raw(plain.clone());
    }
}

/// AGPMITEM op 4 : objet utilisé.
/// AGPMITEM op 28 (UPDATE_COOLDOWN) : recharge restante des objets du modèle `tid` (3 TID, 4 CID, 5 entier non relevé
/// laissé à 0, 18 temps restant en ms).
pub fn cooldown_packet(tid: u32, cid: u32, left: Duration) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, UPDATE_COOLDOWN).with(3, tid).with(4, cid).with(5, 0).with(18, left.as_millis() as u64))
}

pub fn used_packet(tid: u32, cid: u32, delay: Duration) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, USE_ITEM_BY_TID).with(3, tid).with(4, cid).with(12, cid).with(18, delay.as_millis() as u64))
}

/// AGPMITEM op 3 reçu : utiliser un objet de l'inventaire.
pub async fn use_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet, catalog: &ItemCatalog) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let now = Instant::now();
    let lord = super::archlord::is_lord(st, sid);
    // Objet du sac du familier : seulement si le sac est disponible (le client refuse aussi, « sub inventory disabled »).
    let bag_shut = st.players.get(&sid).is_some_and(|x| x.inventory.sub.contains_key(&iid)) && !super::pet_bag::available(ctx, st, sid);
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    if !player.in_game || bag_shut {
        return Ok(());
    }
    // De l'inventaire, de l'inventaire Cash (familiers, objets de la boutique) ou du sac du familier.
    let Some(item) = player.inventory.get_usable(iid).cloned() else { return Ok(()) };
    // Objets interdits (`disturb` 4, skill_extra.rs).
    if player.skills.status_sent & super::effects::status::NO_ITEM != 0 {
        debug!("[objet] {} : objets interdits (interruption)", player.ch.name);
        return Ok(());
    }
    // Orbe de résurrection : le mort seul s'en sert (consumables.rs).
    if let Some(u) = catalog.uses.get(&item.tid).filter(|u| u.usable_type == super::consumables::USABLE_REVERSE_ORB) {
        return super::consumables::use_reverse_orb(ctx, st, sid, &item, u, now).await;
    }
    if player.dead {
        return Ok(());
    }
    // Potion de métamorphose : ses propres refus (à cheval, transformé, siège : AGPMITEM op 27), morph.rs.
    if let Some(potion) = catalog.morph.get(&item.tid).copied() {
        return super::morph::use_potion(ctx, st, sid, &item, potion, now).await;
    }
    // Sous métamorphose (sauf l'ArchLord) : ni parchemin de téléportation, ni Potion of Quickness ou Beam (morph.rs).
    if player.form.morph.is_some() && !lord && (catalog.get(item.tid).is_some_and(|d| d.scroll.is_some()) || catalog.speed_potions.contains(&item.tid)) {
        debug!("[objet] {} : objet {} refusé sous métamorphose", player.ch.name, item.tid);
        super::send_system(st, sid, "Cet objet ne s'utilise pas sous métamorphose.");
        return Ok(());
    }
    // Évolution du Scion : objet d'oubli des compétences refusé (« IsEvolutionNotUseInitSkillItem »), evolution.rs.
    if player.form.evolved.is_some() && catalog.get(item.tid).is_some_and(|d| d.usable_type == super::morph::USABLE_INIT_SKILL) {
        super::send_system(st, sid, "Vous ne pouvez pas utiliser cet objet pendant l'évolution.");
        return Ok(());
    }
    // À cheval : seuls les consommables marqués « EnableOnRide » servent (ride.rs), et jamais une Potion of Quickness ou
    // Beam (sous-type 1 ou 2), sauf pour l'ArchLord, comme le refuse le client.
    let speed = catalog.speed_potions.contains(&item.tid) && !lord;
    if super::ride::is_mounted(&player.inventory) && (speed || catalog.get(item.tid).is_some_and(|d| !d.enable_on_ride)) {
        debug!("[objet] {} : objet {} refusé à cheval", player.ch.name, item.tid);
        super::send_system(st, sid, "Cet objet ne s'utilise pas à cheval.");
        return Ok(());
    }
    if let Some(scroll) = catalog.get(item.tid).and_then(|d| d.scroll) {
        return super::travel::use_scroll(ctx, st, sid, &item, scroll, now).await;
    }
    // Gem of origin : oubli de toutes les compétences (consumables.rs).
    if catalog.get(item.tid).is_some_and(|d| d.usable_type == super::consumables::USABLE_INIT_SKILL) {
        return super::consumables::use_skill_init_item(ctx, st, sid, &item).await;
    }
    // Consommables sans compétence : Secret Safe (bank.rs), objets allumés et feux d'artifice (consumables.rs). Les
    // autres (Return Orb, objets de commande, Release of Agreement, Marvel Scroll) servent ailleurs : l'op 3 est sans effet.
    match catalog.get(item.tid).map(|d| d.usable_type).unwrap_or(0) {
        super::consumables::USABLE_ADD_BANK_SLOT => return super::bank::use_secret_safe(ctx, st, sid, &item).await,
        u if super::consumables::lit_usage(u) => {
            super::consumables::toggle_lit(ctx, st, sid, &item, u);
            return Ok(());
        }
        super::consumables::USABLE_USE_EFFECT if super::consumables::FIREWORKS.contains(&item.tid) => {
            let u = catalog.uses.get(&item.tid).cloned().unwrap_or_default();
            return super::consumables::use_firework(ctx, st, sid, &item, &u, now).await;
        }
        _ => {}
    }
    // Summoning Orb, Bead of Summoning : rappel du groupe (party.rs).
    if catalog.get(item.tid).is_some_and(|d| d.usable_type == super::party::USABLE_RECALL_PARTY) {
        return super::party::use_recall_item(ctx, st, sid, &item, now).await;
    }
    // Familier, parchemin d'invocation : compétence d'invocation de l'objet (summons.rs).
    if let Some(u) = catalog.use_skill.get(&item.tid).copied().filter(|u| super::summons::is_summons_skill(ctx, u.skill_tid)) {
        return super::summons::use_item(ctx, st, sid, &item, u, now).await;
    }
    // Ragoûts, potions de vitesse, pierres, ailes, charmes, costumes, talismans : compétence de l'objet (consumables.rs).
    let potion = catalog.get(item.tid).and_then(|d| d.potion);
    if let Some(u) = catalog.uses.get(&item.tid).filter(|u| potion.is_none() && !u.skills.is_empty()) {
        return super::consumables::use_skill_item(ctx, st, sid, &item, u, now).await;
    }
    let Some(mut potion) = potion else {
        debug!("[objet] {} utilise l'objet {} : sans effet géré", player.ch.name, item.tid);
        return Ok(());
    };
    // Titre équipé : récupération en plus pour cette potion (effet 9, titles.rs).
    if potion.hp > 0 {
        potion.hp += super::titles::potion_extra(ctx, player, item.tid, potion.hp);
    } else {
        potion.mp += super::titles::potion_extra(ctx, player, item.tid, potion.mp);
    }
    if let Err(why) = drink(&mut player.factors, &potion, &mut player.timers, now) {
        debug!("[objet] {} : potion {} refusée ({why:?})", player.ch.name, item.tid);
        return Ok(());
    }
    let (cid, name) = (player.ch.id, player.ch.name.clone());
    match player.inventory.consume(&ctx.db, iid, 1).await? {
        Some(left) => player.send_raw(player.inventory.count_packet(&left, cid)),
        None => player.send_raw(item_remove(iid, Some(cid))),
    }
    player.send_raw(used_packet(item.tid, cid, potion.delay));
    let (hp, mp) = (player.factors.point[POINT_HP], player.factors.point[POINT_MP]);
    send_factors(st, sid);
    let label = catalog.get(item.tid).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[objet] {name} boit {label} : {hp} PV, {mp} PM");
    // Objectifs de titre : une utilisation de plus de cet objet (Addict).
    super::titles::on_item_used(ctx, st, sid, item.tid).await?;
    Ok(())
}

/// Régénération des joueurs vivants hors combat.
pub fn regen_step(st: &mut WorldState, now: Instant) {
    let mut changed = Vec::new();
    for (sid, p) in st.players.iter_mut() {
        if !p.in_game || p.dead || now < p.timers.regen_at {
            continue;
        }
        p.timers.regen_at = now + REGEN_INTERVAL;
        // Maladie (état 23, status_rules.rs) : aucune régénération.
        if super::status_rules::no_regen(p.skills.status_sent) {
            continue;
        }
        // Options de régénération de l'équipement : en combat aussi.
        let before = (p.factors.point[POINT_HP], p.factors.point[POINT_MP]);
        p.factors.point[POINT_HP] = (p.factors.point[POINT_HP] + p.gear.regen_hp.max(0)).min(p.factors.point_max[0]);
        p.factors.point[POINT_MP] = (p.factors.point[POINT_MP] + p.gear.regen_mp.max(0)).min(p.factors.point_max[1]);
        let geared = before != (p.factors.point[POINT_HP], p.factors.point[POINT_MP]);
        if now < p.timers.combat_until {
            if geared {
                changed.push(*sid);
            }
            continue;
        }
        if regenerate(&mut p.factors) || geared {
            changed.push(*sid);
        }
    }
    for sid in changed {
        send_factors(st, sid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recharge_par_modele() {
        let p = archcore_net::decode_packet(&schema::ITEM, &cooldown_packet(190, 7, Duration::from_millis(1500))).unwrap();
        assert_eq!((p.int(0), p.int(3), p.int(4), p.int(5), p.int(18)), (Some(UPDATE_COOLDOWN), Some(190), Some(7), Some(0), Some(1500)));
    }

    fn hurt(hp: i32, mp: i32) -> Factors {
        let mut f = Factors::default();
        f.point_max[0] = 90;
        f.point_max[1] = 50;
        f.point[POINT_HP] = hp;
        f.point[POINT_MP] = mp;
        f
    }

    #[test]
    fn potions() {
        let life = Potion { hp: 75, mp: 0, delay: Duration::from_secs(2) };
        let mana = Potion { hp: 0, mp: 45, delay: Duration::from_secs(2) };
        let now = Instant::now();
        let mut t = Timers { hp_potion: now, mp_potion: now, combat_until: now, regen_at: now };
        let mut f = hurt(10, 50);
        assert_eq!(drink(&mut f, &life, &mut t, now), Ok(()));
        assert_eq!(f.point[POINT_HP], 85);
        assert_eq!(drink(&mut f, &life, &mut t, now), Err(Refusal::Cooldown), "délai de 2 s");
        assert_eq!(drink(&mut f, &mana, &mut t, now), Err(Refusal::Full), "PM pleins");
        assert_eq!(drink(&mut f, &life, &mut t, now + Duration::from_secs(2)), Ok(()));
        assert_eq!(f.point[POINT_HP], 90, "plafonné au maximum");
        assert_eq!(drink(&mut f, &life, &mut t, now + Duration::from_secs(9)), Err(Refusal::Full));
        f.point[POINT_MP] = 0;
        assert_eq!(drink(&mut f, &mana, &mut t, now), Ok(()), "délai propre à la mana");
        assert_eq!(f.point[POINT_MP], 45);
    }

    #[test]
    fn lecture_d_une_potion() {
        let raw = serde_json::json!({"ItemType":"1","HP":"75","Use Interval":"2000"});
        assert_eq!(Potion::from_raw(&raw), Some(Potion { hp: 75, mp: 0, delay: Duration::from_millis(2000) }));
        assert!(Potion::from_raw(&serde_json::json!({"ItemType":"0","HP":"10"})).is_none(), "équipement avec PV");
        assert!(Potion::from_raw(&serde_json::json!({"ItemType":"1"})).is_none());
    }

    #[test]
    fn regeneration() {
        assert_eq!(regen_amount(90, 0), 6, "1 + 0 + 5");
        assert_eq!(regen_amount(1000, 3), 23);
        let mut f = hurt(85, 48);
        assert!(regenerate(&mut f));
        assert_eq!((f.point[POINT_HP], f.point[POINT_MP]), (90, 50));
        assert!(!regenerate(&mut f), "tout est plein");
    }
}
