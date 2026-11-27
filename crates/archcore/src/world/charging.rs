//! Inventaire étendu (statut d'objet 15, fenêtres `UI_ExpandedInventoryA` et `UI_ExpandedInventoryB` du client),
//! système de l'épisode 8 (présent aussi dans le client de Webzen de 2017).
//!
//! ```text
//!   Disponible : un sac d'inventaire de l'inventaire Cash (usage 38 : « Small Size Inventory », « Large Size
//!     Inventory », TID 50310 à 50318) « en cours d'utilisation » (mini Cash du champ 23, octet à 1), allumé ou éteint
//!     par AGPMITEM op 3 comme un Fortune Manual (consumables.rs). « SubType » 1 : petit sac, 2 : grand sac (Archonia
//!     FUN_14064a4c0 : fenêtre A ou B).
//!   Grille (Archonia FUN_14029a250, grille de type 12) : 4 sacs × 8 lignes × 4 colonnes ; la fenêtre du petit sac
//!     n'en montre qu'un sac de 4 × 4 : ArchCore le borne au sac 0, lignes 0 à 3 (déduit de la fenêtre).
//!   Client → serveur, AGPMITEM op 2 (Archonia FUN_140648270 : statut 15, IID, CID, champ 7 = case), comme le sac du
//!     familier (pet_bag.rs) : dépôt depuis l'inventaire (2), l'équipement (3) ou la banque (100) ; déplacement ou
//!     réunion de piles dans le sac ; statut 2 ou 100 d'un objet du sac : retrait vers l'inventaire, dépôt en banque.
//!   op 3 (utiliser), 17 (séparer), 21 (détruire) : admis pour un objet du sac disponible.
//!   Serveur → client : le sac part (op 0, statut 15, case) la première fois qu'il devient disponible dans la session.
//! ```
//! Sac éteint ou expiré : les objets restent au statut 15 (en base, statut 8), invisibles, jusqu'au prochain sac.
//! Sans sac, le client affiche « HaveToBuyChargingBag ».

