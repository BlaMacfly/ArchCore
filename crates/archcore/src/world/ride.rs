//! Montures : monter, descendre, vitesse à cheval.
//!
//! Une monture est un objet d'équipement de partie 14 (Kind 32). Le client ne demande jamais de monter par
//! AgpmRide (0x4D) : il équipe l'objet par AGPMITEM op 2 (statut 3) et le retire pour descendre (statut 2). Le
//! serveur fait l'équipement ordinaire (`equipment.rs`), recalcule les facteurs, puis annonce la montée par AgpmRide
//! op 2 (CID, IID, temps restant 0) au cavalier et aux joueurs qui le voient, la descente par l'op 4. Le client
//! crée alors la monture (modèle « SubType » de l'objet) et y installe le cavalier ; il doit déjà connaître l'objet.
//!
//! - Vitesse : à cheval, « Run_Buff » de l'objet remplace marche et course (facteurs, `recompute_factors`) ; le
//!   client lève son plafond de 60 000 quand le personnage est monté.
//! - Vue : bit 1 du champ 18 du paquet AGPMCHARACTER (« transformation ») ; un joueur qui arrive en vue reçoit le
//!   personnage, son équipement, puis l'op 2.
//! - Combat à cheval permis : l'arme montée (partie 15) remplace celles des mains ; les compétences sont refusées
//!   (seules celles marquées `auto_attack`, que le client ne lance pas, échappent à la règle), comme les
//!   consommables sans « EnableOnRide ».
//! - Refus de monter (l'objet reste dans l'inventaire) : en combat (coup donné ou reçu depuis moins de 10 s,
//!   `recovery::COMBAT_TIME`), meurtrier de palier 2 ou plus, région sans le bit 0x10 du champ Type de
//!   RegionTemplate, personnage mort (déjà refusé par l'équipement).
//! - Descente forcée : à la mort, et dans une région sans montures (marche, téléportation), vérifiées à chaque tour
//!   de la boucle de combat. Pas de retrait des renforcements en montant.
//! - Durée : aucune monture d'itemdatatable.txt n'a « RemainTime » ni « ExpireTime », et l'op 5 (fin de durée)
//!   n'a pas d'effet dans le client : ArchCore ne fait pas expirer les montures (choix d'ArchCore).

use super::equipment::part;
use super::items::{Inventory, InventoryItem, ItemCatalog};
use super::{send_system, WorldCtx, WorldState};
use crate::character::factors_mini;
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields};
use std::time::Instant;

pub mod ride_op {
    pub const RIDE_ACK: i64 = 2;
    pub const DISMOUNT_ACK: i64 = 4;
}

/// Bit 1 du champ 18 d'AGPMCHARACTER : personnage monté.
pub const TRANSFORM_RIDING: i64 = 0x02;
/// Bit du champ Type de RegionTemplate : montures permises.
pub const REGION_RIDE_ALLOWED: u32 = 0x10;
/// Codes AgpmSystemMessage : « CannotRideWhileCombat » et objet interdit dans cette région (déduits de l'énumération
/// dont ArchCore a validé les codes 29 et 30 de la banque).
pub const MSG_CANNOT_RIDE_IN_COMBAT: i64 = 24;
pub const MSG_DISABLE_EQUIP_THIS_REGION: i64 = 42;
// Pénalités « CharacterPenalty » relevées dans le client (`FUN_1404fa8f0`, rappel 0x5D d'AgpmCharacter) : 3
// « _CantConvert » (réparation), 5 « _CantRide ». Le paquet qui les déclenche n'est pas établi : ArchCore répond
// au meurtrier par un message système.

/// Pourquoi un joueur ne peut pas monter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Combat,
    Murderer,
    Region,
    Dead,
}

/// Le personnage porte une monture.
pub fn is_mounted(inv: &Inventory) -> bool {
    inv.equipment.contains_key(&part::RIDE)
}

/// Vitesse à cheval : « Run_Buff » de la monture portée, s'il y en a une.
pub fn mount_speed(inv: &Inventory, catalog: &ItemCatalog) -> Option<i32> {
    let item = inv.equipment.get(&part::RIDE)?;
    catalog.get(item.tid)?.equip.as_ref().map(|e| e.run_buff).filter(|v| *v > 0)
}

