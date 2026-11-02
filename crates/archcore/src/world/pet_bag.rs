//! Sac du familier (statut d'objet 13, fenêtre `UI_SubInventory` du client) et son ramassage automatique (AGPMITEM
//! op 30).
//!
//! ```text
//!   Disponible : un objet de l'inventaire Cash (statut 51) « en cours d'utilisation » (mini Cash du champ 23, octet à
//!     1), de type Cash 6 (familier), utilisable, dont la créature (`summons_tid` de sa compétence) a « PetType » = 1
//!     (CharacterDataTable). C'est la règle du client : sac indisponible, il ignore tout objet du statut 13.
//!   Grille : 4 sacs × 2 lignes × 4 colonnes (champ 7, mini ITEM_GRID : sac, ligne, colonne).
//!   Client → serveur, AGPMITEM op 2 :
//!     statut 13, IID, CID, champ 7 = case : dépôt depuis l'inventaire (2), l'équipement (3) ou la banque (100) ;
//!       dans le sac, déplacement, ou réunion de deux piles du même objet. Ligne et colonne à -1 (dépôt sur l'onglet
//!       d'un sac) : première case libre de ce sac ; sans champ 7 : première case libre du sac du familier.
//!     statut 2 (case au champ 7) ou 100 (case au champ 8) d'un objet du sac : retrait vers l'inventaire, dépôt en
//!       banque. Poser sur une pile du même objet les réunit.
//!   op 3 (utiliser), 17 (séparer, case du sac au champ 7), 21 (détruire) : admis pour un objet du sac.
//!   op 30 : champ 2 = IID du familier, champ 4 = CID, champ 20 = {INT16 0/1} (bouton « Auto Pick Up ») ← op 30 en
//!     écho ; valeur gardée dans `characters.pet_auto_pick` (1 par défaut, comme le client).
//!   Serveur → client : le sac part (op 0, statut 13, case) la première fois qu'il devient disponible dans la session,
//!     après les objets Cash ; le familier rappelé, le client garde ses objets, invisibles, jusqu'au prochain appel.
//! ```
//! Refus (sac indisponible, case prise, banque fermée, inventaire plein) : l'objet est renvoyé à sa place chez le client.
//! Familier rappelé ou renvoyé : les objets restent au statut 13 (en base, statut 7) ; rien n'est perdu.
//! Choix d'ArchCore (règles d'origine inconnues) :
//! - Ramassage (op 11) : l'inventaire d'abord, puis le sac du familier s'il est disponible. La valeur de l'op 30 est
//!   enregistrée et renvoyée, sans ramassage à la chute (règle exacte inconnue).
//! - Écartés (ajouts d'Archonia) : marquage des sacs (Ctrl+clic, ops 37/38), « Sell All », et les familiers des TID
//!   14821 à 15674 (absents des tables du monde EP8), qui sortent sans donner de sac.
//! - Ni achat en boutique, ni retrait de la grille d'échange vers le sac, ni équipement depuis le sac.

use super::items::{free_in, item_add_of, item_count_in, item_remove, item_status, item_update, stored_status, use_time_packet, Grid, InventoryItem, SUB_INVENTORY};
use super::{send_system, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, build_packet, decode_mini, Fields, Packet};
use std::collections::BTreeMap;
use std::ops::RangeInclusive;

/// Type Cash d'un familier (colonne « Cash » d'itemdatatable).
pub const CASH_PET: i64 = 6;
/// « PetType » d'une créature qui donne un sac.
pub const PET_TYPE_BAG: i32 = 1;
/// Familiers ajoutés par Archonia (Yabbit, Lamba, Zaryndor, Troll, Gzuz, Bondman), écartés : pas de sac.
pub const ARCHONIA_PETS: RangeInclusive<u32> = 14821..=15674;

/// Règle du sac : objet familier (type Cash 6) utilisable (« ItemType » 1, `usable_type` non nul), créature de
/// « PetType » 1, hors ajouts d'Archonia.
pub fn bag_rule(tid: u32, cash: i64, usable_type: i64, pet_type: Option<i32>) -> bool {
    !ARCHONIA_PETS.contains(&tid) && cash == CASH_PET && usable_type > 0 && pet_type == Some(PET_TYPE_BAG)
}

