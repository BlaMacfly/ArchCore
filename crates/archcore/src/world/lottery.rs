//! Boîtes de loterie (usage 17) et clés (usage 18).
//!
//! ```text
//!   Table : itemlotterybox.txt de l'épisode 8 (LotteryBoxTID, PotItemTID, MinStack, MaxStack, Rate ; 3 121 lignes, 123
//!     boîtes), gardée telle quelle dans `client_table`. Les objets d'usage 17 sans ligne restent sans effet.
//!   Boîte utilisée (AGPMITEM op 3) : un objet tiré, chance proportionnelle à « Rate », quantité entre MinStack et MaxStack
//!     (bornes comprises) ; rangé dans l'inventaire, ou l'inventaire Cash pour un objet Cash. Puis un exemplaire de la boîte
//!     (et de sa clé) consommé, message système 0 « You have acquired %d %s by opening %s. » (champ 2 quantité, 4 nom de
//!     la boîte, 5 nom de l'objet, ordre des paramètres de systemmsgformat.xml).
//!   Clé exigée (`Rules.LotteryKeys`, choix : aucun binaire ne montre la règle, seuls existent les messages) : « SubType » de
//!     la boîte = « SubType » de la clé : 1 or, 2 argent, 3 bronze, 4 platine ; Bronze Treasure Chest, sans « SubType » dans
//!     la table, demande le 3. Sans clé : message 2, 3, 23 ou 28 (« You need to have a Gold Key… »), rien de consommé.
//!   Clé utilisée seule : message 1 « Open the Inventory and right click the treasure chest. »
//!   Sac plein : AGPMITEM op 5, rien de consommé.
//! ```

use super::consumables::{consume_one, failed_packet};
use super::items::{item_add_of, item_status, InventoryItem};
use super::{send_system, WorldCtx, WorldState};
use crate::{debug, info};
use archcore_net::schema;
use archcore_net::{build_packet, Fields};
use rand::Rng;
use std::collections::HashMap;

/// Types d'usage (« Type » d'un objet d'« ItemType » 1).
pub const USABLE_BOX: i64 = 17;
pub const USABLE_KEY: i64 = 18;
/// Bronze Treasure Chest : sans « SubType » dans la table, mais la clé de bronze (« SubType » 3) lui est destinée.
const BRONZE_BOX: u32 = 4777;
const KEY_BRONZE: i64 = 3;

/// Codes du message système (AgpmSystemMessage, systemmsgformat.xml).
pub mod message {
    pub const OPENED: i64 = 0;
    pub const KEY_NOT_USABLE: i64 = 1;
    pub const NEEDS_GOLD_KEY: i64 = 2;
    pub const NEEDS_SILVER_KEY: i64 = 3;
    pub const NEEDS_BRONZE_KEY: i64 = 23;
    pub const NEEDS_PLATINUM_KEY: i64 = 28;
}

/// Une ligne de la table : objet, quantités, poids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pot {
    pub tid: u32,
    pub min: i64,
    pub max: i64,
    pub rate: u64,
}

/// Lit itemlotterybox.txt : lignes par boîte, dans l'ordre de la table.
pub fn parse(text: &str) -> HashMap<u32, Vec<Pot>> {
    let mut lines = text.lines();
    let Some(header) = lines.next() else { return HashMap::new() };
    let columns: Vec<&str> = header.trim_start_matches('\u{feff}').split('\t').map(str::trim).collect();
    let at = |name: &str| columns.iter().position(|c| *c == name);
    let (Some(b), Some(p), Some(lo), Some(hi), Some(r)) = (at("LotteryBoxTID"), at("PotItemTID"), at("MinStack"), at("MaxStack"), at("Rate")) else {
        return HashMap::new();
    };
    let mut out: HashMap<u32, Vec<Pot>> = HashMap::new();
    for line in lines {
        let c: Vec<&str> = line.split('\t').map(str::trim).collect();
        let num = |i: usize| c.get(i).and_then(|v| v.parse::<i64>().ok());
        let (Some(tid), Some(pot)) = (num(b).filter(|v| *v > 0), num(p).filter(|v| *v > 0)) else { continue };
        let min = num(lo).unwrap_or(1).max(1);
        out.entry(tid as u32).or_default().push(Pot { tid: pot as u32, min, max: num(hi).unwrap_or(min).max(min), rate: num(r).unwrap_or(0).max(0) as u64 });
    }
    out
}