/// Bits du champ 18 d'AGPMCHARACTER (« transformation ») pour cet inventaire.
pub fn transform_bits(inv: &Inventory) -> i64 {
    if is_mounted(inv) { TRANSFORM_RIDING } else { 0 }
}

/// Une région permet les montures (bit 0x10 du champ Type) ; une région inconnue aussi.
pub fn region_allows(type_bits: Option<u32>) -> bool {
    type_bits.is_none_or(|b| b & REGION_RIDE_ALLOWED != 0)
}

/// La montée est-elle permise ? Ordre : mort, combat, meurtrier, région.
pub fn check(dead: bool, in_combat: bool, murderer_level: usize, region_bits: Option<u32>) -> Result<(), Refusal> {
    if dead {
        return Err(Refusal::Dead);
    }
    if in_combat {
        return Err(Refusal::Combat);
    }
    if murderer_level >= 2 {
        return Err(Refusal::Murderer);
    }
    if !region_allows(region_bits) {
        return Err(Refusal::Region);
    }
    Ok(())
}

/// Bits du champ Type de la région où se trouve le joueur.
fn region_bits(ctx: &WorldCtx, p: &super::Player) -> Option<u32> {
    ctx.map.region(p.ch.pos_x, p.ch.pos_z).and_then(|r| ctx.regions.type_bits(r))
}

/// Refus de monter pour le joueur `sid`, ou rien.
pub fn refusal(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<Refusal> {
    let p = st.players.get(&sid)?;
    let fighting = Instant::now() < p.timers.combat_until;
    check(p.dead, fighting, super::pvp::murderer_level(p.ch.murderer), region_bits(ctx, p)).err()
}

fn system_message(st: &WorldState, sid: u32, code: i64) {
    if let Some(p) = st.players.get(&sid) {
        let s = &schema::SYSTEM_MESSAGE;
        p.send(s, Fields::new(s).with(0, 0).with(1, code));
    }
}

/// Signale un refus au joueur.
pub fn refuse(st: &WorldState, sid: u32, why: Refusal) {
    match why {
        Refusal::Combat => system_message(st, sid, MSG_CANNOT_RIDE_IN_COMBAT),
        Refusal::Region => system_message(st, sid, MSG_DISABLE_EQUIP_THIS_REGION),
        Refusal::Murderer => send_system(st, sid, "Un meurtrier ne peut pas monter à cheval."),
        Refusal::Dead => {}
    }
    if let Some(p) = st.players.get(&sid) {
        info!("[monture] {} : montée refusée ({why:?})", p.ch.name);
    }
}

/// AgpmRide : opération, cavalier, objet monture.
pub fn ride_packet(op: i64, cid: u32, iid: u32) -> Vec<u8> {
    let s = &schema::RIDE;
    build_packet(s, &Fields::new(s).with(0, op).with(1, cid).with(2, iid).with(3, 0))
}

/// Facteurs du joueur (vitesse) aux joueurs qui le voient, qui sinon le verraient avancer à l'ancienne vitesse.
fn factors_to_viewers(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    let c = &schema::CHARACTER;
    let plain = build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(7, factors_mini(&p.factors)));
    for (_, o) in st.players.iter().filter(|(other, o)| **other != sid && o.seen_players.contains(&p.ch.id)) {
        o.send_raw(plain.clone());
    }
}

/// Monture équipée (objet déjà annoncé, facteurs recalculés) : facteurs aux voisins, op 2 à tous.
pub fn on_mount(st: &WorldState, sid: u32, item: &InventoryItem) {
    let Some(p) = st.players.get(&sid) else { return };
    factors_to_viewers(st, sid);
    super::combat::send_around(st, sid, &ride_packet(ride_op::RIDE_ACK, p.ch.id, item.iid));
    info!("[monture] {} monte (objet {}, course {})", p.ch.name, item.tid, p.factors.status[9]);
}

/// Monture retirée : facteurs aux voisins, op 4 à tous.
pub fn on_dismount(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    factors_to_viewers(st, sid);
    super::combat::send_around(st, sid, &ride_packet(ride_op::DISMOUNT_ACK, p.ch.id, 0));
    info!("[monture] {} descend (course {})", p.ch.name, p.factors.status[9]);
}

