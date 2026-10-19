//! Échange entre joueurs (AGPMPRIVATETRADE, 0x07).
//!
//! ```text
//!   A → op 1 (champ 2 = CID de B)        B ← op 3 (A demande) ; tous deux passent au statut d'action 7
//!   B → op 4 (accepte) / op 25 (refuse)  A, B ← op 6 (début, champ 1 = soi, 2 = l'autre) / op 5 (fin)
//!   → op 9 (IID au champ 3) : objet mis dans la grille d'échange (1 × 3 × 4) : soi ← AGPMITEM op 2 statut 6,
//!     l'autre ← AGPMITEM op 0 statut 6 (propriétaire = soi) ; op 10 le reprend
//!   → op 23 (montant au champ 5) : soi ← op 23, l'autre ← op 24
//!   → op 16 verrouille (soi ← 16, l'autre ← 17 ; tous deux verrouillés ← 20) ; op 18 déverrouille (18 / 19)
//!   → op 21 prêt (soi ← 21, l'autre ← 22) ; tous deux prêts : échange des objets et de l'argent, op 0
//!     (résultat, champ 8 = 8) à chacun, statut d'action 0
//!   → op 5 annule : objets rendus, op 5 aux deux
//! ```
//! Les objets restent dans l'inventaire de leur propriétaire (et en base) jusqu'à l'échange : rien n'est
//! perdu si un joueur se déconnecte, l'échange est alors annulé.

use super::combat;
use super::items::{item_add, item_remove, item_status, item_update, stored_status, Grid};
use super::{send_system, WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema;
use archcore_net::{build_packet, Fields, Packet};
use std::collections::BTreeMap;

/// Grille d'échange : 1 page de 3 lignes × 4 colonnes.
pub const TRADE_GRID: (i64, i64, i64) = (1, 3, 4);
/// AGPMCHARACTER champ 11 : statut d'action « échange ».
const ACTION_STATUS_TRADE: i64 = 7;
/// Distance maximale entre les deux joueurs pour commencer un échange.
const MAX_TRADE_DISTANCE: f64 = 1500.0;
/// Résultat envoyé à la fin d'un échange réussi.
const RESULT_DONE: i64 = 8;

pub mod op {
    pub const RESULT: i64 = 0;
    pub const REQUEST: i64 = 1;
    pub const REQUEST_CONFIRM: i64 = 3;
    pub const CONFIRM: i64 = 4;
    pub const CANCEL: i64 = 5;
    pub const START: i64 = 6;
    pub const ADD: i64 = 9;
    pub const REMOVE: i64 = 10;
    pub const LOCK: i64 = 16;
    pub const TARGET_LOCKED: i64 = 17;
    pub const UNLOCK: i64 = 18;
    pub const TARGET_UNLOCKED: i64 = 19;
    pub const ACTIVE_READY: i64 = 20;
    pub const READY: i64 = 21;
    pub const TARGET_READY: i64 = 22;
    pub const UPDATE_MONEY: i64 = 23;
    pub const TARGET_UPDATE_MONEY: i64 = 24;
    pub const REFUSE: i64 = 25;
}

/// Échange en cours, côté d'un joueur.
#[derive(Clone, Debug, Default)]
pub struct Trade {
    /// Session de l'autre joueur.
    pub peer: u32,
    /// Demande pas encore acceptée.
    pub awaiting: bool,
    pub locked: bool,
    pub ready: bool,
    pub gold: i64,
    /// Objets proposés : IID → case de la grille d'échange.
    pub items: BTreeMap<u32, Grid>,
}

impl Trade {
    pub fn free_slot(&self) -> Option<Grid> {
        let (l, r, c) = TRADE_GRID;
        (0..l).flat_map(|a| (0..r).flat_map(move |b| (0..c).map(move |d| [a, b, d]))).find(|g| !self.items.values().any(|x| x == g))
    }
}

/// Un objet proposé à l'échange ne peut être ni déplacé, ni utilisé, ni détruit.
pub fn is_offered(st: &WorldState, sid: u32, iid: u32) -> bool {
    st.players.get(&sid).and_then(|p| p.trade.as_ref()).is_some_and(|t| t.items.contains_key(&iid))
}

fn packet(op: i64, cid: Option<u32>, target: Option<u32>) -> Fields {
    let s = &schema::PRIVATE_TRADE;
    Fields::new(s).with(0, op).with_opt(1, cid).with_opt(2, target)
}

fn send(st: &WorldState, sid: u32, f: Fields) {
    if let Some(p) = st.players.get(&sid) {
        p.send(&schema::PRIVATE_TRADE, f);
    }
}

/// Statut d'action (0 normal, 7 échange) diffusé autour du joueur.
fn action_status(st: &WorldState, sid: u32, status: i64) {
    let Some(p) = st.players.get(&sid) else { return };
    let c = &schema::CHARACTER;
    combat::send_around(st, sid, &build_packet(c, &Fields::new(c).with(0, 1).with(1, p.ch.id).with(11, status)));
}

/// AGPMPRIVATETRADE reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.get(&sid).is_some_and(|p| p.in_game) {
        return Ok(());
    }
    match p.int(0).unwrap_or(-1) {
        op::REQUEST => request(st, sid, p.int(2).unwrap_or(0) as u32),
        op::CONFIRM => confirm(st, sid),
        op::CANCEL | op::REFUSE => cancel(st, sid),
        op::ADD => add(ctx, st, sid, p.int(3).unwrap_or(0) as u32),
        op::REMOVE => take_back(st, sid, p.int(3).unwrap_or(0) as u32),
        op::UPDATE_MONEY => money(st, sid, p.int(5).unwrap_or(0)),
        op::LOCK => lock(st, sid),
        op::UNLOCK => unlock(st, sid),
        op::READY => ready(ctx, st, sid).await?,
        other => warn!("[échange] #{sid} op {other} inattendu"),
    }
    Ok(())
}

