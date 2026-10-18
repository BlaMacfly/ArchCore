//! Boutiques des PNJ (AgcmEventNPCTrade, 0x32).
//!
//! ```text
//!   client → op 0 (événement au champ 0) : demande la liste. Le serveur envoie pour chaque objet un
//!            AGPMITEM op 0 (statut 10, propriétaire = PNJ, case de la boutique), puis un 0x32 op 0 avec
//!            son IID et son détail (prix unitaire, quantité, case) ; un 0x32 op 1 termine la liste.
//!   client → op 2 : achat de l'objet d'IID donné (quantité au champ 2 du détail, case visée aux champs 7-9)
//!            ← op 2, résultat 2 (réussi), 3 (pas assez d'argent), 4 (inventaire plein)…
//!   client → op 3 : vente de l'objet d'inventaire d'IID donné ← op 3, résultat 0 (réussi), 1 (trop d'argent)
//! ```

use super::items::{alloc_shop_iid, item_add, item_count, item_remove, item_status, SHOP_GRID};
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::event_function;
use super::{set_money, OpenShop, ShopItem, WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};

pub mod trade_op {
    pub const ITEM: i64 = 0;
    pub const LIST_END: i64 = 1;
    pub const BUY: i64 = 2;
    pub const SELL: i64 = 3;
}

pub mod trade_result {
    pub const SELL_SUCCEEDED: i64 = 0;
    pub const SELL_EXCEED_MONEY: i64 = 1;
    pub const BUY_SUCCEEDED: i64 = 2;
    pub const BUY_NOT_ENOUGH_MONEY: i64 = 3;
    pub const BUY_FULL_INVENTORY: i64 = 4;
    pub const INVALID_ITEM: i64 = 5;
    pub const INVALID_COUNT: i64 = 6;
}

/// Argent maximal dans l'inventaire.
pub const MAX_MONEY: i64 = 1_000_000_000;

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    if !st.players.contains_key(&sid) {
        return Ok(());
    }
    match p.int(1).unwrap_or(-1) {
        trade_op::ITEM => open(ctx, st, sid, p),
        trade_op::BUY => buy(ctx, st, sid, p).await?,
        trade_op::SELL => sell(ctx, st, sid, p).await?,
        op => warn!("[boutique] #{sid} op {op} inattendu"),
    }
    Ok(())
}

/// Grille de la boutique pour le n-ième objet.
pub fn shop_grid(slot: i64) -> [i64; 3] {
    let (_, rows, cols) = SHOP_GRID;
    let per_layer = rows * cols;
    [slot / per_layer, (slot % per_layer) / cols, slot % cols]
}

fn open(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    let base = p.bytes(0).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let found = resolve(&ctx.npcs, &base);
    let npc = found.map(|(n, _)| n);
    let event = npc.and_then(|n| n.events.iter().find(|e| e.function == event_function::NPC_TRADE));
    let (Some(npc), Some(event)) = (npc, event) else {
        warn!(
            "[boutique] #{sid} boutique inconnue (source {}, événement {})",
            base.int(1).map(|v| v.to_string()).unwrap_or("-".into()),
            base.int(2).map(|v| v.to_string()).unwrap_or("-".into())
        );
        return;
    };
    let player = st.players.get_mut(&sid).unwrap();
    if npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > MAX_NPC_DISTANCE {
        warn!("[boutique] #{sid} {} trop loin de {}", player.ch.name, npc.name);
        return;
    }
    let template: i64 = event.param("Template").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    // Une boutique déjà ouverte est retirée du client avant d'en envoyer une nouvelle copie.
    close(st, sid);
    let player = st.players.get_mut(&sid).unwrap();
    let event_base = build_mini(&mini::EVENT_BASE, &base);
    let mut items = Vec::new();
    for entry in ctx.catalog.shop(template) {
        let Some(data) = ctx.catalog.get(entry.tid) else { continue };
        let grid = shop_grid(items.len() as i64);
        if grid[0] >= SHOP_GRID.0 {
            break;
        }
        let item = ShopItem { iid: alloc_shop_iid(), tid: entry.tid, count: entry.count, price: data.npc_price, grid };
        // Propriétaire = le PNJ : le client rattache l'objet à ce personnage.
        player.send_raw(item_add(item.iid, item.tid, Some(npc.cid), item.count, item_status::NPC_TRADE, Some(grid)));
        let d = &mini::NPC_TRADE_DETAIL;
        let detail = build_mini(
            d,
            &Fields::new(d).with(0, item.tid).with(1, item.price).with(2, item.count).with(3, item_status::NPC_TRADE).with(4, grid[0]).with(5, grid[1]).with(6, grid[2]),
        );
        let s = &schema::EVENT_NPC_TRADE;
        player.send(s, Fields::new(s).with(0, event_base.clone()).with(1, trade_op::ITEM).with(3, item.iid).with(4, detail));
        items.push(item);
    }
    let s = &schema::EVENT_NPC_TRADE;
    player.send(s, Fields::new(s).with(1, trade_op::LIST_END).with(2, player.ch.id));
    let v = |i: usize| base.int(i).map(|x| x.to_string()).unwrap_or("-".into());
    info!(
        "[boutique] {} ouvre la boutique de {} (modèle {template}, {} objet(s), demande : source {}/{}, événement {}, fonction {})",
        player.ch.name, npc.name, items.len(), v(0), v(1), v(2), v(3)
    );
    player.shop = Some(OpenShop { npc_cid: npc.cid, items });
}

