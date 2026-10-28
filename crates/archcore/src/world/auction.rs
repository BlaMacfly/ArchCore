//! Maison des ventes entre joueurs (AgpmAuction 0x0B) : achat immédiat, à prix fixe, de la pile entière.
//!
//! ```text
//!   Paquet (drapeaux 2) : 0 op, 1 CID du joueur, 2 sous-paquet selon l'op, 3 résultat (0 succès).
//!   Entrée en jeu  ← op 1 ADD_SALES par vente du personnage (en vente ou vendue, à encaisser)
//!   Ouvrir         → op 9 (EVENT_BASE d'un PNJ « Item Trader », fonction 28)   ← op 10 (même EVENT_BASE)
//!   Rechercher     → op 12 SELECT2 {TID, curseur, sens}   ← op 12 {TID, curseur, sens, total, résultats}
//!   Mettre en vente → op 4 SELL {IID, quantité, or, CC}  ← op 4 (résultat), op 1 ADD_SALES, AGPMITEM op 1
//!   Acheter        → op 7 BUY {docID, quantité, TID}      ← AGPMITEM op 0, argent, op 7 (résultat)
//!                                                          vendeur en jeu ← op 3 UPDATE_SALES (statut 1, vendu)
//!   Annuler        → op 5 CANCEL {identifiant}            ← AGPMITEM op 0, op 5 (résultat), op 2 REMOVE_SALES
//!   Encaisser      → op 6 CONFIRM {identifiant}           ← argent, op 6 (gains), op 2 REMOVE_SALES
//! ```
//! Recherche : uniquement par TID, 15 lignes au plus, sens 0 = ventes d'identifiant inférieur au curseur par
//! identifiant décroissant (le client part de 9 999 999 999 999 et suit avec le dernier reçu), sens 1 = supérieur.
//! Chaque ligne a 16 colonnes, des chaînes terminées par un zéro : 0 docID, 1 vendeur, 2 identifiant de vente, 3 or,
//! 4 CC, 5 IID, 6 TID, 7 quantité, 8 date, 9 conversion (vide), 10 durabilité, 11 durabilité maximale, 12 options
//! « a,b,c… », 13 à 15 vides.
//!
//! Choix d'ArchCore (le serveur d'origine n'est pas connu) : l'identifiant de la vente sert aussi de docID. Le prix
//! payé est celui de la base, jamais celui du paquet ; or et CC (`characters.cash`, champ 10 d'AGPMCHARACTER) sont
//! débités et crédités. Mise en vente : objet de l'inventaire ordinaire seulement (ni porté, ni en banque, ni dans un
//! échange), classé dans la maison des ventes (« FirstCategory » et « SecondCategory »), ni objet d'événement, ni lié
//! dès l'obtention (`BoundType` 1) ; le parchemin Marvel (champ 5) est ignoré. Prix comme le client : or 0 ou de
//! 100 000 à 1 000 000 000 (l'argent d'un personnage ne dépasse pas ce plafond), CC jusqu'à 1 000 000, pas les deux
//! nuls. Frais de 3 % de l'or et du CC prélevés à la mise en vente, non remboursés à l'annulation ; pas de taxe à
//! l'encaissement. `Auction.MaxSales` ventes par personnage (code 6) ; après `Auction.ExpireDays` jours la vente
//! n'est plus proposée à la recherche mais reste annulable. Ouvrir la planche, chercher et acheter exigent d'être
//! près d'un PNJ de fonction 28 (le palier 3 de meurtrier est refusé) ; vendre, annuler et encaisser se font de la
//! boîte de ventes, partout. Le client n'affiche que certains codes : un refus sans code connu est accompagné d'un
//! message système. L'objet reste une ligne de `character_items` (statut 5) ; en base : `auction_sale`, journal
//! `log_auction`.

use super::items::{convert_add, convert_of, durability_of, item_add_of, item_remove, item_status, options_of, stored_status, InventoryItem, CONVERT_COLUMNS};
use super::npc_events::{npc_distance, resolve, MAX_NPC_DISTANCE};
use super::npcs::{event_function, Npc};
use super::{send_system, show_money, Player, WorldCtx, WorldState};
use crate::db::{col, Databases};
use crate::journal::{self, AuctionLog};
use crate::{debug, info, warn};
use archcore_net::schema::{self, auction_op, auction_result, mini};
use archcore_net::{build_mini, decode_mini, Fields, Packet};

/// Frais de mise en vente, en pour cent de l'or et du CC (calcul du client, division entière).
pub const FEE_PERCENT: i64 = 3;
/// Prix en or : 0, ou au moins 100 000 (sysstr 922) ; au plus le plafond de l'argent d'un personnage.
pub const MIN_GOLD: i64 = 100_000;
pub const MAX_GOLD: i64 = super::npc_trade::MAX_MONEY;
/// Prix en CC au plus (le client le bride à cette valeur).
pub const MAX_CC: i64 = 1_000_000;
/// Lignes au plus par réponse de recherche.
pub const PAGE: usize = 15;
/// Colonnes des résultats de recherche.
pub const COLUMNS: [&str; 16] =
    ["doc_id", "seller", "sales_id", "gold", "cc", "iid", "tid", "quantity", "date", "convert", "durability", "max_durability", "options", "skill_plus", "prestige", "extra"];
