//! Banque : commune aux personnages d'un même compte (objets et argent), ouverte auprès d'un gardien.
//!
//! ```text
//!   AGPMEVENTBANK (0x36) : client → op 0 (événement du PNJ, fonction 20) ← op 1 (accord) : fenêtre ouverte.
//!   Objets : AGPMITEM op 2 avec le statut 100 (banque, case au champ 8) pour déposer ou ranger, le statut 2
//!            (inventaire, case au champ 7) pour retirer. Poser sur une pile du même objet les réunit.
//!   Argent : AGPMCHARACTER op 7, champ 9 = montant déposé (négatif : retiré) ← op 1, champs 8 et 9.
//!   À l'entrée en jeu, les objets de la banque partent avec l'inventaire (statut 100), l'argent au champ 9
//!   du personnage et les pages en plus de la première au champ 23.
//!   Page en plus : AGPMITEM op 22 (CID au champ 4), payée sur l'argent porté au prix de bankslotprice.txt
//!            (-1 : pas en vente contre de l'or) ← AGPMCHARACTER op 1 champs 8 et 23, message système 30
//!            (page ajoutée) ou 29 (banque déjà au maximum).
//! ```
//! En base : `account_bank` (argent, pages) ; les objets sont des lignes de `character_items` au statut 2,
//! rattachées au personnage qui les a déposés et passées à celui qui les retire.

