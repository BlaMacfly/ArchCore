//! Journaux de l'économie et de l'administration, dans la base `archcore_logs` (écriture seule) : échanges
//! conclus, achats et ventes aux PNJ, mouvements de la banque, maison des ventes, commandes MJ.
//!
//! Chaque écriture part dans sa propre tâche, hors du verrou du monde : un journal lent ou indisponible ne
//! retarde jamais le jeu, et son échec ne fait qu'un avertissement.

use crate::db::{Databases, Pool};
use mysql_async::Params;

fn write(pool: &Pool, what: &'static str, sql: &'static str, params: Params) {
    let pool = pool.clone();
    tokio::spawn(async move {
        if let Err(e) = pool.exec(sql, params).await {
            crate::warn!("journal {what} non écrit : {e:#}");
        }
    });
}

fn cut(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Objets d'un côté d'un échange : `[{iid, tid, count}]`.
pub fn items_json(items: &[(u32, u32, i64)]) -> String {
    serde_json::Value::Array(items.iter().map(|(iid, tid, count)| serde_json::json!({ "iid": iid, "tid": tid, "count": count })).collect()).to_string()
}

/// Échange conclu : ce que chacun a donné.
#[allow(clippy::too_many_arguments)]
pub fn trade(db: &Databases, a: (u32, &str), b: (u32, &str), gold_a: i64, gold_b: i64, items_a: &[(u32, u32, i64)], items_b: &[(u32, u32, i64)]) {
    write(
        &db.logs,
        "d'échange",
        "INSERT INTO log_trade (char_a, name_a, char_b, name_b, gold_a, gold_b, items_a, items_b) VALUES (?,?,?,?,?,?,?,?)",
        (a.0, cut(a.1, 48), b.0, cut(b.1, 48), gold_a, gold_b, items_json(items_a), items_json(items_b)).into(),
    );
}

/// Achat (`achat`) ou vente (`vente`) à un PNJ.
#[allow(clippy::too_many_arguments)]
pub fn shop(db: &Databases, character: (u32, &str), npc: &str, action: &'static str, tid: u32, iid: u32, count: i64, price: i64) {
    write(
        &db.logs,
        "de boutique",
        "INSERT INTO log_shop (character_id, name, npc, action, item_tid, item_iid, count, price) VALUES (?,?,?,?,?,?,?,?)",
        (character.0, cut(character.1, 48), cut(npc, 96), action, tid, iid, count, price).into(),
    );
}

/// Objet déposé (`dépôt`) ou retiré (`retrait`) de la banque.
pub fn bank_item(db: &Databases, account_id: u32, character: (u32, &str), action: &'static str, tid: u32, iid: u32, count: i64) {
    write(
        &db.logs,
        "de banque",
        "INSERT INTO log_bank (account_id, character_id, name, action, item_tid, item_iid, count) VALUES (?,?,?,?,?,?,?)",
        (account_id, character.0, cut(character.1, 48), action, tid, iid, count).into(),
    );
}

/// Or déposé (montant positif) ou retiré (négatif) de la banque.
pub fn bank_money(db: &Databases, account_id: u32, character: (u32, &str), amount: i64) {
    let action = if amount > 0 { "dépôt" } else { "retrait" };
    write(
        &db.logs,
        "de banque",
        "INSERT INTO log_bank (account_id, character_id, name, action, money) VALUES (?,?,?,?,?)",
        (account_id, character.0, cut(character.1, 48), action, amount.abs()).into(),
    );
}

/// Page de banque achetée (`page` : 2 pour la deuxième), au prix `price`.
pub fn bank_page(db: &Databases, account_id: u32, character: (u32, &str), page: i64, price: i64) {
    write(
        &db.logs,
        "de banque",
        "INSERT INTO log_bank (account_id, character_id, name, action, count, money) VALUES (?,?,?,'page',?,?)",
        (account_id, character.0, cut(character.1, 48), page, price).into(),
    );
}

/// Opération de la maison des ventes (`world/auction.rs`).
pub struct AuctionLog<'a> {
    pub sale_id: u32,
    /// `vente`, `achat`, `annulation` ou `encaissement`.
    pub action: &'static str,
    /// Auteur : le vendeur, ou l'acheteur pour un achat (alors `other` est le vendeur).
    pub character: (u32, &'a str),
    pub other: Option<u32>,
    pub tid: u32,
    pub iid: Option<u32>,
    pub count: i64,
    /// Prix (or, CC) de la vente.
    pub price: (i64, i64),
    /// Frais payés à la mise en vente (or, CC).
    pub fee: (i64, i64),
}

pub fn auction(db: &Databases, e: &AuctionLog) {
    write(
        &db.logs,
        "de la maison des ventes",
        "INSERT INTO log_auction (sale_id, action, character_id, name, other_id, item_tid, item_iid, count, gold, cc, fee_gold, fee_cc) VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
        (e.sale_id, e.action, e.character.0, cut(e.character.1, 48), e.other, e.tid, e.iid, e.count, e.price.0, e.price.1, e.fee.0, e.fee.1).into(),
    );
}

/// Commande MJ exécutée (ou refusée pour ses arguments), avec sa réponse.
#[allow(clippy::too_many_arguments)]
pub fn gm(db: &Databases, account_id: u32, character: (u32, &str), gm_level: i64, command: &str, args: &str, ok: bool, reply: &str) {
    write(
        &db.logs,
        "MJ",
        "INSERT INTO log_gm (account_id, character_id, name, gm_level, command, args, ok, reply) VALUES (?,?,?,?,?,?,?,?)",
        (account_id, character.0, cut(character.1, 48), gm_level.clamp(0, 255), cut(command, 48), cut(args, 512), ok, cut(reply, 512)).into(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objets_en_json() {
        assert_eq!(items_json(&[(10, 190, 3)]), r#"[{"iid":10,"tid":190,"count":3}]"#);
        assert_eq!(items_json(&[]), "[]");
        assert_eq!(cut("éàü", 2), "éà");
    }
}