/// Statuts d'une vente.
pub const ON_SALE: i64 = 0;
pub const SOLD: i64 = 1;

/// Vente d'un personnage (sa boîte de ventes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sale {
    pub id: u32,
    /// Objet en vente (`character_items`), `None` une fois vendu.
    pub iid: Option<u32>,
    pub tid: u32,
    pub count: i64,
    pub gold: i64,
    pub cc: i64,
    pub status: i64,
    /// Date d'enregistrement « AAAA-MM-JJ hh:mm:ss ».
    pub registered: String,
}

/// Maison des ventes d'un joueur en jeu : ses ventes, et le PNJ chez qui la planche est ouverte.
#[derive(Clone, Debug, Default)]
pub struct Auction {
    pub sales: Vec<Sale>,
    pub npc: Option<u32>,
}

const SALE_COLUMNS: &str = "id, item_id, item_tid, quantity, price_gold, price_cc, status, DATE_FORMAT(registered_at, '%Y-%m-%d %H:%i:%s') AS registered";

fn sale_of(r: &mysql_async::Row) -> Sale {
    Sale {
        id: col(r, "id"),
        iid: col::<Option<u32>>(r, "item_id"),
        tid: col(r, "item_tid"),
        count: col(r, "quantity"),
        gold: col(r, "price_gold"),
        cc: col(r, "price_cc"),
        status: col(r, "status"),
        registered: col(r, "registered"),
    }
}

impl Auction {
    pub async fn load(db: &Databases, cid: u32) -> anyhow::Result<Self> {
        let rows = db.characters.rows(&format!("SELECT {SALE_COLUMNS} FROM auction_sale WHERE seller_id = ? ORDER BY id"), (cid,)).await?;
        Ok(Auction { sales: rows.iter().map(sale_of).collect(), npc: None })
    }

    pub fn get(&self, id: u32) -> Option<&Sale> {
        self.sales.iter().find(|s| s.id == id)
    }
}

/// Frais d'une mise en vente : (or, CC).
pub fn fees(gold: i64, cc: i64) -> (i64, i64) {
    (gold * FEE_PERCENT / 100, cc * FEE_PERCENT / 100)
}

/// Prix admis (règles du client) ?
pub fn price_ok(gold: i64, cc: i64) -> bool {
    let gold_ok = gold == 0 || (MIN_GOLD..=MAX_GOLD).contains(&gold);
    gold_ok && (0..=MAX_CC).contains(&cc) && gold + cc > 0
}

/// Mini SALES complet (ADD_SALES).
pub fn sales_mini(s: &Sale) -> Vec<u8> {
    let m = &mini::AUCTION_SALES;
    let iid = s.iid.unwrap_or(0);
    let f = Fields::new(m)
        .with(0, s.id)
        .with(1, s.id as u64)
        .with(2, iid as u64)
        .with(3, iid)
        .with(4, s.gold)
        .with(5, s.cc)
        .with(6, s.count)
        .with(7, s.status)
        .with(8, s.registered.as_str())
        .with(9, s.tid);
    build_mini(m, &f)
}

/// Mini SALES réduit à l'identifiant (REMOVE_SALES) ou à l'identifiant et au statut (UPDATE_SALES).
fn sales_id_mini(id: u32, status: Option<i64>) -> Vec<u8> {
    let m = &mini::AUCTION_SALES;
    build_mini(m, &Fields::new(m).with(0, id).with_opt(7, status))
}

fn fields(op: i64, cid: u32) -> Fields {
    let s = &schema::AUCTION;
    Fields::new(s).with(0, op).with(1, cid)
}

/// Résultats d'une recherche (mini ROWSET) : chaque valeur est une chaîne terminée par un zéro, colonnes de largeur
/// fixe (la plus longue valeur de la colonne), décalages en U32.
pub fn rowset(rows: &[Vec<String>]) -> Vec<u8> {
    let n = COLUMNS.len();
    let widths: Vec<usize> = (0..n).map(|c| rows.iter().map(|r| r.get(c).map_or(0, String::len)).max().unwrap_or(0) + 1).collect();
    let offsets: Vec<usize> = widths.iter().scan(0, |at, w| {
        let o = *at;
        *at += w;
        Some(o)
    }).collect();
    let row_size: usize = widths.iter().sum();
    let mut buffer = vec![0u8; row_size * rows.len()];
    for (i, r) in rows.iter().enumerate() {
        for (c, v) in r.iter().take(n).enumerate() {
            let at = i * row_size + offsets[c];
            buffer[at..at + v.len()].copy_from_slice(v.as_bytes());
        }
    }
    let headers: Vec<u8> = COLUMNS.iter().flat_map(|h| h.bytes().chain(std::iter::once(0))).collect();
    let offsets: Vec<u8> = offsets.iter().flat_map(|o| (*o as u32).to_le_bytes()).collect();
    let m = &mini::AUCTION_ROWSET;
    let f = Fields::new(m)
        .with(0, 0)
        .with(1, b"SELECT2\0".to_vec())
        .with(2, headers)
        .with(3, rows.len() as u32)
        .with(4, n as u32)
        .with(5, row_size as u32)
        .with(6, buffer)
        .with(7, offsets);
    build_mini(m, &f)
}