/// Les deux côtés d'un échange en cours et accepté : (moi, l'autre).
fn sides(st: &WorldState, sid: u32) -> Option<(Trade, Trade)> {
    let mine = st.players.get(&sid)?.trade.clone()?;
    let theirs = st.players.get(&mine.peer)?.trade.clone()?;
    (theirs.peer == sid && !mine.awaiting).then_some((mine, theirs))
}

fn request(st: &mut WorldState, sid: u32, target_cid: u32) {
    let Some(target) = st.by_cid.get(&target_cid).copied() else { return cancel(st, sid) };
    let (me, other) = (&st.players[&sid], st.players.get(&target));
    let Some(other) = other else { return cancel(st, sid) };
    let near = combat::dist2d(me.ch.pos(), other.ch.pos()) <= MAX_TRADE_DISTANCE;
    if target == sid || me.trade.is_some() || other.trade.is_some() || me.dead || other.dead || !other.in_game || !near {
        info!("[échange] demande de {} à {} refusée (déjà occupé, mort ou trop loin)", me.ch.name, other.ch.name);
        return cancel(st, sid);
    }
    let (cid, names) = (me.ch.id, (me.ch.name.clone(), other.ch.name.clone()));
    for (a, b) in [(sid, target), (target, sid)] {
        st.players.get_mut(&a).unwrap().trade = Some(Trade { peer: b, awaiting: true, ..Default::default() });
        action_status(st, a, ACTION_STATUS_TRADE);
    }
    send(st, target, packet(op::REQUEST_CONFIRM, Some(cid), Some(target_cid)));
    info!("[échange] {} propose un échange à {}", names.0, names.1);
}

fn confirm(st: &mut WorldState, sid: u32) {
    let Some(peer) = st.players[&sid].trade.as_ref().filter(|t| t.awaiting).map(|t| t.peer) else { return cancel(st, sid) };
    for s in [sid, peer] {
        if let Some(t) = st.players.get_mut(&s).and_then(|p| p.trade.as_mut()) {
            t.awaiting = false;
        }
    }
    let (a, b) = (st.players[&sid].ch.id, st.players.get(&peer).map(|p| p.ch.id).unwrap_or(0));
    send(st, sid, packet(op::START, Some(a), Some(b)));
    send(st, peer, packet(op::START, Some(b), Some(a)));
    info!("[échange] début entre {} et {}", st.players[&sid].ch.name, st.players.get(&peer).map(|p| p.ch.name.as_str()).unwrap_or("?"));
}

/// Annule l'échange des deux côtés : objets rendus à l'inventaire, op 5, statut d'action normal.
pub fn cancel(st: &mut WorldState, sid: u32) {
    let peer = st.players.get(&sid).and_then(|p| p.trade.as_ref()).map(|t| t.peer);
    end_side(st, sid, peer);
    if let Some(peer) = peer {
        end_side(st, peer, Some(sid));
    }
}