/// Paquet AgpmRide op 2 d'un joueur monté, pour lui-même (entrée en jeu, après ses objets) ou un joueur qui
/// le voit (après son équipement).
pub fn view_packet(p: &super::Player) -> Option<Vec<u8>> {
    p.inventory.equipment.get(&part::RIDE).map(|i| ride_packet(ride_op::RIDE_ACK, p.ch.id, i.iid))
}

/// Entrée en jeu avec une monture portée : l'op 2, ses objets étant déjà connus du client.
pub fn on_enter(st: &WorldState, sid: u32) {
    if let Some(pkt) = st.players.get(&sid).and_then(view_packet) {
        st.players[&sid].send_raw(pkt);
    }
}

/// Descentes forcées : cavalier mort, ou dans une région sans montures. Un inventaire plein empêche de ranger la
/// monture : le joueur en est averti une fois, et la descente est retentée aux tours suivants.
pub async fn step(ctx: &WorldCtx, st: &mut WorldState) -> anyhow::Result<()> {
    let due: Vec<(u32, &'static str)> = st
        .players
        .iter()
        .filter(|(_, p)| p.in_game && is_mounted(&p.inventory))
        .filter_map(|(sid, p)| {
            if p.dead {
                Some((*sid, "mort"))
            } else if !region_allows(region_bits(ctx, p)) {
                Some((*sid, "région sans montures"))
            } else {
                None
            }
        })
        .collect();
    for (sid, why) in due {
        if super::equipment::force_unequip(ctx, st, sid, part::RIDE).await? {
            if let Some(p) = st.players.get_mut(&sid) {
                p.ride_stuck = false;
                info!("[monture] {} descend de force ({why})", p.ch.name);
            }
            if why != "mort" {
                system_message(st, sid, MSG_DISABLE_EQUIP_THIS_REGION);
            }
        } else if let Some(p) = st.players.get_mut(&sid).filter(|p| !p.ride_stuck) {
            p.ride_stuck = true;
            debug!("[monture] {} : descente forcée impossible, inventaire plein ({why})", p.ch.name);
            send_system(st, sid, "Inventaire plein : impossible de ranger votre monture.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::items::{EquipData, ItemData, DURABILITY_FULL};

    #[test]
    fn refus_de_monter() {
        assert_eq!(check(false, false, 0, Some(786929)), Ok(()), "Anchorville : bit 0x10");
        assert_eq!(check(false, false, 0, None), Ok(()), "région inconnue");
        assert_eq!(check(false, true, 0, Some(0x10)), Err(Refusal::Combat));
        assert_eq!(check(false, false, 2, Some(0x10)), Err(Refusal::Murderer));
        assert_eq!(check(false, false, 1, Some(0x10)), Ok(()), "palier 1 : permis");
        assert_eq!(check(false, false, 0, Some(0x0f)), Err(Refusal::Region));
        assert_eq!(check(true, true, 3, Some(0)), Err(Refusal::Dead));
    }

    #[test]
    fn vitesse_et_vue_a_cheval() {
        let horse = EquipData { part: part::RIDE, kind: 32, run_buff: 10500, ride_tid: 567, durability: 252000, ..Default::default() };
        let mut catalog = ItemCatalog::default();
        catalog.items.insert(3002, ItemData { tid: 3002, name: "White Horse".into(), npc_price: 200000, pc_price: 0, stack: 1, equip: Some(horse), potion: None, scroll: None, stone: None, usable_type: 0, option_tids: Vec::new(), kind: 32, bound_type: 0, cash: false, enable_on_ride: false, auction_category: (0, 0), event_item: false });
        let mut inv = Inventory::default();
        assert_eq!((mount_speed(&inv, &catalog), transform_bits(&inv)), (None, 0));
        inv.equipment.insert(part::RIDE, InventoryItem { iid: 7, tid: 3002, count: 1, grid: [14, 0, 0], conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL });
        assert_eq!((mount_speed(&inv, &catalog), transform_bits(&inv)), (Some(10500), TRANSFORM_RIDING));
        let p = archcore_net::decode_packet(&schema::RIDE, &ride_packet(ride_op::RIDE_ACK, 3, 7)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(3)), (Some(2), Some(3), Some(7), Some(0)));
    }
}