/// Relit une chaîne d'un tampon de résultats (tests, simulateur) : ligne `row`, colonne `col`.
pub fn rowset_value(rs: &Fields, row: usize, column: usize) -> Option<String> {
    let size = rs.int(5)? as usize;
    let offsets = rs.bytes(7)?;
    let off = u32::from_le_bytes(offsets.get(column * 4..column * 4 + 4)?.try_into().ok()?) as usize;
    let buf = rs.bytes(6)?;
    let start = row * size + off;
    let cell = buf.get(start..(row + 1) * size)?;
    let end = cell.iter().position(|b| *b == 0).unwrap_or(cell.len());
    Some(String::from_utf8_lossy(&cell[..end]).into_owned())
}

/// Message système qui accompagne un refus.
fn refuse(st: &WorldState, sid: u32, op: i64, sub: Option<Vec<u8>>, code: i64, text: &str) {
    let Some(p) = st.players.get(&sid) else { return };
    p.send(&schema::AUCTION, fields(op, p.ch.id).with_opt(2, sub).with(3, code));
    if !text.is_empty() {
        send_system(st, sid, text);
    }
}

/// CC d'un joueur : enregistré par l'appelant, reporté en mémoire et envoyé (AGPMCHARACTER champ 10).
fn show_cash(p: &mut Player, cash: u64) {
    p.ch.cash = cash;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(10, cash));
}

/// PNJ de la maison des ventes à portée, chez qui la planche est ouverte.
fn trader<'a>(ctx: &'a WorldCtx, p: &Player) -> Option<&'a Npc> {
    let npc = ctx.npcs.get(p.auction.npc?)?;
    (npc_distance(npc, p.ch.pos_x, p.ch.pos_z) <= MAX_NPC_DISTANCE).then_some(npc)
}

/// AgpmAuction reçu.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    if !player.in_game {
        return Ok(());
    }
    let sub = p.bytes(2).unwrap_or_default();
    match p.int(0).unwrap_or(-1) {
        auction_op::EVENT_REQUEST => open(ctx, st, sid, sub),
        auction_op::SELECT2 => search(ctx, st, sid, sub).await?,
        auction_op::SELL => sell(ctx, st, sid, sub).await?,
        auction_op::BUY => buy(ctx, st, sid, sub).await?,
        auction_op::CANCEL => cancel(ctx, st, sid, sub).await?,
        auction_op::CONFIRM => confirm(ctx, st, sid, sub).await?,
        op => debug!("[ventes] #{sid} op {op} ignorée"),
    }
    Ok(())
}

/// Op 9 : ouverture de la planche chez un PNJ de fonction 28 → op 10.
fn open(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sub: &[u8]) {
    let base = decode_mini(&mini::EVENT_BASE, sub).ok();
    let p = &st.players[&sid];
    let trades = |n: &Npc| n.events.iter().any(|e| e.function == event_function::AUCTION);
    let Some(npc) = base.as_ref().and_then(|b| resolve(&ctx.npcs, b)).map(|(n, _)| n).filter(|n| trades(n) && npc_distance(n, p.ch.pos_x, p.ch.pos_z) <= MAX_NPC_DISTANCE) else {
        warn!("[ventes] #{sid} {} : pas de marchand de la maison des ventes à portée", p.ch.name);
        return;
    };
    // « Rogue status level 3 » (CharacterPenalty_CantAuction) : le client refuse déjà.
    if super::pvp::murderer_level(p.ch.murderer) >= 3 {
        send_system(st, sid, "Un meurtrier ne peut pas utiliser la maison des ventes.");
        return;
    }
    let (npc_cid, npc_name) = (npc.cid, npc.name.clone());
    let base = base.map(|b| build_mini(&mini::EVENT_BASE, &b)).unwrap_or_else(|| super::npcs::event_base(npc_cid, event_function::AUCTION));
    let p = st.players.get_mut(&sid).unwrap();
    p.auction.npc = Some(npc_cid);
    p.send(&schema::AUCTION, fields(auction_op::EVENT_GRANT, p.ch.id).with(2, base));
    debug!("[ventes] {} ouvre la maison des ventes chez {npc_name}", p.ch.name);
}

