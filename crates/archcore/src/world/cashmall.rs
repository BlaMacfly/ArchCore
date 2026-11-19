//! Boutique Cash de l'épisode 8 (AgpmCashMall 0x2E, fenêtres `UI_CashMall` et `UI_CashMallBuy`).
//!
//! ```text
//!   Table : cashmall.txt des tables du serveur (`archcore import`, gardée dans `client_table`) : une ligne
//!     `Category_Name` ouvre un onglet, les suivantes (première colonne vide) sont ses produits : `CashItem_TID`,
//!     `CashItem_Stack` (nombre d'objets donnés), `CashItem_Price` (en Cash), `CashItem_Type` (genre affiché :
//!     0 normal, 1 nouveau, 2 populaire…, événements `CashMall*Product`), `CashItem_Description`. Produits numérotés
//!     dans l'ordre du fichier à partir de 1 ; un TID absent du catalogue est écarté.
//!   Monnaie : le Cash du personnage (`characters.cash`, champ 10 d'AGPMCHARACTER, « KeepCash » de la fenêtre) ;
//!     aucun service de paiement : la commande MJ `/cash` le fixe.
//!   Ouverture (W et Archonia) : op 7 (version connue) puis op 1 (onglet courant) → op 2 (onglets, version) puis
//!     op 3 (produits de l'onglet). Textes ASCII lus par les deux clients (W 0x556e20 et 0x557120) :
//!       op 2 champ 6 : « nom;produit,produit,…,: » par onglet (nom de 32 octets au plus) ;
//!       op 3 champ 7 : « produit,TID,nombre,prix,genre,description: » par produit (`%d,%d,%d,%I64d,%d,`, la
//!       description va de la dernière virgule aux deux-points).
//!   Achat : op 4 (produit au champ 5, version au champ 2 ; champ 8 : 0, ou 2 chez Archonia, sens inconnu) → op 5
//!     champ 1 : 1 réussite, 2 Cash insuffisant, 3 inventaire plein, 4 liste périmée (le client redemande
//!     l'onglet). Le Cash est pris avant de ranger l'objet : inventaire Cash pour un objet Cash, inventaire sinon,
//!     en piles de la taille maximale de l'objet (contrôles de place du gacha), puis champ 10 d'AGPMCHARACTER.
//!   Op 6 (« revoir le solde », au plus une fois par seconde côté client) : champ 10 renvoyé. Op 8 (offre du jour
//!     d'Archonia) : jamais envoyée.
//! ```

use super::{WorldCtx, WorldState};
use crate::{info, warn};
use archcore_net::schema::{self, cash_mall_op as op, cash_mall_result as result};
use archcore_net::{build_packet, Fields, Packet};
use std::collections::BTreeMap;

/// Version des listes envoyée aux clients (op 2, 3) et attendue à l'achat (op 4) ; non nulle, le client part de 0.
pub const VERSION: i64 = 1;
/// Longueur maximale d'un nom d'onglet (W : copié s'il fait moins de 0x21 octets).
const TAB_NAME_MAX: usize = 32;
/// Longueur gardée d'une description.
const DESCRIPTION_MAX: usize = 100;
/// Onglets au plus (W : rang ≤ 9 accepté à la réception de l'op 3).
const TABS_MAX: usize = 10;