/// Retire du client les objets de la dernière boutique ouverte.
pub fn close(st: &mut WorldState, sid: u32) {
    let Some(player) = st.players.get_mut(&sid) else { return };
    let Some(shop) = player.shop.take() else { return };
    for item in &shop.items {
        player.send_raw(item_remove(item.iid, Some(shop.npc_cid)));
    }
}

fn result(st: &WorldState, sid: u32, op: i64, res: i64) {
    let p = &st.players[&sid];
    let s = &schema::EVENT_NPC_TRADE;
    p.send(s, Fields::new(s).with(1, op).with(2, p.ch.id).with(6, res));
}

/// Boutique ouverte et PNJ encore à portée : (CID du PNJ, nom du PNJ).
fn active_shop(ctx: &WorldCtx, st: &WorldState, sid: u32) -> Option<(u32, String)> {
    let p = st.players.get(&sid)?;
    let shop = p.shop.as_ref()?;
    let npc = ctx.npcs.get(shop.npc_cid)?;
    (npc_distance(npc, p.ch.pos_x, p.ch.pos_z) <= MAX_NPC_DISTANCE).then(|| (npc.cid, npc.name.clone()))
}

async fn buy(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, npc_name)) = active_shop(ctx, st, sid) else {
        warn!("[boutique] #{sid} achat sans boutique ouverte à portée");
        return Ok(());
    };
    let iid = p.int(3).unwrap_or(0) as u32;
    let entry = st.players[&sid].shop.as_ref().and_then(|s| s.items.iter().find(|i| i.iid == iid).cloned());
    let Some((entry, data)) = entry.and_then(|e| ctx.catalog.get(e.tid).map(|d| (e, d))) else {
        result(st, sid, trade_op::BUY, trade_result::INVALID_ITEM);
        return Ok(());
    };
    let detail = p.bytes(4).and_then(|b| decode_mini(&mini::NPC_TRADE_DETAIL, b).ok());
    let count = detail.and_then(|d| d.int(2)).filter(|&c| c > 0).unwrap_or(entry.count);
    if count < 1 || count > data.stack {
        result(st, sid, trade_op::BUY, trade_result::INVALID_COUNT);
        return Ok(());
    }
    let price = data.npc_price * count;
    let money = st.players[&sid].ch.money as i64;
    if price > money {
        result(st, sid, trade_op::BUY, trade_result::BUY_NOT_ENOUGH_MONEY);
        return Ok(());
    }
    let wanted = match (p.int(7), p.int(8), p.int(9)) {
        (Some(l), Some(r), Some(c)) => Some([l, r, c]),
        _ => None,
    };
    let player = st.players.get_mut(&sid).unwrap();
    let Some((item, created)) = player.inventory.add(&ctx.db, entry.tid, count, data.stack, wanted).await? else {
        result(st, sid, trade_op::BUY, trade_result::BUY_FULL_INVENTORY);
        return Ok(());
    };
    let cid = player.ch.id;
    player.send_raw(if created {
        item_add(item.iid, item.tid, Some(cid), item.count, item_status::INVENTORY, Some(item.grid))
    } else {
        item_count(&item, cid)
    });
    let name = player.ch.name.clone();
    set_money(ctx, st, sid, money - price).await?;
    result(st, sid, trade_op::BUY, trade_result::BUY_SUCCEEDED);
    info!("[boutique] {name} achète {count} × {} (TID {}) pour {price} à {npc_name}", data.name, data.tid);
    Ok(())
}

async fn sell(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, npc_name)) = active_shop(ctx, st, sid) else {
        warn!("[boutique] #{sid} vente sans boutique ouverte à portée");
        return Ok(());
    };
    let iid = p.int(3).unwrap_or(0) as u32;
    let item = st.players[&sid].inventory.get(iid).cloned();
    let Some((item, data)) = item.and_then(|i| ctx.catalog.get(i.tid).map(|d| (i, d))) else {
        result(st, sid, trade_op::SELL, trade_result::INVALID_ITEM);
        return Ok(());
    };
    let price = data.pc_price * item.count;
    let money = st.players[&sid].ch.money as i64;
    if money + price > MAX_MONEY {
        result(st, sid, trade_op::SELL, trade_result::SELL_EXCEED_MONEY);
        return Ok(());
    }
    let player = st.players.get_mut(&sid).unwrap();
    player.inventory.remove(&ctx.db, item.iid).await?;
    let cid = player.ch.id;
    player.send_raw(item_remove(item.iid, Some(cid)));
    let name = player.ch.name.clone();
    set_money(ctx, st, sid, money + price).await?;
    result(st, sid, trade_op::SELL, trade_result::SELL_SUCCEEDED);
    info!("[boutique] {name} vend {} × {} (TID {}) pour {price} à {npc_name}", item.count, data.name, data.tid);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn grille_de_la_boutique() {
        assert_eq!(super::shop_grid(0), [0, 0, 0]);
        assert_eq!(super::shop_grid(1), [0, 0, 1]);
        assert_eq!(super::shop_grid(5), [0, 1, 1]);
        assert_eq!(super::shop_grid(80), [1, 0, 0]);
    }
}