use super::items::{free_in, item_add_of, item_count_in, item_remove, item_status, item_update, stored_status, Grid, InventoryItem, CHARGING_INVENTORY, CHARGING_SMALL};
use super::pet_bag::{target_of, Target};
use super::{send_system, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::Packet;
use std::collections::BTreeMap;

/// Usage (« Type ») d'un sac d'inventaire.
pub const USABLE_CHARGING_BAG: i64 = 38;
/// « SubType » du petit et du grand sac.
pub const SMALL_BAG: i64 = 1;
pub const LARGE_BAG: i64 = 2;

/// Grille d'un sac selon son « SubType » (petit : un sac de 4 × 4, grand : 4 × 8 × 4).
pub fn dims_for(sub_type: i64) -> (i64, i64, i64) {
    // « SubType » inconnu : la grille du grand sac, celle du client.
    if sub_type == SMALL_BAG && sub_type != LARGE_BAG { CHARGING_SMALL } else { CHARGING_INVENTORY }
}

/// Grille du sac allumé du joueur `sid`, s'il en a un (le plus grand s'il en allume plusieurs).
pub fn dims_of(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<(i64, i64, i64)> {
    let p = st.players.get(&sid)?;
    p.inventory
        .in_use
        .iter()
        .filter_map(|iid| p.inventory.cash.get(iid))
        .filter_map(|i| ctx.catalog.charging_bags.get(&i.tid).copied())
        .max()
        .map(dims_for)
}

/// L'inventaire étendu du joueur `sid` est disponible.
pub fn available(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    dims_of(ctx, st, sid).is_some()
}

/// Sac allumé ou éteint (AGPMITEM op 3, `consumables::toggle_lit`) : le sac part au client s'il devient disponible.
pub fn on_toggled(ctx: &WorldCtx, st: &mut WorldState, sid: u32) {
    if !available(ctx, st, sid) {
        return;
    }
    send_bag(st, sid);
    if let Some(p) = st.players.get(&sid) {
        info!("[inventaire étendu] {} : sac disponible ({} objet(s))", p.ch.name, p.inventory.charging.len());
    }
}

/// Case libre du sac pour `target` ; `None` si elle est prise ou si le sac (ou ce sac-là) est plein.
pub fn place(bag: &BTreeMap<u32, InventoryItem>, dims: (i64, i64, i64), target: Target) -> Option<Grid> {
    match target {
        Target::Cell(g) => super::items::is_free_in(bag, dims, g).then_some(g),
        Target::Bag(b) => free_in(bag, dims, Some(b)),
        Target::Any => free_in(bag, dims, None),
    }
}

fn max_stack(ctx: &WorldCtx, tid: u32) -> i64 {
    ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1)
}

fn label(ctx: &WorldCtx, tid: u32) -> String {
    ctx.catalog.get(tid).map(|d| d.name.clone()).unwrap_or_else(|| format!("TID {tid}"))
}

/// Envoie au client les objets de l'inventaire étendu (op 0, statut 13), une fois par session.
fn send_bag(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.inventory.charging_sent {
        return;
    }
    p.inventory.charging_sent = true;
    let cid = p.ch.id;
    for i in p.inventory.charging.values() {
        p.send_raw(item_add_of(i, cid, item_status::CHARGING_INVENTORY, Some(i.grid)));
        if let Some(c) = super::items::convert_add(i, cid) {
            p.send_raw(c);
        }
    }
}

/// Renvoie au client la place réelle d'un objet (après un refus).
fn resync(st: &WorldState, sid: u32, iid: u32) {
    let p = &st.players[&sid];
    let cid = p.ch.id;
    if let Some(i) = p.inventory.items.get(&iid) {
        p.send_raw(item_update(i, cid, item_status::INVENTORY));
    } else if let Some(i) = p.inventory.charging.get(&iid) {
        p.send_raw(item_update(i, cid, item_status::CHARGING_INVENTORY));
    } else if let Some(i) = p.inventory.sub.get(&iid) {
        p.send_raw(item_update(i, cid, item_status::SUB_INVENTORY));
    } else if let Some(i) = p.bank.items.get(&iid) {
        p.send_raw(item_update(i, cid, item_status::BANK));
    } else if let Some(i) = p.inventory.equipment.values().find(|i| i.iid == iid) {
        p.send_raw(item_update(i, cid, item_status::EQUIP));
    }
}

fn refuse(st: &WorldState, sid: u32, iid: u32, text: &str) {
    if !text.is_empty() {
        send_system(st, sid, text);
    }
    resync(st, sid, iid);
}

/// AGPMITEM op 2 vers l'inventaire étendu (statut 15) ou d'un objet qui y est.
pub async fn on_item_update(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let status = p.int(1).unwrap_or(-1);
    let Some(dims) = dims_of(ctx, st, sid) else {
        debug!("[inventaire étendu] #{sid} objet {iid} vers le statut {status} : sac indisponible");
        refuse(st, sid, iid, "L'inventaire étendu n'est pas disponible : utilisez d'abord un sac d'inventaire.");
        return Ok(());
    };
    let inside = st.players[&sid].inventory.charging.contains_key(&iid);
    match (status, inside) {
        (item_status::CHARGING_INVENTORY, true) => move_in_bag(ctx, st, sid, iid, target_of(p.bytes(7)), dims).await,
        (item_status::CHARGING_INVENTORY, false) => deposit(ctx, st, sid, iid, target_of(p.bytes(7)), dims).await,
        (item_status::INVENTORY, true) => withdraw(ctx, st, sid, iid, super::equipment::wanted_grid(p)).await,
        (item_status::BANK, true) => to_bank(ctx, st, sid, iid, super::bank::grid_of(p.bytes(8))).await,
        _ => {
            debug!("[inventaire étendu] #{sid} statut {status} pour l'objet {iid} : ignoré");
            resync(st, sid, iid);
            Ok(())
        }
    }
}

/// Origine d'un dépôt dans le sac.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum From {
    Inventory,
    Worn(i64),
    Bank,
}