/// Ligne de résultat d'une vente lue en base.
fn search_row(ctx: &WorldCtx, r: &mysql_async::Row) -> Vec<String> {
    let tid: u32 = col(r, "item_tid");
    let max = ctx.catalog.get(tid).and_then(|d| d.equip.as_ref()).map(|e| e.durability).unwrap_or(0);
    let stored = durability_of(r);
    let current = if stored < 0 { max } else { stored.min(max) };
    let options = options_of(r).iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    let id: u32 = col(r, "id");
    vec![
        id.to_string(),
        col::<String>(r, "seller"),
        id.to_string(),
        col::<u64>(r, "price_gold").to_string(),
        col::<u64>(r, "price_cc").to_string(),
        col::<u64>(r, "item_id").to_string(),
        tid.to_string(),
        col::<u32>(r, "quantity").to_string(),
        col::<String>(r, "registered"),
        String::new(),
        current.to_string(),
        max.to_string(),
        options,
        String::new(),
        String::new(),
        String::new(),
    ]
}

/// Op 12 : ventes d'un TID autour d'un curseur.
async fn search(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sub: &[u8]) -> anyhow::Result<()> {
    let p = &st.players[&sid];
    if trader(ctx, p).is_none() {
        send_system(st, sid, "Parlez à un marchand de la maison des ventes (Item Trader).");
        return Ok(());
    }
    let q = decode_mini(&mini::AUCTION_SELECT2, sub).unwrap_or_else(|_| Fields::new(&mini::AUCTION_SELECT2));
    let (tid, cursor, after) = (q.int(0).unwrap_or(0).max(0) as u32, q.int(1).unwrap_or(i64::MAX).max(0), q.int(2) == Some(1));
    let sql = format!(
        "SELECT s.id, c.name AS seller, s.price_gold, s.price_cc, s.item_id, s.item_tid, s.quantity, \
         DATE_FORMAT(s.registered_at, '%Y-%m-%d %H:%i:%s') AS registered, i.durability, i.option_tids \
         FROM auction_sale s JOIN characters c ON c.id = s.seller_id JOIN character_items i ON i.id = s.item_id \
         WHERE s.item_tid = ? AND s.status = 0 AND s.expires_at > NOW() AND s.id {} ? ORDER BY s.id {} LIMIT {PAGE}",
        if after { ">" } else { "<" },
        if after { "ASC" } else { "DESC" }
    );
    let mut rows = ctx.db.characters.rows(&sql, (tid, cursor)).await?;
    if after {
        rows.reverse();
    }
    let total: u64 = ctx
        .db
        .characters
        .first("SELECT COUNT(*) AS n FROM auction_sale WHERE item_tid = ? AND status = 0 AND expires_at > NOW()", (tid,))
        .await?
        .map(|r| col(&r, "n"))
        .unwrap_or(0);
    let lines: Vec<Vec<String>> = rows.iter().map(|r| search_row(ctx, r)).collect();
    let m = &mini::AUCTION_SELECT2;
    let answer = build_mini(m, &Fields::new(m).with(0, tid).with(1, cursor).with(2, i64::from(after)).with(3, total).with(4, rowset(&lines)));
    let p = &st.players[&sid];
    p.send(&schema::AUCTION, fields(auction_op::SELECT2, p.ch.id).with(2, answer).with(3, auction_result::OK));
    debug!("[ventes] {} cherche le TID {tid} : {} ligne(s) sur {total}", p.ch.name, lines.len());
    Ok(())
}

