//! Récupération : potions de vie et de mana, régénération hors combat.
//!
//! Le client utilise un objet par AGPMITEM op 3 (champ 2 = IID). Une potion (ItemType 1 avec « HP » ou « MP »
//! dans itemdatatable.txt) rend sa quantité de PV ou de PM, sans dépasser le maximum ; elle est refusée au
//! mort, à jauge pleine, ou tant que le délai de sa sorte (vie ou mana) court encore (« Use Interval »).
//! Une utilisation consomme un exemplaire (pile diminuée par op 2, ou objet retiré par op 1) et se confirme
//! par op 4 : TID au champ 3, utilisateur aux champs 4 et 12, délai de réutilisation (ms) au champ 18.
//!
//! Hors combat (aucun coup donné ni reçu depuis 10 s), un joueur vivant regagne toutes les 10 s 1,5 % de ses
//! PV et PM maximum, plus son taux de récupération, plus 5.

use super::items::{item_count, item_remove, ItemCatalog};
use super::{WorldCtx, WorldState};
use crate::character::{factors_mini, Factors, POINT_HP};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use std::time::{Duration, Instant};

/// Opérations AGPMITEM de l'utilisation.
pub const USE_ITEM: i64 = 3;
pub const USE_ITEM_BY_TID: i64 = 4;
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
pub fn used_packet(tid: u32, cid: u32, delay: Duration) -> Vec<u8> {
    let s = &schema::ITEM;
    build_packet(s, &Fields::new(s).with(0, USE_ITEM_BY_TID).with(3, tid).with(4, cid).with(12, cid).with(18, delay.as_millis() as u64))
}

/// AGPMITEM op 3 reçu : utiliser un objet de l'inventaire.
pub async fn use_item(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet, catalog: &ItemCatalog) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let now = Instant::now();
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    if !player.in_game || player.dead {
        return Ok(());
    }
    let Some(item) = player.inventory.get(iid).cloned() else { return Ok(()) };
    let Some(potion) = catalog.get(item.tid).and_then(|d| d.potion) else {
        debug!("[objet] {} utilise l'objet {} : sans effet géré", player.ch.name, item.tid);
        return Ok(());
    };
    if let Err(why) = drink(&mut player.factors, &potion, &mut player.timers, now) {
        debug!("[objet] {} : potion {} refusée ({why:?})", player.ch.name, item.tid);
        return Ok(());
    }
    let (cid, name) = (player.ch.id, player.ch.name.clone());
    match player.inventory.consume(&ctx.db, iid, 1).await? {
        Some(left) => player.send_raw(item_count(&left, cid)),
        None => player.send_raw(item_remove(iid, Some(cid))),
    }
    player.send_raw(used_packet(item.tid, cid, potion.delay));
    let (hp, mp) = (player.factors.point[POINT_HP], player.factors.point[POINT_MP]);
    send_factors(st, sid);
    let label = catalog.get(item.tid).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[objet] {name} boit {label} : {hp} PV, {mp} PM");
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
        if now < p.timers.combat_until {
            continue;
        }
        if regenerate(&mut p.factors) {
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