/// Créature d'un objet familier : `summons_tid` de sa compétence (« Use skill id », nature 8 de SKILL_SPEC).
pub fn pet_creature(ctx: &WorldCtx, tid: u32) -> Option<u32> {
    let u = ctx.catalog.use_skill.get(&tid)?;
    let t = ctx.skills.get(u.skill_tid).filter(|t| t.summons == super::summons::skill_kind::PET)?;
    let lv = t.level(u.level).or_else(|| t.levels.first())?;
    (lv.summons_tid > 0).then_some(lv.summons_tid)
}

/// L'objet `tid` donne un sac du familier quand il est en cours d'utilisation.
pub fn gives_bag(ctx: &WorldCtx, st: &WorldState, tid: u32) -> bool {
    let Some(d) = ctx.catalog.get(tid) else { return false };
    let pet_type = pet_creature(ctx, tid).and_then(|c| st.monsters.stats.get(&c)).map(|s| s.pet_type);
    bag_rule(tid, d.cash, d.usable_type, pet_type)
}

/// Le sac du familier du joueur `sid` est disponible.
pub fn available(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    let Some(p) = st.players.get(&sid) else { return false };
    p.inventory.in_use.iter().any(|iid| p.inventory.cash.get(iid).is_some_and(|i| gives_bag(ctx, st, i.tid)))
}

/// AGPMITEM op 30 : ramassage automatique du sac du familier (`iid` : l'objet du familier).
pub fn auto_pick_packet(iid: u32, owner: u32, on: bool) -> Vec<u8> {
    let (s, m) = (&schema::ITEM, &mini::ITEM_AUTO_PICK);
    let value = build_mini(m, &Fields::new(m).with(0, i64::from(on)));
    build_packet(s, &Fields::new(s).with(0, super::items::item_op::CHANGE_AUTO_PICK).with(2, iid).with(4, owner).with(20, value))
}

/// Case visée dans le sac (champ 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Pas de champ 7, ou sac à -1 : première case libre.
    Any,
    /// Onglet d'un sac (ligne ou colonne à -1) : première case libre de ce sac.
    Bag(i64),
    Cell(Grid),
}

/// Case visée d'après le champ 7 (une coordonnée à 0xFFFF se lit -1).
pub fn target_of(bytes: Option<&[u8]>) -> Target {
    let Some(g) = bytes.and_then(|b| decode_mini(&mini::ITEM_GRID, b).ok()) else { return Target::Any };
    let v = [g.int(0).unwrap_or(-1), g.int(1).unwrap_or(-1), g.int(2).unwrap_or(-1)];
    match v {
        [b, r, c] if b >= 0 && r >= 0 && c >= 0 => Target::Cell(v),
        [b, _, _] if b >= 0 => Target::Bag(b),
        _ => Target::Any,
    }
}

/// Case libre du sac pour `target` ; `None` si elle est prise ou si le sac (ou ce sac-là) est plein.
pub fn place(sub: &BTreeMap<u32, InventoryItem>, target: Target) -> Option<Grid> {
    match target {
        Target::Cell(g) => super::items::is_free_in(sub, SUB_INVENTORY, g).then_some(g),
        Target::Bag(b) => free_in(sub, SUB_INVENTORY, Some(b)),
        Target::Any => free_in(sub, SUB_INVENTORY, None),
    }
}

fn max_stack(ctx: &WorldCtx, tid: u32) -> i64 {
    ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1)
}

fn label(ctx: &WorldCtx, tid: u32) -> String {
    ctx.catalog.get(tid).map(|d| d.name.clone()).unwrap_or_else(|| format!("TID {tid}"))
}