/// Op 4 : mise en vente d'une pile de l'inventaire.
async fn sell(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sub: &[u8]) -> anyhow::Result<()> {
    let q = decode_mini(&mini::AUCTION_SELL, sub).unwrap_or_else(|_| Fields::new(&mini::AUCTION_SELL));
    let (iid, count, gold, cc) = (q.int(0).unwrap_or(0) as u32, q.int(1).unwrap_or(0), q.int(2).unwrap_or(0), q.int(3).unwrap_or(0));
    let echo = || {
        let m = &mini::AUCTION_SELL;
        build_mini(m, &Fields::new(m).with(0, iid).with(1, count).with(2, gold).with(3, cc))
    };
    let refused = |st: &WorldState, code: i64, text: &str| refuse(st, sid, auction_op::SELL, Some(echo()), code, text);
    let p = &st.players[&sid];
    let in_trade = p.trade.as_ref().is_some_and(|t| t.items.contains_key(&iid));
    let Some(item) = p.inventory.get(iid).filter(|_| !in_trade).cloned() else {
        refused(st, auction_result::FAILED, "Seul un objet de l'inventaire peut être mis en vente.");
        return Ok(());
    };
    let Some(data) = ctx.catalog.get(item.tid) else {
        refused(st, auction_result::FAILED, "Cet objet ne peut pas être mis en vente.");
        return Ok(());
    };
    if data.auction_category.0 == 0 || data.auction_category.1 == 0 {
        refused(st, auction_result::FAILED, "Cet objet ne peut pas être mis en vente (sans catégorie dans la maison des ventes).");
        return Ok(());
    }
    if data.event_item {
        refused(st, auction_result::FAILED, "Un objet d'événement ne peut pas être mis en vente.");
        return Ok(());
    }
    if data.bound_type == 1 {
        refused(st, auction_result::FAILED, "Un objet lié ne peut pas être mis en vente.");
        return Ok(());
    }
    if count != item.count {
        refused(st, auction_result::FAILED, "La pile entière est mise en vente.");
        return Ok(());
    }
    if !price_ok(gold, cc) {
        refused(st, auction_result::FAILED, "Prix refusé : or nul ou de 100 000 à 1 000 000 000, CC jusqu'à 1 000 000, pas les deux nuls.");
        return Ok(());
    }
    if p.auction.sales.len() >= ctx.config.world.auction_max_sales {
        refused(st, auction_result::TOO_MANY_SALES, "");
        return Ok(());
    }
    let (fee_gold, fee_cc) = fees(gold, cc);
    let (money, cash) = (p.ch.money as i64, p.ch.cash as i64);
    if money < fee_gold || cash < fee_cc {
        refused(st, auction_result::FAILED, &format!("Frais de mise en vente : {fee_gold} pièce(s) d'or et {fee_cc} CC."));
        return Ok(());
    }
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let days = ctx.config.world.auction_expire_days;
    let id = ctx
        .db
        .characters
        .atomic(vec![
            (
                "UPDATE character_items SET status = ?, layer = 0, row_idx = 0, col_idx = 0 WHERE id = ? AND character_id = ? AND status = ?",
                (stored_status::AUCTION, iid, cid, stored_status::INVENTORY).into(),
            ),
            ("UPDATE characters SET money = ?, cash = ? WHERE id = ?", (money - fee_gold, cash - fee_cc, cid).into()),
            (
                "INSERT INTO auction_sale (seller_id, item_id, item_tid, quantity, price_gold, price_cc, fee_gold, fee_cc, expires_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, DATE_ADD(NOW(), INTERVAL ? DAY))",
                (cid, iid, item.tid, item.count, gold, cc, fee_gold, fee_cc, days).into(),
            ),
        ])
        .await? as u32;
    let registered = ctx.db.characters.first("SELECT DATE_FORMAT(registered_at, '%Y-%m-%d %H:%i:%s') AS d FROM auction_sale WHERE id = ?", (id,)).await?.map(|r| col::<String>(&r, "d")).unwrap_or_default();
    let sale = Sale { id, iid: Some(iid), tid: item.tid, count: item.count, gold, cc, status: ON_SALE, registered };
    let p = st.players.get_mut(&sid).unwrap();
    // Réponse avant le retrait : le client lit le nom de l'objet encore présent (sysstr 451).
    p.send(&schema::AUCTION, fields(auction_op::SELL, cid).with(2, echo()).with(3, auction_result::OK));
    p.send(&schema::AUCTION, fields(auction_op::ADD_SALES, cid).with(2, sales_mini(&sale)));
    p.inventory.items.remove(&iid);
    p.send_raw(item_remove(iid, Some(cid)));
    if fee_gold > 0 {
        show_money(p, (money - fee_gold) as u64);
    }
    if fee_cc > 0 {
        show_cash(p, (cash - fee_cc) as u64);
    }
    p.auction.sales.push(sale);
    info!("[ventes] {name} met en vente {} × {} pour {gold} or et {cc} CC (vente {id}, frais {fee_gold} or, {fee_cc} CC)", item.count, data.name);
    journal::auction(&ctx.db, &AuctionLog { sale_id: id, action: "vente", character: (cid, &name), other: None, tid: item.tid, iid: Some(iid), count: item.count, price: (gold, cc), fee: (fee_gold, fee_cc) });
    Ok(())
}

