//! Entrepôt de guilde : AgpmGuildWarehouse (0x54) et AgpmEventGuild ops 2 et 3.
//!
//! Relevé dans Archonia (AgpmGuildWarehouse FUN_140309ad0, réception FUN_140309e50 ; AgcmUIGuildWarehouse
//! FUN_1405fca70 et suivantes ; AgcmEventGuild FUN_1406a6740) :
//!
//! ```text
//!   Gardien de banque à fonction 35 (bouton sysstr 611) : AgpmEventGuild op 2 (EVENT_BASE, CID) ← op 3
//!   (accord) : le client envoie alors l'op 1.
//!   Paquet 0x54, drapeaux 2 : 0 op, 1 CID, 2 argent (INT64), 3 pages en plus, 4 IID, 5 page, 6 ligne,
//!   7 colonne, 8 résultat.
//!   → 1 ouverture        ← objets (AGPMITEM op 0, statut 53, case au champ 7), op 0 (argent, pages), op 1
//!                          (résultat 2 : la fenêtre s'ouvre, FUN_1405fc6a0)
//!   → 2 fermeture (le joueur s'éloigne de plus de 149 unités, FUN_1405fc830)
//!   → 3 dépôt, 4 retrait d'argent (champ 2)          ← op 0 (nouvel argent) ; argent du personnage
//!   → 5 objet déposé : 4 IID, 5 page, 6 ligne, 7 colonne (objet du sac, statut 2)
//!   → 6 objet retiré : 3 statut visé (2), 4 IID, 5 à 7 case     ← AGPMITEM op 2 (statut 53 ou 2)
//!   → 7 agrandissement (FUN_1405fb920 : 999 999 999 999 pièces, jamais assez) ← op 7, résultat 4
//! ```
//! Grille : 4 pages de 7 lignes × 4 colonnes (`CTL_GRID_GWARE`), la première ouverte. Droits (FUN_14030b830,
//! masques de AgpdGuildWarehouse) : le chef a tout (0x1f), les autres membres déposent objets et argent (5).
//! 10 membres au moins (AgpmGuild code 0x5f) ; une guilde dont l'entrepôt n'est pas vide ne peut pas être
//! dissoute (code 0x60). En base : `guild_warehouse` (argent, pages), objets dans `character_items` au statut
//! enregistré 3 avec `guild_id` (le personnage qui l'a déposé reste noté).

use super::{mine, rank, send_to, system_packet, Guild};
use crate::db::{col, Databases};
use crate::info;
use crate::world::items::{convert_add, item_add_of, item_remove, item_update, stored_status, Grid, InventoryItem};
use crate::world::{send_system, WorldCtx, WorldState};
use archcore_net::schema::{self};
use archcore_net::{build_packet, Fields, Packet};
use std::collections::BTreeMap;

pub mod op {
    pub const INFO: i64 = 0;
    pub const OPEN: i64 = 1;
    pub const CLOSE: i64 = 2;
    pub const MONEY_IN: i64 = 3;
    pub const MONEY_OUT: i64 = 4;
    pub const ITEM_IN: i64 = 5;
    pub const ITEM_OUT: i64 = 6;
    pub const EXPAND: i64 = 7;
}

/// Droits (bits) : déposer, retirer un objet, déposer, retirer de l'argent, agrandir.
pub mod right {
    pub const ITEM_IN: i64 = 0x01;
    pub const ITEM_OUT: i64 = 0x02;
    pub const MONEY_IN: i64 = 0x04;
    pub const MONEY_OUT: i64 = 0x08;
    pub const EXPAND: i64 = 0x10;
    pub const MASTER: i64 = 0x1f;
    pub const MEMBER: i64 = 0x05;
}

/// Statut d'un objet de l'entrepôt dans AGPMITEM.
pub const ITEM_STATUS: i64 = 53;
/// Grille : pages, lignes, colonnes.
pub const GRID: (i64, i64, i64) = (4, 7, 4);
pub const MIN_MEMBERS: usize = 10;
/// Prix d'une page en plus selon Archonia (aucun personnage ne porte autant d'or).
pub const EXPAND_PRICE: i64 = 999_999_999_999;
/// Résultats (champ 8) : fenêtre ouverte ; agrandissement faute d'or.
pub const RESULT_OPEN: i64 = 2;
pub const RESULT_NOT_ENOUGH_MONEY: i64 = 4;
/// Codes AgpmGuild : effectif insuffisant, dissolution refusée.
pub const STORE_MEMBER_FAIL: i64 = 0x5f;
pub const DESTROY_KEEP_FAIL: i64 = 0x60;
const MAX_MONEY: i64 = 2_000_000_000;