use super::items::{item_remove, item_status, item_update, stored_status, Grid, InventoryItem};
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::event_function;
use super::{send_system, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::{info, warn};
use archcore_net::schema::{self, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};
use std::collections::BTreeMap;

/// Grille de la banque : 4 pages de 7 lignes × 4 colonnes ; la première est toujours ouverte.
pub const BANK_GRID: (i64, i64, i64) = (4, 7, 4);
/// Argent maximal en banque.
pub const MAX_BANK_MONEY: i64 = 2_000_000_000;
/// AGPMCHARACTER : opération « déplacer de l'argent vers la banque ».
pub const MOVE_BANK_MONEY: i64 = 7;

pub mod op {
    pub const REQUEST: i64 = 0;
    pub const GRANT: i64 = 1;
}

/// AGPMITEM : opération « acheter une page de banque ».
pub const REQUEST_BUY_BANK_SLOT: i64 = 22;

/// Codes AGPMSYSTEMMESSAGE de la banque.
pub mod message {
    pub const BANK_SLOT_IS_MAX: i64 = 29;
    pub const BANK_ADD_SLOT_SUCCESS: i64 = 30;
}

/// Prix des pages en plus (`bank_slot_price`) : indice 0 pour la deuxième page.
pub async fn slot_prices(db: &Databases) -> Vec<i64> {
    let Ok(rows) = db.world.rows("SELECT slot, price FROM bank_slot_price ORDER BY slot", ()).await else {
        warn!("prix des pages de banque non chargés : lancer `archcore db-init` puis `archcore import`");
        return Vec::new();
    };
    let mut prices = Vec::new();
    for r in &rows {
        let slot: usize = col(r, "slot");
        if slot >= 1 {
            prices.resize(prices.len().max(slot), -1);
            prices[slot - 1] = col(r, "price");
        }
    }
    prices
}

/// Prix de la page suivante pour un compte qui en a déjà `extra` en plus ; `None` si elle n'est pas en vente.
pub fn next_slot_price(prices: &[i64], extra: i64) -> Option<i64> {
    usize::try_from(extra).ok().and_then(|i| prices.get(i)).copied().filter(|p| *p >= 0)
}

fn system_message(st: &WorldState, sid: u32, code: i64) {
    if let Some(p) = st.players.get(&sid) {
        let s = &schema::SYSTEM_MESSAGE;
        p.send(s, Fields::new(s).with(0, 0).with(1, code));
    }
}

/// Banque d'un compte, chargée à l'entrée en jeu.
#[derive(Clone, Debug, Default)]
pub struct Bank {
    pub account_id: u32,
    pub money: u64,
    pub extra_slots: i64,
    pub items: BTreeMap<u32, InventoryItem>,
    /// Gardien auprès duquel la fenêtre est ouverte.
    pub keeper: Option<u32>,
}

impl Bank {
    /// Charge la banque d'un compte ; la crée au besoin, avec l'argent que ses personnages avaient en banque.
    pub async fn load(db: &Databases, account_id: u32) -> anyhow::Result<Self> {
        db.characters
            .exec(
                "INSERT IGNORE INTO account_bank (account_id, money) SELECT ?, COALESCE(SUM(bank_money), 0) FROM characters WHERE account_id = ?",
                (account_id, account_id),
            )
            .await?;
        let row = db.characters.first("SELECT money, extra_slots FROM account_bank WHERE account_id = ?", (account_id,)).await?;
        let (money, extra_slots) = row.map(|r| (col::<u64>(&r, "money"), col::<i64>(&r, "extra_slots"))).unwrap_or((0, 0));
        let rows = db
            .characters
            .rows(
                "SELECT i.id, i.template_id, i.layer, i.row_idx, i.col_idx, i.count, i.convert_level, i.socket_count, i.socket_tids \
                 FROM character_items i JOIN characters c ON c.id = i.character_id WHERE c.account_id = ? AND i.status = ?",
                (account_id, stored_status::BANK),
            )
            .await?;
        let items = rows
            .iter()
            .map(|r| {
                let iid: u32 = col(r, "id");
                (iid, InventoryItem { iid, tid: col(r, "template_id"), count: col(r, "count"), grid: [col(r, "layer"), col(r, "row_idx"), col(r, "col_idx")], conv: super::items::convert_of(r) })
            })
            .collect();
        Ok(Bank { account_id, money, extra_slots, items, keeper: None })
    }

    /// Pages ouvertes.
    pub fn layers(&self) -> i64 {
        (1 + self.extra_slots).clamp(1, BANK_GRID.0)
    }

    pub fn is_free(&self, g: Grid) -> bool {
        let (_, r, c) = BANK_GRID;
        g[0] >= 0 && g[0] < self.layers() && g[1] >= 0 && g[1] < r && g[2] >= 0 && g[2] < c && !self.items.values().any(|i| i.grid == g)
    }

    pub fn free_slot(&self) -> Option<Grid> {
        let (_, r, c) = BANK_GRID;
        (0..self.layers()).flat_map(|a| (0..r).flat_map(move |b| (0..c).map(move |d| [a, b, d]))).find(|g| self.is_free(*g))
    }

    pub fn at(&self, g: Grid) -> Option<&InventoryItem> {
        self.items.values().find(|i| i.grid == g)
    }

    /// Paquets AGPMITEM d'ajout des objets de la banque (entrée en jeu).
    pub fn add_packets(&self, owner: u32) -> Vec<Vec<u8>> {
        self.items
            .values()
            .flat_map(|i| std::iter::once(super::items::item_add(i.iid, i.tid, Some(owner), i.count, item_status::BANK, Some(i.grid))).chain(super::items::convert_add(i, owner)))
            .collect()
    }
}

/// AGPMEVENTBANK reçu : ouverture de la fenêtre auprès d'un gardien.
pub fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) {
    if p.int(0) != Some(op::REQUEST) {
        return;
    }
    let base = p.bytes(1).and_then(|b| decode_mini(&mini::EVENT_BASE, b).ok()).unwrap_or_else(|| Fields::new(&mini::EVENT_BASE));
    let Some(player) = st.players.get_mut(&sid) else { return };
    let Some((npc, _)) = resolve(&ctx.npcs, &base) else {
        warn!("[banque] #{sid} gardien inconnu");
        return;
    };
    if !npc.events.iter().any(|e| e.function == event_function::BANK) {
        warn!("[banque] #{sid} {} n'est pas un gardien de banque", npc.name);
        return;
    }
    if npc_distance(npc, player.ch.pos_x, player.ch.pos_z) > MAX_NPC_DISTANCE {
        warn!("[banque] #{sid} {} trop loin de {}", player.ch.name, npc.name);
        return;
    }
    player.bank.keeper = Some(npc.cid);
    info!("[banque] {} ouvre sa banque auprès de {}", player.ch.name, npc.name);
    let s = &schema::EVENT_BANK;
    player.send(s, Fields::new(s).with(0, op::GRANT).with(1, build_mini(&mini::EVENT_BASE, &base)).with(2, player.ch.id));
}