fn end_side(st: &mut WorldState, sid: u32, peer: Option<u32>) {
    let Some(p) = st.players.get_mut(&sid) else { return };
    let Some(trade) = p.trade.take() else {
        p.send(&schema::PRIVATE_TRADE, packet(op::CANCEL, Some(p.ch.id), None));
        return;
    };
    let cid = p.ch.id;
    for iid in trade.items.keys() {
        if let Some(item) = st.players[&sid].inventory.get(*iid) {
            st.players[&sid].send_raw(item_update(item, cid, item_status::INVENTORY));
        }
        if let Some(peer) = peer.and_then(|s| st.players.get(&s)) {
            peer.send_raw(item_remove(*iid, Some(cid)));
        }
    }
    send(st, sid, packet(op::CANCEL, Some(cid), None));
    action_status(st, sid, 0);
}

fn add(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32) {
    let Some((mine, _)) = sides(st, sid) else { return cancel(st, sid) };
    let p = &st.players[&sid];
    let Some(item) = p.inventory.get(iid).cloned() else { return };
    if mine.locked || mine.items.contains_key(&iid) {
        return;
    }
    let Some(grid) = mine.free_slot() else {
        send_system(st, sid, "La grille d'échange est pleine.");
        return;
    };
    let cid = p.ch.id;
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().items.insert(iid, grid);
    let shown = super::items::InventoryItem { grid, ..item.clone() };
    st.players[&sid].send_raw(item_update(&shown, cid, item_status::TRADE_GRID));
    if let Some(peer) = st.players.get(&mine.peer) {
        peer.send_raw(item_add(item.iid, item.tid, Some(cid), item.count, item_status::TRADE_GRID, Some(grid)));
    }
    let name = ctx.catalog.get(item.tid).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[échange] {} propose {} × {name}", st.players[&sid].ch.name, item.count);
}