/// Entrepôt d'une guilde, chargé à la première ouverture.
#[derive(Clone, Debug, Default)]
pub struct Warehouse {
    pub loaded: bool,
    pub money: i64,
    pub slots: i64,
    pub items: BTreeMap<u32, InventoryItem>,
    /// Joueurs (sessions) qui ont la fenêtre ouverte.
    pub viewers: Vec<u32>,
}

impl Warehouse {
    pub fn layers(&self) -> i64 {
        (1 + self.slots).clamp(1, GRID.0)
    }

    pub fn is_free(&self, g: Grid) -> bool {
        let (_, r, c) = GRID;
        g[0] >= 0 && g[0] < self.layers() && g[1] >= 0 && g[1] < r && g[2] >= 0 && g[2] < c && !self.items.values().any(|i| i.grid == g)
    }

    pub fn free_slot(&self) -> Option<Grid> {
        let (_, r, c) = GRID;
        (0..self.layers()).flat_map(|a| (0..r).flat_map(move |b| (0..c).map(move |d| [a, b, d]))).find(|g| self.is_free(*g))
    }

    pub fn is_empty(&self) -> bool {
        self.money == 0 && self.items.is_empty()
    }
}

async fn load(db: &Databases, gid: u32) -> anyhow::Result<Warehouse> {
    let row = db.characters.first("SELECT money, slots FROM guild_warehouse WHERE guild_id = ?", (gid,)).await?;
    let (money, slots) = row.map(|r| (col::<i64>(&r, "money"), col::<i64>(&r, "slots"))).unwrap_or((0, 0));
    let rows = db
        .characters
        .rows(
            "SELECT id, template_id, layer, row_idx, col_idx, count, convert_level, socket_count, socket_tids, option_tids, durability \
             FROM character_items WHERE guild_id = ? AND status = ?",
            (gid, stored_status::GUILD_WAREHOUSE),
        )
        .await?;
    let items = rows
        .iter()
        .map(|r| {
            let iid: u32 = col(r, "id");
            (iid, InventoryItem { iid, tid: col(r, "template_id"), count: col(r, "count"), grid: [col(r, "layer"), col(r, "row_idx"), col(r, "col_idx")], conv: crate::world::items::convert_of(r), options: crate::world::items::options_of(r), durability: crate::world::items::durability_of(r) })
        })
        .collect();
    Ok(Warehouse { loaded: true, money, slots, items, viewers: Vec::new() })
}

/// Charge l'entrepôt de la guilde au besoin (aussi pour vérifier qu'il est vide avant une dissolution).
pub async fn ensure_loaded(ctx: &WorldCtx, st: &mut WorldState, gid: u32) -> anyhow::Result<()> {
    if st.guilds.by_id.get(&gid).is_some_and(|g| !g.warehouse.loaded) {
        let w = load(&ctx.db, gid).await?;
        if let Some(g) = st.guilds.by_id.get_mut(&gid) {
            g.warehouse = w;
        }
    }
    Ok(())
}

fn packet(o: i64, cid: u32, f: impl FnOnce(Fields) -> Fields) -> Vec<u8> {
    let s = &schema::GUILD_WAREHOUSE;
    build_packet(s, &f(Fields::new(s).with(0, o).with(1, cid)))
}

fn rights(g: &Guild, cid: u32) -> i64 {
    if g.members.get(&cid).is_some_and(|m| m.rank == rank::MASTER) {
        right::MASTER
    } else {
        right::MEMBER
    }
}

/// Op 0 (argent, pages) aux joueurs qui ont la fenêtre ouverte.
fn send_info(st: &WorldState, g: &Guild) {
    for &sid in &g.warehouse.viewers {
        if let Some(p) = st.players.get(&sid) {
            p.send_raw(packet(op::INFO, p.ch.id, |f| f.with(2, g.warehouse.money).with(3, g.warehouse.slots)));
        }
    }
}

/// Autres joueurs qui regardent l'entrepôt de `g` (encore dans la guilde).
fn other_viewers(st: &WorldState, g: &Guild, sid: u32) -> Vec<u32> {
    g.warehouse.viewers.iter().copied().filter(|v| *v != sid && st.players.get(v).is_some_and(|p| g.members.contains_key(&p.ch.id))).collect()
}

pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some((cid, gid, _)) = mine(st, sid) else { return Ok(()) };
    match p.int(0) {
        Some(op::OPEN) => open(ctx, st, sid, cid, gid).await?,
        Some(op::CLOSE) => {
            if let Some(g) = st.guilds.by_id.get_mut(&gid) {
                g.warehouse.viewers.retain(|v| *v != sid);
            }
        }
        Some(o @ (op::MONEY_IN | op::MONEY_OUT)) => money(ctx, st, sid, cid, gid, o == op::MONEY_IN, p.int(2).unwrap_or(0)).await?,
        Some(op::ITEM_IN) => {
            let grid = [p.int(5).unwrap_or(-1), p.int(6).unwrap_or(-1), p.int(7).unwrap_or(-1)];
            item_in(ctx, st, sid, cid, gid, p.int(4).unwrap_or(0) as u32, grid).await?
        }
        Some(op::ITEM_OUT) => {
            let grid = [p.int(5).unwrap_or(-1), p.int(6).unwrap_or(-1), p.int(7).unwrap_or(-1)];
            item_out(ctx, st, sid, cid, gid, p.int(4).unwrap_or(0) as u32, grid).await?
        }
        Some(op::EXPAND) => {
            // Archonia demande 999 999 999 999 pièces (FUN_1405fb920) : le résultat 4 affiche GWARE_NOT_ENOUGH_MONEY.
            let g = &st.guilds.by_id[&gid];
            if rights(g, cid) & right::EXPAND != 0 && (st.players[&sid].ch.money as i64) < EXPAND_PRICE {
                send_to(st, sid, packet(op::EXPAND, cid, |f| f.with(8, RESULT_NOT_ENOUGH_MONEY)));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Op 1 : objets, argent, puis le résultat qui ouvre la fenêtre.
async fn open(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, gid: u32) -> anyhow::Result<()> {
    if !st.guilds.warehouse_keepers.contains_key(&sid) {
        return Ok(());
    }
    if st.guilds.by_id[&gid].active_members() < MIN_MEMBERS {
        send_to(st, sid, system_packet(STORE_MEMBER_FAIL, None, None));
        return Ok(());
    }
    ensure_loaded(ctx, st, gid).await?;
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    if !g.warehouse.viewers.contains(&sid) {
        g.warehouse.viewers.push(sid);
    }
    let g = &st.guilds.by_id[&gid];
    let p = &st.players[&sid];
    for i in g.warehouse.items.values() {
        p.send_raw(item_add_of(i, cid, ITEM_STATUS, Some(i.grid)));
        if let Some(c) = convert_add(i, cid) {
            p.send_raw(c);
        }
    }
    p.send_raw(packet(op::INFO, cid, |f| f.with(2, g.warehouse.money).with(3, g.warehouse.slots)));
    p.send_raw(packet(op::OPEN, cid, |f| f.with(8, RESULT_OPEN)));
    info!("[guilde] {} ouvre l'entrepôt de {} ({} objet(s), {} pièce(s))", p.ch.name, g.name, g.warehouse.items.len(), g.warehouse.money);
    Ok(())
}

fn viewing(st: &WorldState, gid: u32, sid: u32) -> bool {
    st.guilds.by_id.get(&gid).is_some_and(|g| g.warehouse.loaded && g.warehouse.viewers.contains(&sid))
}

/// Ops 3 et 4 : argent déposé (tout membre) ou retiré (le chef).
async fn money(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, gid: u32, deposit: bool, amount: i64) -> anyhow::Result<()> {
    if !viewing(st, gid, sid) || amount <= 0 {
        return Ok(());
    }
    let g = &st.guilds.by_id[&gid];
    if rights(g, cid) & if deposit { right::MONEY_IN } else { right::MONEY_OUT } == 0 {
        return Ok(());
    }
    let p = &st.players[&sid];
    let delta = if deposit { amount } else { -amount };
    let (carried, stored) = (p.ch.money as i64 - delta, g.warehouse.money + delta);
    if !(0..=crate::world::npc_trade::MAX_MONEY).contains(&carried) || !(0..=MAX_MONEY).contains(&stored) {
        return Ok(());
    }
    ctx.db
        .characters
        .atomic(vec![
            (crate::world::SAVE_MONEY, (carried, cid).into()),
            ("INSERT INTO guild_warehouse (guild_id, money) VALUES (?, ?) ON DUPLICATE KEY UPDATE money = VALUES(money)", (gid, stored).into()),
        ])
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.ch.money = carried as u64;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, cid).with(8, carried));
    let name = p.ch.name.clone();
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    g.warehouse.money = stored;
    let g = &st.guilds.by_id[&gid];
    send_info(st, g);
    info!("[guilde] {name} {} {amount} pièce(s) : {stored} dans l'entrepôt de {}", if deposit { "dépose" } else { "retire" }, g.name);
    Ok(())
}

/// Op 5 : objet du sac déposé dans la case demandée (ou la première libre).
async fn item_in(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, gid: u32, iid: u32, wanted: Grid) -> anyhow::Result<()> {
    if !viewing(st, gid, sid) || rights(&st.guilds.by_id[&gid], cid) & right::ITEM_IN == 0 {
        return Ok(());
    }
    // Un objet proposé à l'échange ne se dépose pas.
    if super::super::trade::is_offered(st, sid, iid) {
        return Ok(());
    }
    let Some(item) = st.players[&sid].inventory.get(iid).cloned() else { return Ok(()) };
    let w = &st.guilds.by_id[&gid].warehouse;
    let Some(grid) = Some(wanted).filter(|g| w.is_free(*g)).or_else(|| w.free_slot()) else {
        send_system(st, sid, "L'entrepôt de guilde est plein.");
        return Ok(());
    };
    ctx.db
        .characters
        .exec(
            "UPDATE character_items SET status = ?, guild_id = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ? AND character_id = ?",
            (stored_status::GUILD_WAREHOUSE, gid, grid[0], grid[1], grid[2], iid, cid),
        )
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.inventory.items.remove(&iid);
    let stored = InventoryItem { grid, ..item };
    p.send_raw(item_update(&stored, cid, ITEM_STATUS));
    let name = p.ch.name.clone();
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    g.warehouse.items.insert(iid, stored.clone());
    let g = &st.guilds.by_id[&gid];
    for v in other_viewers(st, g, sid) {
        let pv = &st.players[&v];
        pv.send_raw(item_add_of(&stored, pv.ch.id, ITEM_STATUS, Some(grid)));
    }
    info!("[guilde] {name} dépose {} × {} dans l'entrepôt de {}", stored.count, ctx.catalog.get(stored.tid).map(|d| d.name.as_str()).unwrap_or("?"), g.name);
    Ok(())
}

/// Op 6 : objet de l'entrepôt repris dans le sac (le chef).
async fn item_out(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cid: u32, gid: u32, iid: u32, wanted: Grid) -> anyhow::Result<()> {
    if !viewing(st, gid, sid) {
        return Ok(());
    }
    let g = &st.guilds.by_id[&gid];
    let Some(item) = g.warehouse.items.get(&iid).cloned() else { return Ok(()) };
    if rights(g, cid) & right::ITEM_OUT == 0 {
        // Remis à sa place chez le client.
        send_to(st, sid, item_update(&item, cid, ITEM_STATUS));
        return Ok(());
    }
    let inv = &st.players[&sid].inventory;
    let Some(grid) = Some(wanted).filter(|g| inv.is_free(*g)).or_else(|| inv.free_slot()) else {
        send_system(st, sid, "Votre inventaire est plein.");
        send_to(st, sid, item_update(&item, cid, ITEM_STATUS));
        return Ok(());
    };
    ctx.db
        .characters
        .exec(
            "UPDATE character_items SET character_id = ?, status = ?, guild_id = NULL, layer = ?, row_idx = ?, col_idx = ? WHERE id = ?",
            (cid, stored_status::INVENTORY, grid[0], grid[1], grid[2], iid),
        )
        .await?;
    let g = st.guilds.by_id.get_mut(&gid).unwrap();
    g.warehouse.items.remove(&iid);
    let p = st.players.get_mut(&sid).unwrap();
    let taken = InventoryItem { grid, ..item };
    p.inventory.items.insert(iid, taken.clone());
    p.send_raw(item_update(&taken, cid, crate::world::items::item_status::INVENTORY));
    let name = p.ch.name.clone();
    let g = &st.guilds.by_id[&gid];
    for v in other_viewers(st, g, sid) {
        let pv = &st.players[&v];
        pv.send_raw(item_remove(iid, Some(pv.ch.id)));
    }
    info!("[guilde] {name} retire {} × {} de l'entrepôt de {}", taken.count, ctx.catalog.get(taken.tid).map(|d| d.name.as_str()).unwrap_or("?"), g.name);
    Ok(())
}

/// Sortie du jeu : la fenêtre se ferme.
pub fn on_leave_world(st: &mut WorldState, sid: u32) {
    for g in st.guilds.by_id.values_mut() {
        g.warehouse.viewers.retain(|v| *v != sid);
    }
    st.guilds.warehouse_keepers.remove(&sid);
}
