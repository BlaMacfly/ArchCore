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
//! L'achat paie en plus la taxe de la région du joueur (`tax.rs`), qui va à la recette de la région ; la vente n'est pas
//! taxée. `/merchant` (objet Merchant, `consumables.rs`) ouvre à distance la boutique du marchand le plus proche, valable
//! tant que le joueur reste à portée de l'endroit où il l'a ouverte.
//!
//! Boutique des Shrines (épisode 8) : un objet peut coûter en plus de l'or des points de charisme (« NPC Charisma
//! Price ») et des Shrine Coins (« NPC Shrine Coin », objets TID 53000, toutes les piles de l'inventaire comptées), le
//! tout multiplié par la quantité. Le client n'envoie aucun prix (fenêtre `UI_SHRINEITEM_BUY_CONFIRM` d'Archonia, qui
//! s'ouvre à tout achat) : le serveur relit le modèle. Contrôles dans l'ordre : charisme (résultat 10,
//! « NPCTMSG_NotEnoughCharisma »), pièces (11, « NPCTMSG_NotEnoughShrineCoin »), or (3), place (4) ; puis le charisme,
//! les pièces, l'or et l'objet sont enregistrés ensemble. Ni la taxe ni le doublement du meurtrier ne portent sur le
//! charisme et les pièces (choix d'ArchCore). Les monnaies d'Archonia (Archonium Coin, Siege Token, Bloodmark, résultats
//! 12 à 14) sont ignorées. Aucune liste de vente des données ne propose de tels objets (le vendeur d'origine n'est pas
//! connu) : le mécanisme sert dès qu'une boutique en contient.

use super::items::{alloc_shop_iid, item_add, item_count, item_remove, item_status, SHOP_GRID};
use super::npc_events::{in_range, range, resolve};
use super::npcs::event_function;
use super::{clamp_money, show_money, OpenShop, ShopItem, WorldCtx, WorldState, SAVE_MONEY};
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
    /// Meurtrier de palier 2 ou plus : « Alert_NPCTrade_MurdererRejectBuyItem ».
    pub const NO_SELL_TO_MURDERER: i64 = 9;
    /// Boutique des Shrines : charisme ou Shrine Coins insuffisants (rappels 10 et 11 d'AgcmEventNPCTrade).
    pub const BUY_NOT_ENOUGH_CHARISMA: i64 = 10;
    pub const BUY_NOT_ENOUGH_SHRINE_COIN: i64 = 11;
}

/// Argent maximal dans l'inventaire.
pub const MAX_MONEY: i64 = 1_000_000_000;

/// Shrine Coin (« Arena Coin » dans Archonia) : objet d'ItemType 2 et de Type 6, monnaie de la boutique des Shrines.
pub const SHRINE_COIN: u32 = 53000;

/// Ce que coûte un achat : or (taxe comprise), points de charisme, Shrine Coins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cost {
    pub gold: i64,
    pub charisma: i64,
    pub coins: i64,
}

impl Cost {
    /// Charisme et pièces de `count` exemplaires de l'objet ; l'or (`gold`, déjà taxé) est donné.
    pub fn of(data: &super::items::ItemData, count: i64, gold: i64) -> Self {
        Cost { gold, charisma: data.charisma_price.max(0) * count, coins: data.coin_price.max(0) * count }
    }

    /// Le joueur peut payer, sinon le résultat du refus : charisme (10), pièces (11), or (3), dans cet ordre.
    pub fn afford(&self, charisma: i64, coins: i64, money: i64) -> Result<(), i64> {
        if self.charisma > charisma {
            return Err(trade_result::BUY_NOT_ENOUGH_CHARISMA);
        }
        if self.coins > coins {
            return Err(trade_result::BUY_NOT_ENOUGH_SHRINE_COIN);
        }
        if self.gold > money {
            return Err(trade_result::BUY_NOT_ENOUGH_MONEY);
        }
        Ok(())
    }
}