/// Envoie au client les objets du sac du familier (op 0, statut 13), une fois par session.
fn send_bag(st: &mut WorldState, sid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if p.inventory.sub_sent {
        return;
    }
    p.inventory.sub_sent = true;
    let cid = p.ch.id;
    for i in p.inventory.sub.values() {
        p.send_raw(item_add_of(i, cid, item_status::SUB_INVENTORY, Some(i.grid)));
        if let Some(c) = super::items::convert_add(i, cid) {
            p.send_raw(c);
        }
    }
}

/// Familier sorti par l'objet `iid` : objet « en cours d'utilisation » (op 26, mini Cash à 1), puis le sac s'il devient
/// disponible. Rien pour un objet hors de l'inventaire Cash (le client n'y attend pas de mini Cash).
pub fn on_pet_used(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    if !p.inventory.cash.contains_key(&iid) {
        return;
    }
    p.inventory.in_use.insert(iid);
    p.send_raw(use_time_packet(iid, p.ch.id, true));
    let name = p.ch.name.clone();
    if available(ctx, st, sid) {
        send_bag(st, sid);
        let n = st.players.get(&sid).map(|p| p.inventory.sub.len()).unwrap_or(0);
        info!("[sac du familier] {name} : sac disponible ({n} objet(s))");
    }
}

/// Familier de l'objet `iid` parti (rappelé, renvoyé, maître mort ou parti) : l'objet n'est plus « en cours
/// d'utilisation » (op 26, mini Cash à 0) ; le client ferme le sac, dont les objets restent.
pub fn on_pet_gone(st: &mut WorldState, owner: u32, iid: u32) {
    let Some(p) = st.by_cid.get(&owner).copied().and_then(|sid| st.players.get_mut(&sid)) else { return };
    if p.inventory.in_use.remove(&iid) {
        p.send_raw(use_time_packet(iid, owner, false));
        debug!("[sac du familier] {} : objet {iid} rappelé, sac fermé", p.ch.name);
    }
}