/// La fenêtre de la banque est ouverte et le gardien à portée.
fn at_keeper(ctx: &WorldCtx, st: &WorldState, sid: u32) -> bool {
    let Some(p) = st.players.get(&sid) else { return false };
    let Some(npc) = p.bank.keeper.and_then(|cid| ctx.npcs.get(cid)) else { return false };
    let pos = p.dest.unwrap_or_else(|| p.ch.pos());
    npc_distance(npc, pos.x as f64, pos.z as f64) <= MAX_NPC_DISTANCE * 2.0
}

/// Case demandée dans un mini ITEM_GRID ; une coordonnée négative (0xFFFF) laisse le serveur choisir.
pub fn grid_of(bytes: Option<&[u8]>) -> Option<Grid> {
    let g = decode_mini(&mini::ITEM_GRID, bytes?).ok()?;
    let v = [g.int(0)?, g.int(1)?, g.int(2)?];
    v.iter().all(|x| *x >= 0).then_some(v)
}

/// Remet l'objet à sa place chez le client après un refus.
fn resync(st: &WorldState, sid: u32, iid: u32) {
    let p = &st.players[&sid];
    if let Some(item) = p.inventory.get(iid) {
        p.send_raw(item_update(item, p.ch.id, item_status::INVENTORY));
    } else if let Some(item) = p.bank.items.get(&iid) {
        p.send_raw(item_update(item, p.ch.id, item_status::BANK));
    }
}

/// AGPMITEM op 2 vers la banque (statut 100) ou depuis la banque (objet en banque, statut 2).
pub async fn on_item_update(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let iid = p.int(2).unwrap_or(0) as u32;
    let status = p.int(1).unwrap_or(-1);
    if !at_keeper(ctx, st, sid) {
        send_system(st, sid, "Parlez à un gardien de banque pour utiliser votre banque.");
        resync(st, sid, iid);
        return Ok(());
    }
    let in_bank = st.players[&sid].bank.items.contains_key(&iid);
    match (status, in_bank) {
        (item_status::BANK, false) => deposit(ctx, st, sid, iid, grid_of(p.bytes(8))).await,
        (item_status::BANK, true) => move_in_bank(ctx, st, sid, iid, grid_of(p.bytes(8))).await,
        (item_status::INVENTORY, true) => withdraw(ctx, st, sid, iid, grid_of(p.bytes(7))).await,
        _ => Ok(()),
    }
}

fn max_stack(ctx: &WorldCtx, tid: u32) -> i64 {
    ctx.catalog.get(tid).map(|d| d.stack).unwrap_or(1)
}

async fn deposit(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let Some(item) = p.inventory.get(iid).cloned() else { return Ok(()) };
    let cid = p.ch.id;
    // Sur une pile du même objet : les réunir.
    if let Some(target) = wanted.and_then(|g| p.bank.at(g)).cloned() {
        if target.tid != item.tid || target.count + item.count > max_stack(ctx, item.tid) {
            resync(st, sid, iid);
            return Ok(());
        }
        let total = target.count + item.count;
        ctx.db
            .characters
            .atomic(vec![
                ("UPDATE character_items SET count = ? WHERE id = ?", (total, target.iid).into()),
                ("DELETE FROM character_items WHERE id = ? AND character_id = ?", (iid, cid).into()),
            ])
            .await?;
        p.inventory.items.remove(&iid);
        let merged = p.bank.items.get_mut(&target.iid).unwrap();
        merged.count = total;
        let merged = merged.clone();
        p.send_raw(item_remove(iid, Some(cid)));
        p.send_raw(item_update(&merged, cid, item_status::BANK));
        crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "dépôt", item.tid, item.iid, item.count);
        return Ok(());
    }
    let Some(grid) = wanted.filter(|g| p.bank.is_free(*g)).or_else(|| p.bank.free_slot()) else {
        send_system(st, sid, "Votre banque est pleine.");
        resync(st, sid, iid);
        return Ok(());
    };
    ctx.db
        .characters
        .exec("UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (stored_status::BANK, grid[0], grid[1], grid[2], iid))
        .await?;
    p.inventory.items.remove(&iid);
    let stored = InventoryItem { grid, ..item };
    p.bank.items.insert(iid, stored.clone());
    p.send_raw(item_update(&stored, cid, item_status::BANK));
    crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "dépôt", stored.tid, stored.iid, stored.count);
    info!("[banque] {} dépose {} × {}", p.ch.name, stored.count, ctx.catalog.get(stored.tid).map(|d| d.name.as_str()).unwrap_or("?"));
    Ok(())
}