/// Un produit de la boutique.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Product {
    pub id: u32,
    pub tid: u32,
    /// Objets donnés (`CashItem_Stack`).
    pub count: i64,
    /// Prix en Cash.
    pub price: i64,
    /// Genre affiché (`CashItem_Type`).
    pub kind: i64,
    pub description: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tab {
    pub name: String,
    pub products: Vec<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct CashMall {
    pub tabs: Vec<Tab>,
    pub products: BTreeMap<u32, Product>,
}

/// Texte sans les séparateurs des listes (`,` `;` `:`) ni caractères de contrôle.
fn clean(s: &str, max: usize) -> String {
    s.trim().chars().map(|c| if matches!(c, ',' | ';' | ':') || c.is_control() { ' ' } else { c }).take(max).collect::<String>().trim().to_string()
}

/// Onglets et produits de cashmall.txt ; `known` dit si un TID est au catalogue. Rend aussi le nombre de produits
/// écartés.
pub fn parse(text: &str, known: impl Fn(u32) -> bool) -> (CashMall, usize) {
    let mut out = CashMall::default();
    let mut dropped = 0;
    let mut id = 0u32;
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return (out, 0) };
    let columns: Vec<&str> = header.trim_start_matches('\u{feff}').split('\t').map(str::trim).collect();
    let at = |name: &str| columns.iter().position(|c| *c == name);
    let (Some(cat), Some(tid), Some(stack), Some(price)) = (at("Category_Name"), at("CashItem_TID"), at("CashItem_Stack"), at("CashItem_Price")) else {
        return (out, 0);
    };
    let (kind, desc) = (at("CashItem_Type"), at("CashItem_Description"));
    for line in lines {
        let c: Vec<&str> = line.split('\t').collect();
        let cell = |i: Option<usize>| i.and_then(|i| c.get(i)).map(|v| v.trim()).unwrap_or("");
        let num = |i: Option<usize>| cell(i).parse::<i64>().unwrap_or(0);
        let category = cell(Some(cat));
        if !category.is_empty() {
            out.tabs.push(Tab { name: clean(category, TAB_NAME_MAX), products: Vec::new() });
            continue;
        }
        let t = num(Some(tid));
        if t <= 0 {
            continue;
        }
        id += 1;
        let (count, cost) = (num(Some(stack)).max(1), num(Some(price)));
        let Some(tab) = out.tabs.last_mut().filter(|_| known(t as u32) && cost > 0) else {
            dropped += 1;
            continue;
        };
        tab.products.push(id);
        out.products.insert(id, Product { id, tid: t as u32, count, price: cost, kind: num(kind).max(0), description: clean(cell(desc), DESCRIPTION_MAX) });
    }
    out.tabs.retain(|t| !t.products.is_empty());
    if out.tabs.len() > TABS_MAX {
        let cut: Vec<Tab> = out.tabs.split_off(TABS_MAX);
        for t in cut {
            dropped += t.products.len();
            for p in t.products {
                out.products.remove(&p);
            }
        }
    }
    (out, dropped)
}

impl CashMall {
    /// Texte des onglets (op 2 champ 6).
    pub fn mall_list(&self) -> String {
        self.tabs.iter().map(|t| format!("{};{},:", t.name, t.products.iter().map(u32::to_string).collect::<Vec<_>>().join(","))).collect()
    }

    /// Texte des produits d'un onglet (op 3 champ 7) ; `None` hors des onglets.
    pub fn product_list(&self, tab: usize) -> Option<String> {
        let t = self.tabs.get(tab)?;
        Some(t.products.iter().filter_map(|id| self.products.get(id)).map(|p| format!("{},{},{},{},{},{}:", p.id, p.tid, p.count, p.price, p.kind, p.description)).collect())
    }

    /// Table gardée par `archcore import`.
    pub async fn load(db: &crate::db::Databases, catalog: &super::items::ItemCatalog) -> Self {
        use crate::db::col;
        let row = db.world.first("SELECT data FROM client_table WHERE row_idx = 0 AND name = 'cashmall.txt'", ()).await.ok().flatten();
        let text = row.and_then(|r| serde_json::from_str::<String>(&col::<String>(&r, "data")).ok()).unwrap_or_default();
        let (mall, dropped) = parse(&text, |tid| catalog.get(tid).is_some());
        if mall.tabs.is_empty() {
            warn!("boutique Cash : cashmall.txt absent ou vide : lancer `archcore import`");
        } else {
            info!("boutique Cash : {} onglet(s), {} produit(s) ({dropped} écarté(s) : objet inconnu ou sans prix)", mall.tabs.len(), mall.products.len());
        }
        mall
    }
}

/// Piles de `count` objets de taille maximale `stack`.
pub fn piles(count: i64, stack: i64) -> Vec<i64> {
    let stack = stack.max(1);
    let mut out = Vec::new();
    let mut left = count.max(1);
    while left > 0 {
        out.push(left.min(stack));
        left -= stack;
    }
    out
}

/// Code de l'achat avant tout changement : liste périmée, Cash, place.
pub fn check(version: i64, product: Option<&Product>, cash: i64, room: impl Fn(&Product) -> bool) -> i64 {
    match product {
        _ if version != VERSION => result::NEED_NEW_ITEM_LIST,
        None => result::NEED_NEW_ITEM_LIST,
        Some(p) if cash < p.price => result::NOT_ENOUGH_CASH,
        Some(p) if !room(p) => result::INVENTORY_FULL,
        Some(_) => result::SUCCESS,
    }
}

fn fields(o: i64, cid: u32) -> Fields {
    Fields::new(&schema::CASH_MALL).with(0, o).with(3, cid)
}

/// Op 2 : onglets.
pub fn mall_packet(mall: &CashMall, cid: u32) -> Vec<u8> {
    build_packet(&schema::CASH_MALL, &fields(op::MALL_LIST, cid).with(2, VERSION).with(6, mall.mall_list().into_bytes()))
}

/// Op 3 : produits d'un onglet.
pub fn products_packet(mall: &CashMall, cid: u32, tab: usize) -> Option<Vec<u8>> {
    let text = mall.product_list(tab)?;
    Some(build_packet(&schema::CASH_MALL, &fields(op::PRODUCT_LIST, cid).with(2, VERSION).with(4, tab as i64).with(7, text.into_bytes())))
}