/// Renvoie au client la place réelle d'un objet (après un refus).
fn resync(st: &WorldState, sid: u32, iid: u32) {
    let p = &st.players[&sid];
    let cid = p.ch.id;
    if let Some(i) = p.inventory.items.get(&iid) {
        p.send_raw(item_update(i, cid, item_status::INVENTORY));
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

/// AGPMITEM op 2 vers le sac du familier (statut 13) ou d'un objet qui y est.
pub async fn on_item_update(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let status = p.int(1).unwrap_or(-1);
    if !available(ctx, st, sid) {
        debug!("[sac du familier] #{sid} objet {iid} vers le statut {status} : sac indisponible");
        refuse(st, sid, iid, "Le sac du familier n'est pas disponible : appelez d'abord votre familier.");
        return Ok(());
    }
    let in_sub = st.players[&sid].inventory.sub.contains_key(&iid);
    match (status, in_sub) {
        (item_status::SUB_INVENTORY, true) => move_in_bag(ctx, st, sid, iid, target_of(p.bytes(7))).await,
        (item_status::SUB_INVENTORY, false) => deposit(ctx, st, sid, iid, target_of(p.bytes(7))).await,
        (item_status::INVENTORY, true) => withdraw(ctx, st, sid, iid, super::equipment::wanted_grid(p)).await,
        (item_status::BANK, true) => to_bank(ctx, st, sid, iid, super::bank::grid_of(p.bytes(8))).await,
        _ => {
            debug!("[sac du familier] #{sid} statut {status} pour l'objet {iid} : ignoré");
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
async fn deposit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, target: Target) -> anyhow::Result<()> {
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
        if let Some(t) = p.inventory.sub.values().find(|i| i.grid == g).cloned() {
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
            let merged = p.inventory.sub.get_mut(&t.iid).unwrap();
            merged.count = total;
            let merged = merged.clone();
            p.send_raw(item_remove(iid, Some(cid)));
            p.send_raw(item_count_in(&merged, cid, item_status::SUB_INVENTORY));
            if from == From::Bank {
                crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "retrait", item.tid, item.iid, item.count);
            }
            debug!("[sac du familier] {} réunit deux piles de l'objet {} : {total}", p.ch.name, item.tid);
            return Ok(());
        }
    }
    let Some(g) = place(&p.inventory.sub, target) else {
        refuse(st, sid, iid, "Le sac du familier est plein.");
        return Ok(());
    };
    ctx.db
        .characters
        .exec("UPDATE character_items SET character_id = ?, status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (cid, stored_status::SUB_INVENTORY, g[0], g[1], g[2], iid))
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
    p.inventory.sub.insert(iid, stored.clone());
    p.send_raw(item_update(&stored, cid, item_status::SUB_INVENTORY));
    if from == From::Bank {
        crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "retrait", stored.tid, stored.iid, stored.count);
    }
    info!("[sac du familier] {} range {} × {} en {g:?} ({from:?})", p.ch.name, stored.count, label(ctx, stored.tid));
    if let From::Worn(part) = from {
        super::equipment::taken_off(ctx, st, sid, &stored, part);
    }
    Ok(())
}

/// Déplacement dans le sac : case libre, ou réunion avec une pile du même objet.
async fn move_in_bag(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, target: Target) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let item = p.inventory.sub[&iid].clone();
    if let Target::Cell(g) = target {
        if let Some(t) = p.inventory.sub.values().find(|i| i.grid == g && i.iid != iid).cloned() {
            if t.tid != item.tid || t.count + item.count > max_stack(ctx, item.tid) {
                resync(st, sid, iid);
                return Ok(());
            }
            let total = t.count + item.count;
            ctx.db
                .characters
                .atomic(vec![("UPDATE character_items SET count = ? WHERE id = ?", (total, t.iid).into()), ("DELETE FROM character_items WHERE id = ?", (iid,).into())])
                .await?;
            p.inventory.sub.remove(&iid);
            let merged = p.inventory.sub.get_mut(&t.iid).unwrap();
            merged.count = total;
            let merged = merged.clone();
            p.send_raw(item_remove(iid, Some(cid)));
            p.send_raw(item_count_in(&merged, cid, item_status::SUB_INVENTORY));
            return Ok(());
        }
    }
    let Some(g) = place(&p.inventory.sub, target) else {
        resync(st, sid, iid);
        return Ok(());
    };
    ctx.db.characters.exec("UPDATE character_items SET layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (g[0], g[1], g[2], iid)).await?;
    let moved = p.inventory.sub.get_mut(&iid).unwrap();
    moved.grid = g;
    let moved = moved.clone();
    p.send_raw(item_update(&moved, cid, item_status::SUB_INVENTORY));
    Ok(())
}

/// Retrait vers l'inventaire : case voulue (champ 7) si libre, réunion avec une pile du même objet, sinon première
/// case libre.
async fn withdraw(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let item = p.inventory.sub[&iid].clone();
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
        p.inventory.sub.remove(&iid);
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
    p.inventory.sub.remove(&iid);
    let taken = InventoryItem { grid: g, ..item };
    p.inventory.items.insert(iid, taken.clone());
    p.send_raw(item_update(&taken, cid, item_status::INVENTORY));
    info!("[sac du familier] {} sort {} × {} vers l'inventaire", p.ch.name, taken.count, label(ctx, taken.tid));
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
    let item = p.inventory.sub[&iid].clone();
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
        p.inventory.sub.remove(&iid);
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
    p.inventory.sub.remove(&iid);
    let stored = InventoryItem { grid: g, ..item };
    p.bank.items.insert(iid, stored.clone());
    p.send_raw(item_update(&stored, cid, item_status::BANK));
    crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "dépôt", stored.tid, stored.iid, stored.count);
    info!("[sac du familier] {} dépose en banque {} × {}", p.ch.name, stored.count, label(ctx, stored.tid));
    Ok(())
}

/// AGPMITEM op 17 d'une pile du sac : `n` exemplaires vers une case libre du sac (champ 7).
pub async fn split(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, n: i64, grid: Grid) -> anyhow::Result<()> {
    if !available(ctx, st, sid) {
        resync(st, sid, iid);
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    let cid = p.ch.id;
    let Some(item) = p.inventory.sub.get(&iid).cloned() else { return Ok(()) };
    if n <= 0 || n >= item.count || !super::items::is_free_in(&p.inventory.sub, SUB_INVENTORY, grid) {
        resync(st, sid, iid);
        return Ok(());
    }
    let left = item.count - n;
    let new_iid = ctx
        .db
        .characters
        .atomic(vec![
            ("UPDATE character_items SET count = ? WHERE id = ?", (left, iid).into()),
            (super::items::INSERT_ITEM, (cid, item.tid, stored_status::SUB_INVENTORY, grid[0], grid[1], grid[2], n).into()),
        ])
        .await? as u32;
    let part = InventoryItem { iid: new_iid, tid: item.tid, count: n, grid, conv: Default::default(), options: Vec::new(), durability: super::items::DURABILITY_FULL };
    p.inventory.sub.insert(new_iid, part.clone());
    let origin = p.inventory.sub.get_mut(&iid).unwrap();
    origin.count = left;
    let origin = origin.clone();
    p.send_raw(super::items::item_add(part.iid, part.tid, Some(cid), part.count, item_status::SUB_INVENTORY, Some(part.grid)));
    p.send_raw(item_count_in(&origin, cid, item_status::SUB_INVENTORY));
    debug!("[sac du familier] {} sépare {n} exemplaire(s) de l'objet {}", p.ch.name, item.tid);
    Ok(())
}

/// AGPMITEM op 30 : bouton « Auto Pick Up » de la fenêtre du sac ; valeur enregistrée puis renvoyée.
pub async fn on_auto_pick(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let on = p.bytes(20).and_then(|b| decode_mini(&mini::ITEM_AUTO_PICK, b).ok()).and_then(|f| f.int(0)).unwrap_or(1) != 0;
    let Some(player) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = player.ch.id;
    player.inventory.no_auto_pick = !on;
    ctx.db.characters.exec("UPDATE characters SET pet_auto_pick = ? WHERE id = ?", (i64::from(on), cid)).await?;
    player.send_raw(auto_pick_packet(iid, cid, on));
    info!("[sac du familier] {} : ramassage automatique {}", player.ch.name, if on { "activé" } else { "coupé" });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::items::{cash_mini, use_time_packet, DURABILITY_FULL};
    use archcore_net::decode_packet;

    fn item(iid: u32, grid: Grid) -> InventoryItem {
        InventoryItem { iid, tid: 190, count: 1, grid, conv: Default::default(), options: Vec::new(), durability: DURABILITY_FULL }
    }

    #[test]
    fn regle_du_sac() {
        assert!(bag_rule(7564, 6, 12, Some(1)), "Doogie <Rank 1> : créature 1362, PetType 1");
        assert!(!bag_rule(7497, 6, 12, Some(0)), "Test Ramda : créature 195 sans PetType");
        assert!(!bag_rule(7564, 6, 12, None), "créature inconnue");
        assert!(!bag_rule(50101, 3, 37, Some(1)), "talisman : pas un familier");
        assert!(!bag_rule(14821, 6, 12, Some(1)), "Yabbit : ajout d'Archonia, écarté");
        assert!(!bag_rule(15674, 6, 12, Some(1)) && bag_rule(15675, 6, 12, Some(1)));
    }

    #[test]
    fn case_visee() {
        let g = |b: i64, r: i64, c: i64| {
            // Sans les 4 octets de longueur, comme le rend la lecture du champ 7.
            let s = &mini::ITEM_GRID;
            build_mini(s, &Fields::new(s).with(0, b).with(1, r).with(2, c))[4..].to_vec()
        };
        assert_eq!(target_of(None), Target::Any);
        assert_eq!(target_of(Some(&g(1, 0, 3))), Target::Cell([1, 0, 3]));
        assert_eq!(target_of(Some(&g(2, -1, -1))), Target::Bag(2), "onglet « Bag 3 »");
        assert_eq!(target_of(Some(&g(-1, -1, -1))), Target::Any);
        let mut sub = BTreeMap::new();
        sub.insert(1, item(1, [0, 0, 0]));
        assert_eq!(place(&sub, Target::Any), Some([0, 0, 1]));
        assert_eq!(place(&sub, Target::Cell([0, 0, 0])), None, "case prise");
        assert_eq!(place(&sub, Target::Cell([0, 2, 0])), None, "hors du sac : 2 lignes");
        assert_eq!(place(&sub, Target::Bag(3)), Some([3, 0, 0]));
        assert_eq!(place(&sub, Target::Bag(4)), None, "4 sacs");
        for (i, g) in (0..2).flat_map(|r| (0..4).map(move |c| [1, r, c])).enumerate() {
            sub.insert(10 + i as u32, item(10 + i as u32, g));
        }
        assert_eq!(place(&sub, Target::Bag(1)), None, "sac 2 plein (2 × 4)");
    }

    #[test]
    fn paquets_cash_et_ramassage() {
        let m = decode_mini(&mini::ITEM_CASH, &cash_mini(true)[4..]).unwrap();
        assert_eq!((m.int(0), m.int(1), m.int(2)), (Some(1), None, None), "seul l'octet « en cours d'utilisation »");
        let p = decode_packet(&schema::ITEM, &use_time_packet(42, 7, false)).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(2), p.int(4)), (Some(26), None, Some(42), Some(7)));
        assert_eq!(decode_mini(&mini::ITEM_CASH, p.bytes(23).unwrap()).unwrap().int(0), Some(0));
        let a = decode_packet(&schema::ITEM, &auto_pick_packet(42, 7, true)).unwrap();
        assert_eq!((a.int(0), a.int(2), a.int(4)), (Some(30), Some(42), Some(7)));
        assert_eq!(decode_mini(&mini::ITEM_AUTO_PICK, a.bytes(20).unwrap()).unwrap().int(0), Some(1));
    }

    #[test]
    fn objets_de_l_inventaire_cash() {
        use crate::world::items::{item_add_of, Inventory};
        // Statut 51 : ni case (le client choisit la place), mais le mini Cash.
        let pet = InventoryItem { tid: 7564, ..item(5, [0, 3, 0]) };
        let add = decode_packet(&schema::ITEM, &item_add_of(&pet, 9, item_status::CASH_INVENTORY, Some(pet.grid))).unwrap();
        assert_eq!((add.int(0), add.int(1), add.int(3)), (Some(0), Some(51), Some(7564)));
        assert!(add.bytes(7).is_none(), "pas de case");
        assert_eq!(decode_mini(&mini::ITEM_CASH, add.bytes(23).unwrap()).unwrap().int(0), Some(0));
        let sub = decode_packet(&schema::ITEM, &item_add_of(&item(6, [2, 1, 3]), 9, item_status::SUB_INVENTORY, Some([2, 1, 3]))).unwrap();
        let g = decode_mini(&mini::ITEM_GRID, sub.bytes(7).unwrap()).unwrap();
        assert_eq!((sub.int(1), g.int(0), g.int(1), g.int(2)), (Some(13), Some(2), Some(1), Some(3)));
        assert!(sub.bytes(23).is_none());
        // Lecture : inventaire, Cash et sac ; la quantité part avec le statut de la grille.
        let mut inv = Inventory { character_id: 9, ..Default::default() };
        inv.items.insert(1, item(1, [0, 0, 0]));
        inv.cash.insert(5, pet.clone());
        inv.sub.insert(6, item(6, [2, 1, 3]));
        assert_eq!((inv.status_of(1), inv.status_of(5), inv.status_of(6), inv.status_of(7)), (Some(2), Some(51), Some(13), None));
        assert!(inv.get(5).is_none() && inv.get_usable(5).is_some() && inv.get_usable(6).is_some());
        assert_eq!(decode_packet(&schema::ITEM, &inv.count_packet(&pet, 9)).unwrap().int(1), Some(51));
        // Entrée en jeu : l'inventaire Cash part, le sac du familier non.
        let sent: Vec<i64> = inv.add_packets().iter().filter_map(|b| decode_packet(&schema::ITEM, b).ok()?.int(1)).collect();
        assert_eq!(sent, vec![2, 51]);
    }
}