/// Tire un objet selon les poids, et sa quantité ; `None` si aucun poids.
pub fn draw(rng: &mut impl Rng, pots: &[Pot]) -> Option<(u32, i64)> {
    let total: u64 = pots.iter().map(|p| p.rate).sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.gen_range(0..total);
    for p in pots {
        if roll < p.rate {
            return Some((p.tid, rng.gen_range(p.min..=p.max)));
        }
        roll -= p.rate;
    }
    None
}

/// Nature de la clé qu'une boîte exige, d'après son « SubType ».
pub fn key_of_box(tid: u32, sub_type: i64) -> Option<i64> {
    match sub_type {
        1..=4 => Some(sub_type),
        _ if tid == BRONZE_BOX => Some(KEY_BRONZE),
        _ => None,
    }
}

/// Message d'une clé manquante.
pub fn missing_key_message(kind: i64) -> i64 {
    match kind {
        1 => message::NEEDS_GOLD_KEY,
        2 => message::NEEDS_SILVER_KEY,
        KEY_BRONZE => message::NEEDS_BRONZE_KEY,
        _ => message::NEEDS_PLATINUM_KEY,
    }
}

/// AgpmSystemMessage op 0 : boîte `box_name` ouverte, `count` × `pot_name`.
pub fn opened_packet(count: i64, box_name: &str, pot_name: &str) -> Vec<u8> {
    let s = &schema::SYSTEM_MESSAGE;
    build_packet(s, &Fields::new(s).with(0, 0).with(1, message::OPENED).with(2, count).with(3, -1).with(4, box_name).with(5, pot_name))
}

/// AgpmSystemMessage op 0 sans paramètre.
pub fn code_packet(code: i64) -> Vec<u8> {
    let s = &schema::SYSTEM_MESSAGE;
    build_packet(s, &Fields::new(s).with(0, 0).with(1, code))
}

/// Clé utilisée seule : le client renvoie à la boîte.
pub fn use_key(st: &WorldState, sid: u32) {
    if let Some(p) = st.players.get(&sid) {
        p.send_raw(code_packet(message::KEY_NOT_USABLE));
    }
}