/// Dépôt dans le sac depuis l'inventaire, l'équipement (contrôle du retrait sous une forme) ou la banque (gardien à
/// portée).
async fn deposit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, target: Target, dims: (i64, i64, i64)) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    let (from, item) = if let Some(i) = p.inventory.items.get(&iid) {
        (From::Inventory, i.clone())
    } else if let Some((part, i)) = p.inventory.equipment.iter().find(|(_, i)| i.iid == iid) {
        (From::Worn(*part), i.clone())
    } else if let Some(i) = p.bank.items.get(&iid) {
        (From::Bank, i.clone())
    } else {
        // Objet Cash, objet d'échange… : rien de tel ne va dans le sac.
        resync(st, sid, iid);
        return Ok(());
    };
    if from == From::Bank && !super::bank::at_keeper(ctx, st, sid) {
        refuse(st, sid, iid, "Parlez à un gardien de banque pour utiliser votre banque.");
        return Ok(());
    }
    if let From::Worn(part) = from {
        if super::equipment::take_off_refused(ctx, st, sid, part, false) {
            return Ok(());
        }
    }
    let cid = p.ch.id;
    // Sur une pile du même objet : les réunir.
    if let Target::Cell(g) = target {
        if let Some(t) = p.inventory.charging.values().find(|i| i.grid == g).cloned() {
            if matches!(from, From::Worn(_)) || t.tid != item.tid || t.count + item.count > max_stack(ctx, item.tid) {
                refuse(st, sid, iid, "");
                return Ok(());
            }
            let total = t.count + item.count;
            ctx.db
                .characters
                .atomic(vec![("UPDATE character_items SET count = ? WHERE id = ?", (total, t.iid).into()), ("DELETE FROM character_items WHERE id = ?", (iid,).into())])
                .await?;
            let p = st.players.get_mut(&sid).unwrap();
            match from {
                From::Bank => p.bank.items.remove(&iid),
                _ => p.inventory.items.remove(&iid),
            };
            let merged = p.inventory.charging.get_mut(&t.iid).unwrap();
            merged.count = total;
            let merged = merged.clone();
            p.send_raw(item_remove(iid, Some(cid)));
            p.send_raw(item_count_in(&merged, cid, item_status::CHARGING_INVENTORY));
            if from == From::Bank {
                crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "retrait", item.tid, item.iid, item.count);
            }
            debug!("[inventaire étendu] {} réunit deux piles de l'objet {} : {total}", p.ch.name, item.tid);
            return Ok(());
        }
    }
    let Some(g) = place(&p.inventory.charging, dims, target) else {
        refuse(st, sid, iid, "L'inventaire étendu est plein.");
        return Ok(());
    };
    ctx.db
        .characters
        .exec("UPDATE character_items SET character_id = ?, status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (cid, stored_status::CHARGING_INVENTORY, g[0], g[1], g[2], iid))
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    match from {
        From::Inventory => {
            p.inventory.items.remove(&iid);
        }
        From::Worn(part) => {
            p.inventory.equipment.remove(&part);
        }
        From::Bank => {
            p.bank.items.remove(&iid);
        }
    }
    let stored = InventoryItem { grid: g, ..item };
    p.inventory.charging.insert(iid, stored.clone());
    p.send_raw(item_update(&stored, cid, item_status::CHARGING_INVENTORY));
    if from == From::Bank {
        crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "retrait", stored.tid, stored.iid, stored.count);
    }
    info!("[inventaire étendu] {} range {} × {} en {g:?} ({from:?})", p.ch.name, stored.count, label(ctx, stored.tid));
    if let From::Worn(part) = from {
        super::equipment::taken_off(ctx, st, sid, &stored, part);
    }
    Ok(())
}