/// Op 7 : achat d'une vente entière, au prix de la base.
async fn buy(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sub: &[u8]) -> anyhow::Result<()> {
    let q = decode_mini(&mini::AUCTION_BUY, sub).unwrap_or_else(|_| Fields::new(&mini::AUCTION_BUY));
    let (id, count) = (q.int(0).unwrap_or(0).clamp(0, u32::MAX as i64) as u32, q.int(1).unwrap_or(0));
    let echo = |count: i64, tid: i64, gold: i64, cc: i64| {
        let m = &mini::AUCTION_BUY;
        build_mini(m, &Fields::new(m).with(0, id).with(1, count).with(2, tid).with(3, gold).with(4, cc))
    };
    let refused = |st: &WorldState, code: i64, text: &str| refuse(st, sid, auction_op::BUY, Some(echo(count, q.int(2).unwrap_or(0), 0, 0)), code, text);
    let p = &st.players[&sid];
    if trader(ctx, p).is_none() {
        refused(st, auction_result::FAILED, "Parlez à un marchand de la maison des ventes (Item Trader).");
        return Ok(());
    }
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let row = ctx
        .db
        .characters
        .first(
            &format!(
                "SELECT s.seller_id, s.item_id, s.item_tid, s.quantity, s.price_gold, s.price_cc, i.template_id, i.count, {} \
                 FROM auction_sale s JOIN character_items i ON i.id = s.item_id AND i.status = ? \
                 WHERE s.id = ? AND s.status = 0 AND s.expires_at > NOW()",
                CONVERT_COLUMNS.split(", ").map(|c| format!("i.{c}")).collect::<Vec<_>>().join(", ")
            ),
            (stored_status::AUCTION, id),
        )
        .await?;
    let Some(row) = row else {
        refused(st, auction_result::NOT_AVAILABLE, "");
        return Ok(());
    };
    let (seller, iid, tid, qty, gold, cc): (u32, u32, u32, i64, i64, i64) = (col(&row, "seller_id"), col(&row, "item_id"), col(&row, "item_tid"), col(&row, "quantity"), col(&row, "price_gold"), col(&row, "price_cc"));
    if seller == cid {
        refused(st, auction_result::FAILED, "Vous ne pouvez pas acheter votre propre vente.");
        return Ok(());
    }
    if count != qty {
        refused(st, auction_result::QUANTITY, "");
        return Ok(());
    }
    let p = &st.players[&sid];
    let (money, cash) = (p.ch.money as i64, p.ch.cash as i64);
    if money < gold || cash < cc {
        refused(st, auction_result::FAILED, &format!("Il faut {gold} pièce(s) d'or et {cc} CC."));
        return Ok(());
    }
    let Some(grid) = p.inventory.free_slot() else {
        refused(st, auction_result::INVENTORY_FULL, "");
        return Ok(());
    };
    ctx.db
        .characters
        .atomic(vec![
            (
                "UPDATE character_items SET character_id = ?, status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ? AND status = ?",
                (cid, stored_status::INVENTORY, grid[0], grid[1], grid[2], iid, stored_status::AUCTION).into(),
            ),
            ("UPDATE auction_sale SET status = 1, item_id = NULL, buyer_id = ?, sold_at = NOW() WHERE id = ? AND status = 0", (cid, id).into()),
            ("UPDATE characters SET money = ?, cash = ? WHERE id = ?", (money - gold, cash - cc, cid).into()),
        ])
        .await?;
    let item = InventoryItem { iid, tid: col(&row, "template_id"), count: col(&row, "count"), grid, conv: convert_of(&row), options: options_of(&row), durability: durability_of(&row) };
    let p = st.players.get_mut(&sid).unwrap();
    p.inventory.items.insert(iid, item.clone());
    p.send_raw(item_add_of(&item, cid, item_status::INVENTORY, Some(grid)));
    if let Some(c) = convert_add(&item, cid) {
        p.send_raw(c);
    }
    if gold > 0 {
        show_money(p, (money - gold) as u64);
    }
    if cc > 0 {
        show_cash(p, (cash - cc) as u64);
    }
    p.send(&schema::AUCTION, fields(auction_op::BUY, cid).with(2, echo(qty, i64::from(tid), gold, cc)).with(3, auction_result::OK));
    let label = ctx.catalog.get(tid).map(|d| d.name.as_str()).unwrap_or("?");
    info!("[ventes] {name} achète {qty} × {label} (vente {id}) pour {gold} or et {cc} CC");
    journal::auction(&ctx.db, &AuctionLog { sale_id: id, action: "achat", character: (cid, &name), other: Some(seller), tid, iid: Some(iid), count: qty, price: (gold, cc), fee: (0, 0) });
    // Vendeur en jeu : la vente passe à « vendu » (sysstr 455, bouton « Confirm »).
    if let Some(s) = st.by_cid.get(&seller).copied().and_then(|s| st.players.get_mut(&s)) {
        if let Some(sale) = s.auction.sales.iter_mut().find(|x| x.id == id) {
            sale.status = SOLD;
            sale.iid = None;
        }
        if s.in_game {
            s.send(&schema::AUCTION, fields(auction_op::UPDATE_SALES, s.ch.id).with(2, sales_id_mini(id, Some(SOLD))));
        }
    }
    Ok(())
}

/// Identifiant de vente d'un mini CANCEL ou CONFIRM (champ 0, le même).
fn sale_id(sub: &[u8]) -> u32 {
    decode_mini(&mini::AUCTION_CANCEL, sub).ok().and_then(|f| f.int(0)).unwrap_or(0).clamp(0, u32::MAX as i64) as u32
}