/// Op 5 : résultat d'un achat.
pub fn result_packet(cid: u32, code: i64) -> Vec<u8> {
    build_packet(&schema::CASH_MALL, &fields(op::BUY_RESULT, cid).with(1, code))
}

/// Cash enregistré : reporté en mémoire et envoyé (AGPMCHARACTER champ 10).
fn show_cash(p: &mut super::Player, cash: u64) {
    p.ch.cash = cash;
    let c = &schema::CHARACTER;
    p.send(c, Fields::new(c).with(0, 1).with(1, p.ch.id).with(10, cash));
}

/// Fixe le Cash d'un joueur (commande MJ `/cash`).
pub async fn set_cash(ctx: &WorldCtx, st: &mut WorldState, sid: u32, cash: i64) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cash = cash.max(0) as u64;
    ctx.db.characters.exec("UPDATE characters SET cash = ? WHERE id = ?", (cash, p.ch.id)).await?;
    show_cash(p, cash);
    Ok(())
}

/// Place pour les piles d'un produit : inventaire Cash pour un objet Cash, inventaire sinon.
fn has_room(ctx: &WorldCtx, inv: &super::items::Inventory, p: &Product) -> bool {
    let stack = ctx.catalog.get(p.tid).map(|d| d.stack).unwrap_or(1).max(1);
    let piles = piles(p.count, stack);
    if ctx.catalog.is_cash(p.tid) {
        if piles.len() == 1 {
            return inv.cash_has_room(p.tid, p.count, stack);
        }
        let (l, r, c) = super::items::CASH_INVENTORY;
        ((l * r * c) as usize).saturating_sub(inv.cash.len()) >= piles.len()
    } else if piles.len() == 1 {
        inv.placement(p.tid, p.count, stack, None).is_some()
    } else {
        inv.free_count() >= piles.len()
    }
}

/// AGPMCASHMALL reçu du client.
pub async fn on_packet(ctx: &WorldCtx, st: &mut WorldState, sid: u32, p: &Packet) -> anyhow::Result<()> {
    let Some(player) = st.players.get(&sid) else { return Ok(()) };
    let cid = player.ch.id;
    match p.int(0) {
        Some(op::CHECK_VERSION) => {
            player.send_raw(mall_packet(&ctx.cash_mall, cid));
            info!("[boutique] {} ouvre la boutique (version connue {:?}) : {} onglet(s)", player.ch.name, p.int(2), ctx.cash_mall.tabs.len());
        }
        Some(op::REQUEST_PRODUCTS) => {
            let tab = p.int(4).unwrap_or(-1);
            match usize::try_from(tab).ok().and_then(|t| products_packet(&ctx.cash_mall, cid, t)) {
                Some(bytes) => player.send_raw(bytes),
                None => warn!("[boutique] {} : onglet {tab} inconnu", player.ch.name),
            }
        }
        Some(op::REFRESH_CASH) => {
            let c = &schema::CHARACTER;
            player.send(c, Fields::new(c).with(0, 1).with(1, cid).with(10, player.ch.cash));
        }
        Some(op::BUY) => buy(ctx, st, sid, p).await?,
        other => warn!("[boutique] {} : op {other:?} inattendue", player.ch.name),
    }
    Ok(())
}