async fn move_in_bank(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let Some(grid) = wanted.filter(|g| p.bank.is_free(*g)) else {
        resync(st, sid, iid);
        return Ok(());
    };
    ctx.db.characters.exec("UPDATE character_items SET layer = ?, row_idx = ?, col_idx = ? WHERE id = ?", (grid[0], grid[1], grid[2], iid)).await?;
    let item = p.bank.items.get_mut(&iid).unwrap();
    item.grid = grid;
    let item = item.clone();
    p.send_raw(item_update(&item, p.ch.id, item_status::BANK));
    Ok(())
}

async fn withdraw(ctx: &WorldCtx, st: &mut WorldState, sid: u32, iid: u32, wanted: Option<Grid>) -> anyhow::Result<()> {
    let p = st.players.get_mut(&sid).unwrap();
    let Some(item) = p.bank.items.get(&iid).cloned() else { return Ok(()) };
    let (cid, character_id) = (p.ch.id, p.ch.id);
    if let Some(target) = wanted.and_then(|g| p.inventory.at(g)).cloned() {
        if target.tid != item.tid || target.count + item.count > max_stack(ctx, item.tid) {
            resync(st, sid, iid);
            return Ok(());
        }
        let total = target.count + item.count;
        ctx.db
            .characters
            .atomic(vec![
                ("UPDATE character_items SET count = ? WHERE id = ?", (total, target.iid).into()),
                ("DELETE FROM character_items WHERE id = ?", (iid,).into()),
            ])
            .await?;
        p.bank.items.remove(&iid);
        let merged = p.inventory.items.get_mut(&target.iid).unwrap();
        merged.count = total;
        let merged = merged.clone();
        p.send_raw(item_remove(iid, Some(cid)));
        p.send_raw(item_update(&merged, cid, item_status::INVENTORY));
        crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "retrait", item.tid, item.iid, item.count);
        return Ok(());
    }
    let Some(grid) = wanted.filter(|g| p.inventory.is_free(*g)).or_else(|| p.inventory.free_slot()) else {
        send_system(st, sid, "Votre inventaire est plein.");
        resync(st, sid, iid);
        return Ok(());
    };
    ctx.db
        .characters
        .exec(
            "UPDATE character_items SET character_id = ?, status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?",
            (character_id, stored_status::INVENTORY, grid[0], grid[1], grid[2], iid),
        )
        .await?;
    p.bank.items.remove(&iid);
    let taken = InventoryItem { grid, ..item };
    p.inventory.items.insert(iid, taken.clone());
    p.send_raw(item_update(&taken, cid, item_status::INVENTORY));
    crate::journal::bank_item(&ctx.db, p.bank.account_id, (cid, &p.ch.name), "retrait", taken.tid, taken.iid, taken.count);
    info!("[banque] {} retire {} × {}", p.ch.name, taken.count, ctx.catalog.get(taken.tid).map(|d| d.name.as_str()).unwrap_or("?"));
    Ok(())
}