/// Piles de Shrine Coins à entamer pour en payer `need` : (IID, quantité restante, 0 : pile retirée), dans l'ordre des
/// piles données. `None` si elles n'y suffisent pas.
pub fn coin_plan(piles: &[(u32, i64)], need: i64) -> Option<Vec<(u32, i64)>> {
    let mut left = need;
    let mut plan = Vec::new();
    for &(iid, count) in piles {
        if left <= 0 {
            break;
        }
        let taken = count.min(left);
        left -= taken;
        plan.push((iid, count - taken));
    }
    (left <= 0).then_some(plan)
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    // Sous métamorphose (sauf l'ArchLord) : pas de boutique, comme aucun dialogue de PNJ (morph.rs).
    if !st.players.contains_key(&sid) || super::morph::restricted(st, sid) {
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
    if !in_range(npc, player, range::NPC_TRADE) {
        warn!("[boutique] #{sid} {} trop loin de {}", player.ch.name, npc.name);
        return;
    }
    send_shop(ctx, st, sid, npc, event, &base, None);
}

/// Ouvre à distance (`/merchant`, `consumables.rs`) la boutique du marchand `npc` : la même liste qu'auprès de lui,
/// valable tant que le joueur reste à portée de l'endroit où il l'a ouverte.
pub fn open_remote(ctx: &WorldCtx, st: &mut WorldState, sid: u32, npc: &super::npcs::Npc) {
    let Some(event) = npc.events.iter().find(|e| e.function == event_function::NPC_TRADE) else { return };
    let Some(player) = st.players.get(&sid) else { return };
    let at = super::presence::position_now(player, std::time::Instant::now());
    let base = Fields::new(&mini::EVENT_BASE).with(0, 3).with(1, npc.cid).with(2, event.event_id).with(3, event_function::NPC_TRADE);
    send_shop(ctx, st, sid, npc, event, &base, Some((at.x as f64, at.z as f64)));
}

/// Envoie la liste de la boutique de `npc` ; `remote_at` : ouverte à distance depuis cette position.
fn send_shop(ctx: &WorldCtx, st: &mut WorldState, sid: u32, npc: &super::npcs::Npc, event: &super::npcs::NpcEvent, base: &Fields, remote_at: Option<(f64, f64)>) {
    let template: i64 = event.param("Template").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    // Une boutique déjà ouverte est retirée du client avant d'en envoyer une nouvelle copie.
    close(st, sid);
    let player = st.players.get_mut(&sid).unwrap();
    let event_base = build_mini(&mini::EVENT_BASE, base);
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
    player.shop = Some(OpenShop { npc_cid: npc.cid, items, remote_at });
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
    let near = match shop.remote_at {
        Some(at) => super::npc_events::near_point(p, at, range::NPC_TRADE),
        None => in_range(npc, p, range::NPC_TRADE),
    };
    near.then(|| (npc.cid, npc.name.clone()))
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
    // Meurtrier : prix doublé dès le palier 1 (« ConfirmMsg_BuyNPCTradeItemMurderer ») ; au palier 2, les
    // objets de genre 0x20 ne se vendent plus.
    let rogue = super::pvp::murderer_level(st.players[&sid].ch.murderer);
    if rogue >= 2 && data.kind & 0x20 != 0 {
        result(st, sid, trade_op::BUY, trade_result::NO_SELL_TO_MURDERER);
        return Ok(());
    }
    // Taxe de la région du joueur en plus (tax.rs), versée à la recette de la région.
    let taxed = super::tax::quote(ctx, st, sid, data.npc_price * count * if rogue >= 1 { 2 } else { 1 });
    let price = taxed.total;
    let cost = Cost::of(data, count, price);
    let player = &st.players[&sid];
    let money = player.ch.money as i64;
    // Shrine Coins : toutes les piles de l'inventaire, comme les compte le client.
    let piles: Vec<(u32, i64)> = player.inventory.items.values().filter(|i| i.tid == SHRINE_COIN).map(|i| (i.iid, i.count)).collect();
    let coins_held: i64 = piles.iter().map(|(_, n)| n).sum();
    if let Err(code) = cost.afford(player.ch.charisma as i64, coins_held, money) {
        result(st, sid, trade_op::BUY, code);
        return Ok(());
    }
    let wanted = match (p.int(7), p.int(8), p.int(9)) {
        (Some(l), Some(r), Some(c)) => Some([l, r, c]),
        _ => None,
    };
    let player = st.players.get_mut(&sid).unwrap();
    let Some(place) = player.inventory.placement(entry.tid, count, data.stack, wanted) else {
        result(st, sid, trade_op::BUY, trade_result::BUY_FULL_INVENTORY);
        return Ok(());
    };
    let cid = player.ch.id;
    let coins = coin_plan(&piles, cost.coins).unwrap_or_default();
    // L'objet et le paiement (or, charisme, pièces) sont enregistrés ensemble ; l'insertion en dernier, pour son IID.
    let left = clamp_money(money - price);
    let charisma_left = (player.ch.charisma as i64 - cost.charisma).max(0) as u32;
    let mut stmts: Vec<(&str, mysql_async::Params)> = vec![(SAVE_MONEY, (left, cid).into())];
    if cost.charisma > 0 {
        stmts.push(("UPDATE characters SET charisma = ? WHERE id = ?", (charisma_left, cid).into()));
    }
    for &(iid, n) in &coins {
        stmts.push(if n > 0 {
            ("UPDATE character_items SET count = ? WHERE id = ? AND character_id = ?", (n, iid, cid).into())
        } else {
            ("DELETE FROM character_items WHERE id = ? AND character_id = ?", (iid, cid).into())
        });
    }
    stmts.push(player.inventory.placement_sql(entry.tid, count, place));
    let new_iid = ctx.db.characters.atomic(stmts).await? as u32;
    for &(iid, n) in &coins {
        if n > 0 {
            let pile = player.inventory.items.get_mut(&iid).map(|i| {
                i.count = n;
                i.clone()
            });
            if let Some(pile) = pile {
                player.send_raw(item_count(&pile, cid));
            }
        } else {
            player.inventory.items.remove(&iid);
            player.send_raw(item_remove(iid, Some(cid)));
        }
    }
    if cost.charisma > 0 {
        player.ch.charisma = charisma_left;
        player.factors.status[4] = charisma_left as i32;
        super::skills::send_points(player);
    }
    let (item, created) = player.inventory.apply_placement(entry.tid, count, place, new_iid);
    player.send_raw(if created {
        item_add(item.iid, item.tid, Some(cid), item.count, item_status::INVENTORY, Some(item.grid))
    } else {
        item_count(&item, cid)
    });
    show_money(player, left);
    let name = player.ch.name.clone();
    result(st, sid, trade_op::BUY, trade_result::BUY_SUCCEEDED);
    crate::journal::shop(&ctx.db, (cid, &name), &npc_name, "achat", data.tid, item.iid, count as i64, price as i64);
    let extra = match (cost.charisma, cost.coins) {
        (0, 0) => String::new(),
        (c, s) => format!(", {c} point(s) de charisme et {s} Shrine Coin(s)"),
    };
    info!(
        "[boutique] {name} achète {count} × {} (TID {}) pour {price}{extra} à {npc_name}{}",
        data.name,
        data.tid,
        if taxed.tax > 0 { format!(" (dont {} de taxe)", taxed.tax) } else { String::new() }
    );
    super::tax::collect(ctx, st, &taxed).await?;
    Ok(())
}

async fn sell(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((_, npc_name)) = active_shop(ctx, st, sid) else {
        warn!("[boutique] #{sid} vente sans boutique ouverte à portée");
        return Ok(());
    };
    let iid = p.int(3).unwrap_or(0) as u32;
    // Un objet proposé à l'échange ne se vend pas.
    // Un objet de quête (« QuestItem ») ne se vend pas non plus.
    let item = st.players[&sid].inventory.get(iid).cloned().filter(|i| !super::trade::is_offered(st, sid, iid) && !ctx.catalog.quest_items.contains_key(&i.tid));
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
    let cid = player.ch.id;
    // L'objet part et le paiement arrive ensemble.
    let total = clamp_money(money + price);
    ctx.db
        .characters
        .atomic(vec![("DELETE FROM character_items WHERE id = ? AND character_id = ?", (item.iid, cid).into()), (SAVE_MONEY, (total, cid).into())])
        .await?;
    player.inventory.items.remove(&item.iid);
    player.send_raw(item_remove(item.iid, Some(cid)));
    show_money(player, total);
    let name = player.ch.name.clone();
    result(st, sid, trade_op::SELL, trade_result::SELL_SUCCEEDED);
    crate::journal::shop(&ctx.db, (cid, &name), &npc_name, "vente", data.tid, item.iid, item.count, price);
    info!("[boutique] {name} vend {} × {} (TID {}) pour {price} à {npc_name}", item.count, data.name, data.tid);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::items::{ItemCatalog, ItemData, ShopEntry};

    fn item(tid: u32, gold: i64, charisma: i64, coins: i64) -> ItemData {
        ItemData {
            tid,
            name: format!("objet {tid}"),
            npc_price: gold,
            pc_price: 0,
            charisma_price: charisma,
            coin_price: coins,
            stack: 99,
            equip: None,
            potion: None,
            scroll: None,
            stone: None,
            usable_type: 0,
            option_tids: Vec::new(),
            kind: 0,
            bound_type: 0,
            cash: 0,
            enable_on_ride: false,
            auction_category: (0, 0),
            event_item: false,
        }
    }

    #[test]
    fn boutique_des_shrines_fictive() {
        // Boutique fictive (modèle 900) : Elixir of Cold-hearted Spirit (2 000 charisme), Letter of Great Pardon
        // (75 pièces), et un objet aux trois prix.
        let mut c = ItemCatalog::default();
        for d in [item(50139, 0, 2000, 0), item(1836, 0, 0, 75), item(7000, 100, 10, 5)] {
            c.items.insert(d.tid, d);
        }
        c.shops.insert(900, vec![ShopEntry { tid: 50139, count: 1 }, ShopEntry { tid: 1836, count: 1 }, ShopEntry { tid: 7000, count: 1 }]);
        let cost = |tid: u32, n: i64| {
            let d = c.get(tid).unwrap();
            Cost::of(d, n, d.npc_price * n)
        };
        assert_eq!(c.shop(900).len(), 3);
        // Quantité : tous les prix multipliés.
        assert_eq!(cost(7000, 3), Cost { gold: 300, charisma: 30, coins: 15 });
        assert_eq!(cost(50139, 1).afford(1999, 0, 0), Err(trade_result::BUY_NOT_ENOUGH_CHARISMA));
        assert_eq!(cost(50139, 1).afford(2000, 0, 0), Ok(()));
        assert_eq!(cost(1836, 2).afford(0, 149, 0), Err(trade_result::BUY_NOT_ENOUGH_SHRINE_COIN));
        assert_eq!(cost(1836, 2).afford(0, 150, 0), Ok(()));
        // Ordre des contrôles : charisme, pièces, or.
        assert_eq!(cost(7000, 1).afford(0, 0, 0), Err(trade_result::BUY_NOT_ENOUGH_CHARISMA));
        assert_eq!(cost(7000, 1).afford(10, 0, 0), Err(trade_result::BUY_NOT_ENOUGH_SHRINE_COIN));
        assert_eq!(cost(7000, 1).afford(10, 5, 99), Err(trade_result::BUY_NOT_ENOUGH_MONEY));
        assert_eq!(cost(7000, 1).afford(10, 5, 100), Ok(()));
        // Pièces prises pile après pile ; 0 : pile retirée.
        assert_eq!(coin_plan(&[(1, 40), (2, 60)], 75), Some(vec![(1, 0), (2, 25)]));
        assert_eq!(coin_plan(&[(1, 40)], 40), Some(vec![(1, 0)]));
        assert_eq!(coin_plan(&[(1, 40), (2, 60)], 0), Some(vec![]));
        assert_eq!(coin_plan(&[(1, 40)], 41), None);
    }

    #[test]
    fn grille_de_la_boutique() {
        assert_eq!(super::shop_grid(0), [0, 0, 0]);
        assert_eq!(super::shop_grid(1), [0, 0, 1]);
        assert_eq!(super::shop_grid(5), [0, 1, 1]);
        assert_eq!(super::shop_grid(80), [1, 0, 0]);
    }
}