/// Op 5 : annulation d'une vente pas encore conclue (échue comprise) ; l'objet revient dans l'inventaire.
async fn cancel(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sub: &[u8]) -> anyhow::Result<()> {
    let id = sale_id(sub);
    let echo = |tid: u32, count: i64| {
        let m = &mini::AUCTION_CANCEL;
        build_mini(m, &Fields::new(m).with(0, id).with(1, tid).with(2, count))
    };
    let p = &st.players[&sid];
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let Some(sale) = p.auction.get(id).filter(|s| s.status == ON_SALE).cloned() else {
        refuse(st, sid, auction_op::CANCEL, Some(echo(0, 0)), auction_result::FAILED, "Cette vente ne peut pas être annulée.");
        return Ok(());
    };
    let Some(grid) = p.inventory.free_slot() else {
        refuse(st, sid, auction_op::CANCEL, Some(echo(sale.tid, sale.count)), auction_result::INVENTORY_FULL, "");
        return Ok(());
    };
    let iid = sale.iid.unwrap_or(0);
    let Some(row) = ctx
        .db
        .characters
        .first(&format!("SELECT template_id, count, {CONVERT_COLUMNS} FROM character_items WHERE id = ? AND character_id = ? AND status = ?"), (iid, cid, stored_status::AUCTION))
        .await?
    else {
        warn!("[ventes] {name} : objet {iid} de la vente {id} introuvable");
        refuse(st, sid, auction_op::CANCEL, Some(echo(sale.tid, sale.count)), auction_result::FAILED, "L'objet de cette vente est introuvable.");
        return Ok(());
    };
    ctx.db
        .characters
        .atomic(vec![
            (
                "UPDATE character_items SET status = ?, layer = ?, row_idx = ?, col_idx = ? WHERE id = ? AND character_id = ? AND status = ?",
                (stored_status::INVENTORY, grid[0], grid[1], grid[2], iid, cid, stored_status::AUCTION).into(),
            ),
            ("DELETE FROM auction_sale WHERE id = ? AND seller_id = ? AND status = 0", (id, cid).into()),
        ])
        .await?;
    let item = InventoryItem { iid, tid: col(&row, "template_id"), count: col(&row, "count"), grid, conv: convert_of(&row), options: options_of(&row), durability: durability_of(&row) };
    let p = st.players.get_mut(&sid).unwrap();
    p.auction.sales.retain(|s| s.id != id);
    p.inventory.items.insert(iid, item.clone());
    p.send_raw(item_add_of(&item, cid, item_status::INVENTORY, Some(grid)));
    if let Some(c) = convert_add(&item, cid) {
        p.send_raw(c);
    }
    p.send(&schema::AUCTION, fields(auction_op::CANCEL, cid).with(2, echo(sale.tid, sale.count)).with(3, auction_result::OK));
    p.send(&schema::AUCTION, fields(auction_op::REMOVE_SALES, cid).with(2, sales_id_mini(id, None)));
    info!("[ventes] {name} annule la vente {id} ({} × objet {})", sale.count, sale.tid);
    journal::auction(&ctx.db, &AuctionLog { sale_id: id, action: "annulation", character: (cid, &name), other: None, tid: sale.tid, iid: Some(iid), count: sale.count, price: (sale.gold, sale.cc), fee: (0, 0) });
    Ok(())
}

/// Op 6 : encaissement d'une vente conclue.
async fn confirm(ctx: &WorldCtx, st: &mut WorldState, sid: u32, sub: &[u8]) -> anyhow::Result<()> {
    let id = sale_id(sub);
    let echo = |s: Option<&Sale>| {
        let m = &mini::AUCTION_CONFIRM;
        let f = Fields::new(m).with(0, id);
        build_mini(m, &match s {
            Some(s) => f.with(1, s.tid).with(2, s.count).with(3, s.gold).with(4, s.cc),
            None => f,
        })
    };
    let p = &st.players[&sid];
    let (cid, name) = (p.ch.id, p.ch.name.clone());
    let Some(sale) = p.auction.get(id).filter(|s| s.status == SOLD).cloned() else {
        refuse(st, sid, auction_op::CONFIRM, Some(echo(None)), auction_result::FAILED, "Cette vente n'est pas encore conclue.");
        return Ok(());
    };
    let (money, cash) = (p.ch.money as i64 + sale.gold, p.ch.cash.saturating_add(sale.cc as u64));
    if money > super::npc_trade::MAX_MONEY {
        refuse(st, sid, auction_op::CONFIRM, Some(echo(Some(&sale))), auction_result::FAILED, "Vous ne pouvez pas porter plus d'or.");
        return Ok(());
    }
    ctx.db
        .characters
        .atomic(vec![
            ("UPDATE characters SET money = ?, cash = ? WHERE id = ?", (money, cash, cid).into()),
            ("DELETE FROM auction_sale WHERE id = ? AND seller_id = ? AND status = 1", (id, cid).into()),
        ])
        .await?;
    let p = st.players.get_mut(&sid).unwrap();
    p.auction.sales.retain(|s| s.id != id);
    if sale.gold > 0 {
        show_money(p, money as u64);
    }
    if sale.cc > 0 {
        show_cash(p, cash);
    }
    p.send(&schema::AUCTION, fields(auction_op::CONFIRM, cid).with(2, echo(Some(&sale))).with(3, auction_result::OK));
    p.send(&schema::AUCTION, fields(auction_op::REMOVE_SALES, cid).with(2, sales_id_mini(id, None)));
    info!("[ventes] {name} encaisse la vente {id} : {} or, {} CC", sale.gold, sale.cc);
    journal::auction(&ctx.db, &AuctionLog { sale_id: id, action: "encaissement", character: (cid, &name), other: None, tid: sale.tid, iid: None, count: sale.count, price: (sale.gold, sale.cc), fee: (0, 0) });
    Ok(())
}