fn take_back(st: &mut WorldState, sid: u32, iid: u32) {
    let Some((mine, _)) = sides(st, sid) else { return cancel(st, sid) };
    if mine.locked || !mine.items.contains_key(&iid) {
        return;
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().items.remove(&iid);
    let p = &st.players[&sid];
    if let Some(item) = p.inventory.get(iid) {
        p.send_raw(item_update(item, p.ch.id, item_status::INVENTORY));
    }
    if let Some(peer) = st.players.get(&mine.peer) {
        peer.send_raw(item_remove(iid, Some(p.ch.id)));
    }
}

fn money(st: &mut WorldState, sid: u32, amount: i64) {
    let Some((mine, _)) = sides(st, sid) else { return cancel(st, sid) };
    if mine.locked {
        return;
    }
    let amount = amount.clamp(0, st.players[&sid].ch.money as i64);
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().gold = amount;
    let s = &schema::PRIVATE_TRADE;
    send(st, sid, Fields::new(s).with(0, op::UPDATE_MONEY).with(5, amount));
    send(st, mine.peer, Fields::new(s).with(0, op::TARGET_UPDATE_MONEY).with(5, amount));
}

fn lock(st: &mut WorldState, sid: u32) {
    let Some((mine, theirs)) = sides(st, sid) else { return cancel(st, sid) };
    if mine.locked {
        return;
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().locked = true;
    send(st, sid, packet(op::LOCK, None, None));
    send(st, mine.peer, packet(op::TARGET_LOCKED, None, None));
    if theirs.locked {
        send(st, sid, packet(op::ACTIVE_READY, None, None));
        send(st, mine.peer, packet(op::ACTIVE_READY, None, None));
    }
}

fn unlock(st: &mut WorldState, sid: u32) {
    let Some((mine, theirs)) = sides(st, sid) else { return cancel(st, sid) };
    if !mine.locked || mine.ready || theirs.ready {
        return;
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().locked = false;
    send(st, sid, packet(op::UNLOCK, None, None));
    send(st, mine.peer, packet(op::TARGET_UNLOCKED, None, None));
}

async fn ready(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    let Some((mine, theirs)) = sides(st, sid) else {
        cancel(st, sid);
        return Ok(());
    };
    if !mine.locked || !theirs.locked || mine.ready {
        return Ok(());
    }
    st.players.get_mut(&sid).unwrap().trade.as_mut().unwrap().ready = true;
    send(st, sid, packet(op::READY, None, None));
    send(st, mine.peer, packet(op::TARGET_READY, None, None));
    if theirs.ready {
        execute(ctx, st, sid, mine.peer).await?;
    }
    Ok(())
}

/// Ce qui empêche l'échange, côté `a` qui reçoit ce que `b` propose.
fn check(st: &WorldState, a: u32, b: u32) -> Option<&'static str> {
    let (pa, pb) = (&st.players[&a], &st.players[&b]);
    let (ta, tb) = (pa.trade.as_ref()?, pb.trade.as_ref()?);
    if pa.dead || pb.dead {
        return Some("un joueur est mort");
    }
    if (pa.ch.money as i64) < ta.gold || pa.ch.money as i64 - ta.gold + tb.gold > super::npc_trade::MAX_MONEY {
        return Some("argent");
    }
    if ta.items.keys().chain(tb.items.keys()).any(|iid| pa.inventory.get(*iid).is_none() && pb.inventory.get(*iid).is_none()) {
        return Some("objet disparu");
    }
    if pa.inventory.free_count() + ta.items.len() < tb.items.len() {
        return Some("inventaire plein");
    }
    None
}

async fn execute(ctx: &WorldCtx, st: &mut WorldState, a: u32, b: u32) -> anyhow::Result<()> {
    if let Some(why) = check(st, a, b).or_else(|| check(st, b, a)) {
        info!("[échange] annulé : {why}");
        send_system(st, a, &format!("Échange annulé : {why}."));
        send_system(st, b, &format!("Échange annulé : {why}."));
        cancel(st, a);
        return Ok(());
    }
    let (ta, tb) = (st.players.get_mut(&a).unwrap().trade.take().unwrap(), st.players.get_mut(&b).unwrap().trade.take().unwrap());
    // Les objets quittent d'abord les deux inventaires, pour libérer leurs cases.
    let mut outgoing: Vec<(u32, u32, super::items::InventoryItem)> = Vec::new();
    for (from, to, trade) in [(a, b, &ta), (b, a, &tb)] {
        let p = st.players.get_mut(&from).unwrap();
        for iid in trade.items.keys() {
            if let Some(item) = p.inventory.items.remove(iid) {
                outgoing.push((from, to, item));
            }
        }
    }
    for (from, to, item) in outgoing {
        let from_cid = st.players[&from].ch.id;
        let receiver = st.players.get_mut(&to).unwrap();
        let grid = receiver.inventory.free_slot().unwrap_or([0, 0, 0]);
        let to_cid = receiver.ch.id;
        ctx.db
            .characters
            .exec(
                "UPDATE character_items SET character_id = ?, status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?",
                (to_cid, stored_status::INVENTORY, grid[0], grid[1], grid[2], item.iid),
            )
            .await?;
        let moved = super::items::InventoryItem { grid, ..item };
        receiver.inventory.items.insert(moved.iid, moved.clone());
        receiver.send_raw(item_remove(moved.iid, Some(from_cid)));
        receiver.send_raw(item_add(moved.iid, moved.tid, Some(to_cid), moved.count, item_status::INVENTORY, Some(moved.grid)));
        st.players[&from].send_raw(item_remove(moved.iid, Some(from_cid)));
    }
    for (me, mine, theirs) in [(a, &ta, &tb), (b, &tb, &ta)] {
        let p = st.players.get_mut(&me).unwrap();
        let money = p.ch.money as i64 - mine.gold + theirs.gold;
        p.ch.money = money as u64;
        ctx.db.characters.exec("UPDATE characters SET money = ? WHERE id = ?", (money, p.ch.id)).await?;
        let c = &schema::CHARACTER;
        p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(8, money));
        let s = &schema::PRIVATE_TRADE;
        p.send(s, Fields::new(s).with(0, op::RESULT).with(1, p.ch.id).with(8, RESULT_DONE));
        action_status(st, me, 0);
    }
    info!(
        "[échange] {} ↔ {} : {} objet(s) et {} or contre {} objet(s) et {} or",
        st.players[&a].ch.name,
        st.players[&b].ch.name,
        ta.items.len(),
        ta.gold,
        tb.items.len(),
        tb.gold
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grille_d_echange() {
        let mut t = Trade::default();
        for i in 0..12u32 {
            let g = t.free_slot().unwrap();
            t.items.insert(i, g);
        }
        assert_eq!(t.items[&11], [0, 2, 3]);
        assert!(t.free_slot().is_none(), "12 cases");
    }
}