/// Déplacement dans le sac : case libre, ou réunion avec une pile du même objet.
async fn move_in_bag(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, target: Target, dims: (i64, i64, i64)) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let item = p.inventory.charging[&iid].clone();
    if let Target::Cell(g) = target {
        if let Some(t) = p.inventory.charging.values().find(|i| i.grid == g && i.iid != iid).cloned() {
            if t.tid != item.tid || t.count + item.count > max_stack(ctx, item.tid) {
                resync(st, sid, iid);
                return Ok(());
            }
            let total = t.count + item.count;
            ctx.db
                .characters
                .atomic(vec![("UPDATE character_items SET count = ? WHERE id = ?", (total, t.iid).into()), ("DELETE FROM character_items WHERE id = ?", (iid,).into())])
                .await?;
            p.inventory.charging.remove(&iid);
            let merged = p.inventory.charging.get_mut(&t.iid).unwrap();
            merged.count = total;
            let merged = merged.clone();
            p.send_raw(item_remove(iid, Some(cid)));
            p.send_raw(item_count_in(&merged, cid, item_status::CHARGING_INVENTORY));
            return Ok(());
        }
    }
    let Some(g) = place(&p.inventory.charging, dims, target) else {
        resync(st, sid, iid);
        return Ok(());
    };
    ctx.db.characters.exec("UPDATE character_items SET layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (g[0], g[1], g[2], iid)).await?;
    let moved = p.inventory.charging.get_mut(&iid).unwrap();
    moved.grid = g;
    let moved = moved.clone();
    p.send_raw(item_update(&moved, cid, item_status::CHARGING_INVENTORY));
    Ok(())
}