/// AGPMITEM op 22 : achat d'une page de banque, payée sur l'argent porté.
pub async fn buy_slot(ctx: &WorldCtx, st: &mut WorldState, sid: u32) -> anyhow::Result<()> {
    if !at_keeper(ctx, st, sid) {
        send_system(st, sid, "Parlez à un gardien de banque pour agrandir votre banque.");
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    let extra = p.bank.extra_slots.max(0);
    if extra + 1 >= BANK_GRID.0 {
        system_message(st, sid, message::BANK_SLOT_IS_MAX);
        return Ok(());
    }
    let Some(price) = next_slot_price(&ctx.bank_slot_prices, extra) else {
        send_system(st, sid, "Cette page de banque ne s'achète pas contre de l'or.");
        return Ok(());
    };
    let money = p.ch.money as i64;
    if price > money {
        send_system(st, sid, &format!("Il faut {price} pièce(s) d'or pour une page de banque en plus."));
        return Ok(());
    }
    let left = money - price;
    let (cid, account_id) = (p.ch.id, p.bank.account_id);
    ctx.db
        .characters
        .atomic(vec![
            (super::SAVE_MONEY, (left, cid).into()),
            ("UPDATE account_bank SET extra_slots = ? WHERE account_id = ?", (extra + 1, account_id).into()),
        ])
        .await?;
    p.ch.money = left as u64;
    p.bank.extra_slots = extra + 1;
    p.ch.bank_size = (extra + 1) as i32;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, cid).with(8, left).with(23, extra + 1));
    crate::journal::bank_page(&ctx.db, account_id, (cid, &p.ch.name), extra + 2, price);
    info!("[banque] {} achète la page {} de sa banque pour {price} pièce(s)", p.ch.name, extra + 2);
    system_message(st, sid, message::BANK_ADD_SLOT_SUCCESS);
    Ok(())
}

/// AGPMCHARACTER op 7 : `amount` pièces vers la banque (négatif : vers l'inventaire).
pub async fn move_money(ctx: &WorldCtx, st: &mut WorldState, sid: u32, amount: i64) -> anyhow::Result<()> {
    if !at_keeper(ctx, st, sid) {
        send_system(st, sid, "Parlez à un gardien de banque pour utiliser votre banque.");
        return Ok(());
    }
    let p = st.players.get_mut(&sid).unwrap();
    let money = p.ch.money as i64 - amount;
    let bank = p.bank.money as i64 + amount;
    if amount == 0 || !(0..=super::npc_trade::MAX_MONEY).contains(&money) || !(0..=MAX_BANK_MONEY).contains(&bank) {
        return Ok(());
    }
    ctx.db
        .characters
        .atomic(vec![
            (super::SAVE_MONEY, (money, p.ch.id).into()),
            ("UPDATE account_bank SET money = ? WHERE account_id = ?", (bank, p.bank.account_id).into()),
        ])
        .await?;
    p.ch.money = money as u64;
    p.bank.money = bank as u64;
    p.ch.bank_money = bank as u64;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(8, money).with(9, bank));
    crate::journal::bank_money(&ctx.db, p.bank.account_id, (p.ch.id, &p.ch.name), amount);
    info!("[banque] {} {} {} pièce(s) : {money} sur lui, {bank} en banque", p.ch.name, if amount > 0 { "dépose" } else { "retire" }, amount.abs());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grille_de_la_banque() {
        let mut bank = Bank::default();
        assert_eq!(bank.layers(), 1);
        assert_eq!(bank.free_slot(), Some([0, 0, 0]));
        assert!(!bank.is_free([1, 0, 0]), "page fermée");
        bank.extra_slots = 9;
        assert_eq!(bank.layers(), 4);
        assert!(bank.is_free([3, 6, 3]) && !bank.is_free([0, 7, 0]));
        let prices = [1_000_000, -1, -1];
        assert_eq!(next_slot_price(&prices, 0), Some(1_000_000));
        assert_eq!(next_slot_price(&prices, 1), None, "pas en vente contre de l'or");
        assert_eq!(next_slot_price(&prices, 3), None);
        for r in 0..7 {
            for c in 0..4 {
                bank.items.insert((r * 4 + c) as u32 + 1, InventoryItem { iid: (r * 4 + c) as u32 + 1, tid: 1, count: 1, grid: [0, r, c], conv: Default::default() });
            }
        }
        assert_eq!(bank.free_slot(), Some([1, 0, 0]));
        assert_eq!(bank.at([0, 2, 3]).map(|i| i.iid), Some(12));
    }
}