/// Boîte utilisée (AGPMITEM op 3).
pub async fn open(ctx: &WorldCtx, st: &mut WorldState, sid: u32, item: &InventoryItem) -> anyhow::Result<()> {
    let Some(p) = st.players.get_mut(&sid) else { return Ok(()) };
    let cid = p.ch.id;
    let Some(pots) = ctx.catalog.lottery.get(&item.tid) else {
        debug!("[objet] {} : objet {} sans ligne dans la table des boîtes", p.ch.name, item.tid);
        return Ok(());
    };
    // Clé de la même nature.
    let kind = ctx.catalog.lottery_sub_types.get(&item.tid).copied().unwrap_or(0);
    let mut key = None;
    if let Some(wanted) = key_of_box(item.tid, kind).filter(|_| crate::config::rules().lottery_keys) {
        let fits = |i: &&InventoryItem| ctx.catalog.get(i.tid).is_some_and(|d| d.usable_type == USABLE_KEY) && ctx.catalog.lottery_sub_types.get(&i.tid) == Some(&wanted);
        key = p.inventory.items.values().chain(p.inventory.cash.values()).find(fits).map(|i| i.iid);
        if key.is_none() {
            debug!("[objet] {} : boîte {} sans clé de nature {wanted}", p.ch.name, item.tid);
            p.send_raw(code_packet(missing_key_message(wanted)));
            return Ok(());
        }
    }
    let Some((pot, count)) = draw(&mut rand::thread_rng(), pots) else {
        p.send_raw(failed_packet(item.tid, cid));
        return Ok(());
    };
    // Rangé d'abord : sac plein, rien de consommé.
    let stack = ctx.catalog.get(pot).map(|d| d.stack).unwrap_or(1);
    let (added, status) = if ctx.catalog.is_cash(pot) {
        (p.inventory.add_cash(&ctx.db, pot, count, stack).await?, item_status::CASH_INVENTORY)
    } else {
        (p.inventory.add(&ctx.db, pot, count, stack, None).await?, item_status::INVENTORY)
    };
    let Some((it, created)) = added else {
        p.send_raw(failed_packet(item.tid, cid));
        send_system(st, sid, "Inventaire plein.");
        return Ok(());
    };
    if created {
        let level = ctx.catalog.get(pot).and_then(|d| d.equip.as_ref()).map(|e| e.level.max(1)).unwrap_or(1);
        let (options, sockets) = super::options::generate(&ctx.catalog, &ctx.options, pot, level, &mut rand::thread_rng());
        p.inventory.set_generated(&ctx.db, it.iid, &options, sockets).await?;
    }
    let it = p.inventory.get_usable(it.iid).cloned().unwrap_or(it);
    p.send_raw(item_add_of(&it, cid, status, Some(it.grid)));
    if let Some(c) = super::items::convert_add(&it, cid) {
        p.send_raw(c);
    }
    if let Some(k) = key {
        consume_one(ctx, p, k).await?;
    }
    consume_one(ctx, p, item.iid).await?;
    let name = |tid: u32| ctx.catalog.get(tid).map(|d| d.name.clone()).unwrap_or_default();
    p.send_raw(opened_packet(count, &name(item.tid), &name(pot)));
    info!("[objet] {} ouvre {} : {count} × {}", p.ch.name, name(item.tid), name(pot));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    const TABLE: &str = "LotteryBoxTID\tLotteryBoxName\tPotItemTID\tPotItemName\tMinStack\tMaxStack\tRate\r\n3021\tCopper\t50067\tHero\t1\t1\t10000\r\n3316\tBalloon\t6007\tLens\t5\t8\t3\r\n3316\tBalloon\t6008\tOther\t1\t1\t1\r\n\t\t\t\t\t\t\r\n";

    #[test]
    fn table_et_tirage() {
        let t = parse(TABLE);
        assert_eq!(t.len(), 2);
        assert_eq!(t[&3021], vec![Pot { tid: 50067, min: 1, max: 1, rate: 10000 }]);
        assert_eq!(t[&3316][0], Pot { tid: 6007, min: 5, max: 8, rate: 3 });
        let mut rng = rand::rngs::StdRng::seed_from_u64(3);
        let mut seen = [0; 2];
        for _ in 0..4000 {
            let (tid, n) = draw(&mut rng, &t[&3316]).unwrap();
            if tid == 6007 {
                seen[0] += 1;
                assert!((5..=8).contains(&n), "{n}");
            } else {
                seen[1] += 1;
                assert_eq!(n, 1);
            }
        }
        assert!(seen[0] > seen[1] * 2 && seen[1] > 600, "poids 3 contre 1 : {seen:?}");
        assert_eq!(draw(&mut rng, &[Pot { tid: 1, min: 1, max: 1, rate: 0 }]), None);
    }

    #[test]
    fn cles_et_messages() {
        assert_eq!(key_of_box(3616, 1), Some(1), "Gold Treasure Chest");
        assert_eq!(key_of_box(6053, 4), Some(4), "Platinum treasure chest");
        assert_eq!(key_of_box(BRONZE_BOX, 0), Some(3));
        assert_eq!(key_of_box(3316, 0), None, "sans clé");
        assert_eq!([1, 2, 3, 4].map(missing_key_message), [2, 3, 23, 28]);
        let p = archcore_net::decode_packet(&schema::SYSTEM_MESSAGE, &opened_packet(2, "Box", "Pot")).unwrap();
        assert_eq!((p.int(1), p.int(2), p.text(4).as_deref(), p.text(5).as_deref()), (Some(0), Some(2), Some("Box"), Some("Pot")));
    }
}
