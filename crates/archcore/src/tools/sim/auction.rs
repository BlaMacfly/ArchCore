//! `archcore sim auction` : maison des ventes (AgpmAuction 0x0B) chez Fotherbee (« Item Trader », fonction 28).
//!
//! Ventes des deux personnages effacées ; en base, GmTest part de 1 000 000 pièces d'or, SimA de 500 000 et 50 CC. Le
//! MJ ouvre la planche (op 9 → op 10), reçoit 5 potions (TID 190) et une Short Sword. Refus : épée sans catégorie,
//! prix sous 100 000, pile partielle (code 1). Il met les potions en vente pour 150 000 or et 20 CC : op 4 (code 0),
//! op 1 ADD_SALES, potions retirées, 4 500 d'or de frais. SimA entre, ouvre la planche, cherche le TID 190 (op 12 :
//! ligne avec docID, vendeur, prix, quantité), achète avec une mauvaise quantité (code 5) puis la pile (même IID dans
//! son inventaire, or et CC débités, op 7 code 0) ; le MJ reçoit op 3 (vendu) ; second achat : code 12. Le MJ
//! encaisse (op 6, or et CC crédités, op 2). Il remet 3 potions en vente, se reconnecte (op 1 à l'entrée), annule
//! (potions rendues, op 5, op 2 ; frais gardés).

use super::{enter_world, step, InWorld, Recv};
use archcore_net::schema::{self, auction_op, auction_result, chat_type, mini};
use archcore_net::{build_mini, decode_mini, Fields, Vec3};
use std::time::Duration;

const POTION: i64 = 190;
const SWORD: i64 = 11;
const TRADER: Vec3 = Vec3 { x: -471_700.0, y: 4_500.0, z: -61_850.0 };
const STOP: i64 = 0x04;

fn say(w: &InWorld, text: &str) -> Fields {
    let ch = &schema::CHATTING;
    Fields::new(ch).with(0, 0).with(1, w.cid).with(2, chat_type::NORMAL).with(6, crate::tools::to_cp1252(text))
}

async fn gm(w: &mut InWorld, text: &str, expect: &str) -> anyhow::Result<()> {
    w.world.send(&schema::CHATTING, say(w, text)).await?;
    let ch = &schema::CHATTING;
    w.world.expect_where(|q| q.ptype == ch.id && q.fields().text(6).is_some_and(|t| t.contains(expect)), 3000, text).await?;
    Ok(())
}

async fn give(w: &mut InWorld, tid: i64, n: i64) -> anyhow::Result<i64> {
    w.world.send(&schema::CHATTING, say(w, &format!("/additem {tid} {n}"))).await?;
    let it = &schema::ITEM;
    Ok(w.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(tid) && q.int(1) == Some(2), 3000, "objet ajouté").await?.int(2).unwrap_or(0))
}

fn auction(op: i64, w: &InWorld, sub: Vec<u8>) -> Fields {
    let s = &schema::AUCTION;
    Fields::new(s).with(0, op).with(1, w.cid).with(2, sub)
}

fn is_op(op: i64) -> impl Fn(&Recv) -> bool {
    move |q: &Recv| q.is(&schema::AUCTION, Some(op))
}

async fn expect_code(w: &mut InWorld, op: i64, code: i64, what: &str) -> anyhow::Result<Recv> {
    let r = w.world.expect_where(is_op(op), 3000, what).await?;
    anyhow::ensure!(r.int(3).unwrap_or(0) == code && r.int(1) == Some(w.cid as i64), "{what} : code {:?}, CID {:?}", r.int(3), r.int(1));
    println!("  ✓ {what} : op {op}, code {code}");
    Ok(r)
}

fn sell(iid: i64, count: i64, gold: i64, cc: i64) -> Vec<u8> {
    let m = &mini::AUCTION_SELL;
    build_mini(m, &Fields::new(m).with(0, iid).with(1, count).with(2, gold).with(3, cc).with(5, 0))
}

fn sale_id(id: i64) -> Vec<u8> {
    let m = &mini::AUCTION_CANCEL;
    build_mini(m, &Fields::new(m).with(0, id))
}