/// Op 4 : Cash pris, objets rangés, résultat.
async fn buy(ctx: &WorldCtx, st: &mut WorldState, sid: u32, pk: &Packet) -> anyhow::Result<()> {
    use super::items::{item_add_of, item_status};
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let id = pk.int(5).unwrap_or(0);
    let product = u32::try_from(id).ok().and_then(|id| ctx.cash_mall.products.get(&id)).cloned();
    let code = if p.dead { result::NEED_NEW_ITEM_LIST } else { check(pk.int(2).unwrap_or(-1), product.as_ref(), p.ch.cash as i64, |pr| has_room(ctx, &p.inventory, pr)) };
    let (Some(product), result::SUCCESS) = (product, code) else {
        info!("[boutique] {} : achat du produit {id} refusé (code {code}, mort {})", p.ch.name, p.dead);
        p.send_raw(result_packet(cid, code));
        return Ok(());
    };
    // Cash pris d'abord, puis les objets.
    let cash = p.ch.cash - product.price as u64;
    ctx.db.characters.exec("UPDATE characters SET cash = ? WHERE id = ?", (cash, cid)).await?;
    show_cash(p, cash);
    let stack = ctx.catalog.get(product.tid).map(|d| d.stack).unwrap_or(1).max(1);
    let in_cash = ctx.catalog.is_cash(product.tid);
    let mut given = 0;
    for n in piles(product.count, stack) {
        let (added, status) = if in_cash {
            (p.inventory.add_cash(&ctx.db, product.tid, n, stack).await?, item_status::CASH_INVENTORY)
        } else {
            (p.inventory.add(&ctx.db, product.tid, n, stack, None).await?, item_status::INVENTORY)
        };
        let Some((it, created)) = added else { break };
        given += n;
        p.send_raw(if created { item_add_of(&it, cid, status, Some(it.grid)) } else { p.inventory.count_packet(&it, cid) });
        if let Some(c) = super::items::convert_add(&it, cid) {
            p.send_raw(c);
        }
    }
    if given < product.count {
        // Place prise entre-temps : le contrôle a précédé l'achat, rien d'autre n'est rendu que le journal.
        warn!("[boutique] {} : {given} objet(s) rangé(s) sur {} (inventaire plein)", p.ch.name, product.count);
    }
    p.send_raw(result_packet(cid, result::SUCCESS));
    let name = ctx.catalog.get(product.tid).map(|d| d.name.clone()).unwrap_or_default();
    info!("[boutique] {} achète le produit {} : {} × {name} (TID {}) pour {} Cash ; reste {cash} (champ 8 : {:?})", p.ch.name, product.id, product.count, product.tid, product.price, pk.int(8));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use archcore_net::decode_packet;

    const TABLE: &str = "Category_Name\tCashItem_TID\tCashItem_Stack\tCashItem_Price\tCashItem_Type\tCashItem_Description\r\nBundles\t\t\t\t\t\r\n\t6007\t50000\t450\t2\tLens Stone+ Bundle (50000)\r\n\t9999\t1\t10\t0\tInconnu\r\n\t7436\t50\t2300\t0\tFortune Gate Pass, (50)\r\nVide\t\t\t\t\t\r\nConsumable\t\t\t\t\t\r\n\t842\t5\t150\t0\tResurrecting Orb (5)\r\n";

    #[test]
    fn table_et_listes() {
        let (mall, dropped) = parse(TABLE, |tid| tid != 9999);
        assert_eq!(dropped, 1);
        assert_eq!(mall.tabs, vec![Tab { name: "Bundles".into(), products: vec![1, 3] }, Tab { name: "Consumable".into(), products: vec![4] }]);
        assert_eq!(mall.products[&3], Product { id: 3, tid: 7436, count: 50, price: 2300, kind: 0, description: "Fortune Gate Pass  (50)".into() });
        assert_eq!(mall.mall_list(), "Bundles;1,3,:Consumable;4,:");
        assert_eq!(mall.product_list(0).unwrap(), "1,6007,50000,450,2,Lens Stone+ Bundle (50000):3,7436,50,2300,0,Fortune Gate Pass  (50):");
        assert_eq!(mall.product_list(2), None);
        let op3 = decode_packet(&schema::CASH_MALL, &products_packet(&mall, 7, 1).unwrap()).unwrap();
        assert_eq!((op3.int(0), op3.int(2), op3.int(3), op3.int(4)), (Some(3), Some(VERSION), Some(7), Some(1)));
        assert_eq!(op3.bytes(7), Some(&b"4,842,5,150,0,Resurrecting Orb (5):"[..]));
        let op2 = decode_packet(&schema::CASH_MALL, &mall_packet(&mall, 7)).unwrap();
        assert_eq!((op2.int(0), op2.int(2), op2.bytes(6)), (Some(2), Some(1), Some(&b"Bundles;1,3,:Consumable;4,:"[..])));
        let op5 = decode_packet(&schema::CASH_MALL, &result_packet(7, result::INVENTORY_FULL)).unwrap();
        assert_eq!((op5.int(0), op5.int(1)), (Some(5), Some(3)));
    }

    #[test]
    fn controles_et_piles() {
        let p = Product { id: 1, tid: 842, count: 5, price: 150, ..Default::default() };
        assert_eq!(check(VERSION, Some(&p), 150, |_| true), result::SUCCESS);
        assert_eq!(check(VERSION, Some(&p), 149, |_| true), result::NOT_ENOUGH_CASH);
        assert_eq!(check(VERSION, Some(&p), 500, |_| false), result::INVENTORY_FULL);
        assert_eq!(check(0, Some(&p), 500, |_| true), result::NEED_NEW_ITEM_LIST, "version périmée");
        assert_eq!(check(VERSION, None, 500, |_| true), result::NEED_NEW_ITEM_LIST, "produit inconnu");
        assert_eq!(piles(50000, 9999), vec![9999, 9999, 9999, 9999, 9999, 5]);
        assert_eq!(piles(5, 1), vec![1; 5]);
        assert_eq!(piles(1, 0), vec![1]);
    }
}