/// Retrait vers l'inventaire : case voulue (champ 7) si libre, réunion avec une pile du même objet, sinon première
/// case libre.
async fn withdraw(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let item = p.inventory.charging[&iid].clone();
    if let Some(t) = wanted.and_then(|g| p.inventory.at(g)).cloned() {
        if t.tid != item.tid || t.count + item.count > max_stack(ctx, item.tid) {
            resync(st, sid, iid);
            return Ok(());
        }
        let total = t.count + item.count;
        ctx.db
            .characters
            .atomic(vec![("UPDATE character_items SET count = ? WHERE id = ?", (total, t.iid).into()), ("DELETE FROM character_items WHERE id = ?", (iid,).into())])
            .await?;
        p.inventory.charging.remove(&iid);
        let merged = p.inventory.items.get_mut(&t.iid).unwrap();
        merged.count = total;
        let merged = merged.clone();
        p.send_raw(item_remove(iid, Some(cid)));
        p.send_raw(item_count_in(&merged, cid, item_status::INVENTORY));
        return Ok(());
    }
    let Some(g) = wanted.filter(|g| p.inventory.is_free(*g)).or_else(|| p.inventory.free_slot()) else {
        refuse(st, sid, iid, "Votre inventaire est plein.");
        return Ok(());
    };
    ctx.db
        .characters
        .exec("UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (stored_status::INVENTORY, g[0], g[1], g[2], iid))
        .await?;
    p.inventory.charging.remove(&iid);
    let taken = InventoryItem { grid: g, ..item };
    p.inventory.items.insert(iid, taken.clone());
    p.send_raw(item_update(&taken, cid, item_status::INVENTORY));
    info!("[inventaire étendu] {} sort {} × {} vers l'inventaire", p.ch.name, taken.count, label(ctx, taken.tid));
    Ok(())
}

/// Dépôt en banque d'un objet du sac (gardien à portée) : case voulue (champ 8) si libre, réunion avec une pile du
/// même objet, sinon première case libre.
async fn to_bank(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    if !super::bank::at_keeper(ctx, st, sid) {
        refuse(st, sid, iid, "Parlez à un gardien de banque pour utiliser votre banque.");
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let item = p.inventory.charging[&iid].clone();
    if let Some(t) = wanted.and_then(|g| p.bank.at(g)).cloned() {
        if t.tid != item.tid || t.count + item.count > max_stack(ctx, item.tid) {
            resync(st, sid, iid);
            return Ok(());
        }
        let total = t.count + item.count;
        ctx.db
            .characters
            .atomic(vec![("UPDATE character_items SET count = ? WHERE id = ?", (total, t.iid).into()), ("DELETE FROM character_items WHERE id = ?", (iid,).into())])
            .await?;
        p.inventory.charging.remove(&iid);
        let merged = p.bank.items.get_mut(&t.iid).unwrap();
        merged.count = total;
        let merged = merged.clone();
        p.send_raw(item_remove(iid, Some(cid)));
        p.send_raw(item_update(&merged, cid, item_status::BANK));
        crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "dépôt", item.tid, item.iid, item.count);
        return Ok(());
    }
    let Some(g) = wanted.filter(|g| p.bank.is_free(*g)).or_else(|| p.bank.free_slot()) else {
        refuse(st, sid, iid, "Votre banque est pleine.");
        return Ok(());
    };
    ctx.db
        .characters
        .exec("UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (stored_status::BANK, g[0], g[1], g[2], iid))
        .await?;
    p.inventory.charging.remove(&iid);
    let stored = InventoryItem { grid: g, ..item };
    p.bank.items.insert(iid, stored.clone());
    p.send_raw(item_update(&stored, cid, item_status::BANK));
    crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "dépôt", stored.tid, stored.iid, stored.count);
    info!("[inventaire étendu] {} dépose en banque {} × {}", p.ch.name, stored.count, label(ctx, stored.tid));
    Ok(())
}

/// AGPMITEM op 17 d'une pile du sac : `n` exemplaires vers une case libre du sac (champ 7).
pub async fn split(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, n: i64, grid: Grid) -> anyhow::Result<()> {
    let Some(dims) = dims_of(ctx, st, sid) else {
        resync(st, sid, iid);
        return Ok(());
    };
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let Some(item) = p.inventory.charging.get(&iid).cloned() else { return Ok(()) };
    if n <= 0 || n >= item.count || !super::items::is_free_in(&p.inventory.charging, dims, grid) {
        resync(st, sid, iid);
        return Ok(());
    }
    let left = item.count - n;
    let new_iid = ctx
        .db
        .characters
        .atomic(vec![
            ("UPDATE character_items SET count = ? WHERE id = ?", (left, iid).into()),
            (super::items::INSERT_ITEM, (cid, item.tid, stored_status::CHARGING_INVENTORY, grid[0], grid[1], grid[2], n).into()),
        ])
        .await? as u32;
    let part = InventoryItem { iid: new_iid, tid: item.tid, count: n, grid, conv: Default::default(), options: Vec::new(), durability: super::items::DURABILITY_FULL };
    p.inventory.charging.insert(new_iid, part.clone());
    let origin = p.inventory.charging.get_mut(&iid).unwrap();
    origin.count = left;
    let origin = origin.clone();
    p.send_raw(super::items::item_add(part.iid, part.tid, Some(cid), part.count, item_status::CHARGING_INVENTORY, Some(part.grid)));
    p.send_raw(item_count_in(&origin, cid, item_status::CHARGING_INVENTORY));
    debug!("[inventaire étendu] {} sépare {n} exemplaire(s) de l'objet {}", p.ch.name, item.tid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(iid: u32, grid: Grid) -> InventoryItem {
        InventoryItem { iid, tid: 190, count: 1, grid, conv: Default::default(), options: Vec::new(), durability: super::super::items::DURABILITY_FULL }
    }

    #[test]
    fn grilles_du_petit_et_du_grand_sac() {
        assert_eq!((dims_for(SMALL_BAG), dims_for(LARGE_BAG)), ((1, 4, 4), (4, 8, 4)));
        let mut bag = BTreeMap::new();
        bag.insert(1, item(1, [0, 0, 0]));
        assert_eq!(place(&bag, dims_for(SMALL_BAG), Target::Any), Some([0, 0, 1]));
        assert_eq!(place(&bag, dims_for(SMALL_BAG), Target::Cell([0, 4, 0])), None, "ligne 4 hors du petit sac");
        assert_eq!(place(&bag, dims_for(LARGE_BAG), Target::Cell([3, 7, 3])), Some([3, 7, 3]));
        assert_eq!(place(&bag, dims_for(SMALL_BAG), Target::Bag(1)), None, "un seul sac dans le petit");
    }
}