/// Va chez Fotherbee et ouvre la planche (op 9 → op 10).
async fn open(w: &mut InWorld) -> anyhow::Result<i64> {
    let mv = &schema::OPT_CHAR_MOVE;
    w.world.send(mv, Fields::new(mv).with(0, w.cid).with(1, TRADER).with(2, TRADER).with(5, STOP).with(6, 0)).await?;
    let mut trader = w.npcs.iter().find(|n| n.1.ends_with("Fotherbee")).map(|n| n.0);
    let ov = &schema::OPT_VIEW;
    while let Ok(p) = w.world.expect_where(|q| q.is(ov, Some(1)), 1500, "PNJ").await {
        if let Ok(view) = decode_mini(&mini::OPT_VIEW_CHAR, p.fields().bytes(1).unwrap_or_default()) {
            if view.text(2).is_some_and(|n| n.ends_with("Fotherbee")) {
                trader = view.int(0);
            }
        }
    }
    let trader = trader.ok_or_else(|| anyhow::anyhow!("Fotherbee introuvable"))?;
    let eb = &mini::EVENT_BASE;
    let event = build_mini(eb, &Fields::new(eb).with(0, 3).with(1, trader).with(3, 28));
    w.world.send(&schema::AUCTION, auction(auction_op::EVENT_REQUEST, w, event)).await?;
    let grant = w.world.expect_where(is_op(auction_op::EVENT_GRANT), 3000, "planche ouverte (op 10)").await?;
    let base = decode_mini(eb, grant.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(base.int(1) == Some(trader) && base.int(3) == Some(28), "EVENT_BASE {base:?}");
    println!("  ✓ planche ouverte chez Fotherbee ({trader:#x})");
    Ok(trader)
}

/// Recherche d'un TID depuis le début : (total, lignes [docID, vendeur, or, CC, TID, quantité]).
async fn search(w: &mut InWorld, tid: i64) -> anyhow::Result<(i64, Vec<[String; 6]>)> {
    let m = &mini::AUCTION_SELECT2;
    let q = build_mini(m, &Fields::new(m).with(0, tid).with(1, 9_999_999_999_999i64).with(2, 0).with(3, 0));
    w.world.send(&schema::AUCTION, auction(auction_op::SELECT2, w, q)).await?;
    let r = w.world.expect_where(is_op(auction_op::SELECT2), 3000, "résultats de recherche").await?;
    let a = decode_mini(m, r.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(a.int(0) == Some(tid), "TID de la réponse {:?}", a.int(0));
    let rs = decode_mini(&mini::AUCTION_ROWSET, a.bytes(4).unwrap_or_default())?;
    let v = |row: usize, c: usize| crate::world::auction::rowset_value(&rs, row, c).unwrap_or_default();
    let rows = (0..rs.int(3).unwrap_or(0) as usize).map(|i| [v(i, 0), v(i, 1), v(i, 3), v(i, 4), v(i, 6), v(i, 7)]).collect();
    Ok((a.int(3).unwrap_or(0), rows))
}

async fn money(w: &mut InWorld, field: usize, what: &str) -> anyhow::Result<i64> {
    let (c, me) = (&schema::CHARACTER, w.cid as i64);
    Ok(w.world.expect_where(|q| q.is(c, Some(1)) && q.int(1) == Some(me) && q.int(field).is_some(), 3000, what).await?.int(field).unwrap_or(-1))
}

pub async fn run(ua: &str, pa: &str, ca: &str, ub: &str, pb: &str, cb: &str) -> anyhow::Result<()> {
    let cfg = crate::config::load()?;
    let it = &schema::ITEM;
    let db = crate::db::Databases::new(&cfg.database);
    let (ida, idb) = (super::character_id(&db, ca).await?, super::character_id(&db, cb).await?);
    let wipe = || async {
        db.characters.exec("DELETE FROM auction_sale WHERE seller_id IN (?, ?) OR buyer_id IN (?, ?)", (ida, idb, ida, idb)).await?;
        db.characters.exec("DELETE FROM character_items WHERE character_id IN (?, ?) AND status = 5", (ida, idb)).await
    };
    wipe().await?;
    db.characters.exec("UPDATE characters SET money = 1000000, cash = 0 WHERE id = ?", (ida,)).await?;
    db.characters.exec("UPDATE characters SET money = 500000, cash = 50 WHERE id = ?", (idb,)).await?;

    step(&format!("{ca} ouvre la maison des ventes et reçoit potions et épée"));
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    anyhow::ensure!(a.world.expect_where(is_op(auction_op::ADD_SALES), 800, "ventes").await.is_err(), "boîte de ventes de {ca} non vide");
    open(&mut a).await?;
    let potions = give(&mut a, POTION, 5).await?;
    let sword = give(&mut a, SWORD, 1).await?;

    step("mises en vente refusées");
    a.world.send(&schema::AUCTION, auction(auction_op::SELL, &a, sell(sword, 1, 200_000, 0))).await?;
    expect_code(&mut a, auction_op::SELL, auction_result::FAILED, "épée sans catégorie").await?;
    a.world.send(&schema::AUCTION, auction(auction_op::SELL, &a, sell(potions, 5, 50_000, 0))).await?;
    expect_code(&mut a, auction_op::SELL, auction_result::FAILED, "moins de 100 000 pièces d'or").await?;
    a.world.send(&schema::AUCTION, auction(auction_op::SELL, &a, sell(potions, 2, 150_000, 0))).await?;
    expect_code(&mut a, auction_op::SELL, auction_result::FAILED, "pile partielle").await?;

    step(&format!("{ca} met les 5 potions en vente pour 150 000 or et 20 CC"));
    a.world.send(&schema::AUCTION, auction(auction_op::SELL, &a, sell(potions, 5, 150_000, 20))).await?;
    let r = expect_code(&mut a, auction_op::SELL, auction_result::OK, "mise en vente").await?;
    let echo = decode_mini(&mini::AUCTION_SELL, r.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(echo.int(0) == Some(potions) && echo.int(1) == Some(5) && echo.int(2) == Some(150_000) && echo.int(3) == Some(20), "SELL {echo:?}");
    let added = a.world.expect_where(is_op(auction_op::ADD_SALES), 3000, "ADD_SALES").await?;
    let s = decode_mini(&mini::AUCTION_SALES, added.fields().bytes(2).unwrap_or_default())?;
    let id = s.int(0).unwrap_or(0);
    anyhow::ensure!(id > 0 && s.int(3) == Some(potions) && s.int(9) == Some(POTION) && s.int(6) == Some(5) && s.int(7) == Some(0), "vente {s:?}");
    a.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(potions), 3000, "potions retirées").await?;
    let left = money(&mut a, 8, "frais payés").await?;
    anyhow::ensure!(left == 1_000_000 - 4_500, "or après les frais : {left}");
    println!("  ✓ vente {id} (statut 0), potions retirées, 4 500 d'or de frais (reste {left})");
    let (total, rows) = search(&mut a, POTION).await?;
    anyhow::ensure!(total >= 1 && rows.iter().any(|r| r[0] == id.to_string()), "{ca} ne voit pas sa vente : {total}, {rows:?}");

    step(&format!("{cb} cherche les potions et les achète"));
    let mut b = enter_world(&cfg, ub, pb, cb, "B-").await?;
    open(&mut b).await?;
    let (total, rows) = search(&mut b, POTION).await?;
    let row = rows.iter().find(|r| r[0] == id.to_string()).ok_or_else(|| anyhow::anyhow!("vente {id} absente : {rows:?}"))?;
    anyhow::ensure!(row[1] == ca && row[2] == "150000" && row[3] == "20" && row[4] == POTION.to_string() && row[5] == "5", "ligne {row:?}");
    println!("  ✓ {total} vente(s) du TID {POTION} ; ligne {row:?}");
    let buy = |count: i64| {
        let m = &mini::AUCTION_BUY;
        build_mini(m, &Fields::new(m).with(0, id).with(1, count).with(2, POTION).with(3, 150_000).with(4, 0))
    };
    b.world.send(&schema::AUCTION, auction(auction_op::BUY, &b, buy(4))).await?;
    expect_code(&mut b, auction_op::BUY, auction_result::QUANTITY, "mauvaise quantité").await?;
    b.world.send(&schema::AUCTION, auction(auction_op::BUY, &b, buy(5))).await?;
    let got = b.world.expect_where(|q| q.is(it, Some(0)) && q.int(3) == Some(POTION), 3000, "potions achetées").await?;
    anyhow::ensure!(got.int(2) == Some(potions) && got.int(5) == Some(5) && got.int(4) == Some(b.cid as i64) && got.int(26) == Some(potions), "objet {:?} × {:?}", got.int(2), got.int(5));
    let gold = money(&mut b, 8, "or débité").await?;
    let cc = money(&mut b, 10, "CC débités").await?;
    anyhow::ensure!(gold == 350_000 && cc == 30, "après l'achat : {gold} or, {cc} CC");
    let r = expect_code(&mut b, auction_op::BUY, auction_result::OK, "achat").await?;
    let e = decode_mini(&mini::AUCTION_BUY, r.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(e.int(3) == Some(150_000) && e.int(4) == Some(20) && e.int(1) == Some(5), "réponse {e:?}");
    println!("  ✓ même objet (IID {potions}) chez {cb}, reste {gold} or et {cc} CC");
    let sold = a.world.expect_where(is_op(auction_op::UPDATE_SALES), 3000, "vente conclue (op 3)").await?;
    let u = decode_mini(&mini::AUCTION_SALES, sold.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(u.int(0) == Some(id) && u.int(7) == Some(1), "UPDATE_SALES {u:?}");
    println!("  ✓ {ca} voit sa vente conclue (statut 1)");
    b.world.send(&schema::AUCTION, auction(auction_op::BUY, &b, buy(5))).await?;
    expect_code(&mut b, auction_op::BUY, auction_result::NOT_AVAILABLE, "second achat").await?;
    let (_, rows) = search(&mut b, POTION).await?;
    anyhow::ensure!(!rows.iter().any(|r| r[0] == id.to_string()), "vente {id} toujours proposée");

    step(&format!("{ca} encaisse"));
    a.world.send(&schema::AUCTION, auction(auction_op::CONFIRM, &a, sale_id(id))).await?;
    let gold = money(&mut a, 8, "or crédité").await?;
    let cc = money(&mut a, 10, "CC crédités").await?;
    anyhow::ensure!(gold == 995_500 + 150_000 && cc == 20, "après l'encaissement : {gold} or, {cc} CC");
    let r = expect_code(&mut a, auction_op::CONFIRM, auction_result::OK, "encaissement").await?;
    let c = decode_mini(&mini::AUCTION_CONFIRM, r.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(c.int(0) == Some(id) && c.int(1) == Some(POTION) && c.int(2) == Some(5) && c.int(3) == Some(150_000) && c.int(4) == Some(20), "CONFIRM {c:?}");
    let rm = a.world.expect_where(is_op(auction_op::REMOVE_SALES), 3000, "REMOVE_SALES").await?;
    anyhow::ensure!(decode_mini(&mini::AUCTION_SALES, rm.fields().bytes(2).unwrap_or_default())?.int(0) == Some(id), "REMOVE_SALES");
    a.world.send(&schema::AUCTION, auction(auction_op::CONFIRM, &a, sale_id(id))).await?;
    expect_code(&mut a, auction_op::CONFIRM, auction_result::FAILED, "second encaissement").await?;
    println!("  ✓ {gold} or et {cc} CC, vente retirée de la boîte");

    step(&format!("{ca} remet des potions en vente, se reconnecte, puis annule"));
    let more = give(&mut a, POTION, 3).await?;
    a.world.send(&schema::AUCTION, auction(auction_op::SELL, &a, sell(more, 3, 100_000, 0))).await?;
    expect_code(&mut a, auction_op::SELL, auction_result::OK, "mise en vente").await?;
    let added = a.world.expect_where(is_op(auction_op::ADD_SALES), 3000, "ADD_SALES").await?;
    let id2 = decode_mini(&mini::AUCTION_SALES, added.fields().bytes(2).unwrap_or_default())?.int(0).unwrap_or(0);
    drop(a);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let mut a = enter_world(&cfg, ua, pa, ca, "A-").await?;
    let at_entry = a.world.expect_where(is_op(auction_op::ADD_SALES), 3000, "vente à l'entrée (op 1)").await?;
    let s = decode_mini(&mini::AUCTION_SALES, at_entry.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(s.int(0) == Some(id2) && s.int(3) == Some(more) && s.int(4) == Some(100_000) && s.int(6) == Some(3) && s.int(7) == Some(0), "vente à l'entrée {s:?}");
    println!("  ✓ vente {id2} reçue à l'entrée");
    a.world.send(&schema::AUCTION, auction(auction_op::CANCEL, &a, sale_id(id2))).await?;
    let back = a.world.expect_where(|q| q.is(it, Some(0)) && q.int(2) == Some(more), 3000, "potions rendues").await?;
    anyhow::ensure!(back.int(5) == Some(3) && back.int(1) == Some(2), "objet rendu × {:?}", back.int(5));
    let r = expect_code(&mut a, auction_op::CANCEL, auction_result::OK, "annulation").await?;
    let c = decode_mini(&mini::AUCTION_CANCEL, r.fields().bytes(2).unwrap_or_default())?;
    anyhow::ensure!(c.int(0) == Some(id2) && c.int(1) == Some(POTION) && c.int(2) == Some(3), "CANCEL {c:?}");
    a.world.expect_where(is_op(auction_op::REMOVE_SALES), 3000, "REMOVE_SALES").await?;
    let n: u64 = db.characters.first("SELECT COUNT(*) AS n FROM auction_sale WHERE seller_id = ?", (ida,)).await?.map(|r| crate::db::col(&r, "n")).unwrap_or(9);
    anyhow::ensure!(n == 0, "{n} vente(s) restée(s) en base");
    println!("  ✓ potions rendues, plus aucune vente en base");

    // Remise en état : objets détruits, or de départ.
    for (w, iid) in [(&mut a, more), (&mut b, potions)] {
        w.world.send(it, Fields::new(it).with(0, 21).with(2, iid)).await?;
        w.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(iid), 3000, "objet détruit").await?;
    }
    a.world.send(it, Fields::new(it).with(0, 21).with(2, sword)).await?;
    a.world.expect_where(|q| q.is(it, Some(1)) && q.int(2) == Some(sword), 3000, "objet détruit").await?;
    gm(&mut a, "/money 10000", "pièce(s) d'or").await?;
    drop((a, b));
    tokio::time::sleep(Duration::from_millis(500)).await;
    db.characters.exec("UPDATE characters SET money = 10000, cash = 0 WHERE id IN (?, ?)", (ida, idb)).await?;
    wipe().await?;
    println!("\nmaison des ventes : OK");
    Ok(())
}