/// Entrée en jeu : chaque vente du personnage (op 1), en vente ou à encaisser.
pub fn on_enter(st: &WorldState, sid: u32) {
    let Some(p) = st.players.get(&sid) else { return };
    for s in &p.auction.sales {
        p.send(&schema::AUCTION, fields(auction_op::ADD_SALES, p.ch.id).with(2, sales_mini(s)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::packet::mini_payload;
    use archcore_net::{build_packet, decode_packet};

    #[test]
    fn frais_et_prix() {
        assert_eq!(fees(100_000, 0), (3_000, 0));
        assert_eq!(fees(123_456, 999), (3_703, 29), "division entière, comme le client");
        assert!(price_ok(100_000, 0) && price_ok(0, 10) && price_ok(500_000, 20));
        assert!(!price_ok(99_999, 0), "moins de 100 000 pièces d'or");
        assert!(!price_ok(0, 0) && !price_ok(MAX_GOLD + 1, 0) && !price_ok(0, MAX_CC + 1) && !price_ok(0, -1));
    }

    #[test]
    fn vente_dans_la_boite() {
        let sale = Sale { id: 7, iid: Some(301), tid: 190, count: 5, gold: 150_000, cc: 2, status: ON_SALE, registered: "2026-10-28 15:00:00".into() };
        let raw = build_packet(&schema::AUCTION, &fields(auction_op::ADD_SALES, 9).with(2, sales_mini(&sale)));
        let p = decode_packet(&schema::AUCTION, &raw).unwrap();
        assert_eq!((p.int(0), p.int(1), p.int(3)), (Some(1), Some(9), None));
        let m = decode_mini(&mini::AUCTION_SALES, p.bytes(2).unwrap()).unwrap();
        assert_eq!((m.int(0), m.int(1), m.int(2), m.int(3)), (Some(7), Some(7), Some(301), Some(301)));
        assert_eq!((m.int(4), m.int(5), m.int(6), m.int(7), m.int(9)), (Some(150_000), Some(2), Some(5), Some(0), Some(190)));
        assert_eq!(m.text(8).as_deref(), Some("2026-10-28 15:00:00"));
        let u = decode_mini(&mini::AUCTION_SALES, mini_payload(&sales_id_mini(7, Some(SOLD)))).unwrap();
        assert_eq!((u.int(0), u.int(6), u.int(7)), (Some(7), None, Some(1)));
    }

    #[test]
    fn resultats_de_recherche() {
        let row = |id: &str, seller: &str| {
            let mut r: Vec<String> = vec![id.into(), seller.into(), id.into(), "150000".into(), "0".into(), "301".into(), "190".into(), "5".into()];
            r.resize(COLUMNS.len(), String::new());
            r[12] = "4,9".into();
            r
        };
        let rs = decode_mini(&mini::AUCTION_ROWSET, mini_payload(&rowset(&[row("12", "GmTest"), row("9", "Un vendeur au long nom")]))).unwrap();
        assert_eq!((rs.int(3), rs.int(4)), (Some(2), Some(16)));
        assert_eq!(rs.bytes(7).unwrap().len(), 16 * 4, "un décalage U32 par colonne");
        assert_eq!(rs.bytes(6).unwrap().len(), 2 * rs.int(5).unwrap() as usize);
        assert_eq!(rowset_value(&rs, 0, 0).as_deref(), Some("12"));
        assert_eq!(rowset_value(&rs, 1, 1).as_deref(), Some("Un vendeur au long nom"));
        assert_eq!(rowset_value(&rs, 1, 6).as_deref(), Some("190"));
        assert_eq!(rowset_value(&rs, 0, 12).as_deref(), Some("4,9"));
        assert_eq!(rowset_value(&rs, 0, 15).as_deref(), Some(""));
        // Aucune ligne : tampon absent, nombre de colonnes et décalages présents.
        let empty = decode_mini(&mini::AUCTION_ROWSET, mini_payload(&rowset(&[]))).unwrap();
        assert_eq!((empty.int(3), empty.int(4), empty.bytes(6)), (Some(0), Some(16), None));
    }

    #[test]
    fn demandes_du_client() {
        // Achat comme le client : docID, quantité, TID, prix en or, CC à 0.
        let m = &mini::AUCTION_BUY;
        let buy = build_mini(m, &Fields::new(m).with(0, 12).with(1, 5).with(2, 190).with(3, 150_000).with(4, 0));
        let f = decode_mini(m, mini_payload(&buy)).unwrap();
        assert_eq!((f.int(0), f.int(1), f.int(2), f.int(3)), (Some(12), Some(5), Some(190), Some(150_000)));
        let c = &mini::AUCTION_CANCEL;
        assert_eq!(sale_id(mini_payload(&build_mini(c, &Fields::new(c).with(0, 33)))), 33);
        let s = &mini::AUCTION_SELECT2;
        let q = decode_mini(s, mini_payload(&build_mini(s, &Fields::new(s).with(0, 190).with(1, 9_999_999_999_999i64).with(2, 0).with(3, 0)))).unwrap();
        assert_eq!((q.int(0), q.int(1), q.int(2), q.bytes(4)), (Some(190), Some(9_999_999_999_999), Some(0), None));
    }
}
